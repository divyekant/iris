# V11-S1: Inbox Snapshot Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Pre-computed inbox statistics so AI chat can instantly answer aggregate questions ("How many unread emails?", "Who sends me the most email?").

**Architecture:** New `inbox_stats` table materialized after each sync batch. Direct SQL aggregation (not job queue — fast enough synchronous). GET endpoint for UI. Stats injected into chat system prompt.

**Tech Stack:** Rust (rusqlite, Axum, serde, chrono), SQLite

---

### Task 1: Create migration file

**Files:**
- Create: `migrations/007_inbox_stats.sql`

**Step 1: Write the migration SQL**

```sql
-- migrations/007_inbox_stats.sql
-- Pre-computed inbox statistics, refreshed after each sync batch.

CREATE TABLE IF NOT EXISTS inbox_stats (
    account_id TEXT NOT NULL PRIMARY KEY,
    total INTEGER NOT NULL DEFAULT 0,
    unread INTEGER NOT NULL DEFAULT 0,
    starred INTEGER NOT NULL DEFAULT 0,
    by_category TEXT NOT NULL DEFAULT '{}',
    top_senders TEXT NOT NULL DEFAULT '[]',
    today_count INTEGER NOT NULL DEFAULT 0,
    week_count INTEGER NOT NULL DEFAULT 0,
    month_count INTEGER NOT NULL DEFAULT 0,
    last_updated INTEGER NOT NULL DEFAULT (unixepoch())
);

INSERT OR IGNORE INTO schema_version (version) VALUES (7);
```

**Step 2: Register migration in runner**

Modify: `src/db/migrations.rs`

Add after line 8:
```rust
const MIGRATION_007: &str = include_str!("../../migrations/007_inbox_stats.sql");
```

Add after line 55 (inside `run()`, after the `if current_version < 6` block):
```rust
    if current_version < 7 {
        conn.execute_batch(MIGRATION_007)?;
        tracing::info!("Applied migration 007_inbox_stats");
    }
```

**Step 3: Run tests to verify migration applies**

Run: `~/.cargo/bin/cargo test -p iris test_create_pool_and_run_migrations -- --nocapture`
Expected: PASS (existing test validates all migrations run without error)

**Step 4: Commit**

```bash
git add migrations/007_inbox_stats.sql src/db/migrations.rs
git commit -m "feat(v11-s1): inbox_stats table migration"
```

---

### Task 2: Write compute_inbox_stats with test

**Files:**
- Create: `src/api/inbox_stats.rs`
- Modify: `src/api/mod.rs` (add module)

**Step 1: Write the failing test**

Create `src/api/inbox_stats.rs`:

```rust
use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::Serialize;
use std::sync::Arc;

use crate::AppState;

// ---------------------------------------------------------------------------
// Inbox stats model
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Clone)]
pub struct InboxStats {
    pub account_id: String,
    pub total: i64,
    pub unread: i64,
    pub starred: i64,
    pub by_category: serde_json::Value,
    pub top_senders: serde_json::Value,
    pub today_count: i64,
    pub week_count: i64,
    pub month_count: i64,
    pub last_updated: i64,
}

// ---------------------------------------------------------------------------
// Compute and upsert stats for one account
// ---------------------------------------------------------------------------

pub fn compute_and_store(conn: &rusqlite::Connection, account_id: &str) -> Result<(), rusqlite::Error> {
    let now = chrono::Utc::now().timestamp();
    let today_start = {
        let today = chrono::Local::now().date_naive();
        today.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp()
    };
    let week_start = today_start - 7 * 86400;
    let month_start = today_start - 30 * 86400;

    // Core counts
    let (total, unread, starred): (i64, i64, i64) = conn.query_row(
        "SELECT COUNT(*) as total,
                SUM(CASE WHEN is_read = 0 THEN 1 ELSE 0 END) as unread,
                SUM(CASE WHEN is_starred = 1 THEN 1 ELSE 0 END) as starred
         FROM messages WHERE account_id = ?1 AND is_draft = 0",
        rusqlite::params![account_id],
        |row| Ok((row.get(0)?, row.get::<_, Option<i64>>(1)?.unwrap_or(0), row.get::<_, Option<i64>>(2)?.unwrap_or(0))),
    )?;

    // Date-range counts
    let today_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM messages WHERE account_id = ?1 AND is_draft = 0 AND date >= ?2",
        rusqlite::params![account_id, today_start],
        |row| row.get(0),
    )?;
    let week_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM messages WHERE account_id = ?1 AND is_draft = 0 AND date >= ?2",
        rusqlite::params![account_id, week_start],
        |row| row.get(0),
    )?;
    let month_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM messages WHERE account_id = ?1 AND is_draft = 0 AND date >= ?2",
        rusqlite::params![account_id, month_start],
        |row| row.get(0),
    )?;

    // Category breakdown
    let mut cat_stmt = conn.prepare(
        "SELECT COALESCE(ai_category, 'uncategorized'), COUNT(*)
         FROM messages WHERE account_id = ?1 AND is_draft = 0
         GROUP BY COALESCE(ai_category, 'uncategorized')",
    )?;
    let cats: Vec<(String, i64)> = cat_stmt
        .query_map(rusqlite::params![account_id], |row| Ok((row.get(0)?, row.get(1)?)))
        .map_err(|e| e)?
        .filter_map(|r| r.ok())
        .collect();
    let by_category = serde_json::json!(cats.into_iter().collect::<std::collections::HashMap<_, _>>());

    // Top 10 senders
    let mut sender_stmt = conn.prepare(
        "SELECT COALESCE(from_name, from_address), from_address, COUNT(*) as cnt
         FROM messages WHERE account_id = ?1 AND is_draft = 0 AND from_address IS NOT NULL
         GROUP BY from_address
         ORDER BY cnt DESC
         LIMIT 10",
    )?;
    let senders: Vec<serde_json::Value> = sender_stmt
        .query_map(rusqlite::params![account_id], |row| {
            Ok(serde_json::json!({
                "name": row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                "address": row.get::<_, String>(1)?,
                "count": row.get::<_, i64>(2)?,
            }))
        })?
        .filter_map(|r| r.ok())
        .collect();
    let top_senders = serde_json::Value::Array(senders);

    conn.execute(
        "INSERT INTO inbox_stats (account_id, total, unread, starred, by_category, top_senders, today_count, week_count, month_count, last_updated)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
         ON CONFLICT(account_id) DO UPDATE SET
            total = excluded.total,
            unread = excluded.unread,
            starred = excluded.starred,
            by_category = excluded.by_category,
            top_senders = excluded.top_senders,
            today_count = excluded.today_count,
            week_count = excluded.week_count,
            month_count = excluded.month_count,
            last_updated = excluded.last_updated",
        rusqlite::params![
            account_id,
            total,
            unread,
            starred,
            by_category.to_string(),
            top_senders.to_string(),
            today_count,
            week_count,
            month_count,
            now,
        ],
    )?;

    tracing::debug!(account_id, total, unread, "Inbox stats updated");
    Ok(())
}

// ---------------------------------------------------------------------------
// Read stats for all accounts
// ---------------------------------------------------------------------------

pub fn get_all_stats(conn: &rusqlite::Connection) -> Vec<InboxStats> {
    let mut stmt = match conn.prepare(
        "SELECT account_id, total, unread, starred, by_category, top_senders,
                today_count, week_count, month_count, last_updated
         FROM inbox_stats",
    ) {
        Ok(s) => s,
        Err(_) => return vec![],
    };

    stmt.query_map([], |row| {
        Ok(InboxStats {
            account_id: row.get(0)?,
            total: row.get(1)?,
            unread: row.get(2)?,
            starred: row.get(3)?,
            by_category: serde_json::from_str(&row.get::<_, String>(4)?).unwrap_or(serde_json::json!({})),
            top_senders: serde_json::from_str(&row.get::<_, String>(5)?).unwrap_or(serde_json::json!([])),
            today_count: row.get(6)?,
            week_count: row.get(7)?,
            month_count: row.get(8)?,
            last_updated: row.get(9)?,
        })
    })
    .map(|rows| rows.filter_map(|r| r.ok()).collect())
    .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// GET /api/ai/inbox-stats
// ---------------------------------------------------------------------------

pub async fn get_inbox_stats(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<InboxStats>>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(get_all_stats(&conn)))
}

// ---------------------------------------------------------------------------
// Format stats for system prompt injection
// ---------------------------------------------------------------------------

pub fn format_stats_for_prompt(conn: &rusqlite::Connection) -> String {
    let stats = get_all_stats(conn);
    if stats.is_empty() {
        return String::new();
    }

    let mut lines = vec!["\n=== Inbox Overview ===".to_string()];
    for s in &stats {
        lines.push(format!(
            "Account: {} — {} total emails, {} unread, {} starred",
            s.account_id, s.total, s.unread, s.starred
        ));
        lines.push(format!(
            "  Today: {}, This week: {}, This month: {}",
            s.today_count, s.week_count, s.month_count
        ));

        // Top senders
        if let Some(arr) = s.top_senders.as_array() {
            if !arr.is_empty() {
                let top: Vec<String> = arr
                    .iter()
                    .take(5)
                    .filter_map(|s| {
                        let name = s.get("name")?.as_str()?;
                        let count = s.get("count")?.as_i64()?;
                        Some(format!("{} ({})", name, count))
                    })
                    .collect();
                lines.push(format!("  Top senders: {}", top.join(", ")));
            }
        }

        // Category breakdown
        if let Some(obj) = s.by_category.as_object() {
            if !obj.is_empty() {
                let cats: Vec<String> = obj
                    .iter()
                    .map(|(k, v)| format!("{}: {}", k, v))
                    .collect();
                lines.push(format!("  Categories: {}", cats.join(", ")));
            }
        }
    }
    lines.join("\n")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::create_test_pool;

    fn insert_test_messages(conn: &rusqlite::Connection, account_id: &str) {
        // Insert a test account
        conn.execute(
            "INSERT INTO accounts (id, provider, email) VALUES (?1, 'gmail', 'test@test.com')",
            rusqlite::params![account_id],
        ).unwrap();

        let now = chrono::Utc::now().timestamp();
        let yesterday = now - 86400;
        let last_week = now - 5 * 86400;
        let last_month = now - 20 * 86400;

        // 5 messages: 2 unread, 1 starred, spread across dates
        let messages = vec![
            ("m1", account_id, "alice@test.com", "Alice", "Meeting", now, 0, 0, "primary"),
            ("m2", account_id, "bob@test.com", "Bob", "Invoice", now, 0, 1, "updates"),
            ("m3", account_id, "alice@test.com", "Alice", "Follow up", yesterday, 1, 0, "primary"),
            ("m4", account_id, "carol@test.com", "Carol", "Newsletter", last_week, 1, 0, "promotions"),
            ("m5", account_id, "dave@test.com", "Dave", "Old email", last_month, 1, 0, "primary"),
        ];

        for (id, aid, addr, name, subj, date, is_read, is_starred, cat) in messages {
            conn.execute(
                "INSERT INTO messages (id, account_id, from_address, from_name, subject, date, is_read, is_starred, is_draft, ai_category)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0, ?9)",
                rusqlite::params![id, aid, addr, name, subj, date, is_read, is_starred, cat],
            ).unwrap();
        }
    }

    #[test]
    fn test_compute_and_store() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let account_id = "acct-1";
        insert_test_messages(&conn, account_id);

        compute_and_store(&conn, account_id).unwrap();

        let stats = get_all_stats(&conn);
        assert_eq!(stats.len(), 1);
        let s = &stats[0];
        assert_eq!(s.account_id, account_id);
        assert_eq!(s.total, 5);
        assert_eq!(s.unread, 2); // m1, m2 have is_read = 0
        assert_eq!(s.starred, 1); // m2 has is_starred = 1
        assert!(s.today_count >= 2); // m1, m2 are from now (today)
        assert!(s.week_count >= 3); // m1, m2, m3 within 7 days
        assert_eq!(s.month_count, 5); // all within 30 days
    }

    #[test]
    fn test_compute_updates_existing() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let account_id = "acct-2";
        insert_test_messages(&conn, account_id);

        compute_and_store(&conn, account_id).unwrap();
        let stats1 = get_all_stats(&conn);
        assert_eq!(stats1[0].total, 5);

        // Mark one as read — recompute
        conn.execute("UPDATE messages SET is_read = 1 WHERE id = 'm1' AND account_id = ?1", rusqlite::params![account_id]).unwrap();
        compute_and_store(&conn, account_id).unwrap();

        let stats2 = get_all_stats(&conn);
        assert_eq!(stats2[0].unread, 1); // was 2, now 1
    }

    #[test]
    fn test_format_stats_for_prompt() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let account_id = "acct-3";
        insert_test_messages(&conn, account_id);
        compute_and_store(&conn, account_id).unwrap();

        let prompt = format_stats_for_prompt(&conn);
        assert!(prompt.contains("Inbox Overview"));
        assert!(prompt.contains("5 total emails"));
        assert!(prompt.contains("2 unread"));
        assert!(prompt.contains("Top senders"));
        assert!(prompt.contains("Alice"));
    }

    #[test]
    fn test_empty_stats() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let prompt = format_stats_for_prompt(&conn);
        assert!(prompt.is_empty());
    }
}
```

**Step 2: Add module to mod.rs**

Add to `src/api/mod.rs` between `health` and `messages`:
```rust
pub mod inbox_stats;
```

**Step 3: Run tests**

Run: `~/.cargo/bin/cargo test -p iris inbox_stats -- --nocapture`
Expected: 4 tests PASS

**Step 4: Commit**

```bash
git add src/api/inbox_stats.rs src/api/mod.rs
git commit -m "feat(v11-s1): compute_and_store + get_all_stats + format_stats_for_prompt"
```

---

### Task 3: Wire the API endpoint

**Files:**
- Modify: `src/lib.rs:64` (add route)

**Step 1: Add route**

Add after line 65 (after `/ai/reprocess` route):
```rust
        .route("/ai/inbox-stats", get(api::inbox_stats::get_inbox_stats))
```

**Step 2: Run existing integration tests to verify no breakage**

Run: `~/.cargo/bin/cargo test -p iris -- --nocapture`
Expected: All tests PASS (including new ones from Task 2)

**Step 3: Commit**

```bash
git add src/lib.rs
git commit -m "feat(v11-s1): GET /api/ai/inbox-stats endpoint"
```

---

### Task 4: Wire sync trigger

**Files:**
- Modify: `src/imap/sync.rs:159` (add stats compute after sync completes)

**Step 1: Add compute call after sync completes**

At `src/imap/sync.rs`, between lines 158-159 (after `Account::update_sync_status` and before `self.ws_hub.broadcast`), add:

```rust
            // Refresh inbox stats after sync
            if let Err(e) = crate::api::inbox_stats::compute_and_store(&conn, account_id) {
                tracing::warn!(account_id, error = %e, "Failed to update inbox stats");
            }
```

Note: The `conn` is already available from the block above (line 157). The `compute_and_store` call needs to be inside the same `{ let conn = ... }` block. Move the broadcast outside:

Replace lines 155-162:
```rust
        // Done
        {
            let conn = self.db.get()?;
            Account::update_sync_status(&conn, account_id, "idle", None);
        }
        self.ws_hub.broadcast(WsEvent::SyncComplete {
            account_id: account_id.to_string(),
        });
```

With:
```rust
        // Done
        {
            let conn = self.db.get()?;
            Account::update_sync_status(&conn, account_id, "idle", None);
            // Refresh inbox stats after sync
            if let Err(e) = crate::api::inbox_stats::compute_and_store(&conn, account_id) {
                tracing::warn!(account_id, error = %e, "Failed to update inbox stats");
            }
        }
        self.ws_hub.broadcast(WsEvent::SyncComplete {
            account_id: account_id.to_string(),
        });
```

**Step 2: Run all tests**

Run: `~/.cargo/bin/cargo test -p iris -- --nocapture`
Expected: All PASS

**Step 3: Commit**

```bash
git add src/imap/sync.rs
git commit -m "feat(v11-s1): trigger inbox stats refresh after sync"
```

---

### Task 5: Inject stats into chat system prompt

**Files:**
- Modify: `src/api/chat.rs:70-88` (system prompt) and the chat handler (~line 92-175)

**Step 1: Write the failing test**

Add to the test module in `src/api/chat.rs`:

```rust
    #[test]
    fn test_system_prompt_with_stats() {
        let pool = crate::db::create_test_pool();
        let conn = pool.get().unwrap();

        // Insert test account and messages
        conn.execute(
            "INSERT INTO accounts (id, provider, email) VALUES ('a1', 'gmail', 'test@test.com')",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO messages (id, account_id, from_address, subject, date, is_read, is_starred, is_draft)
             VALUES ('m1', 'a1', 'x@y.com', 'Test', strftime('%s','now'), 0, 0, 0)",
            [],
        ).unwrap();

        crate::api::inbox_stats::compute_and_store(&conn, "a1").unwrap();

        let prompt = chat_system_prompt_with_stats(&conn);
        assert!(prompt.contains("Inbox Overview"));
        assert!(prompt.contains("1 total emails"));
        assert!(prompt.contains("1 unread"));
    }
```

**Step 2: Run test to verify it fails**

Run: `~/.cargo/bin/cargo test -p iris test_system_prompt_with_stats -- --nocapture`
Expected: FAIL — `chat_system_prompt_with_stats` doesn't exist yet

**Step 3: Implement — add stats-aware system prompt**

In `src/api/chat.rs`, replace the `chat_system_prompt()` function (lines 70-88) with:

```rust
fn chat_system_prompt() -> String {
    chat_system_prompt_with_stats_str("")
}

fn chat_system_prompt_with_stats(conn: &rusqlite::Connection) -> String {
    let stats_block = crate::api::inbox_stats::format_stats_for_prompt(conn);
    chat_system_prompt_with_stats_str(&stats_block)
}

fn chat_system_prompt_with_stats_str(stats_block: &str) -> String {
    let today = chrono::Local::now().format("%A, %B %-d, %Y").to_string();
    format!(
        r#"You are Iris, an AI email assistant. You help users understand and manage their inbox through natural conversation.

Today's date is {today}.
{stats_block}
You have access to the user's recent emails provided as context below. Each email shows its date, read/unread status, sender, subject, and a snippet. When answering:
- Reference specific emails by their [ID] markers when citing information
- Be concise and helpful
- Use the date and read/unread status to answer questions about "today's emails", "unread emails", "this week", etc.
- Use the Inbox Overview stats above to answer aggregate questions like "how many emails", "who sends me the most", etc.
- If the user asks to perform an action (archive, delete, mark read, etc.), describe what you'd do and include ACTION_PROPOSAL at the end of your response in this exact format:
  ACTION_PROPOSAL:{{"action":"archive","description":"Archive 3 emails from LinkedIn","message_ids":["id1","id2","id3"]}}
- Valid actions: archive, delete, mark_read, mark_unread, star
- If you don't have enough context to answer, say so honestly
- For briefing requests, summarize the most important unread emails first
- Do not make up information not present in the provided emails"#
    )
}
```

**Step 4: Update the chat handler to use stats-aware prompt**

In the chat handler (around line 174 where `generate` is called), change:
```rust
        .generate(&prompt, Some(&chat_system_prompt()))
```
to:
```rust
        .generate(&prompt, Some(&chat_system_prompt_with_stats(&conn)))
```

The `conn` is already available in the handler (from line ~98).

**Step 5: Run test to verify it passes**

Run: `~/.cargo/bin/cargo test -p iris test_system_prompt_with_stats -- --nocapture`
Expected: PASS

**Step 6: Run all tests**

Run: `~/.cargo/bin/cargo test -p iris -- --nocapture`
Expected: All PASS

**Step 7: Commit**

```bash
git add src/api/chat.rs
git commit -m "feat(v11-s1): inject inbox stats into chat system prompt"
```

---

### Task 6: Verify full build

**Step 1: Run full test suite**

Run: `~/.cargo/bin/cargo test -p iris -- --nocapture`
Expected: All tests PASS (previous 113 + 5 new = ~118)

**Step 2: Build check**

Run: `~/.cargo/bin/cargo build -p iris`
Expected: Clean build, no warnings

**Step 3: Final commit if any remaining changes**

```bash
git status
# If clean, no action needed
```

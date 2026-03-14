use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use rusqlite::{params, Connection};
use serde::Serialize;
use std::sync::Arc;

use crate::AppState;

#[derive(Debug, Serialize)]
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

/// Compute inbox statistics for the given account and upsert into inbox_stats.
pub fn compute_and_store(conn: &Connection, account_id: &str) -> Result<(), rusqlite::Error> {
    let now = chrono::Utc::now().timestamp();
    let today_midnight = {
        let local_now = chrono::Local::now();
        local_now
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_local_timezone(local_now.timezone())
            .unwrap()
            .timestamp()
    };
    let week_ago = today_midnight - 7 * 86400;
    let month_ago = today_midnight - 30 * 86400;

    // Consolidated counts — single table scan instead of 6 separate queries
    let (total, unread, starred, today_count, week_count, month_count): (i64, i64, i64, i64, i64, i64) = conn.query_row(
        "SELECT \
            COUNT(*), \
            SUM(CASE WHEN is_read = 0 THEN 1 ELSE 0 END), \
            SUM(CASE WHEN is_starred = 1 THEN 1 ELSE 0 END), \
            SUM(CASE WHEN date >= ?2 THEN 1 ELSE 0 END), \
            SUM(CASE WHEN date >= ?3 THEN 1 ELSE 0 END), \
            SUM(CASE WHEN date >= ?4 THEN 1 ELSE 0 END) \
         FROM messages WHERE account_id = ?1 AND is_draft = 0 AND is_deleted = 0",
        params![account_id, today_midnight, week_ago, month_ago],
        |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, Option<i64>>(1)?.unwrap_or(0),
                row.get::<_, Option<i64>>(2)?.unwrap_or(0),
                row.get::<_, Option<i64>>(3)?.unwrap_or(0),
                row.get::<_, Option<i64>>(4)?.unwrap_or(0),
                row.get::<_, Option<i64>>(5)?.unwrap_or(0),
            ))
        },
    )?;

    // by_category: GROUP BY category
    let mut cat_stmt = conn.prepare(
        "SELECT COALESCE(ai_category, 'uncategorized') AS cat, COUNT(*) \
         FROM messages WHERE account_id = ?1 AND is_draft = 0 GROUP BY cat",
    )?;
    let cat_rows: Vec<(String, i64)> = cat_stmt
        .query_map(params![account_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?
        .filter_map(|r| r.ok())
        .collect();
    let by_category: serde_json::Value = {
        let mut map = serde_json::Map::new();
        for (cat, count) in &cat_rows {
            map.insert(cat.clone(), serde_json::json!(count));
        }
        serde_json::Value::Object(map)
    };

    // top_senders: GROUP BY from_address, top 10
    let mut sender_stmt = conn.prepare(
        "SELECT from_name, from_address, COUNT(*) as cnt \
         FROM messages WHERE account_id = ?1 AND is_draft = 0 AND from_address IS NOT NULL \
         GROUP BY from_address ORDER BY cnt DESC LIMIT 10",
    )?;
    let sender_rows: Vec<serde_json::Value> = sender_stmt
        .query_map(params![account_id], |row| {
            Ok((
                row.get::<_, Option<String>>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })?
        .filter_map(|r| r.ok())
        .map(|(name, address, count)| {
            serde_json::json!({
                "name": name.unwrap_or_default(),
                "address": address,
                "count": count,
            })
        })
        .collect();
    let top_senders = serde_json::Value::Array(sender_rows);

    let by_category_str = serde_json::to_string(&by_category).unwrap_or_else(|_| "{}".to_string());
    let top_senders_str =
        serde_json::to_string(&top_senders).unwrap_or_else(|_| "[]".to_string());

    conn.execute(
        "INSERT INTO inbox_stats (account_id, total, unread, starred, by_category, top_senders, \
         today_count, week_count, month_count, last_updated) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10) \
         ON CONFLICT(account_id) DO UPDATE SET \
         total = excluded.total, unread = excluded.unread, starred = excluded.starred, \
         by_category = excluded.by_category, top_senders = excluded.top_senders, \
         today_count = excluded.today_count, week_count = excluded.week_count, \
         month_count = excluded.month_count, last_updated = excluded.last_updated",
        params![
            account_id,
            total,
            unread,
            starred,
            by_category_str,
            top_senders_str,
            today_count,
            week_count,
            month_count,
            now,
        ],
    )?;

    Ok(())
}

/// Read all inbox_stats rows.
pub fn get_all_stats(conn: &Connection) -> Result<Vec<InboxStats>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT account_id, total, unread, starred, by_category, top_senders, \
         today_count, week_count, month_count, last_updated FROM inbox_stats",
    )?;
    let rows = stmt
        .query_map([], |row| {
            let by_category_str: String = row.get(4)?;
            let top_senders_str: String = row.get(5)?;
            Ok(InboxStats {
                account_id: row.get(0)?,
                total: row.get(1)?,
                unread: row.get(2)?,
                starred: row.get(3)?,
                by_category: serde_json::from_str(&by_category_str)
                    .unwrap_or(serde_json::json!({})),
                top_senders: serde_json::from_str(&top_senders_str)
                    .unwrap_or(serde_json::json!([])),
                today_count: row.get(6)?,
                week_count: row.get(7)?,
                month_count: row.get(8)?,
                last_updated: row.get(9)?,
            })
        })?
        .filter_map(|r| {
            match r {
                Ok(v) => Some(v),
                Err(e) => {
                    tracing::warn!(error = %e, "Skipping corrupted inbox_stats row");
                    None
                }
            }
        })
        .collect();
    Ok(rows)
}

/// GET /api/inbox-stats — return precomputed inbox statistics for all accounts.
pub async fn get_inbox_stats(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<InboxStats>>, StatusCode> {
    let conn = state
        .db
        .get()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let stats = get_all_stats(&conn).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(stats))
}

/// Format inbox stats as a text block suitable for injection into an AI chat system prompt.
/// Returns empty string if no stats exist.
pub fn format_stats_for_prompt(conn: &Connection) -> String {
    let stats = match get_all_stats(conn) {
        Ok(s) if !s.is_empty() => s,
        _ => return String::new(),
    };

    let mut out = String::from("## Inbox Overview\n\n");

    for s in &stats {
        out.push_str(&format!(
            "### Account: {}\n- Total: {} | Unread: {} | Starred: {}\n- Today: {} | This week: {} | This month: {}\n",
            s.account_id, s.total, s.unread, s.starred,
            s.today_count, s.week_count, s.month_count,
        ));

        // Top senders
        if let Some(senders) = s.top_senders.as_array() {
            if !senders.is_empty() {
                out.push_str("- Top senders: ");
                let sender_strs: Vec<String> = senders
                    .iter()
                    .filter_map(|s| {
                        let addr = s.get("address")?.as_str()?;
                        let count = s.get("count")?.as_i64()?;
                        let name = s.get("name").and_then(|n| n.as_str()).unwrap_or("");
                        if name.is_empty() {
                            Some(format!("{} ({})", addr, count))
                        } else {
                            Some(format!("{} <{}> ({})", name, addr, count))
                        }
                    })
                    .collect();
                out.push_str(&sender_strs.join(", "));
                out.push('\n');
            }
        }

        // Categories
        if let Some(cats) = s.by_category.as_object() {
            if !cats.is_empty() {
                out.push_str("- Categories: ");
                let cat_strs: Vec<String> = cats
                    .iter()
                    .map(|(k, v)| format!("{}={}", k, v))
                    .collect();
                out.push_str(&cat_strs.join(", "));
                out.push('\n');
            }
        }

        out.push('\n');
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::create_test_pool;

    fn insert_test_messages(conn: &Connection, account_id: &str) {
        // Insert test account
        conn.execute(
            "INSERT INTO accounts (id, provider, email, display_name) VALUES (?1, 'gmail', ?2, 'Test')",
            params![account_id, format!("{}@test.com", account_id)],
        )
        .unwrap();

        let now = chrono::Utc::now().timestamp();
        let yesterday = now - 86400;
        let five_days_ago = now - 5 * 86400;
        let twenty_days_ago = now - 20 * 86400;

        let messages = vec![
            // (id, date, is_read, is_starred, from_name, from_address, ai_category)
            ("m1", now, 0, 1, "Alice", "alice@example.com", "primary"),
            ("m2", now, 0, 0, "Alice", "alice@example.com", "primary"),
            ("m3", yesterday, 1, 0, "Bob", "bob@example.com", "updates"),
            ("m4", five_days_ago, 1, 0, "Carol", "carol@example.com", "promotions"),
            ("m5", twenty_days_ago, 1, 0, "Dave", "dave@example.com", "primary"),
        ];

        for (id, date, is_read, is_starred, from_name, from_addr, category) in &messages {
            conn.execute(
                "INSERT INTO messages (id, account_id, subject, date, is_read, is_starred, is_draft, \
                 from_name, from_address, ai_category) \
                 VALUES (?1, ?2, 'Test subject', ?3, ?4, ?5, 0, ?6, ?7, ?8)",
                params![id, account_id, date, is_read, is_starred, from_name, from_addr, category],
            )
            .unwrap();
        }
    }

    #[test]
    fn test_compute_and_store() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        insert_test_messages(&conn, "acct1");

        compute_and_store(&conn, "acct1").unwrap();

        let stats = get_all_stats(&conn).unwrap();
        assert_eq!(stats.len(), 1);
        let s = &stats[0];
        assert_eq!(s.account_id, "acct1");
        assert_eq!(s.total, 5);
        assert_eq!(s.unread, 2);
        assert_eq!(s.starred, 1);

        // Today: m1 + m2 = 2
        assert_eq!(s.today_count, 2);
        // Week: m1 + m2 + m3 + m4 = 4 (5 days ago is within 7 days)
        assert_eq!(s.week_count, 4);
        // Month: all 5 (20 days ago is within 30 days)
        assert_eq!(s.month_count, 5);

        // by_category should have primary=3, updates=1, promotions=1
        let cats = s.by_category.as_object().unwrap();
        assert_eq!(cats.get("primary").unwrap().as_i64().unwrap(), 3);
        assert_eq!(cats.get("updates").unwrap().as_i64().unwrap(), 1);
        assert_eq!(cats.get("promotions").unwrap().as_i64().unwrap(), 1);

        // top_senders should have alice(2), bob(1), carol(1), dave(1)
        let senders = s.top_senders.as_array().unwrap();
        assert_eq!(senders.len(), 4);
        assert_eq!(
            senders[0].get("address").unwrap().as_str().unwrap(),
            "alice@example.com"
        );
        assert_eq!(senders[0].get("count").unwrap().as_i64().unwrap(), 2);
    }

    #[test]
    fn test_compute_updates_existing() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        insert_test_messages(&conn, "acct1");

        compute_and_store(&conn, "acct1").unwrap();
        let stats = get_all_stats(&conn).unwrap();
        assert_eq!(stats[0].unread, 2);

        // Mark one unread message as read
        conn.execute("UPDATE messages SET is_read = 1 WHERE id = 'm1'", [])
            .unwrap();

        compute_and_store(&conn, "acct1").unwrap();
        let stats = get_all_stats(&conn).unwrap();
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].unread, 1);
    }

    #[test]
    fn test_format_stats_for_prompt() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        insert_test_messages(&conn, "acct1");
        compute_and_store(&conn, "acct1").unwrap();

        let prompt = format_stats_for_prompt(&conn);
        assert!(prompt.contains("Inbox Overview"));
        assert!(prompt.contains("Total: 5"));
        assert!(prompt.contains("Unread: 2"));
        assert!(prompt.contains("Top senders:"));
        assert!(prompt.contains("alice@example.com"));
    }

    #[test]
    fn test_empty_stats() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        let prompt = format_stats_for_prompt(&conn);
        assert_eq!(prompt, "");

        let stats = get_all_stats(&conn).unwrap();
        assert!(stats.is_empty());
    }
}

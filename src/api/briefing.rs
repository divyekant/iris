use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::Serialize;
use std::sync::Arc;

use crate::AppState;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct BriefingResponse {
    pub summary: String,
    pub stats: BriefingStats,
    pub highlights: Vec<BriefingHighlight>,
}

#[derive(Debug, Serialize, Clone)]
pub struct BriefingStats {
    pub total_today: i64,
    pub unread: i64,
    pub needs_reply: i64,
    pub urgent: i64,
}

#[derive(Debug, Serialize, Clone)]
pub struct BriefingHighlight {
    pub message_id: String,
    pub from: String,
    pub subject: String,
    pub reason: String,
}

// ---------------------------------------------------------------------------
// GET /api/ai/briefing — generate today's email briefing
// ---------------------------------------------------------------------------

pub async fn get_briefing(
    State(state): State<Arc<AppState>>,
) -> Result<Json<BriefingResponse>, StatusCode> {
    // Phase 1: DB reads — gather today's stats and highlights
    let (stats, highlights, top_senders) = {
        let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        let ai_enabled = conn
            .query_row(
                "SELECT value FROM config WHERE key = 'ai_enabled'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap_or_else(|_| "false".to_string());

        if ai_enabled != "true" || !state.providers.has_providers() {
            return Err(StatusCode::SERVICE_UNAVAILABLE);
        }

        let today_start = chrono::Local::now()
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp();

        let stats = query_briefing_stats(&conn, today_start);
        let highlights = query_briefing_highlights(&conn, today_start);
        let top_senders = query_top_senders(&conn, today_start);

        (stats, highlights, top_senders)
    };

    // Phase 2: Build AI summary prompt and generate narrative
    let prompt = build_briefing_prompt(&stats, &highlights, &top_senders);
    let system = "You are Iris, an AI email assistant. Generate a concise, actionable daily email briefing. \
        Use 3-5 sentences. Be specific about counts and senders. If there are urgent items, mention them first. \
        If the inbox is empty, say so cheerfully. Do not use markdown formatting. Do not use bullet points.";

    let summary = state
        .providers
        .generate(&prompt, Some(system))
        .await
        .unwrap_or_else(|| generate_fallback_summary(&stats, &highlights));

    Ok(Json(BriefingResponse {
        summary,
        stats,
        highlights,
    }))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn query_briefing_stats(conn: &rusqlite::Connection, today_start: i64) -> BriefingStats {
    conn.query_row(
        "SELECT
            COUNT(*) as total,
            SUM(CASE WHEN is_read = 0 THEN 1 ELSE 0 END) as unread,
            SUM(CASE WHEN ai_intent = 'ACTION_REQUEST' THEN 1 ELSE 0 END) as needs_reply,
            SUM(CASE WHEN ai_priority_label = 'urgent' THEN 1 ELSE 0 END) as urgent
         FROM messages
         WHERE date >= ?1 AND is_deleted = 0 AND is_draft = 0",
        rusqlite::params![today_start],
        |row| {
            Ok(BriefingStats {
                total_today: row.get(0)?,
                unread: row.get::<_, i64>(1).unwrap_or(0),
                needs_reply: row.get::<_, i64>(2).unwrap_or(0),
                urgent: row.get::<_, i64>(3).unwrap_or(0),
            })
        },
    )
    .unwrap_or(BriefingStats {
        total_today: 0,
        unread: 0,
        needs_reply: 0,
        urgent: 0,
    })
}

fn query_briefing_highlights(
    conn: &rusqlite::Connection,
    today_start: i64,
) -> Vec<BriefingHighlight> {
    let mut stmt = match conn.prepare(
        "SELECT id, COALESCE(from_name, from_address, 'Unknown') as sender,
                COALESCE(subject, '(no subject)') as subj,
                ai_priority_label, ai_intent
         FROM messages
         WHERE date >= ?1
           AND is_deleted = 0
           AND is_draft = 0
           AND (ai_priority_label IN ('urgent', 'high') OR ai_intent = 'ACTION_REQUEST')
         ORDER BY
           CASE ai_priority_label WHEN 'urgent' THEN 0 WHEN 'high' THEN 1 ELSE 2 END,
           date DESC
         LIMIT 10",
    ) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("Failed to prepare briefing highlights query: {e}");
            return Vec::new();
        }
    };

    match stmt.query_map(rusqlite::params![today_start], |row| {
        let priority: Option<String> = row.get(3)?;
        let intent: Option<String> = row.get(4)?;
        let reason = if priority.as_deref() == Some("urgent") {
            "urgent".to_string()
        } else if intent.as_deref() == Some("ACTION_REQUEST") {
            "needs_reply".to_string()
        } else {
            "high_priority".to_string()
        };
        Ok(BriefingHighlight {
            message_id: row.get(0)?,
            from: row.get(1)?,
            subject: row.get(2)?,
            reason,
        })
    }) {
        Ok(rows) => rows
            .filter_map(|r| r.map_err(|e| tracing::warn!("Briefing highlight row skip: {e}")).ok())
            .collect(),
        Err(e) => {
            tracing::error!("Failed to query briefing highlights: {e}");
            Vec::new()
        }
    }
}

fn query_top_senders(conn: &rusqlite::Connection, today_start: i64) -> Vec<(String, i64)> {
    let mut stmt = match conn.prepare(
        "SELECT COALESCE(from_name, from_address, 'Unknown') as sender, COUNT(*) as cnt
         FROM messages
         WHERE date >= ?1 AND is_deleted = 0 AND is_draft = 0
         GROUP BY sender
         ORDER BY cnt DESC
         LIMIT 5",
    ) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("Failed to prepare top senders query: {e}");
            return Vec::new();
        }
    };

    match stmt.query_map(rusqlite::params![today_start], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    }) {
        Ok(rows) => rows
            .filter_map(|r| r.ok())
            .collect(),
        Err(_) => Vec::new(),
    }
}

fn build_briefing_prompt(
    stats: &BriefingStats,
    highlights: &[BriefingHighlight],
    top_senders: &[(String, i64)],
) -> String {
    let today = chrono::Local::now().format("%A, %B %e, %Y");
    let mut prompt = format!(
        "Generate a daily email briefing for {today}.\n\n\
         Stats: {total} emails today, {unread} unread, {urgent} urgent, {needs_reply} needing reply.\n",
        total = stats.total_today,
        unread = stats.unread,
        urgent = stats.urgent,
        needs_reply = stats.needs_reply,
    );

    if !top_senders.is_empty() {
        prompt.push_str("\nTop senders today:\n");
        for (sender, count) in top_senders {
            prompt.push_str(&format!("- {sender}: {count} email(s)\n"));
        }
    }

    if !highlights.is_empty() {
        prompt.push_str("\nNotable emails:\n");
        for h in highlights {
            prompt.push_str(&format!(
                "- [{reason}] From {from}: \"{subject}\"\n",
                reason = h.reason,
                from = h.from,
                subject = h.subject,
            ));
        }
    }

    if stats.total_today == 0 {
        prompt.push_str("\nThe inbox is empty today. Generate a short, cheerful response.\n");
    }

    prompt.push_str("\nSummarize concisely in 3-5 sentences.");
    prompt
}

fn generate_fallback_summary(stats: &BriefingStats, highlights: &[BriefingHighlight]) -> String {
    if stats.total_today == 0 {
        return "Your inbox is clear today. No new emails have arrived yet.".to_string();
    }

    let mut parts = vec![format!(
        "You have {} email{} today, {} unread.",
        stats.total_today,
        if stats.total_today == 1 { "" } else { "s" },
        stats.unread,
    )];

    if stats.urgent > 0 {
        parts.push(format!(
            "{} urgent email{} need{} attention.",
            stats.urgent,
            if stats.urgent == 1 { "" } else { "s" },
            if stats.urgent == 1 { "s" } else { "" },
        ));
    }

    if stats.needs_reply > 0 {
        parts.push(format!(
            "{} email{} {} waiting for a reply.",
            stats.needs_reply,
            if stats.needs_reply == 1 { "" } else { "s" },
            if stats.needs_reply == 1 { "is" } else { "are" },
        ));
    }

    if !highlights.is_empty() {
        let first = &highlights[0];
        parts.push(format!(
            "Top item: \"{}\" from {}.",
            first.subject, first.from
        ));
    }

    parts.join(" ")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_briefing_stats_query_empty_inbox() {
        let pool = crate::db::create_test_pool();
        let conn = pool.get().unwrap();

        let today_start = chrono::Local::now()
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp();

        let stats = query_briefing_stats(&conn, today_start);
        assert_eq!(stats.total_today, 0);
        assert_eq!(stats.unread, 0);
        assert_eq!(stats.needs_reply, 0);
        assert_eq!(stats.urgent, 0);
    }

    #[test]
    fn test_briefing_stats_with_messages() {
        let pool = crate::db::create_test_pool();
        let conn = pool.get().unwrap();

        // Create test account
        conn.execute(
            "INSERT INTO accounts (id, provider, email) VALUES ('acc1', 'gmail', 'test@example.com')",
            [],
        ).unwrap();

        let now_ts = chrono::Utc::now().timestamp();

        // Insert today's messages with varying attributes
        conn.execute(
            "INSERT INTO messages (id, account_id, folder, from_address, subject, date, is_read, ai_priority_label, ai_intent)
             VALUES ('m1', 'acc1', 'INBOX', 'alice@example.com', 'Urgent: Deploy fix', ?1, 0, 'urgent', 'ACTION_REQUEST')",
            rusqlite::params![now_ts],
        ).unwrap();

        conn.execute(
            "INSERT INTO messages (id, account_id, folder, from_address, subject, date, is_read, ai_priority_label, ai_intent)
             VALUES ('m2', 'acc1', 'INBOX', 'bob@example.com', 'Weekly report', ?1, 1, 'normal', 'INFORMATIONAL')",
            rusqlite::params![now_ts],
        ).unwrap();

        conn.execute(
            "INSERT INTO messages (id, account_id, folder, from_address, subject, date, is_read, ai_intent)
             VALUES ('m3', 'acc1', 'INBOX', 'carol@example.com', 'Please review PR', ?1, 0, 'ACTION_REQUEST')",
            rusqlite::params![now_ts],
        ).unwrap();

        let today_start = chrono::Local::now()
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp();

        let stats = query_briefing_stats(&conn, today_start);
        assert_eq!(stats.total_today, 3);
        assert_eq!(stats.unread, 2);
        assert_eq!(stats.needs_reply, 2); // m1 + m3 (ACTION_REQUEST)
        assert_eq!(stats.urgent, 1);      // only m1
    }

    #[test]
    fn test_briefing_highlights_extraction() {
        let pool = crate::db::create_test_pool();
        let conn = pool.get().unwrap();

        conn.execute(
            "INSERT INTO accounts (id, provider, email) VALUES ('acc1', 'gmail', 'test@example.com')",
            [],
        ).unwrap();

        let now_ts = chrono::Utc::now().timestamp();

        // Urgent message
        conn.execute(
            "INSERT INTO messages (id, account_id, folder, from_name, from_address, subject, date, ai_priority_label, ai_intent)
             VALUES ('m1', 'acc1', 'INBOX', 'Alice', 'alice@example.com', 'Server is down', ?1, 'urgent', 'ACTION_REQUEST')",
            rusqlite::params![now_ts],
        ).unwrap();

        // Normal message (should NOT appear in highlights)
        conn.execute(
            "INSERT INTO messages (id, account_id, folder, from_name, from_address, subject, date, ai_priority_label, ai_intent)
             VALUES ('m2', 'acc1', 'INBOX', 'Bob', 'bob@example.com', 'Newsletter', ?1, 'low', 'INFORMATIONAL')",
            rusqlite::params![now_ts],
        ).unwrap();

        // Action request (should appear)
        conn.execute(
            "INSERT INTO messages (id, account_id, folder, from_name, from_address, subject, date, ai_priority_label, ai_intent)
             VALUES ('m3', 'acc1', 'INBOX', 'Carol', 'carol@example.com', 'Review PR #42', ?1, 'normal', 'ACTION_REQUEST')",
            rusqlite::params![now_ts],
        ).unwrap();

        let today_start = chrono::Local::now()
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp();

        let highlights = query_briefing_highlights(&conn, today_start);
        assert_eq!(highlights.len(), 2); // m1 + m3
        assert_eq!(highlights[0].message_id, "m1"); // urgent first
        assert_eq!(highlights[0].reason, "urgent");
        assert_eq!(highlights[0].from, "Alice");
        assert_eq!(highlights[1].message_id, "m3");
        assert_eq!(highlights[1].reason, "needs_reply");
    }

    #[test]
    fn test_top_senders_query() {
        let pool = crate::db::create_test_pool();
        let conn = pool.get().unwrap();

        conn.execute(
            "INSERT INTO accounts (id, provider, email) VALUES ('acc1', 'gmail', 'test@example.com')",
            [],
        ).unwrap();

        let now_ts = chrono::Utc::now().timestamp();

        // Alice sends 3, Bob sends 1
        for i in 0..3 {
            conn.execute(
                "INSERT INTO messages (id, account_id, folder, from_name, subject, date)
                 VALUES (?1, 'acc1', 'INBOX', 'Alice', 'Msg', ?2)",
                rusqlite::params![format!("a{i}"), now_ts],
            ).unwrap();
        }
        conn.execute(
            "INSERT INTO messages (id, account_id, folder, from_name, subject, date)
             VALUES ('b1', 'acc1', 'INBOX', 'Bob', 'Hi', ?1)",
            rusqlite::params![now_ts],
        ).unwrap();

        let today_start = chrono::Local::now()
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp();

        let senders = query_top_senders(&conn, today_start);
        assert_eq!(senders.len(), 2);
        assert_eq!(senders[0].0, "Alice");
        assert_eq!(senders[0].1, 3);
        assert_eq!(senders[1].0, "Bob");
        assert_eq!(senders[1].1, 1);
    }

    #[test]
    fn test_build_briefing_prompt_empty() {
        let stats = BriefingStats {
            total_today: 0,
            unread: 0,
            needs_reply: 0,
            urgent: 0,
        };
        let prompt = build_briefing_prompt(&stats, &[], &[]);
        assert!(prompt.contains("0 emails today"));
        assert!(prompt.contains("inbox is empty today"));
    }

    #[test]
    fn test_build_briefing_prompt_with_data() {
        let stats = BriefingStats {
            total_today: 15,
            unread: 8,
            needs_reply: 3,
            urgent: 2,
        };
        let highlights = vec![
            BriefingHighlight {
                message_id: "m1".into(),
                from: "Alice".into(),
                subject: "Deploy fix".into(),
                reason: "urgent".into(),
            },
        ];
        let senders = vec![("Alice".into(), 5), ("Bob".into(), 3)];

        let prompt = build_briefing_prompt(&stats, &highlights, &senders);
        assert!(prompt.contains("15 emails today"));
        assert!(prompt.contains("8 unread"));
        assert!(prompt.contains("2 urgent"));
        assert!(prompt.contains("Alice: 5 email(s)"));
        assert!(prompt.contains("[urgent] From Alice"));
    }

    #[test]
    fn test_fallback_summary_empty() {
        let stats = BriefingStats {
            total_today: 0,
            unread: 0,
            needs_reply: 0,
            urgent: 0,
        };
        let summary = generate_fallback_summary(&stats, &[]);
        assert!(summary.contains("clear today"));
    }

    #[test]
    fn test_fallback_summary_with_data() {
        let stats = BriefingStats {
            total_today: 5,
            unread: 3,
            needs_reply: 1,
            urgent: 2,
        };
        let highlights = vec![BriefingHighlight {
            message_id: "m1".into(),
            from: "Alice".into(),
            subject: "Important".into(),
            reason: "urgent".into(),
        }];
        let summary = generate_fallback_summary(&stats, &highlights);
        assert!(summary.contains("5 emails today"));
        assert!(summary.contains("3 unread"));
        assert!(summary.contains("2 urgent"));
        assert!(summary.contains("1 email is waiting"));
        assert!(summary.contains("Important"));
    }
}

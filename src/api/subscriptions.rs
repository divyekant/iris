use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::Serialize;
use std::sync::Arc;

use crate::AppState;

#[derive(Debug, Serialize)]
pub struct SubscriptionAuditResponse {
    pub subscriptions: Vec<SubscriptionInfo>,
}

#[derive(Debug, Serialize)]
pub struct SubscriptionInfo {
    pub sender: String,
    pub sender_name: Option<String>,
    pub total_count: i64,
    pub read_count: i64,
    pub read_rate: f64,
    pub last_received: i64,
    pub has_unsubscribe: bool,
    pub category: Option<String>,
}

pub async fn subscription_audit(
    State(state): State<Arc<AppState>>,
) -> Result<Json<SubscriptionAuditResponse>, StatusCode> {
    let conn = state
        .db
        .get()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Group messages by from_address, filter to senders with 3+ emails
    // (indicating a subscription/newsletter pattern), calculate read rate.
    // Check raw_headers for List-Unsubscribe to detect unsubscribe support.
    // ORDER BY read_rate ASC (least-read senders first).
    let mut stmt = conn
        .prepare(
            "SELECT from_address, from_name,
                    COUNT(*) as total,
                    SUM(CASE WHEN is_read = 1 THEN 1 ELSE 0 END) as read_count,
                    MAX(date) as last_received,
                    MAX(CASE WHEN raw_headers LIKE '%List-Unsubscribe%' THEN 1 ELSE 0 END) as has_unsub,
                    ai_category
             FROM messages
             WHERE is_deleted = 0 AND is_draft = 0
             GROUP BY from_address
             HAVING total >= 3
             ORDER BY (CAST(read_count AS REAL) / total) ASC
             LIMIT 50",
        )
        .map_err(|e| {
            tracing::error!("Subscription audit query error: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let subs = stmt
        .query_map([], |row| {
            let total: i64 = row.get(2)?;
            let read: i64 = row.get(3)?;
            Ok(SubscriptionInfo {
                sender: row.get(0)?,
                sender_name: row.get(1)?,
                total_count: total,
                read_count: read,
                read_rate: if total > 0 {
                    read as f64 / total as f64
                } else {
                    0.0
                },
                last_received: row.get(4)?,
                has_unsubscribe: row.get::<_, i64>(5)? > 0,
                category: row.get(6)?,
            })
        })
        .map_err(|e| {
            tracing::error!("Subscription audit error: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let subscriptions: Vec<SubscriptionInfo> = subs.filter_map(|r| r.ok()).collect();
    Ok(Json(SubscriptionAuditResponse { subscriptions }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::create_test_pool;
    use crate::models::account::{Account, CreateAccount};
    use crate::models::message::InsertMessage;

    fn create_test_account(
        conn: &r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>,
    ) -> Account {
        let input = CreateAccount {
            provider: "gmail".to_string(),
            email: "sub-test@example.com".to_string(),
            display_name: Some("Sub Test".to_string()),
            imap_host: Some("imap.gmail.com".to_string()),
            imap_port: Some(993),
            smtp_host: Some("smtp.gmail.com".to_string()),
            smtp_port: Some(587),
            username: Some("sub-test@example.com".to_string()),
            password: None,
        };
        Account::create(conn, &input)
    }

    fn make_msg(
        account_id: &str,
        sender: &str,
        sender_name: &str,
        subject: &str,
        is_read: bool,
        uid: i64,
        raw_headers: Option<&str>,
    ) -> InsertMessage {
        InsertMessage {
            account_id: account_id.to_string(),
            message_id: Some(format!("<{subject}-{uid}@example.com>")),
            thread_id: None,
            folder: "INBOX".to_string(),
            from_address: Some(sender.to_string()),
            from_name: Some(sender_name.to_string()),
            to_addresses: Some(r#"["sub-test@example.com"]"#.to_string()),
            cc_addresses: None,
            bcc_addresses: None,
            subject: Some(subject.to_string()),
            date: Some(1700000000 + uid),
            snippet: Some("Preview...".to_string()),
            body_text: Some("Body text".to_string()),
            body_html: None,
            is_read,
            is_starred: false,
            is_draft: false,
            labels: None,
            uid: Some(uid),
            modseq: None,
            raw_headers: raw_headers.map(|s| s.to_string()),
            has_attachments: false,
            attachment_names: None,
            size_bytes: Some(512),
        }
    }

    #[test]
    fn test_subscription_audit_groups_by_sender() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);

        // Sender A: 4 emails, 1 read => 25% rate
        for i in 0..4 {
            let msg = make_msg(
                &account.id,
                "newsletter@spam.com",
                "Spam Newsletter",
                &format!("spam-{i}"),
                i == 0, // only first is read
                i + 1,
                Some("List-Unsubscribe: <mailto:unsub@spam.com>"),
            );
            InsertMessage::insert(&conn, &msg);
        }

        // Sender B: 3 emails, 3 read => 100% rate
        for i in 0..3 {
            let msg = make_msg(
                &account.id,
                "important@work.com",
                "Work Updates",
                &format!("work-{i}"),
                true,
                10 + i + 1,
                None,
            );
            InsertMessage::insert(&conn, &msg);
        }

        // Sender C: 2 emails (below threshold, should NOT appear)
        for i in 0..2 {
            let msg = make_msg(
                &account.id,
                "rare@sender.com",
                "Rare",
                &format!("rare-{i}"),
                false,
                20 + i + 1,
                None,
            );
            InsertMessage::insert(&conn, &msg);
        }

        // Run the query directly
        let mut stmt = conn
            .prepare(
                "SELECT from_address, from_name,
                        COUNT(*) as total,
                        SUM(CASE WHEN is_read = 1 THEN 1 ELSE 0 END) as read_count,
                        MAX(date) as last_received,
                        MAX(CASE WHEN raw_headers LIKE '%List-Unsubscribe%' THEN 1 ELSE 0 END) as has_unsub,
                        ai_category
                 FROM messages
                 WHERE is_deleted = 0 AND is_draft = 0
                 GROUP BY from_address
                 HAVING total >= 3
                 ORDER BY (CAST(read_count AS REAL) / total) ASC
                 LIMIT 50",
            )
            .unwrap();

        let results: Vec<SubscriptionInfo> = stmt
            .query_map([], |row| {
                let total: i64 = row.get(2)?;
                let read: i64 = row.get(3)?;
                Ok(SubscriptionInfo {
                    sender: row.get(0)?,
                    sender_name: row.get(1)?,
                    total_count: total,
                    read_count: read,
                    read_rate: if total > 0 {
                        read as f64 / total as f64
                    } else {
                        0.0
                    },
                    last_received: row.get(4)?,
                    has_unsubscribe: row.get::<_, i64>(5)? > 0,
                    category: row.get(6)?,
                })
            })
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        // Should have exactly 2 senders (threshold is 3+ emails)
        assert_eq!(results.len(), 2);

        // First result should be the spam sender (lowest read rate: 25%)
        assert_eq!(results[0].sender, "newsletter@spam.com");
        assert_eq!(results[0].total_count, 4);
        assert_eq!(results[0].read_count, 1);
        assert!((results[0].read_rate - 0.25).abs() < 0.01);
        assert!(results[0].has_unsubscribe);

        // Second result should be work sender (100% read rate)
        assert_eq!(results[1].sender, "important@work.com");
        assert_eq!(results[1].total_count, 3);
        assert_eq!(results[1].read_count, 3);
        assert!((results[1].read_rate - 1.0).abs() < 0.01);
        assert!(!results[1].has_unsubscribe);
    }

    #[test]
    fn test_subscription_audit_empty_inbox() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        let mut stmt = conn
            .prepare(
                "SELECT from_address, from_name,
                        COUNT(*) as total,
                        SUM(CASE WHEN is_read = 1 THEN 1 ELSE 0 END) as read_count,
                        MAX(date) as last_received,
                        MAX(CASE WHEN raw_headers LIKE '%List-Unsubscribe%' THEN 1 ELSE 0 END) as has_unsub,
                        ai_category
                 FROM messages
                 WHERE is_deleted = 0 AND is_draft = 0
                 GROUP BY from_address
                 HAVING total >= 3
                 ORDER BY (CAST(read_count AS REAL) / total) ASC
                 LIMIT 50",
            )
            .unwrap();

        let results: Vec<SubscriptionInfo> = stmt
            .query_map([], |row| {
                let total: i64 = row.get(2)?;
                let read: i64 = row.get(3)?;
                Ok(SubscriptionInfo {
                    sender: row.get(0)?,
                    sender_name: row.get(1)?,
                    total_count: total,
                    read_count: read,
                    read_rate: if total > 0 {
                        read as f64 / total as f64
                    } else {
                        0.0
                    },
                    last_received: row.get(4)?,
                    has_unsubscribe: row.get::<_, i64>(5)? > 0,
                    category: row.get(6)?,
                })
            })
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert!(results.is_empty());
    }

    #[test]
    fn test_subscription_audit_excludes_deleted_and_drafts() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);

        // Insert 3 messages from same sender, but mark one deleted
        for i in 0..3 {
            let msg = make_msg(
                &account.id,
                "deleted@test.com",
                "Deleted Test",
                &format!("del-{i}"),
                false,
                30 + i + 1,
                None,
            );
            InsertMessage::insert(&conn, &msg);
        }

        // Soft-delete one (use subquery since SQLite doesn't support LIMIT on UPDATE)
        conn.execute(
            "UPDATE messages SET is_deleted = 1 WHERE id = (
                SELECT id FROM messages WHERE from_address = 'deleted@test.com' LIMIT 1
            )",
            [],
        )
        .unwrap();

        let mut stmt = conn
            .prepare(
                "SELECT from_address, from_name,
                        COUNT(*) as total,
                        SUM(CASE WHEN is_read = 1 THEN 1 ELSE 0 END) as read_count,
                        MAX(date) as last_received,
                        MAX(CASE WHEN raw_headers LIKE '%List-Unsubscribe%' THEN 1 ELSE 0 END) as has_unsub,
                        ai_category
                 FROM messages
                 WHERE is_deleted = 0 AND is_draft = 0
                 GROUP BY from_address
                 HAVING total >= 3
                 ORDER BY (CAST(read_count AS REAL) / total) ASC
                 LIMIT 50",
            )
            .unwrap();

        let results: Vec<SubscriptionInfo> = stmt
            .query_map([], |row| {
                let total: i64 = row.get(2)?;
                let read: i64 = row.get(3)?;
                Ok(SubscriptionInfo {
                    sender: row.get(0)?,
                    sender_name: row.get(1)?,
                    total_count: total,
                    read_count: read,
                    read_rate: if total > 0 {
                        read as f64 / total as f64
                    } else {
                        0.0
                    },
                    last_received: row.get(4)?,
                    has_unsubscribe: row.get::<_, i64>(5)? > 0,
                    category: row.get(6)?,
                })
            })
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        // Only 2 non-deleted messages from that sender — below threshold
        assert!(results.is_empty());
    }
}

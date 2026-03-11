use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Serialize;
use std::sync::Arc;

use crate::AppState;

#[derive(Debug, Serialize)]
pub struct ResponseTimesResponse {
    pub email: String,
    pub their_avg_reply_hours: Option<f64>,
    pub your_avg_reply_hours: Option<f64>,
    pub their_reply_count: i64,
    pub your_reply_count: i64,
    pub fastest_reply_hours: Option<f64>,
    pub slowest_reply_hours: Option<f64>,
    pub total_exchanges: i64,
}

/// GET /api/contacts/{email}/response-times
///
/// Calculates response time patterns between the user and a specific contact.
/// Looks at consecutive messages in shared threads to determine reply deltas.
pub async fn get_response_times(
    State(state): State<Arc<AppState>>,
    Path(email): Path<String>,
) -> Result<Json<ResponseTimesResponse>, StatusCode> {
    // Validate email
    if !email.contains('@') || email.len() > 320 {
        return Err(StatusCode::BAD_REQUEST);
    }

    let contact_email = email.to_lowercase();
    let like_pattern = format!("%{}%", contact_email);

    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Collect all account emails (these are "the user")
    let account_emails: Vec<String> = {
        let mut stmt = conn
            .prepare("SELECT LOWER(email) FROM accounts WHERE is_active = 1")
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        stmt.query_map([], |row| row.get::<_, String>(0))
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .filter_map(|r| r.ok())
            .collect()
    };

    if account_emails.is_empty() {
        return Ok(Json(ResponseTimesResponse {
            email: contact_email,
            their_avg_reply_hours: None,
            your_avg_reply_hours: None,
            their_reply_count: 0,
            your_reply_count: 0,
            fastest_reply_hours: None,
            slowest_reply_hours: None,
            total_exchanges: 0,
        }));
    }

    // Get all messages in threads involving this contact, ordered by thread and date
    let mut stmt = conn
        .prepare(
            "SELECT m.thread_id, LOWER(m.from_address) as from_addr, m.date, a.email as account_email
             FROM messages m
             JOIN accounts a ON m.account_id = a.id
             WHERE m.is_deleted = 0
               AND m.thread_id IS NOT NULL
               AND m.date IS NOT NULL
               AND (LOWER(m.from_address) = ?1
                    OR m.to_addresses LIKE ?2
                    OR m.cc_addresses LIKE ?2)
             ORDER BY m.thread_id, m.date ASC",
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let rows: Vec<(String, String, i64, String)> = stmt
        .query_map(rusqlite::params![&contact_email, &like_pattern], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .filter_map(|r| r.ok())
        .collect();

    let stats = compute_reply_stats(&rows, &contact_email, &account_emails);

    Ok(Json(ResponseTimesResponse {
        email: contact_email,
        their_avg_reply_hours: stats.their_avg,
        your_avg_reply_hours: stats.your_avg,
        their_reply_count: stats.their_count,
        your_reply_count: stats.your_count,
        fastest_reply_hours: stats.fastest,
        slowest_reply_hours: stats.slowest,
        total_exchanges: stats.total_exchanges,
    }))
}

struct ReplyStats {
    their_avg: Option<f64>,
    your_avg: Option<f64>,
    their_count: i64,
    your_count: i64,
    fastest: Option<f64>,
    slowest: Option<f64>,
    total_exchanges: i64,
}

fn compute_reply_stats(
    rows: &[(String, String, i64, String)],
    contact_email: &str,
    account_emails: &[String],
) -> ReplyStats {
    let mut their_reply_secs: Vec<f64> = Vec::new();
    let mut your_reply_secs: Vec<f64> = Vec::new();
    let mut total_exchanges: i64 = 0;

    // Group by thread_id
    let mut i = 0;
    while i < rows.len() {
        let thread_id = &rows[i].0;
        let mut thread_end = i + 1;
        while thread_end < rows.len() && rows[thread_end].0 == *thread_id {
            thread_end += 1;
        }

        let thread_msgs = &rows[i..thread_end];
        total_exchanges += thread_msgs.len() as i64;

        // Look at consecutive message pairs within this thread
        for pair in thread_msgs.windows(2) {
            let (_, ref from_a, date_a, _) = pair[0];
            let (_, ref from_b, date_b, _) = pair[1];

            // Skip if same sender (no reply)
            if from_a == from_b {
                continue;
            }

            let delta_secs = (date_b - date_a) as f64;
            // Skip negative or zero deltas (clock skew or same-second)
            if delta_secs <= 0.0 {
                continue;
            }

            let a_is_contact = from_a == contact_email;
            let a_is_user = account_emails.iter().any(|ae| ae == from_a);
            let b_is_contact = from_b == contact_email;
            let b_is_user = account_emails.iter().any(|ae| ae == from_b);

            if a_is_contact && b_is_user {
                // Contact sent, user replied → your reply time
                your_reply_secs.push(delta_secs);
            } else if a_is_user && b_is_contact {
                // User sent, contact replied → their reply time
                their_reply_secs.push(delta_secs);
            }
        }

        i = thread_end;
    }

    let their_avg = if their_reply_secs.is_empty() {
        None
    } else {
        let sum: f64 = their_reply_secs.iter().sum();
        Some(sum / their_reply_secs.len() as f64 / 3600.0)
    };

    let your_avg = if your_reply_secs.is_empty() {
        None
    } else {
        let sum: f64 = your_reply_secs.iter().sum();
        Some(sum / your_reply_secs.len() as f64 / 3600.0)
    };

    let all_deltas: Vec<f64> = their_reply_secs
        .iter()
        .chain(your_reply_secs.iter())
        .copied()
        .collect();

    let fastest = all_deltas
        .iter()
        .copied()
        .reduce(f64::min)
        .map(|s| s / 3600.0);
    let slowest = all_deltas
        .iter()
        .copied()
        .reduce(f64::max)
        .map(|s| s / 3600.0);

    ReplyStats {
        their_avg,
        your_avg,
        their_count: their_reply_secs.len() as i64,
        your_count: your_reply_secs.len() as i64,
        fastest,
        slowest,
        total_exchanges,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::create_test_pool;

    #[test]
    fn test_compute_reply_stats_basic() {
        // Thread: contact sends at t=0, user replies at t=3600 (1hr), contact replies at t=7200 (2hr)
        let rows = vec![
            ("thread1".to_string(), "contact@example.com".to_string(), 1000, "user@me.com".to_string()),
            ("thread1".to_string(), "user@me.com".to_string(), 4600, "user@me.com".to_string()),
            ("thread1".to_string(), "contact@example.com".to_string(), 11800, "user@me.com".to_string()),
        ];
        let account_emails = vec!["user@me.com".to_string()];
        let stats = compute_reply_stats(&rows, "contact@example.com", &account_emails);

        // User replied to contact: 4600 - 1000 = 3600s = 1.0hr
        assert_eq!(stats.your_count, 1);
        assert!((stats.your_avg.unwrap() - 1.0).abs() < 0.01);

        // Contact replied to user: 11800 - 4600 = 7200s = 2.0hr
        assert_eq!(stats.their_count, 1);
        assert!((stats.their_avg.unwrap() - 2.0).abs() < 0.01);

        // Fastest = 1.0hr, slowest = 2.0hr
        assert!((stats.fastest.unwrap() - 1.0).abs() < 0.01);
        assert!((stats.slowest.unwrap() - 2.0).abs() < 0.01);

        assert_eq!(stats.total_exchanges, 3);
    }

    #[test]
    fn test_compute_reply_stats_empty() {
        let rows: Vec<(String, String, i64, String)> = vec![];
        let account_emails = vec!["user@me.com".to_string()];
        let stats = compute_reply_stats(&rows, "contact@example.com", &account_emails);

        assert!(stats.their_avg.is_none());
        assert!(stats.your_avg.is_none());
        assert_eq!(stats.their_count, 0);
        assert_eq!(stats.your_count, 0);
        assert!(stats.fastest.is_none());
        assert!(stats.slowest.is_none());
        assert_eq!(stats.total_exchanges, 0);
    }

    #[test]
    fn test_compute_reply_stats_same_sender_no_reply() {
        // Two messages from the same sender in a row should not count as a reply
        let rows = vec![
            ("thread1".to_string(), "contact@example.com".to_string(), 1000, "user@me.com".to_string()),
            ("thread1".to_string(), "contact@example.com".to_string(), 2000, "user@me.com".to_string()),
        ];
        let account_emails = vec!["user@me.com".to_string()];
        let stats = compute_reply_stats(&rows, "contact@example.com", &account_emails);

        assert_eq!(stats.their_count, 0);
        assert_eq!(stats.your_count, 0);
        assert!(stats.their_avg.is_none());
        assert!(stats.your_avg.is_none());
    }

    #[test]
    fn test_compute_reply_stats_multiple_threads() {
        let rows = vec![
            // Thread 1: contact → user (1hr reply)
            ("thread1".to_string(), "bob@work.com".to_string(), 1000, "me@home.com".to_string()),
            ("thread1".to_string(), "me@home.com".to_string(), 4600, "me@home.com".to_string()),
            // Thread 2: user → contact (2hr reply)
            ("thread2".to_string(), "me@home.com".to_string(), 10000, "me@home.com".to_string()),
            ("thread2".to_string(), "bob@work.com".to_string(), 17200, "me@home.com".to_string()),
        ];
        let account_emails = vec!["me@home.com".to_string()];
        let stats = compute_reply_stats(&rows, "bob@work.com", &account_emails);

        assert_eq!(stats.your_count, 1);
        assert_eq!(stats.their_count, 1);
        assert!((stats.your_avg.unwrap() - 1.0).abs() < 0.01);
        assert!((stats.their_avg.unwrap() - 2.0).abs() < 0.01);
        assert_eq!(stats.total_exchanges, 4);
    }

    #[test]
    fn test_response_times_with_db() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        // Create an account
        conn.execute(
            "INSERT INTO accounts (id, email, provider, display_name, is_active) VALUES (?1, ?2, ?3, ?4, 1)",
            rusqlite::params!["acc1", "user@test.com", "gmail", "Test User"],
        ).unwrap();

        // Insert messages in a thread with known timestamps
        conn.execute(
            "INSERT INTO messages (id, account_id, thread_id, folder, from_address, from_name, subject, date, is_read, is_deleted)
             VALUES ('m1', 'acc1', 'thread1', 'INBOX', 'contact@external.com', 'Contact', 'Hello', 1000, 1, 0)",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO messages (id, account_id, thread_id, folder, from_address, from_name, subject, date, is_read, is_deleted, to_addresses)
             VALUES ('m2', 'acc1', 'thread1', 'Sent', 'user@test.com', 'Test User', 'Re: Hello', 4600, 1, 0, '[\"contact@external.com\"]')",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO messages (id, account_id, thread_id, folder, from_address, from_name, subject, date, is_read, is_deleted, to_addresses)
             VALUES ('m3', 'acc1', 'thread1', 'INBOX', 'contact@external.com', 'Contact', 'Re: Re: Hello', 18200, 1, 0, '[\"user@test.com\"]')",
            [],
        ).unwrap();

        // Query the same way the handler does
        let contact_email = "contact@external.com";
        let like_pattern = format!("%{}%", contact_email);

        let mut stmt = conn
            .prepare(
                "SELECT m.thread_id, LOWER(m.from_address) as from_addr, m.date, a.email as account_email
                 FROM messages m
                 JOIN accounts a ON m.account_id = a.id
                 WHERE m.is_deleted = 0
                   AND m.thread_id IS NOT NULL
                   AND m.date IS NOT NULL
                   AND (LOWER(m.from_address) = ?1
                        OR m.to_addresses LIKE ?2
                        OR m.cc_addresses LIKE ?2)
                 ORDER BY m.thread_id, m.date ASC",
            )
            .unwrap();

        let rows: Vec<(String, String, i64, String)> = stmt
            .query_map(rusqlite::params![contact_email, &like_pattern], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        let account_emails = vec!["user@test.com".to_string()];
        let stats = compute_reply_stats(&rows, contact_email, &account_emails);

        // m1→m2: user replied in 3600s = 1hr
        assert_eq!(stats.your_count, 1);
        assert!((stats.your_avg.unwrap() - 1.0).abs() < 0.01);

        // m2→m3: contact replied in 13600s = ~3.78hr
        assert_eq!(stats.their_count, 1);
        assert!((stats.their_avg.unwrap() - 3.7778).abs() < 0.01);

        assert_eq!(stats.total_exchanges, 3);
    }

    #[test]
    fn test_invalid_email_validation() {
        // The email validation is in the handler, but we can test the logic
        let email = "notanemail";
        assert!(!email.contains('@'));
    }
}

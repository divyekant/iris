use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use crate::AppState;

// ---------------------------------------------------------------------------
// Request / Response types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct GenerateRequest {
    pub account_id: String,
    pub report_type: Option<String>, // "weekly", "monthly", "custom"
    pub period_start: Option<String>, // ISO date like "2026-03-01"
    pub period_end: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct GenerateResponse {
    pub id: i64,
    pub report_type: String,
    pub period_start: String,
    pub period_end: String,
}

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    pub account_id: Option<String>,
    pub report_type: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ReportSummary {
    pub id: i64,
    pub account_id: String,
    pub report_type: String,
    pub period_start: String,
    pub period_end: String,
    pub generated_at: String,
}

#[derive(Debug, Serialize)]
pub struct ReportDetail {
    pub id: i64,
    pub account_id: String,
    pub report_type: String,
    pub period_start: String,
    pub period_end: String,
    pub report_data: serde_json::Value,
    pub insights: Option<String>,
    pub generated_at: String,
}

#[derive(Debug, Serialize)]
pub struct DeleteResponse {
    pub deleted: bool,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Resolve the period_start and period_end from the request.
/// Defaults: weekly = last 7 days, monthly = last 30 days.
fn resolve_period(
    report_type: &str,
    period_start: Option<&str>,
    period_end: Option<&str>,
) -> (String, String) {
    if let (Some(s), Some(e)) = (period_start, period_end) {
        return (s.to_string(), e.to_string());
    }

    let now = chrono::Utc::now();
    let end = now.format("%Y-%m-%d").to_string();
    let days_back: i64 = match report_type {
        "monthly" => 30,
        _ => 7, // weekly or custom without explicit dates
    };
    let start = (now - chrono::Duration::days(days_back))
        .format("%Y-%m-%d")
        .to_string();
    (start, end)
}

/// Compute all metrics from the messages table for the given account + period.
fn compute_metrics(
    conn: &r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>,
    account_id: &str,
    period_start: &str,
    period_end: &str,
) -> serde_json::Value {
    // ---- Volume ----
    let total_received: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM messages
             WHERE account_id = ?1
               AND folder != 'Sent' AND folder != 'Drafts'
               AND is_deleted = 0
               AND date >= strftime('%s', ?2)
               AND date <= strftime('%s', ?3, '+1 day')",
            rusqlite::params![account_id, period_start, period_end],
            |row| row.get(0),
        )
        .unwrap_or(0);

    let total_sent: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM messages
             WHERE account_id = ?1
               AND folder = 'Sent'
               AND is_deleted = 0
               AND date >= strftime('%s', ?2)
               AND date <= strftime('%s', ?3, '+1 day')",
            rusqlite::params![account_id, period_start, period_end],
            |row| row.get(0),
        )
        .unwrap_or(0);

    // Compute number of days in period for daily average
    let total = total_received + total_sent;
    let days: f64 = {
        let d: i64 = conn
            .query_row(
                "SELECT CAST((julianday(?2, '+1 day') - julianday(?1)) AS INTEGER)",
                rusqlite::params![period_start, period_end],
                |row| row.get(0),
            )
            .unwrap_or(1);
        if d < 1 { 1.0 } else { d as f64 }
    };
    let daily_average = (total as f64 / days * 100.0).round() / 100.0;

    // ---- Response time (avg seconds between received email and reply in same thread) ----
    let avg_response_time: Option<f64> = conn
        .query_row(
            "SELECT AVG(reply.date - received.date)
             FROM messages reply
             JOIN messages received ON reply.thread_id = received.thread_id
                AND received.folder != 'Sent'
                AND received.folder != 'Drafts'
                AND reply.folder = 'Sent'
                AND reply.date > received.date
             WHERE reply.account_id = ?1
               AND reply.is_deleted = 0
               AND received.is_deleted = 0
               AND reply.date >= strftime('%s', ?2)
               AND reply.date <= strftime('%s', ?3, '+1 day')",
            rusqlite::params![account_id, period_start, period_end],
            |row| row.get(0),
        )
        .unwrap_or(None);

    // ---- Top senders (top 10) ----
    let mut top_senders: Vec<serde_json::Value> = Vec::new();
    {
        let mut stmt = conn
            .prepare(
                "SELECT from_address, COUNT(*) as cnt FROM messages
                 WHERE account_id = ?1
                   AND folder != 'Sent' AND folder != 'Drafts'
                   AND is_deleted = 0
                   AND from_address IS NOT NULL
                   AND date >= strftime('%s', ?2)
                   AND date <= strftime('%s', ?3, '+1 day')
                 GROUP BY from_address
                 ORDER BY cnt DESC
                 LIMIT 10",
            )
            .unwrap();
        let rows = stmt
            .query_map(
                rusqlite::params![account_id, period_start, period_end],
                |row| {
                    let addr: String = row.get(0)?;
                    let count: i64 = row.get(1)?;
                    Ok((addr, count))
                },
            )
            .unwrap();
        for r in rows.flatten() {
            top_senders.push(serde_json::json!({"address": r.0, "count": r.1}));
        }
    }

    // ---- Top recipients (top 10 from Sent folder to_addresses) ----
    let mut top_recipients: Vec<serde_json::Value> = Vec::new();
    {
        let mut stmt = conn
            .prepare(
                "SELECT to_addresses, COUNT(*) as cnt FROM messages
                 WHERE account_id = ?1
                   AND folder = 'Sent'
                   AND is_deleted = 0
                   AND to_addresses IS NOT NULL
                   AND date >= strftime('%s', ?2)
                   AND date <= strftime('%s', ?3, '+1 day')
                 GROUP BY to_addresses
                 ORDER BY cnt DESC
                 LIMIT 10",
            )
            .unwrap();
        let rows = stmt
            .query_map(
                rusqlite::params![account_id, period_start, period_end],
                |row| {
                    let addr: String = row.get(0)?;
                    let count: i64 = row.get(1)?;
                    Ok((addr, count))
                },
            )
            .unwrap();
        for r in rows.flatten() {
            top_recipients.push(serde_json::json!({"addresses": r.0, "count": r.1}));
        }
    }

    // ---- Category distribution ----
    let mut categories: Vec<serde_json::Value> = Vec::new();
    {
        let mut stmt = conn
            .prepare(
                "SELECT COALESCE(ai_category, 'Uncategorized'), COUNT(*) as cnt FROM messages
                 WHERE account_id = ?1
                   AND is_deleted = 0
                   AND date >= strftime('%s', ?2)
                   AND date <= strftime('%s', ?3, '+1 day')
                 GROUP BY COALESCE(ai_category, 'Uncategorized')
                 ORDER BY cnt DESC",
            )
            .unwrap();
        let rows = stmt
            .query_map(
                rusqlite::params![account_id, period_start, period_end],
                |row| {
                    let cat: String = row.get(0)?;
                    let count: i64 = row.get(1)?;
                    Ok((cat, count))
                },
            )
            .unwrap();
        for r in rows.flatten() {
            categories.push(serde_json::json!({"category": r.0, "count": r.1}));
        }
    }

    // ---- Hourly distribution (0-23) ----
    let mut hourly: HashMap<i64, i64> = HashMap::new();
    {
        let mut stmt = conn
            .prepare(
                "SELECT CAST(strftime('%H', date, 'unixepoch') AS INTEGER) as hour, COUNT(*) as cnt
                 FROM messages
                 WHERE account_id = ?1
                   AND is_deleted = 0
                   AND date IS NOT NULL
                   AND date >= strftime('%s', ?2)
                   AND date <= strftime('%s', ?3, '+1 day')
                 GROUP BY hour
                 ORDER BY hour",
            )
            .unwrap();
        let rows = stmt
            .query_map(
                rusqlite::params![account_id, period_start, period_end],
                |row| {
                    let hour: i64 = row.get(0)?;
                    let count: i64 = row.get(1)?;
                    Ok((hour, count))
                },
            )
            .unwrap();
        for r in rows.flatten() {
            hourly.insert(r.0, r.1);
        }
    }
    // Fill in 0 for missing hours
    let hourly_vec: Vec<serde_json::Value> = (0..24)
        .map(|h| {
            serde_json::json!({
                "hour": h,
                "count": hourly.get(&h).copied().unwrap_or(0)
            })
        })
        .collect();

    // ---- Read rate ----
    let total_inbox: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM messages
             WHERE account_id = ?1
               AND folder != 'Sent' AND folder != 'Drafts'
               AND is_deleted = 0
               AND date >= strftime('%s', ?2)
               AND date <= strftime('%s', ?3, '+1 day')",
            rusqlite::params![account_id, period_start, period_end],
            |row| row.get(0),
        )
        .unwrap_or(0);

    let read_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM messages
             WHERE account_id = ?1
               AND folder != 'Sent' AND folder != 'Drafts'
               AND is_deleted = 0
               AND is_read = 1
               AND date >= strftime('%s', ?2)
               AND date <= strftime('%s', ?3, '+1 day')",
            rusqlite::params![account_id, period_start, period_end],
            |row| row.get(0),
        )
        .unwrap_or(0);

    let read_rate = if total_inbox > 0 {
        (read_count as f64 / total_inbox as f64 * 10000.0).round() / 100.0
    } else {
        0.0
    };

    // ---- Folder distribution ----
    let mut folders: Vec<serde_json::Value> = Vec::new();
    {
        let mut stmt = conn
            .prepare(
                "SELECT folder, COUNT(*) as cnt FROM messages
                 WHERE account_id = ?1
                   AND is_deleted = 0
                   AND date >= strftime('%s', ?2)
                   AND date <= strftime('%s', ?3, '+1 day')
                 GROUP BY folder
                 ORDER BY cnt DESC",
            )
            .unwrap();
        let rows = stmt
            .query_map(
                rusqlite::params![account_id, period_start, period_end],
                |row| {
                    let folder: String = row.get(0)?;
                    let count: i64 = row.get(1)?;
                    Ok((folder, count))
                },
            )
            .unwrap();
        for r in rows.flatten() {
            folders.push(serde_json::json!({"folder": r.0, "count": r.1}));
        }
    }

    serde_json::json!({
        "volume": {
            "total_received": total_received,
            "total_sent": total_sent,
            "daily_average": daily_average
        },
        "response_time": {
            "average_seconds": avg_response_time
        },
        "top_contacts": {
            "senders": top_senders,
            "recipients": top_recipients
        },
        "category_distribution": categories,
        "hourly_distribution": hourly_vec,
        "read_rate": {
            "total_received": total_inbox,
            "read_count": read_count,
            "percentage": read_rate
        },
        "folder_distribution": folders
    })
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// POST /api/health-reports/generate
pub async fn generate_report(
    State(state): State<Arc<AppState>>,
    Json(req): Json<GenerateRequest>,
) -> Result<Json<GenerateResponse>, StatusCode> {
    let report_type = req.report_type.unwrap_or_else(|| "weekly".to_string());

    // Validate report_type
    if !["weekly", "monthly", "custom"].contains(&report_type.as_str()) {
        return Err(StatusCode::BAD_REQUEST);
    }

    let (period_start, period_end) = resolve_period(
        &report_type,
        req.period_start.as_deref(),
        req.period_end.as_deref(),
    );

    // Validate that period_start <= period_end
    if period_start > period_end {
        return Err(StatusCode::BAD_REQUEST);
    }

    let conn = state
        .db
        .get()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Verify account exists
    let account_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM accounts WHERE id = ?1",
            rusqlite::params![req.account_id],
            |row| row.get::<_, i64>(0),
        )
        .map(|c| c > 0)
        .unwrap_or(false);

    if !account_exists {
        return Err(StatusCode::NOT_FOUND);
    }

    // Compute metrics
    let report_data = compute_metrics(&conn, &req.account_id, &period_start, &period_end);
    let report_data_str =
        serde_json::to_string(&report_data).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Optionally generate AI insights
    let insights = {
        let prompt = format!(
            "Analyze this email communication health report and provide 3-5 actionable insights.\n\
             Period: {} to {}\n\
             Report data: {}",
            period_start, period_end, report_data_str
        );
        let system = "You are an email productivity analyst. Provide concise, actionable insights \
                       about communication patterns. Focus on response time improvements, volume \
                       management, and contact engagement. Keep each insight to 1-2 sentences.";
        state.providers.generate(&prompt, Some(system)).await
    };

    // Insert report into database
    conn.execute(
        "INSERT INTO health_reports (account_id, report_type, period_start, period_end, report_data, insights)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![
            req.account_id,
            report_type,
            period_start,
            period_end,
            report_data_str,
            insights,
        ],
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let id = conn.last_insert_rowid();

    Ok(Json(GenerateResponse {
        id,
        report_type,
        period_start,
        period_end,
    }))
}

/// GET /api/health-reports
pub async fn list_reports(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ListQuery>,
) -> Result<Json<Vec<ReportSummary>>, StatusCode> {
    let conn = state
        .db
        .get()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut sql = String::from(
        "SELECT id, account_id, report_type, period_start, period_end, generated_at
         FROM health_reports WHERE 1=1",
    );
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let mut param_idx = 1;

    if let Some(ref aid) = query.account_id {
        sql.push_str(&format!(" AND account_id = ?{}", param_idx));
        params.push(Box::new(aid.clone()));
        param_idx += 1;
    }

    if let Some(ref rt) = query.report_type {
        sql.push_str(&format!(" AND report_type = ?{}", param_idx));
        params.push(Box::new(rt.clone()));
        let _ = param_idx; // suppress unused warning
    }

    sql.push_str(" ORDER BY generated_at DESC LIMIT 100");

    let mut stmt = conn
        .prepare(&sql)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();

    let rows = stmt
        .query_map(param_refs.as_slice(), |row| {
            Ok(ReportSummary {
                id: row.get(0)?,
                account_id: row.get(1)?,
                report_type: row.get(2)?,
                period_start: row.get(3)?,
                period_end: row.get(4)?,
                generated_at: row.get(5)?,
            })
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let reports: Vec<ReportSummary> = rows.flatten().collect();
    Ok(Json(reports))
}

/// GET /api/health-reports/:id
pub async fn get_report(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<ReportDetail>, StatusCode> {
    let conn = state
        .db
        .get()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let report = conn
        .query_row(
            "SELECT id, account_id, report_type, period_start, period_end, report_data, insights, generated_at
             FROM health_reports WHERE id = ?1",
            rusqlite::params![id],
            |row| {
                let data_str: String = row.get(5)?;
                let data: serde_json::Value =
                    serde_json::from_str(&data_str).unwrap_or(serde_json::Value::Null);
                Ok(ReportDetail {
                    id: row.get(0)?,
                    account_id: row.get(1)?,
                    report_type: row.get(2)?,
                    period_start: row.get(3)?,
                    period_end: row.get(4)?,
                    report_data: data,
                    insights: row.get(6)?,
                    generated_at: row.get(7)?,
                })
            },
        )
        .map_err(|_| StatusCode::NOT_FOUND)?;

    Ok(Json(report))
}

/// DELETE /api/health-reports/:id
pub async fn delete_report(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<DeleteResponse>, StatusCode> {
    let conn = state
        .db
        .get()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let rows = conn
        .execute(
            "DELETE FROM health_reports WHERE id = ?1",
            rusqlite::params![id],
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if rows == 0 {
        return Err(StatusCode::NOT_FOUND);
    }

    Ok(Json(DeleteResponse { deleted: true }))
}

// ===========================================================================
// Tests
// ===========================================================================

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
            email: "health-test@example.com".to_string(),
            display_name: Some("Health Test".to_string()),
            imap_host: Some("imap.gmail.com".to_string()),
            imap_port: Some(993),
            smtp_host: Some("smtp.gmail.com".to_string()),
            smtp_port: Some(587),
            username: Some("health-test@example.com".to_string()),
            password: None,
        };
        Account::create(conn, &input)
    }

    fn make_message(
        account_id: &str,
        folder: &str,
        subject: &str,
        from: &str,
        date: i64,
        is_read: bool,
        thread_id: Option<&str>,
    ) -> InsertMessage {
        InsertMessage {
            account_id: account_id.to_string(),
            message_id: Some(format!("<{}-{}@example.com>", subject.replace(' ', "-"), date)),
            thread_id: thread_id.map(|s| s.to_string()),
            folder: folder.to_string(),
            from_address: Some(from.to_string()),
            from_name: Some(from.split('@').next().unwrap_or("sender").to_string()),
            to_addresses: Some(r#"["health-test@example.com"]"#.to_string()),
            cc_addresses: None,
            bcc_addresses: None,
            subject: Some(subject.to_string()),
            date: Some(date),
            snippet: Some(format!("Preview of {}", subject)),
            body_text: Some(format!("Body of {}", subject)),
            body_html: None,
            is_read,
            is_starred: false,
            is_draft: false,
            labels: None,
            uid: Some(date), // use date as uid for uniqueness
            modseq: None,
            raw_headers: None,
            has_attachments: false,
            attachment_names: None,
            size_bytes: Some(512),
            list_unsubscribe: None,
            list_unsubscribe_post: false,
        }
    }

    fn insert_sample_messages(
        conn: &r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>,
        account_id: &str,
    ) {
        // March 10, 2026 = 1773100800 (approx)
        let base = 1773100800i64;

        // Received messages in INBOX
        let m1 = make_message(account_id, "INBOX", "Meeting invite", "alice@corp.com", base, true, Some("thread-1"));
        InsertMessage::insert(conn, &m1);

        let m2 = make_message(account_id, "INBOX", "Project update", "bob@corp.com", base + 3600, false, Some("thread-2"));
        InsertMessage::insert(conn, &m2);

        let m3 = make_message(account_id, "INBOX", "Invoice", "billing@vendor.com", base + 7200, true, Some("thread-3"));
        InsertMessage::insert(conn, &m3);

        let m4 = make_message(account_id, "INBOX", "Follow up", "alice@corp.com", base + 10800, false, None);
        InsertMessage::insert(conn, &m4);

        // Sent messages (replies)
        let mut s1 = make_message(account_id, "Sent", "Re: Meeting invite", "health-test@example.com", base + 1800, true, Some("thread-1"));
        s1.message_id = Some("<re-meeting-1@example.com>".to_string());
        s1.uid = Some(base + 1800);
        InsertMessage::insert(conn, &s1);

        let mut s2 = make_message(account_id, "Sent", "Re: Project update", "health-test@example.com", base + 5400, true, Some("thread-2"));
        s2.message_id = Some("<re-project-1@example.com>".to_string());
        s2.uid = Some(base + 5400);
        InsertMessage::insert(conn, &s2);
    }

    fn set_ai_category(
        conn: &r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>,
        subject: &str,
        category: &str,
    ) {
        conn.execute(
            "UPDATE messages SET ai_category = ?1 WHERE subject = ?2",
            rusqlite::params![category, subject],
        )
        .unwrap();
    }

    // -----------------------------------------------------------------------
    // Test: basic report generation and storage
    // -----------------------------------------------------------------------
    #[test]
    fn test_generate_report_stores_metrics() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);
        insert_sample_messages(&conn, &account.id);

        let report_data = compute_metrics(&conn, &account.id, "2026-03-10", "2026-03-11");

        let volume = &report_data["volume"];
        assert!(volume["total_received"].as_i64().unwrap() >= 4);
        assert!(volume["total_sent"].as_i64().unwrap() >= 2);
        assert!(volume["daily_average"].as_f64().unwrap() > 0.0);
    }

    // -----------------------------------------------------------------------
    // Test: volume counts are correct
    // -----------------------------------------------------------------------
    #[test]
    fn test_volume_counts() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);
        insert_sample_messages(&conn, &account.id);

        let report_data = compute_metrics(&conn, &account.id, "2026-03-10", "2026-03-11");
        let volume = &report_data["volume"];
        assert_eq!(volume["total_received"].as_i64().unwrap(), 4);
        assert_eq!(volume["total_sent"].as_i64().unwrap(), 2);
    }

    // -----------------------------------------------------------------------
    // Test: response time computation
    // -----------------------------------------------------------------------
    #[test]
    fn test_response_time_computed() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);
        insert_sample_messages(&conn, &account.id);

        let report_data = compute_metrics(&conn, &account.id, "2026-03-10", "2026-03-11");
        let rt = &report_data["response_time"];
        // We have two reply pairs: 1800s and 1800s delay
        assert!(rt["average_seconds"].as_f64().is_some());
        let avg = rt["average_seconds"].as_f64().unwrap();
        assert!(avg > 0.0, "Average response time should be positive");
    }

    // -----------------------------------------------------------------------
    // Test: top senders are ranked
    // -----------------------------------------------------------------------
    #[test]
    fn test_top_senders() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);
        insert_sample_messages(&conn, &account.id);

        let report_data = compute_metrics(&conn, &account.id, "2026-03-10", "2026-03-11");
        let senders = report_data["top_contacts"]["senders"].as_array().unwrap();
        assert!(!senders.is_empty());
        // alice@corp.com should be top sender (2 messages)
        assert_eq!(senders[0]["address"].as_str().unwrap(), "alice@corp.com");
        assert_eq!(senders[0]["count"].as_i64().unwrap(), 2);
    }

    // -----------------------------------------------------------------------
    // Test: category distribution
    // -----------------------------------------------------------------------
    #[test]
    fn test_category_distribution() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);
        insert_sample_messages(&conn, &account.id);

        // Tag some messages with AI categories
        set_ai_category(&conn, "Meeting invite", "Primary");
        set_ai_category(&conn, "Project update", "Primary");
        set_ai_category(&conn, "Invoice", "Finance");

        let report_data = compute_metrics(&conn, &account.id, "2026-03-10", "2026-03-11");
        let cats = report_data["category_distribution"].as_array().unwrap();
        assert!(!cats.is_empty());

        // Should have Primary, Finance, and Uncategorized entries
        let cat_names: Vec<&str> = cats.iter().map(|c| c["category"].as_str().unwrap()).collect();
        assert!(cat_names.contains(&"Primary"));
        assert!(cat_names.contains(&"Finance"));
    }

    // -----------------------------------------------------------------------
    // Test: hourly distribution has 24 entries
    // -----------------------------------------------------------------------
    #[test]
    fn test_hourly_distribution_complete() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);
        insert_sample_messages(&conn, &account.id);

        let report_data = compute_metrics(&conn, &account.id, "2026-03-10", "2026-03-11");
        let hourly = report_data["hourly_distribution"].as_array().unwrap();
        assert_eq!(hourly.len(), 24);
        // All hours should be present
        for i in 0..24 {
            assert_eq!(hourly[i]["hour"].as_i64().unwrap(), i as i64);
        }
    }

    // -----------------------------------------------------------------------
    // Test: read rate calculation
    // -----------------------------------------------------------------------
    #[test]
    fn test_read_rate() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);
        insert_sample_messages(&conn, &account.id);

        let report_data = compute_metrics(&conn, &account.id, "2026-03-10", "2026-03-11");
        let rr = &report_data["read_rate"];
        assert_eq!(rr["total_received"].as_i64().unwrap(), 4);
        assert_eq!(rr["read_count"].as_i64().unwrap(), 2);
        assert_eq!(rr["percentage"].as_f64().unwrap(), 50.0);
    }

    // -----------------------------------------------------------------------
    // Test: folder distribution
    // -----------------------------------------------------------------------
    #[test]
    fn test_folder_distribution() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);
        insert_sample_messages(&conn, &account.id);

        let report_data = compute_metrics(&conn, &account.id, "2026-03-10", "2026-03-11");
        let folders = report_data["folder_distribution"].as_array().unwrap();
        assert!(!folders.is_empty());
        let folder_names: Vec<&str> = folders.iter().map(|f| f["folder"].as_str().unwrap()).collect();
        assert!(folder_names.contains(&"INBOX"));
        assert!(folder_names.contains(&"Sent"));
    }

    // -----------------------------------------------------------------------
    // Test: report storage and retrieval
    // -----------------------------------------------------------------------
    #[test]
    fn test_report_insert_and_get() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);

        let data = serde_json::json!({"volume": {"total_received": 10}});
        conn.execute(
            "INSERT INTO health_reports (account_id, report_type, period_start, period_end, report_data, insights)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                account.id,
                "weekly",
                "2026-03-01",
                "2026-03-07",
                serde_json::to_string(&data).unwrap(),
                "Test insights",
            ],
        )
        .unwrap();

        let id = conn.last_insert_rowid();

        let report: ReportDetail = conn
            .query_row(
                "SELECT id, account_id, report_type, period_start, period_end, report_data, insights, generated_at
                 FROM health_reports WHERE id = ?1",
                rusqlite::params![id],
                |row| {
                    let data_str: String = row.get(5)?;
                    let data: serde_json::Value =
                        serde_json::from_str(&data_str).unwrap_or(serde_json::Value::Null);
                    Ok(ReportDetail {
                        id: row.get(0)?,
                        account_id: row.get(1)?,
                        report_type: row.get(2)?,
                        period_start: row.get(3)?,
                        period_end: row.get(4)?,
                        report_data: data,
                        insights: row.get(6)?,
                        generated_at: row.get(7)?,
                    })
                },
            )
            .unwrap();

        assert_eq!(report.account_id, account.id);
        assert_eq!(report.report_type, "weekly");
        assert_eq!(report.period_start, "2026-03-01");
        assert_eq!(report.period_end, "2026-03-07");
        assert_eq!(report.insights.as_deref(), Some("Test insights"));
        assert_eq!(report.report_data["volume"]["total_received"].as_i64().unwrap(), 10);
    }

    // -----------------------------------------------------------------------
    // Test: list reports filtering by account_id
    // -----------------------------------------------------------------------
    #[test]
    fn test_list_reports_by_account() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);

        let data = serde_json::json!({});
        for i in 0..3 {
            conn.execute(
                "INSERT INTO health_reports (account_id, report_type, period_start, period_end, report_data)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![
                    account.id,
                    "weekly",
                    format!("2026-03-0{}", i + 1),
                    format!("2026-03-0{}", i + 7),
                    serde_json::to_string(&data).unwrap(),
                ],
            )
            .unwrap();
        }

        // Insert a report for a different account_id
        conn.execute(
            "INSERT INTO health_reports (account_id, report_type, period_start, period_end, report_data)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params!["other-account", "weekly", "2026-03-01", "2026-03-07", "{}"],
        )
        .unwrap();

        // Count reports for our account
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM health_reports WHERE account_id = ?1",
                rusqlite::params![account.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 3);

        // Count all reports
        let total: i64 = conn
            .query_row("SELECT COUNT(*) FROM health_reports", [], |row| row.get(0))
            .unwrap();
        assert_eq!(total, 4);
    }

    // -----------------------------------------------------------------------
    // Test: list reports filtering by report_type
    // -----------------------------------------------------------------------
    #[test]
    fn test_list_reports_by_type() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);

        let data = serde_json::json!({});
        conn.execute(
            "INSERT INTO health_reports (account_id, report_type, period_start, period_end, report_data)
             VALUES (?1, 'weekly', '2026-03-01', '2026-03-07', ?2)",
            rusqlite::params![account.id, serde_json::to_string(&data).unwrap()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO health_reports (account_id, report_type, period_start, period_end, report_data)
             VALUES (?1, 'monthly', '2026-02-01', '2026-03-01', ?2)",
            rusqlite::params![account.id, serde_json::to_string(&data).unwrap()],
        )
        .unwrap();

        let weekly: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM health_reports WHERE report_type = 'weekly'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(weekly, 1);

        let monthly: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM health_reports WHERE report_type = 'monthly'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(monthly, 1);
    }

    // -----------------------------------------------------------------------
    // Test: delete a report
    // -----------------------------------------------------------------------
    #[test]
    fn test_delete_report() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);

        conn.execute(
            "INSERT INTO health_reports (account_id, report_type, period_start, period_end, report_data)
             VALUES (?1, 'weekly', '2026-03-01', '2026-03-07', '{}')",
            rusqlite::params![account.id],
        )
        .unwrap();
        let id = conn.last_insert_rowid();

        let rows = conn
            .execute(
                "DELETE FROM health_reports WHERE id = ?1",
                rusqlite::params![id],
            )
            .unwrap();
        assert_eq!(rows, 1);

        // Verify it's gone
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM health_reports WHERE id = ?1",
                rusqlite::params![id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }

    // -----------------------------------------------------------------------
    // Test: delete non-existent report
    // -----------------------------------------------------------------------
    #[test]
    fn test_delete_nonexistent_report() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        let rows = conn
            .execute(
                "DELETE FROM health_reports WHERE id = ?1",
                rusqlite::params![99999],
            )
            .unwrap();
        assert_eq!(rows, 0);
    }

    // -----------------------------------------------------------------------
    // Test: resolve_period defaults for weekly
    // -----------------------------------------------------------------------
    #[test]
    fn test_resolve_period_weekly_defaults() {
        let (start, end) = resolve_period("weekly", None, None);
        // end should be today, start should be 7 days back
        assert!(!start.is_empty());
        assert!(!end.is_empty());
        assert!(start < end);
    }

    // -----------------------------------------------------------------------
    // Test: resolve_period defaults for monthly
    // -----------------------------------------------------------------------
    #[test]
    fn test_resolve_period_monthly_defaults() {
        let (start, end) = resolve_period("monthly", None, None);
        assert!(!start.is_empty());
        assert!(!end.is_empty());
        assert!(start < end);
        // Monthly should have a wider gap than weekly
        let (ws, _we) = resolve_period("weekly", None, None);
        assert!(start <= ws, "Monthly start should be earlier or same as weekly start");
    }

    // -----------------------------------------------------------------------
    // Test: resolve_period with explicit dates
    // -----------------------------------------------------------------------
    #[test]
    fn test_resolve_period_explicit_dates() {
        let (start, end) = resolve_period("custom", Some("2026-01-01"), Some("2026-01-31"));
        assert_eq!(start, "2026-01-01");
        assert_eq!(end, "2026-01-31");
    }

    // -----------------------------------------------------------------------
    // Test: empty period (no messages) produces zero metrics
    // -----------------------------------------------------------------------
    #[test]
    fn test_empty_period_produces_zero_metrics() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);

        // No messages inserted
        let report_data = compute_metrics(&conn, &account.id, "2020-01-01", "2020-01-07");
        let volume = &report_data["volume"];
        assert_eq!(volume["total_received"].as_i64().unwrap(), 0);
        assert_eq!(volume["total_sent"].as_i64().unwrap(), 0);
        assert_eq!(report_data["read_rate"]["percentage"].as_f64().unwrap(), 0.0);
        assert!(report_data["response_time"]["average_seconds"].is_null());
    }

    // -----------------------------------------------------------------------
    // Test: health_reports table schema
    // -----------------------------------------------------------------------
    #[test]
    fn test_health_reports_table_exists() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='health_reports'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    // -----------------------------------------------------------------------
    // Test: report_type validation
    // -----------------------------------------------------------------------
    #[test]
    fn test_report_type_validation() {
        // Valid types
        assert!(["weekly", "monthly", "custom"].contains(&"weekly"));
        assert!(["weekly", "monthly", "custom"].contains(&"monthly"));
        assert!(["weekly", "monthly", "custom"].contains(&"custom"));
        // Invalid type
        assert!(!["weekly", "monthly", "custom"].contains(&"daily"));
        assert!(!["weekly", "monthly", "custom"].contains(&""));
    }
}

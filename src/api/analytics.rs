use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::AppState;

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
pub struct OverviewResponse {
    pub total_emails: i64,
    pub unread_count: i64,
    pub sent_count: i64,
    pub avg_daily_volume: f64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct VolumePoint {
    pub period: String,
    pub count: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct VolumeResponse {
    pub data: Vec<VolumePoint>,
    pub period: String,
    pub days: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CategoryStat {
    pub category: String,
    pub count: i64,
    pub percentage: f64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CategoriesResponse {
    pub categories: Vec<CategoryStat>,
    pub total: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ContactStat {
    pub address: String,
    pub count: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TopContactsResponse {
    pub top_senders: Vec<ContactStat>,
    pub top_recipients: Vec<ContactStat>,
    pub days: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HourlyPoint {
    pub hour: i64,
    pub count: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HourlyDistributionResponse {
    pub distribution: Vec<HourlyPoint>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ResponseTimePoint {
    pub period: String,
    pub avg_hours: f64,
    pub count: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ResponseTimesResponse {
    pub data: Vec<ResponseTimePoint>,
    pub period: String,
    pub days: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SnapshotResponse {
    pub saved: bool,
    pub snapshot_date: String,
}

// ---------------------------------------------------------------------------
// Query params
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct VolumeParams {
    pub period: Option<String>,
    pub days: Option<i64>,
    pub account_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct DaysParams {
    pub days: Option<i64>,
    pub account_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ResponseTimeParams {
    pub period: Option<String>,
    pub days: Option<i64>,
    pub account_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AccountParams {
    pub account_id: Option<String>,
}

// ---------------------------------------------------------------------------
// Helper: row mapper that reads (String, i64) for VolumePoint
// ---------------------------------------------------------------------------

fn map_volume_point(row: &rusqlite::Row) -> rusqlite::Result<VolumePoint> {
    Ok(VolumePoint {
        period: row.get(0)?,
        count: row.get(1)?,
    })
}

fn map_contact_stat(row: &rusqlite::Row) -> rusqlite::Result<ContactStat> {
    Ok(ContactStat {
        address: row.get(0)?,
        count: row.get(1)?,
    })
}

fn map_hourly_point(row: &rusqlite::Row) -> rusqlite::Result<HourlyPoint> {
    Ok(HourlyPoint {
        hour: row.get(0)?,
        count: row.get(1)?,
    })
}

fn map_response_time_point(row: &rusqlite::Row) -> rusqlite::Result<ResponseTimePoint> {
    Ok(ResponseTimePoint {
        period: row.get(0)?,
        avg_hours: row.get(1)?,
        count: row.get(2)?,
    })
}

// ---------------------------------------------------------------------------
// GET /api/analytics/overview
// ---------------------------------------------------------------------------

pub async fn overview(
    State(state): State<Arc<AppState>>,
    Query(params): Query<AccountParams>,
) -> Result<Json<OverviewResponse>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let (account_filter, account_param): (&str, Option<String>) = match &params.account_id {
        Some(id) => ("AND account_id = ?1", Some(id.clone())),
        None => ("", None),
    };

    let total_emails: i64 = if let Some(ref aid) = account_param {
        conn.query_row(
            &format!("SELECT COUNT(*) FROM messages WHERE is_deleted = 0 AND is_draft = 0 {account_filter}"),
            rusqlite::params![aid],
            |row| row.get(0),
        )
    } else {
        conn.query_row(
            "SELECT COUNT(*) FROM messages WHERE is_deleted = 0 AND is_draft = 0",
            [],
            |row| row.get(0),
        )
    }
    .unwrap_or(0);

    let unread_count: i64 = if let Some(ref aid) = account_param {
        conn.query_row(
            &format!("SELECT COUNT(*) FROM messages WHERE is_deleted = 0 AND is_draft = 0 AND is_read = 0 {account_filter}"),
            rusqlite::params![aid],
            |row| row.get(0),
        )
    } else {
        conn.query_row(
            "SELECT COUNT(*) FROM messages WHERE is_deleted = 0 AND is_draft = 0 AND is_read = 0",
            [],
            |row| row.get(0),
        )
    }
    .unwrap_or(0);

    let sent_count: i64 = if let Some(ref aid) = account_param {
        conn.query_row(
            &format!("SELECT COUNT(*) FROM messages WHERE is_deleted = 0 AND folder = 'Sent' {account_filter}"),
            rusqlite::params![aid],
            |row| row.get(0),
        )
    } else {
        conn.query_row(
            "SELECT COUNT(*) FROM messages WHERE is_deleted = 0 AND folder = 'Sent'",
            [],
            |row| row.get(0),
        )
    }
    .unwrap_or(0);

    // Average daily volume over last 30 days
    let avg_daily_volume: f64 = if let Some(ref aid) = account_param {
        conn.query_row(
            &format!(
                "SELECT CAST(COUNT(*) AS REAL) / MAX(30.0, CAST(COUNT(DISTINCT date(date, 'unixepoch')) AS REAL))
                 FROM messages
                 WHERE is_deleted = 0 AND is_draft = 0
                   AND date >= unixepoch('now', '-30 days')
                   {account_filter}"
            ),
            rusqlite::params![aid],
            |row| row.get(0),
        )
    } else {
        conn.query_row(
            "SELECT CAST(COUNT(*) AS REAL) / MAX(30.0, CAST(COUNT(DISTINCT date(date, 'unixepoch')) AS REAL))
             FROM messages
             WHERE is_deleted = 0 AND is_draft = 0
               AND date >= unixepoch('now', '-30 days')",
            [],
            |row| row.get(0),
        )
    }
    .unwrap_or(0.0);

    Ok(Json(OverviewResponse {
        total_emails,
        unread_count,
        sent_count,
        avg_daily_volume,
    }))
}

// ---------------------------------------------------------------------------
// GET /api/analytics/volume
// ---------------------------------------------------------------------------

pub async fn volume(
    State(state): State<Arc<AppState>>,
    Query(params): Query<VolumeParams>,
) -> Result<Json<VolumeResponse>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let period = params.period.as_deref().unwrap_or("day");
    let days = params.days.unwrap_or(30).min(365).max(1);

    // Validate period
    let group_expr = match period {
        "day" => "date(date, 'unixepoch')",
        "week" => "strftime('%Y-W%W', date, 'unixepoch')",
        "month" => "strftime('%Y-%m', date, 'unixepoch')",
        _ => return Err(StatusCode::BAD_REQUEST),
    };

    let (account_filter, account_param): (&str, Option<String>) = match &params.account_id {
        Some(id) => ("AND account_id = ?2", Some(id.clone())),
        None => ("", None),
    };

    let sql = format!(
        "SELECT {group_expr} as period, COUNT(*) as cnt
         FROM messages
         WHERE is_deleted = 0 AND is_draft = 0
           AND date >= unixepoch('now', '-' || ?1 || ' days')
           {account_filter}
         GROUP BY period
         ORDER BY period ASC"
    );

    let mut stmt = conn.prepare(&sql).map_err(|e| {
        tracing::error!("Analytics volume query error: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let data: Vec<VolumePoint> = if let Some(ref aid) = account_param {
        stmt.query_map(rusqlite::params![days, aid], map_volume_point)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .filter_map(|r| r.ok())
            .collect()
    } else {
        stmt.query_map(rusqlite::params![days], map_volume_point)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .filter_map(|r| r.ok())
            .collect()
    };

    Ok(Json(VolumeResponse {
        data,
        period: period.to_string(),
        days,
    }))
}

// ---------------------------------------------------------------------------
// GET /api/analytics/categories
// ---------------------------------------------------------------------------

pub async fn categories(
    State(state): State<Arc<AppState>>,
    Query(params): Query<AccountParams>,
) -> Result<Json<CategoriesResponse>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let (account_filter, account_param): (&str, Option<String>) = match &params.account_id {
        Some(id) => ("AND account_id = ?1", Some(id.clone())),
        None => ("", None),
    };

    let total: i64 = if let Some(ref aid) = account_param {
        conn.query_row(
            &format!(
                "SELECT COUNT(*) FROM messages WHERE is_deleted = 0 AND is_draft = 0 AND ai_category IS NOT NULL {account_filter}"
            ),
            rusqlite::params![aid],
            |row| row.get(0),
        )
    } else {
        conn.query_row(
            "SELECT COUNT(*) FROM messages WHERE is_deleted = 0 AND is_draft = 0 AND ai_category IS NOT NULL",
            [],
            |row| row.get(0),
        )
    }
    .unwrap_or(0);

    let sql = format!(
        "SELECT COALESCE(ai_category, 'Uncategorized') as cat, COUNT(*) as cnt
         FROM messages
         WHERE is_deleted = 0 AND is_draft = 0 AND ai_category IS NOT NULL
           {account_filter}
         GROUP BY cat
         ORDER BY cnt DESC"
    );

    let mut stmt = conn.prepare(&sql).map_err(|e| {
        tracing::error!("Analytics categories query error: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // Collect raw rows first, then compute percentages
    let raw_rows: Vec<(String, i64)> = if let Some(ref aid) = account_param {
        stmt.query_map(rusqlite::params![aid], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .filter_map(|r| r.ok())
        .collect()
    } else {
        stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .filter_map(|r| r.ok())
        .collect()
    };

    let cats: Vec<CategoryStat> = raw_rows
        .into_iter()
        .map(|(category, count)| CategoryStat {
            category,
            count,
            percentage: if total > 0 {
                (count as f64 / total as f64) * 100.0
            } else {
                0.0
            },
        })
        .collect();

    Ok(Json(CategoriesResponse {
        categories: cats,
        total,
    }))
}

// ---------------------------------------------------------------------------
// GET /api/analytics/top-contacts
// ---------------------------------------------------------------------------

pub async fn top_contacts(
    State(state): State<Arc<AppState>>,
    Query(params): Query<DaysParams>,
) -> Result<Json<TopContactsResponse>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let days = params.days.unwrap_or(30).min(365).max(1);

    let (account_filter, account_param): (&str, Option<String>) = match &params.account_id {
        Some(id) => ("AND account_id = ?2", Some(id.clone())),
        None => ("", None),
    };

    // Top senders (from_address on received mail)
    let senders_sql = format!(
        "SELECT from_address, COUNT(*) as cnt
         FROM messages
         WHERE is_deleted = 0 AND is_draft = 0
           AND from_address IS NOT NULL
           AND folder != 'Sent'
           AND date >= unixepoch('now', '-' || ?1 || ' days')
           {account_filter}
         GROUP BY from_address
         ORDER BY cnt DESC
         LIMIT 10"
    );

    let mut stmt = conn.prepare(&senders_sql).map_err(|e| {
        tracing::error!("Analytics top-contacts senders query error: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let top_senders: Vec<ContactStat> = if let Some(ref aid) = account_param {
        stmt.query_map(rusqlite::params![days, aid], map_contact_stat)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .filter_map(|r| r.ok())
            .collect()
    } else {
        stmt.query_map(rusqlite::params![days], map_contact_stat)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .filter_map(|r| r.ok())
            .collect()
    };

    // Top recipients (to_addresses on sent mail — parse first address from JSON array)
    let recipients_sql = format!(
        "SELECT json_extract(to_addresses, '$[0]') as recipient, COUNT(*) as cnt
         FROM messages
         WHERE is_deleted = 0
           AND folder = 'Sent'
           AND to_addresses IS NOT NULL
           AND date >= unixepoch('now', '-' || ?1 || ' days')
           {account_filter}
         GROUP BY recipient
         HAVING recipient IS NOT NULL
         ORDER BY cnt DESC
         LIMIT 10"
    );

    let mut stmt2 = conn.prepare(&recipients_sql).map_err(|e| {
        tracing::error!("Analytics top-contacts recipients query error: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let top_recipients: Vec<ContactStat> = if let Some(ref aid) = account_param {
        stmt2.query_map(rusqlite::params![days, aid], map_contact_stat)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .filter_map(|r| r.ok())
            .collect()
    } else {
        stmt2.query_map(rusqlite::params![days], map_contact_stat)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .filter_map(|r| r.ok())
            .collect()
    };

    Ok(Json(TopContactsResponse {
        top_senders,
        top_recipients,
        days,
    }))
}

// ---------------------------------------------------------------------------
// GET /api/analytics/hourly-distribution
// ---------------------------------------------------------------------------

pub async fn hourly_distribution(
    State(state): State<Arc<AppState>>,
    Query(params): Query<AccountParams>,
) -> Result<Json<HourlyDistributionResponse>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let (account_filter, account_param): (&str, Option<String>) = match &params.account_id {
        Some(id) => ("AND account_id = ?1", Some(id.clone())),
        None => ("", None),
    };

    let sql = format!(
        "SELECT CAST(strftime('%H', date, 'unixepoch') AS INTEGER) as hour, COUNT(*) as cnt
         FROM messages
         WHERE is_deleted = 0 AND is_draft = 0
           AND date IS NOT NULL
           {account_filter}
         GROUP BY hour
         ORDER BY hour ASC"
    );

    let mut stmt = conn.prepare(&sql).map_err(|e| {
        tracing::error!("Analytics hourly distribution query error: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let rows: Vec<HourlyPoint> = if let Some(ref aid) = account_param {
        stmt.query_map(rusqlite::params![aid], map_hourly_point)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .filter_map(|r| r.ok())
            .collect()
    } else {
        stmt.query_map([], map_hourly_point)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .filter_map(|r| r.ok())
            .collect()
    };

    // Fill in missing hours with zero counts
    let mut distribution = Vec::with_capacity(24);
    for h in 0..24 {
        let count = rows.iter().find(|p| p.hour == h).map_or(0, |p| p.count);
        distribution.push(HourlyPoint { hour: h, count });
    }

    Ok(Json(HourlyDistributionResponse { distribution }))
}

// ---------------------------------------------------------------------------
// GET /api/analytics/response-times
// ---------------------------------------------------------------------------

pub async fn response_times(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ResponseTimeParams>,
) -> Result<Json<ResponseTimesResponse>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let period = params.period.as_deref().unwrap_or("day");
    let days = params.days.unwrap_or(30).min(365).max(1);

    let group_expr = match period {
        "day" => "date(sent.date, 'unixepoch')",
        "week" => "strftime('%Y-W%W', sent.date, 'unixepoch')",
        _ => return Err(StatusCode::BAD_REQUEST),
    };

    let (account_filter, account_param): (&str, Option<String>) = match &params.account_id {
        Some(id) => ("AND sent.account_id = ?2", Some(id.clone())),
        None => ("", None),
    };

    // Find reply pairs: a Sent message in the same thread as an INBOX message,
    // where the Sent message date > INBOX message date. Take the latest INBOX
    // message before each Sent reply to compute response time.
    let sql = format!(
        "SELECT {group_expr} as period,
                AVG(CAST(sent.date - inbox.date AS REAL) / 3600.0) as avg_hours,
                COUNT(*) as cnt
         FROM messages sent
         INNER JOIN (
             SELECT thread_id, MAX(date) as date
             FROM messages
             WHERE folder = 'INBOX' AND is_deleted = 0 AND thread_id IS NOT NULL
             GROUP BY thread_id
         ) inbox ON sent.thread_id = inbox.thread_id
         WHERE sent.folder = 'Sent'
           AND sent.is_deleted = 0
           AND sent.date > inbox.date
           AND sent.thread_id IS NOT NULL
           AND sent.date >= unixepoch('now', '-' || ?1 || ' days')
           {account_filter}
         GROUP BY period
         ORDER BY period ASC"
    );

    let mut stmt = conn.prepare(&sql).map_err(|e| {
        tracing::error!("Analytics response-times query error: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let data: Vec<ResponseTimePoint> = if let Some(ref aid) = account_param {
        stmt.query_map(rusqlite::params![days, aid], map_response_time_point)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .filter_map(|r| r.ok())
            .collect()
    } else {
        stmt.query_map(rusqlite::params![days], map_response_time_point)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .filter_map(|r| r.ok())
            .collect()
    };

    Ok(Json(ResponseTimesResponse {
        data,
        period: period.to_string(),
        days,
    }))
}

// ---------------------------------------------------------------------------
// POST /api/analytics/snapshot
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct SnapshotRequest {
    pub account_id: Option<String>,
}

pub async fn save_snapshot(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SnapshotRequest>,
) -> Result<Json<SnapshotResponse>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let account_id = req.account_id.unwrap_or_else(|| "all".to_string());

    let (account_filter, account_param): (&str, Option<String>) = if account_id != "all" {
        ("AND account_id = ?1", Some(account_id.clone()))
    } else {
        ("", None)
    };

    // Compute current metrics
    let total: i64 = if let Some(ref aid) = account_param {
        conn.query_row(
            &format!("SELECT COUNT(*) FROM messages WHERE is_deleted = 0 AND is_draft = 0 {account_filter}"),
            rusqlite::params![aid],
            |row| row.get(0),
        )
    } else {
        conn.query_row(
            "SELECT COUNT(*) FROM messages WHERE is_deleted = 0 AND is_draft = 0",
            [],
            |row| row.get(0),
        )
    }
    .unwrap_or(0);

    let unread: i64 = if let Some(ref aid) = account_param {
        conn.query_row(
            &format!("SELECT COUNT(*) FROM messages WHERE is_deleted = 0 AND is_draft = 0 AND is_read = 0 {account_filter}"),
            rusqlite::params![aid],
            |row| row.get(0),
        )
    } else {
        conn.query_row(
            "SELECT COUNT(*) FROM messages WHERE is_deleted = 0 AND is_draft = 0 AND is_read = 0",
            [],
            |row| row.get(0),
        )
    }
    .unwrap_or(0);

    let sent: i64 = if let Some(ref aid) = account_param {
        conn.query_row(
            &format!("SELECT COUNT(*) FROM messages WHERE is_deleted = 0 AND folder = 'Sent' {account_filter}"),
            rusqlite::params![aid],
            |row| row.get(0),
        )
    } else {
        conn.query_row(
            "SELECT COUNT(*) FROM messages WHERE is_deleted = 0 AND folder = 'Sent'",
            [],
            |row| row.get(0),
        )
    }
    .unwrap_or(0);

    let snapshot_date: String = conn
        .query_row("SELECT date('now')", [], |row| row.get(0))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let metrics = serde_json::json!({
        "total_emails": total,
        "unread_count": unread,
        "sent_count": sent,
    });

    conn.execute(
        "INSERT OR REPLACE INTO analytics_snapshots (account_id, snapshot_date, metrics)
         VALUES (?1, ?2, ?3)",
        rusqlite::params![account_id, snapshot_date, metrics.to_string()],
    )
    .map_err(|e| {
        tracing::error!("Analytics snapshot save error: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(SnapshotResponse {
        saved: true,
        snapshot_date,
    }))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::memories::MemoriesClient;
    use crate::ai::provider::ProviderPool;
    use crate::config::Config;
    use crate::db::create_test_pool;
    use crate::ws::hub::WsHub;

    fn test_config() -> Config {
        Config {
            port: 3000,
            database_url: ":memory:".to_string(),
            ollama_url: "http://localhost:11434".to_string(),
            memories_url: "http://localhost:8900".to_string(),
            memories_api_key: None,
            anthropic_api_key: None,
            openai_api_key: None,
            gmail_client_id: None,
            gmail_client_secret: None,
            outlook_client_id: None,
            outlook_client_secret: None,
            public_url: "http://localhost:3000".to_string(),
            job_poll_interval_ms: 2000,
            job_max_concurrency: 4,
            job_cleanup_days: 7,
        }
    }

    fn test_state() -> Arc<AppState> {
        let pool = create_test_pool();
        Arc::new(AppState {
            db: pool,
            config: test_config(),
            ws_hub: WsHub::new(),
            providers: ProviderPool::new(vec![]),
            memories: MemoriesClient::new("http://localhost:8900", None),
            session_token: "test-token".to_string(),
        })
    }

    fn seed_account(state: &Arc<AppState>, account_id: &str, email: &str) {
        let conn = state.db.get().unwrap();
        conn.execute(
            "INSERT INTO accounts (id, provider, email) VALUES (?1, 'imap', ?2)",
            rusqlite::params![account_id, email],
        )
        .unwrap();
    }

    fn seed_message(
        state: &Arc<AppState>,
        id: &str,
        account_id: &str,
        folder: &str,
        from: &str,
        to_json: Option<&str>,
        subject: &str,
        date: i64,
        is_read: bool,
        thread_id: Option<&str>,
        ai_category: Option<&str>,
    ) {
        let conn = state.db.get().unwrap();
        conn.execute(
            "INSERT INTO messages (id, account_id, folder, from_address, to_addresses, subject, date, is_read, thread_id, ai_category)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![id, account_id, folder, from, to_json, subject, date, is_read, thread_id, ai_category],
        )
        .unwrap();
    }

    fn now_ts() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
    }

    // -----------------------------------------------------------------------
    // 1. Overview — empty database
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn test_overview_empty() {
        let state = test_state();
        let result = overview(
            State(state.clone()),
            Query(AccountParams { account_id: None }),
        )
        .await
        .unwrap();
        let resp = result.0;
        assert_eq!(resp.total_emails, 0);
        assert_eq!(resp.unread_count, 0);
        assert_eq!(resp.sent_count, 0);
        assert_eq!(resp.avg_daily_volume, 0.0);
    }

    // -----------------------------------------------------------------------
    // 2. Overview — with data
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn test_overview_with_data() {
        let state = test_state();
        seed_account(&state, "acc1", "user@test.com");
        let now = now_ts();

        seed_message(&state, "m1", "acc1", "INBOX", "alice@x.com", None, "Hello", now - 86400, false, None, None);
        seed_message(&state, "m2", "acc1", "INBOX", "bob@x.com", None, "Hi", now - 172800, true, None, None);
        seed_message(&state, "m3", "acc1", "Sent", "user@test.com", Some("[\"alice@x.com\"]"), "Re: Hello", now - 43200, true, None, None);

        let result = overview(
            State(state.clone()),
            Query(AccountParams { account_id: None }),
        )
        .await
        .unwrap();
        let resp = result.0;
        assert_eq!(resp.total_emails, 3);
        assert_eq!(resp.unread_count, 1);
        assert_eq!(resp.sent_count, 1);
        assert!(resp.avg_daily_volume > 0.0);
    }

    // -----------------------------------------------------------------------
    // 3. Overview — filtered by account_id
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn test_overview_account_filter() {
        let state = test_state();
        seed_account(&state, "acc1", "user1@test.com");
        seed_account(&state, "acc2", "user2@test.com");
        let now = now_ts();

        seed_message(&state, "m1", "acc1", "INBOX", "alice@x.com", None, "A", now, false, None, None);
        seed_message(&state, "m2", "acc2", "INBOX", "bob@x.com", None, "B", now, false, None, None);
        seed_message(&state, "m3", "acc2", "INBOX", "carol@x.com", None, "C", now, true, None, None);

        let result = overview(
            State(state.clone()),
            Query(AccountParams {
                account_id: Some("acc2".to_string()),
            }),
        )
        .await
        .unwrap();
        let resp = result.0;
        assert_eq!(resp.total_emails, 2);
        assert_eq!(resp.unread_count, 1);
    }

    // -----------------------------------------------------------------------
    // 4. Volume — empty
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn test_volume_empty() {
        let state = test_state();
        let result = volume(
            State(state.clone()),
            Query(VolumeParams {
                period: Some("day".to_string()),
                days: Some(30),
                account_id: None,
            }),
        )
        .await
        .unwrap();
        let resp = result.0;
        assert!(resp.data.is_empty());
        assert_eq!(resp.period, "day");
        assert_eq!(resp.days, 30);
    }

    // -----------------------------------------------------------------------
    // 5. Volume — daily with data
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn test_volume_daily() {
        let state = test_state();
        seed_account(&state, "acc1", "user@test.com");
        let now = now_ts();

        // Two messages on same day, one on different day
        seed_message(&state, "m1", "acc1", "INBOX", "a@x.com", None, "A", now - 100, false, None, None);
        seed_message(&state, "m2", "acc1", "INBOX", "b@x.com", None, "B", now - 200, false, None, None);
        seed_message(&state, "m3", "acc1", "INBOX", "c@x.com", None, "C", now - 86400 - 100, false, None, None);

        let result = volume(
            State(state.clone()),
            Query(VolumeParams {
                period: Some("day".to_string()),
                days: Some(30),
                account_id: None,
            }),
        )
        .await
        .unwrap();
        let resp = result.0;
        assert!(!resp.data.is_empty());
        let total: i64 = resp.data.iter().map(|p| p.count).sum();
        assert_eq!(total, 3);
    }

    // -----------------------------------------------------------------------
    // 6. Volume — invalid period returns 400
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn test_volume_invalid_period() {
        let state = test_state();
        let result = volume(
            State(state.clone()),
            Query(VolumeParams {
                period: Some("year".to_string()),
                days: Some(30),
                account_id: None,
            }),
        )
        .await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), StatusCode::BAD_REQUEST);
    }

    // -----------------------------------------------------------------------
    // 7. Volume — weekly aggregation
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn test_volume_weekly() {
        let state = test_state();
        seed_account(&state, "acc1", "user@test.com");
        let now = now_ts();

        seed_message(&state, "m1", "acc1", "INBOX", "a@x.com", None, "A", now - 100, false, None, None);

        let result = volume(
            State(state.clone()),
            Query(VolumeParams {
                period: Some("week".to_string()),
                days: Some(30),
                account_id: None,
            }),
        )
        .await
        .unwrap();
        let resp = result.0;
        assert_eq!(resp.period, "week");
        assert!(!resp.data.is_empty());
        assert!(resp.data[0].period.contains("-W"));
    }

    // -----------------------------------------------------------------------
    // 8. Categories — empty
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn test_categories_empty() {
        let state = test_state();
        let result = categories(
            State(state.clone()),
            Query(AccountParams { account_id: None }),
        )
        .await
        .unwrap();
        let resp = result.0;
        assert!(resp.categories.is_empty());
        assert_eq!(resp.total, 0);
    }

    // -----------------------------------------------------------------------
    // 9. Categories — with data
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn test_categories_with_data() {
        let state = test_state();
        seed_account(&state, "acc1", "user@test.com");
        let now = now_ts();

        seed_message(&state, "m1", "acc1", "INBOX", "a@x.com", None, "A", now, false, None, Some("Primary"));
        seed_message(&state, "m2", "acc1", "INBOX", "b@x.com", None, "B", now, false, None, Some("Primary"));
        seed_message(&state, "m3", "acc1", "INBOX", "c@x.com", None, "C", now, false, None, Some("Updates"));

        let result = categories(
            State(state.clone()),
            Query(AccountParams { account_id: None }),
        )
        .await
        .unwrap();
        let resp = result.0;
        assert_eq!(resp.total, 3);
        assert_eq!(resp.categories.len(), 2);
        assert_eq!(resp.categories[0].category, "Primary");
        assert_eq!(resp.categories[0].count, 2);
        let pct_sum: f64 = resp.categories.iter().map(|c| c.percentage).sum();
        assert!((pct_sum - 100.0).abs() < 0.01);
    }

    // -----------------------------------------------------------------------
    // 10. Top contacts — empty
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn test_top_contacts_empty() {
        let state = test_state();
        let result = top_contacts(
            State(state.clone()),
            Query(DaysParams {
                days: Some(30),
                account_id: None,
            }),
        )
        .await
        .unwrap();
        let resp = result.0;
        assert!(resp.top_senders.is_empty());
        assert!(resp.top_recipients.is_empty());
        assert_eq!(resp.days, 30);
    }

    // -----------------------------------------------------------------------
    // 11. Top contacts — with data
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn test_top_contacts_with_data() {
        let state = test_state();
        seed_account(&state, "acc1", "user@test.com");
        let now = now_ts();

        seed_message(&state, "m1", "acc1", "INBOX", "alice@x.com", None, "A", now - 100, false, None, None);
        seed_message(&state, "m2", "acc1", "INBOX", "alice@x.com", None, "B", now - 200, false, None, None);
        seed_message(&state, "m3", "acc1", "INBOX", "bob@x.com", None, "C", now - 300, false, None, None);
        seed_message(&state, "m4", "acc1", "Sent", "user@test.com", Some("[\"carol@x.com\"]"), "D", now - 400, true, None, None);
        seed_message(&state, "m5", "acc1", "Sent", "user@test.com", Some("[\"carol@x.com\"]"), "E", now - 500, true, None, None);

        let result = top_contacts(
            State(state.clone()),
            Query(DaysParams {
                days: Some(30),
                account_id: None,
            }),
        )
        .await
        .unwrap();
        let resp = result.0;
        assert!(!resp.top_senders.is_empty());
        assert_eq!(resp.top_senders[0].address, "alice@x.com");
        assert_eq!(resp.top_senders[0].count, 2);

        assert!(!resp.top_recipients.is_empty());
        assert_eq!(resp.top_recipients[0].address, "carol@x.com");
        assert_eq!(resp.top_recipients[0].count, 2);
    }

    // -----------------------------------------------------------------------
    // 12. Hourly distribution — empty
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn test_hourly_empty() {
        let state = test_state();
        let result = hourly_distribution(
            State(state.clone()),
            Query(AccountParams { account_id: None }),
        )
        .await
        .unwrap();
        let resp = result.0;
        assert_eq!(resp.distribution.len(), 24);
        assert!(resp.distribution.iter().all(|p| p.count == 0));
    }

    // -----------------------------------------------------------------------
    // 13. Hourly distribution — with data
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn test_hourly_with_data() {
        let state = test_state();
        seed_account(&state, "acc1", "user@test.com");

        // 2025-01-15 10:00:00 UTC = 1736935200
        let ts_10am = 1736935200_i64;
        // 2025-01-15 14:00:00 UTC = 1736949600
        let ts_2pm = 1736949600_i64;

        seed_message(&state, "m1", "acc1", "INBOX", "a@x.com", None, "A", ts_10am, false, None, None);
        seed_message(&state, "m2", "acc1", "INBOX", "b@x.com", None, "B", ts_10am + 60, false, None, None);
        seed_message(&state, "m3", "acc1", "INBOX", "c@x.com", None, "C", ts_2pm, false, None, None);

        let result = hourly_distribution(
            State(state.clone()),
            Query(AccountParams { account_id: None }),
        )
        .await
        .unwrap();
        let resp = result.0;
        assert_eq!(resp.distribution.len(), 24);
        assert_eq!(resp.distribution[10].count, 2); // hour 10
        assert_eq!(resp.distribution[14].count, 1); // hour 14
        assert_eq!(resp.distribution[0].count, 0);  // hour 0
    }

    // -----------------------------------------------------------------------
    // 14. Response times — empty
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn test_response_times_empty() {
        let state = test_state();
        let result = response_times(
            State(state.clone()),
            Query(ResponseTimeParams {
                period: Some("day".to_string()),
                days: Some(30),
                account_id: None,
            }),
        )
        .await
        .unwrap();
        let resp = result.0;
        assert!(resp.data.is_empty());
        assert_eq!(resp.period, "day");
    }

    // -----------------------------------------------------------------------
    // 15. Response times — with reply pairs
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn test_response_times_with_replies() {
        let state = test_state();
        seed_account(&state, "acc1", "user@test.com");
        let now = now_ts();

        let thread = "thread-001";
        seed_message(&state, "m1", "acc1", "INBOX", "alice@x.com", None, "Question", now - 7200, true, Some(thread), None);
        seed_message(&state, "m2", "acc1", "Sent", "user@test.com", Some("[\"alice@x.com\"]"), "Re: Question", now - 3600, true, Some(thread), None);

        let result = response_times(
            State(state.clone()),
            Query(ResponseTimeParams {
                period: Some("day".to_string()),
                days: Some(30),
                account_id: None,
            }),
        )
        .await
        .unwrap();
        let resp = result.0;
        assert!(!resp.data.is_empty());
        assert!((resp.data[0].avg_hours - 1.0).abs() < 0.1);
        assert_eq!(resp.data[0].count, 1);
    }

    // -----------------------------------------------------------------------
    // 16. Response times — invalid period
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn test_response_times_invalid_period() {
        let state = test_state();
        let result = response_times(
            State(state.clone()),
            Query(ResponseTimeParams {
                period: Some("month".to_string()),
                days: Some(30),
                account_id: None,
            }),
        )
        .await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), StatusCode::BAD_REQUEST);
    }

    // -----------------------------------------------------------------------
    // 17. Snapshot — save and verify
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn test_snapshot_save() {
        let state = test_state();
        seed_account(&state, "acc1", "user@test.com");
        let now = now_ts();

        seed_message(&state, "m1", "acc1", "INBOX", "a@x.com", None, "A", now, false, None, None);
        seed_message(&state, "m2", "acc1", "Sent", "user@test.com", None, "B", now, true, None, None);

        let result = save_snapshot(
            State(state.clone()),
            Json(SnapshotRequest {
                account_id: Some("acc1".to_string()),
            }),
        )
        .await
        .unwrap();
        let resp = result.0;
        assert!(resp.saved);
        assert!(!resp.snapshot_date.is_empty());

        let conn = state.db.get().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM analytics_snapshots WHERE account_id = 'acc1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);

        let metrics_str: String = conn
            .query_row(
                "SELECT metrics FROM analytics_snapshots WHERE account_id = 'acc1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let metrics: serde_json::Value = serde_json::from_str(&metrics_str).unwrap();
        assert_eq!(metrics["total_emails"], 2);
        assert_eq!(metrics["unread_count"], 1);
        assert_eq!(metrics["sent_count"], 1);
    }

    // -----------------------------------------------------------------------
    // 18. Snapshot — idempotent (upsert on same day)
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn test_snapshot_upsert() {
        let state = test_state();
        seed_account(&state, "acc1", "user@test.com");

        let _ = save_snapshot(
            State(state.clone()),
            Json(SnapshotRequest {
                account_id: Some("acc1".to_string()),
            }),
        )
        .await
        .unwrap();

        let _ = save_snapshot(
            State(state.clone()),
            Json(SnapshotRequest {
                account_id: Some("acc1".to_string()),
            }),
        )
        .await
        .unwrap();

        let conn = state.db.get().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM analytics_snapshots WHERE account_id = 'acc1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    // -----------------------------------------------------------------------
    // 19. Volume — monthly aggregation
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn test_volume_monthly() {
        let state = test_state();
        seed_account(&state, "acc1", "user@test.com");
        let now = now_ts();

        seed_message(&state, "m1", "acc1", "INBOX", "a@x.com", None, "A", now - 100, false, None, None);

        let result = volume(
            State(state.clone()),
            Query(VolumeParams {
                period: Some("month".to_string()),
                days: Some(60),
                account_id: None,
            }),
        )
        .await
        .unwrap();
        let resp = result.0;
        assert_eq!(resp.period, "month");
        assert!(!resp.data.is_empty());
        assert_eq!(resp.data[0].period.len(), 7); // "YYYY-MM"
    }

    // -----------------------------------------------------------------------
    // 20. Drafts and deleted messages are excluded
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn test_overview_excludes_drafts_and_deleted() {
        let state = test_state();
        seed_account(&state, "acc1", "user@test.com");
        let now = now_ts();

        seed_message(&state, "m1", "acc1", "INBOX", "a@x.com", None, "A", now, false, None, None);

        {
            let conn = state.db.get().unwrap();
            conn.execute(
                "INSERT INTO messages (id, account_id, folder, subject, date, is_draft) VALUES ('m2', 'acc1', 'Drafts', 'Draft', ?1, 1)",
                rusqlite::params![now],
            )
            .unwrap();
        }

        {
            let conn = state.db.get().unwrap();
            conn.execute(
                "INSERT INTO messages (id, account_id, folder, subject, date, is_deleted) VALUES ('m3', 'acc1', 'INBOX', 'Deleted', ?1, 1)",
                rusqlite::params![now],
            )
            .unwrap();
        }

        let result = overview(
            State(state.clone()),
            Query(AccountParams { account_id: None }),
        )
        .await
        .unwrap();
        let resp = result.0;
        assert_eq!(resp.total_emails, 1);
    }

    // -----------------------------------------------------------------------
    // 21. Top contacts — days filter limits old messages
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn test_top_contacts_days_filter() {
        let state = test_state();
        seed_account(&state, "acc1", "user@test.com");
        let now = now_ts();

        seed_message(&state, "m1", "acc1", "INBOX", "alice@x.com", None, "Recent", now - 100, false, None, None);
        seed_message(&state, "m2", "acc1", "INBOX", "bob@x.com", None, "Old", now - 86400 * 60, false, None, None);

        let result = top_contacts(
            State(state.clone()),
            Query(DaysParams {
                days: Some(7),
                account_id: None,
            }),
        )
        .await
        .unwrap();
        let resp = result.0;
        assert_eq!(resp.days, 7);
        assert_eq!(resp.top_senders.len(), 1);
        assert_eq!(resp.top_senders[0].address, "alice@x.com");
    }

    // -----------------------------------------------------------------------
    // 22. Snapshot with "all" account_id
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn test_snapshot_all_accounts() {
        let state = test_state();
        seed_account(&state, "acc1", "user1@test.com");
        seed_account(&state, "acc2", "user2@test.com");
        let now = now_ts();

        seed_message(&state, "m1", "acc1", "INBOX", "a@x.com", None, "A", now, false, None, None);
        seed_message(&state, "m2", "acc2", "INBOX", "b@x.com", None, "B", now, false, None, None);

        let result = save_snapshot(
            State(state.clone()),
            Json(SnapshotRequest { account_id: None }),
        )
        .await
        .unwrap();
        let resp = result.0;
        assert!(resp.saved);

        let conn = state.db.get().unwrap();
        let stored_aid: String = conn
            .query_row(
                "SELECT account_id FROM analytics_snapshots LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(stored_aid, "all");

        let metrics_str: String = conn
            .query_row(
                "SELECT metrics FROM analytics_snapshots LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let metrics: serde_json::Value = serde_json::from_str(&metrics_str).unwrap();
        assert_eq!(metrics["total_emails"], 2);
    }
}

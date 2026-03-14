use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, LazyLock};

use crate::AppState;

// Pre-compiled regex patterns for data extraction (compiled once, reused)
static RE_DOLLAR_AMOUNT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\$\d{1,3}(?:,\d{3})*(?:\.\d{2})?").unwrap());
static RE_DATE_MDY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b(0[1-9]|1[0-2])/(0[1-9]|[12]\d|3[01])/(\d{4})\b").unwrap());
static RE_DATE_ISO: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b(\d{4})-(0[1-9]|1[0-2])-(0[1-9]|[12]\d|3[01])\b").unwrap());
static RE_EMAIL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}\b").unwrap());
static RE_URL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"https?://[^\s<>"']+"#).unwrap());
static RE_UPS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b1Z[0-9A-Z]{16}\b").unwrap());
static RE_FEDEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b\d{12,15}\b").unwrap());
static RE_USPS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b\d{20,22}\b").unwrap());

// --- Types ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedDatum {
    pub id: i64,
    pub message_id: String,
    pub account_id: i64,
    pub data_type: String,
    pub data_key: String,
    pub data_value: String,
    pub confidence: f64,
    pub source: String,
    pub extracted_at: String,
}

#[derive(Debug, Serialize)]
pub struct ExtractResponse {
    pub message_id: String,
    pub extracted: Vec<ExtractedDatum>,
    pub count: usize,
}

#[derive(Debug, Deserialize)]
pub struct ListParams {
    #[serde(rename = "type")]
    pub data_type: Option<String>,
    pub since: Option<String>,
    pub message_id: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct ListResponse {
    pub data: Vec<ExtractedDatum>,
    pub total: i64,
}

#[derive(Debug, Serialize)]
pub struct TypeSummary {
    pub data_type: String,
    pub count: i64,
}

#[derive(Debug, Serialize)]
pub struct SummaryResponse {
    pub total: i64,
    pub by_type: Vec<TypeSummary>,
}

#[derive(Debug, Serialize)]
pub struct DeleteResponse {
    pub deleted: bool,
}

/// Internal struct for AI-parsed extraction items.
#[derive(Debug, Deserialize)]
struct AiExtraction {
    #[serde(rename = "type")]
    data_type: String,
    key: String,
    value: String,
    confidence: Option<f64>,
}

// --- Valid data types ---

const VALID_DATA_TYPES: &[&str] = &[
    "date", "amount", "address", "tracking", "flight", "order", "contact", "link",
];

fn is_valid_data_type(t: &str) -> bool {
    VALID_DATA_TYPES.contains(&t)
}

// --- Regex extraction ---

struct RegexExtraction {
    data_type: String,
    data_key: String,
    data_value: String,
    confidence: f64,
}

fn extract_with_regex(text: &str) -> Vec<RegexExtraction> {
    let mut results = Vec::new();

    // Dollar amounts: $X.XX or $X,XXX.XX
    for m in RE_DOLLAR_AMOUNT.find_iter(text) {
        results.push(RegexExtraction {
            data_type: "amount".to_string(),
            data_key: "dollar_amount".to_string(),
            data_value: m.as_str().to_string(),
            confidence: 0.9,
        });
    }

    // Dates: MM/DD/YYYY
    for m in RE_DATE_MDY.find_iter(text) {
        results.push(RegexExtraction {
            data_type: "date".to_string(),
            data_key: "date_mdy".to_string(),
            data_value: m.as_str().to_string(),
            confidence: 0.85,
        });
    }

    // Dates: YYYY-MM-DD
    for m in RE_DATE_ISO.find_iter(text) {
        results.push(RegexExtraction {
            data_type: "date".to_string(),
            data_key: "date_iso".to_string(),
            data_value: m.as_str().to_string(),
            confidence: 0.9,
        });
    }

    // Email addresses
    for m in RE_EMAIL.find_iter(text) {
        results.push(RegexExtraction {
            data_type: "contact".to_string(),
            data_key: "email_address".to_string(),
            data_value: m.as_str().to_string(),
            confidence: 0.95,
        });
    }

    // URLs (http/https)
    for m in RE_URL.find_iter(text) {
        results.push(RegexExtraction {
            data_type: "link".to_string(),
            data_key: "url".to_string(),
            data_value: m.as_str().to_string(),
            confidence: 0.95,
        });
    }

    // Tracking numbers: UPS (1Z...), FedEx (12-34 digits), USPS (20-22 digits)
    for m in RE_UPS.find_iter(text) {
        results.push(RegexExtraction {
            data_type: "tracking".to_string(),
            data_key: "ups_tracking".to_string(),
            data_value: m.as_str().to_string(),
            confidence: 0.9,
        });
    }

    // Only match FedEx-like numbers in context of "tracking" or "fedex"
    let text_lower = text.to_lowercase();
    if text_lower.contains("tracking") || text_lower.contains("fedex") {
        for m in RE_FEDEX.find_iter(text) {
            results.push(RegexExtraction {
                data_type: "tracking".to_string(),
                data_key: "tracking_number".to_string(),
                data_value: m.as_str().to_string(),
                confidence: 0.7,
            });
        }
    }

    for m in RE_USPS.find_iter(text) {
        results.push(RegexExtraction {
            data_type: "tracking".to_string(),
            data_key: "usps_tracking".to_string(),
            data_value: m.as_str().to_string(),
            confidence: 0.8,
        });
    }

    results
}

// --- AI extraction ---

fn build_extraction_prompt(subject: &str, body: &str) -> String {
    format!(
        r#"Extract structured data from this email. Return a JSON array of objects with these fields:
- "type": one of "date", "amount", "address", "tracking", "flight", "order", "contact", "link"
- "key": descriptive key like "delivery_date", "total_amount", "tracking_number", "flight_number", "order_id"
- "value": the extracted value as a string
- "confidence": float 0.0 to 1.0

Only extract data that is clearly present. Return an empty array [] if nothing is found.

Subject: {subject}

Body:
{body}

Respond ONLY with the JSON array, no other text."#
    )
}

const EXTRACTION_SYSTEM_PROMPT: &str =
    "You are a structured data extraction engine. Extract dates, amounts, addresses, tracking numbers, flight info, order confirmations, contacts, and links from emails. Return ONLY valid JSON.";

async fn extract_with_ai(
    providers: &crate::ai::provider::ProviderPool,
    subject: &str,
    body: &str,
) -> Vec<AiExtraction> {
    let prompt = build_extraction_prompt(subject, body);
    let response = match providers
        .generate(&prompt, Some(EXTRACTION_SYSTEM_PROMPT))
        .await
    {
        Some(r) => r,
        None => return Vec::new(),
    };

    // Try to parse the AI response as JSON array
    // Handle cases where AI wraps in markdown code blocks
    let trimmed = response.trim();
    let json_str = if trimmed.starts_with("```") {
        // Strip markdown code fence
        let stripped = trimmed
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim();
        stripped
    } else {
        trimmed
    };

    match serde_json::from_str::<Vec<AiExtraction>>(json_str) {
        Ok(extractions) => extractions
            .into_iter()
            .filter(|e| is_valid_data_type(&e.data_type))
            .collect(),
        Err(_) => Vec::new(),
    }
}

// --- Handlers ---

/// POST /api/extract/:message_id — extract structured data from a specific message
pub async fn extract_from_message(
    State(state): State<Arc<AppState>>,
    Path(message_id): Path<String>,
) -> Result<Json<ExtractResponse>, StatusCode> {
    let conn = state
        .db
        .get()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Fetch the message
    let (account_id, subject, body_text): (String, String, String) = conn
        .query_row(
            "SELECT account_id, COALESCE(subject, ''), COALESCE(body_text, '') FROM messages WHERE id = ?1",
            rusqlite::params![message_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(|_| StatusCode::NOT_FOUND)?;

    let account_id_num: i64 = account_id.parse().unwrap_or(0);

    // Delete any existing extractions for this message (re-extract)
    conn.execute(
        "DELETE FROM extracted_data WHERE message_id = ?1",
        rusqlite::params![message_id],
    )
    .ok();

    let mut all_extracted: Vec<ExtractedDatum> = Vec::new();

    // Pass 1: Regex extraction
    let combined_text = format!("{} {}", subject, body_text);
    let regex_results = extract_with_regex(&combined_text);
    for r in &regex_results {
        conn.execute(
            "INSERT INTO extracted_data (message_id, account_id, data_type, data_key, data_value, confidence, source)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'regex')",
            rusqlite::params![
                message_id,
                account_id_num,
                r.data_type,
                r.data_key,
                r.data_value,
                r.confidence,
            ],
        )
        .ok();
    }

    // Pass 2: AI extraction
    let ai_results = extract_with_ai(&state.providers, &subject, &body_text).await;
    for a in &ai_results {
        let confidence = a.confidence.unwrap_or(0.8);
        conn.execute(
            "INSERT INTO extracted_data (message_id, account_id, data_type, data_key, data_value, confidence, source)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'ai')",
            rusqlite::params![
                message_id,
                account_id_num,
                a.data_type,
                a.key,
                a.value,
                confidence,
            ],
        )
        .ok();
    }

    // Read back all inserted extractions
    {
        let mut stmt = conn
            .prepare(
                "SELECT id, message_id, account_id, data_type, data_key, data_value, confidence, source, extracted_at
                 FROM extracted_data WHERE message_id = ?1 ORDER BY id",
            )
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        let rows = stmt
            .query_map(rusqlite::params![message_id], |row| {
                Ok(ExtractedDatum {
                    id: row.get(0)?,
                    message_id: row.get(1)?,
                    account_id: row.get(2)?,
                    data_type: row.get(3)?,
                    data_key: row.get(4)?,
                    data_value: row.get(5)?,
                    confidence: row.get(6)?,
                    source: row.get(7)?,
                    extracted_at: row.get(8)?,
                })
            })
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        for r in rows {
            if let Ok(d) = r {
                all_extracted.push(d);
            }
        }
    }

    let count = all_extracted.len();
    Ok(Json(ExtractResponse {
        message_id,
        extracted: all_extracted,
        count,
    }))
}

/// GET /api/extracted-data — list extracted data with optional filters
pub async fn list_extracted_data(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListParams>,
) -> Result<Json<ListResponse>, StatusCode> {
    let conn = state
        .db
        .get()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let limit = params.limit.unwrap_or(100).min(500);
    let offset = params.offset.unwrap_or(0).max(0);

    // Build dynamic WHERE clause
    let mut conditions: Vec<String> = Vec::new();
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let mut param_idx = 1;

    if let Some(ref dt) = params.data_type {
        if !is_valid_data_type(dt) {
            return Err(StatusCode::BAD_REQUEST);
        }
        conditions.push(format!("data_type = ?{}", param_idx));
        param_values.push(Box::new(dt.clone()));
        param_idx += 1;
    }

    if let Some(ref since) = params.since {
        conditions.push(format!("extracted_at >= ?{}", param_idx));
        param_values.push(Box::new(since.clone()));
        param_idx += 1;
    }

    if let Some(ref mid) = params.message_id {
        conditions.push(format!("message_id = ?{}", param_idx));
        param_values.push(Box::new(mid.clone()));
        param_idx += 1;
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    // Count total
    let count_sql = format!("SELECT COUNT(*) FROM extracted_data {}", where_clause);
    let params_refs: Vec<&dyn rusqlite::types::ToSql> = param_values.iter().map(|p| p.as_ref()).collect();
    let total: i64 = conn
        .query_row(&count_sql, params_refs.as_slice(), |row| row.get(0))
        .unwrap_or(0);

    // Fetch data
    let select_sql = format!(
        "SELECT id, message_id, account_id, data_type, data_key, data_value, confidence, source, extracted_at
         FROM extracted_data {} ORDER BY id DESC LIMIT ?{} OFFSET ?{}",
        where_clause, param_idx, param_idx + 1
    );

    let mut all_params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    for p in param_values {
        all_params.push(p);
    }
    all_params.push(Box::new(limit));
    all_params.push(Box::new(offset));

    let all_refs: Vec<&dyn rusqlite::types::ToSql> = all_params.iter().map(|p| p.as_ref()).collect();

    let mut stmt = conn
        .prepare(&select_sql)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let rows = stmt
        .query_map(all_refs.as_slice(), |row| {
            Ok(ExtractedDatum {
                id: row.get(0)?,
                message_id: row.get(1)?,
                account_id: row.get(2)?,
                data_type: row.get(3)?,
                data_key: row.get(4)?,
                data_value: row.get(5)?,
                confidence: row.get(6)?,
                source: row.get(7)?,
                extracted_at: row.get(8)?,
            })
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut data = Vec::new();
    for r in rows {
        if let Ok(d) = r {
            data.push(d);
        }
    }

    Ok(Json(ListResponse { data, total }))
}

/// GET /api/extracted-data/summary — summary of extracted data grouped by type
pub async fn extracted_data_summary(
    State(state): State<Arc<AppState>>,
) -> Result<Json<SummaryResponse>, StatusCode> {
    let conn = state
        .db
        .get()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let total: i64 = conn
        .query_row("SELECT COUNT(*) FROM extracted_data", [], |row| row.get(0))
        .unwrap_or(0);

    let mut by_type = Vec::new();
    {
        let mut stmt = conn
            .prepare(
                "SELECT data_type, COUNT(*) as cnt FROM extracted_data GROUP BY data_type ORDER BY cnt DESC",
            )
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        let rows = stmt
            .query_map([], |row| {
                Ok(TypeSummary {
                    data_type: row.get(0)?,
                    count: row.get(1)?,
                })
            })
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        for r in rows {
            if let Ok(s) = r {
                by_type.push(s);
            }
        }
    }

    Ok(Json(SummaryResponse { total, by_type }))
}

/// DELETE /api/extracted-data/:id — delete a specific extraction
pub async fn delete_extracted_datum(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<DeleteResponse>, StatusCode> {
    let conn = state
        .db
        .get()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let deleted = conn
        .execute(
            "DELETE FROM extracted_data WHERE id = ?1",
            rusqlite::params![id],
        )
        .unwrap_or(0)
        > 0;

    if !deleted {
        return Err(StatusCode::NOT_FOUND);
    }

    Ok(Json(DeleteResponse { deleted }))
}

// --- Tests ---

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::create_test_pool;
    use crate::models::account::{Account, CreateAccount};
    use crate::models::message::InsertMessage;

    /// Helper: create an account and insert a test message, return (pool, account_id, message_id).
    fn setup_test_message(body: &str, subject: &str) -> (crate::db::DbPool, String, String) {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        let account = Account::create(
            &conn,
            &CreateAccount {
                provider: "gmail".to_string(),
                email: "test@example.com".to_string(),
                display_name: Some("Test User".to_string()),
                imap_host: Some("imap.gmail.com".to_string()),
                imap_port: Some(993),
                smtp_host: Some("smtp.gmail.com".to_string()),
                smtp_port: Some(587),
                username: Some("test@example.com".to_string()),
                password: Some("secret".to_string()),
            },
        );

        let msg = InsertMessage {
            account_id: account.id.clone(),
            message_id: Some("<test-msg@example.com>".to_string()),
            thread_id: None,
            folder: "INBOX".to_string(),
            from_address: Some("sender@example.com".to_string()),
            from_name: Some("Sender".to_string()),
            to_addresses: Some("[\"test@example.com\"]".to_string()),
            cc_addresses: None,
            bcc_addresses: None,
            subject: Some(subject.to_string()),
            date: Some(1700000000),
            snippet: Some(body.chars().take(200).collect()),
            body_text: Some(body.to_string()),
            body_html: None,
            is_read: false,
            is_starred: false,
            is_draft: false,
            labels: None,
            uid: Some(1),
            modseq: None,
            raw_headers: None,
            has_attachments: false,
            attachment_names: None,
            size_bytes: None,
            list_unsubscribe: None,
            list_unsubscribe_post: false,
        };

        let msg_id = InsertMessage::insert(&conn, &msg).expect("insert should succeed");
        (pool, account.id, msg_id)
    }

    fn insert_extracted(
        conn: &r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>,
        message_id: &str,
        account_id: i64,
        data_type: &str,
        data_key: &str,
        data_value: &str,
        confidence: f64,
        source: &str,
    ) -> i64 {
        conn.execute(
            "INSERT INTO extracted_data (message_id, account_id, data_type, data_key, data_value, confidence, source)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![message_id, account_id, data_type, data_key, data_value, confidence, source],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    // --- Regex extraction tests ---

    #[test]
    fn test_regex_extract_dollar_amounts() {
        let results = extract_with_regex("Your total is $49.99 plus $3.50 shipping.");
        let amounts: Vec<_> = results
            .iter()
            .filter(|r| r.data_type == "amount")
            .collect();
        assert_eq!(amounts.len(), 2);
        assert_eq!(amounts[0].data_value, "$49.99");
        assert_eq!(amounts[1].data_value, "$3.50");
    }

    #[test]
    fn test_regex_extract_large_dollar_amount() {
        let results = extract_with_regex("Invoice total: $1,234.56");
        let amounts: Vec<_> = results
            .iter()
            .filter(|r| r.data_type == "amount")
            .collect();
        assert_eq!(amounts.len(), 1);
        assert_eq!(amounts[0].data_value, "$1,234.56");
    }

    #[test]
    fn test_regex_extract_dates_mdy() {
        let results = extract_with_regex("Delivery expected by 01/15/2024.");
        let dates: Vec<_> = results
            .iter()
            .filter(|r| r.data_type == "date" && r.data_key == "date_mdy")
            .collect();
        assert_eq!(dates.len(), 1);
        assert_eq!(dates[0].data_value, "01/15/2024");
    }

    #[test]
    fn test_regex_extract_dates_iso() {
        let results = extract_with_regex("Meeting on 2024-03-15 at 10am.");
        let dates: Vec<_> = results
            .iter()
            .filter(|r| r.data_type == "date" && r.data_key == "date_iso")
            .collect();
        assert_eq!(dates.len(), 1);
        assert_eq!(dates[0].data_value, "2024-03-15");
    }

    #[test]
    fn test_regex_extract_email_addresses() {
        let results = extract_with_regex("Contact us at support@acme.com or sales@acme.com");
        let emails: Vec<_> = results
            .iter()
            .filter(|r| r.data_type == "contact")
            .collect();
        assert_eq!(emails.len(), 2);
        assert_eq!(emails[0].data_value, "support@acme.com");
        assert_eq!(emails[1].data_value, "sales@acme.com");
    }

    #[test]
    fn test_regex_extract_urls() {
        let results = extract_with_regex("Visit https://example.com/track?id=123 for details.");
        let links: Vec<_> = results
            .iter()
            .filter(|r| r.data_type == "link")
            .collect();
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].data_value, "https://example.com/track?id=123");
    }

    #[test]
    fn test_regex_extract_ups_tracking() {
        let results = extract_with_regex("Your UPS tracking: 1Z999AA10123456784");
        let tracking: Vec<_> = results
            .iter()
            .filter(|r| r.data_type == "tracking" && r.data_key == "ups_tracking")
            .collect();
        assert_eq!(tracking.len(), 1);
        assert_eq!(tracking[0].data_value, "1Z999AA10123456784");
    }

    #[test]
    fn test_regex_extract_nothing_from_empty() {
        let results = extract_with_regex("");
        assert!(results.is_empty());
    }

    #[test]
    fn test_regex_extract_no_false_dates() {
        // 13/45/2024 is not a valid MM/DD pattern
        let results = extract_with_regex("Invalid date: 13/45/2024");
        let dates: Vec<_> = results
            .iter()
            .filter(|r| r.data_type == "date")
            .collect();
        assert!(dates.is_empty());
    }

    // --- Database tests ---

    #[test]
    fn test_extracted_data_table_exists() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='extracted_data'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_insert_and_query_extracted_data() {
        let (pool, _account_id, msg_id) =
            setup_test_message("Your order total is $99.99", "Order Confirmation");
        let conn = pool.get().unwrap();

        let id = insert_extracted(&conn, &msg_id, 1, "amount", "total", "$99.99", 0.9, "regex");
        assert!(id > 0);

        let row: (String, String, String, f64) = conn
            .query_row(
                "SELECT message_id, data_type, data_value, confidence FROM extracted_data WHERE id = ?1",
                rusqlite::params![id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();

        assert_eq!(row.0, msg_id);
        assert_eq!(row.1, "amount");
        assert_eq!(row.2, "$99.99");
        assert!((row.3 - 0.9).abs() < 0.001);
    }

    #[test]
    fn test_delete_extracted_data() {
        let (pool, _account_id, msg_id) =
            setup_test_message("Meeting on 2024-01-15", "Calendar");
        let conn = pool.get().unwrap();

        let id = insert_extracted(&conn, &msg_id, 1, "date", "meeting_date", "2024-01-15", 0.85, "ai");

        let deleted = conn
            .execute("DELETE FROM extracted_data WHERE id = ?1", rusqlite::params![id])
            .unwrap();
        assert_eq!(deleted, 1);

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM extracted_data WHERE id = ?1",
                rusqlite::params![id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_filter_by_type() {
        let (pool, _account_id, msg_id) =
            setup_test_message("Total $50 and date 2024-01-01", "Invoice");
        let conn = pool.get().unwrap();

        insert_extracted(&conn, &msg_id, 1, "amount", "total", "$50", 0.9, "regex");
        insert_extracted(&conn, &msg_id, 1, "date", "invoice_date", "2024-01-01", 0.85, "regex");
        insert_extracted(&conn, &msg_id, 1, "amount", "tax", "$5", 0.8, "regex");

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM extracted_data WHERE data_type = 'amount'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM extracted_data WHERE data_type = 'date'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_summary_groups_by_type() {
        let (pool, _account_id, msg_id) =
            setup_test_message("Mixed data email", "Subject");
        let conn = pool.get().unwrap();

        insert_extracted(&conn, &msg_id, 1, "amount", "total", "$100", 0.9, "regex");
        insert_extracted(&conn, &msg_id, 1, "amount", "tax", "$10", 0.9, "regex");
        insert_extracted(&conn, &msg_id, 1, "date", "due", "2024-06-01", 0.85, "ai");
        insert_extracted(&conn, &msg_id, 1, "tracking", "ups", "1Z123", 0.8, "ai");

        let mut stmt = conn
            .prepare("SELECT data_type, COUNT(*) as cnt FROM extracted_data GROUP BY data_type ORDER BY cnt DESC")
            .unwrap();
        let rows: Vec<(String, i64)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].0, "amount");
        assert_eq!(rows[0].1, 2);
        assert_eq!(rows[1].0, "date");
        assert_eq!(rows[1].1, 1);
        assert_eq!(rows[2].0, "tracking");
        assert_eq!(rows[2].1, 1);
    }

    #[test]
    fn test_valid_data_types() {
        assert!(is_valid_data_type("date"));
        assert!(is_valid_data_type("amount"));
        assert!(is_valid_data_type("address"));
        assert!(is_valid_data_type("tracking"));
        assert!(is_valid_data_type("flight"));
        assert!(is_valid_data_type("order"));
        assert!(is_valid_data_type("contact"));
        assert!(is_valid_data_type("link"));
        assert!(!is_valid_data_type("invalid"));
        assert!(!is_valid_data_type(""));
    }

    #[test]
    fn test_ai_json_parsing() {
        // Test that we can parse the expected AI response format
        let json = r#"[{"type": "amount", "key": "total", "value": "$49.99", "confidence": 0.95}]"#;
        let parsed: Vec<AiExtraction> = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].data_type, "amount");
        assert_eq!(parsed[0].key, "total");
        assert_eq!(parsed[0].value, "$49.99");
        assert!((parsed[0].confidence.unwrap() - 0.95).abs() < 0.001);
    }

    #[test]
    fn test_ai_json_parsing_with_markdown_fences() {
        let raw = "```json\n[{\"type\": \"date\", \"key\": \"deadline\", \"value\": \"2024-03-15\", \"confidence\": 0.9}]\n```";
        let trimmed = raw.trim();
        let json_str = trimmed
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim();
        let parsed: Vec<AiExtraction> = serde_json::from_str(json_str).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].data_type, "date");
        assert_eq!(parsed[0].value, "2024-03-15");
    }

    #[test]
    fn test_ai_json_parsing_invalid_returns_empty() {
        let bad_json = "This is not JSON at all";
        let result: Result<Vec<AiExtraction>, _> = serde_json::from_str(bad_json);
        assert!(result.is_err());
    }

    #[test]
    fn test_extraction_prompt_contains_content() {
        let prompt = build_extraction_prompt("Order #12345", "Your total is $99.99");
        assert!(prompt.contains("Order #12345"));
        assert!(prompt.contains("$99.99"));
        assert!(prompt.contains("JSON array"));
    }

    #[test]
    fn test_regex_combined_extraction() {
        let text = "Hi, your order total is $249.99. Expected delivery 01/20/2024. \
                    Track at https://track.example.com/1Z999AA10123456784. \
                    Contact support@shop.com for questions. Meeting 2024-02-15.";
        let results = extract_with_regex(text);

        let types: Vec<&str> = results.iter().map(|r| r.data_type.as_str()).collect();
        assert!(types.contains(&"amount"));
        assert!(types.contains(&"date"));
        assert!(types.contains(&"link"));
        assert!(types.contains(&"contact"));
        assert!(types.contains(&"tracking"));
    }

    #[test]
    fn test_multiple_messages_isolated() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        let account = Account::create(
            &conn,
            &CreateAccount {
                provider: "gmail".to_string(),
                email: "multi@example.com".to_string(),
                display_name: None,
                imap_host: Some("imap.gmail.com".to_string()),
                imap_port: Some(993),
                smtp_host: Some("smtp.gmail.com".to_string()),
                smtp_port: Some(587),
                username: Some("multi@example.com".to_string()),
                password: Some("pass".to_string()),
            },
        );

        let msg1 = InsertMessage {
            account_id: account.id.clone(),
            message_id: Some("<msg1@example.com>".to_string()),
            thread_id: None,
            folder: "INBOX".to_string(),
            from_address: Some("a@example.com".to_string()),
            from_name: None,
            to_addresses: None,
            cc_addresses: None,
            bcc_addresses: None,
            subject: Some("Msg 1".to_string()),
            date: Some(1700000000),
            snippet: None,
            body_text: Some("Body 1".to_string()),
            body_html: None,
            is_read: false,
            is_starred: false,
            is_draft: false,
            labels: None,
            uid: Some(1),
            modseq: None,
            raw_headers: None,
            has_attachments: false,
            attachment_names: None,
            size_bytes: None,
            list_unsubscribe: None,
            list_unsubscribe_post: false,
        };
        let msg2 = InsertMessage {
            account_id: account.id.clone(),
            message_id: Some("<msg2@example.com>".to_string()),
            thread_id: None,
            folder: "INBOX".to_string(),
            from_address: Some("b@example.com".to_string()),
            from_name: None,
            to_addresses: None,
            cc_addresses: None,
            bcc_addresses: None,
            subject: Some("Msg 2".to_string()),
            date: Some(1700001000),
            snippet: None,
            body_text: Some("Body 2".to_string()),
            body_html: None,
            is_read: false,
            is_starred: false,
            is_draft: false,
            labels: None,
            uid: Some(2),
            modseq: None,
            raw_headers: None,
            has_attachments: false,
            attachment_names: None,
            size_bytes: None,
            list_unsubscribe: None,
            list_unsubscribe_post: false,
        };

        let id1 = InsertMessage::insert(&conn, &msg1).unwrap();
        let id2 = InsertMessage::insert(&conn, &msg2).unwrap();

        insert_extracted(&conn, &id1, 1, "amount", "total", "$10", 0.9, "regex");
        insert_extracted(&conn, &id2, 1, "date", "due", "2024-01-01", 0.85, "ai");

        let count1: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM extracted_data WHERE message_id = ?1",
                rusqlite::params![id1],
                |row| row.get(0),
            )
            .unwrap();
        let count2: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM extracted_data WHERE message_id = ?1",
                rusqlite::params![id2],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(count1, 1);
        assert_eq!(count2, 1);
    }
}

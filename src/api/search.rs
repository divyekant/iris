use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct SearchParams {
    pub q: Option<String>,
    pub has_attachment: Option<bool>,
    pub after: Option<i64>,
    pub before: Option<i64>,
    pub account_id: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub semantic: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct SearchResult {
    pub id: String,
    pub account_id: String,
    pub thread_id: Option<String>,
    pub from_address: Option<String>,
    pub from_name: Option<String>,
    pub subject: Option<String>,
    pub snippet: String,
    pub date: Option<i64>,
    pub is_read: bool,
    pub has_attachments: bool,
}

#[derive(Debug, Serialize)]
pub struct SearchResponse {
    pub results: Vec<SearchResult>,
    pub total: i64,
    pub query: String,
    pub parsed_operators: Vec<ParsedOperator>,
}

#[derive(Debug, Serialize, Clone)]
pub struct ParsedOperator {
    pub key: String,
    pub value: String,
}

/// Parse search operators from a query string.
/// Supported: from:, to:, subject:, is:unread/read/starred, has:attachment,
/// before:YYYY-MM-DD, after:YYYY-MM-DD, category:
/// Returns (remaining_text, operators)
fn parse_operators(query: &str) -> (String, Vec<ParsedOperator>) {
    let mut operators = Vec::new();
    let mut text_parts = Vec::new();

    // Handle quoted operator values like from:"John Doe"
    let mut chars = query.chars().peekable();
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;

    while let Some(ch) = chars.next() {
        match ch {
            '"' => {
                in_quotes = !in_quotes;
                current.push(ch);
            }
            ' ' if !in_quotes => {
                if !current.is_empty() {
                    tokens.push(current.clone());
                    current.clear();
                }
            }
            _ => current.push(ch),
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }

    for token in tokens {
        let lower = token.to_lowercase();
        // Skip boolean operators (keep them as-is for FTS5 if they're standalone)
        if lower == "and" || lower == "or" {
            // Don't pass AND/OR to FTS5 — just skip them
            continue;
        }

        if let Some(colon_pos) = token.find(':') {
            let key = token[..colon_pos].to_lowercase();
            let value = token[colon_pos + 1..].trim_matches('"').to_string();

            match key.as_str() {
                "from" | "to" | "subject" | "is" | "has" | "before" | "after" | "category" => {
                    if !value.is_empty() {
                        operators.push(ParsedOperator { key, value });
                    } else {
                        text_parts.push(token);
                    }
                }
                _ => text_parts.push(token),
            }
        } else {
            text_parts.push(token);
        }
    }

    (text_parts.join(" "), operators)
}

/// Parse a date string (YYYY-MM-DD or relative like "today", "yesterday") to Unix timestamp
fn parse_date_to_timestamp(date_str: &str, end_of_day: bool) -> Option<i64> {
    use chrono::{Local, NaiveDate, NaiveTime, TimeZone};

    let today = Local::now().date_naive();

    let date = match date_str.to_lowercase().as_str() {
        "today" => Some(today),
        "yesterday" => today.pred_opt(),
        _ => NaiveDate::parse_from_str(date_str, "%Y-%m-%d").ok(),
    };

    date.map(|d| {
        let time = if end_of_day {
            NaiveTime::from_hms_opt(23, 59, 59).unwrap()
        } else {
            NaiveTime::from_hms_opt(0, 0, 0).unwrap()
        };
        let dt = d.and_time(time);
        Local.from_local_datetime(&dt).single().map(|ldt| ldt.timestamp()).unwrap_or(0)
    })
}

pub async fn search(
    State(state): State<Arc<AppState>>,
    Query(params): Query<SearchParams>,
) -> Result<Json<SearchResponse>, StatusCode> {
    let raw_query = params.q.as_deref().unwrap_or("").trim().to_string();
    if raw_query.is_empty() {
        return Ok(Json(SearchResponse {
            results: Vec::new(),
            total: 0,
            query: raw_query,
            parsed_operators: Vec::new(),
        }));
    }

    // Parse operators from query
    let (text_query, operators) = parse_operators(&raw_query);

    // Semantic search path — use Memories hybrid BM25+vector search
    // Only use semantic for text queries (operators still applied as SQL filters)
    if params.semantic == Some(true) && !text_query.is_empty() {
        let source_prefix = params.account_id.as_ref()
            .map(|id| format!("iris/{}/", id))
            .unwrap_or_else(|| "iris/".to_string());

        let limit = params.limit.unwrap_or(50) as usize;
        let mem_results = state.memories.search(&text_query, limit, Some(&source_prefix)).await;

        if !mem_results.is_empty() {
            let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            let mut results = Vec::new();
            for r in &mem_results {
                let msg_id = r.source.rsplit('/').next().unwrap_or("").to_string();
                if msg_id.is_empty() { continue; }
                if let Ok(sr) = conn.query_row(
                    "SELECT id, account_id, thread_id, from_address, from_name, subject, date, is_read, has_attachments
                     FROM messages WHERE id = ?1 AND is_deleted = 0",
                    rusqlite::params![msg_id],
                    |row| {
                        Ok(SearchResult {
                            id: row.get(0)?,
                            account_id: row.get(1)?,
                            thread_id: row.get(2)?,
                            from_address: row.get(3)?,
                            from_name: row.get(4)?,
                            subject: row.get(5)?,
                            snippet: r.text.chars().take(200).collect(),
                            date: row.get(6)?,
                            is_read: row.get(7)?,
                            has_attachments: row.get(8)?,
                        })
                    },
                ) {
                    results.push(sr);
                }
            }
            let total = results.len() as i64;
            return Ok(Json(SearchResponse { results, total, query: raw_query, parsed_operators: operators }));
        }
        // Fall through to FTS5 if Memories returned nothing
    }

    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let limit = params.limit.unwrap_or(50).min(500);
    let offset = params.offset.unwrap_or(0).max(0);

    // Build dynamic WHERE clauses from both operators and legacy params
    let mut conditions = Vec::new();
    let mut filter_params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let mut param_idx = 1;

    // Determine if we have FTS text to search
    let has_fts = !text_query.is_empty();

    if has_fts {
        // FTS5 match
        conditions.push(format!("fts.fts_messages MATCH ?{param_idx}"));
        let fts_query = text_query
            .split_whitespace()
            .map(|term| {
                let clean = term.replace('"', "");
                format!("\"{clean}\"")
            })
            .collect::<Vec<_>>()
            .join(" ");
        filter_params.push(Box::new(fts_query));
        param_idx += 1;
    }

    // Apply parsed operators as SQL conditions
    for op in &operators {
        match op.key.as_str() {
            "from" => {
                conditions.push(format!(
                    "(LOWER(m.from_address) LIKE ?{p} OR LOWER(m.from_name) LIKE ?{p})",
                    p = param_idx
                ));
                filter_params.push(Box::new(format!("%{}%", op.value.to_lowercase())));
                param_idx += 1;
            }
            "to" => {
                conditions.push(format!("LOWER(m.to_addresses) LIKE ?{param_idx}"));
                filter_params.push(Box::new(format!("%{}%", op.value.to_lowercase())));
                param_idx += 1;
            }
            "subject" => {
                conditions.push(format!("LOWER(m.subject) LIKE ?{param_idx}"));
                filter_params.push(Box::new(format!("%{}%", op.value.to_lowercase())));
                param_idx += 1;
            }
            "is" => {
                match op.value.as_str() {
                    "unread" => conditions.push("m.is_read = 0".to_string()),
                    "read" => conditions.push("m.is_read = 1".to_string()),
                    "starred" => conditions.push("m.is_starred = 1".to_string()),
                    _ => {}
                }
            }
            "has" => {
                if op.value == "attachment" || op.value == "attachments" {
                    conditions.push("m.has_attachments = 1".to_string());
                }
            }
            "before" => {
                if let Some(ts) = parse_date_to_timestamp(&op.value, true) {
                    conditions.push(format!("m.date <= ?{param_idx}"));
                    filter_params.push(Box::new(ts));
                    param_idx += 1;
                }
            }
            "after" => {
                if let Some(ts) = parse_date_to_timestamp(&op.value, false) {
                    conditions.push(format!("m.date >= ?{param_idx}"));
                    filter_params.push(Box::new(ts));
                    param_idx += 1;
                }
            }
            "category" => {
                conditions.push(format!("LOWER(m.ai_category) = LOWER(?{param_idx})"));
                filter_params.push(Box::new(op.value.clone()));
                param_idx += 1;
            }
            _ => {}
        }
    }

    // Apply legacy query params (override if operator already set)
    if let Some(true) = params.has_attachment {
        if !operators.iter().any(|o| o.key == "has") {
            conditions.push("m.has_attachments = 1".to_string());
        }
    }

    if let Some(after) = params.after {
        if !operators.iter().any(|o| o.key == "after") {
            conditions.push(format!("m.date >= ?{param_idx}"));
            filter_params.push(Box::new(after));
            param_idx += 1;
        }
    }

    if let Some(before) = params.before {
        if !operators.iter().any(|o| o.key == "before") {
            conditions.push(format!("m.date <= ?{param_idx}"));
            filter_params.push(Box::new(before));
            param_idx += 1;
        }
    }

    if let Some(ref account_id) = params.account_id {
        conditions.push(format!("m.account_id = ?{param_idx}"));
        filter_params.push(Box::new(account_id.clone()));
        param_idx += 1;
    }

    conditions.push("m.is_deleted = 0".to_string());

    // If no FTS text and no conditions except is_deleted, we need at least one filter
    if !has_fts && conditions.len() <= 1 {
        return Ok(Json(SearchResponse {
            results: Vec::new(),
            total: 0,
            query: raw_query,
            parsed_operators: operators,
        }));
    }

    let where_clause = conditions.join(" AND ");

    // Build query — use FTS5 table join only when text search is needed
    let (sql, count_sql) = if has_fts {
        let sql = format!(
            "SELECT m.id, m.account_id, m.thread_id, m.from_address, m.from_name,
                    m.subject, snippet(fts_messages, -1, '<mark>', '</mark>', '...', 40) as match_snippet,
                    m.date, m.is_read, m.has_attachments
             FROM fts_messages fts
             JOIN messages m ON fts.rowid = m.rowid
             WHERE {where_clause}
             ORDER BY rank
             LIMIT ?{param_idx} OFFSET ?{}",
            param_idx + 1
        );
        let count_sql = format!(
            "SELECT COUNT(*)
             FROM fts_messages fts
             JOIN messages m ON fts.rowid = m.rowid
             WHERE {where_clause}"
        );
        (sql, count_sql)
    } else {
        // Operator-only query (no text) — query messages directly
        let sql = format!(
            "SELECT m.id, m.account_id, m.thread_id, m.from_address, m.from_name,
                    m.subject, COALESCE(SUBSTR(m.body_text, 1, 200), '') as match_snippet,
                    m.date, m.is_read, m.has_attachments
             FROM messages m
             WHERE {where_clause}
             ORDER BY m.date DESC
             LIMIT ?{param_idx} OFFSET ?{}",
            param_idx + 1
        );
        let count_sql = format!(
            "SELECT COUNT(*)
             FROM messages m
             WHERE {where_clause}"
        );
        (sql, count_sql)
    };

    filter_params.push(Box::new(limit));
    filter_params.push(Box::new(offset));

    let params_refs: Vec<&dyn rusqlite::types::ToSql> = filter_params.iter().map(|p| p.as_ref()).collect();

    let mut stmt = conn.prepare(&sql).map_err(|e| {
        tracing::error!("Search query error: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let results: Vec<SearchResult> = stmt
        .query_map(params_refs.as_slice(), |row| {
            Ok(SearchResult {
                id: row.get("id")?,
                account_id: row.get("account_id")?,
                thread_id: row.get("thread_id")?,
                from_address: row.get("from_address")?,
                from_name: row.get("from_name")?,
                subject: row.get("subject")?,
                snippet: row.get("match_snippet")?,
                date: row.get("date")?,
                is_read: row.get("is_read")?,
                has_attachments: row.get("has_attachments")?,
            })
        })
        .map_err(|e| {
            tracing::error!("Search execution error: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .filter_map(|r| r.ok())
        .collect();

    // Count total matches (without LIMIT/OFFSET)
    let count_params: Vec<&dyn rusqlite::types::ToSql> = filter_params[..filter_params.len() - 2]
        .iter()
        .map(|p| p.as_ref())
        .collect();
    let total: i64 = conn
        .query_row(&count_sql, count_params.as_slice(), |row| row.get(0))
        .unwrap_or(0);

    Ok(Json(SearchResponse {
        results,
        total,
        query: raw_query,
        parsed_operators: operators,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_basic_operators() {
        let (text, ops) = parse_operators("from:sarah@acme.com is:unread hello");
        assert_eq!(text, "hello");
        assert_eq!(ops.len(), 2);
        assert_eq!(ops[0].key, "from");
        assert_eq!(ops[0].value, "sarah@acme.com");
        assert_eq!(ops[1].key, "is");
        assert_eq!(ops[1].value, "unread");
    }

    #[test]
    fn test_parse_has_attachment() {
        let (text, ops) = parse_operators("has:attachment invoice");
        assert_eq!(text, "invoice");
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].key, "has");
        assert_eq!(ops[0].value, "attachment");
    }

    #[test]
    fn test_parse_quoted_value() {
        let (text, ops) = parse_operators("from:\"John Doe\" subject:\"quarterly report\"");
        assert_eq!(text, "");
        assert_eq!(ops.len(), 2);
        assert_eq!(ops[0].value, "John Doe");
        assert_eq!(ops[1].value, "quarterly report");
    }

    #[test]
    fn test_parse_date_operators() {
        let (text, ops) = parse_operators("after:2026-01-01 before:2026-03-01 meeting");
        assert_eq!(text, "meeting");
        assert_eq!(ops.len(), 2);
        assert_eq!(ops[0].key, "after");
        assert_eq!(ops[1].key, "before");
    }

    #[test]
    fn test_parse_category() {
        let (text, ops) = parse_operators("category:promotions");
        assert_eq!(text, "");
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].key, "category");
        assert_eq!(ops[0].value, "promotions");
    }

    #[test]
    fn test_parse_no_operators() {
        let (text, ops) = parse_operators("just a plain search");
        assert_eq!(text, "just a plain search");
        assert_eq!(ops.len(), 0);
    }

    #[test]
    fn test_parse_boolean_operators_stripped() {
        let (text, ops) = parse_operators("from:alice OR from:bob");
        assert_eq!(text, "");
        assert_eq!(ops.len(), 2);
    }

    #[test]
    fn test_parse_empty_value_ignored() {
        let (text, ops) = parse_operators("from: hello");
        assert_eq!(text, "from: hello");
        assert_eq!(ops.len(), 0);
    }

    #[test]
    fn test_parse_unknown_operator_kept_as_text() {
        let (text, ops) = parse_operators("label:important hello");
        assert_eq!(text, "label:important hello");
        assert_eq!(ops.len(), 0);
    }
}

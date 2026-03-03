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
}

pub async fn search(
    State(state): State<Arc<AppState>>,
    Query(params): Query<SearchParams>,
) -> Result<Json<SearchResponse>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let query_str = params.q.as_deref().unwrap_or("").trim().to_string();
    if query_str.is_empty() {
        return Ok(Json(SearchResponse {
            results: Vec::new(),
            total: 0,
            query: query_str,
        }));
    }

    let limit = params.limit.unwrap_or(50);
    let offset = params.offset.unwrap_or(0);

    // Build dynamic WHERE clauses for filters
    let mut conditions = Vec::new();
    let mut filter_params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let mut param_idx = 1;

    // FTS5 match — parameter index 1
    conditions.push(format!("fts.fts_messages MATCH ?{param_idx}"));
    let fts_query = query_str
        .split_whitespace()
        .map(|term| {
            let clean = term.replace('"', "");
            format!("\"{clean}\"")
        })
        .collect::<Vec<_>>()
        .join(" ");
    filter_params.push(Box::new(fts_query));
    param_idx += 1;

    if let Some(true) = params.has_attachment {
        conditions.push("m.has_attachments = 1".to_string());
    }

    if let Some(after) = params.after {
        conditions.push(format!("m.date >= ?{param_idx}"));
        filter_params.push(Box::new(after));
        param_idx += 1;
    }

    if let Some(before) = params.before {
        conditions.push(format!("m.date <= ?{param_idx}"));
        filter_params.push(Box::new(before));
        param_idx += 1;
    }

    if let Some(ref account_id) = params.account_id {
        conditions.push(format!("m.account_id = ?{param_idx}"));
        filter_params.push(Box::new(account_id.clone()));
        param_idx += 1;
    }

    conditions.push("m.is_deleted = 0".to_string());

    let where_clause = conditions.join(" AND ");

    // Search query with FTS5 snippet for highlighting
    // snippet(table, col, open, close, ellipsis, max_tokens) — col -1 = best matching column
    let sql = format!(
        "SELECT m.id, m.account_id, m.thread_id, m.from_address, m.from_name,
                m.subject, snippet(fts, -1, '<mark>', '</mark>', '...', 40) as match_snippet,
                m.date, m.is_read, m.has_attachments
         FROM fts_messages fts
         JOIN messages m ON fts.rowid = m.rowid
         WHERE {where_clause}
         ORDER BY rank
         LIMIT ?{param_idx} OFFSET ?{}",
        param_idx + 1
    );
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
    let count_sql = format!(
        "SELECT COUNT(*)
         FROM fts_messages fts
         JOIN messages m ON fts.rowid = m.rowid
         WHERE {where_clause}"
    );
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
        query: query_str,
    }))
}

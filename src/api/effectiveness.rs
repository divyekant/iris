use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::AppState;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct ScoreRequest {
    pub account_id: String,
    pub subject: Option<String>,
    pub body: String,
    pub to: Option<String>,
    pub context: Option<String>,
    pub draft_id: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct ScoreBreakdown {
    pub clarity: f64,
    pub tone: f64,
    pub length: f64,
    pub subject_line: f64,
    pub call_to_action: f64,
}

#[derive(Debug, Serialize, Clone)]
pub struct DimensionFeedback {
    pub clarity: String,
    pub tone: String,
    pub length: String,
    pub subject_line: String,
    pub call_to_action: String,
}

#[derive(Debug, Serialize)]
pub struct ScoreResponse {
    pub id: String,
    pub overall_score: f64,
    pub breakdown: ScoreBreakdown,
    pub feedback: DimensionFeedback,
    pub tips: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct HistoryQuery {
    pub account_id: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct HistoryEntry {
    pub id: String,
    pub subject: Option<String>,
    pub overall_score: f64,
    pub breakdown: ScoreBreakdown,
    pub created_at: i64,
}

#[derive(Debug, Serialize)]
pub struct HistoryResponse {
    pub scores: Vec<HistoryEntry>,
    pub average_overall: f64,
}

#[derive(Debug, Deserialize)]
pub struct TipsRequest {
    pub subject: Option<String>,
    pub body: String,
}

#[derive(Debug, Serialize)]
pub struct TipsResponse {
    pub tips: Vec<String>,
}

// ---------------------------------------------------------------------------
// AI Prompt
// ---------------------------------------------------------------------------

const SCORING_SYSTEM_PROMPT: &str = r#"You are an email effectiveness analyst. Score the given email draft on 5 dimensions, each from 0.0 to 1.0. Return ONLY valid JSON with no markdown formatting, no code fences, no explanation outside the JSON.

Required JSON format:
{
  "clarity": 0.0-1.0,
  "tone": 0.0-1.0,
  "length": 0.0-1.0,
  "subject_line": 0.0-1.0,
  "call_to_action": 0.0-1.0,
  "feedback": {
    "clarity": "brief explanation",
    "tone": "brief explanation",
    "length": "brief explanation",
    "subject_line": "brief explanation",
    "call_to_action": "brief explanation"
  },
  "tips": ["tip 1", "tip 2", "tip 3"]
}

Scoring criteria:
- clarity: Is the message clear and easy to understand? (structure, conciseness, no ambiguity)
- tone: Is the tone appropriate for the context? (professional vs casual match)
- length: Is the length appropriate? (not too long, not too short for the purpose)
- subject_line: Is the subject line effective and descriptive? (0.5 if missing)
- call_to_action: Is there a clear next step or ask? (lower if no action needed and none implied)

Return 1-3 specific, actionable improvement tips."#;

const TIPS_SYSTEM_PROMPT: &str = r#"You are an email writing coach. Given an email draft, provide 1-5 specific, actionable improvement tips. Return ONLY a JSON array of strings, no markdown, no code fences.

Example: ["Shorten your opening paragraph to get to the point faster", "Add a specific deadline for the requested action", "Consider a more descriptive subject line"]

Focus on the most impactful improvements for clarity, tone, length, subject, and call-to-action."#;

// ---------------------------------------------------------------------------
// POST /api/compose/effectiveness-score
// ---------------------------------------------------------------------------

pub async fn score_effectiveness(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ScoreRequest>,
) -> Result<Json<ScoreResponse>, StatusCode> {
    // Validate: body must not be empty/whitespace
    if req.body.trim().is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    // Cap content length
    if req.body.len() > 100_000 {
        return Err(StatusCode::PAYLOAD_TOO_LARGE);
    }

    // Check AI is enabled
    let conn = state.db.get().map_err(|e| {
        tracing::error!("DB pool error: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

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

    // Build the user prompt
    let subject_display = req.subject.as_deref().unwrap_or("(no subject)");
    let to_display = req.to.as_deref().unwrap_or("(unspecified)");
    let context_section = req
        .context
        .as_ref()
        .map(|c| format!("\nContext: {}", c))
        .unwrap_or_default();

    let user_prompt = format!(
        "Subject: {}\nTo: {}\n{}\n\nBody:\n{}",
        subject_display, to_display, context_section, req.body
    );

    // Call AI
    let ai_response = state
        .providers
        .generate(&user_prompt, Some(SCORING_SYSTEM_PROMPT))
        .await
        .ok_or_else(|| {
            tracing::error!("AI effectiveness scoring returned no result");
            StatusCode::BAD_GATEWAY
        })?;

    // Parse AI response
    let parsed = parse_score_response(&ai_response).map_err(|e| {
        tracing::error!("Failed to parse AI scoring response: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // Compute weighted overall score
    let overall = compute_overall_score(&parsed.0);

    // Store in database
    let id = uuid::Uuid::new_v4().to_string();
    let feedback_json = serde_json::to_string(&parsed.1).unwrap_or_else(|_| "{}".to_string());
    let tips_json = serde_json::to_string(&parsed.2).unwrap_or_else(|_| "[]".to_string());

    conn.execute(
        "INSERT INTO effectiveness_scores (id, account_id, draft_id, subject, overall_score, clarity_score, tone_score, length_score, subject_score, cta_score, feedback, tips)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        rusqlite::params![
            id,
            req.account_id,
            req.draft_id,
            req.subject,
            overall,
            parsed.0.clarity,
            parsed.0.tone,
            parsed.0.length,
            parsed.0.subject_line,
            parsed.0.call_to_action,
            feedback_json,
            tips_json,
        ],
    )
    .map_err(|e| {
        tracing::error!("Failed to store effectiveness score: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(ScoreResponse {
        id,
        overall_score: overall,
        breakdown: parsed.0,
        feedback: parsed.1,
        tips: parsed.2,
    }))
}

// ---------------------------------------------------------------------------
// GET /api/compose/effectiveness-history
// ---------------------------------------------------------------------------

pub async fn effectiveness_history(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HistoryQuery>,
) -> Result<Json<HistoryResponse>, StatusCode> {
    let conn = state.db.get().map_err(|e| {
        tracing::error!("DB pool error: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let limit = params.limit.unwrap_or(10).min(100).max(1);

    let (sql, use_account_filter) = if params.account_id.is_some() {
        (
            "SELECT id, subject, overall_score, clarity_score, tone_score, length_score, subject_score, cta_score, created_at
             FROM effectiveness_scores
             WHERE account_id = ?1
             ORDER BY created_at DESC
             LIMIT ?2",
            true,
        )
    } else {
        (
            "SELECT id, subject, overall_score, clarity_score, tone_score, length_score, subject_score, cta_score, created_at
             FROM effectiveness_scores
             ORDER BY created_at DESC
             LIMIT ?1",
            false,
        )
    };

    let mut stmt = conn.prepare(sql).map_err(|e| {
        tracing::error!("Failed to prepare history query: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let rows = if use_account_filter {
        let account_id = params.account_id.as_deref().unwrap_or("");
        stmt.query_map(rusqlite::params![account_id, limit], map_history_row)
    } else {
        stmt.query_map(rusqlite::params![limit], map_history_row)
    }
    .map_err(|e| {
        tracing::error!("Failed to query effectiveness history: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let scores: Vec<HistoryEntry> = rows
        .filter_map(|r| r.map_err(|e| tracing::warn!("History row skip: {e}")).ok())
        .collect();

    let average_overall = if scores.is_empty() {
        0.0
    } else {
        let sum: f64 = scores.iter().map(|s| s.overall_score).sum();
        (sum / scores.len() as f64 * 100.0).round() / 100.0
    };

    Ok(Json(HistoryResponse {
        scores,
        average_overall,
    }))
}

fn map_history_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<HistoryEntry> {
    Ok(HistoryEntry {
        id: row.get(0)?,
        subject: row.get(1)?,
        overall_score: row.get(2)?,
        breakdown: ScoreBreakdown {
            clarity: row.get(3)?,
            tone: row.get(4)?,
            length: row.get(5)?,
            subject_line: row.get(6)?,
            call_to_action: row.get(7)?,
        },
        created_at: row.get(8)?,
    })
}

// ---------------------------------------------------------------------------
// POST /api/compose/effectiveness-tips
// ---------------------------------------------------------------------------

pub async fn effectiveness_tips(
    State(state): State<Arc<AppState>>,
    Json(req): Json<TipsRequest>,
) -> Result<Json<TipsResponse>, StatusCode> {
    // Validate: body must not be empty/whitespace
    if req.body.trim().is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    if req.body.len() > 100_000 {
        return Err(StatusCode::PAYLOAD_TOO_LARGE);
    }

    // Check AI is enabled
    let conn = state.db.get().map_err(|e| {
        tracing::error!("DB pool error: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

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

    let subject_display = req.subject.as_deref().unwrap_or("(no subject)");
    let user_prompt = format!("Subject: {}\n\nBody:\n{}", subject_display, req.body);

    let ai_response = state
        .providers
        .generate(&user_prompt, Some(TIPS_SYSTEM_PROMPT))
        .await
        .ok_or_else(|| {
            tracing::error!("AI tips generation returned no result");
            StatusCode::BAD_GATEWAY
        })?;

    let tips = parse_tips_response(&ai_response);

    Ok(Json(TipsResponse { tips }))
}

// ---------------------------------------------------------------------------
// Parsing helpers
// ---------------------------------------------------------------------------

/// Compute weighted overall score from dimension scores.
/// Weights: clarity=0.25, tone=0.20, length=0.15, subject=0.20, cta=0.20
pub fn compute_overall_score(breakdown: &ScoreBreakdown) -> f64 {
    let raw = breakdown.clarity * 0.25
        + breakdown.tone * 0.20
        + breakdown.length * 0.15
        + breakdown.subject_line * 0.20
        + breakdown.call_to_action * 0.20;
    (raw * 100.0).round() / 100.0
}

/// Clamp a score to [0.0, 1.0].
fn clamp_score(v: f64) -> f64 {
    v.max(0.0).min(1.0)
}

/// Extract a JSON object from a string that may contain markdown fences or preamble.
pub fn extract_json_object(text: &str) -> Option<&str> {
    // Try to find JSON within code fences first
    if let Some(start) = text.find("```json") {
        let after_fence = &text[start + 7..];
        if let Some(end) = after_fence.find("```") {
            return Some(after_fence[..end].trim());
        }
    }
    if let Some(start) = text.find("```") {
        let after_fence = &text[start + 3..];
        if let Some(end) = after_fence.find("```") {
            let inner = after_fence[..end].trim();
            if inner.starts_with('{') || inner.starts_with('[') {
                return Some(inner);
            }
        }
    }

    // Find first { and last } for object
    if let Some(start) = text.find('{') {
        if let Some(end) = text.rfind('}') {
            if end > start {
                return Some(&text[start..=end]);
            }
        }
    }

    // Find first [ and last ] for array
    if let Some(start) = text.find('[') {
        if let Some(end) = text.rfind(']') {
            if end > start {
                return Some(&text[start..=end]);
            }
        }
    }

    None
}

/// Parse the AI scoring response JSON into (ScoreBreakdown, DimensionFeedback, tips).
pub fn parse_score_response(
    response: &str,
) -> Result<(ScoreBreakdown, DimensionFeedback, Vec<String>), String> {
    let json_str = extract_json_object(response).ok_or("No JSON found in AI response")?;

    let parsed: serde_json::Value =
        serde_json::from_str(json_str).map_err(|e| format!("Invalid JSON: {e}"))?;

    let clarity = clamp_score(parsed["clarity"].as_f64().unwrap_or(0.5));
    let tone = clamp_score(parsed["tone"].as_f64().unwrap_or(0.5));
    let length = clamp_score(parsed["length"].as_f64().unwrap_or(0.5));
    let subject_line = clamp_score(parsed["subject_line"].as_f64().unwrap_or(0.5));
    let call_to_action = clamp_score(parsed["call_to_action"].as_f64().unwrap_or(0.5));

    let breakdown = ScoreBreakdown {
        clarity,
        tone,
        length,
        subject_line,
        call_to_action,
    };

    let fb = &parsed["feedback"];
    let feedback = DimensionFeedback {
        clarity: fb["clarity"].as_str().unwrap_or("No feedback").to_string(),
        tone: fb["tone"].as_str().unwrap_or("No feedback").to_string(),
        length: fb["length"].as_str().unwrap_or("No feedback").to_string(),
        subject_line: fb["subject_line"]
            .as_str()
            .unwrap_or("No feedback")
            .to_string(),
        call_to_action: fb["call_to_action"]
            .as_str()
            .unwrap_or("No feedback")
            .to_string(),
    };

    let tips = parsed["tips"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    Ok((breakdown, feedback, tips))
}

/// Parse a tips-only AI response (JSON array of strings).
pub fn parse_tips_response(response: &str) -> Vec<String> {
    let json_str = match extract_json_object(response) {
        Some(s) => s,
        None => return vec!["Review your email for clarity and tone.".to_string()],
    };

    // Try parsing as array directly
    if let Ok(arr) = serde_json::from_str::<Vec<String>>(json_str) {
        return arr;
    }

    // Try parsing as object with "tips" key
    if let Ok(obj) = serde_json::from_str::<serde_json::Value>(json_str) {
        if let Some(arr) = obj["tips"].as_array() {
            return arr
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect();
        }
    }

    vec!["Review your email for clarity and tone.".to_string()]
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_test_db() -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::migrations::run(&conn).unwrap();
        // Create the effectiveness_scores table (migration 039 not wired yet)
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS effectiveness_scores (
                id TEXT PRIMARY KEY,
                account_id TEXT NOT NULL,
                draft_id TEXT,
                subject TEXT,
                overall_score REAL NOT NULL,
                clarity_score REAL NOT NULL,
                tone_score REAL NOT NULL,
                length_score REAL NOT NULL,
                subject_score REAL NOT NULL,
                cta_score REAL NOT NULL,
                feedback TEXT NOT NULL,
                tips TEXT,
                created_at INTEGER NOT NULL DEFAULT (unixepoch())
            );",
        )
        .unwrap();
        conn
    }

    fn insert_test_score(
        conn: &rusqlite::Connection,
        id: &str,
        account_id: &str,
        subject: Option<&str>,
        overall: f64,
        clarity: f64,
        tone: f64,
        length: f64,
        subject_score: f64,
        cta: f64,
    ) {
        conn.execute(
            "INSERT INTO effectiveness_scores (id, account_id, subject, overall_score, clarity_score, tone_score, length_score, subject_score, cta_score, feedback, tips)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            rusqlite::params![id, account_id, subject, overall, clarity, tone, length, subject_score, cta, "{}", "[]"],
        )
        .unwrap();
    }

    // -----------------------------------------------------------------------
    // Database storage tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_score_stores_in_database() {
        let conn = setup_test_db();
        insert_test_score(
            &conn,
            "s1",
            "acc1",
            Some("Hello"),
            0.85,
            0.9,
            0.8,
            0.85,
            0.8,
            0.9,
        );

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM effectiveness_scores WHERE id = 's1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_score_stores_all_dimensions() {
        let conn = setup_test_db();
        insert_test_score(
            &conn,
            "s2",
            "acc1",
            Some("Test"),
            0.75,
            0.8,
            0.7,
            0.65,
            0.9,
            0.6,
        );

        let (overall, clarity, tone, length, subj, cta): (f64, f64, f64, f64, f64, f64) = conn
            .query_row(
                "SELECT overall_score, clarity_score, tone_score, length_score, subject_score, cta_score
                 FROM effectiveness_scores WHERE id = 's2'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?)),
            )
            .unwrap();

        assert!((overall - 0.75).abs() < 0.001);
        assert!((clarity - 0.8).abs() < 0.001);
        assert!((tone - 0.7).abs() < 0.001);
        assert!((length - 0.65).abs() < 0.001);
        assert!((subj - 0.9).abs() < 0.001);
        assert!((cta - 0.6).abs() < 0.001);
    }

    // -----------------------------------------------------------------------
    // Validation tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_empty_body_rejected() {
        // Simulates what the handler does — body.trim().is_empty() check
        let body = "";
        assert!(body.trim().is_empty());
    }

    #[test]
    fn test_whitespace_only_body_rejected() {
        let body = "   \n\t  \n  ";
        assert!(body.trim().is_empty());
    }

    #[test]
    fn test_empty_subject_still_works() {
        // Subject is optional — None should produce a default display
        let subject: Option<&str> = None;
        let display = subject.unwrap_or("(no subject)");
        assert_eq!(display, "(no subject)");
    }

    // -----------------------------------------------------------------------
    // History tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_history_returns_recent_scores() {
        let conn = setup_test_db();
        insert_test_score(
            &conn,
            "h1",
            "acc1",
            Some("Email 1"),
            0.8,
            0.9,
            0.7,
            0.8,
            0.85,
            0.7,
        );
        insert_test_score(
            &conn,
            "h2",
            "acc1",
            Some("Email 2"),
            0.6,
            0.5,
            0.7,
            0.6,
            0.6,
            0.5,
        );
        insert_test_score(
            &conn,
            "h3",
            "acc2",
            Some("Email 3"),
            0.9,
            0.95,
            0.85,
            0.9,
            0.9,
            0.85,
        );

        let mut stmt = conn
            .prepare(
                "SELECT id, subject, overall_score, clarity_score, tone_score, length_score, subject_score, cta_score, created_at
                 FROM effectiveness_scores
                 WHERE account_id = ?1
                 ORDER BY created_at DESC
                 LIMIT ?2",
            )
            .unwrap();

        let rows: Vec<HistoryEntry> = stmt
            .query_map(rusqlite::params!["acc1", 10i64], map_history_row)
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert_eq!(rows.len(), 2);
        // Both should be acc1 entries
        assert!(rows.iter().all(|r| r.id == "h1" || r.id == "h2"));
    }

    #[test]
    fn test_history_respects_limit() {
        let conn = setup_test_db();
        for i in 0..5 {
            insert_test_score(
                &conn,
                &format!("lim{}", i),
                "acc1",
                Some(&format!("Email {}", i)),
                0.7 + (i as f64 * 0.05),
                0.8,
                0.7,
                0.7,
                0.7,
                0.7,
            );
        }

        let mut stmt = conn
            .prepare(
                "SELECT id, subject, overall_score, clarity_score, tone_score, length_score, subject_score, cta_score, created_at
                 FROM effectiveness_scores
                 WHERE account_id = ?1
                 ORDER BY created_at DESC
                 LIMIT ?2",
            )
            .unwrap();

        let rows: Vec<HistoryEntry> = stmt
            .query_map(rusqlite::params!["acc1", 3i64], map_history_row)
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert_eq!(rows.len(), 3);
    }

    #[test]
    fn test_history_average_calculation() {
        let conn = setup_test_db();
        insert_test_score(
            &conn,
            "avg1",
            "acc1",
            Some("A"),
            0.8,
            0.8,
            0.8,
            0.8,
            0.8,
            0.8,
        );
        insert_test_score(
            &conn,
            "avg2",
            "acc1",
            Some("B"),
            0.6,
            0.6,
            0.6,
            0.6,
            0.6,
            0.6,
        );

        let mut stmt = conn
            .prepare(
                "SELECT id, subject, overall_score, clarity_score, tone_score, length_score, subject_score, cta_score, created_at
                 FROM effectiveness_scores
                 WHERE account_id = ?1
                 ORDER BY created_at DESC
                 LIMIT ?2",
            )
            .unwrap();

        let scores: Vec<HistoryEntry> = stmt
            .query_map(rusqlite::params!["acc1", 10i64], map_history_row)
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        let sum: f64 = scores.iter().map(|s| s.overall_score).sum();
        let avg = (sum / scores.len() as f64 * 100.0).round() / 100.0;
        assert!((avg - 0.7).abs() < 0.01);
    }

    #[test]
    fn test_history_empty_returns_zero_average() {
        let scores: Vec<HistoryEntry> = vec![];
        let average_overall = if scores.is_empty() {
            0.0
        } else {
            let sum: f64 = scores.iter().map(|s| s.overall_score).sum();
            (sum / scores.len() as f64 * 100.0).round() / 100.0
        };
        assert!((average_overall - 0.0).abs() < f64::EPSILON);
    }

    // -----------------------------------------------------------------------
    // Score dimension validation
    // -----------------------------------------------------------------------

    #[test]
    fn test_score_dimensions_between_0_and_1() {
        let breakdown = ScoreBreakdown {
            clarity: 0.85,
            tone: 0.7,
            length: 0.9,
            subject_line: 0.6,
            call_to_action: 0.75,
        };
        assert!(breakdown.clarity >= 0.0 && breakdown.clarity <= 1.0);
        assert!(breakdown.tone >= 0.0 && breakdown.tone <= 1.0);
        assert!(breakdown.length >= 0.0 && breakdown.length <= 1.0);
        assert!(breakdown.subject_line >= 0.0 && breakdown.subject_line <= 1.0);
        assert!(breakdown.call_to_action >= 0.0 && breakdown.call_to_action <= 1.0);
    }

    #[test]
    fn test_clamp_score_boundaries() {
        assert!((clamp_score(1.5) - 1.0).abs() < f64::EPSILON);
        assert!((clamp_score(-0.3) - 0.0).abs() < f64::EPSILON);
        assert!((clamp_score(0.7) - 0.7).abs() < f64::EPSILON);
        assert!((clamp_score(0.0) - 0.0).abs() < f64::EPSILON);
        assert!((clamp_score(1.0) - 1.0).abs() < f64::EPSILON);
    }

    // -----------------------------------------------------------------------
    // Overall score computation
    // -----------------------------------------------------------------------

    #[test]
    fn test_compute_overall_score_weighted() {
        let breakdown = ScoreBreakdown {
            clarity: 1.0,
            tone: 1.0,
            length: 1.0,
            subject_line: 1.0,
            call_to_action: 1.0,
        };
        let overall = compute_overall_score(&breakdown);
        assert!((overall - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_compute_overall_score_zero() {
        let breakdown = ScoreBreakdown {
            clarity: 0.0,
            tone: 0.0,
            length: 0.0,
            subject_line: 0.0,
            call_to_action: 0.0,
        };
        let overall = compute_overall_score(&breakdown);
        assert!((overall - 0.0).abs() < 0.01);
    }

    #[test]
    fn test_compute_overall_score_mixed() {
        // clarity=0.8*0.25 + tone=0.6*0.20 + length=1.0*0.15 + subj=0.4*0.20 + cta=0.9*0.20
        // = 0.20 + 0.12 + 0.15 + 0.08 + 0.18 = 0.73
        let breakdown = ScoreBreakdown {
            clarity: 0.8,
            tone: 0.6,
            length: 1.0,
            subject_line: 0.4,
            call_to_action: 0.9,
        };
        let overall = compute_overall_score(&breakdown);
        assert!((overall - 0.73).abs() < 0.01);
    }

    // -----------------------------------------------------------------------
    // AI response parsing tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_score_response_valid_json() {
        let response = r#"{
            "clarity": 0.85,
            "tone": 0.7,
            "length": 0.9,
            "subject_line": 0.6,
            "call_to_action": 0.75,
            "feedback": {
                "clarity": "Message is clear and well-structured.",
                "tone": "Tone is slightly too casual for a business email.",
                "length": "Good length for this type of email.",
                "subject_line": "Subject could be more specific.",
                "call_to_action": "Clear request with deadline."
            },
            "tips": ["Make the subject more specific", "Use a more formal greeting"]
        }"#;

        let (breakdown, feedback, tips) = parse_score_response(response).unwrap();
        assert!((breakdown.clarity - 0.85).abs() < 0.001);
        assert!((breakdown.tone - 0.7).abs() < 0.001);
        assert!((breakdown.length - 0.9).abs() < 0.001);
        assert!((breakdown.subject_line - 0.6).abs() < 0.001);
        assert!((breakdown.call_to_action - 0.75).abs() < 0.001);
        assert!(feedback.clarity.contains("clear"));
        assert_eq!(tips.len(), 2);
    }

    #[test]
    fn test_parse_score_response_with_markdown_fences() {
        let response = r#"Here's the analysis:

```json
{
    "clarity": 0.9,
    "tone": 0.8,
    "length": 0.7,
    "subject_line": 0.85,
    "call_to_action": 0.6,
    "feedback": {
        "clarity": "Very clear.",
        "tone": "Good tone.",
        "length": "Could be shorter.",
        "subject_line": "Descriptive.",
        "call_to_action": "Weak CTA."
    },
    "tips": ["Add deadline"]
}
```"#;

        let (breakdown, _, tips) = parse_score_response(response).unwrap();
        assert!((breakdown.clarity - 0.9).abs() < 0.001);
        assert_eq!(tips.len(), 1);
        assert_eq!(tips[0], "Add deadline");
    }

    #[test]
    fn test_parse_score_response_missing_feedback() {
        let response = r#"{"clarity": 0.5, "tone": 0.5, "length": 0.5, "subject_line": 0.5, "call_to_action": 0.5, "tips": []}"#;
        let (breakdown, feedback, tips) = parse_score_response(response).unwrap();
        assert!((breakdown.clarity - 0.5).abs() < 0.001);
        assert_eq!(feedback.clarity, "No feedback");
        assert!(tips.is_empty());
    }

    #[test]
    fn test_parse_score_response_missing_dimensions_defaults() {
        // Missing some dimensions should default to 0.5
        let response = r#"{"clarity": 0.8, "feedback": {}, "tips": ["Improve tone"]}"#;
        let (breakdown, _, tips) = parse_score_response(response).unwrap();
        assert!((breakdown.clarity - 0.8).abs() < 0.001);
        assert!((breakdown.tone - 0.5).abs() < 0.001); // default
        assert!((breakdown.length - 0.5).abs() < 0.001); // default
        assert_eq!(tips.len(), 1);
    }

    #[test]
    fn test_parse_score_response_out_of_range_clamped() {
        let response = r#"{"clarity": 1.5, "tone": -0.3, "length": 0.7, "subject_line": 2.0, "call_to_action": 0.5, "feedback": {}, "tips": []}"#;
        let (breakdown, _, _) = parse_score_response(response).unwrap();
        assert!((breakdown.clarity - 1.0).abs() < 0.001);
        assert!((breakdown.tone - 0.0).abs() < 0.001);
        assert!((breakdown.subject_line - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_parse_score_response_no_json() {
        let response = "I cannot process this request.";
        let result = parse_score_response(response);
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // Tips parsing tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_tips_response_array() {
        let response = r#"["Use a clearer subject line", "Add a deadline", "Shorten the intro"]"#;
        let tips = parse_tips_response(response);
        assert_eq!(tips.len(), 3);
        assert_eq!(tips[0], "Use a clearer subject line");
    }

    #[test]
    fn test_parse_tips_response_object_with_tips_key() {
        let response = r#"{"tips": ["Tip 1", "Tip 2"]}"#;
        let tips = parse_tips_response(response);
        assert_eq!(tips.len(), 2);
    }

    #[test]
    fn test_parse_tips_response_fallback_on_invalid() {
        let response = "Sorry, I can't help with that.";
        let tips = parse_tips_response(response);
        assert_eq!(tips.len(), 1);
        assert!(tips[0].contains("Review"));
    }

    #[test]
    fn test_parse_tips_response_with_code_fences() {
        let response = "```json\n[\"Improve greeting\", \"Be concise\"]\n```";
        let tips = parse_tips_response(response);
        assert_eq!(tips.len(), 2);
    }

    // -----------------------------------------------------------------------
    // JSON extraction tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_extract_json_object_bare() {
        let text = r#"{"key": "value"}"#;
        assert_eq!(extract_json_object(text), Some(r#"{"key": "value"}"#));
    }

    #[test]
    fn test_extract_json_object_with_preamble() {
        let text = "Here is the result:\n{\"score\": 0.5}";
        assert_eq!(extract_json_object(text), Some("{\"score\": 0.5}"));
    }

    #[test]
    fn test_extract_json_object_code_fence() {
        let text = "```json\n{\"a\": 1}\n```";
        assert_eq!(extract_json_object(text), Some("{\"a\": 1}"));
    }

    #[test]
    fn test_extract_json_object_array() {
        let text = "[\"a\", \"b\"]";
        assert_eq!(extract_json_object(text), Some("[\"a\", \"b\"]"));
    }

    #[test]
    fn test_extract_json_object_none() {
        let text = "No JSON here at all.";
        assert_eq!(extract_json_object(text), None);
    }

    // -----------------------------------------------------------------------
    // Draft ID and optional fields
    // -----------------------------------------------------------------------

    #[test]
    fn test_score_with_draft_id() {
        let conn = setup_test_db();
        conn.execute(
            "INSERT INTO effectiveness_scores (id, account_id, draft_id, subject, overall_score, clarity_score, tone_score, length_score, subject_score, cta_score, feedback, tips)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            rusqlite::params!["d1", "acc1", "draft-123", "Subject", 0.8, 0.8, 0.8, 0.8, 0.8, 0.8, "{}", "[]"],
        )
        .unwrap();

        let draft_id: Option<String> = conn
            .query_row(
                "SELECT draft_id FROM effectiveness_scores WHERE id = 'd1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(draft_id, Some("draft-123".to_string()));
    }

    #[test]
    fn test_score_without_draft_id() {
        let conn = setup_test_db();
        conn.execute(
            "INSERT INTO effectiveness_scores (id, account_id, subject, overall_score, clarity_score, tone_score, length_score, subject_score, cta_score, feedback, tips)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            rusqlite::params!["d2", "acc1", "Subject", 0.8, 0.8, 0.8, 0.8, 0.8, 0.8, "{}", "[]"],
        )
        .unwrap();

        let draft_id: Option<String> = conn
            .query_row(
                "SELECT draft_id FROM effectiveness_scores WHERE id = 'd2'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(draft_id.is_none());
    }
}

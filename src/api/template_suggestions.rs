use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::AppState;

// ---------------------------------------------------------------------------
// Shared types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateSuggestion {
    pub id: String,
    pub name: String,
    pub subject_pattern: Option<String>,
    pub body_pattern: String,
    pub sample_message_ids: Vec<String>,
    pub pattern_count: i64,
    pub confidence: f64,
    pub status: String,
    pub created_at: i64,
    pub accepted_at: Option<i64>,
    pub dismissed_at: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct Template {
    pub id: String,
    pub account_id: String,
    pub name: String,
    pub subject: Option<String>,
    pub body: String,
    pub created_at: i64,
    pub updated_at: i64,
}

// ---------------------------------------------------------------------------
// POST /api/ai/template-suggestions/scan
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct ScanResponse {
    pub scanned: i64,
    pub suggestions_created: i64,
}

const SCAN_SYSTEM_PROMPT: &str = r#"You are an email template analyzer. Given a set of similar sent emails, extract a reusable template.

Identify the FIXED structure (greetings, closings, repeated phrases, formatting) vs VARIABLE parts (recipient names, dates, specific details, amounts).

Mark variable parts with double curly braces like {{variable_name}}.

Respond with ONLY valid JSON (no markdown fencing, no commentary):
{
  "name": "short template name",
  "subject_pattern": "subject with {{variables}} or null if subjects differ too much",
  "body_pattern": "template body with {{variable_name}} placeholders",
  "variables": ["variable_name_1", "variable_name_2"],
  "confidence": 0.85
}

The confidence should be between 0.0 and 1.0, where:
- 0.9+ = very clear repeated pattern with minor variable differences
- 0.7-0.9 = clear structure with some variation
- 0.5-0.7 = loose pattern, moderate differences
- Below 0.5 = too different to template reliably"#;

/// Scan sent emails for repeated patterns and generate template suggestions.
pub async fn scan_templates(
    State(state): State<Arc<AppState>>,
) -> Result<Json<ScanResponse>, StatusCode> {
    let conn = state.db.get().map_err(|e| {
        tracing::error!("DB pool error: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // Check AI is available
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

    // Query sent emails from last 90 days
    let ninety_days_ago = chrono::Utc::now().timestamp() - (90 * 24 * 3600);

    let sent_emails: Vec<(String, String, Option<String>, Option<String>, Option<String>)> = {
        let mut stmt = conn
            .prepare(
                "SELECT id, account_id, to_addresses, subject, body_text
                 FROM messages
                 WHERE (folder = 'Sent' OR folder = '[Gmail]/Sent Mail')
                   AND is_deleted = 0
                   AND is_draft = 0
                   AND date >= ?1
                   AND body_text IS NOT NULL
                   AND body_text != ''
                 ORDER BY date DESC
                 LIMIT 500",
            )
            .map_err(|e| {
                tracing::error!("Failed to prepare sent query: {e}");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;

        stmt.query_map(rusqlite::params![ninety_days_ago], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
            ))
        })
        .map_err(|e| {
            tracing::error!("Failed to query sent emails: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .filter_map(|r| r.ok())
        .collect()
    };

    let scanned = sent_emails.len() as i64;

    if sent_emails.is_empty() {
        return Ok(Json(ScanResponse {
            scanned: 0,
            suggestions_created: 0,
        }));
    }

    // Group by recipient + normalized subject similarity
    let groups = group_similar_emails(&sent_emails);

    let mut suggestions_created: i64 = 0;

    for group in groups {
        if group.message_ids.len() < 3 {
            continue;
        }

        // Build prompt with sample emails (up to 5 from the group)
        let sample_count = group.samples.len().min(5);
        let mut prompt = format!(
            "I found {} similar sent emails. Here are {} samples:\n\n",
            group.message_ids.len(),
            sample_count
        );

        for (i, sample) in group.samples.iter().take(5).enumerate() {
            let subject = sample.subject.as_deref().unwrap_or("(no subject)");
            let body = sample.body_text.as_deref().unwrap_or("");
            let body_truncated: String = if body.chars().count() > 500 {
                let mut s: String = body.chars().take(500).collect();
                s.push_str("...");
                s
            } else {
                body.to_string()
            };

            prompt.push_str(&format!(
                "--- Email {} ---\nTo: {}\nSubject: {}\nBody:\n{}\n\n",
                i + 1,
                sample.to_addresses.as_deref().unwrap_or("unknown"),
                subject,
                body_truncated
            ));
        }

        // Send to AI
        let response = match state.providers.generate(&prompt, Some(SCAN_SYSTEM_PROMPT)).await {
            Some(r) => r,
            None => {
                tracing::warn!("AI generation failed for template group");
                continue;
            }
        };

        // Parse AI response
        let parsed = parse_ai_template_response(&response);
        let (name, subject_pattern, body_pattern, confidence) = match parsed {
            Some(p) => p,
            None => {
                tracing::warn!("Failed to parse AI template response");
                continue;
            }
        };

        if confidence < 0.5 {
            continue;
        }

        let sample_ids_json =
            serde_json::to_string(&group.message_ids).unwrap_or_else(|_| "[]".to_string());
        let suggestion_id = uuid::Uuid::new_v4().to_string();

        // Upsert: check if a suggestion with the same name already exists (pending status)
        let existing: Option<String> = conn
            .query_row(
                "SELECT id FROM template_suggestions WHERE name = ?1 AND status = 'pending'",
                rusqlite::params![name],
                |row| row.get(0),
            )
            .ok();

        if let Some(existing_id) = existing {
            // Update existing suggestion with fresh data
            conn.execute(
                "UPDATE template_suggestions SET
                    subject_pattern = ?1,
                    body_pattern = ?2,
                    sample_message_ids = ?3,
                    pattern_count = ?4,
                    confidence = ?5
                 WHERE id = ?6",
                rusqlite::params![
                    subject_pattern,
                    body_pattern,
                    sample_ids_json,
                    group.message_ids.len() as i64,
                    confidence,
                    existing_id,
                ],
            )
            .map_err(|e| {
                tracing::error!("Failed to update template suggestion: {e}");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
        } else {
            conn.execute(
                "INSERT INTO template_suggestions (id, name, subject_pattern, body_pattern, sample_message_ids, pattern_count, confidence)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    suggestion_id,
                    name,
                    subject_pattern,
                    body_pattern,
                    sample_ids_json,
                    group.message_ids.len() as i64,
                    confidence,
                ],
            )
            .map_err(|e| {
                tracing::error!("Failed to insert template suggestion: {e}");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
        }

        suggestions_created += 1;
    }

    Ok(Json(ScanResponse {
        scanned,
        suggestions_created,
    }))
}

// ---------------------------------------------------------------------------
// GET /api/ai/template-suggestions
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct ListParams {
    pub status: Option<String>,
    pub min_confidence: Option<f64>,
}

pub async fn list_suggestions(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListParams>,
) -> Result<Json<Vec<TemplateSuggestion>>, StatusCode> {
    let conn = state.db.get().map_err(|e| {
        tracing::error!("DB pool error: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let status_filter = params.status.as_deref().unwrap_or("pending");

    // Validate status filter
    if !["pending", "accepted", "dismissed", "all"].contains(&status_filter) {
        return Err(StatusCode::BAD_REQUEST);
    }

    let min_confidence = params.min_confidence.unwrap_or(0.0);

    let suggestions = if status_filter == "all" {
        let mut stmt = conn
            .prepare(
                "SELECT id, name, subject_pattern, body_pattern, sample_message_ids,
                        pattern_count, confidence, status, created_at, accepted_at, dismissed_at
                 FROM template_suggestions
                 WHERE confidence >= ?1
                 ORDER BY confidence DESC, created_at DESC",
            )
            .map_err(|e| {
                tracing::error!("Failed to prepare suggestions query: {e}");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;

        stmt.query_map(rusqlite::params![min_confidence], |row| {
            row_to_suggestion(row)
        })
        .map_err(|e| {
            tracing::error!("Failed to query suggestions: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .filter_map(|r| r.ok())
        .collect()
    } else {
        let mut stmt = conn
            .prepare(
                "SELECT id, name, subject_pattern, body_pattern, sample_message_ids,
                        pattern_count, confidence, status, created_at, accepted_at, dismissed_at
                 FROM template_suggestions
                 WHERE status = ?1 AND confidence >= ?2
                 ORDER BY confidence DESC, created_at DESC",
            )
            .map_err(|e| {
                tracing::error!("Failed to prepare suggestions query: {e}");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;

        stmt.query_map(rusqlite::params![status_filter, min_confidence], |row| {
            row_to_suggestion(row)
        })
        .map_err(|e| {
            tracing::error!("Failed to query suggestions: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .filter_map(|r| r.ok())
        .collect()
    };

    Ok(Json(suggestions))
}

fn row_to_suggestion(row: &rusqlite::Row) -> rusqlite::Result<TemplateSuggestion> {
    let sample_ids_json: String = row.get(4)?;
    let sample_message_ids: Vec<String> =
        serde_json::from_str(&sample_ids_json).unwrap_or_default();

    Ok(TemplateSuggestion {
        id: row.get(0)?,
        name: row.get(1)?,
        subject_pattern: row.get(2)?,
        body_pattern: row.get(3)?,
        sample_message_ids,
        pattern_count: row.get(5)?,
        confidence: row.get(6)?,
        status: row.get(7)?,
        created_at: row.get(8)?,
        accepted_at: row.get(9)?,
        dismissed_at: row.get(10)?,
    })
}

// ---------------------------------------------------------------------------
// POST /api/ai/template-suggestions/{id}/accept
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct AcceptResponse {
    pub template: Template,
}

pub async fn accept_suggestion(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<AcceptResponse>, StatusCode> {
    let conn = state.db.get().map_err(|e| {
        tracing::error!("DB pool error: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // Load the suggestion
    let suggestion: TemplateSuggestion = conn
        .query_row(
            "SELECT id, name, subject_pattern, body_pattern, sample_message_ids,
                    pattern_count, confidence, status, created_at, accepted_at, dismissed_at
             FROM template_suggestions WHERE id = ?1",
            rusqlite::params![id],
            |row| row_to_suggestion(row),
        )
        .map_err(|_| StatusCode::NOT_FOUND)?;

    if suggestion.status != "pending" {
        return Err(StatusCode::CONFLICT);
    }

    // Determine account_id from the first sample message
    let account_id: String = if let Some(first_msg_id) = suggestion.sample_message_ids.first() {
        conn.query_row(
            "SELECT account_id FROM messages WHERE id = ?1",
            rusqlite::params![first_msg_id],
            |row| row.get(0),
        )
        .unwrap_or_else(|_| "default".to_string())
    } else {
        "default".to_string()
    };

    // Create the template
    let template_id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();

    conn.execute(
        "INSERT INTO templates (id, account_id, name, subject, body, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![
            template_id,
            account_id,
            suggestion.name,
            suggestion.subject_pattern,
            suggestion.body_pattern,
            now,
            now,
        ],
    )
    .map_err(|e| {
        tracing::error!("Failed to create template: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // Mark suggestion as accepted
    conn.execute(
        "UPDATE template_suggestions SET status = 'accepted', accepted_at = ?1 WHERE id = ?2",
        rusqlite::params![now, id],
    )
    .map_err(|e| {
        tracing::error!("Failed to update suggestion status: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(AcceptResponse {
        template: Template {
            id: template_id,
            account_id,
            name: suggestion.name,
            subject: suggestion.subject_pattern,
            body: suggestion.body_pattern,
            created_at: now,
            updated_at: now,
        },
    }))
}

// ---------------------------------------------------------------------------
// DELETE /api/ai/template-suggestions/{id}
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct DismissResponse {
    pub dismissed: bool,
}

pub async fn dismiss_suggestion(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<DismissResponse>, StatusCode> {
    let conn = state.db.get().map_err(|e| {
        tracing::error!("DB pool error: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let now = chrono::Utc::now().timestamp();

    let updated = conn
        .execute(
            "UPDATE template_suggestions SET status = 'dismissed', dismissed_at = ?1 WHERE id = ?2 AND status = 'pending'",
            rusqlite::params![now, id],
        )
        .map_err(|e| {
            tracing::error!("Failed to dismiss suggestion: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    if updated == 0 {
        return Err(StatusCode::NOT_FOUND);
    }

    Ok(Json(DismissResponse { dismissed: true }))
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

struct EmailSample {
    to_addresses: Option<String>,
    subject: Option<String>,
    body_text: Option<String>,
}

struct EmailGroup {
    message_ids: Vec<String>,
    samples: Vec<EmailSample>,
}

/// Group sent emails by recipient pattern and subject similarity.
/// Emails to the same recipient domain with similar subjects get grouped.
fn group_similar_emails(
    emails: &[(String, String, Option<String>, Option<String>, Option<String>)],
) -> Vec<EmailGroup> {
    // Simple grouping: normalize subject by stripping Re:/Fwd:/numbers/dates,
    // then group by (recipient_domain, normalized_subject_prefix)
    let mut groups: std::collections::HashMap<String, Vec<usize>> =
        std::collections::HashMap::new();

    for (idx, (_id, _account_id, _to, subject, _body)) in emails.iter().enumerate() {
        let normalized = normalize_subject(subject.as_deref().unwrap_or(""));
        let key = normalized;
        groups.entry(key).or_default().push(idx);
    }

    groups
        .into_iter()
        .map(|(_key, indices)| {
            let message_ids: Vec<String> = indices.iter().map(|&i| emails[i].0.clone()).collect();
            let samples: Vec<EmailSample> = indices
                .iter()
                .map(|&i| EmailSample {
                    to_addresses: emails[i].2.clone(),
                    subject: emails[i].3.clone(),
                    body_text: emails[i].4.clone(),
                })
                .collect();
            EmailGroup {
                message_ids,
                samples,
            }
        })
        .collect()
}

/// Normalize a subject line for grouping: strip prefixes (Re:/Fwd:), lowercase,
/// remove numbers and common date-like tokens.
fn normalize_subject(subject: &str) -> String {
    let mut s = subject.to_lowercase();

    // Strip Re:/Fwd:/Fw: prefixes (possibly repeated)
    loop {
        let trimmed = s.trim_start();
        if let Some(rest) = trimmed.strip_prefix("re:") {
            s = rest.to_string();
        } else if let Some(rest) = trimmed.strip_prefix("fwd:") {
            s = rest.to_string();
        } else if let Some(rest) = trimmed.strip_prefix("fw:") {
            s = rest.to_string();
        } else {
            break;
        }
    }

    // Remove digits (dates, invoice numbers, etc.)
    let s: String = s.chars().map(|c| if c.is_ascii_digit() { ' ' } else { c }).collect();

    // Collapse whitespace and trim
    let parts: Vec<&str> = s.split_whitespace().collect();
    let normalized = parts.join(" ");

    // Take first 60 chars as the grouping key (captures the template pattern)
    if normalized.len() > 60 {
        normalized[..60].to_string()
    } else {
        normalized
    }
}

/// Parse the AI response JSON to extract template fields.
/// Returns (name, subject_pattern, body_pattern, confidence).
fn parse_ai_template_response(response: &str) -> Option<(String, Option<String>, String, f64)> {
    // Try to extract JSON from the response (AI may wrap in markdown code blocks)
    let json_str = extract_json_from_response(response)?;

    let parsed: serde_json::Value = serde_json::from_str(&json_str).ok()?;

    let name = parsed.get("name")?.as_str()?.to_string();
    let subject_pattern = parsed
        .get("subject_pattern")
        .and_then(|v| {
            if v.is_null() {
                None
            } else {
                v.as_str().map(|s| s.to_string())
            }
        });
    let body_pattern = parsed.get("body_pattern")?.as_str()?.to_string();
    let confidence = parsed
        .get("confidence")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.7);

    if name.is_empty() || body_pattern.is_empty() {
        return None;
    }

    Some((name, subject_pattern, body_pattern, confidence))
}

/// Extract JSON from a potentially markdown-wrapped AI response.
fn extract_json_from_response(response: &str) -> Option<String> {
    let trimmed = response.trim();

    // If it starts with '{', assume it's raw JSON
    if trimmed.starts_with('{') {
        return Some(trimmed.to_string());
    }

    // Try to extract from ```json ... ``` blocks
    if let Some(start) = trimmed.find("```json") {
        let after_fence = &trimmed[start + 7..];
        if let Some(end) = after_fence.find("```") {
            return Some(after_fence[..end].trim().to_string());
        }
    }

    // Try to extract from ``` ... ``` blocks
    if let Some(start) = trimmed.find("```") {
        let after_fence = &trimmed[start + 3..];
        if let Some(end) = after_fence.find("```") {
            let content = after_fence[..end].trim();
            if content.starts_with('{') {
                return Some(content.to_string());
            }
        }
    }

    // Try to find first { to last }
    if let (Some(start), Some(end)) = (trimmed.find('{'), trimmed.rfind('}')) {
        if start < end {
            return Some(trimmed[start..=end].to_string());
        }
    }

    None
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

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
            email: "template-test@example.com".to_string(),
            display_name: Some("Template Test".to_string()),
            imap_host: Some("imap.gmail.com".to_string()),
            imap_port: Some(993),
            smtp_host: Some("smtp.gmail.com".to_string()),
            smtp_port: Some(587),
            username: Some("template-test@example.com".to_string()),
            password: None,
        };
        Account::create(conn, &input)
    }

    fn make_sent_message(
        account_id: &str,
        subject: &str,
        body: &str,
        to: &str,
        date: i64,
        uid: i64,
    ) -> InsertMessage {
        InsertMessage {
            account_id: account_id.to_string(),
            message_id: Some(format!("<sent-{uid}@example.com>")),
            thread_id: None,
            folder: "Sent".to_string(),
            from_address: Some("template-test@example.com".to_string()),
            from_name: Some("Template Test".to_string()),
            to_addresses: Some(format!(r#"["{}"]"#, to)),
            cc_addresses: None,
            bcc_addresses: None,
            subject: Some(subject.to_string()),
            date: Some(date),
            snippet: Some(body.chars().take(200).collect()),
            body_text: Some(body.to_string()),
            body_html: None,
            is_read: true,
            is_starred: false,
            is_draft: false,
            labels: None,
            uid: Some(uid),
            modseq: None,
            raw_headers: None,
            has_attachments: false,
            attachment_names: None,
            size_bytes: Some(512),
            list_unsubscribe: None,
            list_unsubscribe_post: false,
        }
    }

    fn run_template_suggestions_migration(
        conn: &r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>,
    ) {
        conn.execute_batch(include_str!("../../migrations/036_template_suggestions.sql"))
            .expect("Failed to run template_suggestions migration");
    }

    fn insert_suggestion(
        conn: &r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>,
        id: &str,
        name: &str,
        body_pattern: &str,
        confidence: f64,
        status: &str,
        sample_ids: &[&str],
    ) {
        let sample_json = serde_json::to_string(sample_ids).unwrap();
        conn.execute(
            "INSERT INTO template_suggestions (id, name, subject_pattern, body_pattern, sample_message_ids, pattern_count, confidence, status)
             VALUES (?1, ?2, NULL, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                id,
                name,
                body_pattern,
                sample_json,
                sample_ids.len() as i64,
                confidence,
                status,
            ],
        )
        .expect("Failed to insert test suggestion");
    }

    // -----------------------------------------------------------------------
    // Test 1: normalize_subject strips prefixes and digits
    // -----------------------------------------------------------------------
    #[test]
    fn test_normalize_subject() {
        assert_eq!(normalize_subject("Re: Weekly Update"), "weekly update");
        assert_eq!(normalize_subject("Fwd: Re: Invoice #12345"), "invoice #");
        assert_eq!(normalize_subject("FW: Meeting 2024-03-15"), "meeting - -");
        assert_eq!(normalize_subject("  Re:  Re: Hello  "), "hello");
        assert_eq!(normalize_subject(""), "");
    }

    // -----------------------------------------------------------------------
    // Test 2: parse_ai_template_response handles valid JSON
    // -----------------------------------------------------------------------
    #[test]
    fn test_parse_ai_template_response_valid() {
        let response = r#"{"name":"Weekly Report","subject_pattern":"Weekly Report for {{week}}","body_pattern":"Hi {{name}},\n\nHere is the weekly report...","variables":["name","week"],"confidence":0.92}"#;
        let result = parse_ai_template_response(response);
        assert!(result.is_some());
        let (name, subject, body, confidence) = result.unwrap();
        assert_eq!(name, "Weekly Report");
        assert_eq!(
            subject.as_deref(),
            Some("Weekly Report for {{week}}")
        );
        assert!(body.contains("weekly report"));
        assert!((confidence - 0.92).abs() < 0.01);
    }

    // -----------------------------------------------------------------------
    // Test 3: parse_ai_template_response handles markdown-wrapped JSON
    // -----------------------------------------------------------------------
    #[test]
    fn test_parse_ai_template_response_markdown_wrapped() {
        let response = "Here is the template:\n\n```json\n{\"name\":\"Follow-up\",\"subject_pattern\":null,\"body_pattern\":\"Hi {{name}}, just following up...\",\"confidence\":0.75}\n```";
        let result = parse_ai_template_response(response);
        assert!(result.is_some());
        let (name, subject, _body, confidence) = result.unwrap();
        assert_eq!(name, "Follow-up");
        assert!(subject.is_none());
        assert!((confidence - 0.75).abs() < 0.01);
    }

    // -----------------------------------------------------------------------
    // Test 4: parse_ai_template_response returns None on invalid
    // -----------------------------------------------------------------------
    #[test]
    fn test_parse_ai_template_response_invalid() {
        assert!(parse_ai_template_response("not json at all").is_none());
        assert!(parse_ai_template_response("{}").is_none());
        assert!(parse_ai_template_response(r#"{"name":"","body_pattern":""}"#).is_none());
    }

    // -----------------------------------------------------------------------
    // Test 5: extract_json_from_response handles various formats
    // -----------------------------------------------------------------------
    #[test]
    fn test_extract_json_from_response() {
        // Raw JSON
        let raw = r#"{"key": "value"}"#;
        assert_eq!(extract_json_from_response(raw), Some(raw.to_string()));

        // Markdown code block
        let md = "```json\n{\"key\": \"value\"}\n```";
        assert_eq!(
            extract_json_from_response(md),
            Some("{\"key\": \"value\"}".to_string())
        );

        // Plain code block
        let plain = "```\n{\"key\": \"value\"}\n```";
        assert_eq!(
            extract_json_from_response(plain),
            Some("{\"key\": \"value\"}".to_string())
        );

        // Embedded JSON in text
        let text = "The result is {\"key\": \"value\"} here";
        assert_eq!(
            extract_json_from_response(text),
            Some("{\"key\": \"value\"}".to_string())
        );

        // No JSON
        assert!(extract_json_from_response("no json here").is_none());
    }

    // -----------------------------------------------------------------------
    // Test 6: group_similar_emails groups correctly
    // -----------------------------------------------------------------------
    #[test]
    fn test_group_similar_emails() {
        let emails = vec![
            ("m1".to_string(), "a1".to_string(), Some("alice@test.com".to_string()), Some("Weekly Update".to_string()), Some("Body 1".to_string())),
            ("m2".to_string(), "a1".to_string(), Some("bob@test.com".to_string()), Some("Weekly Update".to_string()), Some("Body 2".to_string())),
            ("m3".to_string(), "a1".to_string(), Some("carol@test.com".to_string()), Some("Weekly Update".to_string()), Some("Body 3".to_string())),
            ("m4".to_string(), "a1".to_string(), Some("dave@test.com".to_string()), Some("Different Topic".to_string()), Some("Body 4".to_string())),
        ];

        let groups = group_similar_emails(&emails);

        // Should have 2 groups: "weekly update" (3 emails) and "different topic" (1 email)
        let large_group = groups.iter().find(|g| g.message_ids.len() == 3);
        let small_group = groups.iter().find(|g| g.message_ids.len() == 1);

        assert!(large_group.is_some());
        assert!(small_group.is_some());

        let large = large_group.unwrap();
        assert!(large.message_ids.contains(&"m1".to_string()));
        assert!(large.message_ids.contains(&"m2".to_string()));
        assert!(large.message_ids.contains(&"m3".to_string()));
    }

    // -----------------------------------------------------------------------
    // Test 7: list pending suggestions from DB
    // -----------------------------------------------------------------------
    #[test]
    fn test_list_pending_suggestions() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();


        insert_suggestion(&conn, "s1", "Template A", "Hi {{name}}", 0.9, "pending", &["m1", "m2", "m3"]);
        insert_suggestion(&conn, "s2", "Template B", "Dear {{name}}", 0.7, "pending", &["m4", "m5", "m6"]);
        insert_suggestion(&conn, "s3", "Template C", "Hello", 0.8, "accepted", &["m7", "m8"]);
        insert_suggestion(&conn, "s4", "Template D", "Hey", 0.6, "dismissed", &["m9"]);

        // List pending only
        let mut stmt = conn
            .prepare(
                "SELECT id, name, subject_pattern, body_pattern, sample_message_ids,
                        pattern_count, confidence, status, created_at, accepted_at, dismissed_at
                 FROM template_suggestions WHERE status = 'pending'
                 ORDER BY confidence DESC",
            )
            .unwrap();
        let results: Vec<TemplateSuggestion> = stmt
            .query_map([], |row| row_to_suggestion(row))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].name, "Template A");
        assert_eq!(results[0].confidence, 0.9);
        assert_eq!(results[1].name, "Template B");
    }

    // -----------------------------------------------------------------------
    // Test 8: accept suggestion creates template and updates status
    // -----------------------------------------------------------------------
    #[test]
    fn test_accept_suggestion_creates_template() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);


        // Insert a sent message so we can resolve account_id
        let msg = make_sent_message(&account.id, "Test", "body", "alice@test.com", 1700000000, 1);
        let msg_id = InsertMessage::insert(&conn, &msg).unwrap();

        // Create suggestion referencing that message
        insert_suggestion(
            &conn,
            "s1",
            "Weekly Report",
            "Hi {{name}}, here is the report",
            0.9,
            "pending",
            &[&msg_id],
        );

        // Accept the suggestion
        let now = chrono::Utc::now().timestamp();
        let suggestion: TemplateSuggestion = conn
            .query_row(
                "SELECT id, name, subject_pattern, body_pattern, sample_message_ids,
                        pattern_count, confidence, status, created_at, accepted_at, dismissed_at
                 FROM template_suggestions WHERE id = 's1'",
                [],
                |row| row_to_suggestion(row),
            )
            .unwrap();

        assert_eq!(suggestion.status, "pending");

        // Simulate accept: create template + update status
        let template_id = uuid::Uuid::new_v4().to_string();
        let resolved_account_id: String = conn
            .query_row(
                "SELECT account_id FROM messages WHERE id = ?1",
                rusqlite::params![msg_id],
                |row| row.get(0),
            )
            .unwrap();

        conn.execute(
            "INSERT INTO templates (id, account_id, name, subject, body, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                template_id,
                resolved_account_id,
                suggestion.name,
                suggestion.subject_pattern,
                suggestion.body_pattern,
                now,
                now,
            ],
        )
        .unwrap();

        conn.execute(
            "UPDATE template_suggestions SET status = 'accepted', accepted_at = ?1 WHERE id = ?2",
            rusqlite::params![now, "s1"],
        )
        .unwrap();

        // Verify template was created
        let template_name: String = conn
            .query_row(
                "SELECT name FROM templates WHERE id = ?1",
                rusqlite::params![template_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(template_name, "Weekly Report");

        // Verify suggestion status changed
        let status: String = conn
            .query_row(
                "SELECT status FROM template_suggestions WHERE id = 's1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "accepted");

        // Verify accepted_at is set
        let accepted_at: Option<i64> = conn
            .query_row(
                "SELECT accepted_at FROM template_suggestions WHERE id = 's1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(accepted_at.is_some());
    }

    // -----------------------------------------------------------------------
    // Test 9: dismiss suggestion marks as dismissed
    // -----------------------------------------------------------------------
    #[test]
    fn test_dismiss_suggestion() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();


        insert_suggestion(&conn, "s1", "Unwanted Template", "body", 0.6, "pending", &["m1"]);

        let now = chrono::Utc::now().timestamp();
        let updated = conn
            .execute(
                "UPDATE template_suggestions SET status = 'dismissed', dismissed_at = ?1 WHERE id = ?2 AND status = 'pending'",
                rusqlite::params![now, "s1"],
            )
            .unwrap();
        assert_eq!(updated, 1);

        let status: String = conn
            .query_row(
                "SELECT status FROM template_suggestions WHERE id = 's1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "dismissed");

        let dismissed_at: Option<i64> = conn
            .query_row(
                "SELECT dismissed_at FROM template_suggestions WHERE id = 's1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(dismissed_at.is_some());
    }

    // -----------------------------------------------------------------------
    // Test 10: dismiss already dismissed/accepted returns 0 affected rows
    // -----------------------------------------------------------------------
    #[test]
    fn test_dismiss_non_pending_no_op() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();


        insert_suggestion(&conn, "s1", "Already Accepted", "body", 0.8, "accepted", &["m1"]);

        let now = chrono::Utc::now().timestamp();
        let updated = conn
            .execute(
                "UPDATE template_suggestions SET status = 'dismissed', dismissed_at = ?1 WHERE id = ?2 AND status = 'pending'",
                rusqlite::params![now, "s1"],
            )
            .unwrap();
        assert_eq!(updated, 0);

        // Status should remain 'accepted'
        let status: String = conn
            .query_row(
                "SELECT status FROM template_suggestions WHERE id = 's1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "accepted");
    }

    // -----------------------------------------------------------------------
    // Test 11: duplicate scan updates existing suggestion instead of inserting
    // -----------------------------------------------------------------------
    #[test]
    fn test_duplicate_scan_updates_existing() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();


        // Insert initial suggestion
        insert_suggestion(&conn, "s1", "Weekly Report", "Hi {{name}}", 0.7, "pending", &["m1", "m2", "m3"]);

        // Simulate a rescan finding same template name — should update, not duplicate
        let existing_id: Option<String> = conn
            .query_row(
                "SELECT id FROM template_suggestions WHERE name = ?1 AND status = 'pending'",
                rusqlite::params!["Weekly Report"],
                |row| row.get(0),
            )
            .ok();

        assert_eq!(existing_id.as_deref(), Some("s1"));

        // Update the existing one
        let new_sample_ids = serde_json::to_string(&["m1", "m2", "m3", "m4", "m5"]).unwrap();
        conn.execute(
            "UPDATE template_suggestions SET
                body_pattern = ?1, sample_message_ids = ?2, pattern_count = ?3, confidence = ?4
             WHERE id = ?5",
            rusqlite::params![
                "Hi {{name}}, updated template",
                new_sample_ids,
                5i64,
                0.85,
                "s1",
            ],
        )
        .unwrap();

        // Verify only 1 row still exists
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM template_suggestions WHERE name = 'Weekly Report'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);

        // Verify updated fields
        let (body, confidence, pattern_count): (String, f64, i64) = conn
            .query_row(
                "SELECT body_pattern, confidence, pattern_count FROM template_suggestions WHERE id = 's1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(body, "Hi {{name}}, updated template");
        assert!((confidence - 0.85).abs() < 0.01);
        assert_eq!(pattern_count, 5);
    }

    // -----------------------------------------------------------------------
    // Test 12: confidence threshold filtering
    // -----------------------------------------------------------------------
    #[test]
    fn test_confidence_threshold_filtering() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();


        insert_suggestion(&conn, "s1", "High Conf", "body", 0.95, "pending", &["m1", "m2", "m3"]);
        insert_suggestion(&conn, "s2", "Med Conf", "body", 0.7, "pending", &["m4", "m5", "m6"]);
        insert_suggestion(&conn, "s3", "Low Conf", "body", 0.4, "pending", &["m7", "m8", "m9"]);

        // Filter by min_confidence = 0.6
        let mut stmt = conn
            .prepare(
                "SELECT id, name, subject_pattern, body_pattern, sample_message_ids,
                        pattern_count, confidence, status, created_at, accepted_at, dismissed_at
                 FROM template_suggestions
                 WHERE status = 'pending' AND confidence >= ?1
                 ORDER BY confidence DESC",
            )
            .unwrap();
        let results: Vec<TemplateSuggestion> = stmt
            .query_map(rusqlite::params![0.6], |row| row_to_suggestion(row))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].name, "High Conf");
        assert_eq!(results[1].name, "Med Conf");

        // Filter by min_confidence = 0.9
        let results: Vec<TemplateSuggestion> = stmt
            .query_map(rusqlite::params![0.9], |row| row_to_suggestion(row))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "High Conf");
    }

    // -----------------------------------------------------------------------
    // Test 13: status filtering on list (all statuses)
    // -----------------------------------------------------------------------
    #[test]
    fn test_status_filtering_all() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();


        insert_suggestion(&conn, "s1", "Pending", "body", 0.8, "pending", &["m1"]);
        insert_suggestion(&conn, "s2", "Accepted", "body", 0.9, "accepted", &["m2"]);
        insert_suggestion(&conn, "s3", "Dismissed", "body", 0.6, "dismissed", &["m3"]);

        // "all" filter
        let mut stmt = conn
            .prepare(
                "SELECT id, name, subject_pattern, body_pattern, sample_message_ids,
                        pattern_count, confidence, status, created_at, accepted_at, dismissed_at
                 FROM template_suggestions
                 WHERE confidence >= ?1
                 ORDER BY confidence DESC",
            )
            .unwrap();
        let results: Vec<TemplateSuggestion> = stmt
            .query_map(rusqlite::params![0.0], |row| row_to_suggestion(row))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert_eq!(results.len(), 3);

        // Accepted only
        let mut stmt = conn
            .prepare(
                "SELECT id, name, subject_pattern, body_pattern, sample_message_ids,
                        pattern_count, confidence, status, created_at, accepted_at, dismissed_at
                 FROM template_suggestions WHERE status = 'accepted'",
            )
            .unwrap();
        let results: Vec<TemplateSuggestion> = stmt
            .query_map([], |row| row_to_suggestion(row))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "Accepted");
    }

    // -----------------------------------------------------------------------
    // Test 14: scan with no sent emails returns 0 suggestions
    // -----------------------------------------------------------------------
    #[test]
    fn test_no_sent_emails_returns_zero() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let _account = create_test_account(&conn);


        // No sent emails — verify the query returns empty
        let ninety_days_ago = chrono::Utc::now().timestamp() - (90 * 24 * 3600);
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM messages
                 WHERE (folder = 'Sent' OR folder = '[Gmail]/Sent Mail')
                   AND is_deleted = 0 AND is_draft = 0 AND date >= ?1",
                rusqlite::params![ninety_days_ago],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }

    // -----------------------------------------------------------------------
    // Test 15: scan detects patterns from similar sent emails
    // -----------------------------------------------------------------------
    #[test]
    fn test_scan_groups_similar_sent_emails() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);


        let now = chrono::Utc::now().timestamp();

        // Insert 4 similar sent emails (same subject pattern)
        for i in 0..4 {
            let msg = make_sent_message(
                &account.id,
                "Weekly Status Update",
                &format!("Hi Team,\n\nHere is the weekly status for project {}.\n\nBest,\nTemplate Test", i),
                &format!("team{}@example.com", i),
                now - (i * 3600),
                (100 + i) as i64,
            );
            InsertMessage::insert(&conn, &msg);
        }

        // Insert 2 different emails (not enough to form a pattern)
        for i in 0..2 {
            let msg = make_sent_message(
                &account.id,
                &format!("Random topic {}", i),
                &format!("Unrelated body {}", i),
                "other@example.com",
                now - (10000 + i * 3600),
                (200 + i) as i64,
            );
            InsertMessage::insert(&conn, &msg);
        }

        // Query sent emails and group them
        let sent_emails: Vec<(String, String, Option<String>, Option<String>, Option<String>)> = {
            let mut stmt = conn
                .prepare(
                    "SELECT id, account_id, to_addresses, subject, body_text
                     FROM messages
                     WHERE folder = 'Sent' AND is_deleted = 0 AND is_draft = 0
                     ORDER BY date DESC",
                )
                .unwrap();
            stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                ))
            })
            .unwrap()
            .filter_map(|r| r.ok())
            .collect()
        };

        assert_eq!(sent_emails.len(), 6);

        let groups = group_similar_emails(&sent_emails);

        // The 4 "Weekly Status Update" emails should be in one group
        let large_groups: Vec<&EmailGroup> =
            groups.iter().filter(|g| g.message_ids.len() >= 3).collect();
        assert_eq!(large_groups.len(), 1);
        assert_eq!(large_groups[0].message_ids.len(), 4);
    }

    // -----------------------------------------------------------------------
    // Test 16: subject normalization groups Re:/Fwd: variants
    // -----------------------------------------------------------------------
    #[test]
    fn test_normalize_subject_groups_variants() {
        let s1 = normalize_subject("Weekly Update");
        let s2 = normalize_subject("Re: Weekly Update");
        let s3 = normalize_subject("Re: Re: Weekly Update");
        let s4 = normalize_subject("Fwd: Weekly Update");

        // All should normalize to the same key
        assert_eq!(s1, s2);
        assert_eq!(s2, s3);
        assert_eq!(s3, s4);
    }
}

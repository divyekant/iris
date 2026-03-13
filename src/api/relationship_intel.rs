use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::AppState;

// ── Types ────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct ScoreBreakdown {
    pub frequency: f64,
    pub recency: f64,
    pub reply_rate: f64,
    pub bidirectional: f64,
    pub thread_depth: f64,
}

#[derive(Debug, Serialize)]
pub struct ContactStats {
    pub total_emails: i64,
    pub sent_by_you: i64,
    pub received: i64,
    pub avg_response_time_hours: f64,
    pub first_contact: Option<i64>,
    pub last_contact: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct CommunicationPattern {
    pub most_active_day: String,
    pub most_active_hour: i64,
    pub avg_emails_per_week: f64,
}

#[derive(Debug, Serialize)]
pub struct ContactIntelligence {
    pub email: String,
    pub display_name: Option<String>,
    pub relationship_score: f64,
    pub score_breakdown: ScoreBreakdown,
    pub stats: ContactStats,
    pub common_topics: Vec<String>,
    pub communication_pattern: CommunicationPattern,
}

#[derive(Debug, Serialize)]
pub struct ContactSummaryEntry {
    pub email: String,
    pub display_name: Option<String>,
    pub score: f64,
    pub total_emails: i64,
    pub last_contact: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct IntelligenceSummary {
    pub contacts: Vec<ContactSummaryEntry>,
    pub total_contacts: i64,
}

#[derive(Debug, Deserialize)]
pub struct SummaryParams {
    pub limit: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct AiSummaryResponse {
    pub summary: String,
    pub key_insights: Vec<String>,
}

// ── Day-of-week mapping ───────────────────────────────────────────────────────

fn day_name(dow: i64) -> &'static str {
    // SQLite strftime('%w', ...) returns 0=Sunday, 1=Monday, ..., 6=Saturday
    match dow {
        0 => "Sunday",
        1 => "Monday",
        2 => "Tuesday",
        3 => "Wednesday",
        4 => "Thursday",
        5 => "Friday",
        6 => "Saturday",
        _ => "Unknown",
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Extract common topics from subjects: split on whitespace/punctuation,
/// filter stop words, count occurrences, return top N.
pub fn extract_topics_from_subjects(subjects: &[String], top_n: usize) -> Vec<String> {
    const STOP_WORDS: &[&str] = &[
        "re", "fwd", "fw", "the", "a", "an", "and", "or", "but", "in", "on", "at",
        "to", "for", "of", "with", "is", "are", "was", "were", "be", "been", "has",
        "have", "had", "do", "did", "will", "would", "can", "could", "should", "may",
        "might", "shall", "not", "this", "that", "it", "its", "you", "your", "we",
        "our", "my", "i", "he", "she", "they", "their", "from", "by", "about",
        "up", "as", "into", "through", "during", "before", "after", "above",
        "below", "between", "out", "off", "over", "under", "again", "then",
        "once", "here", "there", "when", "where", "why", "how", "all", "both",
        "each", "more", "most", "other", "some", "such", "no", "nor", "so",
        "yet", "too", "very", "just", "than", "any", "also", "new",
        "per", "if", "hi", "hello", "please", "thanks", "thank",
    ];

    let mut word_counts: std::collections::HashMap<String, u32> = std::collections::HashMap::new();

    for subject in subjects {
        // Split on non-alphanumeric, lowercase, filter short/stop words
        for word in subject.split(|c: char| !c.is_alphanumeric()) {
            let w = word.trim().to_lowercase();
            if w.len() < 3 {
                continue;
            }
            if STOP_WORDS.contains(&w.as_str()) {
                continue;
            }
            *word_counts.entry(w).or_insert(0) += 1;
        }
    }

    // Sort by count desc, then alphabetically for determinism
    let mut ranked: Vec<(String, u32)> = word_counts.into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

    ranked
        .into_iter()
        .take(top_n)
        .map(|(w, _)| {
            // Title-case the word
            let mut chars = w.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            }
        })
        .collect()
}

/// Parse the AI model's structured text response into summary + insights.
pub fn parse_ai_summary_response(response: &str, name: &str) -> (String, Vec<String>) {
    if response.is_empty() {
        return (
            format!("Unable to generate a summary for {}.", name),
            vec![
                "No AI provider available.".to_string(),
                "Try configuring an AI provider in Settings.".to_string(),
            ],
        );
    }

    let mut summary = String::new();
    let mut insights: Vec<String> = Vec::new();
    let mut in_insights = false;

    for line in response.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("SUMMARY:") {
            summary = rest.trim().to_string();
        } else if line == "INSIGHTS:" {
            in_insights = true;
        } else if in_insights {
            // Strip leading bullets/dashes
            let cleaned = line
                .trim_start_matches(['•', '-', '*', '–'])
                .trim()
                .to_string();
            if !cleaned.is_empty() {
                insights.push(cleaned);
            }
        }
    }

    // Fallback: if parsing failed, use the first few lines as summary
    if summary.is_empty() {
        summary = response
            .lines()
            .take(3)
            .collect::<Vec<_>>()
            .join(" ")
            .trim()
            .to_string();
    }

    // Ensure we have at least some insights
    if insights.is_empty() {
        insights.push(format!("Contact with {name} found in email history."));
    }

    (summary, insights)
}

// ── Handlers ─────────────────────────────────────────────────────────────────

/// GET /api/contacts/{email}/intelligence
pub async fn get_contact_intelligence(
    State(state): State<Arc<AppState>>,
    Path(email): Path<String>,
) -> Result<Json<ContactIntelligence>, StatusCode> {
    // Axum automatically URL-decodes path params; just normalize case
    let email_lower = email.to_lowercase();

    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Get user account emails for identifying sent messages
    let user_emails: Vec<String> = {
        let mut stmt = conn
            .prepare("SELECT LOWER(email) FROM accounts WHERE is_active = 1")
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        stmt.query_map([], |row| row.get::<_, String>(0))
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .filter_map(|r| r.ok())
            .collect()
    };

    // ── Received count ────────────────────────────────────────────────────────

    let received: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM messages
             WHERE LOWER(from_address) = ?1 AND is_deleted = 0 AND is_draft = 0",
            rusqlite::params![email_lower],
            |row| row.get(0),
        )
        .unwrap_or(0);

    // ── Sent count ────────────────────────────────────────────────────────────
    // Count Sent folder messages from user accounts where contact is a recipient.
    // Use LIKE on to_addresses JSON for efficiency (parameterized, no injection).

    let sent: i64 = {
        let escaped = email_lower.replace('%', "\\%").replace('_', "\\_");
    let contact_like = format!("%{}%", escaped);
        let mut total = 0i64;
        for user_email in &user_emails {
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM messages
                     WHERE LOWER(from_address) = ?1
                       AND folder = 'Sent'
                       AND is_deleted = 0
                       AND is_draft = 0
                       AND LOWER(to_addresses) LIKE ?2 ESCAPE '\\'",
                    rusqlite::params![user_email, contact_like],
                    |row| row.get(0),
                )
                .unwrap_or(0);
            total += count;
        }
        total
    };

    let total_emails = received + sent;

    // Return 404 if no interaction at all
    if total_emails == 0 {
        return Err(StatusCode::NOT_FOUND);
    }

    // ── Display name ─────────────────────────────────────────────────────────

    let display_name: Option<String> = conn
        .query_row(
            "SELECT from_name FROM messages
             WHERE LOWER(from_address) = ?1 AND from_name IS NOT NULL AND is_deleted = 0
             ORDER BY date DESC
             LIMIT 1",
            rusqlite::params![email_lower],
            |row| row.get::<_, Option<String>>(0),
        )
        .ok()
        .flatten()
        .and_then(|n| if n.is_empty() { None } else { Some(n) });

    // ── First / last contact ─────────────────────────────────────────────────

    let escaped = email_lower.replace('%', "\\%").replace('_', "\\_");
    let contact_like = format!("%{}%", escaped);

    let first_contact: Option<i64> = conn
        .query_row(
            "SELECT MIN(date) FROM messages
             WHERE (LOWER(from_address) = ?1 OR LOWER(to_addresses) LIKE ?2)
               AND is_deleted = 0 AND is_draft = 0",
            rusqlite::params![email_lower, &contact_like],
            |row| row.get(0),
        )
        .ok()
        .flatten();

    let last_contact: Option<i64> = conn
        .query_row(
            "SELECT MAX(date) FROM messages
             WHERE (LOWER(from_address) = ?1 OR LOWER(to_addresses) LIKE ?2)
               AND is_deleted = 0 AND is_draft = 0",
            rusqlite::params![email_lower, &contact_like],
            |row| row.get(0),
        )
        .ok()
        .flatten();

    // ── Average response time (user's reply latency to contact's messages) ────
    // For each contact message in a thread, find earliest user reply after it.

    let avg_response_time_hours: f64 = {
        let mut stmt = conn
            .prepare(
                "SELECT thread_id, date FROM messages
                 WHERE LOWER(from_address) = ?1
                   AND thread_id IS NOT NULL
                   AND is_deleted = 0
                   AND is_draft = 0
                 ORDER BY date ASC",
            )
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        let contact_msgs: Vec<(String, i64)> = stmt
            .query_map(rusqlite::params![email_lower], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .filter_map(|r| r.ok())
            .collect();

        let mut response_times: Vec<f64> = Vec::new();

        for (thread_id, contact_date) in &contact_msgs {
            // Find the earliest reply from any user account after this message
            let reply_date: Option<i64> = user_emails
                .iter()
                .filter_map(|ue| {
                    conn.query_row(
                        "SELECT MIN(date) FROM messages
                         WHERE thread_id = ?1
                           AND LOWER(from_address) = ?2
                           AND date > ?3
                           AND is_deleted = 0
                           AND is_draft = 0",
                        rusqlite::params![thread_id, ue, contact_date],
                        |row| row.get::<_, Option<i64>>(0),
                    )
                    .ok()
                    .flatten()
                })
                .min();

            if let Some(reply) = reply_date {
                let diff_hours = (reply - contact_date) as f64 / 3600.0;
                if diff_hours >= 0.0 {
                    response_times.push(diff_hours);
                }
            }
        }

        if response_times.is_empty() {
            0.0
        } else {
            let avg = response_times.iter().sum::<f64>() / response_times.len() as f64;
            (avg * 10.0).round() / 10.0
        }
    };

    // ── Most active day of week ───────────────────────────────────────────────

    let most_active_dow: i64 = {
        let mut stmt = conn
            .prepare(
                "SELECT CAST(strftime('%w', date, 'unixepoch') AS INTEGER) as dow, COUNT(*) as cnt
                 FROM messages
                 WHERE LOWER(from_address) = ?1 AND is_deleted = 0 AND is_draft = 0
                 GROUP BY dow
                 ORDER BY cnt DESC
                 LIMIT 1",
            )
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        stmt.query_row(rusqlite::params![email_lower], |row| row.get::<_, i64>(0))
            .unwrap_or(1) // default Monday
    };

    // ── Most active hour ────────────────────────────────────────────────────

    let most_active_hour: i64 = {
        let mut stmt = conn
            .prepare(
                "SELECT CAST(strftime('%H', date, 'unixepoch') AS INTEGER) as hr, COUNT(*) as cnt
                 FROM messages
                 WHERE LOWER(from_address) = ?1 AND is_deleted = 0 AND is_draft = 0
                 GROUP BY hr
                 ORDER BY cnt DESC
                 LIMIT 1",
            )
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        stmt.query_row(rusqlite::params![email_lower], |row| row.get::<_, i64>(0))
            .unwrap_or(9) // default 9am
    };

    // ── Avg emails per week ──────────────────────────────────────────────────

    let avg_emails_per_week: f64 = if let (Some(first), Some(last)) = (first_contact, last_contact) {
        let span_seconds = (last - first).max(0) as f64;
        let weeks = (span_seconds / (7.0 * 86400.0)).max(1.0);
        let avg = (total_emails as f64) / weeks;
        (avg * 10.0).round() / 10.0
    } else {
        0.0
    };

    // ── Common topics ────────────────────────────────────────────────────────

    let subjects: Vec<String> = {
        let mut stmt = conn
            .prepare(
                "SELECT DISTINCT subject FROM messages
                 WHERE (LOWER(from_address) = ?1 OR LOWER(to_addresses) LIKE ?2)
                   AND subject IS NOT NULL
                   AND is_deleted = 0
                   AND is_draft = 0
                 LIMIT 200",
            )
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        stmt.query_map(
            rusqlite::params![email_lower, &contact_like],
            |row| row.get::<_, String>(0),
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .filter_map(|r| r.ok())
        .collect()
    };

    let common_topics = extract_topics_from_subjects(&subjects, 5);

    // ── Relationship score from relationship_scores cache ─────────────────────

    let score_row: Option<(f64, f64, f64, f64, f64, f64)> = conn
        .query_row(
            "SELECT score, frequency_score, recency_score, reply_rate_score,
                    bidirectional_score, thread_depth_score
             FROM relationship_scores WHERE email = ?1",
            rusqlite::params![email_lower],
            |row| {
                Ok((
                    row.get::<_, f64>(0)?,
                    row.get::<_, f64>(1)?,
                    row.get::<_, f64>(2)?,
                    row.get::<_, f64>(3)?,
                    row.get::<_, f64>(4)?,
                    row.get::<_, f64>(5)?,
                ))
            },
        )
        .ok();

    let (relationship_score, score_breakdown) =
        if let Some((score, freq, rec, reply, bidir, depth)) = score_row {
            (
                score,
                ScoreBreakdown {
                    frequency: freq,
                    recency: rec,
                    reply_rate: reply,
                    bidirectional: bidir,
                    thread_depth: depth,
                },
            )
        } else {
            (
                0.0,
                ScoreBreakdown {
                    frequency: 0.0,
                    recency: 0.0,
                    reply_rate: 0.0,
                    bidirectional: 0.0,
                    thread_depth: 0.0,
                },
            )
        };

    Ok(Json(ContactIntelligence {
        email: email_lower,
        display_name,
        relationship_score,
        score_breakdown,
        stats: ContactStats {
            total_emails,
            sent_by_you: sent,
            received,
            avg_response_time_hours,
            first_contact,
            last_contact,
        },
        common_topics,
        communication_pattern: CommunicationPattern {
            most_active_day: day_name(most_active_dow).to_string(),
            most_active_hour,
            avg_emails_per_week,
        },
    }))
}

/// GET /api/contacts/intelligence/summary
pub async fn get_intelligence_summary(
    State(state): State<Arc<AppState>>,
    Query(params): Query<SummaryParams>,
) -> Result<Json<IntelligenceSummary>, StatusCode> {
    let limit = params.limit.unwrap_or(10).clamp(1, 50);
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Count total distinct contacts with a relationship score
    let total_contacts: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM relationship_scores",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    // Top contacts by score
    let mut stmt = conn
        .prepare(
            "SELECT email, score FROM relationship_scores
             ORDER BY score DESC
             LIMIT ?1",
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let rows: Vec<(String, f64)> = stmt
        .query_map(rusqlite::params![limit], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .filter_map(|r| r.ok())
        .collect();

    let mut contacts: Vec<ContactSummaryEntry> = Vec::with_capacity(rows.len());
    for (email, score) in rows {
        // Most recent display name for this contact
        let display_name: Option<String> = conn
            .query_row(
                "SELECT from_name FROM messages
                 WHERE LOWER(from_address) = ?1 AND from_name IS NOT NULL AND is_deleted = 0
                 ORDER BY date DESC LIMIT 1",
                rusqlite::params![email],
                |row| row.get::<_, Option<String>>(0),
            )
            .ok()
            .flatten()
            .and_then(|n| if n.is_empty() { None } else { Some(n) });

        let (total_emails, last_contact): (i64, Option<i64>) = conn
            .query_row(
                "SELECT COUNT(*), MAX(date) FROM messages
                 WHERE LOWER(from_address) = ?1 AND is_deleted = 0 AND is_draft = 0",
                rusqlite::params![email],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<i64>>(1)?)),
            )
            .unwrap_or((0, None));

        contacts.push(ContactSummaryEntry {
            email,
            display_name,
            score,
            total_emails,
            last_contact,
        });
    }

    Ok(Json(IntelligenceSummary {
        contacts,
        total_contacts,
    }))
}

/// POST /api/contacts/{email}/intelligence/ai-summary
pub async fn get_contact_ai_summary(
    State(state): State<Arc<AppState>>,
    Path(email): Path<String>,
) -> Result<Json<AiSummaryResponse>, StatusCode> {
    let email_lower = email.to_lowercase();

    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Verify the contact exists
    let exists: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM messages
             WHERE LOWER(from_address) = ?1 AND is_deleted = 0 AND is_draft = 0",
            rusqlite::params![email_lower],
            |row| row.get::<_, bool>(0),
        )
        .unwrap_or(false);

    if !exists {
        return Err(StatusCode::NOT_FOUND);
    }

    // Gather context for the AI prompt
    let display_name: Option<String> = conn
        .query_row(
            "SELECT from_name FROM messages
             WHERE LOWER(from_address) = ?1 AND from_name IS NOT NULL AND is_deleted = 0
             ORDER BY date DESC LIMIT 1",
            rusqlite::params![email_lower],
            |row| row.get::<_, Option<String>>(0),
        )
        .ok()
        .flatten()
        .and_then(|n| if n.is_empty() { None } else { Some(n) });

    let name = display_name.as_deref().unwrap_or(&email_lower).to_string();

    let (total_received, first_date, last_date): (i64, Option<i64>, Option<i64>) = conn
        .query_row(
            "SELECT COUNT(*), MIN(date), MAX(date) FROM messages
             WHERE LOWER(from_address) = ?1 AND is_deleted = 0 AND is_draft = 0",
            rusqlite::params![email_lower],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Option<i64>>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                ))
            },
        )
        .unwrap_or((0, None, None));

    let score_info: Option<(f64, f64, f64)> = conn
        .query_row(
            "SELECT score, reply_rate_score, bidirectional_score
             FROM relationship_scores WHERE email = ?1",
            rusqlite::params![email_lower],
            |row| {
                Ok((
                    row.get::<_, f64>(0)?,
                    row.get::<_, f64>(1)?,
                    row.get::<_, f64>(2)?,
                ))
            },
        )
        .ok();

    let subjects: Vec<String> = {
        let mut stmt = conn
            .prepare(
                "SELECT DISTINCT subject FROM messages
                 WHERE LOWER(from_address) = ?1
                   AND subject IS NOT NULL
                   AND is_deleted = 0
                   AND is_draft = 0
                 LIMIT 50",
            )
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        stmt.query_map(rusqlite::params![email_lower], |row| {
            row.get::<_, String>(0)
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .filter_map(|r| r.ok())
        .collect()
    };

    let topics = extract_topics_from_subjects(&subjects, 5);

    // Format dates for the prompt
    let first_str = first_date
        .and_then(|d| chrono::DateTime::from_timestamp(d, 0))
        .map(|dt| dt.format("%B %Y").to_string())
        .unwrap_or_else(|| "an unknown date".to_string());

    let last_str = last_date
        .and_then(|d| chrono::DateTime::from_timestamp(d, 0))
        .map(|dt| dt.format("%B %Y").to_string())
        .unwrap_or_else(|| "recently".to_string());

    let topics_str = if topics.is_empty() {
        "various topics".to_string()
    } else {
        topics.join(", ")
    };

    let score_desc = if let Some((score, reply_rate, bidir)) = score_info {
        format!(
            "Relationship score: {:.0}%. Reply rate: {:.0}%. Communication is {}.",
            score * 100.0,
            reply_rate * 100.0,
            if bidir > 0.5 { "bidirectional" } else { "mostly one-way" }
        )
    } else {
        "No relationship score computed yet.".to_string()
    };

    let prompt = format!(
        "Generate a relationship intelligence summary for the email contact: {name} ({email_lower}).\n\
         \n\
         Data:\n\
         - Total emails received from them: {total_received}\n\
         - First contact: {first_str}\n\
         - Most recent contact: {last_str}\n\
         - Common discussion topics: {topics_str}\n\
         - {score_desc}\n\
         \n\
         Write a 2-3 sentence natural language summary of this relationship from the user's perspective, \
         then provide exactly 3 key insights as short bullet points (no bullet symbols, one per line).\n\
         \n\
         Format your response as:\n\
         SUMMARY: <the summary sentences>\n\
         INSIGHTS:\n\
         <insight 1>\n\
         <insight 2>\n\
         <insight 3>"
    );

    let system = "You are an email relationship analyst. Be factual, concise, and professional. \
                  Base your analysis only on the data provided. Do not invent details.";

    let ai_response = state
        .providers
        .generate(&prompt, Some(system))
        .await
        .unwrap_or_default();

    let (summary, key_insights) = parse_ai_summary_response(&ai_response, &name);

    Ok(Json(AiSummaryResponse {
        summary,
        key_insights,
    }))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

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
            email: "user@example.com".to_string(),
            display_name: Some("Test User".to_string()),
            imap_host: Some("imap.gmail.com".to_string()),
            imap_port: Some(993),
            smtp_host: Some("smtp.gmail.com".to_string()),
            smtp_port: Some(587),
            username: Some("user@example.com".to_string()),
            password: None,
        };
        Account::create(conn, &input)
    }

    fn make_message(
        account_id: &str,
        from: &str,
        to: &str,
        subject: &str,
        folder: &str,
        date: i64,
        thread_id: Option<&str>,
    ) -> InsertMessage {
        InsertMessage {
            account_id: account_id.to_string(),
            message_id: Some(format!("<intel-{subject}-{date}@test>")),
            thread_id: thread_id.map(|s| s.to_string()),
            folder: folder.to_string(),
            from_address: Some(from.to_string()),
            from_name: Some(from.split('@').next().unwrap_or(from).to_string()),
            to_addresses: Some(format!(r#"["{}"]"#, to)),
            cc_addresses: None,
            bcc_addresses: None,
            subject: Some(subject.to_string()),
            date: Some(date),
            snippet: Some("test snippet".to_string()),
            body_text: Some("test body".to_string()),
            body_html: None,
            is_read: false,
            is_starred: false,
            is_draft: false,
            labels: None,
            uid: Some(date),
            modseq: None,
            raw_headers: None,
            has_attachments: false,
            attachment_names: None,
            size_bytes: Some(100),
            list_unsubscribe: None,
            list_unsubscribe_post: false,
        }
    }

    fn make_test_state(
        pool: crate::db::DbPool,
    ) -> Arc<AppState> {
        Arc::new(AppState {
            db: pool,
            config: crate::config::Config::from_env(),
            ws_hub: crate::ws::hub::WsHub::new(),
            providers: crate::ai::provider::ProviderPool::new(vec![]),
            memories: crate::ai::memories::MemoriesClient::new(
                "http://localhost:8900",
                None,
            ),
            session_token: "test-token".to_string(),
        })
    }

    // ── Unit tests (no AppState) ──────────────────────────────────────────────

    // Test 1: extract_topics_from_subjects filters stop words and picks top N
    #[test]
    fn test_extract_topics_basic() {
        let subjects = vec![
            "Q3 Planning Meeting".to_string(),
            "Re: Q3 Planning".to_string(),
            "Budget Review for Q3".to_string(),
            "Team Sync - Weekly".to_string(),
            "Q3 Budget Approval".to_string(),
        ];
        let topics = extract_topics_from_subjects(&subjects, 5);

        // Should return some topics
        assert!(!topics.is_empty());
        // All returned topics should be title-cased
        for topic in &topics {
            let first_char = topic.chars().next().unwrap();
            assert!(first_char.is_uppercase(), "Expected title case, got: {topic}");
        }
        // q3 and budget should rank highly
        let lower_topics: Vec<String> = topics.iter().map(|t| t.to_lowercase()).collect();
        assert!(
            lower_topics.contains(&"q3".to_string())
                || lower_topics.iter().any(|t| t.contains("budget")),
            "Expected q3 or budget in topics, got: {:?}",
            topics
        );
    }

    // Test 2: extract_topics_from_subjects with empty input
    #[test]
    fn test_extract_topics_empty() {
        let topics = extract_topics_from_subjects(&[], 5);
        assert!(topics.is_empty());
    }

    // Test 3: extract_topics_from_subjects respects top_n limit
    #[test]
    fn test_extract_topics_limit() {
        let subjects: Vec<String> = (0..20)
            .map(|i| format!("word{i} discussion planning review"))
            .collect();
        let topics = extract_topics_from_subjects(&subjects, 3);
        assert!(topics.len() <= 3);
    }

    // Test 4: extract_topics_from_subjects filters stop words
    #[test]
    fn test_extract_topics_filters_stop_words() {
        let subjects = vec![
            "Re: the meeting about the budget".to_string(),
            "Fwd: from and to or but".to_string(),
        ];
        let topics = extract_topics_from_subjects(&subjects, 10);
        let lower: Vec<String> = topics.iter().map(|t| t.to_lowercase()).collect();
        // Stop words should not appear
        assert!(!lower.contains(&"the".to_string()));
        assert!(!lower.contains(&"re".to_string()));
        assert!(!lower.contains(&"fwd".to_string()));
        assert!(!lower.contains(&"and".to_string()));
    }

    // Test 5: parse_ai_summary_response with well-formed response
    #[test]
    fn test_parse_ai_summary_well_formed() {
        let response = "SUMMARY: Sarah is a key collaborator you work with regularly.\n\
                        INSIGHTS:\n\
                        Strong bidirectional communication pattern\n\
                        Primarily discusses project planning\n\
                        Quick responder with 2 hour average";
        let (summary, insights) = parse_ai_summary_response(response, "Sarah");
        assert_eq!(summary, "Sarah is a key collaborator you work with regularly.");
        assert_eq!(insights.len(), 3);
        assert_eq!(insights[0], "Strong bidirectional communication pattern");
        assert_eq!(insights[1], "Primarily discusses project planning");
    }

    // Test 6: parse_ai_summary_response with empty response falls back gracefully
    #[test]
    fn test_parse_ai_summary_empty() {
        let (summary, insights) = parse_ai_summary_response("", "Alice");
        assert!(summary.contains("Alice"));
        assert!(!insights.is_empty());
    }

    // Test 7: parse_ai_summary_response strips bullet markers
    #[test]
    fn test_parse_ai_summary_bullets_stripped() {
        let response =
            "SUMMARY: Great contact.\nINSIGHTS:\n- Insight one\n• Insight two\n* Insight three";
        let (_summary, insights) = parse_ai_summary_response(response, "Bob");
        assert_eq!(insights[0], "Insight one");
        assert_eq!(insights[1], "Insight two");
        assert_eq!(insights[2], "Insight three");
    }

    // Test 8: parse_ai_summary_response with malformed response uses fallback
    #[test]
    fn test_parse_ai_summary_malformed_fallback() {
        let response = "This contact is very important to you.";
        let (summary, insights) = parse_ai_summary_response(response, "Carol");
        // Fallback: first lines become summary
        assert!(!summary.is_empty());
        // At least one insight from fallback
        assert!(!insights.is_empty());
    }

    // Test 9: day_name covers all 7 days and unknown
    #[test]
    fn test_day_name() {
        assert_eq!(day_name(0), "Sunday");
        assert_eq!(day_name(1), "Monday");
        assert_eq!(day_name(2), "Tuesday");
        assert_eq!(day_name(3), "Wednesday");
        assert_eq!(day_name(4), "Thursday");
        assert_eq!(day_name(5), "Friday");
        assert_eq!(day_name(6), "Saturday");
        assert_eq!(day_name(7), "Unknown");
        assert_eq!(day_name(-1), "Unknown");
    }

    // ── Integration tests (with AppState + DB) ────────────────────────────────

    // Test 10: get_contact_intelligence returns NOT_FOUND for unknown contact
    #[tokio::test]
    async fn test_contact_intelligence_not_found() {
        let pool = create_test_pool();
        let _ = pool.get().unwrap();
        let state = make_test_state(pool);

        let result = get_contact_intelligence(
            State(state),
            Path("unknown@nobody.com".to_string()),
        )
        .await;

        assert_eq!(result.unwrap_err(), StatusCode::NOT_FOUND);
    }

    // Test 11: get_contact_intelligence returns data for known contact
    #[tokio::test]
    async fn test_contact_intelligence_found() {
        let pool = create_test_pool();
        let account = {
            let conn = pool.get().unwrap();
            create_test_account(&conn)
        };

        let now = 1700000000_i64;
        {
            let conn = pool.get().unwrap();
            for i in 0..5 {
                let msg = make_message(
                    &account.id,
                    "alice@example.com",
                    "user@example.com",
                    &format!("Budget Review {i}"),
                    "INBOX",
                    now + i * 3600,
                    None,
                );
                InsertMessage::insert(&conn, &msg);
            }
        }

        let state = make_test_state(pool);

        let result = get_contact_intelligence(
            State(state),
            Path("alice@example.com".to_string()),
        )
        .await;

        assert!(result.is_ok(), "Expected Ok, got: {:?}", result.unwrap_err());
        let Json(intel) = result.unwrap();
        assert_eq!(intel.email, "alice@example.com");
        assert_eq!(intel.stats.received, 5);
        assert_eq!(intel.stats.sent_by_you, 0);
        assert_eq!(intel.stats.total_emails, 5);
        assert!(intel.stats.first_contact.is_some());
        assert!(intel.stats.last_contact.is_some());
    }

    // Test 12: email comparison is case-insensitive (LOWER)
    #[tokio::test]
    async fn test_email_case_insensitive() {
        let pool = create_test_pool();
        let account = {
            let conn = pool.get().unwrap();
            create_test_account(&conn)
        };

        {
            let conn = pool.get().unwrap();
            // Insert message with mixed-case from_address
            let msg = make_message(
                &account.id,
                "Alice@EXAMPLE.COM", // mixed case
                "user@example.com",
                "Hello",
                "INBOX",
                1700000000,
                None,
            );
            InsertMessage::insert(&conn, &msg);
        }

        let state = make_test_state(pool);

        // Query with lowercase — LOWER() in SQL makes this case-insensitive
        let result = get_contact_intelligence(
            State(state),
            Path("alice@example.com".to_string()),
        )
        .await;

        assert!(result.is_ok());
        let Json(intel) = result.unwrap();
        assert_eq!(intel.stats.received, 1);
    }

    // Test 13: SQL injection protection via parameterized queries
    #[tokio::test]
    async fn test_email_sql_injection_protection() {
        let pool = create_test_pool();
        let _ = pool.get().unwrap();
        let state = make_test_state(pool);

        // Attempt injection via email path param — should return 404, not panic or 500
        let result = get_contact_intelligence(
            State(state),
            Path("'; DROP TABLE messages; --@evil.com".to_string()),
        )
        .await;

        assert_eq!(result.unwrap_err(), StatusCode::NOT_FOUND);
    }

    // Test 14: get_intelligence_summary with limit parameter
    #[tokio::test]
    async fn test_intelligence_summary_limit() {
        let pool = create_test_pool();
        let account = {
            let conn = pool.get().unwrap();
            create_test_account(&conn)
        };

        {
            let conn = pool.get().unwrap();
            for i in 1..=10i64 {
                let email = format!("contact{}@example.com", i);
                conn.execute(
                    "INSERT OR REPLACE INTO relationship_scores
                     (email, score, frequency_score, recency_score, reply_rate_score,
                      bidirectional_score, thread_depth_score, computed_at)
                     VALUES (?1, ?2, 0.5, 0.5, 0.5, 0.5, 0.5, 1700000000)",
                    rusqlite::params![email, i as f64 / 10.0],
                )
                .unwrap();

                let msg = make_message(
                    &account.id,
                    &email,
                    "user@example.com",
                    "Subject",
                    "INBOX",
                    1700000000 + i,
                    None,
                );
                InsertMessage::insert(&conn, &msg);
            }
        }

        let state = make_test_state(pool);

        let result = get_intelligence_summary(
            State(state),
            Query(SummaryParams { limit: Some(5) }),
        )
        .await;

        assert!(result.is_ok());
        let Json(summary) = result.unwrap();
        assert_eq!(summary.contacts.len(), 5);
        assert_eq!(summary.total_contacts, 10);

        // Verify descending score order
        let scores: Vec<f64> = summary.contacts.iter().map(|c| c.score).collect();
        for w in scores.windows(2) {
            assert!(w[0] >= w[1], "Expected descending scores, got {:?}", scores);
        }
    }

    // Test 15: get_intelligence_summary enforces max 50 limit
    #[tokio::test]
    async fn test_intelligence_summary_max_limit() {
        let pool = create_test_pool();
        let _ = pool.get().unwrap();
        let state = make_test_state(pool);

        // limit=200 should be clamped to 50
        let result = get_intelligence_summary(
            State(state),
            Query(SummaryParams { limit: Some(200) }),
        )
        .await;

        assert!(result.is_ok());
        let Json(summary) = result.unwrap();
        // Should return at most 50 contacts
        assert!(summary.contacts.len() <= 50);
    }

    // Test 16: get_intelligence_summary returns empty contacts when no scores
    #[tokio::test]
    async fn test_intelligence_summary_empty() {
        let pool = create_test_pool();
        let _ = pool.get().unwrap();
        let state = make_test_state(pool);

        let result = get_intelligence_summary(
            State(state),
            Query(SummaryParams { limit: None }),
        )
        .await;

        assert!(result.is_ok());
        let Json(summary) = result.unwrap();
        assert_eq!(summary.contacts.len(), 0);
        assert_eq!(summary.total_contacts, 0);
    }
}

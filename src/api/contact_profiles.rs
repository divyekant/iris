use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::AppState;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContactProfile {
    pub id: i64,
    pub account_id: String,
    pub email_address: String,
    pub display_name: Option<String>,
    pub organization: Option<String>,
    pub first_seen_at: Option<String>,
    pub last_seen_at: Option<String>,
    pub total_emails_from: i64,
    pub total_emails_to: i64,
    pub avg_response_time_hours: Option<f64>,
    pub top_categories: Option<String>,
    pub communication_style: Option<String>,
    pub ai_summary: Option<String>,
    pub profile_data: String,
    pub generated_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
pub struct ListProfilesParams {
    pub account_id: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct ListProfilesResponse {
    pub profiles: Vec<ContactProfile>,
    pub total: i64,
}

#[derive(Debug, Serialize)]
pub struct GenerateResponse {
    pub profile: ContactProfile,
    pub generated: bool,
}

#[derive(Debug, Deserialize)]
pub struct GenerateAllParams {
    pub account_id: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct GenerateAllResponse {
    pub generated: usize,
    pub profiles: Vec<ContactProfile>,
}

#[derive(Debug, Deserialize)]
pub struct SearchProfilesParams {
    pub q: Option<String>,
    pub account_id: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct SearchProfilesResponse {
    pub profiles: Vec<ContactProfile>,
    pub total: i64,
    pub query: String,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn row_to_profile(row: &rusqlite::Row) -> rusqlite::Result<ContactProfile> {
    Ok(ContactProfile {
        id: row.get("id")?,
        account_id: row.get("account_id")?,
        email_address: row.get("email_address")?,
        display_name: row.get("display_name")?,
        organization: row.get("organization")?,
        first_seen_at: row.get("first_seen_at")?,
        last_seen_at: row.get("last_seen_at")?,
        total_emails_from: row.get("total_emails_from")?,
        total_emails_to: row.get("total_emails_to")?,
        avg_response_time_hours: row.get("avg_response_time_hours")?,
        top_categories: row.get("top_categories")?,
        communication_style: row.get("communication_style")?,
        ai_summary: row.get("ai_summary")?,
        profile_data: row.get("profile_data")?,
        generated_at: row.get("generated_at")?,
        updated_at: row.get("updated_at")?,
    })
}

/// Detect communication style from body text samples.
/// Looks at greeting/closing patterns across messages.
fn detect_communication_style(bodies: &[String]) -> String {
    if bodies.is_empty() {
        return "mixed".to_string();
    }

    let mut formal_signals = 0i32;
    let mut casual_signals = 0i32;

    for body in bodies {
        let lower = body.to_lowercase();
        // Formal indicators
        if lower.starts_with("dear ") || lower.contains("sincerely") || lower.contains("regards,")
            || lower.contains("best regards") || lower.contains("kind regards")
            || lower.contains("respectfully") || lower.contains("cordially")
        {
            formal_signals += 1;
        }
        // Casual indicators
        if lower.starts_with("hey ") || lower.starts_with("hi ") || lower.starts_with("yo ")
            || lower.contains("cheers") || lower.contains("thanks!")
            || lower.contains("lol") || lower.contains("haha")
            || lower.contains("btw") || lower.contains("gonna")
        {
            casual_signals += 1;
        }
    }

    let total = formal_signals + casual_signals;
    if total == 0 {
        return "mixed".to_string();
    }

    let formal_ratio = formal_signals as f64 / total as f64;
    if formal_ratio >= 0.7 {
        "formal".to_string()
    } else if formal_ratio <= 0.3 {
        "casual".to_string()
    } else {
        "mixed".to_string()
    }
}

/// Compute average response time in hours from thread reply pairs.
/// Looks at messages in the same thread where one is from the contact
/// and the next is to the contact (or vice versa), computing the time gap.
fn compute_avg_response_time(
    conn: &r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>,
    account_id: &str,
    email: &str,
) -> Option<f64> {
    // Get all thread_ids involving this contact
    let mut stmt = conn
        .prepare(
            "SELECT DISTINCT thread_id FROM messages
             WHERE account_id = ?1 AND thread_id IS NOT NULL
               AND (from_address = ?2 OR to_addresses LIKE ?3)
             ORDER BY date ASC",
        )
        .ok()?;

    let like_email = format!("%{}%", email);
    let thread_ids: Vec<String> = stmt
        .query_map(rusqlite::params![account_id, email, like_email], |row| {
            row.get(0)
        })
        .ok()?
        .filter_map(|r| r.ok())
        .collect();

    let mut response_times = Vec::new();

    for thread_id in &thread_ids {
        // Get messages in this thread ordered by date
        let mut msg_stmt = conn
            .prepare(
                "SELECT from_address, date FROM messages
                 WHERE account_id = ?1 AND thread_id = ?2 AND date IS NOT NULL
                 ORDER BY date ASC",
            )
            .ok()?;

        let msgs: Vec<(String, i64)> = msg_stmt
            .query_map(rusqlite::params![account_id, thread_id], |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                    row.get::<_, i64>(1)?,
                ))
            })
            .ok()?
            .filter_map(|r| r.ok())
            .collect();

        // Look for reply pairs: contact sends, then someone else replies (or vice versa)
        for window in msgs.windows(2) {
            let (ref sender_a, date_a) = window[0];
            let (ref sender_b, date_b) = window[1];

            let a_is_contact = sender_a == email;
            let b_is_contact = sender_b == email;

            // A reply pair: one from contact, next from someone else (or vice versa)
            if a_is_contact != b_is_contact && date_b > date_a {
                let diff_hours = (date_b - date_a) as f64 / 3600.0;
                // Cap at 7 days to avoid skewing from long gaps
                if diff_hours <= 168.0 {
                    response_times.push(diff_hours);
                }
            }
        }
    }

    if response_times.is_empty() {
        return None;
    }

    let sum: f64 = response_times.iter().sum();
    Some((sum / response_times.len() as f64 * 10.0).round() / 10.0)
}

/// Generate or update a contact profile from email data.
fn generate_profile_for_email(
    conn: &r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>,
    account_id: &str,
    email: &str,
) -> Result<ContactProfile, StatusCode> {
    let like_email = format!("%{}%", email);

    // Count emails from this contact
    let total_from: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM messages WHERE account_id = ?1 AND from_address = ?2 AND is_deleted = 0",
            rusqlite::params![account_id, email],
            |row| row.get(0),
        )
        .unwrap_or(0);

    // Count emails to this contact
    let total_to: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM messages WHERE account_id = ?1 AND to_addresses LIKE ?2 AND is_deleted = 0",
            rusqlite::params![account_id, like_email],
            |row| row.get(0),
        )
        .unwrap_or(0);

    if total_from == 0 && total_to == 0 {
        return Err(StatusCode::NOT_FOUND);
    }

    // First and last seen dates (unix timestamps -> ISO 8601)
    let first_seen: Option<String> = conn
        .query_row(
            "SELECT datetime(MIN(date), 'unixepoch') FROM messages
             WHERE account_id = ?1 AND (from_address = ?2 OR to_addresses LIKE ?3) AND date IS NOT NULL AND is_deleted = 0",
            rusqlite::params![account_id, email, like_email],
            |row| row.get(0),
        )
        .ok();

    let last_seen: Option<String> = conn
        .query_row(
            "SELECT datetime(MAX(date), 'unixepoch') FROM messages
             WHERE account_id = ?1 AND (from_address = ?2 OR to_addresses LIKE ?3) AND date IS NOT NULL AND is_deleted = 0",
            rusqlite::params![account_id, email, like_email],
            |row| row.get(0),
        )
        .ok();

    // Display name: most recent from_name where this contact is the sender
    let display_name: Option<String> = conn
        .query_row(
            "SELECT from_name FROM messages
             WHERE account_id = ?1 AND from_address = ?2 AND from_name IS NOT NULL AND from_name != '' AND is_deleted = 0
             ORDER BY date DESC LIMIT 1",
            rusqlite::params![account_id, email],
            |row| row.get(0),
        )
        .ok();

    // Organization: extract domain from email (simple heuristic)
    let organization = email
        .split('@')
        .nth(1)
        .and_then(|domain| {
            // Skip common free email providers
            let free_providers = [
                "gmail.com", "yahoo.com", "hotmail.com", "outlook.com",
                "aol.com", "icloud.com", "protonmail.com", "mail.com",
                "live.com", "msn.com",
            ];
            if free_providers.contains(&domain) {
                None
            } else {
                Some(domain.to_string())
            }
        });

    // Top categories from ai_category distribution
    let mut cat_stmt = conn
        .prepare(
            "SELECT ai_category, COUNT(*) as cnt FROM messages
             WHERE account_id = ?1 AND (from_address = ?2 OR to_addresses LIKE ?3)
               AND ai_category IS NOT NULL AND is_deleted = 0
             GROUP BY ai_category ORDER BY cnt DESC LIMIT 5",
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let categories: Vec<String> = cat_stmt
        .query_map(rusqlite::params![account_id, email, like_email], |row| {
            row.get::<_, String>(0)
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .filter_map(|r| r.ok())
        .collect();

    let top_categories = if categories.is_empty() {
        None
    } else {
        Some(serde_json::to_string(&categories).unwrap_or_else(|_| "[]".to_string()))
    };

    // Communication style from body_text samples
    let mut body_stmt = conn
        .prepare(
            "SELECT body_text FROM messages
             WHERE account_id = ?1 AND from_address = ?2 AND body_text IS NOT NULL AND is_deleted = 0
             ORDER BY date DESC LIMIT 20",
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let bodies: Vec<String> = body_stmt
        .query_map(rusqlite::params![account_id, email], |row| {
            row.get::<_, String>(0)
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .filter_map(|r| r.ok())
        .collect();

    let communication_style = detect_communication_style(&bodies);

    // Avg response time
    let avg_response_time = compute_avg_response_time(conn, account_id, email);

    // UPSERT the profile
    conn.execute(
        "INSERT INTO contact_profiles (
            account_id, email_address, display_name, organization,
            first_seen_at, last_seen_at, total_emails_from, total_emails_to,
            avg_response_time_hours, top_categories, communication_style,
            profile_data, generated_at, updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, '{}', datetime('now'), datetime('now'))
        ON CONFLICT(account_id, email_address) DO UPDATE SET
            display_name = ?3,
            organization = ?4,
            first_seen_at = ?5,
            last_seen_at = ?6,
            total_emails_from = ?7,
            total_emails_to = ?8,
            avg_response_time_hours = ?9,
            top_categories = ?10,
            communication_style = ?11,
            updated_at = datetime('now')",
        rusqlite::params![
            account_id,
            email,
            display_name,
            organization,
            first_seen,
            last_seen,
            total_from,
            total_to,
            avg_response_time,
            top_categories,
            communication_style,
        ],
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Read back the profile
    conn.query_row(
        "SELECT id, account_id, email_address, display_name, organization,
                first_seen_at, last_seen_at, total_emails_from, total_emails_to,
                avg_response_time_hours, top_categories, communication_style,
                ai_summary, profile_data, generated_at, updated_at
         FROM contact_profiles
         WHERE account_id = ?1 AND email_address = ?2",
        rusqlite::params![account_id, email],
        row_to_profile,
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

// ---------------------------------------------------------------------------
// Endpoints
// ---------------------------------------------------------------------------

/// POST /api/contacts/profiles/generate/:email — generate/update a profile
pub async fn generate_profile(
    State(state): State<Arc<AppState>>,
    Path(email): Path<String>,
    Query(params): Query<GenerateAllParams>,
) -> Result<Json<GenerateResponse>, StatusCode> {
    let email = email.to_lowercase().trim().to_string();
    if email.is_empty() || !email.contains('@') {
        return Err(StatusCode::BAD_REQUEST);
    }

    let account_id = params.account_id.ok_or(StatusCode::BAD_REQUEST)?;

    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut profile = generate_profile_for_email(&conn, &account_id, &email)?;

    // Optionally generate AI summary if providers are available
    if state.providers.has_providers() {
        let prompt = format!(
            "Generate a brief professional profile summary (2-3 sentences) for a contact based on email interactions.\n\n\
             Contact: {} ({})\n\
             Organization: {}\n\
             Total emails from them: {}\n\
             Total emails to them: {}\n\
             First contact: {}\n\
             Last contact: {}\n\
             Top categories: {}\n\
             Communication style: {}\n\
             Avg response time: {} hours\n\n\
             Write a concise summary of this contact's communication patterns and relationship.",
            profile.display_name.as_deref().unwrap_or("Unknown"),
            profile.email_address,
            profile.organization.as_deref().unwrap_or("Unknown"),
            profile.total_emails_from,
            profile.total_emails_to,
            profile.first_seen_at.as_deref().unwrap_or("Unknown"),
            profile.last_seen_at.as_deref().unwrap_or("Unknown"),
            profile.top_categories.as_deref().unwrap_or("[]"),
            profile.communication_style.as_deref().unwrap_or("mixed"),
            profile.avg_response_time_hours.map(|h| format!("{:.1}", h)).unwrap_or_else(|| "N/A".to_string()),
        );

        if let Some(summary) = state.providers.generate(&prompt, Some("You are a professional contact profile summarizer. Be concise and factual.")).await {
            let trimmed = summary.trim().to_string();
            conn.execute(
                "UPDATE contact_profiles SET ai_summary = ?1, updated_at = datetime('now')
                 WHERE account_id = ?2 AND email_address = ?3",
                rusqlite::params![trimmed, account_id, email],
            )
            .ok();
            profile.ai_summary = Some(trimmed);
        }
    }

    Ok(Json(GenerateResponse {
        profile,
        generated: true,
    }))
}

/// GET /api/contacts/profiles — list all profiles
pub async fn list_profiles(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListProfilesParams>,
) -> Result<Json<ListProfilesResponse>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let limit = params.limit.unwrap_or(50).min(500);
    let offset = params.offset.unwrap_or(0).max(0);

    let (profiles, total) = if let Some(ref account_id) = params.account_id {
        let mut stmt = conn
            .prepare(
                "SELECT id, account_id, email_address, display_name, organization,
                        first_seen_at, last_seen_at, total_emails_from, total_emails_to,
                        avg_response_time_hours, top_categories, communication_style,
                        ai_summary, profile_data, generated_at, updated_at
                 FROM contact_profiles
                 WHERE account_id = ?1
                 ORDER BY total_emails_from + total_emails_to DESC
                 LIMIT ?2 OFFSET ?3",
            )
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        let profiles: Vec<ContactProfile> = stmt
            .query_map(rusqlite::params![account_id, limit, offset], row_to_profile)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .filter_map(|r| r.ok())
            .collect();

        let total: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM contact_profiles WHERE account_id = ?1",
                rusqlite::params![account_id],
                |row| row.get(0),
            )
            .unwrap_or(0);

        (profiles, total)
    } else {
        let mut stmt = conn
            .prepare(
                "SELECT id, account_id, email_address, display_name, organization,
                        first_seen_at, last_seen_at, total_emails_from, total_emails_to,
                        avg_response_time_hours, top_categories, communication_style,
                        ai_summary, profile_data, generated_at, updated_at
                 FROM contact_profiles
                 ORDER BY total_emails_from + total_emails_to DESC
                 LIMIT ?1 OFFSET ?2",
            )
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        let profiles: Vec<ContactProfile> = stmt
            .query_map(rusqlite::params![limit, offset], row_to_profile)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .filter_map(|r| r.ok())
            .collect();

        let total: i64 = conn
            .query_row("SELECT COUNT(*) FROM contact_profiles", [], |row| row.get(0))
            .unwrap_or(0);

        (profiles, total)
    };

    Ok(Json(ListProfilesResponse { profiles, total }))
}

/// GET /api/contacts/profiles/:email — get a specific profile
pub async fn get_profile(
    State(state): State<Arc<AppState>>,
    Path(email): Path<String>,
    Query(params): Query<ListProfilesParams>,
) -> Result<Json<ContactProfile>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let email = email.to_lowercase();

    if let Some(ref account_id) = params.account_id {
        conn.query_row(
            "SELECT id, account_id, email_address, display_name, organization,
                    first_seen_at, last_seen_at, total_emails_from, total_emails_to,
                    avg_response_time_hours, top_categories, communication_style,
                    ai_summary, profile_data, generated_at, updated_at
             FROM contact_profiles
             WHERE account_id = ?1 AND email_address = ?2",
            rusqlite::params![account_id, email],
            row_to_profile,
        )
        .map(Json)
        .map_err(|_| StatusCode::NOT_FOUND)
    } else {
        // Return the first matching profile across all accounts
        conn.query_row(
            "SELECT id, account_id, email_address, display_name, organization,
                    first_seen_at, last_seen_at, total_emails_from, total_emails_to,
                    avg_response_time_hours, top_categories, communication_style,
                    ai_summary, profile_data, generated_at, updated_at
             FROM contact_profiles
             WHERE email_address = ?1
             ORDER BY total_emails_from + total_emails_to DESC
             LIMIT 1",
            rusqlite::params![email],
            row_to_profile,
        )
        .map(Json)
        .map_err(|_| StatusCode::NOT_FOUND)
    }
}

/// DELETE /api/contacts/profiles/:email — delete a profile
pub async fn delete_profile(
    State(state): State<Arc<AppState>>,
    Path(email): Path<String>,
    Query(params): Query<ListProfilesParams>,
) -> Result<StatusCode, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let email = email.to_lowercase();

    let deleted = if let Some(ref account_id) = params.account_id {
        conn.execute(
            "DELETE FROM contact_profiles WHERE account_id = ?1 AND email_address = ?2",
            rusqlite::params![account_id, email],
        )
        .unwrap_or(0)
    } else {
        conn.execute(
            "DELETE FROM contact_profiles WHERE email_address = ?1",
            rusqlite::params![email],
        )
        .unwrap_or(0)
    };

    if deleted > 0 {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

/// POST /api/contacts/profiles/generate-all — batch generate for top contacts
pub async fn generate_all_profiles(
    State(state): State<Arc<AppState>>,
    Query(params): Query<GenerateAllParams>,
) -> Result<Json<GenerateAllResponse>, StatusCode> {
    let account_id = params.account_id.ok_or(StatusCode::BAD_REQUEST)?;
    let limit = params.limit.unwrap_or(20).min(100);

    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Find top contacts by email frequency (both sent and received)
    let mut stmt = conn
        .prepare(
            "SELECT email, SUM(cnt) as total FROM (
                SELECT from_address as email, COUNT(*) as cnt
                FROM messages WHERE account_id = ?1 AND from_address IS NOT NULL AND is_deleted = 0
                GROUP BY from_address
                UNION ALL
                SELECT json_each.value as email, COUNT(*) as cnt
                FROM messages, json_each(to_addresses)
                WHERE account_id = ?1 AND to_addresses IS NOT NULL AND is_deleted = 0
                GROUP BY json_each.value
            ) GROUP BY email ORDER BY total DESC LIMIT ?2",
        )
        .map_err(|e| {
            tracing::error!("Failed to prepare top contacts query: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let top_emails: Vec<String> = stmt
        .query_map(rusqlite::params![account_id, limit], |row| {
            row.get::<_, String>(0)
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .filter_map(|r| r.ok())
        .collect();

    let mut profiles = Vec::new();
    for email in &top_emails {
        if let Ok(profile) = generate_profile_for_email(&conn, &account_id, email) {
            profiles.push(profile);
        }
    }

    let generated = profiles.len();
    Ok(Json(GenerateAllResponse {
        generated,
        profiles,
    }))
}

/// GET /api/contacts/profiles/search — search profiles
pub async fn search_profiles(
    State(state): State<Arc<AppState>>,
    Query(params): Query<SearchProfilesParams>,
) -> Result<Json<SearchProfilesResponse>, StatusCode> {
    let query_str = params.q.as_deref().unwrap_or("").trim().to_string();
    if query_str.is_empty() {
        return Ok(Json(SearchProfilesResponse {
            profiles: Vec::new(),
            total: 0,
            query: query_str,
        }));
    }

    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let limit = params.limit.unwrap_or(50).min(500);
    let offset = params.offset.unwrap_or(0).max(0);
    let like_query = format!("%{}%", query_str);

    let (profiles, total) = if let Some(ref account_id) = params.account_id {
        let mut stmt = conn
            .prepare(
                "SELECT id, account_id, email_address, display_name, organization,
                        first_seen_at, last_seen_at, total_emails_from, total_emails_to,
                        avg_response_time_hours, top_categories, communication_style,
                        ai_summary, profile_data, generated_at, updated_at
                 FROM contact_profiles
                 WHERE account_id = ?1
                   AND (email_address LIKE ?2 OR display_name LIKE ?2 OR organization LIKE ?2 OR ai_summary LIKE ?2)
                 ORDER BY total_emails_from + total_emails_to DESC
                 LIMIT ?3 OFFSET ?4",
            )
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        let profiles: Vec<ContactProfile> = stmt
            .query_map(
                rusqlite::params![account_id, like_query, limit, offset],
                row_to_profile,
            )
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .filter_map(|r| r.ok())
            .collect();

        let total: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM contact_profiles
                 WHERE account_id = ?1
                   AND (email_address LIKE ?2 OR display_name LIKE ?2 OR organization LIKE ?2 OR ai_summary LIKE ?2)",
                rusqlite::params![account_id, like_query],
                |row| row.get(0),
            )
            .unwrap_or(0);

        (profiles, total)
    } else {
        let mut stmt = conn
            .prepare(
                "SELECT id, account_id, email_address, display_name, organization,
                        first_seen_at, last_seen_at, total_emails_from, total_emails_to,
                        avg_response_time_hours, top_categories, communication_style,
                        ai_summary, profile_data, generated_at, updated_at
                 FROM contact_profiles
                 WHERE email_address LIKE ?1 OR display_name LIKE ?1 OR organization LIKE ?1 OR ai_summary LIKE ?1
                 ORDER BY total_emails_from + total_emails_to DESC
                 LIMIT ?2 OFFSET ?3",
            )
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        let profiles: Vec<ContactProfile> = stmt
            .query_map(
                rusqlite::params![like_query, limit, offset],
                row_to_profile,
            )
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .filter_map(|r| r.ok())
            .collect();

        let total: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM contact_profiles
                 WHERE email_address LIKE ?1 OR display_name LIKE ?1 OR organization LIKE ?1 OR ai_summary LIKE ?1",
                rusqlite::params![like_query],
                |row| row.get(0),
            )
            .unwrap_or(0);

        (profiles, total)
    };

    Ok(Json(SearchProfilesResponse {
        profiles,
        total,
        query: query_str,
    }))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::create_test_pool;

    /// Helper: insert test account + messages for profile generation tests.
    fn setup_test_data(pool: &crate::db::DbPool) {
        let conn = pool.get().unwrap();
        conn.execute(
            "INSERT INTO accounts (id, provider, email) VALUES ('acc1', 'imap', 'me@example.com')",
            [],
        )
        .unwrap();

        // Messages FROM alice
        conn.execute(
            "INSERT INTO messages (id, account_id, folder, from_address, from_name, to_addresses, subject, body_text, date, is_read, thread_id, ai_category)
             VALUES ('m1', 'acc1', 'INBOX', 'alice@acme.com', 'Alice Smith', '[\"me@example.com\"]', 'Project Update', 'Dear team, please review the latest changes. Best regards, Alice', 1709500800, 0, 't1', 'Primary')",
            [],
        ).unwrap();

        conn.execute(
            "INSERT INTO messages (id, account_id, folder, from_address, from_name, to_addresses, subject, body_text, date, is_read, thread_id, ai_category)
             VALUES ('m2', 'acc1', 'INBOX', 'alice@acme.com', 'Alice Smith', '[\"me@example.com\"]', 'Follow up', 'Dear colleague, sincerely hope this finds you well. Regards, Alice', 1709587200, 1, 't2', 'Primary')",
            [],
        ).unwrap();

        // Messages TO alice (from me)
        conn.execute(
            "INSERT INTO messages (id, account_id, folder, from_address, from_name, to_addresses, subject, body_text, date, is_read, thread_id, ai_category)
             VALUES ('m3', 'acc1', 'Sent', 'me@example.com', 'Me', '[\"alice@acme.com\"]', 'Re: Project Update', 'Thanks Alice, looks good!', 1709504400, 1, 't1', 'Primary')",
            [],
        ).unwrap();

        // Messages from bob (casual)
        conn.execute(
            "INSERT INTO messages (id, account_id, folder, from_address, from_name, to_addresses, subject, body_text, date, is_read, thread_id, ai_category)
             VALUES ('m4', 'acc1', 'INBOX', 'bob@gmail.com', 'Bob Jones', '[\"me@example.com\"]', 'Hey!', 'Hey dude, haha check this out btw', 1709600000, 0, 't3', 'Social')",
            [],
        ).unwrap();
    }

    #[test]
    fn test_migration_creates_table() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='contact_profiles'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_migration_creates_indexes() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let indexes: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='index' AND tbl_name='contact_profiles'")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert!(indexes.contains(&"idx_contact_profiles_unique".to_string()));
        assert!(indexes.contains(&"idx_contact_profiles_account".to_string()));
    }

    #[test]
    fn test_generate_profile_basic() {
        let pool = create_test_pool();
        setup_test_data(&pool);

        let conn = pool.get().unwrap();
        let profile = generate_profile_for_email(&conn, "acc1", "alice@acme.com").unwrap();

        assert_eq!(profile.email_address, "alice@acme.com");
        assert_eq!(profile.account_id, "acc1");
        assert_eq!(profile.total_emails_from, 2);
        assert_eq!(profile.total_emails_to, 1);
        assert_eq!(profile.display_name.as_deref(), Some("Alice Smith"));
        assert_eq!(profile.organization.as_deref(), Some("acme.com"));
        assert!(profile.first_seen_at.is_some());
        assert!(profile.last_seen_at.is_some());
    }

    #[test]
    fn test_generate_profile_categories() {
        let pool = create_test_pool();
        setup_test_data(&pool);

        let conn = pool.get().unwrap();
        let profile = generate_profile_for_email(&conn, "acc1", "alice@acme.com").unwrap();

        // Alice's emails are all categorized as "Primary"
        let cats = profile.top_categories.unwrap();
        let parsed: Vec<String> = serde_json::from_str(&cats).unwrap();
        assert!(parsed.contains(&"Primary".to_string()));
    }

    #[test]
    fn test_generate_profile_formal_style() {
        let pool = create_test_pool();
        setup_test_data(&pool);

        let conn = pool.get().unwrap();
        let profile = generate_profile_for_email(&conn, "acc1", "alice@acme.com").unwrap();

        // Alice uses "Dear", "Regards", "Sincerely" -> formal
        assert_eq!(profile.communication_style.as_deref(), Some("formal"));
    }

    #[test]
    fn test_generate_profile_casual_style() {
        let pool = create_test_pool();
        setup_test_data(&pool);

        let conn = pool.get().unwrap();
        let profile = generate_profile_for_email(&conn, "acc1", "bob@gmail.com").unwrap();

        // Bob uses "Hey", "haha", "btw" -> casual
        assert_eq!(profile.communication_style.as_deref(), Some("casual"));
    }

    #[test]
    fn test_generate_profile_free_email_no_org() {
        let pool = create_test_pool();
        setup_test_data(&pool);

        let conn = pool.get().unwrap();
        let profile = generate_profile_for_email(&conn, "acc1", "bob@gmail.com").unwrap();

        // gmail.com is a free provider, so organization should be None
        assert!(profile.organization.is_none());
    }

    #[test]
    fn test_generate_profile_response_time() {
        let pool = create_test_pool();
        setup_test_data(&pool);

        let conn = pool.get().unwrap();
        let profile = generate_profile_for_email(&conn, "acc1", "alice@acme.com").unwrap();

        // Thread t1: m1 at 1709500800 (from alice), m3 at 1709504400 (from me) = 1 hour gap
        assert!(profile.avg_response_time_hours.is_some());
        let avg = profile.avg_response_time_hours.unwrap();
        assert!(avg > 0.0, "Expected positive avg response time, got {avg}");
        assert!(avg <= 168.0, "Expected <= 168h cap, got {avg}");
    }

    #[test]
    fn test_generate_profile_upsert() {
        let pool = create_test_pool();
        setup_test_data(&pool);

        let conn = pool.get().unwrap();

        // Generate first time
        let p1 = generate_profile_for_email(&conn, "acc1", "alice@acme.com").unwrap();
        assert_eq!(p1.total_emails_from, 2);

        // Add another message from alice
        conn.execute(
            "INSERT INTO messages (id, account_id, folder, from_address, from_name, to_addresses, subject, body_text, date, is_read)
             VALUES ('m5', 'acc1', 'INBOX', 'alice@acme.com', 'Alice Smith', '[\"me@example.com\"]', 'Another update', 'Hello again', 1709700000, 0)",
            [],
        ).unwrap();

        // Re-generate — should update, not duplicate
        let p2 = generate_profile_for_email(&conn, "acc1", "alice@acme.com").unwrap();
        assert_eq!(p2.total_emails_from, 3);
        assert_eq!(p2.id, p1.id, "UPSERT should keep the same row id");

        // Verify only one profile exists
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM contact_profiles WHERE account_id = 'acc1' AND email_address = 'alice@acme.com'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_generate_profile_nonexistent_contact() {
        let pool = create_test_pool();
        setup_test_data(&pool);

        let conn = pool.get().unwrap();
        let result = generate_profile_for_email(&conn, "acc1", "nobody@nowhere.com");
        assert!(result.is_err());
    }

    #[test]
    fn test_detect_communication_style_empty() {
        assert_eq!(detect_communication_style(&[]), "mixed");
    }

    #[test]
    fn test_detect_communication_style_formal() {
        let bodies = vec![
            "Dear team, please find the report attached. Best regards, John".to_string(),
            "Dear Sir, I wanted to follow up. Sincerely, John".to_string(),
            "Dear colleagues, kind regards".to_string(),
        ];
        assert_eq!(detect_communication_style(&bodies), "formal");
    }

    #[test]
    fn test_detect_communication_style_casual() {
        let bodies = vec![
            "Hey! Check this out lol".to_string(),
            "Hi there, btw gonna send it later, cheers".to_string(),
            "Hey, haha that was fun".to_string(),
        ];
        assert_eq!(detect_communication_style(&bodies), "casual");
    }

    #[test]
    fn test_detect_communication_style_mixed() {
        let bodies = vec![
            "Dear team, please review. Best regards".to_string(),
            "Hey! lol that was funny".to_string(),
        ];
        assert_eq!(detect_communication_style(&bodies), "mixed");
    }

    #[test]
    fn test_list_profiles_empty() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM contact_profiles", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_list_profiles_after_generation() {
        let pool = create_test_pool();
        setup_test_data(&pool);

        let conn = pool.get().unwrap();
        generate_profile_for_email(&conn, "acc1", "alice@acme.com").unwrap();
        generate_profile_for_email(&conn, "acc1", "bob@gmail.com").unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM contact_profiles WHERE account_id = 'acc1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn test_delete_profile() {
        let pool = create_test_pool();
        setup_test_data(&pool);

        let conn = pool.get().unwrap();
        generate_profile_for_email(&conn, "acc1", "alice@acme.com").unwrap();

        let before: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM contact_profiles WHERE account_id = 'acc1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(before, 1);

        conn.execute(
            "DELETE FROM contact_profiles WHERE account_id = 'acc1' AND email_address = 'alice@acme.com'",
            [],
        )
        .unwrap();

        let after: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM contact_profiles WHERE account_id = 'acc1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(after, 0);
    }

    #[test]
    fn test_search_profiles_by_email() {
        let pool = create_test_pool();
        setup_test_data(&pool);

        let conn = pool.get().unwrap();
        generate_profile_for_email(&conn, "acc1", "alice@acme.com").unwrap();
        generate_profile_for_email(&conn, "acc1", "bob@gmail.com").unwrap();

        // Search by email fragment
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM contact_profiles WHERE email_address LIKE '%alice%'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_search_profiles_by_name() {
        let pool = create_test_pool();
        setup_test_data(&pool);

        let conn = pool.get().unwrap();
        generate_profile_for_email(&conn, "acc1", "alice@acme.com").unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM contact_profiles WHERE display_name LIKE '%Alice%'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_search_profiles_by_organization() {
        let pool = create_test_pool();
        setup_test_data(&pool);

        let conn = pool.get().unwrap();
        generate_profile_for_email(&conn, "acc1", "alice@acme.com").unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM contact_profiles WHERE organization LIKE '%acme%'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_profile_unique_constraint() {
        let pool = create_test_pool();
        setup_test_data(&pool);

        let conn = pool.get().unwrap();

        // Insert manually to test the unique constraint
        conn.execute(
            "INSERT INTO contact_profiles (account_id, email_address, total_emails_from, total_emails_to, profile_data)
             VALUES ('acc1', 'test@test.com', 1, 0, '{}')",
            [],
        )
        .unwrap();

        // Second insert with same account_id + email should fail (without ON CONFLICT)
        let result = conn.execute(
            "INSERT INTO contact_profiles (account_id, email_address, total_emails_from, total_emails_to, profile_data)
             VALUES ('acc1', 'test@test.com', 2, 0, '{}')",
            [],
        );
        assert!(result.is_err(), "Unique constraint should prevent duplicate");
    }

    #[test]
    fn test_profile_default_values() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        conn.execute(
            "INSERT INTO contact_profiles (account_id, email_address)
             VALUES ('acc1', 'defaults@test.com')",
            [],
        )
        .unwrap();

        let profile = conn
            .query_row(
                "SELECT total_emails_from, total_emails_to, profile_data FROM contact_profiles WHERE email_address = 'defaults@test.com'",
                [],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .unwrap();

        assert_eq!(profile.0, 0); // total_emails_from default
        assert_eq!(profile.1, 0); // total_emails_to default
        assert_eq!(profile.2, "{}"); // profile_data default
    }

    #[test]
    fn test_multiple_accounts_same_contact() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        // Create two accounts
        conn.execute(
            "INSERT INTO accounts (id, provider, email) VALUES ('acc1', 'imap', 'me1@example.com')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO accounts (id, provider, email) VALUES ('acc2', 'imap', 'me2@example.com')",
            [],
        )
        .unwrap();

        // Messages from same contact to different accounts
        conn.execute(
            "INSERT INTO messages (id, account_id, folder, from_address, from_name, subject, body_text, date, is_read)
             VALUES ('m1', 'acc1', 'INBOX', 'contact@corp.com', 'Contact', 'Hello', 'Dear team, regards', 1709500800, 0)",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO messages (id, account_id, folder, from_address, from_name, subject, body_text, date, is_read)
             VALUES ('m2', 'acc2', 'INBOX', 'contact@corp.com', 'Contact', 'Hi there', 'Hey, cheers', 1709500800, 0)",
            [],
        ).unwrap();

        // Generate profiles for each account separately
        let p1 = generate_profile_for_email(&conn, "acc1", "contact@corp.com").unwrap();
        let p2 = generate_profile_for_email(&conn, "acc2", "contact@corp.com").unwrap();

        assert_eq!(p1.account_id, "acc1");
        assert_eq!(p2.account_id, "acc2");
        assert_ne!(p1.id, p2.id); // Different profile rows

        // Both should exist
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM contact_profiles WHERE email_address = 'contact@corp.com'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);
    }
}

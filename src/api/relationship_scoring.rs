use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use crate::AppState;

// ── Types ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum StrengthLabel {
    Strong,
    Regular,
    Weak,
    Dormant,
}

impl StrengthLabel {
    pub fn as_str(&self) -> &'static str {
        match self {
            StrengthLabel::Strong => "strong",
            StrengthLabel::Regular => "regular",
            StrengthLabel::Weak => "weak",
            StrengthLabel::Dormant => "dormant",
        }
    }

    pub fn from_score(score: f64) -> Self {
        if score >= 0.7 {
            StrengthLabel::Strong
        } else if score >= 0.4 {
            StrengthLabel::Regular
        } else if score >= 0.15 {
            StrengthLabel::Weak
        } else {
            StrengthLabel::Dormant
        }
    }
}

impl std::fmt::Display for StrengthLabel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Debug, Serialize)]
pub struct ComputeResult {
    pub computed: i64,
    pub strong: i64,
    pub regular: i64,
    pub weak: i64,
    pub dormant: i64,
}

#[derive(Debug, Serialize)]
pub struct RelationshipContact {
    pub email: String,
    pub display_name: Option<String>,
    pub strength_label: String,
    pub overall_score: f64,
    pub total_sent: i64,
    pub total_received: i64,
    pub avg_response_time_secs: Option<i64>,
    pub last_sent: Option<i64>,
    pub last_received: Option<i64>,
    pub first_interaction: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct ListRelationshipsResponse {
    pub contacts: Vec<RelationshipContact>,
    pub total: i64,
}

#[derive(Debug, Serialize)]
pub struct RelationshipDetail {
    pub email: String,
    pub display_name: Option<String>,
    pub strength_label: String,
    pub overall_score: f64,
    pub frequency_score: f64,
    pub recency_score: f64,
    pub reciprocity_score: f64,
    pub response_time_score: f64,
    pub thread_engagement_score: f64,
    pub total_sent: i64,
    pub total_received: i64,
    pub avg_response_time_secs: Option<i64>,
    pub last_sent: Option<i64>,
    pub last_received: Option<i64>,
    pub first_interaction: Option<i64>,
    pub computed_at: i64,
}

#[derive(Debug, Serialize)]
pub struct MostActive {
    pub email: String,
    pub score: f64,
}

#[derive(Debug, Serialize)]
pub struct RelationshipStats {
    pub total_contacts: i64,
    pub by_strength: ByStrength,
    pub avg_score: f64,
    pub most_active: Option<MostActive>,
}

#[derive(Debug, Serialize)]
pub struct ByStrength {
    pub strong: i64,
    pub regular: i64,
    pub weak: i64,
    pub dormant: i64,
}

#[derive(Debug, Deserialize)]
pub struct ListRelationshipsParams {
    pub strength: Option<String>,
    pub sort: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

// ── Scoring algorithm ─────────────────────────────────────────────────────────

/// Frequency score: emails per week, >5/week = 1.0
pub fn compute_frequency_score(total_emails: i64, days_span: f64) -> f64 {
    if days_span <= 0.0 || total_emails == 0 {
        return 0.0;
    }
    let weeks = days_span / 7.0;
    let per_week = total_emails as f64 / weeks.max(1.0);
    (per_week / 5.0).min(1.0).max(0.0)
}

/// Recency score: today = 1.0, >90 days = 0.0, linear decay
pub fn compute_recency_score(days_since_last: f64) -> f64 {
    if days_since_last <= 0.0 {
        return 1.0;
    }
    if days_since_last >= 90.0 {
        return 0.0;
    }
    1.0 - (days_since_last / 90.0)
}

/// Reciprocity score: min(sent,received)/max(sent,received). Equal exchange = 1.0
pub fn compute_reciprocity_score(sent: i64, received: i64) -> f64 {
    if sent == 0 || received == 0 {
        return 0.0;
    }
    let min = sent.min(received) as f64;
    let max = sent.max(received) as f64;
    min / max
}

/// Response time score: <1h = 1.0, >48h = 0.0, logarithmic scale
pub fn compute_response_time_score(avg_secs: Option<i64>) -> f64 {
    let secs = match avg_secs {
        Some(s) if s > 0 => s as f64,
        _ => return 0.0,
    };
    let hours = secs / 3600.0;
    if hours <= 1.0 {
        return 1.0;
    }
    if hours >= 48.0 {
        return 0.0;
    }
    // Logarithmic scale: log(1) = 0, log(48) ≈ 3.87
    // score = 1 - log(hours) / log(48)
    let score = 1.0 - hours.ln() / 48.0_f64.ln();
    score.clamp(0.0, 1.0)
}

/// Thread engagement score: avg thread depth, >5 replies = 1.0
pub fn compute_thread_engagement_score(avg_thread_depth: f64) -> f64 {
    if avg_thread_depth <= 1.0 {
        return 0.0;
    }
    ((avg_thread_depth - 1.0) / 4.0).min(1.0).max(0.0)
}

/// Overall weighted score
pub fn compute_overall_score(
    frequency: f64,
    recency: f64,
    reciprocity: f64,
    response_time: f64,
    thread_engagement: f64,
) -> f64 {
    let raw = frequency * 0.25
        + recency * 0.25
        + reciprocity * 0.20
        + response_time * 0.15
        + thread_engagement * 0.15;
    raw.clamp(0.0, 1.0)
}

fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

// ── Internal data structures ──────────────────────────────────────────────────

#[derive(Debug, Default)]
struct ContactData {
    display_name: Option<String>,
    total_received: i64,
    total_sent: i64,
    last_received: Option<i64>,
    last_sent: Option<i64>,
    first_interaction: Option<i64>,
    thread_ids_received: Vec<String>,
    /// Sum of deltas (secs) for response time pairs
    response_time_sum_secs: i64,
    response_time_count: i64,
}

/// Compute detailed relationship scores for all contacts under a given account.
pub fn compute_detailed_scores(
    conn: &r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>,
    account_id: &str,
    account_email: &str,
    now_epoch: i64,
) -> Result<Vec<(String, RelationshipDetail)>, rusqlite::Error> {
    let account_email_lower = account_email.to_lowercase();
    let mut contacts: HashMap<String, ContactData> = HashMap::new();

    // 1. Received messages: from contact to user
    {
        let mut stmt = conn.prepare(
            "SELECT LOWER(from_address), from_name, date
             FROM messages
             WHERE account_id = ?1
               AND is_deleted = 0
               AND is_draft = 0
               AND from_address IS NOT NULL
               AND LOWER(from_address) != ?2",
        )?;
        let rows = stmt.query_map(
            rusqlite::params![account_id, &account_email_lower],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                ))
            },
        )?;
        for row in rows.flatten() {
            let (email, name, date) = row;
            let entry = contacts.entry(email).or_default();
            entry.total_received += 1;
            if entry.display_name.is_none() {
                entry.display_name = name;
            }
            if let Some(d) = date {
                entry.last_received = Some(entry.last_received.map_or(d, |prev| prev.max(d)));
                entry.first_interaction = Some(entry.first_interaction.map_or(d, |prev| prev.min(d)));
            }
        }
    }

    // 2. Sent messages: from user to contact (folder='Sent', parse to_addresses)
    {
        let mut stmt = conn.prepare(
            "SELECT to_addresses, date
             FROM messages
             WHERE account_id = ?1
               AND folder = 'Sent'
               AND is_deleted = 0
               AND to_addresses IS NOT NULL",
        )?;
        let rows = stmt.query_map(rusqlite::params![account_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<i64>>(1)?))
        })?;
        for row in rows.flatten() {
            let (to_json, date) = row;
            let recipients = parse_email_list(&to_json);
            for recip in recipients {
                let recip_lower = recip.to_lowercase();
                if recip_lower == account_email_lower {
                    continue;
                }
                let entry = contacts.entry(recip_lower).or_default();
                entry.total_sent += 1;
                if let Some(d) = date {
                    entry.last_sent = Some(entry.last_sent.map_or(d, |prev| prev.max(d)));
                    entry.first_interaction = Some(entry.first_interaction.map_or(d, |prev| prev.min(d)));
                }
            }
        }
    }

    // 3. Thread depth: gather thread participants per account
    {
        let mut stmt = conn.prepare(
            "SELECT LOWER(from_address), thread_id
             FROM messages
             WHERE account_id = ?1
               AND is_deleted = 0
               AND thread_id IS NOT NULL
               AND from_address IS NOT NULL
               AND LOWER(from_address) != ?2",
        )?;
        let rows = stmt.query_map(
            rusqlite::params![account_id, &account_email_lower],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )?;
        for row in rows.flatten() {
            let (email, thread_id) = row;
            let entry = contacts.entry(email).or_default();
            if !entry.thread_ids_received.contains(&thread_id) {
                entry.thread_ids_received.push(thread_id);
            }
        }
    }

    // thread_id → message count
    let mut thread_depths: HashMap<String, i64> = HashMap::new();
    {
        let mut stmt = conn.prepare(
            "SELECT thread_id, COUNT(*) FROM messages
             WHERE account_id = ?1 AND is_deleted = 0 AND thread_id IS NOT NULL
             GROUP BY thread_id",
        )?;
        let rows = stmt.query_map(rusqlite::params![account_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        for row in rows.flatten() {
            thread_depths.insert(row.0, row.1);
        }
    }

    // 4. Response times: for each thread, look at consecutive message pairs
    //    where sender alternates between contact and user
    {
        // Gather all threads involving both the user and each contact
        let mut stmt = conn.prepare(
            "SELECT LOWER(from_address), date, thread_id
             FROM messages
             WHERE account_id = ?1
               AND is_deleted = 0
               AND thread_id IS NOT NULL
               AND date IS NOT NULL
             ORDER BY thread_id, date ASC",
        )?;
        let rows: Vec<(String, i64, String)> = stmt
            .query_map(rusqlite::params![account_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?
            .filter_map(|r| r.ok())
            .collect();

        // Group by thread_id
        let mut threads: HashMap<String, Vec<(String, i64)>> = HashMap::new();
        for (from, date, tid) in rows {
            threads.entry(tid).or_default().push((from, date));
        }

        for msgs in threads.values() {
            for window in msgs.windows(2) {
                let (from_a, date_a) = &window[0];
                let (from_b, date_b) = &window[1];
                if from_a == from_b {
                    continue;
                }
                let delta = date_b - date_a;
                if delta <= 0 {
                    continue;
                }

                // If user replied to contact: from_a = contact, from_b = user
                if from_b == &account_email_lower && from_a != &account_email_lower {
                    let entry = contacts.entry(from_a.clone()).or_default();
                    entry.response_time_sum_secs += delta;
                    entry.response_time_count += 1;
                }
                // If contact replied to user: from_a = user, from_b = contact
                if from_a == &account_email_lower && from_b != &account_email_lower {
                    let entry = contacts.entry(from_b.clone()).or_default();
                    entry.response_time_sum_secs += delta;
                    entry.response_time_count += 1;
                }
            }
        }
    }

    // 5. Build scores
    let mut results = Vec::new();

    for (email, data) in contacts {
        if data.total_received == 0 && data.total_sent == 0 {
            continue;
        }

        let first = data.first_interaction.unwrap_or(now_epoch);
        let last = data
            .last_received
            .unwrap_or(0)
            .max(data.last_sent.unwrap_or(0));
        let last = if last == 0 { now_epoch } else { last };

        let days_span = ((now_epoch - first) as f64 / 86400.0).max(1.0);
        let days_since_last = ((now_epoch - last) as f64 / 86400.0).max(0.0);

        let total_emails = data.total_received + data.total_sent;

        let frequency = compute_frequency_score(total_emails, days_span);
        let recency = compute_recency_score(days_since_last);
        let reciprocity = compute_reciprocity_score(data.total_sent, data.total_received);

        let avg_response_time_secs: Option<i64> = if data.response_time_count > 0 {
            Some(data.response_time_sum_secs / data.response_time_count)
        } else {
            None
        };
        let response_time = compute_response_time_score(avg_response_time_secs);

        let avg_thread_depth = if !data.thread_ids_received.is_empty() {
            let total: i64 = data
                .thread_ids_received
                .iter()
                .filter_map(|tid| thread_depths.get(tid))
                .sum();
            total as f64 / data.thread_ids_received.len() as f64
        } else {
            1.0
        };
        let thread_engagement = compute_thread_engagement_score(avg_thread_depth);

        let overall = compute_overall_score(frequency, recency, reciprocity, response_time, thread_engagement);
        let strength = StrengthLabel::from_score(overall);

        results.push((
            email.clone(),
            RelationshipDetail {
                email,
                display_name: data.display_name,
                strength_label: strength.as_str().to_string(),
                overall_score: round3(overall),
                frequency_score: round3(frequency),
                recency_score: round3(recency),
                reciprocity_score: round3(reciprocity),
                response_time_score: round3(response_time),
                thread_engagement_score: round3(thread_engagement),
                total_sent: data.total_sent,
                total_received: data.total_received,
                avg_response_time_secs,
                last_sent: data.last_sent,
                last_received: data.last_received,
                first_interaction: data.first_interaction,
                computed_at: now_epoch,
            },
        ));
    }

    Ok(results)
}

fn parse_email_list(json_str: &str) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(json_str).unwrap_or_default()
}

// ── Handlers ──────────────────────────────────────────────────────────────────

/// POST /api/contacts/relationships/compute
pub async fn compute_relationships(
    State(state): State<Arc<AppState>>,
) -> Result<Json<ComputeResult>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let now = chrono::Utc::now().timestamp();

    // Get all active accounts
    let accounts: Vec<(String, String)> = {
        let mut stmt = conn
            .prepare("SELECT id, LOWER(email) FROM accounts WHERE is_active = 1")
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .filter_map(|r| r.ok())
            .collect()
    };

    let mut total_computed: i64 = 0;
    let mut count_strong: i64 = 0;
    let mut count_regular: i64 = 0;
    let mut count_weak: i64 = 0;
    let mut count_dormant: i64 = 0;

    for (account_id, account_email) in &accounts {
        let scored = compute_detailed_scores(&conn, account_id, account_email, now)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        for (_, detail) in &scored {
            conn.execute(
                "INSERT INTO relationship_details
                    (account_id, email, display_name, strength_label, overall_score,
                     frequency_score, recency_score, reciprocity_score, response_time_score,
                     thread_engagement_score, total_sent, total_received,
                     avg_response_time_secs, last_sent, last_received, first_interaction,
                     computed_at)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)
                 ON CONFLICT(account_id, email) DO UPDATE SET
                     display_name        = excluded.display_name,
                     strength_label      = excluded.strength_label,
                     overall_score       = excluded.overall_score,
                     frequency_score     = excluded.frequency_score,
                     recency_score       = excluded.recency_score,
                     reciprocity_score   = excluded.reciprocity_score,
                     response_time_score = excluded.response_time_score,
                     thread_engagement_score = excluded.thread_engagement_score,
                     total_sent          = excluded.total_sent,
                     total_received      = excluded.total_received,
                     avg_response_time_secs = excluded.avg_response_time_secs,
                     last_sent           = excluded.last_sent,
                     last_received       = excluded.last_received,
                     first_interaction   = excluded.first_interaction,
                     computed_at         = excluded.computed_at",
                rusqlite::params![
                    account_id,
                    detail.email,
                    detail.display_name,
                    detail.strength_label,
                    detail.overall_score,
                    detail.frequency_score,
                    detail.recency_score,
                    detail.reciprocity_score,
                    detail.response_time_score,
                    detail.thread_engagement_score,
                    detail.total_sent,
                    detail.total_received,
                    detail.avg_response_time_secs,
                    detail.last_sent,
                    detail.last_received,
                    detail.first_interaction,
                    detail.computed_at,
                ],
            )
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

            match detail.strength_label.as_str() {
                "strong" => count_strong += 1,
                "regular" => count_regular += 1,
                "weak" => count_weak += 1,
                "dormant" => count_dormant += 1,
                _ => {}
            }
            total_computed += 1;
        }
    }

    Ok(Json(ComputeResult {
        computed: total_computed,
        strong: count_strong,
        regular: count_regular,
        weak: count_weak,
        dormant: count_dormant,
    }))
}

/// GET /api/contacts/relationships
pub async fn list_relationships(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListRelationshipsParams>,
) -> Result<Json<ListRelationshipsResponse>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let limit = params.limit.unwrap_or(50).min(200).max(1);
    let offset = params.offset.unwrap_or(0).max(0);

    // Validate strength filter
    let strength_filter = params.strength.as_deref().filter(|s| {
        matches!(*s, "strong" | "regular" | "weak" | "dormant")
    });

    // Build ORDER BY
    let order_clause = match params.sort.as_deref() {
        Some("name") => "LOWER(COALESCE(display_name, email)) ASC",
        Some("recent") => "COALESCE(last_received, last_sent) DESC NULLS LAST",
        _ => "overall_score DESC",
    };

    let (where_clause, where_param): (&str, Option<&str>) = if let Some(s) = strength_filter {
        ("WHERE strength_label = ?1", Some(s))
    } else {
        ("", None)
    };

    // Count total
    let total: i64 = if let Some(strength) = strength_filter {
        conn.query_row(
            &format!("SELECT COUNT(DISTINCT email) FROM relationship_details WHERE strength_label = ?1"),
            rusqlite::params![strength],
            |row| row.get(0),
        )
        .unwrap_or(0)
    } else {
        conn.query_row(
            "SELECT COUNT(DISTINCT email) FROM relationship_details",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0)
    };

    let query = format!(
        "SELECT email, display_name, strength_label, overall_score,
                total_sent, total_received, avg_response_time_secs,
                last_sent, last_received, first_interaction
         FROM relationship_details
         {where_clause}
         ORDER BY {order_clause}
         LIMIT ?{limit_idx} OFFSET ?{offset_idx}",
        where_clause = where_clause,
        order_clause = order_clause,
        limit_idx = if where_param.is_some() { 2 } else { 1 },
        offset_idx = if where_param.is_some() { 3 } else { 2 },
    );

    let mut stmt = conn.prepare(&query).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let map_row = |row: &rusqlite::Row<'_>| {
        Ok(RelationshipContact {
            email: row.get(0)?,
            display_name: row.get(1)?,
            strength_label: row.get(2)?,
            overall_score: row.get(3)?,
            total_sent: row.get(4)?,
            total_received: row.get(5)?,
            avg_response_time_secs: row.get(6)?,
            last_sent: row.get(7)?,
            last_received: row.get(8)?,
            first_interaction: row.get(9)?,
        })
    };

    let contacts: Vec<RelationshipContact> = if let Some(strength) = where_param {
        stmt.query_map(rusqlite::params![strength, limit, offset], map_row)
    } else {
        stmt.query_map(rusqlite::params![limit, offset], map_row)
    }
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .filter_map(|r| r.ok())
    .collect();

    Ok(Json(ListRelationshipsResponse { contacts, total }))
}

/// GET /api/contacts/relationships/stats
pub async fn relationship_stats(
    State(state): State<Arc<AppState>>,
) -> Result<Json<RelationshipStats>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let total_contacts: i64 = conn
        .query_row(
            "SELECT COUNT(DISTINCT email) FROM relationship_details",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    // Count by strength
    let mut by_strength = ByStrength {
        strong: 0,
        regular: 0,
        weak: 0,
        dormant: 0,
    };
    {
        let mut stmt = conn
            .prepare("SELECT strength_label, COUNT(*) FROM relationship_details GROUP BY strength_label")
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let rows = stmt
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        for row in rows.flatten() {
            match row.0.as_str() {
                "strong" => by_strength.strong = row.1,
                "regular" => by_strength.regular = row.1,
                "weak" => by_strength.weak = row.1,
                "dormant" => by_strength.dormant = row.1,
                _ => {}
            }
        }
    }

    let avg_score: f64 = if total_contacts > 0 {
        conn.query_row(
            "SELECT AVG(overall_score) FROM relationship_details",
            [],
            |row| row.get::<_, Option<f64>>(0),
        )
        .unwrap_or(None)
        .unwrap_or(0.0)
    } else {
        0.0
    };

    let most_active: Option<MostActive> = conn
        .query_row(
            "SELECT email, overall_score FROM relationship_details ORDER BY overall_score DESC LIMIT 1",
            [],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?)),
        )
        .ok()
        .map(|(email, score)| MostActive { email, score });

    Ok(Json(RelationshipStats {
        total_contacts,
        by_strength,
        avg_score: round3(avg_score),
        most_active,
    }))
}

/// GET /api/contacts/relationships/{email}
pub async fn get_relationship(
    State(state): State<Arc<AppState>>,
    Path(email): Path<String>,
) -> Result<Json<RelationshipDetail>, StatusCode> {
    if !email.contains('@') || email.len() > 320 {
        return Err(StatusCode::BAD_REQUEST);
    }

    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let email_lower = email.to_lowercase();

    conn.query_row(
        "SELECT email, display_name, strength_label, overall_score,
                frequency_score, recency_score, reciprocity_score,
                response_time_score, thread_engagement_score,
                total_sent, total_received, avg_response_time_secs,
                last_sent, last_received, first_interaction, computed_at
         FROM relationship_details
         WHERE LOWER(email) = ?1
         ORDER BY overall_score DESC
         LIMIT 1",
        rusqlite::params![email_lower],
        |row| {
            Ok(RelationshipDetail {
                email: row.get(0)?,
                display_name: row.get(1)?,
                strength_label: row.get(2)?,
                overall_score: row.get(3)?,
                frequency_score: row.get(4)?,
                recency_score: row.get(5)?,
                reciprocity_score: row.get(6)?,
                response_time_score: row.get(7)?,
                thread_engagement_score: row.get(8)?,
                total_sent: row.get(9)?,
                total_received: row.get(10)?,
                avg_response_time_secs: row.get(11)?,
                last_sent: row.get(12)?,
                last_received: row.get(13)?,
                first_interaction: row.get(14)?,
                computed_at: row.get(15)?,
            })
        },
    )
    .map(Json)
    .map_err(|e| {
        if matches!(e, rusqlite::Error::QueryReturnedNoRows) {
            StatusCode::NOT_FOUND
        } else {
            StatusCode::INTERNAL_SERVER_ERROR
        }
    })
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Scoring unit tests ────────────────────────────────────────────────────

    #[test]
    fn test_frequency_score_zero() {
        assert_eq!(compute_frequency_score(0, 30.0), 0.0);
    }

    #[test]
    fn test_frequency_score_max() {
        // >5/week → 1.0
        // 7 days, 10 emails = 10/week → capped at 1.0
        assert_eq!(compute_frequency_score(10, 7.0), 1.0);
    }

    #[test]
    fn test_frequency_score_mid() {
        // 2.5/week over 4 weeks = 10 emails / (28 days / 7) = 10/4 = 2.5/week → 0.5
        let score = compute_frequency_score(10, 28.0);
        assert!((score - 0.5).abs() < 0.01, "expected 0.5, got {score}");
    }

    #[test]
    fn test_recency_score_today() {
        assert_eq!(compute_recency_score(0.0), 1.0);
    }

    #[test]
    fn test_recency_score_45_days() {
        // 45/90 = 0.5 decay remaining
        let score = compute_recency_score(45.0);
        assert!((score - 0.5).abs() < 0.01, "expected 0.5, got {score}");
    }

    #[test]
    fn test_recency_score_90_days() {
        assert_eq!(compute_recency_score(90.0), 0.0);
    }

    #[test]
    fn test_recency_score_beyond_90_days() {
        assert_eq!(compute_recency_score(180.0), 0.0);
    }

    #[test]
    fn test_reciprocity_score_equal() {
        assert_eq!(compute_reciprocity_score(5, 5), 1.0);
    }

    #[test]
    fn test_reciprocity_score_one_sided_receive() {
        // 0 sent → 0.0
        assert_eq!(compute_reciprocity_score(0, 10), 0.0);
    }

    #[test]
    fn test_reciprocity_score_one_sided_send() {
        assert_eq!(compute_reciprocity_score(10, 0), 0.0);
    }

    #[test]
    fn test_reciprocity_score_partial() {
        // sent=2, received=10 → min/max = 2/10 = 0.2
        let score = compute_reciprocity_score(2, 10);
        assert!((score - 0.2).abs() < 0.001, "expected 0.2, got {score}");
    }

    #[test]
    fn test_response_time_score_none() {
        assert_eq!(compute_response_time_score(None), 0.0);
    }

    #[test]
    fn test_response_time_score_under_1h() {
        // 1800 secs = 0.5 hours → 1.0
        assert_eq!(compute_response_time_score(Some(1800)), 1.0);
    }

    #[test]
    fn test_response_time_score_exactly_1h() {
        // 3600 secs = 1.0 hours → 1.0
        assert_eq!(compute_response_time_score(Some(3600)), 1.0);
    }

    #[test]
    fn test_response_time_score_48h() {
        // 172800 secs = 48 hours → 0.0
        assert_eq!(compute_response_time_score(Some(172800)), 0.0);
    }

    #[test]
    fn test_response_time_score_over_48h() {
        assert_eq!(compute_response_time_score(Some(200000)), 0.0);
    }

    #[test]
    fn test_response_time_score_between() {
        // Some positive score between 0 and 1 for, say, 6 hours
        let score = compute_response_time_score(Some(21600)); // 6h
        assert!(score > 0.0 && score < 1.0, "expected (0,1), got {score}");
    }

    #[test]
    fn test_thread_engagement_score_one_message() {
        // avg depth = 1.0 → 0.0 (no engagement beyond single message)
        assert_eq!(compute_thread_engagement_score(1.0), 0.0);
    }

    #[test]
    fn test_thread_engagement_score_five_deep() {
        // avg depth = 5 → (5-1)/4 = 1.0
        assert_eq!(compute_thread_engagement_score(5.0), 1.0);
    }

    #[test]
    fn test_thread_engagement_score_three_deep() {
        // avg depth = 3 → (3-1)/4 = 0.5
        let score = compute_thread_engagement_score(3.0);
        assert!((score - 0.5).abs() < 0.001, "expected 0.5, got {score}");
    }

    #[test]
    fn test_thread_engagement_score_beyond_five() {
        // Capped at 1.0
        assert_eq!(compute_thread_engagement_score(10.0), 1.0);
    }

    // ── Strength label tests ──────────────────────────────────────────────────

    #[test]
    fn test_strength_strong() {
        assert_eq!(StrengthLabel::from_score(0.7), StrengthLabel::Strong);
        assert_eq!(StrengthLabel::from_score(1.0), StrengthLabel::Strong);
        assert_eq!(StrengthLabel::from_score(0.85), StrengthLabel::Strong);
    }

    #[test]
    fn test_strength_regular() {
        assert_eq!(StrengthLabel::from_score(0.4), StrengthLabel::Regular);
        assert_eq!(StrengthLabel::from_score(0.5), StrengthLabel::Regular);
        assert_eq!(StrengthLabel::from_score(0.699), StrengthLabel::Regular);
    }

    #[test]
    fn test_strength_weak() {
        assert_eq!(StrengthLabel::from_score(0.15), StrengthLabel::Weak);
        assert_eq!(StrengthLabel::from_score(0.3), StrengthLabel::Weak);
        assert_eq!(StrengthLabel::from_score(0.399), StrengthLabel::Weak);
    }

    #[test]
    fn test_strength_dormant() {
        assert_eq!(StrengthLabel::from_score(0.0), StrengthLabel::Dormant);
        assert_eq!(StrengthLabel::from_score(0.1), StrengthLabel::Dormant);
        assert_eq!(StrengthLabel::from_score(0.149), StrengthLabel::Dormant);
    }

    // ── Overall score weight test ─────────────────────────────────────────────

    #[test]
    fn test_overall_score_all_max() {
        let score = compute_overall_score(1.0, 1.0, 1.0, 1.0, 1.0);
        assert!((score - 1.0).abs() < 1e-10, "all 1.0 should give 1.0, got {score}");
    }

    #[test]
    fn test_overall_score_all_zero() {
        let score = compute_overall_score(0.0, 0.0, 0.0, 0.0, 0.0);
        assert_eq!(score, 0.0);
    }

    #[test]
    fn test_overall_score_weights() {
        // freq=1, rest=0 → 0.25
        let s = compute_overall_score(1.0, 0.0, 0.0, 0.0, 0.0);
        assert!((s - 0.25).abs() < 1e-10, "freq only: {s}");
        // recency=1, rest=0 → 0.25
        let s = compute_overall_score(0.0, 1.0, 0.0, 0.0, 0.0);
        assert!((s - 0.25).abs() < 1e-10, "recency only: {s}");
        // reciprocity=1, rest=0 → 0.20
        let s = compute_overall_score(0.0, 0.0, 1.0, 0.0, 0.0);
        assert!((s - 0.20).abs() < 1e-10, "reciprocity only: {s}");
        // response_time=1, rest=0 → 0.15
        let s = compute_overall_score(0.0, 0.0, 0.0, 1.0, 0.0);
        assert!((s - 0.15).abs() < 1e-10, "response_time only: {s}");
        // thread_engagement=1, rest=0 → 0.15
        let s = compute_overall_score(0.0, 0.0, 0.0, 0.0, 1.0);
        assert!((s - 0.15).abs() < 1e-10, "thread_engagement only: {s}");
    }

    // ── DB integration tests ──────────────────────────────────────────────────
    // create_test_pool() already runs all migrations (including 030), so we
    // don't need to apply the migration manually in tests.

    fn seed_contact(
        conn: &r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>,
        account_id: &str,
        email: &str,
        strength: &str,
        score: f64,
    ) {
        conn.execute(
            "INSERT INTO relationship_details
                (account_id, email, strength_label, overall_score, frequency_score,
                 recency_score, reciprocity_score, response_time_score,
                 thread_engagement_score, total_sent, total_received, computed_at)
             VALUES (?1, ?2, ?3, ?4, 0, 0, 0, 0, 0, 0, 1, unixepoch())",
            rusqlite::params![account_id, email, strength, score],
        )
        .unwrap();
    }

    #[test]
    fn test_list_filter_by_strength() {
        let pool = crate::db::create_test_pool();
        let conn = pool.get().unwrap();


        seed_contact(&conn, "acc1", "strong@test.com", "strong", 0.9);
        seed_contact(&conn, "acc1", "regular@test.com", "regular", 0.5);
        seed_contact(&conn, "acc1", "weak@test.com", "weak", 0.2);
        seed_contact(&conn, "acc1", "dormant@test.com", "dormant", 0.05);

        // Filter strong only
        let rows: Vec<(String, String)> = {
            let mut stmt = conn
                .prepare(
                    "SELECT email, strength_label FROM relationship_details
                     WHERE strength_label = 'strong' ORDER BY overall_score DESC",
                )
                .unwrap();
            stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                .unwrap()
                .filter_map(|r| r.ok())
                .collect()
        };

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, "strong@test.com");
        assert_eq!(rows[0].1, "strong");
    }

    #[test]
    fn test_list_sort_by_score() {
        let pool = crate::db::create_test_pool();
        let conn = pool.get().unwrap();


        seed_contact(&conn, "acc1", "a@test.com", "weak", 0.2);
        seed_contact(&conn, "acc1", "b@test.com", "strong", 0.9);
        seed_contact(&conn, "acc1", "c@test.com", "regular", 0.5);

        let rows: Vec<(String, f64)> = {
            let mut stmt = conn
                .prepare(
                    "SELECT email, overall_score FROM relationship_details ORDER BY overall_score DESC",
                )
                .unwrap();
            stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                .unwrap()
                .filter_map(|r| r.ok())
                .collect()
        };

        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].0, "b@test.com");
        assert_eq!(rows[1].0, "c@test.com");
        assert_eq!(rows[2].0, "a@test.com");
    }

    #[test]
    fn test_list_sort_by_name() {
        let pool = crate::db::create_test_pool();
        let conn = pool.get().unwrap();


        conn.execute(
            "INSERT INTO relationship_details
                (account_id, email, display_name, strength_label, overall_score,
                 frequency_score, recency_score, reciprocity_score, response_time_score,
                 thread_engagement_score, total_sent, total_received, computed_at)
             VALUES ('acc1','z@test.com','Zebra','weak',0.2,0,0,0,0,0,0,1,unixepoch())",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO relationship_details
                (account_id, email, display_name, strength_label, overall_score,
                 frequency_score, recency_score, reciprocity_score, response_time_score,
                 thread_engagement_score, total_sent, total_received, computed_at)
             VALUES ('acc1','a@test.com','Alice','strong',0.9,0,0,0,0,0,0,1,unixepoch())",
            [],
        ).unwrap();

        let rows: Vec<String> = {
            let mut stmt = conn
                .prepare(
                    "SELECT email FROM relationship_details ORDER BY LOWER(COALESCE(display_name, email)) ASC",
                )
                .unwrap();
            stmt.query_map([], |row| row.get(0))
                .unwrap()
                .filter_map(|r| r.ok())
                .collect()
        };

        assert_eq!(rows[0], "a@test.com");
        assert_eq!(rows[1], "z@test.com");
    }

    #[test]
    fn test_stats_by_strength() {
        let pool = crate::db::create_test_pool();
        let conn = pool.get().unwrap();


        seed_contact(&conn, "acc1", "s1@test.com", "strong", 0.8);
        seed_contact(&conn, "acc1", "s2@test.com", "strong", 0.75);
        seed_contact(&conn, "acc1", "r1@test.com", "regular", 0.5);
        seed_contact(&conn, "acc1", "w1@test.com", "weak", 0.25);

        let mut counts: HashMap<String, i64> = HashMap::new();
        let mut stmt = conn
            .prepare("SELECT strength_label, COUNT(*) FROM relationship_details GROUP BY strength_label")
            .unwrap();
        let rows = stmt
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))
            .unwrap();
        for row in rows.flatten() {
            counts.insert(row.0, row.1);
        }

        assert_eq!(counts.get("strong").copied().unwrap_or(0), 2);
        assert_eq!(counts.get("regular").copied().unwrap_or(0), 1);
        assert_eq!(counts.get("weak").copied().unwrap_or(0), 1);
        assert_eq!(counts.get("dormant").copied().unwrap_or(0), 0);
    }

    #[test]
    fn test_stats_most_active() {
        let pool = crate::db::create_test_pool();
        let conn = pool.get().unwrap();


        seed_contact(&conn, "acc1", "top@test.com", "strong", 0.95);
        seed_contact(&conn, "acc1", "mid@test.com", "regular", 0.5);

        let (email, score): (String, f64) = conn
            .query_row(
                "SELECT email, overall_score FROM relationship_details ORDER BY overall_score DESC LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();

        assert_eq!(email, "top@test.com");
        assert!((score - 0.95).abs() < 0.001);
    }

    #[test]
    fn test_relationship_table_exists() {
        let pool = crate::db::create_test_pool();
        let conn = pool.get().unwrap();


        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='relationship_details'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_compute_detailed_scores_no_accounts() {
        let pool = crate::db::create_test_pool();
        let conn = pool.get().unwrap();


        // No messages for this account, so scores should be empty
        let results = compute_detailed_scores(&conn, "nonexistent", "nobody@example.com", 1700000000).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_compute_detailed_scores_with_messages() {
        let pool = crate::db::create_test_pool();
        let conn = pool.get().unwrap();


        // Insert account
        conn.execute(
            "INSERT INTO accounts (id, email, provider, is_active) VALUES ('acc1', 'user@example.com', 'gmail', 1)",
            [],
        )
        .unwrap();

        let now = 1700000000_i64;

        // 5 messages received from alice
        for i in 0..5 {
            conn.execute(
                "INSERT INTO messages (id, account_id, folder, from_address, date, is_deleted, is_draft)
                 VALUES (?1, 'acc1', 'INBOX', 'alice@example.com', ?2, 0, 0)",
                rusqlite::params![format!("m_recv_{i}"), now - i * 86400],
            )
            .unwrap();
        }

        // 3 messages sent to alice
        for i in 0..3 {
            conn.execute(
                "INSERT INTO messages (id, account_id, folder, from_address, to_addresses, date, is_deleted, is_draft)
                 VALUES (?1, 'acc1', 'Sent', 'user@example.com', '[\"alice@example.com\"]', ?2, 0, 0)",
                rusqlite::params![format!("m_sent_{i}"), now - i * 86400 + 3600],
            )
            .unwrap();
        }

        let results = compute_detailed_scores(&conn, "acc1", "user@example.com", now).unwrap();
        assert_eq!(results.len(), 1, "should have exactly one contact");

        let (email, detail) = &results[0];
        assert_eq!(email, "alice@example.com");
        assert_eq!(detail.total_received, 5);
        assert_eq!(detail.total_sent, 3);
        assert!(detail.overall_score > 0.0, "score should be positive");
        // reciprocity: min(3,5)/max(3,5) = 3/5 = 0.6
        assert!((detail.reciprocity_score - 0.6).abs() < 0.01, "reciprocity: {}", detail.reciprocity_score);
    }

    #[test]
    fn test_route_registration() {
        // Verify that handler function signatures are correct (compile-time check).
        // We just call type checking by ensuring the functions can be referenced.
        let _ = compute_relationships as fn(State<Arc<AppState>>) -> _;
        let _ = list_relationships as fn(State<Arc<AppState>>, Query<ListRelationshipsParams>) -> _;
        let _ = get_relationship as fn(State<Arc<AppState>>, Path<String>) -> _;
        let _ = relationship_stats as fn(State<Arc<AppState>>) -> _;
    }
}

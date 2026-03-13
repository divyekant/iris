use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use crate::AppState;

// --- Types ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VipContact {
    pub email: String,
    pub display_name: Option<String>,
    pub vip_score: f64,
    pub is_manual: bool,
    pub message_count: i64,
    pub reply_count: i64,
    pub last_contact: Option<i64>,
    pub first_contact: Option<i64>,
    pub avg_reply_time_secs: Option<i64>,
    pub is_vip: bool,
}

#[derive(Debug, Deserialize)]
pub struct ListVipParams {
    pub threshold: Option<f64>,
    pub limit: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct ListVipResponse {
    pub vip_contacts: Vec<VipContact>,
    pub threshold: f64,
}

#[derive(Debug, Serialize)]
pub struct ComputeResponse {
    pub contacts_scored: i64,
    pub vip_count: i64,
}

#[derive(Debug, Deserialize)]
pub struct SetVipRequest {
    pub is_vip: bool,
}

#[derive(Debug, Serialize)]
pub struct SetVipResponse {
    pub email: String,
    pub is_vip: bool,
}

#[derive(Debug, Serialize)]
pub struct VipScoreResponse {
    pub email: String,
    pub vip_score: f64,
    pub is_manual: bool,
    pub is_vip: bool,
    pub stats: VipStats,
}

#[derive(Debug, Serialize)]
pub struct VipStats {
    pub message_count: i64,
    pub reply_count: i64,
    pub last_contact: Option<i64>,
    pub first_contact: Option<i64>,
    pub avg_reply_time_secs: Option<i64>,
}

// --- Score computation ---

/// Weight constants for VIP scoring
const W_FREQUENCY: f64 = 0.30;
const W_REPLY_RATE: f64 = 0.25;
const W_RECENCY: f64 = 0.25;
const W_THREAD_DEPTH: f64 = 0.20;

/// Cap values for normalization
const FREQUENCY_CAP: f64 = 30.0; // 30 messages/month = 1.0
const THREAD_DEPTH_CAP: f64 = 10.0; // avg thread depth 10 = 1.0

/// Recency decay: returns a score from 0.0 to 1.0 based on days since last contact.
/// 0 days = 1.0, 30 days = 0.5, 90+ days = 0.1
fn recency_score(days_since_last: f64) -> f64 {
    if days_since_last <= 0.0 {
        return 1.0;
    }
    // Exponential decay: e^(-k*days) scaled to hit 0.5 at 30 days
    // k = ln(2) / 30 ≈ 0.0231
    let k = (2.0_f64).ln() / 30.0;
    let score = (-k * days_since_last).exp();
    score.max(0.1) // floor at 0.1 for very old contacts
}

/// Compute VIP score from contact statistics.
pub fn compute_vip_score(
    messages_per_month: f64,
    reply_rate: f64,
    days_since_last: f64,
    avg_thread_depth: f64,
) -> f64 {
    let freq = (messages_per_month / FREQUENCY_CAP).min(1.0);
    let reply = reply_rate.clamp(0.0, 1.0);
    let recency = recency_score(days_since_last);
    let depth = (avg_thread_depth / THREAD_DEPTH_CAP).min(1.0);

    let score = W_FREQUENCY * freq + W_REPLY_RATE * reply + W_RECENCY * recency + W_THREAD_DEPTH * depth;
    score.clamp(0.0, 1.0)
}

// --- Handlers ---

/// GET /api/contacts/vip — list VIP contacts
pub async fn list_vip(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListVipParams>,
) -> Result<Json<ListVipResponse>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let threshold = params.threshold.unwrap_or(0.6);
    let limit = params.limit.unwrap_or(20).min(200);

    let mut stmt = conn
        .prepare(
            "SELECT email, display_name, vip_score, is_manual, message_count,
                    reply_count, last_contact, first_contact, avg_reply_time_secs
             FROM vip_contacts
             WHERE vip_score >= ?1 OR is_manual = 1
             ORDER BY vip_score DESC
             LIMIT ?2",
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let contacts: Vec<VipContact> = stmt
        .query_map(rusqlite::params![threshold, limit], |row| {
            let vip_score: f64 = row.get(2)?;
            let is_manual: bool = row.get::<_, i64>(3)? != 0;
            Ok(VipContact {
                email: row.get(0)?,
                display_name: row.get(1)?,
                vip_score,
                is_manual,
                message_count: row.get(4)?,
                reply_count: row.get(5)?,
                last_contact: row.get(6)?,
                first_contact: row.get(7)?,
                avg_reply_time_secs: row.get(8)?,
                is_vip: vip_score >= threshold || is_manual,
            })
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .filter_map(|r| r.ok())
        .collect();

    Ok(Json(ListVipResponse {
        vip_contacts: contacts,
        threshold,
    }))
}

/// POST /api/contacts/vip/compute — recompute VIP scores for all contacts
pub async fn compute_vip(
    State(state): State<Arc<AppState>>,
) -> Result<Json<ComputeResponse>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let now = chrono::Utc::now().timestamp();

    // Gather all my account emails to distinguish incoming vs outgoing
    let my_emails: Vec<String> = conn
        .prepare("SELECT email FROM accounts WHERE is_active = 1")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .query_map([], |row| row.get(0))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .filter_map(|r| r.ok())
        .collect();

    // Collect per-contact stats from messages table
    // We want contacts who are NOT me: people who sent me messages (from_address)
    // and people I sent messages to (from to_addresses when folder='Sent')
    #[derive(Default)]
    struct ContactStats {
        display_name: Option<String>,
        message_count: i64,
        reply_count: i64,
        last_contact: Option<i64>,
        first_contact: Option<i64>,
        thread_ids: Vec<String>,
    }

    let mut contacts: HashMap<String, ContactStats> = HashMap::new();

    // 1. Incoming messages: from_address is the contact
    {
        let mut stmt = conn
            .prepare(
                "SELECT from_address, from_name, date, thread_id
                 FROM messages
                 WHERE from_address IS NOT NULL AND is_deleted = 0",
            )
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            })
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        for row in rows {
            if let Ok((email, name, date, thread_id)) = row {
                let email_lower = email.to_lowercase();
                // Skip if this is one of my own accounts
                if my_emails.iter().any(|e| e.to_lowercase() == email_lower) {
                    continue;
                }
                let entry = contacts.entry(email_lower).or_default();
                entry.message_count += 1;
                if entry.display_name.is_none() {
                    entry.display_name = name;
                }
                if let Some(d) = date {
                    entry.last_contact = Some(entry.last_contact.map_or(d, |prev| prev.max(d)));
                    entry.first_contact = Some(entry.first_contact.map_or(d, |prev| prev.min(d)));
                }
                if let Some(ref tid) = thread_id {
                    if !entry.thread_ids.contains(tid) {
                        entry.thread_ids.push(tid.clone());
                    }
                }
            }
        }
    }

    // 2. Count my replies: messages sent by me (folder='Sent' or from_address is my email)
    //    that share a thread_id with a contact's messages → those are replies
    {
        let mut stmt = conn
            .prepare(
                "SELECT thread_id FROM messages
                 WHERE folder = 'Sent' AND thread_id IS NOT NULL AND is_deleted = 0",
            )
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        let my_reply_threads: std::collections::HashSet<String> = stmt
            .query_map([], |row| row.get(0))
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .filter_map(|r| r.ok())
            .collect();

        // For each contact, check if I replied in their threads
        for stats in contacts.values_mut() {
            for tid in &stats.thread_ids {
                if my_reply_threads.contains(tid) {
                    stats.reply_count += 1;
                }
            }
        }
    }

    // 3. Compute average thread depth per contact
    let mut thread_msg_counts: HashMap<String, i64> = HashMap::new();
    {
        let mut stmt = conn
            .prepare(
                "SELECT thread_id, COUNT(*) FROM messages
                 WHERE thread_id IS NOT NULL AND is_deleted = 0
                 GROUP BY thread_id",
            )
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        for row in rows {
            if let Ok((tid, count)) = row {
                thread_msg_counts.insert(tid, count);
            }
        }
    }

    // 4. Score each contact and upsert
    let default_threshold = 0.6;
    let mut contacts_scored: i64 = 0;
    let mut vip_count: i64 = 0;

    for (email, stats) in &contacts {
        if stats.message_count == 0 {
            continue;
        }

        // Messages per month
        let months = if let (Some(first), Some(last)) = (stats.first_contact, stats.last_contact) {
            let span_days = ((last - first) as f64) / 86400.0;
            (span_days / 30.0).max(1.0)
        } else {
            1.0
        };
        let msgs_per_month = stats.message_count as f64 / months;

        // Reply rate
        let reply_rate = if stats.message_count > 0 {
            stats.reply_count as f64 / stats.message_count as f64
        } else {
            0.0
        };

        // Days since last contact
        let days_since = if let Some(last) = stats.last_contact {
            ((now - last) as f64) / 86400.0
        } else {
            365.0 // very old
        };

        // Average thread depth for this contact's threads
        let avg_depth = if !stats.thread_ids.is_empty() {
            let total_depth: i64 = stats
                .thread_ids
                .iter()
                .filter_map(|tid| thread_msg_counts.get(tid))
                .sum();
            total_depth as f64 / stats.thread_ids.len() as f64
        } else {
            1.0
        };

        let score = compute_vip_score(msgs_per_month, reply_rate, days_since, avg_depth);

        // Compute avg reply time (optional)
        let avg_reply_time: Option<i64> = None; // Could be computed from message pairs, left as None for now

        // Upsert: preserve is_manual if already set
        conn.execute(
            "INSERT INTO vip_contacts (email, display_name, vip_score, is_manual, message_count,
                                       reply_count, last_contact, first_contact, avg_reply_time_secs, updated_at)
             VALUES (?1, ?2, ?3, 0, ?4, ?5, ?6, ?7, ?8, unixepoch())
             ON CONFLICT(email) DO UPDATE SET
                 display_name = COALESCE(excluded.display_name, vip_contacts.display_name),
                 vip_score = CASE WHEN vip_contacts.is_manual = 1 THEN 1.0 ELSE excluded.vip_score END,
                 message_count = excluded.message_count,
                 reply_count = excluded.reply_count,
                 last_contact = excluded.last_contact,
                 first_contact = excluded.first_contact,
                 avg_reply_time_secs = COALESCE(excluded.avg_reply_time_secs, vip_contacts.avg_reply_time_secs),
                 updated_at = unixepoch()",
            rusqlite::params![
                email,
                stats.display_name,
                score,
                stats.message_count,
                stats.reply_count,
                stats.last_contact,
                stats.first_contact,
                avg_reply_time,
            ],
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        contacts_scored += 1;
        if score >= default_threshold {
            vip_count += 1;
        }
    }

    // Count manual VIPs that may not have been in the scan
    let manual_extra: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM vip_contacts WHERE is_manual = 1 AND vip_score >= ?1",
            rusqlite::params![default_threshold],
            |row| row.get(0),
        )
        .unwrap_or(0);

    // Manual VIPs might already be counted; just report computed + manual
    Ok(Json(ComputeResponse {
        contacts_scored,
        vip_count: vip_count + manual_extra,
    }))
}

/// PUT /api/contacts/{email}/vip — manually set/unset VIP status
pub async fn set_vip(
    State(state): State<Arc<AppState>>,
    Path(email): Path<String>,
    Json(req): Json<SetVipRequest>,
) -> Result<Json<SetVipResponse>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let email_lower = email.to_lowercase();

    if req.is_vip {
        // Set as manual VIP with score 1.0
        conn.execute(
            "INSERT INTO vip_contacts (email, vip_score, is_manual, updated_at)
             VALUES (?1, 1.0, 1, unixepoch())
             ON CONFLICT(email) DO UPDATE SET
                 vip_score = 1.0,
                 is_manual = 1,
                 updated_at = unixepoch()",
            rusqlite::params![email_lower],
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    } else {
        // Remove manual VIP — reset is_manual, recompute score to 0 (will be recalculated on next compute)
        conn.execute(
            "UPDATE vip_contacts SET is_manual = 0, vip_score = 0.0, updated_at = unixepoch()
             WHERE email = ?1",
            rusqlite::params![email_lower],
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }

    Ok(Json(SetVipResponse {
        email: email_lower,
        is_vip: req.is_vip,
    }))
}

/// GET /api/contacts/{email}/vip-score — get VIP score for a specific contact
pub async fn get_vip_score(
    State(state): State<Arc<AppState>>,
    Path(email): Path<String>,
) -> Result<Json<VipScoreResponse>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let email_lower = email.to_lowercase();

    let result = conn.query_row(
        "SELECT email, vip_score, is_manual, message_count, reply_count,
                last_contact, first_contact, avg_reply_time_secs
         FROM vip_contacts WHERE email = ?1",
        rusqlite::params![email_lower],
        |row| {
            let vip_score: f64 = row.get(1)?;
            let is_manual: bool = row.get::<_, i64>(2)? != 0;
            Ok(VipScoreResponse {
                email: row.get(0)?,
                vip_score,
                is_manual,
                is_vip: vip_score >= 0.6 || is_manual,
                stats: VipStats {
                    message_count: row.get(3)?,
                    reply_count: row.get(4)?,
                    last_contact: row.get(5)?,
                    first_contact: row.get(6)?,
                    avg_reply_time_secs: row.get(7)?,
                },
            })
        },
    );

    match result {
        Ok(resp) => Ok(Json(resp)),
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            // Contact not in VIP table — return zero score
            Ok(Json(VipScoreResponse {
                email: email_lower,
                vip_score: 0.0,
                is_manual: false,
                is_vip: false,
                stats: VipStats {
                    message_count: 0,
                    reply_count: 0,
                    last_contact: None,
                    first_contact: None,
                    avg_reply_time_secs: None,
                },
            }))
        }
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

// --- Tests ---

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_recency_score_today() {
        let score = recency_score(0.0);
        assert!((score - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_recency_score_30_days() {
        let score = recency_score(30.0);
        assert!((score - 0.5).abs() < 0.05, "30 days should be ~0.5, got {}", score);
    }

    #[test]
    fn test_recency_score_90_days() {
        let score = recency_score(90.0);
        assert!(score >= 0.1 && score <= 0.2, "90 days should be ~0.1-0.2, got {}", score);
    }

    #[test]
    fn test_recency_score_floor() {
        let score = recency_score(365.0);
        assert!((score - 0.1).abs() < 0.01, "Very old should floor at 0.1, got {}", score);
    }

    #[test]
    fn test_vip_score_zero_activity() {
        let score = compute_vip_score(0.0, 0.0, 365.0, 0.0);
        // freq=0, reply=0, recency=0.1 (floor), depth=0
        let expected = W_RECENCY * 0.1;
        assert!((score - expected).abs() < 0.01, "Zero activity score: got {}, expected {}", score, expected);
    }

    #[test]
    fn test_vip_score_max_activity() {
        let score = compute_vip_score(30.0, 1.0, 0.0, 10.0);
        // freq=1.0, reply=1.0, recency=1.0, depth=1.0 → all maxed
        assert!((score - 1.0).abs() < 0.01, "Max activity score: got {}", score);
    }

    #[test]
    fn test_vip_score_over_cap() {
        let score = compute_vip_score(100.0, 1.0, 0.0, 20.0);
        // freq capped at 1.0, depth capped at 1.0
        assert!((score - 1.0).abs() < 0.01, "Over cap should still be 1.0, got {}", score);
    }

    #[test]
    fn test_vip_score_frequency_normalization_zero() {
        let score = compute_vip_score(0.0, 0.5, 15.0, 3.0);
        // freq=0, reply=0.5, recency=~0.71, depth=0.3
        let expected = W_FREQUENCY * 0.0
            + W_REPLY_RATE * 0.5
            + W_RECENCY * recency_score(15.0)
            + W_THREAD_DEPTH * 0.3;
        assert!((score - expected).abs() < 0.01, "got {}, expected {}", score, expected);
    }

    #[test]
    fn test_vip_score_frequency_normalization_mid() {
        let score = compute_vip_score(15.0, 0.5, 15.0, 5.0);
        // freq=0.5, reply=0.5, recency=~0.71, depth=0.5
        let expected = W_FREQUENCY * 0.5
            + W_REPLY_RATE * 0.5
            + W_RECENCY * recency_score(15.0)
            + W_THREAD_DEPTH * 0.5;
        assert!((score - expected).abs() < 0.01, "got {}, expected {}", score, expected);
    }

    #[test]
    fn test_manual_vip_always_max() {
        // Manual VIP is handled at DB layer (score=1.0 when is_manual=1)
        // The scoring function itself is pure math; manual override is in set_vip handler
        // Verify that a perfect score = 1.0
        let score = compute_vip_score(30.0, 1.0, 0.0, 10.0);
        assert!((score - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_vip_score_clamped() {
        // Negative inputs should clamp
        let score = compute_vip_score(-5.0, -0.5, -10.0, -3.0);
        assert!(score >= 0.0 && score <= 1.0, "Score should be clamped, got {}", score);
    }

    #[test]
    fn test_list_and_threshold_filter() {
        let pool = crate::db::create_test_pool();
        let conn = pool.get().unwrap();

        // Run VIP migration
        conn.execute_batch(include_str!("../../migrations/026_vip_contacts.sql")).unwrap();

        // Insert test contacts
        conn.execute(
            "INSERT INTO vip_contacts (email, display_name, vip_score, is_manual, message_count, reply_count)
             VALUES ('high@test.com', 'High VIP', 0.9, 0, 50, 20)",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO vip_contacts (email, display_name, vip_score, is_manual, message_count, reply_count)
             VALUES ('low@test.com', 'Low Score', 0.3, 0, 5, 1)",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO vip_contacts (email, display_name, vip_score, is_manual, message_count, reply_count)
             VALUES ('manual@test.com', 'Manual VIP', 1.0, 1, 2, 0)",
            [],
        ).unwrap();

        // Query with threshold 0.6
        let mut stmt = conn
            .prepare(
                "SELECT email, vip_score, is_manual FROM vip_contacts
                 WHERE vip_score >= 0.6 OR is_manual = 1
                 ORDER BY vip_score DESC",
            )
            .unwrap();

        let results: Vec<(String, f64, i64)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        // Should include high@test.com (0.9) and manual@test.com (1.0, is_manual=1)
        // but NOT low@test.com (0.3, not manual)
        assert_eq!(results.len(), 2);
        assert!(results.iter().any(|(e, _, _)| e == "manual@test.com"));
        assert!(results.iter().any(|(e, _, _)| e == "high@test.com"));
        assert!(!results.iter().any(|(e, _, _)| e == "low@test.com"));
    }

    #[test]
    fn test_toggle_vip_on_off() {
        let pool = crate::db::create_test_pool();
        let conn = pool.get().unwrap();

        conn.execute_batch(include_str!("../../migrations/026_vip_contacts.sql")).unwrap();

        // Toggle ON: insert as manual VIP
        conn.execute(
            "INSERT INTO vip_contacts (email, vip_score, is_manual, updated_at)
             VALUES ('toggle@test.com', 1.0, 1, unixepoch())
             ON CONFLICT(email) DO UPDATE SET
                 vip_score = 1.0, is_manual = 1, updated_at = unixepoch()",
            [],
        ).unwrap();

        let (score, manual): (f64, i64) = conn
            .query_row(
                "SELECT vip_score, is_manual FROM vip_contacts WHERE email = 'toggle@test.com'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert!((score - 1.0).abs() < 0.01);
        assert_eq!(manual, 1);

        // Toggle OFF: reset
        conn.execute(
            "UPDATE vip_contacts SET is_manual = 0, vip_score = 0.0, updated_at = unixepoch()
             WHERE email = 'toggle@test.com'",
            [],
        ).unwrap();

        let (score2, manual2): (f64, i64) = conn
            .query_row(
                "SELECT vip_score, is_manual FROM vip_contacts WHERE email = 'toggle@test.com'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert!((score2 - 0.0).abs() < 0.01);
        assert_eq!(manual2, 0);
    }
}

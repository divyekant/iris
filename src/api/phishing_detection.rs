use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::AppState;

// ---------------------------------------------------------------------------
// Signal detection constants
// ---------------------------------------------------------------------------

const URGENCY_WORDS: &[&str] = &[
    "urgent",
    "immediately",
    "act now",
    "expires",
    "suspended",
    "verify your account",
    "action required",
    "limited time",
    "account will be closed",
    "confirm your identity",
];

const CREDENTIAL_WORDS: &[&str] = &[
    "password",
    "ssn",
    "social security",
    "credit card",
    "bank account",
    "routing number",
    "account number",
    "pin number",
    "security code",
    "cvv",
];

const KNOWN_BRANDS: &[(&str, &[&str])] = &[
    ("google", &["google.com", "gmail.com", "youtube.com"]),
    ("apple", &["apple.com", "icloud.com"]),
    ("microsoft", &["microsoft.com", "outlook.com", "live.com", "hotmail.com"]),
    ("amazon", &["amazon.com", "amazon.co.uk"]),
    ("paypal", &["paypal.com"]),
    ("netflix", &["netflix.com"]),
    ("facebook", &["facebook.com", "meta.com"]),
    ("instagram", &["instagram.com"]),
    ("twitter", &["twitter.com", "x.com"]),
    ("linkedin", &["linkedin.com"]),
    ("dropbox", &["dropbox.com"]),
    ("chase", &["chase.com"]),
    ("wells fargo", &["wellsfargo.com"]),
    ("bank of america", &["bankofamerica.com"]),
];

const GENERIC_GREETINGS: &[&str] = &[
    "dear customer",
    "dear user",
    "dear account holder",
    "dear valued customer",
    "dear sir/madam",
    "dear sir or madam",
    "dear member",
];

// ---------------------------------------------------------------------------
// Signal weights
// ---------------------------------------------------------------------------

const WEIGHT_URGENCY_LANGUAGE: f64 = 0.15;
const WEIGHT_SENDER_MISMATCH: f64 = 0.20;
const WEIGHT_SUSPICIOUS_LINKS: f64 = 0.25;
const WEIGHT_CREDENTIAL_REQUEST: f64 = 0.25;
const WEIGHT_SPOOFED_BRAND: f64 = 0.20;
const WEIGHT_EXCESSIVE_URGENCY: f64 = 0.15;
const WEIGHT_SUSPICIOUS_GREETING: f64 = 0.10;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhishingSignal {
    pub signal_type: String,
    pub description: String,
    pub weight: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhishingReport {
    pub id: i64,
    pub message_id: String,
    pub account_id: i64,
    pub risk_level: String,
    pub risk_score: f64,
    pub signals: Vec<PhishingSignal>,
    pub ai_analysis: Option<String>,
    pub checked_at: String,
}

#[derive(Debug, Serialize)]
pub struct ScanResponse {
    pub message_id: String,
    pub risk_level: String,
    pub risk_score: f64,
    pub signals: Vec<PhishingSignal>,
    pub ai_analysis: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct BulkScanRequest {
    pub message_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct BulkScanResponse {
    pub results: Vec<ScanResponse>,
}

#[derive(Debug, Deserialize)]
pub struct ReportsQuery {
    pub risk_level: Option<String>,
    pub account_id: Option<i64>,
    pub limit: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct PhishingStats {
    pub total_scans: i64,
    pub by_risk_level: Vec<RiskLevelCount>,
    pub common_signals: Vec<SignalCount>,
}

#[derive(Debug, Serialize)]
pub struct RiskLevelCount {
    pub risk_level: String,
    pub count: i64,
}

#[derive(Debug, Serialize)]
pub struct SignalCount {
    pub signal_type: String,
    pub count: i64,
}

// ---------------------------------------------------------------------------
// Signal detection engine
// ---------------------------------------------------------------------------

/// Extract the domain part from an email address.
fn extract_email_domain(email: &str) -> Option<String> {
    email.split('@').nth(1).map(|d| d.trim().to_lowercase())
}

/// Extract the domain from a URL.
fn extract_url_domain(url: &str) -> Option<String> {
    let without_protocol = if let Some(pos) = url.find("://") {
        &url[pos + 3..]
    } else {
        url
    };
    let domain = without_protocol.split('/').next().unwrap_or(without_protocol);
    let domain = domain.split(':').next().unwrap_or(domain);
    if domain.is_empty() {
        None
    } else {
        Some(domain.to_lowercase())
    }
}

/// Check if domain matches any of the allowed domains for a brand (including subdomains).
fn domain_matches_brand(domain: &str, brand_domains: &[&str]) -> bool {
    brand_domains.iter().any(|bd| domain == *bd || domain.ends_with(&format!(".{}", bd)))
}

/// Detect all phishing signals from email content.
pub fn detect_signals(
    subject: &str,
    body_text: &str,
    body_html: &str,
    from_address: &str,
    from_name: &str,
) -> Vec<PhishingSignal> {
    let mut signals = Vec::new();
    let subject_lower = subject.to_lowercase();
    let body_lower = body_text.to_lowercase();
    let combined_lower = format!("{} {}", subject_lower, body_lower);

    // 1. Urgency language
    let urgency_count = URGENCY_WORDS
        .iter()
        .filter(|w| combined_lower.contains(**w))
        .count();
    if urgency_count > 0 {
        signals.push(PhishingSignal {
            signal_type: "urgency_language".to_string(),
            description: format!(
                "Found {} urgency indicator(s) in message content",
                urgency_count
            ),
            weight: WEIGHT_URGENCY_LANGUAGE,
        });
    }

    // 2. Excessive urgency (multiple urgency indicators)
    if urgency_count >= 3 {
        signals.push(PhishingSignal {
            signal_type: "excessive_urgency".to_string(),
            description: format!(
                "Excessive urgency: {} indicators found, suggesting pressure tactics",
                urgency_count
            ),
            weight: WEIGHT_EXCESSIVE_URGENCY,
        });
    }

    // 3. Sender mismatch — display name suggests a brand but domain doesn't match
    let from_name_lower = from_name.to_lowercase();
    let sender_domain = extract_email_domain(from_address).unwrap_or_default();
    for (brand, domains) in KNOWN_BRANDS {
        if from_name_lower.contains(brand) && !domain_matches_brand(&sender_domain, domains) {
            signals.push(PhishingSignal {
                signal_type: "sender_mismatch".to_string(),
                description: format!(
                    "Display name contains '{}' but sender domain is '{}'",
                    brand, sender_domain
                ),
                weight: WEIGHT_SENDER_MISMATCH,
            });
            break;
        }
    }

    // 4. Spoofed brand — content mentions known brands but sender doesn't match
    for (brand, domains) in KNOWN_BRANDS {
        if combined_lower.contains(brand) && !domain_matches_brand(&sender_domain, domains) {
            // Only flag if sender domain doesn't match any known brand
            let sender_is_any_brand = KNOWN_BRANDS
                .iter()
                .any(|(_, ds)| domain_matches_brand(&sender_domain, ds));
            if !sender_is_any_brand {
                signals.push(PhishingSignal {
                    signal_type: "spoofed_brand".to_string(),
                    description: format!(
                        "Content references '{}' but sender domain '{}' is not associated with that brand",
                        brand, sender_domain
                    ),
                    weight: WEIGHT_SPOOFED_BRAND,
                });
                break; // only one spoofed_brand signal
            }
        }
    }

    // 5. Credential request
    let cred_found: Vec<&&str> = CREDENTIAL_WORDS
        .iter()
        .filter(|w| body_lower.contains(**w))
        .collect();
    if !cred_found.is_empty() {
        signals.push(PhishingSignal {
            signal_type: "credential_request".to_string(),
            description: format!(
                "Message requests sensitive information: {}",
                cred_found.iter().map(|w| format!("'{}'", w)).collect::<Vec<_>>().join(", ")
            ),
            weight: WEIGHT_CREDENTIAL_REQUEST,
        });
    }

    // 6. Suspicious links — href points to different domain than displayed URL text
    detect_suspicious_links(body_html, &mut signals);

    // 7. Suspicious greeting
    for greeting in GENERIC_GREETINGS {
        if body_lower.contains(greeting) {
            signals.push(PhishingSignal {
                signal_type: "suspicious_greeting".to_string(),
                description: format!("Generic greeting '{}' used instead of personal name", greeting),
                weight: WEIGHT_SUSPICIOUS_GREETING,
            });
            break;
        }
    }

    signals
}

/// Detect links where the visible text is a URL but the href points elsewhere.
fn detect_suspicious_links(html: &str, signals: &mut Vec<PhishingSignal>) {
    let html_lower = html.to_lowercase();
    let mut search_from = 0;

    while let Some(a_start) = html_lower[search_from..].find("<a ") {
        let abs_start = search_from + a_start;
        let tag_end = match html_lower[abs_start..].find('>') {
            Some(pos) => abs_start + pos + 1,
            None => break,
        };

        // Find closing </a> tag
        let close_tag = match html_lower[tag_end..].find("</a>") {
            Some(pos) => tag_end + pos,
            None => {
                search_from = tag_end;
                continue;
            }
        };

        let tag = &html[abs_start..tag_end];
        let tag_lower = &html_lower[abs_start..tag_end];
        let link_text = &html[tag_end..close_tag].trim();
        let link_text_lower = link_text.to_lowercase();

        // Extract href
        if let Some(href) = extract_href(tag, tag_lower) {
            // Check if link text looks like a URL
            if link_text_lower.starts_with("http://")
                || link_text_lower.starts_with("https://")
                || link_text_lower.starts_with("www.")
            {
                let display_domain = extract_url_domain(&link_text_lower);
                let href_domain = extract_url_domain(&href.to_lowercase());

                if let (Some(dd), Some(hd)) = (display_domain, href_domain) {
                    if dd != hd && !hd.ends_with(&format!(".{}", dd)) && !dd.ends_with(&format!(".{}", hd)) {
                        signals.push(PhishingSignal {
                            signal_type: "suspicious_links".to_string(),
                            description: format!(
                                "Link displays '{}' but points to '{}'",
                                dd, hd
                            ),
                            weight: WEIGHT_SUSPICIOUS_LINKS,
                        });
                    }
                }
            }
        }

        search_from = close_tag + 4;
    }
}

/// Extract href attribute value from an <a> tag.
fn extract_href(tag: &str, tag_lower: &str) -> Option<String> {
    let search = "href=";
    let pos = tag_lower.find(search)?;
    let start = pos + search.len();
    let remaining = &tag[start..];

    if remaining.starts_with('"') {
        let end = remaining[1..].find('"')?;
        Some(remaining[1..1 + end].to_string())
    } else if remaining.starts_with('\'') {
        let end = remaining[1..].find('\'')?;
        Some(remaining[1..1 + end].to_string())
    } else {
        let value: String = remaining
            .chars()
            .take_while(|c| !c.is_whitespace() && *c != '>')
            .collect();
        if value.is_empty() {
            None
        } else {
            Some(value)
        }
    }
}

/// Calculate risk score from signals.
pub fn calculate_risk_score(signals: &[PhishingSignal]) -> f64 {
    let total: f64 = signals.iter().map(|s| s.weight).sum();
    total.min(1.0)
}

/// Map risk score to risk level.
pub fn risk_level_from_score(score: f64) -> &'static str {
    match score {
        s if s < 0.2 => "safe",
        s if s < 0.4 => "low",
        s if s < 0.6 => "medium",
        s if s < 0.8 => "high",
        _ => "critical",
    }
}

// ---------------------------------------------------------------------------
// API handlers
// ---------------------------------------------------------------------------

/// POST /api/security/phishing-scan/:message_id — scan a message for phishing signals
pub async fn scan_message(
    State(state): State<Arc<AppState>>,
    Path(message_id): Path<String>,
) -> Result<Json<ScanResponse>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Fetch the message
    let (subject, body_text, body_html, from_address, from_name, account_id): (
        String, String, String, String, String, String,
    ) = conn
        .query_row(
            "SELECT COALESCE(subject, ''), COALESCE(body_text, ''), COALESCE(body_html, ''),
                    COALESCE(from_address, ''), COALESCE(from_name, ''), account_id
             FROM messages WHERE id = ?1 AND is_deleted = 0",
            rusqlite::params![message_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )
        .map_err(|_| StatusCode::NOT_FOUND)?;

    // Rule-based detection
    let signals = detect_signals(&subject, &body_text, &body_html, &from_address, &from_name);
    let risk_score = calculate_risk_score(&signals);
    let risk_level = risk_level_from_score(risk_score).to_string();

    // Optional AI analysis (only if we have providers and signals were found)
    let ai_analysis = if !signals.is_empty() && state.providers.has_providers() {
        let signal_summary: Vec<String> = signals
            .iter()
            .map(|s| format!("- {}: {}", s.signal_type, s.description))
            .collect();
        let prompt = format!(
            "Analyze this email for phishing risk. Provide a brief (2-3 sentence) explanation of the overall risk.\n\n\
             Subject: {}\nFrom: {} <{}>\n\nBody excerpt: {}\n\n\
             Detected signals:\n{}\n\nRisk score: {:.2} ({})\n\n\
             Provide your analysis:",
            subject,
            from_name,
            from_address,
            &body_text.chars().take(500).collect::<String>(),
            signal_summary.join("\n"),
            risk_score,
            risk_level
        );

        let system = Some(
            "You are an email security analyst. Analyze phishing signals and provide a brief, \
             actionable assessment. Be specific about why the email is or isn't suspicious."
        );

        state.providers.generate(&prompt, system).await
    } else {
        None
    };

    // Store the report
    let signals_json = serde_json::to_string(&signals).unwrap_or_else(|_| "[]".to_string());
    conn.execute(
        "INSERT INTO phishing_reports (message_id, account_id, risk_level, risk_score, signals, ai_analysis)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![message_id, account_id, risk_level, risk_score, signals_json, ai_analysis],
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(ScanResponse {
        message_id,
        risk_level,
        risk_score,
        signals,
        ai_analysis,
    }))
}

/// GET /api/security/phishing-report/:message_id — get existing report
pub async fn get_report(
    State(state): State<Arc<AppState>>,
    Path(message_id): Path<String>,
) -> Result<Json<PhishingReport>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let report = conn
        .query_row(
            "SELECT id, message_id, account_id, risk_level, risk_score, signals, ai_analysis, checked_at
             FROM phishing_reports WHERE message_id = ?1
             ORDER BY checked_at DESC LIMIT 1",
            rusqlite::params![message_id],
            |row| {
                let signals_json: String = row.get(5)?;
                let signals: Vec<PhishingSignal> =
                    serde_json::from_str(&signals_json).unwrap_or_default();
                Ok(PhishingReport {
                    id: row.get(0)?,
                    message_id: row.get(1)?,
                    account_id: row.get(2)?,
                    risk_level: row.get(3)?,
                    risk_score: row.get(4)?,
                    signals,
                    ai_analysis: row.get(6)?,
                    checked_at: row.get(7)?,
                })
            },
        )
        .map_err(|_| StatusCode::NOT_FOUND)?;

    Ok(Json(report))
}

/// GET /api/security/phishing-reports — list reports with optional filters
pub async fn list_reports(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ReportsQuery>,
) -> Result<Json<Vec<PhishingReport>>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let limit = query.limit.unwrap_or(50).min(200);
    let mut sql = String::from(
        "SELECT id, message_id, account_id, risk_level, risk_score, signals, ai_analysis, checked_at
         FROM phishing_reports WHERE 1=1",
    );
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let mut param_idx = 1;

    if let Some(ref level) = query.risk_level {
        sql.push_str(&format!(" AND risk_level = ?{}", param_idx));
        params.push(Box::new(level.clone()));
        param_idx += 1;
    }
    if let Some(acct) = query.account_id {
        sql.push_str(&format!(" AND account_id = ?{}", param_idx));
        params.push(Box::new(acct));
        param_idx += 1;
    }

    sql.push_str(&format!(" ORDER BY checked_at DESC LIMIT ?{}", param_idx));
    params.push(Box::new(limit));

    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();

    let mut stmt = conn.prepare(&sql).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let reports: Vec<PhishingReport> = stmt
        .query_map(param_refs.as_slice(), |row| {
            let signals_json: String = row.get(5)?;
            let signals: Vec<PhishingSignal> =
                serde_json::from_str(&signals_json).unwrap_or_default();
            Ok(PhishingReport {
                id: row.get(0)?,
                message_id: row.get(1)?,
                account_id: row.get(2)?,
                risk_level: row.get(3)?,
                risk_score: row.get(4)?,
                signals,
                ai_analysis: row.get(6)?,
                checked_at: row.get(7)?,
            })
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .filter_map(|r| r.ok())
        .collect();

    Ok(Json(reports))
}

/// POST /api/security/phishing-bulk-scan — scan multiple messages
pub async fn bulk_scan(
    State(state): State<Arc<AppState>>,
    Json(req): Json<BulkScanRequest>,
) -> Result<Json<BulkScanResponse>, StatusCode> {
    if req.message_ids.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    if req.message_ids.len() > 100 {
        return Err(StatusCode::BAD_REQUEST);
    }

    let mut results = Vec::with_capacity(req.message_ids.len());

    for msg_id in &req.message_ids {
        let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        let row = conn.query_row(
            "SELECT COALESCE(subject, ''), COALESCE(body_text, ''), COALESCE(body_html, ''),
                    COALESCE(from_address, ''), COALESCE(from_name, ''), account_id
             FROM messages WHERE id = ?1 AND is_deleted = 0",
            rusqlite::params![msg_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                ))
            },
        );

        let (subject, body_text, body_html, from_address, from_name, account_id) = match row {
            Ok(data) => data,
            Err(_) => continue, // skip missing messages
        };

        let signals = detect_signals(&subject, &body_text, &body_html, &from_address, &from_name);
        let risk_score = calculate_risk_score(&signals);
        let risk_level = risk_level_from_score(risk_score).to_string();

        // Store the report
        let signals_json = serde_json::to_string(&signals).unwrap_or_else(|_| "[]".to_string());
        conn.execute(
            "INSERT INTO phishing_reports (message_id, account_id, risk_level, risk_score, signals, ai_analysis)
             VALUES (?1, ?2, ?3, ?4, ?5, NULL)",
            rusqlite::params![msg_id, account_id, risk_level, risk_score, signals_json],
        )
        .ok();

        results.push(ScanResponse {
            message_id: msg_id.clone(),
            risk_level,
            risk_score,
            signals,
            ai_analysis: None,
        });
    }

    Ok(Json(BulkScanResponse { results }))
}

/// GET /api/security/phishing-stats — statistics on phishing scans
pub async fn phishing_stats(
    State(state): State<Arc<AppState>>,
) -> Result<Json<PhishingStats>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let total: i64 = conn
        .query_row("SELECT COUNT(*) FROM phishing_reports", [], |row| row.get(0))
        .unwrap_or(0);

    let mut by_risk = Vec::new();
    {
        let mut stmt = conn
            .prepare(
                "SELECT risk_level, COUNT(*) as cnt FROM phishing_reports
                 GROUP BY risk_level ORDER BY cnt DESC",
            )
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let rows = stmt
            .query_map([], |row| {
                Ok(RiskLevelCount {
                    risk_level: row.get(0)?,
                    count: row.get(1)?,
                })
            })
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        for r in rows {
            if let Ok(rl) = r {
                by_risk.push(rl);
            }
        }
    }

    // Parse signals JSON and count signal types
    let mut signal_counts: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
    {
        let mut stmt = conn
            .prepare("SELECT signals FROM phishing_reports")
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let rows = stmt
            .query_map([], |row| {
                let json_str: String = row.get(0)?;
                Ok(json_str)
            })
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        for r in rows {
            if let Ok(json_str) = r {
                if let Ok(signals) = serde_json::from_str::<Vec<PhishingSignal>>(&json_str) {
                    for sig in signals {
                        *signal_counts.entry(sig.signal_type).or_insert(0) += 1;
                    }
                }
            }
        }
    }

    let mut common_signals: Vec<SignalCount> = signal_counts
        .into_iter()
        .map(|(signal_type, count)| SignalCount { signal_type, count })
        .collect();
    common_signals.sort_by(|a, b| b.count.cmp(&a.count));

    Ok(Json(PhishingStats {
        total_scans: total,
        by_risk_level: by_risk,
        common_signals,
    }))
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
            email: "phishing-test@example.com".to_string(),
            display_name: Some("Phishing Test".to_string()),
            imap_host: Some("imap.gmail.com".to_string()),
            imap_port: Some(993),
            smtp_host: Some("smtp.gmail.com".to_string()),
            smtp_port: Some(587),
            username: Some("phishing-test@example.com".to_string()),
            password: None,
        };
        Account::create(conn, &input)
    }

    fn make_message(
        account_id: &str,
        subject: &str,
        body_text: &str,
        from_address: &str,
        from_name: &str,
    ) -> InsertMessage {
        InsertMessage {
            account_id: account_id.to_string(),
            message_id: Some(format!("<{}-phish@example.com>", subject.replace(' ', "-"))),
            thread_id: None,
            folder: "INBOX".to_string(),
            from_address: Some(from_address.to_string()),
            from_name: Some(from_name.to_string()),
            to_addresses: Some(r#"["phishing-test@example.com"]"#.to_string()),
            cc_addresses: None,
            bcc_addresses: None,
            subject: Some(subject.to_string()),
            date: Some(1700000000),
            snippet: Some(body_text.chars().take(200).collect()),
            body_text: Some(body_text.to_string()),
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
            size_bytes: Some(1024),
            list_unsubscribe: None,
            list_unsubscribe_post: false,
        }
    }

    // --- Signal detection tests ---

    #[test]
    fn test_urgency_language_detected() {
        let signals = detect_signals(
            "URGENT: Your account is suspended",
            "You must act now or your account will be closed.",
            "",
            "scam@evil.com",
            "Scammer",
        );
        let urgency = signals.iter().find(|s| s.signal_type == "urgency_language");
        assert!(urgency.is_some(), "Should detect urgency language");
    }

    #[test]
    fn test_excessive_urgency_detected() {
        let signals = detect_signals(
            "URGENT: Verify your account immediately",
            "Act now before your account expires. Your account is suspended and will be closed.",
            "",
            "scam@evil.com",
            "Scammer",
        );
        let excessive = signals.iter().find(|s| s.signal_type == "excessive_urgency");
        assert!(excessive.is_some(), "Should detect excessive urgency with 3+ indicators");
    }

    #[test]
    fn test_sender_mismatch_detected() {
        let signals = detect_signals(
            "Important Notice",
            "Please review your account.",
            "",
            "support@evil-domain.com",
            "PayPal Security Team",
        );
        let mismatch = signals.iter().find(|s| s.signal_type == "sender_mismatch");
        assert!(mismatch.is_some(), "Should detect sender/display name mismatch");
    }

    #[test]
    fn test_no_sender_mismatch_for_legit_domain() {
        let signals = detect_signals(
            "Payment Confirmation",
            "Your payment was successful.",
            "",
            "support@paypal.com",
            "PayPal Support",
        );
        let mismatch = signals.iter().find(|s| s.signal_type == "sender_mismatch");
        assert!(mismatch.is_none(), "Should not flag legitimate PayPal domain");
    }

    #[test]
    fn test_credential_request_detected() {
        let signals = detect_signals(
            "Account Verification",
            "Please send us your password and credit card number to verify your identity.",
            "",
            "verify@evil.com",
            "Support",
        );
        let cred = signals.iter().find(|s| s.signal_type == "credential_request");
        assert!(cred.is_some(), "Should detect credential request");
    }

    #[test]
    fn test_spoofed_brand_detected() {
        let signals = detect_signals(
            "Your Google Account",
            "Google has detected suspicious activity on your account.",
            "",
            "alerts@phishing-domain.xyz",
            "Alert System",
        );
        let spoofed = signals.iter().find(|s| s.signal_type == "spoofed_brand");
        assert!(spoofed.is_some(), "Should detect spoofed brand");
    }

    #[test]
    fn test_suspicious_links_detected() {
        let signals = detect_signals(
            "Click here",
            "Please click the link.",
            r#"<a href="https://evil-site.com/steal">https://www.paypal.com/login</a>"#,
            "scam@evil.com",
            "Scammer",
        );
        let links = signals.iter().find(|s| s.signal_type == "suspicious_links");
        assert!(links.is_some(), "Should detect mismatched link text vs href");
    }

    #[test]
    fn test_no_suspicious_links_for_matching() {
        let signals = detect_signals(
            "Click here",
            "Visit our site.",
            r#"<a href="https://www.paypal.com/login">https://www.paypal.com/login</a>"#,
            "info@paypal.com",
            "PayPal",
        );
        let links = signals.iter().find(|s| s.signal_type == "suspicious_links");
        assert!(links.is_none(), "Should not flag matching link domains");
    }

    #[test]
    fn test_suspicious_greeting_detected() {
        let signals = detect_signals(
            "Account Notice",
            "Dear Customer, your account requires attention.",
            "",
            "notice@evil.com",
            "Support",
        );
        let greeting = signals.iter().find(|s| s.signal_type == "suspicious_greeting");
        assert!(greeting.is_some(), "Should detect generic greeting");
    }

    #[test]
    fn test_clean_email_no_signals() {
        let signals = detect_signals(
            "Team lunch tomorrow",
            "Hi team, let's grab lunch at noon tomorrow. My treat!",
            "",
            "colleague@mycompany.com",
            "Alice Johnson",
        );
        assert!(signals.is_empty(), "Clean email should have no signals");
    }

    // --- Risk scoring tests ---

    #[test]
    fn test_risk_score_calculation() {
        let signals = vec![
            PhishingSignal {
                signal_type: "urgency_language".to_string(),
                description: "test".to_string(),
                weight: 0.15,
            },
            PhishingSignal {
                signal_type: "credential_request".to_string(),
                description: "test".to_string(),
                weight: 0.25,
            },
        ];
        let score = calculate_risk_score(&signals);
        assert!((score - 0.4).abs() < 0.001);
    }

    #[test]
    fn test_risk_score_capped_at_1() {
        let signals = vec![
            PhishingSignal { signal_type: "a".to_string(), description: "t".to_string(), weight: 0.5 },
            PhishingSignal { signal_type: "b".to_string(), description: "t".to_string(), weight: 0.4 },
            PhishingSignal { signal_type: "c".to_string(), description: "t".to_string(), weight: 0.3 },
        ];
        let score = calculate_risk_score(&signals);
        assert_eq!(score, 1.0);
    }

    #[test]
    fn test_risk_level_mapping() {
        assert_eq!(risk_level_from_score(0.0), "safe");
        assert_eq!(risk_level_from_score(0.1), "safe");
        assert_eq!(risk_level_from_score(0.2), "low");
        assert_eq!(risk_level_from_score(0.39), "low");
        assert_eq!(risk_level_from_score(0.4), "medium");
        assert_eq!(risk_level_from_score(0.59), "medium");
        assert_eq!(risk_level_from_score(0.6), "high");
        assert_eq!(risk_level_from_score(0.79), "high");
        assert_eq!(risk_level_from_score(0.8), "critical");
        assert_eq!(risk_level_from_score(1.0), "critical");
    }

    // --- Database integration tests ---

    #[test]
    fn test_phishing_reports_table_exists() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='phishing_reports'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "phishing_reports table should exist after migration");
    }

    #[test]
    fn test_insert_and_query_report() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);

        let msg = make_message(
            &account.id,
            "Test Phishing",
            "Some body text",
            "sender@example.com",
            "Sender",
        );
        let msg_id = InsertMessage::insert(&conn, &msg).unwrap();

        let signals = vec![PhishingSignal {
            signal_type: "urgency_language".to_string(),
            description: "Found urgency".to_string(),
            weight: 0.15,
        }];
        let signals_json = serde_json::to_string(&signals).unwrap();

        conn.execute(
            "INSERT INTO phishing_reports (message_id, account_id, risk_level, risk_score, signals)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![msg_id, account.id, "low", 0.15, signals_json],
        )
        .unwrap();

        let (level, score): (String, f64) = conn
            .query_row(
                "SELECT risk_level, risk_score FROM phishing_reports WHERE message_id = ?1",
                rusqlite::params![msg_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();

        assert_eq!(level, "low");
        assert!((score - 0.15).abs() < 0.001);
    }

    #[test]
    fn test_phishing_reports_filtered_by_risk_level() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);

        // Insert reports with different risk levels
        for (level, score) in &[("safe", 0.0), ("low", 0.3), ("high", 0.7), ("high", 0.75)] {
            conn.execute(
                "INSERT INTO phishing_reports (message_id, account_id, risk_level, risk_score, signals)
                 VALUES (?1, ?2, ?3, ?4, '[]')",
                rusqlite::params![format!("msg-{}", score), account.id, level, score],
            )
            .unwrap();
        }

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM phishing_reports WHERE risk_level = 'high'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn test_combined_phishing_signals_high_risk() {
        // A sophisticated phishing email with multiple signals
        let signals = detect_signals(
            "URGENT: Your PayPal Account Has Been Suspended",
            "Dear Customer, your PayPal account has been suspended. \
             Please verify your account immediately by providing your password \
             and credit card information. Act now or your account will be closed.",
            r#"<a href="https://evil-paypal-login.com/steal">https://www.paypal.com/verify</a>"#,
            "security@paypa1-support.xyz",
            "PayPal Security",
        );

        let risk_score = calculate_risk_score(&signals);
        let risk_level = risk_level_from_score(risk_score);

        // Should be high or critical risk
        assert!(
            risk_level == "high" || risk_level == "critical",
            "Combined phishing signals should produce high/critical risk, got: {} (score: {:.2})",
            risk_level,
            risk_score
        );

        // Should have multiple signal types
        let signal_types: Vec<&str> = signals.iter().map(|s| s.signal_type.as_str()).collect();
        assert!(signal_types.contains(&"urgency_language"), "Missing urgency signal");
        assert!(signal_types.contains(&"credential_request"), "Missing credential signal");
        assert!(signal_types.contains(&"suspicious_greeting"), "Missing greeting signal");
        assert!(signal_types.contains(&"suspicious_links"), "Missing links signal");
    }

    #[test]
    fn test_extract_email_domain() {
        assert_eq!(extract_email_domain("user@example.com"), Some("example.com".to_string()));
        assert_eq!(extract_email_domain("user@sub.domain.com"), Some("sub.domain.com".to_string()));
        assert_eq!(extract_email_domain("nodomain"), None);
    }

    #[test]
    fn test_extract_url_domain() {
        assert_eq!(extract_url_domain("https://www.example.com/path"), Some("www.example.com".to_string()));
        assert_eq!(extract_url_domain("http://evil.com:8080/phish"), Some("evil.com".to_string()));
        assert_eq!(extract_url_domain("www.test.com"), Some("www.test.com".to_string()));
    }
}

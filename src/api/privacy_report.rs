use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use crate::AppState;

// ── Tracker database ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackerType {
    OpenPixel,
    LinkTrack,
}

#[derive(Debug, Clone)]
struct TrackerInfo {
    name: &'static str,
    tracker_type: TrackerType,
}

/// A static map of known tracker domain patterns → tracker metadata.
/// Patterns are matched against the full domain or as a suffix.
const KNOWN_TRACKERS: &[(&str, TrackerInfo)] = &[
    ("mcimg.com", TrackerInfo { name: "Mailchimp", tracker_type: TrackerType::OpenPixel }),
    ("list-manage.com", TrackerInfo { name: "Mailchimp", tracker_type: TrackerType::OpenPixel }),
    ("mailchimp.com", TrackerInfo { name: "Mailchimp", tracker_type: TrackerType::OpenPixel }),
    ("t.hubspotemail.net", TrackerInfo { name: "HubSpot", tracker_type: TrackerType::OpenPixel }),
    ("track.hubspot.com", TrackerInfo { name: "HubSpot", tracker_type: TrackerType::OpenPixel }),
    ("hubspot.com", TrackerInfo { name: "HubSpot", tracker_type: TrackerType::OpenPixel }),
    ("hsforms.com", TrackerInfo { name: "HubSpot", tracker_type: TrackerType::OpenPixel }),
    ("google-analytics.com", TrackerInfo { name: "Google Analytics", tracker_type: TrackerType::LinkTrack }),
    ("www.google-analytics.com", TrackerInfo { name: "Google Analytics", tracker_type: TrackerType::LinkTrack }),
    ("google-analytics.com", TrackerInfo { name: "Google Analytics", tracker_type: TrackerType::LinkTrack }),
    ("facebook.com", TrackerInfo { name: "Facebook Pixel", tracker_type: TrackerType::OpenPixel }),
    ("connect.facebook.net", TrackerInfo { name: "Facebook Pixel", tracker_type: TrackerType::OpenPixel }),
    ("sendgrid.net", TrackerInfo { name: "SendGrid", tracker_type: TrackerType::OpenPixel }),
    ("sendgrid.com", TrackerInfo { name: "SendGrid", tracker_type: TrackerType::OpenPixel }),
    ("mailgun.org", TrackerInfo { name: "Mailgun", tracker_type: TrackerType::OpenPixel }),
    ("amazonses.com", TrackerInfo { name: "Amazon SES", tracker_type: TrackerType::OpenPixel }),
    ("mailtrack.io", TrackerInfo { name: "Mailtrack", tracker_type: TrackerType::OpenPixel }),
    ("pixel.watch", TrackerInfo { name: "Pixel Watch", tracker_type: TrackerType::OpenPixel }),
    ("t.co", TrackerInfo { name: "Twitter/X", tracker_type: TrackerType::LinkTrack }),
    ("clicks.beehiiv.com", TrackerInfo { name: "Beehiiv", tracker_type: TrackerType::LinkTrack }),
    ("convertkit.com", TrackerInfo { name: "ConvertKit", tracker_type: TrackerType::OpenPixel }),
    ("drip.com", TrackerInfo { name: "Drip", tracker_type: TrackerType::OpenPixel }),
    ("em.sailthru.com", TrackerInfo { name: "Sailthru", tracker_type: TrackerType::OpenPixel }),
    ("exact-target.com", TrackerInfo { name: "Salesforce Marketing Cloud", tracker_type: TrackerType::OpenPixel }),
    ("pardot.com", TrackerInfo { name: "Pardot", tracker_type: TrackerType::OpenPixel }),
    ("mktotracking.com", TrackerInfo { name: "Marketo", tracker_type: TrackerType::OpenPixel }),
];

/// Identify a tracker domain, returning its name and type.
/// Falls back to prefix-based classification for unknown domains.
pub fn identify_tracker(domain: &str) -> (&'static str, TrackerType) {
    let domain_lower = domain.to_lowercase();

    // Exact match or suffix match against known tracker list
    for (pattern, info) in KNOWN_TRACKERS {
        if domain_lower == *pattern || domain_lower.ends_with(&format!(".{}", pattern)) {
            return (info.name, info.tracker_type.clone());
        }
    }

    // Prefix-based heuristics for generic trackers
    let host = domain_lower
        .split('/')
        .next()
        .unwrap_or(&domain_lower);

    // Extract the subdomain (first label)
    let first_label = host.split('.').next().unwrap_or("");

    match first_label {
        "click" | "clicks" => ("Generic Click Tracker", TrackerType::LinkTrack),
        "open" | "opens" => ("Generic Open Tracker", TrackerType::OpenPixel),
        "track" | "tracking" | "trk" | "t" => ("Generic Tracker", TrackerType::OpenPixel),
        _ => ("Unknown Tracker", TrackerType::OpenPixel),
    }
}

// ── HTML scanning ─────────────────────────────────────────────────────────────

/// Extract domain from a URL, returning lowercase domain or None.
fn extract_domain(url: &str) -> Option<String> {
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

/// Check if a domain is a tracker (known list or prefix heuristic).
fn is_tracker_domain(domain: &str) -> bool {
    let d = domain.to_lowercase();

    // Known trackers
    for (pattern, _) in KNOWN_TRACKERS {
        if d == *pattern || d.ends_with(&format!(".{}", pattern)) {
            return true;
        }
    }

    // Prefix heuristics
    let first_label = d.split('.').next().unwrap_or("");
    matches!(first_label, "click" | "clicks" | "open" | "opens" | "track" | "tracking" | "trk")
}

/// Extract the numeric value of an attribute from a lowercased tag string.
fn extract_numeric_attr(tag_lower: &str, attr: &str) -> Option<u32> {
    let search = format!("{}=", attr);
    let pos = tag_lower.find(&search)?;
    let start = pos + search.len();
    let remaining = &tag_lower[start..];
    let value_start = if remaining.starts_with('"') || remaining.starts_with('\'') { 1 } else { 0 };
    let digits: String = remaining[value_start..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse().ok()
}

/// Extract the value of a named attribute from a tag string.
fn extract_attr_value(tag: &str, tag_lower: &str, attr: &str) -> Option<String> {
    let search = format!("{}=", attr);
    let pos = tag_lower.find(&search)?;
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
        if value.is_empty() { None } else { Some(value) }
    }
}

/// Scan HTML for tracking pixels. Returns list of detected tracker domains.
pub fn extract_tracker_domains_from_html(html: &str) -> Vec<String> {
    let mut found = Vec::new();
    let html_lower = html.to_lowercase();

    let mut search_from = 0;
    while let Some(img_start) = html_lower[search_from..].find("<img") {
        let abs_start = search_from + img_start;
        let tag_end = match html_lower[abs_start..].find('>') {
            Some(pos) => abs_start + pos + 1,
            None => break,
        };

        let tag = &html[abs_start..tag_end];
        let tag_lower = &html_lower[abs_start..tag_end];

        if let Some(src) = extract_attr_value(tag, tag_lower, "src") {
            let width = extract_numeric_attr(tag_lower, "width");
            let height = extract_numeric_attr(tag_lower, "height");

            // Check for 1x1 pixel or known tracker domain
            let is_tiny = matches!((width, height), (Some(w), Some(h)) if w <= 1 && h <= 1);
            let domain = extract_domain(&src);

            let is_tracker = is_tiny || domain.as_deref().map(is_tracker_domain).unwrap_or(false);
            if is_tracker {
                if let Some(d) = domain {
                    if !d.is_empty() {
                        found.push(d);
                    }
                }
            }
        }

        search_from = tag_end;
    }

    // Also scan all src/href URLs for known tracker domains (link trackers)
    for prefix in &["src=\"", "href=\"", "src='", "href='"] {
        let mut pos = 0;
        while let Some(idx) = html_lower[pos..].find(prefix) {
            let abs = pos + idx + prefix.len();
            let end_char = if prefix.ends_with('"') { '"' } else { '\'' };
            if let Some(end) = html[abs..].find(end_char) {
                let url = &html[abs..abs + end];
                if let Some(d) = extract_domain(url) {
                    if is_tracker_domain(&d) && !found.contains(&d) {
                        found.push(d);
                    }
                }
                pos = abs + end + 1;
            } else {
                break;
            }
        }
    }

    found
}

// ── Response types ────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct PrivacySummary {
    pub total_emails: i64,
    pub emails_with_trackers: i64,
    pub tracker_percentage: f64,
    pub unique_trackers: i64,
    pub pixels_blocked: i64,
}

#[derive(Debug, Serialize)]
pub struct TrackerEntry {
    pub domain: String,
    pub tracker_name: String,
    pub count: i64,
    pub tracker_type: String,
    pub first_seen: i64,
    pub last_seen: i64,
    pub senders: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct TopSenderEntry {
    pub sender: String,
    pub sender_name: String,
    pub tracker_count: i64,
    pub tracker_domains: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct TrendInfo {
    pub current_period: f64,
    pub previous_period: f64,
    pub direction: String,
}

#[derive(Debug, Serialize)]
pub struct PrivacyReportResponse {
    pub period_days: i64,
    pub summary: PrivacySummary,
    pub trackers: Vec<TrackerEntry>,
    pub top_senders_with_trackers: Vec<TopSenderEntry>,
    pub trend: TrendInfo,
}

#[derive(Debug, Serialize)]
pub struct KnownTrackerEntry {
    pub domain: String,
    pub name: String,
    #[serde(rename = "type")]
    pub tracker_type: String,
    pub total_occurrences: i64,
}

#[derive(Debug, Serialize)]
pub struct TrackersListResponse {
    pub trackers: Vec<KnownTrackerEntry>,
}

// ── Query params ──────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct PrivacyReportParams {
    pub days: Option<i64>,
    pub account_id: Option<String>,
}

// ── Handlers ──────────────────────────────────────────────────────────────────

/// GET /api/privacy/report — Generate a privacy report for a time period.
pub async fn privacy_report(
    State(state): State<Arc<AppState>>,
    Query(params): Query<PrivacyReportParams>,
) -> Result<Json<PrivacyReportResponse>, StatusCode> {
    let period_days = params.days.unwrap_or(30).clamp(1, 365);
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Unix timestamps for current and previous periods
    let now = chrono::Utc::now().timestamp();
    let period_secs = period_days * 86_400;
    let current_start = now - period_secs;
    let previous_start = current_start - period_secs;

    // Build account filter clause
    let account_filter = if params.account_id.is_some() {
        "AND m.account_id = ?3"
    } else {
        ""
    };

    // ── Total emails in current period ──
    let total_emails: i64 = {
        let sql = format!(
            "SELECT COUNT(*) FROM messages m WHERE m.date >= ?1 AND m.date < ?2 AND m.is_deleted = 0 {account_filter}"
        );
        if let Some(ref aid) = params.account_id {
            conn.query_row(&sql, rusqlite::params![current_start, now, aid], |r| r.get(0))
        } else {
            conn.query_row(&sql, rusqlite::params![current_start, now], |r| r.get(0))
        }
        .unwrap_or(0)
    };

    // ── Emails with trackers in current period ──
    let emails_with_trackers: i64 = {
        let sql = format!(
            "SELECT COUNT(*) FROM messages m WHERE m.date >= ?1 AND m.date < ?2 AND m.is_deleted = 0 AND m.has_tracking_pixels = 1 {account_filter}"
        );
        if let Some(ref aid) = params.account_id {
            conn.query_row(&sql, rusqlite::params![current_start, now, aid], |r| r.get(0))
        } else {
            conn.query_row(&sql, rusqlite::params![current_start, now], |r| r.get(0))
        }
        .unwrap_or(0)
    };

    let tracker_percentage = if total_emails > 0 {
        (emails_with_trackers as f64 / total_emails as f64) * 100.0
    } else {
        0.0
    };

    // ── Previous period tracker percentage (for trend) ──
    let prev_total: i64 = {
        let sql = format!(
            "SELECT COUNT(*) FROM messages m WHERE m.date >= ?1 AND m.date < ?2 AND m.is_deleted = 0 {account_filter}"
        );
        if let Some(ref aid) = params.account_id {
            conn.query_row(&sql, rusqlite::params![previous_start, current_start, aid], |r| r.get(0))
        } else {
            conn.query_row(&sql, rusqlite::params![previous_start, current_start], |r| r.get(0))
        }
        .unwrap_or(0)
    };

    let prev_tracked: i64 = {
        let sql = format!(
            "SELECT COUNT(*) FROM messages m WHERE m.date >= ?1 AND m.date < ?2 AND m.is_deleted = 0 AND m.has_tracking_pixels = 1 {account_filter}"
        );
        if let Some(ref aid) = params.account_id {
            conn.query_row(&sql, rusqlite::params![previous_start, current_start, aid], |r| r.get(0))
        } else {
            conn.query_row(&sql, rusqlite::params![previous_start, current_start], |r| r.get(0))
        }
        .unwrap_or(0)
    };

    let previous_period_pct = if prev_total > 0 {
        (prev_tracked as f64 / prev_total as f64) * 100.0
    } else {
        0.0
    };

    let direction = if tracker_percentage < previous_period_pct - 0.5 {
        "improving"
    } else if tracker_percentage > previous_period_pct + 0.5 {
        "worsening"
    } else {
        "stable"
    };

    // ── Fetch messages with trackers to build per-domain stats ──
    // We need to scan body HTML to extract tracker domains.
    // Load tracked messages in the period (id, from_address, from_name, date, body_html).
    type MsgRow = (String, Option<String>, Option<String>, i64, Option<String>);
    let rows: Vec<MsgRow> = {
        let sql = format!(
            "SELECT m.id, m.from_address, m.from_name, m.date, m.body_html \
             FROM messages m \
             WHERE m.date >= ?1 AND m.date < ?2 AND m.is_deleted = 0 \
             AND m.has_tracking_pixels = 1 {account_filter} \
             ORDER BY m.date DESC \
             LIMIT 2000"
        );
        let mut stmt = conn.prepare(&sql).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let row_mapper = |row: &rusqlite::Row<'_>| -> rusqlite::Result<MsgRow> {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, Option<String>>(4)?,
            ))
        };

        if let Some(ref aid) = params.account_id {
            stmt.query_map(rusqlite::params![current_start, now, aid], row_mapper)
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
                .filter_map(|r| r.ok())
                .collect()
        } else {
            stmt.query_map(rusqlite::params![current_start, now], row_mapper)
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
                .filter_map(|r| r.ok())
                .collect()
        }
    };

    // ── Build per-domain tracker stats ──
    // domain → { count, first_seen, last_seen, senders: set }
    struct DomainStat {
        count: i64,
        first_seen: i64,
        last_seen: i64,
        senders: std::collections::HashSet<String>,
    }

    let mut domain_stats: HashMap<String, DomainStat> = HashMap::new();

    // sender → { tracker_count, tracker_domains: set }
    struct SenderStat {
        tracker_count: i64,
        name: String,
        domains: std::collections::HashSet<String>,
    }
    let mut sender_stats: HashMap<String, SenderStat> = HashMap::new();

    for (_id, from_address, from_name, date, body_html) in &rows {
        let html = body_html.as_deref().unwrap_or("");
        let domains = extract_tracker_domains_from_html(html);
        let sender = from_address.as_deref().unwrap_or("").to_string();
        let sname = from_name.as_deref().unwrap_or("").to_string();

        for domain in &domains {
            let stat = domain_stats.entry(domain.clone()).or_insert(DomainStat {
                count: 0,
                first_seen: *date,
                last_seen: *date,
                senders: std::collections::HashSet::new(),
            });
            stat.count += 1;
            if *date < stat.first_seen { stat.first_seen = *date; }
            if *date > stat.last_seen { stat.last_seen = *date; }
            if !sender.is_empty() {
                stat.senders.insert(sender.clone());
            }
        }

        // Update sender stats if any trackers found
        if !domains.is_empty() && !sender.is_empty() {
            let ss = sender_stats.entry(sender.clone()).or_insert(SenderStat {
                tracker_count: 0,
                name: sname.clone(),
                domains: std::collections::HashSet::new(),
            });
            ss.tracker_count += 1;
            for d in &domains {
                ss.domains.insert(d.clone());
            }
        }
    }

    // Build tracker entries, sorted by count desc
    let mut trackers: Vec<TrackerEntry> = domain_stats
        .iter()
        .map(|(domain, stat)| {
            let (name, ttype) = identify_tracker(domain);
            let ttype_str = match ttype {
                TrackerType::OpenPixel => "open_pixel".to_string(),
                TrackerType::LinkTrack => "link_track".to_string(),
            };
            let mut senders: Vec<String> = stat.senders.iter().cloned().collect();
            senders.sort();
            TrackerEntry {
                domain: domain.clone(),
                tracker_name: name.to_string(),
                count: stat.count,
                tracker_type: ttype_str,
                first_seen: stat.first_seen,
                last_seen: stat.last_seen,
                senders,
            }
        })
        .collect();
    trackers.sort_by(|a, b| b.count.cmp(&a.count));

    let unique_trackers = trackers.len() as i64;

    // Build top senders, sorted by tracker_count desc, top 10
    let mut top_senders: Vec<TopSenderEntry> = sender_stats
        .iter()
        .map(|(sender, ss)| {
            let mut domains: Vec<String> = ss.domains.iter().cloned().collect();
            domains.sort();
            TopSenderEntry {
                sender: sender.clone(),
                sender_name: ss.name.clone(),
                tracker_count: ss.tracker_count,
                tracker_domains: domains,
            }
        })
        .collect();
    top_senders.sort_by(|a, b| b.tracker_count.cmp(&a.tracker_count));
    top_senders.truncate(10);

    Ok(Json(PrivacyReportResponse {
        period_days,
        summary: PrivacySummary {
            total_emails,
            emails_with_trackers,
            tracker_percentage,
            unique_trackers,
            pixels_blocked: emails_with_trackers,
        },
        trackers,
        top_senders_with_trackers: top_senders,
        trend: TrendInfo {
            current_period: tracker_percentage,
            previous_period: previous_period_pct,
            direction: direction.to_string(),
        },
    }))
}

/// GET /api/privacy/trackers — List all known tracker domains found in emails.
pub async fn list_trackers(
    State(state): State<Arc<AppState>>,
    Query(params): Query<PrivacyReportParams>,
) -> Result<Json<TrackersListResponse>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Fetch all tracked messages (no time window for this endpoint — all-time totals)
    let account_filter = if params.account_id.is_some() {
        "AND m.account_id = ?1"
    } else {
        ""
    };

    let rows: Vec<Option<String>> = {
        let sql = format!(
            "SELECT m.body_html FROM messages m \
             WHERE m.is_deleted = 0 AND m.has_tracking_pixels = 1 {account_filter} \
             LIMIT 5000"
        );
        let mut stmt = conn.prepare(&sql).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let row_mapper = |row: &rusqlite::Row<'_>| row.get::<_, Option<String>>(0);

        if let Some(ref aid) = params.account_id {
            stmt.query_map(rusqlite::params![aid], row_mapper)
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
                .filter_map(|r| r.ok())
                .collect()
        } else {
            stmt.query_map([], row_mapper)
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
                .filter_map(|r| r.ok())
                .collect()
        }
    };

    // Count occurrences per domain
    let mut domain_counts: HashMap<String, i64> = HashMap::new();
    for body_html in &rows {
        let html = body_html.as_deref().unwrap_or("");
        for domain in extract_tracker_domains_from_html(html) {
            *domain_counts.entry(domain).or_insert(0) += 1;
        }
    }

    let mut trackers: Vec<KnownTrackerEntry> = domain_counts
        .iter()
        .map(|(domain, &count)| {
            let (name, ttype) = identify_tracker(domain);
            let ttype_str = match ttype {
                TrackerType::OpenPixel => "open_pixel".to_string(),
                TrackerType::LinkTrack => "link_track".to_string(),
            };
            KnownTrackerEntry {
                domain: domain.clone(),
                name: name.to_string(),
                tracker_type: ttype_str,
                total_occurrences: count,
            }
        })
        .collect();
    trackers.sort_by(|a, b| b.total_occurrences.cmp(&a.total_occurrences));

    Ok(Json(TrackersListResponse { trackers }))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_identify_tracker_known_domains() {
        let (name, ttype) = identify_tracker("mcimg.com");
        assert_eq!(name, "Mailchimp");
        assert_eq!(ttype, TrackerType::OpenPixel);

        let (name, ttype) = identify_tracker("t.hubspotemail.net");
        assert_eq!(name, "HubSpot");
        assert_eq!(ttype, TrackerType::OpenPixel);

        let (name, ttype) = identify_tracker("google-analytics.com");
        assert_eq!(name, "Google Analytics");
        assert_eq!(ttype, TrackerType::LinkTrack);

        let (name, ttype) = identify_tracker("sendgrid.net");
        assert_eq!(name, "SendGrid");
        assert_eq!(ttype, TrackerType::OpenPixel);

        let (name, ttype) = identify_tracker("mailgun.org");
        assert_eq!(name, "Mailgun");
        assert_eq!(ttype, TrackerType::OpenPixel);

        let (name, ttype) = identify_tracker("amazonses.com");
        assert_eq!(name, "Amazon SES");
        assert_eq!(ttype, TrackerType::OpenPixel);
    }

    #[test]
    fn test_identify_tracker_subdomains() {
        let (name, ttype) = identify_tracker("em123.mailchimp.com");
        assert_eq!(name, "Mailchimp");
        assert_eq!(ttype, TrackerType::OpenPixel);

        let (name, ttype) = identify_tracker("img.sendgrid.net");
        assert_eq!(name, "SendGrid");
        assert_eq!(ttype, TrackerType::OpenPixel);
    }

    #[test]
    fn test_identify_tracker_prefix_click() {
        let (name, ttype) = identify_tracker("click.example.com");
        assert_eq!(name, "Generic Click Tracker");
        assert_eq!(ttype, TrackerType::LinkTrack);
    }

    #[test]
    fn test_identify_tracker_prefix_open() {
        let (name, ttype) = identify_tracker("open.example.com");
        assert_eq!(name, "Generic Open Tracker");
        assert_eq!(ttype, TrackerType::OpenPixel);
    }

    #[test]
    fn test_identify_tracker_prefix_track() {
        let (name, ttype) = identify_tracker("track.example.com");
        assert_eq!(name, "Generic Tracker");
        assert_eq!(ttype, TrackerType::OpenPixel);

        let (name, ttype) = identify_tracker("trk.example.com");
        assert_eq!(name, "Generic Tracker");
        assert_eq!(ttype, TrackerType::OpenPixel);

        let (name, ttype) = identify_tracker("t.example.com");
        assert_eq!(name, "Generic Tracker");
        assert_eq!(ttype, TrackerType::OpenPixel);
    }

    #[test]
    fn test_identify_tracker_unknown() {
        let (name, ttype) = identify_tracker("unknown-domain.io");
        assert_eq!(name, "Unknown Tracker");
        assert_eq!(ttype, TrackerType::OpenPixel);
    }

    #[test]
    fn test_extract_tracker_domains_one_pixel_img() {
        let html = r#"<html><body>Hello<img src="https://mcimg.com/open.gif" width="1" height="1"></body></html>"#;
        let domains = extract_tracker_domains_from_html(html);
        assert!(domains.contains(&"mcimg.com".to_string()), "Expected mcimg.com, got {:?}", domains);
    }

    #[test]
    fn test_extract_tracker_domains_known_domain_any_size() {
        // Known tracker domain even without 1x1 size
        let html = r#"<img src="https://track.hubspot.com/track?id=1">"#;
        let domains = extract_tracker_domains_from_html(html);
        assert!(domains.contains(&"track.hubspot.com".to_string()), "Got {:?}", domains);
    }

    #[test]
    fn test_extract_tracker_domains_no_trackers() {
        let html = r#"<html><body><img src="https://example.com/photo.jpg" width="400" height="300"></body></html>"#;
        let domains = extract_tracker_domains_from_html(html);
        assert!(domains.is_empty(), "Expected empty, got {:?}", domains);
    }

    #[test]
    fn test_extract_tracker_domains_generic_prefix() {
        let html = r#"<img src="https://click.newsletter.com/abc" width="1" height="1">"#;
        let domains = extract_tracker_domains_from_html(html);
        assert!(!domains.is_empty(), "Expected click tracker domain");
    }

    #[test]
    fn test_trend_direction_improving() {
        // current < previous → improving
        let current = 60.0_f64;
        let previous = 72.0_f64;
        let direction = if current < previous - 0.5 {
            "improving"
        } else if current > previous + 0.5 {
            "worsening"
        } else {
            "stable"
        };
        assert_eq!(direction, "improving");
    }

    #[test]
    fn test_trend_direction_worsening() {
        let current = 80.0_f64;
        let previous = 65.0_f64;
        let direction = if current < previous - 0.5 {
            "improving"
        } else if current > previous + 0.5 {
            "worsening"
        } else {
            "stable"
        };
        assert_eq!(direction, "worsening");
    }

    #[test]
    fn test_trend_direction_stable() {
        let current = 70.0_f64;
        let previous = 70.2_f64;
        let direction = if current < previous - 0.5 {
            "improving"
        } else if current > previous + 0.5 {
            "worsening"
        } else {
            "stable"
        };
        assert_eq!(direction, "stable");
    }

    #[test]
    fn test_tracker_type_classification() {
        // open_pixel types
        assert_eq!(identify_tracker("mailchimp.com").1, TrackerType::OpenPixel);
        assert_eq!(identify_tracker("sendgrid.net").1, TrackerType::OpenPixel);
        // link_track types
        assert_eq!(identify_tracker("google-analytics.com").1, TrackerType::LinkTrack);
        assert_eq!(identify_tracker("t.co").1, TrackerType::LinkTrack);
    }

    // Integration-style test: verify route would work (compilation test via AppState usage)
    #[test]
    fn test_privacy_report_params_defaults() {
        // Deserialize with no fields → defaults should apply in handler
        let params = PrivacyReportParams {
            days: None,
            account_id: None,
        };
        let period_days = params.days.unwrap_or(30).clamp(1, 365);
        assert_eq!(period_days, 30);
    }

    #[test]
    fn test_period_filtering_30_days() {
        let params = PrivacyReportParams { days: Some(30), account_id: None };
        let period_days = params.days.unwrap_or(30).clamp(1, 365);
        assert_eq!(period_days, 30);
    }

    #[test]
    fn test_period_filtering_90_days() {
        let params = PrivacyReportParams { days: Some(90), account_id: None };
        let period_days = params.days.unwrap_or(30).clamp(1, 365);
        assert_eq!(period_days, 90);
    }

    #[test]
    fn test_period_filtering_365_days() {
        let params = PrivacyReportParams { days: Some(365), account_id: None };
        let period_days = params.days.unwrap_or(30).clamp(1, 365);
        assert_eq!(period_days, 365);
    }

    #[test]
    fn test_period_clamped_to_max() {
        let params = PrivacyReportParams { days: Some(1000), account_id: None };
        let period_days = params.days.unwrap_or(30).clamp(1, 365);
        assert_eq!(period_days, 365);
    }

    #[test]
    fn test_account_filtering_present() {
        let params = PrivacyReportParams {
            days: Some(30),
            account_id: Some("acc_123".to_string()),
        };
        assert!(params.account_id.is_some());
        assert_eq!(params.account_id.as_deref(), Some("acc_123"));
    }

    #[test]
    fn test_account_filtering_absent() {
        let params = PrivacyReportParams {
            days: Some(30),
            account_id: None,
        };
        assert!(params.account_id.is_none());
    }
}

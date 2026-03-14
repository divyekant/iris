use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Serialize;
use std::sync::Arc;

use crate::utils::strip_html_tags;
use crate::AppState;

// ---------------------------------------------------------------------------
// Known domain lists
// ---------------------------------------------------------------------------

/// Known trusted domains. Subdomains of these are also considered trusted.
const KNOWN_TRUSTED_DOMAINS: &[&str] = &[
    // Google
    "google.com",
    "gmail.com",
    "googlemail.com",
    "googleapis.com",
    "docs.google.com",
    "drive.google.com",
    "mail.google.com",
    "accounts.google.com",
    "youtube.com",
    "google.co.uk",
    "google.ca",
    "google.com.au",
    // Microsoft
    "microsoft.com",
    "outlook.com",
    "hotmail.com",
    "live.com",
    "office.com",
    "office365.com",
    "sharepoint.com",
    "onedrive.com",
    "microsoftonline.com",
    "azure.com",
    "teams.microsoft.com",
    // Apple
    "apple.com",
    "icloud.com",
    // Amazon
    "amazon.com",
    "amazonaws.com",
    "aws.amazon.com",
    // Social
    "linkedin.com",
    "twitter.com",
    "facebook.com",
    "instagram.com",
    "github.com",
    "gitlab.com",
    // Communication
    "slack.com",
    "zoom.us",
    "dropbox.com",
    "notion.so",
    "atlassian.com",
    "jira.atlassian.com",
    "confluence.atlassian.com",
    // Finance
    "paypal.com",
    "stripe.com",
    "chase.com",
    "bankofamerica.com",
    "wellsfargo.com",
    // Other tech
    "cloudflare.com",
    "mozilla.org",
    "wikipedia.org",
    "stackoverflow.com",
];

/// Known URL shortener domains.
const KNOWN_SHORTENERS: &[&str] = &[
    "bit.ly",
    "t.co",
    "goo.gl",
    "tinyurl.com",
    "ow.ly",
    "is.gd",
    "buff.ly",
    "rb.gy",
    "short.link",
    "tiny.cc",
    "lnkd.in",
    "ift.tt",
    "dlvr.it",
    "soo.gd",
    "clck.ru",
    "cutt.ly",
    "snip.ly",
    "yourls.org",
];

/// Suspicious URL paths that are common in phishing.
const SUSPICIOUS_PATHS: &[&str] = &[
    "/verify",
    "/login",
    "/secure",
    "/account",
    "/password",
    "/signin",
    "/update",
    "/confirm",
    "/validate",
    "/reset",
    "/auth",
    "/authenticate",
];

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum SafetyLevel {
    Safe,
    Caution,
    Danger,
}

#[derive(Debug, Clone, Serialize)]
pub struct LinkAnalysis {
    pub url: String,
    pub display_text: String,
    pub safety_level: SafetyLevel,
    pub reasons: Vec<String>,
    pub domain: String,
    pub is_shortened: bool,
    pub redirect_target: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ScanSummary {
    pub total: usize,
    pub safe: usize,
    pub caution: usize,
    pub danger: usize,
}

#[derive(Debug, Serialize)]
pub struct ScanLinksResponse {
    pub links: Vec<LinkAnalysis>,
    pub summary: ScanSummary,
}

// ---------------------------------------------------------------------------
// Link extraction
// ---------------------------------------------------------------------------

/// Represents a raw extracted link before analysis.
#[derive(Debug, Clone)]
pub struct ExtractedLink {
    pub url: String,
    pub display_text: String,
}

/// Extract links from HTML. Finds:
/// - `<a href="...">display text</a>` anchor tags
/// - Plain-text URLs (http:// or https://) not already captured in an anchor
pub fn extract_links(html: &str) -> Vec<ExtractedLink> {
    let mut links: Vec<ExtractedLink> = Vec::new();
    let mut seen_urls: std::collections::HashSet<String> = std::collections::HashSet::new();
    let html_lower = html.to_lowercase();

    // --- Pass 1: parse <a href="...">...</a> anchors ---
    let mut search_from = 0;
    while let Some(a_start) = html_lower[search_from..].find("<a ") {
        let abs_start = search_from + a_start;

        // Find the end of the opening tag
        let tag_end = match html_lower[abs_start..].find('>') {
            Some(pos) => abs_start + pos + 1,
            None => break,
        };

        let tag = &html[abs_start..tag_end];
        let tag_lower = &html_lower[abs_start..tag_end];

        // Extract href attribute
        if let Some(href) = extract_attr_value(tag, tag_lower, "href") {
            // Skip mailto:, javascript:, #anchor, etc.
            if href.starts_with("http://") || href.starts_with("https://") {
                // Find closing </a>
                let display_text = match html_lower[tag_end..].find("</a>") {
                    Some(close_pos) => {
                        let raw = &html[tag_end..tag_end + close_pos];
                        strip_html_tags(raw).trim().to_string()
                    }
                    None => String::new(),
                };

                seen_urls.insert(href.clone());
                links.push(ExtractedLink {
                    url: href,
                    display_text,
                });
            }
        }

        search_from = tag_end;
    }

    // --- Pass 2: plain-text URLs not already in anchors ---
    let plain_url_re = find_plain_urls(html);
    for url in plain_url_re {
        if !seen_urls.contains(&url) {
            seen_urls.insert(url.clone());
            links.push(ExtractedLink {
                url,
                display_text: String::new(),
            });
        }
    }

    links
}

/// Find plain-text http/https URLs in text (not inside HTML tags).
fn find_plain_urls(text: &str) -> Vec<String> {
    let mut urls = Vec::new();
    let mut pos = 0;

    while pos < text.len() {
        // Look for http:// or https://
        let lower = text[pos..].to_lowercase();
        let found = lower.find("http://").or_else(|| lower.find("https://"));

        let rel_start = match found {
            Some(p) => p,
            None => break,
        };
        let abs_start = pos + rel_start;

        // Check that we're not inside an HTML tag or attribute by looking backward
        // for an unclosed '<' (simple heuristic)
        if is_inside_tag(text, abs_start) {
            pos = abs_start + 4; // skip past "http"
            continue;
        }

        // Extract the URL — ends at whitespace, <, >, ", ', or end of string
        let url: String = text[abs_start..]
            .chars()
            .take_while(|c| !c.is_whitespace() && *c != '<' && *c != '>' && *c != '"' && *c != '\'')
            .collect();

        if url.len() > 7 {
            // longer than "http://"
            // Strip trailing punctuation (., ,, ), ], etc.)
            let url = url.trim_end_matches(|c: char| matches!(c, '.' | ',' | ')' | ']' | ';'));
            urls.push(url.to_string());
        }

        pos = abs_start + 7;
    }

    urls
}

/// Rough check: is the character at `pos` inside an HTML tag?
fn is_inside_tag(text: &str, pos: usize) -> bool {
    // Walk backward — if we find '<' before '>', we're inside a tag
    let before = &text[..pos];
    let last_lt = before.rfind('<');
    let last_gt = before.rfind('>');
    match (last_lt, last_gt) {
        (Some(lt), Some(gt)) => lt > gt,
        (Some(_), None) => true,
        _ => false,
    }
}

// strip_html_tags is imported from crate::utils

/// Extract the value of an HTML attribute (handles double, single, unquoted).
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
        // Unquoted: ends at whitespace or >
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

// ---------------------------------------------------------------------------
// Domain extraction and matching helpers
// ---------------------------------------------------------------------------

/// Extract the domain (lowercased, no port) from a URL.
pub fn extract_domain(url: &str) -> Option<String> {
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

/// Extract the path component from a URL.
fn extract_path(url: &str) -> String {
    let without_protocol = if let Some(pos) = url.find("://") {
        &url[pos + 3..]
    } else {
        url
    };
    // Skip the host part
    match without_protocol.find('/') {
        Some(slash) => without_protocol[slash..].to_lowercase(),
        None => String::new(),
    }
}

/// Check if a domain is known-trusted (exact match or subdomain of a trusted domain).
pub fn is_trusted_domain(domain: &str) -> bool {
    KNOWN_TRUSTED_DOMAINS.iter().any(|trusted| {
        domain == *trusted || domain.ends_with(&format!(".{}", trusted))
    })
}

/// Check if a domain is a known URL shortener.
pub fn is_shortener_domain(domain: &str) -> bool {
    KNOWN_SHORTENERS.iter().any(|s| domain == *s)
}

/// Compute Levenshtein distance between two strings.
fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let m = a.len();
    let n = b.len();

    if m == 0 {
        return n;
    }
    if n == 0 {
        return m;
    }

    let mut dp = vec![vec![0usize; n + 1]; m + 1];
    for i in 0..=m {
        dp[i][0] = i;
    }
    for j in 0..=n {
        dp[0][j] = j;
    }

    for i in 1..=m {
        for j in 1..=n {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            dp[i][j] = (dp[i - 1][j] + 1)
                .min(dp[i][j - 1] + 1)
                .min(dp[i - 1][j - 1] + cost);
        }
    }

    dp[m][n]
}

/// Normalize homoglyphs in a domain name.
/// Common substitutions: 0→o, 1→l/i, rn→m, vv→w, cl→d.
pub fn normalize_homoglyphs(domain: &str) -> String {
    // Work on the domain labels (split by dot), normalize each label, rejoin
    let labels: Vec<String> = domain
        .split('.')
        .map(|label| {
            let mut s = label.to_string();
            s = s.replace('0', "o");
            s = s.replace('1', "l");
            s = s.replace("rn", "m");
            s = s.replace("vv", "w");
            s = s.replace("cl", "d");
            s
        })
        .collect();
    labels.join(".")
}

/// Extract the second-level domain label (the part just before the TLD).
/// e.g. "docs.google.com" → "google", "paypa1-secure.com" → "paypa1-secure"
fn sld_label(domain: &str) -> &str {
    let parts: Vec<&str> = domain.split('.').collect();
    if parts.len() >= 2 {
        parts[parts.len() - 2]
    } else {
        domain
    }
}

/// Check if the domain is a lookalike of any known trusted domain.
/// Returns the trusted domain it resembles, if any.
///
/// Strategy:
/// 1. SLD label comparison: compare the second-level domain label (before TLD)
///    against known brand labels. Distance 1-2 = danger, but only when both labels
///    are at least 5 chars (short labels produce too many false positives).
/// 2. Hyphenated brand check: if the SLD contains a hyphen, split on '-' and check
///    if the first token closely resembles a known brand label.
pub fn find_lookalike(domain: &str) -> Option<String> {
    // Don't flag if domain is already trusted
    if is_trusted_domain(domain) {
        return None;
    }

    let sld = sld_label(domain);

    for trusted in KNOWN_TRUSTED_DOMAINS {
        let trusted_sld = sld_label(trusted);

        // Exact match on SLD → not a lookalike (it IS the domain or a subdomain)
        if sld == trusted_sld {
            continue;
        }

        // Guard: only compare SLDs that are long enough to avoid false positives.
        // Short SLDs (< 5 chars) are too prone to accidental matches.
        if sld.len() < 5 || trusted_sld.len() < 5 {
            continue;
        }

        let dist = levenshtein(sld, trusted_sld);
        if dist >= 1 && dist <= 2 {
            return Some(trusted.to_string());
        }
    }

    // Hyphenated brand check: "paypa1-secure.com" → prefix "paypa1" ≈ "paypal"
    if sld.contains('-') {
        let prefix = sld.split('-').next().unwrap_or(sld);
        if prefix.len() >= 5 {
            for trusted in KNOWN_TRUSTED_DOMAINS {
                let trusted_sld = sld_label(trusted);
                if trusted_sld.len() < 5 {
                    continue;
                }
                if prefix != trusted_sld {
                    let dist = levenshtein(prefix, trusted_sld);
                    if dist <= 2 {
                        return Some(trusted.to_string());
                    }
                }
            }
        }
    }

    None
}

/// Check for homoglyph substitutions that, after normalization, match a known domain.
pub fn find_homoglyph_match(domain: &str) -> Option<String> {
    let normalized = normalize_homoglyphs(domain);
    // Only flag if the domain changed after normalization
    if normalized == domain {
        return None;
    }
    // Compare normalized SLD against known trusted SLDs
    let norm_sld = sld_label(&normalized).to_string();
    for trusted in KNOWN_TRUSTED_DOMAINS {
        let trusted_sld = sld_label(trusted);
        if norm_sld == trusted_sld && sld_label(domain) != trusted_sld {
            return Some(trusted.to_string());
        }
    }
    None
}

/// Try to extract a domain from plain display text (e.g. "Visit paypal.com").
/// Returns None if no domain-like token is found.
fn extract_display_domain(display_text: &str) -> Option<String> {
    // Look for something that looks like a URL in the display text
    if display_text.contains("://") {
        return extract_domain(display_text);
    }
    // Otherwise look for tokens that contain a dot and look like a domain
    for token in display_text.split_whitespace() {
        let token = token.trim_matches(|c: char| !c.is_alphanumeric() && c != '.' && c != '-');
        if token.contains('.') && token.len() > 3 && !token.starts_with('.') {
            // Rough domain heuristic: at least one dot, no spaces, reasonable chars
            let looks_like_domain = token
                .chars()
                .all(|c| c.is_alphanumeric() || c == '.' || c == '-');
            if looks_like_domain {
                return Some(token.to_lowercase());
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Core analysis
// ---------------------------------------------------------------------------

/// Analyze a single extracted link and return a full LinkAnalysis.
pub fn analyze_link(link: &ExtractedLink) -> LinkAnalysis {
    let url = &link.url;
    let display_text = link.display_text.trim().to_string();

    let domain = match extract_domain(url) {
        Some(d) => d,
        None => {
            return LinkAnalysis {
                url: url.clone(),
                display_text,
                safety_level: SafetyLevel::Caution,
                reasons: vec!["Could not parse domain".to_string()],
                domain: String::new(),
                is_shortened: false,
                redirect_target: None,
            };
        }
    };

    let is_https = url.starts_with("https://");
    let is_shortened = is_shortener_domain(&domain);
    let path = extract_path(url);

    let mut reasons: Vec<String> = Vec::new();
    let mut danger_signals = 0u32;
    let mut caution_signals = 0u32;

    // --- HTTPS check ---
    if is_https {
        reasons.push("HTTPS".to_string());
    } else {
        reasons.push("HTTP only (not encrypted)".to_string());
        caution_signals += 1;
    }

    // --- Known trusted domain ---
    if is_trusted_domain(&domain) {
        reasons.push("Known trusted domain".to_string());
        // For major services, provide more specific reason
        if domain.ends_with("google.com") || domain == "google.com" {
            reasons.push("Verified Google domain".to_string());
        } else if domain.ends_with("microsoft.com") || domain.ends_with("office.com") {
            reasons.push("Verified Microsoft domain".to_string());
        }
    }

    // --- URL shortener ---
    if is_shortened {
        reasons.push("URL shortener".to_string());
        reasons.push("Destination unknown".to_string());
        caution_signals += 1;

        if display_text.is_empty()
            || display_text == *url
            || display_text.to_lowercase().contains("click")
            || display_text.to_lowercase().contains("here")
        {
            reasons.push("Vague display text".to_string());
            caution_signals += 1;
        }
    }

    // --- Lookalike domain ---
    if !is_trusted_domain(&domain) {
        if let Some(resembles) = find_lookalike(&domain) {
            reasons.push(format!("Lookalike domain of {}", resembles));
            danger_signals += 1;
        }
    }

    // --- Homoglyph detection ---
    if !is_trusted_domain(&domain) {
        if let Some(impersonates) = find_homoglyph_match(&domain) {
            reasons.push(format!("Character substitution detected (impersonates {})", impersonates));
            danger_signals += 1;
        }
    }

    // --- Suspicious paths ---
    for suspicious_path in SUSPICIOUS_PATHS {
        // Match path component (starts with the suspicious segment)
        let path_lower = path.to_lowercase();
        if path_lower == *suspicious_path
            || path_lower.starts_with(&format!("{}/", suspicious_path))
            || path_lower.starts_with(&format!("{}?", suspicious_path))
            || path_lower.starts_with(&format!("{}#", suspicious_path))
        {
            // Only flag if not on a trusted domain
            if !is_trusted_domain(&domain) {
                reasons.push(format!("Suspicious path: {}", suspicious_path));
                caution_signals += 1;
            }
            break;
        }
    }

    // --- Display text mismatch ---
    if !display_text.is_empty() {
        if let Some(display_domain) = extract_display_domain(&display_text) {
            // Check if the display domain looks like a known domain but the URL points elsewhere
            if display_domain != domain && !domain.ends_with(&format!(".{}", display_domain)) {
                // The display text mentions a different domain than the actual URL
                if is_trusted_domain(&display_domain) || find_lookalike(&display_domain).is_none() {
                    reasons.push(format!(
                        "Mismatched display text (shows {} but links to {})",
                        display_domain, domain
                    ));
                    danger_signals += 1;
                }
            }
        }
    }

    // --- Determine safety level ---
    let safety_level = if danger_signals > 0 {
        SafetyLevel::Danger
    } else if caution_signals >= 2 || (caution_signals >= 1 && !is_trusted_domain(&domain)) {
        SafetyLevel::Caution
    } else if is_trusted_domain(&domain) && is_https {
        SafetyLevel::Safe
    } else if caution_signals > 0 {
        SafetyLevel::Caution
    } else {
        // Unknown domain but no red flags
        SafetyLevel::Caution
    };

    LinkAnalysis {
        url: url.clone(),
        display_text,
        safety_level,
        reasons,
        domain,
        is_shortened,
        redirect_target: None,
    }
}

/// Scan all links in a message body and return the full analysis.
pub fn scan_links(html: &str) -> ScanLinksResponse {
    let extracted = extract_links(html);
    let links: Vec<LinkAnalysis> = extracted.iter().map(analyze_link).collect();

    let safe = links.iter().filter(|l| l.safety_level == SafetyLevel::Safe).count();
    let caution = links.iter().filter(|l| l.safety_level == SafetyLevel::Caution).count();
    let danger = links.iter().filter(|l| l.safety_level == SafetyLevel::Danger).count();
    let total = links.len();

    ScanLinksResponse {
        links,
        summary: ScanSummary {
            total,
            safe,
            caution,
            danger,
        },
    }
}

// ---------------------------------------------------------------------------
// HTTP handler
// ---------------------------------------------------------------------------

/// POST /api/messages/{id}/scan-links
///
/// Scan all links in the given message's HTML body and return safety analysis.
pub async fn scan_message_links(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<ScanLinksResponse>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Fetch the message body_html from the DB
    let body_html: Option<String> = conn
        .query_row(
            "SELECT body_html FROM messages WHERE id = ?1",
            rusqlite::params![&id],
            |row| row.get(0),
        )
        .ok()
        .flatten();

    // Verify message exists (if body is NULL, row still exists)
    let exists: bool = conn
        .query_row(
            "SELECT COUNT(1) FROM messages WHERE id = ?1",
            rusqlite::params![&id],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0)
        > 0;

    if !exists {
        return Err(StatusCode::NOT_FOUND);
    }

    let html = body_html.as_deref().unwrap_or("");
    let response = scan_links(html);

    Ok(Json(response))
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- Link extraction ---

    #[test]
    fn test_extract_anchor_links() {
        let html = r#"<p>Visit <a href="https://docs.google.com/spreadsheet/abc">Q3 Report</a> for details.</p>"#;
        let links = extract_links(html);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].url, "https://docs.google.com/spreadsheet/abc");
        assert_eq!(links[0].display_text, "Q3 Report");
    }

    #[test]
    fn test_extract_multiple_anchor_links() {
        let html = r#"
            <a href="https://example.com">Example</a>
            <a href="https://bit.ly/3xK9mP2">Click here</a>
        "#;
        let links = extract_links(html);
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].url, "https://example.com");
        assert_eq!(links[1].url, "https://bit.ly/3xK9mP2");
        assert_eq!(links[1].display_text, "Click here");
    }

    #[test]
    fn test_extract_plain_text_url() {
        let html = r#"<p>Visit https://example.com for more info.</p>"#;
        let links = extract_links(html);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].url, "https://example.com");
        assert_eq!(links[0].display_text, "");
    }

    #[test]
    fn test_no_duplicate_urls_anchor_and_plain() {
        // URL that appears both in an anchor and as plain text — should not duplicate
        let html = r#"<a href="https://example.com">Link</a> and also https://example.com"#;
        let links = extract_links(html);
        assert_eq!(links.len(), 1);
    }

    #[test]
    fn test_skip_mailto_links() {
        let html = r#"<a href="mailto:user@example.com">Email us</a>"#;
        let links = extract_links(html);
        assert_eq!(links.len(), 0);
    }

    // --- Known domain detection (safe) ---

    #[test]
    fn test_known_trusted_domain_google() {
        let link = ExtractedLink {
            url: "https://docs.google.com/spreadsheet/abc".to_string(),
            display_text: "Q3 Report".to_string(),
        };
        let analysis = analyze_link(&link);
        assert_eq!(analysis.safety_level, SafetyLevel::Safe);
        assert!(analysis.reasons.iter().any(|r| r.contains("trusted domain")));
    }

    #[test]
    fn test_known_trusted_domain_microsoft() {
        let link = ExtractedLink {
            url: "https://office.com/page".to_string(),
            display_text: "Office".to_string(),
        };
        let analysis = analyze_link(&link);
        assert_eq!(analysis.safety_level, SafetyLevel::Safe);
    }

    // --- URL shortener detection (caution) ---

    #[test]
    fn test_url_shortener_bitly() {
        let link = ExtractedLink {
            url: "https://bit.ly/3xK9mP2".to_string(),
            display_text: "Click here".to_string(),
        };
        let analysis = analyze_link(&link);
        assert_eq!(analysis.safety_level, SafetyLevel::Caution);
        assert!(analysis.is_shortened);
        assert!(analysis.reasons.iter().any(|r| r.contains("URL shortener")));
    }

    #[test]
    fn test_url_shortener_tco() {
        let link = ExtractedLink {
            url: "https://t.co/abcdef".to_string(),
            display_text: "tweet".to_string(),
        };
        let analysis = analyze_link(&link);
        assert!(analysis.is_shortened);
        assert_eq!(analysis.safety_level, SafetyLevel::Caution);
    }

    #[test]
    fn test_url_shortener_tinyurl() {
        let link = ExtractedLink {
            url: "https://tinyurl.com/xyz123".to_string(),
            display_text: "".to_string(),
        };
        let analysis = analyze_link(&link);
        assert!(analysis.is_shortened);
    }

    // --- Lookalike domain detection (danger) ---

    #[test]
    fn test_lookalike_paypal() {
        let link = ExtractedLink {
            url: "https://paypa1-secure.com/verify".to_string(),
            display_text: "Verify your account".to_string(),
        };
        let analysis = analyze_link(&link);
        assert_eq!(analysis.safety_level, SafetyLevel::Danger);
        assert!(analysis.reasons.iter().any(|r| r.contains("Lookalike") || r.contains("paypal")));
    }

    #[test]
    fn test_lookalike_google() {
        // "g00gle.com" — levenshtein 2 from "google.com"
        let link = ExtractedLink {
            url: "https://g00gle.com/page".to_string(),
            display_text: "Google".to_string(),
        };
        let analysis = analyze_link(&link);
        assert_eq!(analysis.safety_level, SafetyLevel::Danger);
    }

    // --- Homoglyph detection ---

    #[test]
    fn test_homoglyph_zero_for_o() {
        // "paypa0.com" → after normalization "paypao.com" — still not paypal.com
        // "goog1e.com" → normalize '1'→'l' → "google.com" — match!
        let link = ExtractedLink {
            url: "https://goog1e.com/drive".to_string(),
            display_text: "Google Drive".to_string(),
        };
        let analysis = analyze_link(&link);
        // Either homoglyph or lookalike detection fires
        assert_eq!(analysis.safety_level, SafetyLevel::Danger);
    }

    #[test]
    fn test_homoglyph_rn_for_m() {
        // "arnazon.com" (rn→m → "amazon.com")
        let link = ExtractedLink {
            url: "https://arnazon.com/order".to_string(),
            display_text: "Amazon Order".to_string(),
        };
        let analysis = analyze_link(&link);
        assert_eq!(analysis.safety_level, SafetyLevel::Danger);
    }

    // --- Display text mismatch detection ---

    #[test]
    fn test_display_text_mismatch() {
        let link = ExtractedLink {
            url: "https://phishing-site.com/login".to_string(),
            display_text: "paypal.com — verify your account".to_string(),
        };
        let analysis = analyze_link(&link);
        assert_eq!(analysis.safety_level, SafetyLevel::Danger);
        assert!(analysis.reasons.iter().any(|r| r.contains("Mismatched")));
    }

    // --- Suspicious path detection ---

    #[test]
    fn test_suspicious_path_verify() {
        let link = ExtractedLink {
            url: "https://unknown-bank.net/verify".to_string(),
            display_text: "Verify Now".to_string(),
        };
        let analysis = analyze_link(&link);
        assert!(analysis.reasons.iter().any(|r| r.contains("/verify")));
        assert!(matches!(
            analysis.safety_level,
            SafetyLevel::Caution | SafetyLevel::Danger
        ));
    }

    #[test]
    fn test_suspicious_path_login() {
        let link = ExtractedLink {
            url: "https://suspicious-domain.net/login".to_string(),
            display_text: "Log in".to_string(),
        };
        let analysis = analyze_link(&link);
        assert!(analysis.reasons.iter().any(|r| r.contains("/login")));
    }

    #[test]
    fn test_suspicious_path_not_flagged_on_trusted_domain() {
        // /login on github.com is totally normal — should not add suspicious path reason
        let link = ExtractedLink {
            url: "https://github.com/login".to_string(),
            display_text: "Sign in to GitHub".to_string(),
        };
        let analysis = analyze_link(&link);
        assert_eq!(analysis.safety_level, SafetyLevel::Safe);
        assert!(!analysis.reasons.iter().any(|r| r.contains("Suspicious path")));
    }

    // --- HTTP (not HTTPS) ---

    #[test]
    fn test_http_only_is_caution() {
        let link = ExtractedLink {
            url: "http://example.com/page".to_string(),
            display_text: "Page".to_string(),
        };
        let analysis = analyze_link(&link);
        assert!(analysis.reasons.iter().any(|r| r.contains("HTTP only")));
        assert!(matches!(
            analysis.safety_level,
            SafetyLevel::Caution | SafetyLevel::Danger
        ));
    }

    // --- Mixed links summary counts ---

    #[test]
    fn test_scan_links_summary_counts() {
        let html = r#"
            <a href="https://docs.google.com/spreadsheet/abc">Q3 Report</a>
            <a href="https://bit.ly/3xK9mP2">Click here</a>
            <a href="https://paypa1-secure.com/verify">Verify your account</a>
        "#;
        let result = scan_links(html);
        assert_eq!(result.summary.total, 3);
        assert_eq!(result.summary.safe, 1);
        assert_eq!(result.summary.caution, 1);
        assert_eq!(result.summary.danger, 1);
    }

    #[test]
    fn test_scan_links_empty_html() {
        let result = scan_links("");
        assert_eq!(result.summary.total, 0);
        assert_eq!(result.summary.safe, 0);
    }

    #[test]
    fn test_scan_links_no_links() {
        let html = "<p>No links in this email.</p>";
        let result = scan_links(html);
        assert_eq!(result.summary.total, 0);
    }

    // --- Domain utility functions ---

    #[test]
    fn test_extract_domain_https() {
        assert_eq!(
            extract_domain("https://docs.google.com/spreadsheet/abc"),
            Some("docs.google.com".to_string())
        );
    }

    #[test]
    fn test_extract_domain_with_port() {
        assert_eq!(
            extract_domain("http://example.com:8080/path"),
            Some("example.com".to_string())
        );
    }

    #[test]
    fn test_normalize_homoglyphs() {
        assert_eq!(normalize_homoglyphs("g00g1e.com"), "google.com");
        assert_eq!(normalize_homoglyphs("arnazon.com"), "amazon.com");
    }

    #[test]
    fn test_is_trusted_domain_subdomain() {
        assert!(is_trusted_domain("docs.google.com"));
        assert!(is_trusted_domain("mail.google.com"));
        assert!(!is_trusted_domain("goog1e.com"));
    }

    #[test]
    fn test_levenshtein_distance() {
        assert_eq!(levenshtein("google.com", "g00gle.com"), 2);
        assert_eq!(levenshtein("paypal.com", "paypa1.com"), 1);
        assert_eq!(levenshtein("amazon.com", "amazon.com"), 0);
    }
}

// ---------------------------------------------------------------------------
// Integration tests (route registration + 404 for non-existent message)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod integration_tests {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use r2d2::Pool;
    use r2d2_sqlite::SqliteConnectionManager;
    use std::sync::Arc;
    use tower::ServiceExt;

    use crate::ai::memories::MemoriesClient;
    use crate::ai::provider::ProviderPool;
    use crate::config::Config;
    use crate::db::migrations;
    use crate::ws::hub::WsHub;
    use crate::{build_app, AppState};

    const TEST_TOKEN: &str = "test-session-token-link-safety";

    fn create_test_state() -> Arc<AppState> {
        let manager = SqliteConnectionManager::memory().with_init(|conn| {
            conn.execute_batch(
                "PRAGMA foreign_keys = ON;
                 PRAGMA synchronous = NORMAL;
                 PRAGMA busy_timeout = 5000;",
            )
        });
        let pool = Pool::builder().max_size(1).build(manager).unwrap();
        {
            let conn = pool.get().unwrap();
            migrations::run(&conn).unwrap();
        }

        Arc::new(AppState {
            db: pool,
            config: Config {
                port: 3000,
                database_url: ":memory:".to_string(),
                ollama_url: "http://localhost:11434".to_string(),
                memories_url: "http://localhost:8900".to_string(),
                memories_api_key: None,
                anthropic_api_key: None,
                openai_api_key: None,
                gmail_client_id: None,
                gmail_client_secret: None,
                outlook_client_id: None,
                outlook_client_secret: None,
                public_url: "http://localhost:3000".to_string(),
                job_poll_interval_ms: 2000,
                job_max_concurrency: 4,
                job_cleanup_days: 7,
            },
            ws_hub: WsHub::new(),
            providers: ProviderPool::new(vec![]),
            memories: MemoriesClient::new("http://localhost:8900", None),
            session_token: TEST_TOKEN.to_string(),
        })
    }

    #[tokio::test]
    async fn test_scan_links_route_registered() {
        // Route should return 404 for non-existent message (not 405 method not allowed)
        let state = create_test_state();
        let app = build_app(state);

        let res = app
            .oneshot(
                Request::post("/api/messages/nonexistent-id/scan-links")
                    .header("x-session-token", TEST_TOKEN)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_scan_links_requires_auth() {
        let state = create_test_state();
        let app = build_app(state);

        let res = app
            .oneshot(
                Request::post("/api/messages/some-id/scan-links")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_scan_links_returns_json_for_existing_message() {
        let state = create_test_state();
        // Insert a test account and message with HTML body
        {
            let conn = state.db.get().unwrap();
            conn.execute(
                "INSERT INTO accounts (id, provider, email, display_name, created_at, updated_at)
                 VALUES ('acc1', 'gmail', 'test@example.com', 'Test User', 0, 0)",
                [],
            ).unwrap();
            conn.execute(
                "INSERT INTO messages (id, account_id, folder, subject, date, body_html, is_read, is_starred, is_deleted, has_attachments, is_draft)
                 VALUES (?1, 'acc1', 'INBOX', 'Test', 1000, ?2, 0, 0, 0, 0, 0)",
                rusqlite::params![
                    "msg-link-test-001",
                    r#"<a href="https://docs.google.com/spreadsheet">Report</a>"#
                ],
            ).unwrap();
        }

        let app = build_app(state);

        let res = app
            .oneshot(
                Request::post("/api/messages/msg-link-test-001/scan-links")
                    .header("x-session-token", TEST_TOKEN)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::OK);

        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

        assert!(json["links"].is_array());
        assert_eq!(json["summary"]["total"], 1);
        assert_eq!(json["summary"]["safe"], 1);
        assert_eq!(json["summary"]["danger"], 0);
    }
}

use serde::Serialize;

// --- Impersonation detection ---

/// Known trusted domains to check against for lookalike attacks.
const KNOWN_DOMAINS: &[&str] = &[
    "gmail.com",
    "outlook.com",
    "hotmail.com",
    "yahoo.com",
    "icloud.com",
    "paypal.com",
    "apple.com",
    "microsoft.com",
    "amazon.com",
    "google.com",
    "facebook.com",
    "instagram.com",
    "twitter.com",
    "linkedin.com",
    "chase.com",
    "bankofamerica.com",
    "wellsfargo.com",
    "citi.com",
    "netflix.com",
    "dropbox.com",
    "github.com",
    "slack.com",
    "zoom.us",
    "stripe.com",
    "protonmail.com",
    "aol.com",
];

/// Impersonation risk detected for a sender domain.
#[derive(Debug, Clone, Serialize)]
pub struct ImpersonationRisk {
    /// The legitimate domain this sender's domain resembles.
    pub lookalike_of: String,
    /// "high" (edit distance 1 or homoglyph match) or "medium" (edit distance 2).
    pub risk_level: String,
}

/// Compute the Levenshtein (edit) distance between two strings.
fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let (a_len, b_len) = (a.len(), b.len());

    // Quick reject: if length difference > 2, distance is at least that.
    if a_len.abs_diff(b_len) > 2 {
        return a_len.abs_diff(b_len);
    }

    let mut matrix = vec![vec![0usize; b_len + 1]; a_len + 1];
    for i in 0..=a_len {
        matrix[i][0] = i;
    }
    for j in 0..=b_len {
        matrix[0][j] = j;
    }
    for i in 1..=a_len {
        for j in 1..=b_len {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            matrix[i][j] = (matrix[i - 1][j] + 1)
                .min(matrix[i][j - 1] + 1)
                .min(matrix[i - 1][j - 1] + cost);
        }
    }
    matrix[a_len][b_len]
}

/// Check if `suspect` could be a homoglyph variant of `genuine`.
///
/// Applies common character substitutions to the suspect domain and checks
/// if the normalised form matches the genuine domain.
fn has_homoglyphs(suspect: &str, genuine: &str) -> bool {
    // Multi-char substitutions first (order matters: longer patterns before shorter)
    let multi_subs: &[(&str, &str)] = &[
        ("rn", "m"),
        ("vv", "w"),
        ("cl", "d"),
        ("nn", "m"),
    ];
    let mut normalized = suspect.to_string();
    for (from, to) in multi_subs {
        normalized = normalized.replace(from, to);
    }

    // Single-char substitutions
    let single_subs: &[(char, char)] = &[
        ('0', 'o'),
        ('1', 'l'),
        ('1', 'i'),
        ('5', 's'),
    ];
    // We need to try each single sub independently (some overlap), so apply all at once.
    let normalized: String = normalized
        .chars()
        .map(|c| {
            for (from, to) in single_subs {
                if c == *from {
                    return *to;
                }
            }
            c
        })
        .collect();

    normalized == genuine && suspect != genuine
}

/// Check if a sender domain looks like it's impersonating a known trusted domain.
///
/// Returns `None` if the domain is safe (exact match or unrelated).
/// Returns `Some(ImpersonationRisk)` with risk level if a lookalike is detected.
pub fn check_impersonation(sender_domain: &str) -> Option<ImpersonationRisk> {
    let domain_lower = sender_domain.to_lowercase();

    for known in KNOWN_DOMAINS {
        // Exact match = legitimate, skip
        if domain_lower == *known {
            return None;
        }
    }

    for known in KNOWN_DOMAINS {
        // Homoglyph check (highest confidence)
        if has_homoglyphs(&domain_lower, known) {
            return Some(ImpersonationRisk {
                lookalike_of: known.to_string(),
                risk_level: "high".to_string(),
            });
        }

        // Levenshtein distance check
        let distance = levenshtein(&domain_lower, known);
        if distance == 1 {
            return Some(ImpersonationRisk {
                lookalike_of: known.to_string(),
                risk_level: "high".to_string(),
            });
        }
        if distance == 2 {
            return Some(ImpersonationRisk {
                lookalike_of: known.to_string(),
                risk_level: "medium".to_string(),
            });
        }
    }

    None
}

/// Extract the domain part from an email address (e.g. "user@example.com" -> "example.com").
pub fn domain_from_email(email: &str) -> Option<&str> {
    email.rsplit_once('@').map(|(_, domain)| domain)
}

// --- SPF / DKIM / DMARC trust indicators ---

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AuthStatus {
    Pass,
    Fail,
    Softfail,
    None,
    Neutral,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct TrustIndicators {
    pub spf: Option<AuthStatus>,
    pub dkim: Option<AuthStatus>,
    pub dmarc: Option<AuthStatus>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TrackingPixel {
    pub url: String,
    pub domain: String,
}

/// Known tracking pixel domains.
const KNOWN_TRACKER_DOMAINS: &[&str] = &[
    "mailchimp.com",
    "list-manage.com",
    "sendgrid.net",
    "sendgrid.com",
    "hubspot.com",
    "hsforms.com",
    "pixel.watch",
    "mailtrack.io",
    "google-analytics.com",
    "t.co",
    "clicks.beehiiv.com",
    "convertkit.com",
    "drip.com",
    "em.sailthru.com",
    "exact-target.com",
    "pardot.com",
    "mktotracking.com",
];

/// Parse a status word into an AuthStatus enum variant.
fn parse_status(s: &str) -> Option<AuthStatus> {
    match s.to_lowercase().as_str() {
        "pass" => Some(AuthStatus::Pass),
        "fail" => Some(AuthStatus::Fail),
        "softfail" => Some(AuthStatus::Softfail),
        "none" => Some(AuthStatus::None),
        "neutral" => Some(AuthStatus::Neutral),
        _ => Option::None,
    }
}

/// Parse an Authentication-Results header value into TrustIndicators.
///
/// The header value is split by semicolons. The first segment is typically the
/// authserv-id (e.g., `mx.google.com`). Subsequent segments contain results
/// like `spf=pass (google.com: ...)` or `dkim=pass header.d=example.com`.
pub fn parse_authentication_results(header_value: &str) -> TrustIndicators {
    let mut indicators = TrustIndicators::default();

    for part in header_value.split(';') {
        let trimmed = part.trim();
        let lower = trimmed.to_lowercase();

        for (prefix, field) in [("spf=", "spf"), ("dkim=", "dkim"), ("dmarc=", "dmarc")] {
            if let Some(pos) = lower.find(prefix) {
                let after_prefix = &lower[pos + prefix.len()..];
                // The status word is the first token, ending at whitespace or '(' or end of string
                let status_word: String = after_prefix
                    .chars()
                    .take_while(|c| c.is_alphanumeric())
                    .collect();
                if let Some(status) = parse_status(&status_word) {
                    match field {
                        "spf" => indicators.spf = Some(status),
                        "dkim" => indicators.dkim = Some(status),
                        "dmarc" => indicators.dmarc = Some(status),
                        _ => {}
                    }
                }
            }
        }
    }

    indicators
}

/// Extract trust indicators from raw email headers.
///
/// Searches for the `Authentication-Results:` header line, handling possible
/// continuation lines (lines starting with whitespace), then delegates to
/// `parse_authentication_results`.
pub fn extract_trust_indicators(raw_headers: &str) -> TrustIndicators {
    let mut auth_value = String::new();
    let mut in_auth_header = false;

    for line in raw_headers.lines() {
        if in_auth_header {
            // Continuation lines start with whitespace (folded header)
            if line.starts_with(' ') || line.starts_with('\t') {
                auth_value.push(' ');
                auth_value.push_str(line.trim());
                continue;
            } else {
                // End of the Authentication-Results header
                break;
            }
        }

        let lower = line.to_lowercase();
        if lower.starts_with("authentication-results:") {
            in_auth_header = true;
            let value = &line["authentication-results:".len()..];
            auth_value.push_str(value.trim());
        }
    }

    if auth_value.is_empty() {
        return TrustIndicators::default();
    }

    parse_authentication_results(&auth_value)
}

/// Extract the value of an HTML attribute from a tag string.
///
/// Handles double-quoted, single-quoted, and unquoted attribute values.
/// `tag` is the original-case tag text, `tag_lower` is the lowercased version,
/// and `attr` should be lowercase.
fn extract_attr(tag: &str, tag_lower: &str, attr: &str) -> Option<String> {
    let search = format!("{}=", attr);
    let pos = tag_lower.find(&search)?;
    let start = pos + search.len();
    let remaining = &tag[start..];

    if remaining.starts_with('"') {
        // Double-quoted value
        let end = remaining[1..].find('"')?;
        Some(remaining[1..1 + end].to_string())
    } else if remaining.starts_with('\'') {
        // Single-quoted value
        let end = remaining[1..].find('\'')?;
        Some(remaining[1..1 + end].to_string())
    } else {
        // Unquoted value — ends at whitespace or '>'
        let value: String = remaining
            .chars()
            .take_while(|c| !c.is_whitespace() && *c != '>')
            .collect();
        if value.is_empty() {
            Option::None
        } else {
            Some(value)
        }
    }
}

/// Extract a numeric attribute value from a lowercased tag string.
fn extract_numeric_attr(tag_lower: &str, attr: &str) -> Option<u32> {
    let search = format!("{}=", attr);
    let pos = tag_lower.find(&search)?;
    let start = pos + search.len();
    let remaining = &tag_lower[start..];

    // Skip opening quote if present
    let value_start = if remaining.starts_with('"') || remaining.starts_with('\'') {
        1
    } else {
        0
    };

    let digits: String = remaining[value_start..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();

    digits.parse().ok()
}

/// Check if an img tag represents a tiny (tracking-sized) image.
/// Returns true if width <= 1 AND height <= 1.
fn is_tiny_image(tag_lower: &str) -> bool {
    let width = extract_numeric_attr(tag_lower, "width");
    let height = extract_numeric_attr(tag_lower, "height");

    match (width, height) {
        (Some(w), Some(h)) => w <= 1 && h <= 1,
        _ => false,
    }
}

/// Extract the domain from a URL, stripping protocol and path.
fn extract_domain(url: &str) -> Option<String> {
    let without_protocol = if let Some(pos) = url.find("://") {
        &url[pos + 3..]
    } else {
        url
    };

    // Remove path, query, and fragment
    let domain = without_protocol
        .split('/')
        .next()
        .unwrap_or(without_protocol);

    // Remove port if present
    let domain = domain.split(':').next().unwrap_or(domain);

    if domain.is_empty() {
        Option::None
    } else {
        Some(domain.to_lowercase())
    }
}

/// Check if a domain matches any known tracker domain (including subdomains).
fn is_known_tracker(domain: &str) -> bool {
    KNOWN_TRACKER_DOMAINS.iter().any(|tracker| {
        domain == *tracker || domain.ends_with(&format!(".{}", tracker))
    })
}

/// Detect tracking pixels in HTML content.
///
/// Scans for `<img` tags and checks each for:
/// - Tiny size (width/height <= 1)
/// - Source URL from a known tracker domain
///
/// Returns a list of detected tracking pixels.
pub fn detect_tracking_pixels(html: &str) -> Vec<TrackingPixel> {
    let mut trackers = Vec::new();
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

        // Extract the src attribute
        if let Some(src) = extract_attr(tag, tag_lower, "src") {
            let is_tracker = if is_tiny_image(tag_lower) {
                true
            } else if let Some(domain) = extract_domain(&src) {
                is_known_tracker(&domain)
            } else {
                false
            };

            if is_tracker {
                let domain = extract_domain(&src).unwrap_or_default();
                trackers.push(TrackingPixel { url: src, domain });
            }
        }

        search_from = tag_end;
    }

    trackers
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_auth_results_all_pass() {
        let header = "mx.google.com; dkim=pass header.d=example.com; spf=pass (google.com: domain of sender@example.com); dmarc=pass (p=REJECT)";
        let result = parse_authentication_results(header);
        assert_eq!(result.spf, Some(AuthStatus::Pass));
        assert_eq!(result.dkim, Some(AuthStatus::Pass));
        assert_eq!(result.dmarc, Some(AuthStatus::Pass));
    }

    #[test]
    fn test_parse_auth_results_mixed() {
        let header = "mx.example.com; spf=fail; dkim=none; dmarc=fail";
        let result = parse_authentication_results(header);
        assert_eq!(result.spf, Some(AuthStatus::Fail));
        assert_eq!(result.dkim, Some(AuthStatus::None));
        assert_eq!(result.dmarc, Some(AuthStatus::Fail));
    }

    #[test]
    fn test_parse_auth_results_missing_header() {
        let headers = "From: sender@example.com\r\nSubject: Test\r\n";
        let result = extract_trust_indicators(headers);
        assert!(result.spf.is_none());
        assert!(result.dkim.is_none());
        assert!(result.dmarc.is_none());
    }

    #[test]
    fn test_detect_tracking_pixels_found() {
        let html = r#"<html><body>Hello<img src="https://track.mailchimp.com/open.gif" width="1" height="1"></body></html>"#;
        let trackers = detect_tracking_pixels(html);
        assert!(!trackers.is_empty());
    }

    #[test]
    fn test_detect_tracking_pixels_none() {
        let html = r#"<html><body><img src="photo.jpg" width="400" height="300"></body></html>"#;
        let trackers = detect_tracking_pixels(html);
        assert!(trackers.is_empty());
    }

    #[test]
    fn test_detect_tracking_pixels_known_domains() {
        let html = r#"<img src="https://pixel.watch/abc123"><img src="https://t.co/open">"#;
        let trackers = detect_tracking_pixels(html);
        assert!(!trackers.is_empty());
    }

    #[test]
    fn test_end_to_end_trust_from_raw_headers() {
        let raw = "Authentication-Results: mx.google.com; dkim=pass header.d=example.com; spf=pass; dmarc=pass\r\nFrom: test@example.com\r\n";
        let indicators = extract_trust_indicators(raw);
        assert_eq!(indicators.spf, Some(AuthStatus::Pass));
        assert_eq!(indicators.dkim, Some(AuthStatus::Pass));
        assert_eq!(indicators.dmarc, Some(AuthStatus::Pass));
    }

    // --- Impersonation detection tests ---

    #[test]
    fn test_levenshtein_basic() {
        assert_eq!(levenshtein("kitten", "sitting"), 3);
        assert_eq!(levenshtein("abc", "abc"), 0);
        assert_eq!(levenshtein("abc", "ab"), 1);
        assert_eq!(levenshtein("abc", "axc"), 1);
        assert_eq!(levenshtein("", "abc"), 3);
        assert_eq!(levenshtein("abc", ""), 3);
    }

    #[test]
    fn test_exact_domain_no_alert() {
        // Exact match of a known domain must NOT trigger
        assert!(check_impersonation("gmail.com").is_none());
        assert!(check_impersonation("paypal.com").is_none());
        assert!(check_impersonation("outlook.com").is_none());
    }

    #[test]
    fn test_levenshtein_lookalike() {
        // gmai1.com — edit distance 1 from gmail.com
        let risk = check_impersonation("gmai1.com");
        assert!(risk.is_some());
        let risk = risk.unwrap();
        assert_eq!(risk.lookalike_of, "gmail.com");
        assert_eq!(risk.risk_level, "high");
    }

    #[test]
    fn test_homoglyph_detection() {
        // paypa1.com — '1' looks like 'l'
        let risk = check_impersonation("paypa1.com");
        assert!(risk.is_some());
        let risk = risk.unwrap();
        assert_eq!(risk.lookalike_of, "paypal.com");
        assert_eq!(risk.risk_level, "high");
    }

    #[test]
    fn test_homoglyph_rn_for_m() {
        // arnazon.com — 'rn' looks like 'm'
        let risk = check_impersonation("arnazon.com");
        assert!(risk.is_some());
        let risk = risk.unwrap();
        assert_eq!(risk.lookalike_of, "amazon.com");
        assert_eq!(risk.risk_level, "high");
    }

    #[test]
    fn test_homoglyph_zero_for_o() {
        // g00gle.com — '0' looks like 'o'
        let risk = check_impersonation("g00gle.com");
        assert!(risk.is_some());
        let risk = risk.unwrap();
        assert_eq!(risk.lookalike_of, "google.com");
        assert_eq!(risk.risk_level, "high");
    }

    #[test]
    fn test_unrelated_domain_no_alert() {
        // Completely unrelated domains should not trigger
        assert!(check_impersonation("customdomain.com").is_none());
        assert!(check_impersonation("mycompany.org").is_none());
        assert!(check_impersonation("university.edu").is_none());
    }

    #[test]
    fn test_medium_risk_edit_distance_2() {
        // gnaik.com — edit distance 2 from gmail.com (m→n, l→k)
        let risk = check_impersonation("gnaik.com");
        assert!(risk.is_some());
        let risk = risk.unwrap();
        assert_eq!(risk.risk_level, "medium");
    }

    #[test]
    fn test_domain_from_email() {
        assert_eq!(domain_from_email("user@example.com"), Some("example.com"));
        assert_eq!(domain_from_email("user@gmail.com"), Some("gmail.com"));
        assert_eq!(domain_from_email("noemail"), None);
    }
}

use serde::Serialize;

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
}

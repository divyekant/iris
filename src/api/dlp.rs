use axum::http::StatusCode;
use axum::Json;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use std::sync::Mutex;
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FindingType {
    CreditCard,
    Ssn,
    ApiKey,
    Password,
    PrivateKey,
    BankAccount,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DlpFinding {
    #[serde(rename = "type")]
    pub finding_type: FindingType,
    #[serde(rename = "match")]
    pub masked_match: String,
    pub location: String, // "subject" or "body"
    pub line: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    None,
    Low,
    High,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DlpScanResult {
    pub findings: Vec<DlpFinding>,
    pub risk_level: RiskLevel,
    pub allow_send: bool,
}

#[derive(Debug, Deserialize)]
pub struct ScanRequest {
    pub subject: String,
    pub body: String,
    #[allow(dead_code)]
    pub to: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct OverrideRequest {
    pub findings_acknowledged: bool,
}

#[derive(Debug, Serialize)]
pub struct OverrideResponse {
    pub token: String,
}

// ---------------------------------------------------------------------------
// One-time override tokens (in-memory store, stateless across restarts)
// ---------------------------------------------------------------------------

static OVERRIDE_TOKENS: Lazy<Mutex<HashMap<String, bool>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

pub fn validate_override_token(token: &str) -> bool {
    let mut tokens = OVERRIDE_TOKENS.lock().unwrap();
    tokens.remove(token).is_some()
}

// ---------------------------------------------------------------------------
// Compiled regex patterns (compiled once, reused)
// ---------------------------------------------------------------------------

static RE_HTML_TAGS: Lazy<Regex> = Lazy::new(|| Regex::new(r"<[^>]+>").unwrap());

// Credit card: 13-19 digits optionally separated by spaces or dashes
static RE_CREDIT_CARD: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\b(?:\d[ -]?){13,19}\b").unwrap());

// SSN: XXX-XX-XXXX with or without dashes
static RE_SSN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\b\d{3}[- ]?\d{2}[- ]?\d{4}\b").unwrap());

// API keys
static RE_API_KEY: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?:sk-[a-zA-Z0-9_-]{20,}|pk_(?:live|test)_[a-zA-Z0-9]{20,}|AKIA[0-9A-Z]{16}|ghp_[a-zA-Z0-9]{20,}|glpat-[a-zA-Z0-9_-]{20,}|xoxb-[a-zA-Z0-9-]{20,}|Bearer\s+ey[a-zA-Z0-9._-]{20,})").unwrap()
});

// Passwords: "password:", "pwd:", "pass:" followed by non-whitespace
static RE_PASSWORD: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)(?:password|pwd|pass)\s*[:=]\s*\S+").unwrap()
});

// Private keys
static RE_PRIVATE_KEY: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"-----BEGIN (?:RSA |EC )?PRIVATE KEY-----").unwrap()
});

// IBAN: 2 letters + 2 digits + 4-30 alphanumeric
static RE_IBAN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\b[A-Z]{2}\d{2}[A-Z0-9]{4,30}\b").unwrap());

// US routing number: 9 digits starting with 0-3
static RE_ROUTING: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\b[0-3]\d{8}\b").unwrap());

// ---------------------------------------------------------------------------
// Luhn algorithm
// ---------------------------------------------------------------------------

pub fn luhn_check(number: &str) -> bool {
    let digits: Vec<u32> = number
        .chars()
        .filter(|c| c.is_ascii_digit())
        .map(|c| c.to_digit(10).unwrap())
        .collect();

    if digits.len() < 13 || digits.len() > 19 {
        return false;
    }

    let mut sum = 0u32;
    let mut double = false;
    for &d in digits.iter().rev() {
        let mut val = d;
        if double {
            val *= 2;
            if val > 9 {
                val -= 9;
            }
        }
        sum += val;
        double = !double;
    }
    sum % 10 == 0
}

/// Check if digits start with a valid card prefix (Visa, MC, Amex).
fn is_card_prefix(digits: &str) -> bool {
    let clean: String = digits.chars().filter(|c| c.is_ascii_digit()).collect();
    if clean.is_empty() {
        return false;
    }
    // Visa: starts with 4
    if clean.starts_with('4') {
        return true;
    }
    // Mastercard: starts with 51-55
    if clean.len() >= 2 {
        if let Ok(prefix) = clean[..2].parse::<u32>() {
            if (51..=55).contains(&prefix) {
                return true;
            }
        }
    }
    // Amex: starts with 34 or 37
    if clean.starts_with("34") || clean.starts_with("37") {
        return true;
    }
    false
}

// ---------------------------------------------------------------------------
// Masking
// ---------------------------------------------------------------------------

pub fn mask_value(value: &str) -> String {
    let clean: String = value.chars().filter(|c| !c.is_whitespace()).collect();
    if clean.len() <= 8 {
        return "*".repeat(clean.len());
    }
    let first4 = &clean[..4];
    let last4 = &clean[clean.len() - 4..];
    let middle_len = clean.len() - 8;
    format!("{}{}{}", first4, "*".repeat(middle_len), last4)
}

// ---------------------------------------------------------------------------
// Strip HTML tags
// ---------------------------------------------------------------------------

pub fn strip_html(html: &str) -> String {
    RE_HTML_TAGS.replace_all(html, "").to_string()
}

// ---------------------------------------------------------------------------
// Core scan logic
// ---------------------------------------------------------------------------

pub fn scan_text(text: &str, location: &str) -> Vec<DlpFinding> {
    let mut findings = Vec::new();

    for (line_num, line) in text.lines().enumerate() {
        let line_1based = line_num + 1;

        // Credit cards
        for m in RE_CREDIT_CARD.find_iter(line) {
            let matched = m.as_str();
            if is_card_prefix(matched) && luhn_check(matched) {
                findings.push(DlpFinding {
                    finding_type: FindingType::CreditCard,
                    masked_match: mask_value(matched),
                    location: location.to_string(),
                    line: line_1based,
                });
            }
        }

        // SSN
        for m in RE_SSN.find_iter(line) {
            let matched = m.as_str();
            let digits: String = matched.chars().filter(|c| c.is_ascii_digit()).collect();
            // Reduce false positives: skip if all digits are the same or sequential
            if digits.len() == 9 && !all_same(&digits) && !is_sequential(&digits) {
                // Skip if it matches a credit card we already found
                if !findings.iter().any(|f| {
                    f.finding_type == FindingType::CreditCard && f.line == line_1based
                }) {
                    findings.push(DlpFinding {
                        finding_type: FindingType::Ssn,
                        masked_match: mask_value(matched),
                        location: location.to_string(),
                        line: line_1based,
                    });
                }
            }
        }

        // API keys
        for m in RE_API_KEY.find_iter(line) {
            findings.push(DlpFinding {
                finding_type: FindingType::ApiKey,
                masked_match: mask_value(m.as_str()),
                location: location.to_string(),
                line: line_1based,
            });
        }

        // Passwords
        for m in RE_PASSWORD.find_iter(line) {
            findings.push(DlpFinding {
                finding_type: FindingType::Password,
                masked_match: mask_value(m.as_str()),
                location: location.to_string(),
                line: line_1based,
            });
        }

        // Private keys
        if RE_PRIVATE_KEY.is_match(line) {
            findings.push(DlpFinding {
                finding_type: FindingType::PrivateKey,
                masked_match: "-----BEGIN ***PRIVATE KEY-----".to_string(),
                location: location.to_string(),
                line: line_1based,
            });
        }

        // Bank accounts (IBAN)
        for m in RE_IBAN.find_iter(line) {
            let matched = m.as_str();
            // Must be at least 15 chars to be a real IBAN
            if matched.len() >= 15 {
                findings.push(DlpFinding {
                    finding_type: FindingType::BankAccount,
                    masked_match: mask_value(matched),
                    location: location.to_string(),
                    line: line_1based,
                });
            }
        }

        // US routing numbers
        for m in RE_ROUTING.find_iter(line) {
            let matched = m.as_str();
            // Only flag if preceded/followed by context like "routing", "account", "ABA"
            let line_lower = line.to_lowercase();
            if line_lower.contains("routing")
                || line_lower.contains("account")
                || line_lower.contains("aba")
                || line_lower.contains("bank")
            {
                findings.push(DlpFinding {
                    finding_type: FindingType::BankAccount,
                    masked_match: mask_value(matched),
                    location: location.to_string(),
                    line: line_1based,
                });
            }
        }
    }

    findings
}

fn all_same(s: &str) -> bool {
    let first = s.chars().next();
    s.chars().all(|c| Some(c) == first)
}

fn is_sequential(s: &str) -> bool {
    s == "123456789" || s == "987654321"
}

/// Determine risk level from findings.
pub fn determine_risk_level(findings: &[DlpFinding]) -> RiskLevel {
    if findings.is_empty() {
        return RiskLevel::None;
    }

    let has_critical = findings.iter().any(|f| {
        matches!(
            f.finding_type,
            FindingType::CreditCard
                | FindingType::PrivateKey
                | FindingType::Ssn
        )
    });

    if has_critical || findings.len() > 1 {
        RiskLevel::High
    } else {
        // Single finding of password or API key = low risk
        RiskLevel::Low
    }
}

/// Full scan: strip HTML from body, scan subject and body.
pub fn full_scan(subject: &str, body: &str) -> DlpScanResult {
    let clean_body = strip_html(body);
    let mut findings = scan_text(subject, "subject");
    findings.extend(scan_text(&clean_body, "body"));

    let risk_level = determine_risk_level(&findings);
    let allow_send = risk_level == RiskLevel::None;

    DlpScanResult {
        findings,
        risk_level,
        allow_send,
    }
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

pub async fn scan_dlp(
    Json(req): Json<ScanRequest>,
) -> Result<Json<DlpScanResult>, (StatusCode, Json<serde_json::Value>)> {
    let result = full_scan(&req.subject, &req.body);
    Ok(Json(result))
}

pub async fn dlp_override(
    Json(req): Json<OverrideRequest>,
) -> Result<Json<OverrideResponse>, (StatusCode, Json<serde_json::Value>)> {
    if !req.findings_acknowledged {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "findings must be acknowledged"})),
        ));
    }

    let token = Uuid::new_v4().to_string();
    {
        let mut tokens = OVERRIDE_TOKENS.lock().unwrap();
        tokens.insert(token.clone(), true);
    }

    Ok(Json(OverrideResponse { token }))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Credit card detection ----

    #[test]
    fn test_detect_visa_card() {
        let findings = scan_text("My card is 4111111111111111", "body");
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].finding_type, FindingType::CreditCard);
        assert_eq!(findings[0].line, 1);
    }

    #[test]
    fn test_detect_visa_card_with_dashes() {
        let findings = scan_text("Card: 4111-1111-1111-1111", "body");
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].finding_type, FindingType::CreditCard);
    }

    #[test]
    fn test_detect_visa_card_with_spaces() {
        let findings = scan_text("Card: 4111 1111 1111 1111", "body");
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].finding_type, FindingType::CreditCard);
    }

    #[test]
    fn test_detect_mastercard() {
        // Mastercard test number: 5500000000000004
        let findings = scan_text("MC: 5500000000000004", "body");
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].finding_type, FindingType::CreditCard);
    }

    #[test]
    fn test_detect_amex() {
        // Amex test number: 378282246310005
        let findings = scan_text("Amex: 378282246310005", "body");
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].finding_type, FindingType::CreditCard);
    }

    #[test]
    fn test_luhn_valid() {
        assert!(luhn_check("4111111111111111"));
        assert!(luhn_check("5500000000000004"));
        assert!(luhn_check("378282246310005"));
    }

    #[test]
    fn test_luhn_invalid() {
        assert!(!luhn_check("4111111111111112"));
        assert!(!luhn_check("1234567890123456"));
    }

    #[test]
    fn test_phone_number_not_credit_card() {
        // Phone numbers should not be flagged as credit cards
        let findings = scan_text("Call me at 555-123-4567", "body");
        let cc_findings: Vec<_> = findings
            .iter()
            .filter(|f| f.finding_type == FindingType::CreditCard)
            .collect();
        assert!(cc_findings.is_empty());
    }

    // ---- SSN detection ----

    #[test]
    fn test_detect_ssn_with_dashes() {
        let findings = scan_text("SSN: 219-09-9999", "body");
        let ssn_findings: Vec<_> = findings
            .iter()
            .filter(|f| f.finding_type == FindingType::Ssn)
            .collect();
        assert_eq!(ssn_findings.len(), 1);
    }

    #[test]
    fn test_detect_ssn_without_dashes() {
        let findings = scan_text("SSN: 219099999", "body");
        let ssn_findings: Vec<_> = findings
            .iter()
            .filter(|f| f.finding_type == FindingType::Ssn)
            .collect();
        assert_eq!(ssn_findings.len(), 1);
    }

    #[test]
    fn test_random_9_digit_not_ssn() {
        // All same digits -> should not be flagged
        let findings = scan_text("ID: 111111111", "body");
        let ssn_findings: Vec<_> = findings
            .iter()
            .filter(|f| f.finding_type == FindingType::Ssn)
            .collect();
        assert!(ssn_findings.is_empty());
    }

    // ---- API key detection ----

    #[test]
    fn test_detect_stripe_secret_key() {
        let findings = scan_text(
            "api_key = sk-abc123def456ghi789jkl012mno345pqr678",
            "body",
        );
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].finding_type, FindingType::ApiKey);
    }

    #[test]
    fn test_detect_aws_access_key() {
        let findings = scan_text("key: AKIAIOSFODNN7EXAMPLE", "body");
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].finding_type, FindingType::ApiKey);
    }

    #[test]
    fn test_detect_github_token() {
        let findings =
            scan_text("token: ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefgh", "body");
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].finding_type, FindingType::ApiKey);
    }

    #[test]
    fn test_detect_gitlab_token() {
        let findings = scan_text("token: glpat-abcdefghijklmnopqrstu", "body");
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].finding_type, FindingType::ApiKey);
    }

    #[test]
    fn test_detect_slack_token() {
        let findings =
            scan_text("SLACK_TOKEN=xoxb-123456789012-abcdefghij", "body");
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].finding_type, FindingType::ApiKey);
    }

    #[test]
    fn test_detect_jwt_bearer() {
        let findings = scan_text(
            "Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.abc.def",
            "body",
        );
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].finding_type, FindingType::ApiKey);
    }

    #[test]
    fn test_detect_stripe_publishable_key() {
        let findings = scan_text(
            "pk_live_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghij",
            "body",
        );
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].finding_type, FindingType::ApiKey);
    }

    // ---- Password detection ----

    #[test]
    fn test_detect_password() {
        let findings = scan_text("password: mysecretpass123", "body");
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].finding_type, FindingType::Password);
    }

    #[test]
    fn test_detect_pwd() {
        let findings = scan_text("pwd=SuperSecret!", "body");
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].finding_type, FindingType::Password);
    }

    // ---- Private key detection ----

    #[test]
    fn test_detect_rsa_private_key() {
        let findings =
            scan_text("-----BEGIN RSA PRIVATE KEY-----", "body");
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].finding_type, FindingType::PrivateKey);
    }

    #[test]
    fn test_detect_ec_private_key() {
        let findings =
            scan_text("-----BEGIN EC PRIVATE KEY-----", "body");
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].finding_type, FindingType::PrivateKey);
    }

    #[test]
    fn test_detect_generic_private_key() {
        let findings =
            scan_text("-----BEGIN PRIVATE KEY-----", "body");
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].finding_type, FindingType::PrivateKey);
    }

    // ---- Bank account detection ----

    #[test]
    fn test_detect_iban() {
        let findings =
            scan_text("IBAN: GB29NWBK60161331926819", "body");
        let bank_findings: Vec<_> = findings
            .iter()
            .filter(|f| f.finding_type == FindingType::BankAccount)
            .collect();
        assert_eq!(bank_findings.len(), 1);
    }

    #[test]
    fn test_detect_us_routing_with_context() {
        let findings = scan_text("Routing number: 021000021", "body");
        let bank_findings: Vec<_> = findings
            .iter()
            .filter(|f| f.finding_type == FindingType::BankAccount)
            .collect();
        assert_eq!(bank_findings.len(), 1);
    }

    #[test]
    fn test_random_9_digit_not_routing() {
        // Without banking context, should not flag
        let findings = scan_text("Order ID: 012345678", "body");
        let bank_findings: Vec<_> = findings
            .iter()
            .filter(|f| f.finding_type == FindingType::BankAccount)
            .collect();
        assert!(bank_findings.is_empty());
    }

    // ---- Masking ----

    #[test]
    fn test_mask_long_value() {
        let masked = mask_value("4111111111111111");
        assert_eq!(masked, "4111********1111");
    }

    #[test]
    fn test_mask_short_value() {
        let masked = mask_value("12345");
        assert_eq!(masked, "*****");
    }

    // ---- HTML stripping ----

    #[test]
    fn test_strip_html_tags() {
        let text = strip_html("<p>Hello <b>world</b></p>");
        assert_eq!(text, "Hello world");
    }

    #[test]
    fn test_strip_html_preserves_text_with_card() {
        let html = "<div>Card: 4111111111111111</div>";
        let text = strip_html(html);
        assert!(text.contains("4111111111111111"));
    }

    // ---- Multiple findings ----

    #[test]
    fn test_multiple_findings() {
        let text = "Card: 4111111111111111\nSSN: 219-09-9999\npassword: secret123";
        let findings = scan_text(text, "body");
        assert!(findings.len() >= 3);
        let types: Vec<_> = findings.iter().map(|f| &f.finding_type).collect();
        assert!(types.contains(&&FindingType::CreditCard));
        assert!(types.contains(&&FindingType::Ssn));
        assert!(types.contains(&&FindingType::Password));
    }

    // ---- Empty body/subject ----

    #[test]
    fn test_empty_body() {
        let result = full_scan("", "");
        assert!(result.findings.is_empty());
        assert_eq!(result.risk_level, RiskLevel::None);
        assert!(result.allow_send);
    }

    // ---- Risk levels ----

    #[test]
    fn test_risk_level_none() {
        let result = full_scan("Hello", "Just a normal email");
        assert_eq!(result.risk_level, RiskLevel::None);
        assert!(result.allow_send);
    }

    #[test]
    fn test_risk_level_high_for_credit_card() {
        let result = full_scan("", "Card: 4111111111111111");
        assert_eq!(result.risk_level, RiskLevel::High);
        assert!(!result.allow_send);
    }

    #[test]
    fn test_risk_level_low_for_bank_iban() {
        // IBAN alone produces Low risk (bank_account finding is Low category)
        let result = full_scan("", "Wire to IBAN: GB29NWBK60161331926819");
        assert_eq!(result.risk_level, RiskLevel::Low);
        assert!(!result.allow_send);
    }

    #[test]
    fn test_routing_with_ssn_overlap_is_high() {
        // A 9-digit routing number also matches SSN pattern, so risk is High
        let result = full_scan("", "My routing number is 021000021");
        assert_eq!(result.risk_level, RiskLevel::High);
        let has_bank = result.findings.iter().any(|f| f.finding_type == FindingType::BankAccount);
        assert!(has_bank);
    }

    #[test]
    fn test_risk_level_high_for_private_key() {
        let result = full_scan("", "-----BEGIN RSA PRIVATE KEY-----");
        assert_eq!(result.risk_level, RiskLevel::High);
        assert!(!result.allow_send);
    }

    // ---- Subject scanning ----

    #[test]
    fn test_scan_subject() {
        let result = full_scan("password: secret123", "Nothing here");
        assert_eq!(result.findings.len(), 1);
        assert_eq!(result.findings[0].location, "subject");
    }

    // ---- Full scan with HTML ----

    #[test]
    fn test_full_scan_strips_html() {
        let result = full_scan("Hello", "<p>Card: 4111111111111111</p>");
        assert_eq!(result.findings.len(), 1);
        assert_eq!(result.findings[0].finding_type, FindingType::CreditCard);
        assert_eq!(result.findings[0].location, "body");
    }

    // ---- Override token ----

    #[test]
    fn test_override_token_single_use() {
        let token = Uuid::new_v4().to_string();
        {
            let mut tokens = OVERRIDE_TOKENS.lock().unwrap();
            tokens.insert(token.clone(), true);
        }
        assert!(validate_override_token(&token));
        // Second use should fail
        assert!(!validate_override_token(&token));
    }
}

//! Step 1 of specification 1, section 7.1: the headers that the store and
//! the rules need. Mail text is data: this code only reads values.

/// The most bytes of the header block that the parser reads (the same limit
/// as `iris-clean`).
const HEAD_LIMIT: usize = 256 * 1024;
/// The most addresses of one `To` or `Cc` header.
const MAX_ADDRS: usize = 200;
/// The authserv-id of the `Authentication-Results` header that Gmail adds.
pub const GMAIL_AUTHSERV: &str = "mx.google.com";

/// The parsed headers of one message.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Head {
    /// The bare `From` address, lowercase.
    pub from_addr: Option<String>,
    /// The `From` display name.
    pub from_name: Option<String>,
    /// Bare `To` addresses, lowercase.
    pub to: Vec<String>,
    /// Bare `Cc` addresses, lowercase.
    pub cc: Vec<String>,
    /// The `Date` header in milliseconds since the Unix epoch.
    pub date: Option<i64>,
    pub message_id: Option<String>,
    pub in_reply_to: Option<String>,
    pub list_id: Option<String>,
    pub list_unsubscribe: bool,
    /// `Precedence: bulk` or `list`, or `Auto-Submitted` other than `no`.
    pub automated: bool,
    /// The results of the top `Authentication-Results` header of Gmail.
    pub auth: Option<Auth>,
    /// A `text/calendar` part with the method `REQUEST`.
    pub invitation: bool,
}

/// SPF, DKIM and DMARC results, lowercase (`pass`, `fail`, ...).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Auth {
    pub spf: Option<String>,
    /// `(domain, result)` for each DKIM signature.
    pub dkim: Vec<(String, String)>,
    pub dmarc: Option<String>,
    /// The `header.from` domain of the DMARC result.
    pub dmarc_from: Option<String>,
}

impl Auth {
    /// The JSON text of the `header.auth` column.
    pub fn json(&self) -> String {
        serde_json::json!({ "spf": self.spf, "dkim": self.dkim, "dmarc": self.dmarc }).to_string()
    }

    /// True if a DKIM signature of `domain` passes.
    pub fn dkim_pass(&self, domain: &str) -> bool {
        self.dkim
            .iter()
            .any(|(d, r)| r == "pass" && d.eq_ignore_ascii_case(domain))
    }
}

/// The domain of an address, lowercase.
pub fn domain(addr: &str) -> &str {
    addr.rsplit_once('@').map_or("", |(_, d)| d)
}

impl Head {
    pub fn parse(raw: &[u8]) -> Head {
        let fields = fields(raw);
        let get = |name: &str| {
            fields
                .iter()
                .find(|(n, _)| n.eq_ignore_ascii_case(name))
                .map(|(_, v)| v.trim())
        };
        let (from_addr, from_name) = match get("from").map(addresses) {
            Some(mut list) if !list.is_empty() => {
                let (a, n) = list.swap_remove(0);
                (Some(a), n)
            }
            _ => (None, None),
        };
        let addrs = |name| {
            get(name)
                .map(|v| addresses(v).into_iter().map(|(a, _)| a).collect())
                .unwrap_or_default()
        };
        let auto = get("auto-submitted").is_some_and(|v| !v.eq_ignore_ascii_case("no"));
        let bulk = get("precedence")
            .is_some_and(|v| v.eq_ignore_ascii_case("bulk") || v.eq_ignore_ascii_case("list"));
        // Gmail adds its header at the top. A sender can add more below it.
        let auth = fields
            .iter()
            .filter(|(n, _)| n.eq_ignore_ascii_case("authentication-results"))
            .find_map(|(_, v)| auth(v));
        let nonempty = |v: Option<&str>| v.filter(|v| !v.is_empty()).map(str::to_string);
        Head {
            from_addr,
            from_name,
            to: addrs("to"),
            cc: addrs("cc"),
            date: get("date").and_then(date),
            message_id: nonempty(get("message-id")),
            in_reply_to: nonempty(get("in-reply-to")),
            list_id: nonempty(get("list-id")),
            list_unsubscribe: get("list-unsubscribe").is_some(),
            automated: auto || bulk,
            auth,
            invitation: invitation(raw),
        }
    }
}

/// The unfolded header fields of the top entity: `(name, value)`, in order.
fn fields(raw: &[u8]) -> Vec<(String, String)> {
    let head = &raw[..raw.len().min(HEAD_LIMIT)];
    let text = String::from_utf8_lossy(head);
    let mut out: Vec<(String, String)> = Vec::new();
    for line in text.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.is_empty() {
            break;
        }
        if line.starts_with([' ', '\t']) {
            if let Some((_, v)) = out.last_mut() {
                v.push(' ');
                v.push_str(line.trim());
            }
        } else if let Some((n, v)) = line.split_once(':') {
            out.push((n.trim().to_string(), v.trim().to_string()));
        }
    }
    out
}

/// The addresses of an address list: `(bare address lowercase, display name)`.
/// Commas inside quotes, angle brackets and comments do not split.
pub fn addresses(v: &str) -> Vec<(String, Option<String>)> {
    let mut parts = Vec::new();
    let (mut quote, mut angle, mut paren, mut start) = (false, false, 0u32, 0);
    let b = v.as_bytes();
    for (i, &c) in b.iter().enumerate() {
        match c {
            b'"' if paren == 0 && (i == 0 || b[i - 1] != b'\\') => quote = !quote,
            b'<' if !quote && paren == 0 => angle = true,
            b'>' if !quote && paren == 0 => angle = false,
            b'(' if !quote => paren += 1,
            b')' if !quote => paren = paren.saturating_sub(1),
            b',' if !quote && !angle && paren == 0 => {
                parts.push(&v[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&v[start..]);
    parts
        .into_iter()
        .filter_map(one_address)
        .take(MAX_ADDRS)
        .collect()
}

/// Decode RFC 2047 encoded words of a display name with the decoder of
/// `iris-clean`, which decodes the `Subject` of a message.
fn decode_name(n: &str) -> String {
    if !n.contains("=?") {
        return n.trim().to_string();
    }
    let one = n.replace(['\r', '\n'], " ");
    iris_clean::clean(format!("Subject: {one}\r\n\r\n").as_bytes()).subject
}

fn one_address(p: &str) -> Option<(String, Option<String>)> {
    let p = p.trim();
    let (addr, name) = match (p.rfind('<'), p.rfind('>')) {
        (Some(l), Some(r)) if l < r => (&p[l + 1..r], Some(p[..l].trim())),
        _ => (p.split('(').next().unwrap_or(p), None),
    };
    let addr = addr.trim().to_ascii_lowercase();
    if !addr.contains('@') || addr.contains(char::is_whitespace) {
        return None;
    }
    let name = name
        .map(|n| decode_name(&n.trim_matches('"').replace("\\\"", "\"")))
        .filter(|n| !n.is_empty());
    Some((addr, name))
}

/// The results of one `Authentication-Results` value, if its authserv-id is
/// the id of Gmail.
fn auth(v: &str) -> Option<Auth> {
    let v = strip_comments(v);
    let mut parts = v.split(';');
    let id = parts.next()?.split_whitespace().next()?;
    if !id.eq_ignore_ascii_case(GMAIL_AUTHSERV) {
        return None;
    }
    let mut a = Auth::default();
    for part in parts {
        let mut words = part.split_whitespace();
        let Some((method, result)) = words.next().and_then(|w| w.split_once('=')) else {
            continue;
        };
        let result = result.to_ascii_lowercase();
        let prop = |names: &[&str]| {
            part.split_whitespace().find_map(|w| {
                let (k, v) = w.split_once('=')?;
                names
                    .iter()
                    .any(|n| k.eq_ignore_ascii_case(n))
                    .then(|| domain_of(v))
            })
        };
        match method.to_ascii_lowercase().as_str() {
            "spf" => a.spf = Some(result),
            "dkim" => {
                if let Some(d) = prop(&["header.d", "header.i"]) {
                    a.dkim.push((d, result));
                }
            }
            "dmarc" => {
                a.dmarc = Some(result);
                a.dmarc_from = prop(&["header.from"]);
            }
            _ => {}
        }
    }
    Some(a)
}

/// `@example.com`, `a@example.com` or `example.com` to `example.com`.
fn domain_of(v: &str) -> String {
    v.rsplit('@')
        .next()
        .unwrap_or(v)
        .trim_matches('"')
        .to_ascii_lowercase()
}

fn strip_comments(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    let mut depth = 0u32;
    for c in v.chars() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

/// True if the message has a `text/calendar` MIME entity with the method
/// `REQUEST`: a `method=REQUEST` parameter of its `Content-Type`, or a
/// `METHOD:REQUEST` line in its body. Only the boundaries that a
/// `Content-Type` header declares start a new entity, so calendar words in
/// a plain-text body do not count.
// shortcut: a base64 calendar body with no method parameter is not found;
// the model still sees the attachment name and type.
fn invitation(raw: &[u8]) -> bool {
    let mut boundaries: Vec<Vec<u8>> = Vec::new();
    let (mut in_head, mut cal) = (true, false);
    // The Content-Type value of the header block that the loop reads now.
    let mut ct: Option<Vec<u8>> = None;
    let end_header = |ct: &mut Option<Vec<u8>>, cal: &mut bool, b: &mut Vec<Vec<u8>>| -> bool {
        let Some(v) = ct.take() else { return false };
        let low = v.to_ascii_lowercase();
        if let Some(bd) = param(&low, &v, b"boundary=") {
            if b.len() < 50 {
                b.push(bd);
            }
        }
        *cal = contains(&low, b"text/calendar");
        *cal && param(&low, &v, b"method=").is_some_and(|m| m.eq_ignore_ascii_case(b"request"))
    };
    for line in raw.split(|&c| c == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let is_boundary = line.starts_with(b"--")
            && boundaries.iter().any(|bd| {
                let rest = &line[2..];
                rest.starts_with(bd) && matches!(&rest[bd.len()..], b"" | b"--")
            });
        if is_boundary {
            if end_header(&mut ct, &mut cal, &mut boundaries) {
                return true;
            }
            (in_head, cal) = (true, false);
            continue;
        }
        if in_head {
            if line.is_empty() {
                in_head = false;
                if end_header(&mut ct, &mut cal, &mut boundaries) {
                    return true;
                }
            } else if line.starts_with(b" ") || line.starts_with(b"\t") {
                if let Some(v) = ct.as_mut() {
                    v.extend_from_slice(line);
                }
            } else {
                if end_header(&mut ct, &mut cal, &mut boundaries) {
                    return true;
                }
                if line.len() > 13 && line[..13].eq_ignore_ascii_case(b"content-type:") {
                    ct = Some(line[13..].to_vec());
                }
            }
        } else if cal && line.trim_ascii().eq_ignore_ascii_case(b"METHOD:REQUEST") {
            return true;
        }
    }
    false
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

/// The value of the parameter `name` (lowercase, with `=`) in a header
/// value, with its case kept. `low` is the value in lowercase.
fn param(low: &[u8], v: &[u8], name: &[u8]) -> Option<Vec<u8>> {
    let at = low.windows(name.len()).position(|w| w == name)? + name.len();
    let rest = &v[at..];
    let val = match rest.strip_prefix(b"\"") {
        Some(q) => &q[..q.iter().position(|&c| c == b'"').unwrap_or(q.len())],
        None => {
            &rest[..rest
                .iter()
                .position(|&c| c == b';' || c.is_ascii_whitespace())
                .unwrap_or(rest.len())]
        }
    };
    (!val.is_empty()).then(|| val.to_vec())
}

const MONTHS: [&str; 12] = [
    "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
];

/// An RFC 5322 date, for example `Thu, 25 Jan 2024 06:26:35 +0000`, in
/// milliseconds since the Unix epoch.
pub fn date(v: &str) -> Option<i64> {
    let v = strip_comments(v);
    let v = v.split_once(',').map_or(v.as_str(), |(_, r)| r);
    let mut w = v.split_whitespace();
    let day: i64 = w.next()?.parse().ok()?;
    let mon = w.next()?.to_ascii_lowercase();
    let month = MONTHS.iter().position(|m| mon.starts_with(m))? as i64 + 1;
    let mut year: i64 = w.next()?.parse().ok()?;
    if year < 100 {
        year += if year < 50 { 2000 } else { 1900 };
    }
    // The header is untrusted: a huge year would overflow the arithmetic.
    if !(1..=9999).contains(&year) {
        return None;
    }
    let mut hms = w.next()?.split(':').map(|x| x.parse::<i64>().ok());
    let (h, m) = (hms.next()??, hms.next()??);
    let s = hms.next().flatten().unwrap_or(0);
    let zone = w.next().unwrap_or("+0000");
    let off = match zone.as_bytes().first() {
        Some(b'+' | b'-') if zone.len() == 5 => {
            let n: i64 = zone[1..].parse().ok()?;
            let mins = n / 100 * 60 + n % 100;
            if zone.starts_with('-') {
                -mins
            } else {
                mins
            }
        }
        _ => 0,
    };
    if !(1..=31).contains(&day) || h > 23 || m > 59 || s > 60 {
        return None;
    }
    let days = crate::time::days_from_civil(year, month, day);
    Some(((days * 86_400 + h * 3600 + m * 60 + s) - off * 60) * 1000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_split_and_lowercase() {
        let a =
            addresses(r#""Lee, Ana" <Ana@Example.COM>, bob@example.org (Bob, B), <c@x.example>"#);
        assert_eq!(
            a,
            vec![
                ("ana@example.com".into(), Some("Lee, Ana".into())),
                ("bob@example.org".into(), None),
                ("c@x.example".into(), None),
            ]
        );
        assert!(addresses("undisclosed-recipients:;").is_empty());
    }

    #[test]
    fn only_the_top_gmail_auth_header_counts() {
        let raw = b"Authentication-Results: other.example; dmarc=pass\r\n\
Authentication-Results: mx.google.com;\r\n dkim=pass header.i=@Bank.example (comment; x);\r\n spf=pass smtp.mailfrom=a@bank.example;\r\n dmarc=fail (p=REJECT) header.from=bank.example\r\n\
Authentication-Results: mx.google.com; dmarc=pass header.from=bank.example\r\n\
From: a@bank.example\r\n\r\nbody";
        let a = Head::parse(raw).auth.unwrap();
        assert_eq!(a.dmarc.as_deref(), Some("fail"));
        assert_eq!(a.dmarc_from.as_deref(), Some("bank.example"));
        assert!(a.dkim_pass("bank.example"));
        assert_eq!(a.spf.as_deref(), Some("pass"));
    }

    #[test]
    fn dates() {
        assert_eq!(date("Thu, 01 Jan 1970 00:00:00 +0000"), Some(0));
        assert_eq!(date("2 Jan 1970 01:00 +0100"), Some(86_400_000));
        assert_eq!(
            date("Fri, 30 Aug 2024 19:09:01 -0700 (PDT)"),
            Some(1_725_070_141_000)
        );
        assert_eq!(date("not a date"), None);
    }

    #[test]
    fn invitation_needs_calendar_and_request() {
        assert!(invitation(
            b"Content-Type: text/calendar; method=REQUEST\r\n"
        ));
        assert!(!invitation(
            b"Content-Type: text/calendar; method=CANCEL\r\n"
        ));
        assert!(!invitation(b"METHOD:REQUEST in plain text"));
        assert!(!invitation(
            b"Subject: x\r\n\r\nContent-Type: text/calendar; method=REQUEST\r\nMETHOD:REQUEST\r\n"
        ));
        let mixed = b"Content-Type: multipart/mixed; boundary=\"b\"\r\n\r\n--b\r\n\
Content-Type: text/plain\r\n\r\nMETHOD:REQUEST\r\n--b\r\n\
Content-Type: text/calendar;\r\n charset=utf-8\r\n\r\nBEGIN:VCALENDAR\r\nMETHOD:CANCEL\r\n--b--\r\n";
        assert!(!invitation(mixed));
        let ok = String::from_utf8_lossy(mixed).replace("METHOD:CANCEL", "METHOD:REQUEST");
        assert!(invitation(ok.as_bytes()));
        assert_eq!(date("1 Jan 9223372036854775807 00:00 +0000"), None);
    }
}

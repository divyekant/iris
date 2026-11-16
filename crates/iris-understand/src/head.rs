//! Step 1 of specification 1, section 7.1: the headers that the store and
//! the rules need. Mail text is data: this code only reads values.

/// The most bytes of the header block that the parser reads (the same limit
/// as `iris-clean`).
const HEAD_LIMIT: usize = 256 * 1024;
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
    /// `List-Id`, `List-Unsubscribe`, `Precedence: bulk`, or
    /// `Auto-Submitted` other than `no` (the rule of `iris-senders`).
    pub automated: bool,
    /// The results of the top `Authentication-Results` header of Gmail.
    pub auth: Option<Auth>,
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

    /// True if a DKIM signature of `domain` or of a parent domain of it
    /// passes (relaxed alignment, the rule of `iris-senders`).
    pub fn dkim_pass(&self, domain: &str) -> bool {
        let domain = domain.to_ascii_lowercase();
        self.dkim.iter().any(|(d, r)| {
            let d = d.to_ascii_lowercase();
            r == "pass" && !d.is_empty() && (domain == d || domain.ends_with(&format!(".{d}")))
        })
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
            // The same rule as the sender kinds of `iris-senders`.
            automated: iris_senders::head::Head::parse(raw).automated(),
            auth,
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
    // A value can start on the next line (`Name:` then ` value`).
    for (_, v) in &mut out {
        if v.starts_with(' ') {
            v.remove(0);
        }
    }
    out
}

/// The addresses of an address list: `(bare address lowercase, display name)`,
/// with the parser of `iris-senders` (groups, quotes, comments), and the
/// display name decoded (RFC 2047).
pub fn addresses(v: &str) -> Vec<(String, Option<String>)> {
    iris_senders::head::addresses(v)
        .into_iter()
        .filter(|(a, _)| !a.contains(char::is_whitespace))
        // `a@b.example (Ana Lee)`: the comment shows as the display name.
        .map(|(a, n)| {
            let n = n.or_else(|| comment_name(v, &a));
            (a, n)
        })
        .map(|(a, n)| {
            (
                a,
                n.map(|n| decode_name(&n.replace("\\\"", "\"")))
                    .filter(|n| !n.is_empty()),
            )
        })
        .collect()
}

/// The comment right after a bare address in `v`, as in
/// `ana@example.org (Ana Lee)`.
fn comment_name(v: &str, addr: &str) -> Option<String> {
    let low = v.to_ascii_lowercase();
    let at = low.find(addr)? + addr.len();
    let rest = v[at..].trim_start();
    let inner = rest.strip_prefix('(')?;
    let end = inner.find(')')?;
    Some(inner[..end].trim().to_string()).filter(|n| !n.is_empty())
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
        Some(b'+' | b'-') if zone.len() == 5 && zone[1..].bytes().all(|c| c.is_ascii_digit()) => {
            let n: i64 = zone[1..].parse().ok()?;
            if n / 100 > 23 || n % 100 > 59 {
                return None;
            }
            let mins = n / 100 * 60 + n % 100;
            if zone.starts_with('-') {
                -mins
            } else {
                mins
            }
        }
        // The zone names of RFC 5322 section 4.3. Another name gives no date.
        _ => match zone.to_ascii_uppercase().as_str() {
            "UT" | "GMT" | "Z" => 0,
            "EDT" => -4 * 60,
            "EST" | "CDT" => -5 * 60,
            "CST" | "MDT" => -6 * 60,
            "MST" | "PDT" => -7 * 60,
            "PST" => -8 * 60,
            _ => return None,
        },
    };
    if !crate::time::valid(year, month, day, h, m, s) {
        return None;
    }
    let days = crate::time::days_from_civil(year, month, day);
    Some(((days * 86_400 + h * 3600 + m * 60 + s) - off * 60) * 1000)
}

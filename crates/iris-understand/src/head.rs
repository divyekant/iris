//! Step 1 of specification 1, section 7.1: the headers that the store and
//! the rules need. Mail text is data: this code only reads values.

/// The most bytes of the header block that the parser reads (the same limit
/// as `iris-clean`).
pub const HEAD_LIMIT: usize = 256 * 1024;
pub use iris_senders::head::{trusted_authserv, GMAIL_AUTHSERV};

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
    /// Bare `Bcc` addresses, lowercase. Only sent mail keeps this header.
    pub bcc: Vec<String>,
    /// The `Date` header in milliseconds since the Unix epoch.
    pub date: Option<i64>,
    pub message_id: Option<String>,
    pub in_reply_to: Option<String>,
    pub list_id: Option<String>,
    pub list_unsubscribe: bool,
    /// `List-Id`, `List-Unsubscribe`, `Precedence: bulk`, or
    /// `Auto-Submitted` other than `no` (the rule of `iris-senders`).
    pub automated: bool,
    /// The results of the top `Authentication-Results` header of the
    /// provider (the `authserv` of [`Head::parse`]).
    pub auth: Option<Auth>,
    /// True when the header block does not end in the first [`HEAD_LIMIT`]
    /// bytes, so a field past the limit (as `From`) is not read (issue
    /// #1033).
    pub cut: bool,
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
        let domain = ascii_domain(domain);
        self.dkim.iter().any(|(d, r)| {
            let d = ascii_domain(d);
            r == "pass" && !d.is_empty() && (domain == d || domain.ends_with(&format!(".{d}")))
        })
    }
}

/// One form for each name of a domain: lowercase punycode, as a browser
/// reads it. A header can name `münchen.de` and an authentication result
/// `xn--mnchen-3ya.de`; both give `xn--mnchen-3ya.de`. A name that is not a
/// valid domain stays as it is, in lowercase.
pub fn ascii_domain(d: &str) -> String {
    match url::Host::parse(d) {
        Ok(url::Host::Domain(a)) => a,
        _ => d.to_lowercase(),
    }
}

/// The domain of an address, lowercase.
pub fn domain(addr: &str) -> &str {
    addr.rsplit_once('@').map_or("", |(_, d)| d)
}

impl Head {
    /// Parse the headers of `raw`. `authserv` is the authserv-id that the
    /// provider of the account adds ([`trusted_authserv`]). With `None`, no
    /// `Authentication-Results` header counts.
    pub fn parse(raw: &[u8], authserv: Option<&str>) -> Head {
        let (fields, ended) = fields(raw);
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
        let auth = authserv.and_then(|id| {
            fields
                .iter()
                .filter(|(n, _)| n.eq_ignore_ascii_case("authentication-results"))
                .find_map(|(_, v)| auth(v, id))
        });
        let nonempty = |v: Option<&str>| v.filter(|v| !v.is_empty()).map(str::to_string);
        Head {
            from_addr,
            from_name,
            to: addrs("to"),
            cc: addrs("cc"),
            bcc: addrs("bcc"),
            date: get("date").and_then(date),
            message_id: nonempty(get("message-id")),
            in_reply_to: nonempty(get("in-reply-to")),
            list_id: nonempty(get("list-id")),
            list_unsubscribe: get("list-unsubscribe").is_some(),
            // The same rule as the sender kinds of `iris-senders`.
            automated: iris_senders::head::Head::parse(raw).automated(),
            auth,
            cut: !ended && raw.len() > HEAD_LIMIT,
        }
    }
}

/// The unfolded header fields of the top entity: `(name, value)`, in order,
/// and true if the empty line that ends the header block is in the first
/// [`HEAD_LIMIT`] bytes.
fn fields(raw: &[u8]) -> (Vec<(String, String)>, bool) {
    let head = &raw[..raw.len().min(HEAD_LIMIT)];
    let text = String::from_utf8_lossy(head);
    let mut out: Vec<(String, String)> = Vec::new();
    let mut ended = false;
    let mut lines = text.split('\n').peekable();
    while let Some(line) = lines.next() {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.is_empty() {
            // The empty line ends the block only with its own line break in
            // the limit: a last piece with no break can be a cut line.
            ended = lines.peek().is_some();
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
    (out, ended)
}

/// The addresses of an address list: `(bare address lowercase, display name)`,
/// with the parser of `iris-senders` (groups, quotes, comments, a comment
/// name, the display name decoded with RFC 2047).
pub fn addresses(v: &str) -> Vec<(String, Option<String>)> {
    iris_senders::head::addresses(v)
        .into_iter()
        // White space is valid only inside a quoted local part.
        .filter(|(a, _)| {
            a.rsplit_once('@').is_some_and(|(local, domain)| {
                let quoted = local.len() > 1 && local.starts_with('"') && local.ends_with('"');
                (quoted || !local.contains(char::is_whitespace))
                    && !domain.contains(char::is_whitespace)
            })
        })
        .collect()
}

/// The results of one `Authentication-Results` value, if its authserv-id is
/// `authserv`.
fn auth(v: &str, authserv: &str) -> Option<Auth> {
    let v = strip_comments(v);
    let mut parts = v.split(';');
    let id = parts.next()?.split_whitespace().next()?;
    if !id.eq_ignore_ascii_case(authserv) {
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

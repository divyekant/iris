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

/// True if the message has a `text/calendar` MIME entity with the method
/// `REQUEST`: a `method=REQUEST` parameter of its `Content-Type`, or a
/// `METHOD:REQUEST` line in its decoded body (plain, base64 or
/// quoted-printable). Only the boundaries that a `Content-Type` header
/// declares start a new entity, so calendar words in a plain-text body do
/// not count. Each header block is read whole (at most 256 KiB) and
/// unfolded with [`fields`], the same as the top header block.
fn invitation(raw: &[u8]) -> bool {
    let mut boundaries: Vec<Vec<u8>> = Vec::new();
    let mut in_head = true;
    // The header block of the entity that the loop reads now.
    let mut block: Vec<u8> = Vec::new();
    // The body rule of the entity, from its header block.
    let mut body = Enc::Other;
    // The encoded text of a calendar body (base64 or quoted-printable, with
    // the soft line breaks joined), at most CALENDAR_B64 bytes.
    let mut buf: Vec<u8> = Vec::new();
    for line in raw.split(|&c| c == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let is_boundary = line.starts_with(b"--")
            && boundaries.iter().any(|bd| {
                let rest = &line[2..];
                rest.starts_with(bd) && matches!(&rest[bd.len()..], b"" | b"--")
            });
        if is_boundary {
            if in_head && end_block(&block, &mut boundaries, &mut body) || request(&buf, body) {
                return true;
            }
            (in_head, body) = (true, Enc::Other);
            block.clear();
            buf.clear();
            continue;
        }
        if in_head {
            if line.is_empty() {
                in_head = false;
                if end_block(&block, &mut boundaries, &mut body) {
                    return true;
                }
            } else if block.len() < HEAD_LIMIT {
                // A long line is cut, not dropped, so its field name stays.
                let room = HEAD_LIMIT - block.len() - 1;
                block.extend_from_slice(&line[..line.len().min(room)]);
                block.push(b'\n');
            }
            continue;
        }
        match body {
            Enc::Base64 if buf.len() + line.len() <= CALENDAR_B64 => {
                buf.extend_from_slice(line.trim_ascii());
            }
            Enc::Qp => {
                // A line that ends with `=` goes on in the next line.
                let (part, soft) = match line.strip_suffix(b"=") {
                    Some(p) => (p, true),
                    None => (line, false),
                };
                if buf.len() + part.len() < CALENDAR_B64 {
                    buf.extend_from_slice(part);
                    if !soft {
                        buf.push(b'\n');
                    }
                }
            }
            Enc::Plain if line.trim_ascii().eq_ignore_ascii_case(b"METHOD:REQUEST") => {
                return true;
            }
            _ => {}
        }
    }
    (in_head && end_block(&block, &mut boundaries, &mut body)) || request(&buf, body)
}

/// How the scan reads the body of one entity.
#[derive(Clone, Copy, PartialEq)]
enum Enc {
    /// Not a calendar: the scan skips the body.
    Other,
    /// A calendar with no transfer encoding.
    Plain,
    Base64,
    Qp,
}

/// Read one header block: record its boundary, and set the body rule.
/// True if its `Content-Type` has `method=REQUEST`.
fn end_block(block: &[u8], boundaries: &mut Vec<Vec<u8>>, body: &mut Enc) -> bool {
    let f = fields(block);
    let get = |name: &str| {
        f.iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map_or("", |(_, v)| v.as_str())
    };
    let ct = get("content-type").as_bytes();
    let low = ct.to_ascii_lowercase();
    if let Some(bd) = param(&low, ct, b"boundary=") {
        if boundaries.len() < 50 {
            boundaries.push(bd);
        }
    }
    let cal = contains(&low, b"text/calendar");
    *body = match get("content-transfer-encoding")
        .to_ascii_lowercase()
        .as_str()
    {
        _ if !cal => Enc::Other,
        "base64" => Enc::Base64,
        "quoted-printable" => Enc::Qp,
        _ => Enc::Plain,
    };
    cal && param(&low, ct, b"method=").is_some_and(|m| m.eq_ignore_ascii_case(b"request"))
}

/// True if the decoded calendar text in `buf` has a `METHOD:REQUEST` line.
fn request(buf: &[u8], body: Enc) -> bool {
    let text = match body {
        Enc::Base64 => base64(buf),
        Enc::Qp => quoted_printable(buf),
        _ => return false,
    };
    text.split(|&c| c == b'\n')
        .any(|l| l.trim_ascii().eq_ignore_ascii_case(b"METHOD:REQUEST"))
}

/// The most encoded bytes of one calendar body that the scan decodes. The
/// METHOD line comes near the start of a calendar.
const CALENDAR_B64: usize = 64 * 1024;

/// Decode quoted-printable text with its soft line breaks already joined
/// (`=3A` to `:`).
fn quoted_printable(line: &[u8]) -> Vec<u8> {
    let hex = |c: Option<&u8>| c.and_then(|&c| (c as char).to_digit(16));
    let mut out = Vec::with_capacity(line.len());
    let mut i = 0;
    while i < line.len() {
        match (line[i], hex(line.get(i + 1)), hex(line.get(i + 2))) {
            (b'=', Some(h), Some(l)) => {
                out.push((h * 16 + l) as u8);
                i += 3;
            }
            (c, _, _) => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

/// Decode a base64 body. Line ends and other bytes that are not base64 go
/// out first. A body that does not decode gives nothing.
fn base64(src: &[u8]) -> Vec<u8> {
    use base64::engine::{general_purpose::GeneralPurpose, DecodePaddingMode};
    use base64::{alphabet, engine::GeneralPurposeConfig, Engine};
    const LAX: GeneralPurpose = GeneralPurpose::new(
        &alphabet::STANDARD,
        GeneralPurposeConfig::new()
            .with_decode_padding_mode(DecodePaddingMode::Indifferent)
            .with_decode_allow_trailing_bits(true),
    );
    let clean: Vec<u8> = src
        .iter()
        .copied()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, b'+' | b'/'))
        .collect();
    LAX.decode(clean).unwrap_or_default()
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

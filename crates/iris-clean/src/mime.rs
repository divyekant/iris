//! MIME structure: headers, parts, transfer encodings and charsets.
//!
//! The functions read only what they need. They do not decode the body of an
//! attachment to measure its size, so a big attachment costs no memory.

use std::borrow::Cow;

use encoding_rs::{Encoding, UTF_8, WINDOWS_1252};

/// The most header lines that the parser reads in one entity.
const MAX_HEADERS: usize = 1000;
/// The most bytes of a header block that the parser reads.
const HEAD_LIMIT: usize = 256 * 1024;
/// The most bytes of one header value.
const VALUE_LIMIT: usize = 16 * 1024;

/// Split an entity into its header block and its body at the first empty line.
pub fn split_head(raw: &[u8]) -> (&[u8], &[u8]) {
    for (skip, blank) in [(2, &b"\r\n"[..]), (1, &b"\n"[..])] {
        if raw.starts_with(blank) {
            return (&[], &raw[skip..]);
        }
    }
    let mut from = 0;
    while let Some(n) = raw[from..].iter().position(|&b| b == b'\n') {
        let next = from + n + 1;
        if raw[next..].starts_with(b"\r\n") {
            return (&raw[..next], &raw[next + 2..]);
        }
        if raw[next..].starts_with(b"\n") {
            return (&raw[..next], &raw[next + 1..]);
        }
        from = next;
    }
    (raw, &[])
}

/// The header fields of one entity, with lowercase names and unfolded values.
pub struct Headers(Vec<(String, String)>);

impl Headers {
    pub fn parse(head: &[u8]) -> Headers {
        let mut out: Vec<(String, String)> = Vec::new();
        let head = &head[..head.len().min(HEAD_LIMIT)];
        for line in head.split(|&b| b == b'\n') {
            let line = line.strip_suffix(b"\r").unwrap_or(line);
            let line = &line[..line.len().min(VALUE_LIMIT)];
            if line.is_empty() {
                continue;
            }
            if line[0] == b' ' || line[0] == b'\t' {
                if let Some(last) = out.last_mut() {
                    if last.1.len() < VALUE_LIMIT {
                        last.1.push_str(&decode(line, None));
                        let end = floor_char(&last.1, VALUE_LIMIT);
                        last.1.truncate(end);
                    }
                }
                continue;
            }
            if out.len() >= MAX_HEADERS {
                break;
            }
            if let Some(c) = line.iter().position(|&b| b == b':') {
                let name = String::from_utf8_lossy(&line[..c])
                    .trim()
                    .to_ascii_lowercase();
                let value = decode(&line[c + 1..], None).trim_start().to_string();
                out.push((name, value));
            }
        }
        Headers(out)
    }

    /// The first field with this lowercase name.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }
}

/// The largest char boundary at or below `i`.
pub fn floor_char(s: &str, i: usize) -> usize {
    (0..=i.min(s.len()))
        .rev()
        .find(|&k| s.is_char_boundary(k))
        .unwrap_or(0)
}

/// The index in a base64 body after `chars` characters of the alphabet.
pub fn base64_prefix(body: &[u8], chars: usize) -> usize {
    let mut n = 0;
    for (i, &b) in body.iter().enumerate() {
        if n >= chars {
            return i;
        }
        if b64(b).is_some() {
            n += 1;
        }
    }
    body.len()
}

/// A structured header value such as `text/plain; charset=utf-8`.
pub struct Value {
    /// The value before the first `;`, in lowercase.
    pub main: String,
    /// The parameters with lowercase names. Quotes are removed.
    pub params: Vec<(String, String)>,
}

pub fn parse_value(v: &str) -> Value {
    let mut segs = Vec::new();
    let mut cur = String::new();
    let (mut quoted, mut escape) = (false, false);
    for c in v.chars() {
        if escape {
            cur.push(c);
            escape = false;
            continue;
        }
        match c {
            '\\' if quoted => escape = true,
            '"' => quoted = !quoted,
            ';' if !quoted => segs.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    segs.push(cur);
    let main = segs[0].trim().to_ascii_lowercase();
    let params = segs[1..]
        .iter()
        .filter_map(|s| {
            let (k, v) = s.split_once('=')?;
            Some((k.trim().to_ascii_lowercase(), v.trim().to_string()))
        })
        .collect();
    Value { main, params }
}

/// The value of a parameter. Joins RFC 2231 sections (`name*0*=`) and decodes
/// RFC 2231 charsets and RFC 2047 encoded words.
pub fn param(params: &[(String, String)], name: &str) -> Option<String> {
    let mut plain = None;
    let mut sections: Vec<(u32, bool, &str)> = Vec::new();
    for (k, v) in params {
        if k == name {
            plain.get_or_insert_with(|| decode_words(v));
            continue;
        }
        let Some(rest) = k.strip_prefix(name).and_then(|r| r.strip_prefix('*')) else {
            continue;
        };
        let section = if rest.is_empty() {
            Some((0, true))
        } else if let Some(n) = rest.strip_suffix('*') {
            n.parse().ok().map(|n| (n, true))
        } else {
            rest.parse().ok().map(|n| (n, false))
        };
        if let Some((n, extended)) = section {
            sections.push((n, extended, v));
        }
    }
    if sections.is_empty() {
        return plain;
    }
    sections.sort_by_key(|s| s.0);
    let mut charset = None;
    let mut bytes = Vec::new();
    for (i, (_, extended, v)) in sections.iter().enumerate() {
        if !extended {
            bytes.extend_from_slice(v.as_bytes());
            continue;
        }
        let mut text = *v;
        if i == 0 {
            // charset'language'text
            let mut it = v.splitn(3, '\'');
            if let (Some(cs), Some(_), Some(t)) = (it.next(), it.next(), it.next()) {
                charset = Some(cs);
                text = t;
            }
        }
        bytes.extend(percent_decode(text.as_bytes()));
    }
    Some(decode(&bytes, charset))
}

fn percent_decode(b: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            if let (Some(h), Some(l)) = (b.get(i + 1).and_then(hex), b.get(i + 2).and_then(hex)) {
                out.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    out
}

fn hex(c: &u8) -> Option<u8> {
    (*c as char).to_digit(16).map(|d| d as u8)
}

/// Decode text bytes with the charset label of the message.
///
/// Many senders label UTF-8 text as `us-ascii` or `iso-8859-1`. If the label
/// is one of these, or there is no label, and the bytes are valid UTF-8, the
/// bytes are read as UTF-8. Bytes without a label that are not UTF-8 are read
/// as Windows-1252, the superset of Latin-1.
pub fn decode(bytes: &[u8], label: Option<&str>) -> String {
    let enc = label.and_then(|l| Encoding::for_label(l.trim().as_bytes()));
    if enc.is_none() || enc == Some(UTF_8) || enc == Some(WINDOWS_1252) {
        if let Ok(s) = std::str::from_utf8(bytes) {
            return s.to_string();
        }
    }
    enc.unwrap_or(WINDOWS_1252)
        .decode_without_bom_handling(bytes)
        .0
        .into_owned()
}

/// Decode the RFC 2047 encoded words (`=?charset?B?...?=`) of a header value.
pub fn decode_words(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    let mut pending: Option<(String, Vec<u8>)> = None;
    let mut last_was_word = false;
    while let Some((start, end, charset, bytes)) = next_word(rest) {
        let before = &rest[..start];
        // RFC 2047: white space between two encoded words is not text.
        let adjacent = last_was_word && before.trim().is_empty();
        match pending.as_mut() {
            // Join the bytes first, so a character split over two words decodes.
            Some((cs, buf)) if adjacent && cs.eq_ignore_ascii_case(&charset) => buf.extend(bytes),
            _ => {
                flush(&mut out, &mut pending);
                if !adjacent {
                    out.push_str(before);
                }
                pending = Some((charset, bytes));
            }
        }
        last_was_word = true;
        rest = &rest[end..];
    }
    flush(&mut out, &mut pending);
    out.push_str(rest);
    out
}

fn flush(out: &mut String, pending: &mut Option<(String, Vec<u8>)>) {
    if let Some((cs, buf)) = pending.take() {
        out.push_str(&decode(&buf, Some(&cs)));
    }
}

/// Find the next encoded word: (start, end, charset, decoded bytes).
fn next_word(s: &str) -> Option<(usize, usize, String, Vec<u8>)> {
    let mut from = 0;
    while let Some(i) = s[from..].find("=?") {
        let start = from + i;
        if let Some((len, charset, bytes)) = parse_word(&s.as_bytes()[start + 2..]) {
            return Some((start, start + 2 + len, charset, bytes));
        }
        from = start + 2;
    }
    None
}

/// Parse `charset?E?text?=` (the part after `=?`). Return the length, the
/// charset and the bytes. The search windows are short, so the time is linear.
fn parse_word(s: &[u8]) -> Option<(usize, String, Vec<u8>)> {
    let q1 = s.iter().take(64).position(|&b| b == b'?')?;
    // RFC 2231 adds a language after `*`.
    let charset = std::str::from_utf8(&s[..q1]).ok()?.split('*').next()?;
    if charset.is_empty() || !charset.bytes().all(|b| b.is_ascii_graphic()) {
        return None;
    }
    let enc = s.get(q1 + 1)?.to_ascii_uppercase();
    if s.get(q1 + 2) != Some(&b'?') {
        return None;
    }
    let t = q1 + 3;
    let window = &s[t.min(s.len())..s.len().min(t + 1000)];
    let len = window.windows(2).position(|w| w == b"?=")?;
    let text = &window[..len];
    if text.iter().any(u8::is_ascii_whitespace) {
        return None;
    }
    let bytes = match enc {
        b'B' => base64(text),
        b'Q' => {
            let under: Vec<u8> = text
                .iter()
                .map(|&b| if b == b'_' { b' ' } else { b })
                .collect();
            qp(&under)
        }
        _ => return None,
    };
    Some((t + len + 2, charset.to_string(), bytes))
}

/// Decode a body with its `Content-Transfer-Encoding`.
pub fn transfer_decode<'a>(body: &'a [u8], encoding: &str) -> Cow<'a, [u8]> {
    match encoding {
        "base64" => Cow::Owned(base64(body)),
        "quoted-printable" => Cow::Owned(qp(body)),
        _ => Cow::Borrowed(body),
    }
}

/// The decoded size of a body, without decoding it.
pub fn decoded_len(body: &[u8], encoding: &str) -> u64 {
    match encoding {
        "base64" => {
            let n = body
                .iter()
                .take_while(|&&b| b != b'=')
                .filter(|&&b| b64(b).is_some())
                .count() as u64;
            n * 3 / 4
        }
        "quoted-printable" => qp_walk(body, |_| {}),
        _ => body.len() as u64,
    }
}

fn b64(c: u8) -> Option<u8> {
    match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

/// Lenient base64: skip characters outside the alphabet, stop at `=`.
fn base64(b: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(b.len() / 4 * 3 + 3);
    let (mut acc, mut n) = (0u32, 0);
    for &c in b {
        if c == b'=' {
            break;
        }
        let Some(v) = b64(c) else { continue };
        acc = acc << 6 | v as u32;
        n += 1;
        if n == 4 {
            out.extend_from_slice(&[(acc >> 16) as u8, (acc >> 8) as u8, acc as u8]);
            (acc, n) = (0, 0);
        }
    }
    match n {
        2 => out.push((acc >> 4) as u8),
        3 => out.extend_from_slice(&[(acc >> 10) as u8, (acc >> 2) as u8]),
        _ => {}
    }
    out
}

/// Lenient quoted-printable: an `=` that does not start a valid escape stays.
fn qp(b: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(b.len());
    qp_walk(b, |c| out.push(c));
    out
}

/// Decode quoted-printable until `cap` decoded bytes. Return the bytes and
/// true if more decoded bytes follow.
pub fn qp_capped(b: &[u8], cap: usize) -> (Vec<u8>, bool) {
    let mut out = Vec::new();
    let mut at = b.len();
    qp_walk_until(b, |c, i| {
        if out.len() == cap {
            at = i;
            return false;
        }
        out.push(c);
        true
    });
    (out, qp_walk(&b[at..], |_| {}) > 0)
}

/// Give each decoded quoted-printable byte to `f`. Return the count.
fn qp_walk(b: &[u8], mut f: impl FnMut(u8)) -> u64 {
    let mut count = 0;
    qp_walk_until(b, |c, _| {
        count += 1;
        f(c);
        true
    });
    count
}

/// Give each decoded byte and the input index of its escape to `f`. Stop
/// when `f` returns false.
fn qp_walk_until(b: &[u8], mut f: impl FnMut(u8, usize) -> bool) {
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'=' {
            let rest = &b[i + 1..];
            if rest.starts_with(b"\r\n") {
                i += 3;
                continue;
            }
            if rest.starts_with(b"\n") {
                i += 2;
                continue;
            }
            if let (Some(h), Some(l)) = (rest.first().and_then(hex), rest.get(1).and_then(hex)) {
                if !f(h * 16 + l, i) {
                    return;
                }
                i += 3;
                continue;
            }
        }
        if !f(b[i], i) {
            return;
        }
        i += 1;
    }
}

/// Split a multipart body into its parts. The preamble and the epilogue go out.
/// If there is no close delimiter, the last part runs to the end. The split
/// stops after `max` parts.
pub fn split_multipart<'a>(body: &'a [u8], boundary: &str, max: usize) -> Vec<&'a [u8]> {
    let delim = format!("--{boundary}");
    let delim = delim.as_bytes();
    let mut parts = Vec::new();
    let mut start: Option<usize> = None;
    let mut line = 0;
    loop {
        let next = body[line..]
            .iter()
            .position(|&b| b == b'\n')
            .map(|n| line + n + 1);
        if body[line..].starts_with(delim) {
            let after = &body[line + delim.len()..];
            let close = after.starts_with(b"--");
            let only_space = after
                .iter()
                .take_while(|&&b| b != b'\n')
                .all(|b| b.is_ascii_whitespace() || close);
            if only_space {
                if let Some(s) = start.take() {
                    parts.push(trim_line_end(&body[s..line]));
                }
                if close || parts.len() >= max {
                    return parts;
                }
                start = next;
            }
        }
        match next {
            Some(n) if n < body.len() => line = n,
            _ => break,
        }
    }
    if let Some(s) = start {
        parts.push(&body[s.min(body.len())..]);
    }
    parts
}

/// Remove the line end before a delimiter. It belongs to the delimiter.
fn trim_line_end(b: &[u8]) -> &[u8] {
    let b = b.strip_suffix(b"\n").unwrap_or(b);
    b.strip_suffix(b"\r").unwrap_or(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoded_words_join_and_decode() {
        assert_eq!(decode_words("=?UTF-8?B?w5xi?= =?UTF-8?B?ZXI=?="), "Über");
        // One character split over two words.
        assert_eq!(decode_words("=?utf-8?B?w4==?= =?utf-8?B?nA==?="), "Ü");
        assert_eq!(
            decode_words("=?utf-8?Q?caf=C3=A9_au_lait?= now"),
            "café au lait now"
        );
        assert_eq!(
            decode_words("Re: =?iso-8859-1?q?d=E9j=E0?= vu"),
            "Re: déjà vu"
        );
        assert_eq!(decode_words("=?bad"), "=?bad");
        assert_eq!(decode_words("a =?x?Z?y?= b"), "a =?x?Z?y?= b");
    }

    #[test]
    fn rfc2231_sections_join() {
        let v = parse_value("attachment; filename*0*=utf-8''Gr%C3%BC; filename*1=\"sse.txt\"");
        assert_eq!(v.main, "attachment");
        assert_eq!(param(&v.params, "filename").as_deref(), Some("Grüsse.txt"));
        let v = parse_value("text/plain; name=\"a \\\"b\\\"; c.txt\"");
        assert_eq!(param(&v.params, "name").as_deref(), Some("a \"b\"; c.txt"));
    }

    #[test]
    fn charset_labels_that_lie() {
        assert_eq!(decode("Grüße".as_bytes(), Some("us-ascii")), "Grüße");
        assert_eq!(decode(b"Caf\xe9", Some("iso-8859-1")), "Café");
        assert_eq!(decode(b"Caf\xe9", None), "Café");
        assert_eq!(decode(b"\xcf\xf0\xe8", Some("windows-1251")), "При");
        assert_eq!(decode(b"x", Some("no-such-charset")), "x");
    }

    #[test]
    fn transfer_encodings() {
        assert_eq!(&*transfer_decode(b"SGVs\r\nbG8=", "base64"), b"Hello");
        assert_eq!(
            &*transfer_decode(b"a=3Db=\r\nc =zz", "quoted-printable"),
            b"a=bc =zz"
        );
        assert_eq!(decoded_len(b"SGVs\r\nbG8=", "base64"), 5);
        assert_eq!(decoded_len(b"a=3Db=\r\n", "quoted-printable"), 3);
        assert_eq!(qp_capped(b"a=\r\n=\r\nb=3Dc", 2), (b"ab".to_vec(), true));
        assert_eq!(qp_capped(b"ab=\r\n=\r\n", 2), (b"ab".to_vec(), false));
    }

    #[test]
    fn multipart_split() {
        let body = b"preamble\r\n--b\r\nA\r\n--b \r\nB\r\n\r\n--b--\r\nepilogue";
        assert_eq!(
            split_multipart(body, "b", 10),
            vec![&b"A"[..], &b"B\r\n"[..]]
        );
        // No close delimiter.
        assert_eq!(
            split_multipart(b"--b\nA\n--b\nB", "b", 10),
            vec![&b"A"[..], &b"B"[..]]
        );
        // `--bc` is not the boundary `b`.
        assert_eq!(
            split_multipart(b"--b\nA\n--bc\n--b--", "b", 10),
            vec![&b"A\n--bc"[..]]
        );
        // The split stops at the limit.
        assert_eq!(
            split_multipart(b"--b\nA\n--b\nB\n--b\nC", "b", 2),
            vec![&b"A"[..], &b"B"[..]]
        );
    }

    #[test]
    fn head_and_body() {
        assert_eq!(
            split_head(b"A: 1\r\n\r\nbody"),
            (&b"A: 1\r\n"[..], &b"body"[..])
        );
        assert_eq!(split_head(b"\nbody"), (&b""[..], &b"body"[..]));
        let h = Headers::parse(b"Subject: a\r\n  b\r\nX: \xe9\r\n");
        assert_eq!(h.get("subject"), Some("a  b"));
        assert_eq!(h.get("x"), Some("é"));
        let big = format!(
            "Subject: {}\r\n{}\r\n",
            "a".repeat(100_000),
            " b\r\n".repeat(100_000)
        );
        let h = Headers::parse(big.as_bytes());
        assert_eq!(h.get("subject").map(str::len), Some(VALUE_LIMIT));
        assert_eq!(base64_prefix(b"AB\r\nCD\r\nEF", 4), 6);
        assert_eq!(base64_prefix(b"AB", 4), 2);
    }
}

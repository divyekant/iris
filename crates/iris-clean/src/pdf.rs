//! The text of a simple PDF file. Bounded, and safe on bad input.
//!
//! The release build stops the program on a panic (`panic = "abort"`), and a
//! PDF attachment can come from an attacker. So this module does not use a
//! full PDF crate. It reads the content streams (raw or `FlateDecode`) and
//! the text operators `Tj`, `TJ`, `'` and `"`. It reads strings as
//! Windows-1252, the encoding of the standard PDF fonts.
//!
//! Known limit: a font with its own code map (CID fonts, `ToUnicode`) gives
//! codes, not letters. Such text fails the quality check, and the PDF stays a
//! name, a type and a size.

/// The most bytes of one decompressed stream.
const STREAM_LIMIT: usize = 4 * 1024 * 1024;
/// The most bytes of decompressed streams for one PDF.
const TOTAL_LIMIT: usize = 32 * 1024 * 1024;

/// The text of a PDF, at most `limit` bytes. `None` if the file has no text
/// that this module can read.
/// True if the first 1 KiB holds the PDF marker `%PDF`.
pub fn is_pdf(start: &[u8]) -> bool {
    find(&start[..start.len().min(1024)], b"%PDF").is_some()
}

pub fn text(pdf: &[u8], limit: usize) -> Option<String> {
    if !is_pdf(pdf) {
        return None;
    }
    let mut out = String::new();
    let mut total = 0;
    let mut at = 0;
    while let Some(n) = find(&pdf[at..], b"stream") {
        let kw = at + n;
        at = kw + 6;
        // The keyword `stream` ends with a line end. `endstream` does not match.
        let start = match &pdf[at..] {
            [b'\r', b'\n', ..] => at + 2,
            [b'\n', ..] => at + 1,
            _ => continue,
        };
        if kw >= 3 && &pdf[kw - 3..kw] == b"end" {
            continue;
        }
        let Some(len) = find(&pdf[start..], b"endstream") else {
            break;
        };
        let data = &pdf[start..start + len];
        at = start + len + 9;
        let dict = dict_before(pdf, kw);
        if !is_content(dict) {
            continue;
        }
        let decoded = if has(dict, b"/FlateDecode") {
            match miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(data, STREAM_LIMIT) {
                Ok(v) => v,
                Err(e) => e.output,
            }
        } else if has(dict, b"/Filter") {
            continue;
        } else {
            data.to_vec()
        };
        total += decoded.len();
        if find(&decoded, b"BT").is_some() {
            content_text(&decoded, &mut out, limit);
        }
        if out.len() >= limit || total >= TOTAL_LIMIT {
            break;
        }
    }
    let lines: Vec<&str> = out
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let text = lines.join("\n");
    readable(&text).then_some(text)
}

/// The stream dictionary: the bytes from the last `obj` keyword before the
/// stream, at most 4 KiB back.
fn dict_before(pdf: &[u8], kw: usize) -> &[u8] {
    let from = kw.saturating_sub(4096);
    let window = &pdf[from..kw];
    let start = window
        .windows(3)
        .rposition(|w| w == b"obj")
        .map_or(0, |p| p + 3);
    &window[start..]
}

/// False for streams that are not page content: images, fonts, metadata,
/// cross-reference streams and object streams.
fn is_content(dict: &[u8]) -> bool {
    ![
        &b"/Image"[..],
        b"/Length1",
        b"/Length2",
        b"/Metadata",
        b"/XRef",
        b"/ObjStm",
        b"/FontFile",
        b"/EmbeddedFile",
    ]
    .iter()
    .any(|k| has(dict, k))
}

fn has(hay: &[u8], needle: &[u8]) -> bool {
    find(hay, needle).is_some()
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// Read the text operators of one content stream into `out`.
fn content_text(c: &[u8], out: &mut String, limit: usize) {
    let mut strings: Vec<Vec<u8>> = Vec::new();
    let mut array: Option<Vec<u8>> = None;
    let mut last_num = 0.0f64;
    let mut i = 0;
    while i < c.len() && out.len() < limit {
        match c[i] {
            b'(' => {
                let (s, n) = literal(&c[i..]);
                i += n;
                match array.as_mut() {
                    Some(a) => a.extend(s),
                    None => strings.push(s),
                }
            }
            b'<' if c.get(i + 1) == Some(&b'<') => i += 2,
            b'<' => {
                let end = c[i..]
                    .iter()
                    .position(|&b| b == b'>')
                    .map_or(c.len(), |n| i + n);
                let s = hex(&c[i + 1..end]);
                i = (end + 1).min(c.len());
                match array.as_mut() {
                    Some(a) => a.extend(s),
                    None => strings.push(s),
                }
            }
            b'[' => {
                array = Some(Vec::new());
                i += 1;
            }
            b']' => {
                strings.extend(array.take());
                i += 1;
            }
            b'%' => {
                i = c[i..]
                    .iter()
                    .position(|&b| b == b'\n' || b == b'\r')
                    .map_or(c.len(), |n| i + n);
            }
            b'+' | b'-' | b'.' | b'0'..=b'9' => {
                let s = i;
                i += 1;
                while i < c.len() && (c[i].is_ascii_digit() || c[i] == b'.') {
                    i += 1;
                }
                let n: f64 = std::str::from_utf8(&c[s..i])
                    .ok()
                    .and_then(|t| t.parse().ok())
                    .unwrap_or(0.0);
                // In a `TJ` array, a large negative number moves to the next word.
                match array.as_mut() {
                    Some(a) if n < -200.0 => a.push(b' '),
                    _ => last_num = n,
                }
            }
            b if b.is_ascii_alphabetic() || b == b'\'' || b == b'"' || b == b'*' => {
                let s = i;
                while i < c.len()
                    && (c[i].is_ascii_alphanumeric() || matches!(c[i], b'\'' | b'"' | b'*'))
                {
                    i += 1;
                }
                match &c[s..i] {
                    b"Tj" | b"TJ" => emit(out, &mut strings),
                    b"'" | b"\"" => {
                        newline(out);
                        emit(out, &mut strings);
                    }
                    b"Td" | b"TD" => {
                        if last_num != 0.0 {
                            newline(out);
                        } else if !out.ends_with(char::is_whitespace) {
                            out.push(' ');
                        }
                    }
                    b"T*" | b"Tm" | b"ET" => newline(out),
                    _ => {}
                }
                strings.clear();
            }
            _ => i += 1,
        }
    }
}

fn emit(out: &mut String, strings: &mut Vec<Vec<u8>>) {
    for s in strings.drain(..) {
        out.push_str(&encoding_rs::WINDOWS_1252.decode_without_bom_handling(&s).0);
    }
}

fn newline(out: &mut String) {
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
}

/// A literal string `( ... )` with nested parentheses and escapes.
/// Return the bytes and the count of input bytes read.
fn literal(c: &[u8]) -> (Vec<u8>, usize) {
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut i = 0;
    while i < c.len() {
        let b = c[i];
        i += 1;
        match b {
            b'(' => {
                depth += 1;
                if depth > 1 {
                    out.push(b);
                }
            }
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return (out, i);
                }
                out.push(b);
            }
            b'\\' => {
                let Some(&e) = c.get(i) else { break };
                i += 1;
                match e {
                    b'n' => out.push(b'\n'),
                    b'r' => out.push(b'\r'),
                    b't' => out.push(b'\t'),
                    b'b' | b'f' => {}
                    b'\r' | b'\n' => {
                        if e == b'\r' && c.get(i) == Some(&b'\n') {
                            i += 1;
                        }
                    }
                    b'0'..=b'7' => {
                        let mut v = (e - b'0') as u32;
                        for _ in 0..2 {
                            match c.get(i) {
                                Some(&d @ b'0'..=b'7') => {
                                    v = v * 8 + (d - b'0') as u32;
                                    i += 1;
                                }
                                _ => break,
                            }
                        }
                        out.push(v as u8);
                    }
                    _ => out.push(e),
                }
            }
            _ => out.push(b),
        }
    }
    (out, i)
}

fn hex(h: &[u8]) -> Vec<u8> {
    let digits: Vec<u8> = h
        .iter()
        .filter_map(|c| (*c as char).to_digit(16).map(|d| d as u8))
        .collect();
    digits
        .chunks(2)
        .map(|p| p[0] * 16 + p.get(1).copied().unwrap_or(0))
        .collect()
}

/// True if the text looks like words: at least 3 letters, and at least 85 %
/// of the characters are letters, digits, white space or common punctuation.
fn readable(t: &str) -> bool {
    let (mut good, mut letters, mut all) = (0usize, 0usize, 0usize);
    for c in t.chars() {
        all += 1;
        if c.is_alphabetic() {
            letters += 1;
        }
        if c.is_alphanumeric() || c.is_whitespace() || ".,;:!?'\"()-/%&@€$£#*+=".contains(c) {
            good += 1;
        }
    }
    letters >= 3 && good * 100 >= all * 85
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn sample(content: &[u8], flate: bool) -> Vec<u8> {
        let (filter, data) = if flate {
            (
                "/Filter /FlateDecode ",
                miniz_oxide::deflate::compress_to_vec_zlib(content, 6),
            )
        } else {
            ("", content.to_vec())
        };
        let mut pdf = format!(
            "%PDF-1.4\n1 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj\n\
             4 0 obj\n<< {filter}/Length {} >>\nstream\n",
            data.len()
        )
        .into_bytes();
        pdf.extend(data);
        pdf.extend_from_slice(b"\nendstream\nendobj\ntrailer\n<< >>\n%%EOF\n");
        pdf
    }

    const PAGE: &[u8] = b"BT /F1 12 Tf 72 720 Td (Invoice 2025-07 for Avel Torun) Tj 0 -14 Td \
        [(Total:) -250 (120.00 EUR)] TJ T* (Caf\\351 \\(Paris\\)) Tj ET";

    #[test]
    fn text_of_a_simple_pdf() {
        let want = "Invoice 2025-07 for Avel Torun\nTotal: 120.00 EUR\nCafé (Paris)";
        assert_eq!(text(&sample(PAGE, true), 1000).as_deref(), Some(want));
        assert_eq!(text(&sample(PAGE, false), 1000).as_deref(), Some(want));
    }

    #[test]
    fn codes_and_bad_input_give_no_text() {
        // Two-byte CID codes are not words.
        let cid = b"BT <0011002200330044005500660077008800990011002200330044005500660077> Tj ET";
        assert_eq!(text(&sample(cid, true), 1000), None);
        assert_eq!(text(b"not a pdf", 1000), None);
        let pdf = sample(PAGE, true);
        for cut in [10, 60, 120, pdf.len() - 20] {
            text(&pdf[..cut], 1000);
        }
        let mut bad = b"%PDF-1.4\nstream\n(((((\\".to_vec();
        bad.extend(std::iter::repeat_n(b'(', 10_000));
        text(&bad, 1000);
    }

    #[test]
    fn short_text_stays() {
        assert_eq!(
            text(&sample(b"BT (PAID) Tj ET", true), 100).as_deref(),
            Some("PAID")
        );
    }

    #[test]
    fn the_limit_holds() {
        let t = text(&sample(PAGE, true), 25).unwrap_or_default();
        assert!(t.len() < 60, "{t}");
    }
}

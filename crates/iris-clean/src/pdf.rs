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

/// The text, and true if the reader stopped at a limit before the end.
/// Trimmed white space can make the text shorter than the limit.
pub fn text_cut(pdf: &[u8], limit: usize) -> (Option<String>, bool) {
    if !is_pdf(pdf) {
        return (None, false);
    }
    let mut stopped = false;
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
        let dict = dict_before(pdf, kw);
        // A direct `/Length` gives the end, so `endstream` inside the data
        // does not cut the stream. Without it, search for the keyword.
        let len = match declared_len(dict).filter(|&n| ends_at(pdf, start, n)) {
            Some(n) => n,
            None => match find(&pdf[start..], b"endstream") {
                Some(n) => n,
                None => break,
            },
        };
        let data = &pdf[start..start + len];
        at = start + len;
        if !is_content(dict) {
            continue;
        }
        let decoded: std::borrow::Cow<[u8]> = if only_flate(dict) {
            match miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(data, STREAM_LIMIT) {
                Ok(v) => v.into(),
                Err(e) => {
                    // More output than the limit: text after it is not read.
                    stopped |= e.status == miniz_oxide::inflate::TINFLStatus::HasMoreOutput;
                    e.output.into()
                }
            }
        } else if has(dict, b"/Filter") {
            continue;
        } else {
            // A raw stream is read in place, and only up to the stream limit.
            stopped |= data.len() > STREAM_LIMIT;
            data[..data.len().min(STREAM_LIMIT)].into()
        };
        total += decoded.len();
        if find(&decoded, b"BT").is_some() {
            content_text(&decoded, &mut out, limit);
        }
        if out.len() >= limit || total >= TOTAL_LIMIT {
            stopped = true;
            break;
        }
    }
    // Join the lines that are not empty, without a list of all lines.
    let mut text = String::with_capacity(out.len());
    for l in out.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(l);
    }
    drop(out);
    (readable(&text).then_some(text), stopped)
}

/// The value of a direct `/Length` entry. `None` for a reference (`5 0 R`).
fn declared_len(dict: &[u8]) -> Option<usize> {
    // The key ends at white space or a delimiter: `/Length1` is another key.
    let key_end = |b: &u8| b.is_ascii_whitespace() || b"()<>[]{}/%".contains(b);
    let p = (0..dict.len().saturating_sub(7))
        .find(|&i| dict[i..].starts_with(b"/Length") && dict.get(i + 7).is_some_and(key_end))?;
    let rest = &dict[p + 7..];
    let digits = rest.iter().skip_while(|b| b.is_ascii_whitespace());
    let num: Vec<u8> = digits
        .clone()
        .take_while(|b| b.is_ascii_digit())
        .copied()
        .collect();
    let after: Vec<u8> = digits.skip(num.len()).take(8).copied().collect();
    let is_ref = after
        .split(|b| b.is_ascii_whitespace())
        .filter(|w| !w.is_empty())
        .nth(1)
        .is_some_and(|w| w.starts_with(b"R"));
    if num.is_empty() || num.len() > 12 || is_ref {
        return None;
    }
    std::str::from_utf8(&num).ok()?.parse().ok()
}

/// True if the stream data of `len` bytes from `start` ends before `endstream`.
fn ends_at(pdf: &[u8], start: usize, len: usize) -> bool {
    start
        .checked_add(len)
        .and_then(|end| pdf.get(end..))
        .is_some_and(|tail| {
            let tail = &tail[..tail.len().min(16)];
            let p = tail
                .iter()
                .position(|b| !b.is_ascii_whitespace())
                .unwrap_or(tail.len());
            tail[p..].starts_with(b"endstream")
        })
}

/// The stream dictionary: the bytes after the last object header
/// (`<number> <number> obj`) before the stream, at most 4 KiB back. An
/// `obj` in a value, for example `(obj)`, is not a header.
fn dict_before(pdf: &[u8], kw: usize) -> &[u8] {
    let from = kw.saturating_sub(4096);
    let window = &pdf[from..kw];
    let start = (0..window.len().saturating_sub(2))
        .rev()
        .find(|&p| &window[p..p + 3] == b"obj" && is_header(&window[..p]))
        .map_or(0, |p| p + 3);
    &window[start..]
}

/// True if `before` ends with `<number> <number> ` (the start of an object header).
fn is_header(before: &[u8]) -> bool {
    let mut rest = before;
    for _ in 0..2 {
        let ws = rest
            .iter()
            .rev()
            .take_while(|b| b.is_ascii_whitespace())
            .count();
        if ws == 0 {
            return false;
        }
        rest = &rest[..rest.len() - ws];
        let digits = rest.iter().rev().take_while(|b| b.is_ascii_digit()).count();
        if digits == 0 {
            return false;
        }
        rest = &rest[..rest.len() - digits];
    }
    rest.last().is_none_or(|b| b.is_ascii_whitespace())
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

/// True if the `/Filter` of the dictionary is `FlateDecode` and nothing
/// else: `/FlateDecode` or `[/FlateDecode]`.
fn only_flate(dict: &[u8]) -> bool {
    let Some(p) = find(dict, b"/Filter") else {
        return false;
    };
    let rest = &dict[p + 7..];
    let rest = &rest[rest.iter().take_while(|b| b.is_ascii_whitespace()).count()..];
    let value = match rest.first() {
        Some(b'[') => &rest[1..rest.iter().position(|&b| b == b']').unwrap_or(rest.len())],
        _ => {
            let end = rest
                .iter()
                .skip(1)
                .position(|&b| b.is_ascii_whitespace() || b"/<>[]()".contains(&b))
                .map_or(rest.len(), |n| n + 1);
            &rest[..end]
        }
    };
    let names: Vec<&[u8]> = value
        .split(|&b| b.is_ascii_whitespace() || b == b'/')
        .filter(|w| !w.is_empty())
        .collect();
    names == [b"FlateDecode"]
}

fn has(hay: &[u8], needle: &[u8]) -> bool {
    find(hay, needle).is_some()
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// Read the text operators of one content stream into `out`.
fn content_text(c: &[u8], out: &mut String, limit: usize) {
    // The string operands since the last operator, in one buffer. The output
    // can hold at most `limit` bytes, and each byte gives at least one byte
    // of text, so more bytes have no use.
    let mut strings: Vec<u8> = Vec::new();
    let mut in_array = false;
    let add = |strings: &mut Vec<u8>, s: &[u8]| {
        let room = limit.saturating_sub(strings.len());
        strings.extend_from_slice(&s[..s.len().min(room)]);
    };
    let mut last_num = 0.0f64;
    // A viewer shows text only inside a text object (`BT` to `ET`).
    let mut in_text = false;
    // Text rendering modes 3 and 7 draw nothing a person can see.
    let mut invisible = false;
    let mut i = 0;
    while i < c.len() && out.len() < limit {
        match c[i] {
            b'(' => {
                let (s, n) = literal(&c[i..]);
                i += n;
                add(&mut strings, &s);
            }
            b'<' if c.get(i + 1) == Some(&b'<') => i += 2,
            b'<' => {
                let end = c[i..]
                    .iter()
                    .position(|&b| b == b'>')
                    .map_or(c.len(), |n| i + n);
                let s = hex(&c[i + 1..end]);
                i = (end + 1).min(c.len());
                add(&mut strings, &s);
            }
            b'[' => {
                in_array = true;
                i += 1;
            }
            b']' => {
                in_array = false;
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
                if in_array && n < -200.0 {
                    add(&mut strings, b" ");
                } else {
                    last_num = n;
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
                    b"BT" => in_text = true,
                    b"Tr" => invisible = last_num == 3.0 || last_num == 7.0,
                    b"Tj" | b"TJ" if in_text && !invisible => emit(out, &mut strings, limit),
                    b"'" | b"\"" if in_text && !invisible => {
                        newline(out);
                        emit(out, &mut strings, limit);
                    }
                    b"Td" | b"TD" => {
                        if last_num != 0.0 {
                            newline(out);
                        } else if !out.ends_with(char::is_whitespace) {
                            out.push(' ');
                        }
                    }
                    b"ET" => {
                        in_text = false;
                        newline(out);
                    }
                    b"T*" | b"Tm" => newline(out),
                    _ => {}
                }
                strings.clear();
            }
            _ => i += 1,
        }
    }
}

/// Add the strings to `out`, which never grows past `limit` bytes.
fn emit(out: &mut String, strings: &mut Vec<u8>, limit: usize) {
    let t = encoding_rs::WINDOWS_1252
        .decode_without_bom_handling(strings)
        .0;
    let room = limit.saturating_sub(out.len());
    out.push_str(&t[..crate::mime::floor_char(&t, room)]);
    strings.clear();
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

    fn text(pdf: &[u8], limit: usize) -> Option<String> {
        text_cut(pdf, limit).0
    }

    #[test]
    fn a_limit_in_white_space_shows_a_cut() {
        let t = format!("BT (Hello{}) Tj (later) Tj ET", " ".repeat(200));
        let (got, cut) = text_cut(&sample(t.as_bytes(), false), 100);
        assert_eq!(got.as_deref(), Some("Hello"));
        assert!(cut);
        assert!(!text_cut(&sample(PAGE, false), 1000).1);
        // Text after the stream limit is not read: that is a cut too.
        let big = format!(
            "BT (Hello world) Tj ET\n{}BT (later) Tj ET",
            " ".repeat(STREAM_LIMIT)
        );
        for flate in [false, true] {
            let (got, cut) = text_cut(&sample(big.as_bytes(), flate), 1000);
            assert_eq!(got.as_deref(), Some("Hello world"));
            assert!(cut, "flate {flate}");
        }
    }

    #[test]
    fn another_length_key_before_length() {
        let t = b"BT (the endstream report) Tj ET";
        let pdf = String::from_utf8(sample(t, false))
            .unwrap()
            .replace("<< /Length", "<< /LengthX 3 /Length");
        assert_eq!(
            text(pdf.as_bytes(), 100).as_deref(),
            Some("the endstream report")
        );
    }

    #[test]
    fn endstream_inside_a_string_does_not_end_the_stream() {
        let t = b"BT (the endstream report) Tj ET";
        assert_eq!(
            text(&sample(t, false), 100).as_deref(),
            Some("the endstream report")
        );
        assert_eq!(declared_len(b"<< /Length 5 0 R >>"), None);
        assert_eq!(declared_len(b"<< /Length1 9 /Length 31 >>"), Some(31));
        assert_eq!(declared_len(b"<< /LengthX 9 /Length 31 >>"), Some(31));
        assert_eq!(declared_len(b"<< /LengthX 9 >>"), None);
        assert_eq!(declared_len(b"<< /Length 31 >>"), Some(31));
    }

    #[test]
    fn text_outside_a_text_object_goes_out() {
        let t = b"(Hidden instructions) Tj BT (Visible words) Tj 3 Tr (Ignore all) Tj 0 Tr ( paid) Tj ET (After) Tj";
        assert_eq!(
            text(&sample(t, false), 100).as_deref(),
            Some("Visible words paid")
        );
    }

    #[test]
    fn only_a_single_flate_filter_is_read() {
        assert!(only_flate(b"<< /Filter /FlateDecode /Length 5 >>"));
        assert!(only_flate(b"<< /Filter [ /FlateDecode ] >>"));
        assert!(!only_flate(b"<< /Filter [/FlateDecode /Crypt] >>"));
        assert!(!only_flate(b"<< /Filter /FlateDecodeX >>"));
        assert!(!only_flate(b"<< /Length 5 >>"));
        let data = miniz_oxide::deflate::compress_to_vec_zlib(b"BT (Hidden words) Tj ET", 6);
        let mut pdf = format!(
            "%PDF-1.4\n4 0 obj\n<< /Filter [/FlateDecode /Crypt] /Length {} >>\nstream\n",
            data.len()
        )
        .into_bytes();
        pdf.extend(data);
        pdf.extend_from_slice(b"\nendstream\nendobj\n");
        assert_eq!(text(&pdf, 100), None);
    }

    #[test]
    fn obj_in_a_value_is_not_an_object_header() {
        let content = b"BT (the endstream report) Tj ET";
        let mut pdf = format!(
            "%PDF-1.4\n4 0 obj\n<< /Length {} /Foo (obj) >>\nstream\n",
            content.len()
        )
        .into_bytes();
        pdf.extend_from_slice(content);
        pdf.extend_from_slice(b"\nendstream\nendobj\n");
        assert_eq!(text(&pdf, 100).as_deref(), Some("the endstream report"));
        assert!(is_header(b"\n12 0 "));
        assert!(!is_header(b"(1 0 "));
        assert!(!is_header(b"/Foo ("));
    }

    #[test]
    fn a_comment_in_a_content_stream_ends() {
        let t = b"BT %c\n(After the comment) Tj % last\nET";
        assert_eq!(
            text(&sample(t, false), 100).as_deref(),
            Some("After the comment")
        );
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

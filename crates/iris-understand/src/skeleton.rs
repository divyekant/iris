//! A bounded copy of one raw message for understanding. The copy keeps
//! every header, every boundary and every part that `iris-clean` decodes.
//! It drops only the bodies that `iris-clean` never decodes (for example an
//! image), and the end of a part after the limit of `iris-clean`. So a
//! large message keeps its later text and calendar parts, and the memory
//! of one message stays bounded (specification 1, M3).

use std::io::{BufRead, Result};

/// The most bytes of the copy.
pub const TOTAL_LIMIT: usize = 16 * 1024 * 1024;
/// The most encoded bytes of a text part or an attached message. Base64
/// of `iris_clean::PART_LIMIT` decoded bytes, with a margin.
const TEXT_KEEP: usize = iris_clean::PART_LIMIT / 3 * 4 + 64 * 1024;
/// The most encoded bytes of a PDF that the copy keeps. A larger PDF is
/// not decoded by `iris-clean` (`iris_clean::PDF_LIMIT`).
const PDF_KEEP: usize = iris_clean::PDF_LIMIT as usize / 3 * 4 + 64 * 1024;
/// The most bytes of one header block and of a preamble.
const HEAD_KEEP: usize = 64 * 1024;
/// The most bytes of one line: the rest of a longer line is dropped.
const LINE_KEEP: usize = 1024 * 1024;
/// The most declared boundaries.
const MAX_BOUNDARIES: usize = 50;

#[derive(Clone, Copy, PartialEq)]
enum Body {
    /// Keep at most this many bytes, then drop the rest.
    Prefix(usize),
    /// Keep the whole body if it has at most this many bytes, else none.
    Whole(usize),
    Drop,
}

/// Read `src` to a bounded copy.
// shortcut: a dropped body gives a size of 0 to `iris-clean`, so the model
// sees 0 bytes for that attachment. Send the original size if the model
// needs it.
pub fn read(mut src: impl BufRead) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut boundaries: Vec<Vec<u8>> = Vec::new();
    let mut in_head = true;
    let mut head: Vec<u8> = Vec::new();
    let mut body = Body::Prefix(HEAD_KEEP);
    let mut kept = 0usize;
    // The body of a `Whole` part until its end.
    let mut whole: Vec<u8> = Vec::new();
    let mut line = Vec::new();
    while next_line(&mut src, &mut line)? {
        let bare = trim_end(&line);
        let is_boundary = bare.starts_with(b"--")
            && boundaries.iter().any(|bd| {
                let rest = &bare[2..];
                rest.starts_with(bd) && matches!(&rest[bd.len()..], b"" | b"--")
            });
        if is_boundary {
            flush_whole(&mut out, &mut whole, body);
            push(&mut out, &line);
            (in_head, kept) = (true, 0);
            head.clear();
            continue;
        }
        if in_head {
            if head.len() + line.len() <= HEAD_KEEP {
                head.extend_from_slice(&line);
                push(&mut out, &line);
            }
            if bare.is_empty() {
                in_head = false;
                body = rule(&head, &mut boundaries);
            }
            continue;
        }
        match body {
            Body::Prefix(n) => {
                if kept + line.len() <= n {
                    kept += line.len();
                    push(&mut out, &line);
                }
            }
            Body::Whole(n) => {
                if whole.len() + line.len() <= n {
                    whole.extend_from_slice(&line);
                } else {
                    // Too large: `iris-clean` does not decode it.
                    whole.clear();
                    body = Body::Drop;
                }
            }
            Body::Drop => {}
        }
    }
    flush_whole(&mut out, &mut whole, body);
    Ok(out)
}

/// The body rule of a part from its header block. Also records a declared
/// boundary.
fn rule(head: &[u8], boundaries: &mut Vec<Vec<u8>>) -> Body {
    let low = head.to_ascii_lowercase();
    let ct = field(&low, b"content-type:");
    if let Some(at) = find(ct, b"boundary=") {
        // The value with its case kept, from the same place in `head`.
        let start = ct.as_ptr() as usize - low.as_ptr() as usize + at + 9;
        let v = &head[start..];
        let v = match v.strip_prefix(b"\"") {
            Some(q) => &q[..q.iter().position(|&c| c == b'"').unwrap_or(q.len())],
            None => {
                let end = v
                    .iter()
                    .position(|&c| c == b';' || c.is_ascii_whitespace())
                    .unwrap_or(v.len());
                &v[..end]
            }
        };
        if !v.is_empty() && boundaries.len() < MAX_BOUNDARIES {
            boundaries.push(v.to_vec());
        }
    }
    let ct = ct.trim_ascii_start();
    if ct.is_empty() || ct.starts_with(b"text/") {
        Body::Prefix(TEXT_KEEP)
    } else if ct.starts_with(b"multipart/") {
        Body::Prefix(HEAD_KEEP)
    } else if ct.starts_with(b"message/rfc822") {
        Body::Prefix(TEXT_KEEP)
    } else if ct.starts_with(b"application/pdf") || ct.starts_with(b"application/octet-stream") {
        // A PDF can have the type application/octet-stream and a .pdf name.
        Body::Whole(PDF_KEEP)
    } else {
        Body::Drop
    }
}

/// The value of the header `name` (lowercase, with the colon) in a
/// lowercase header block, with its folded lines. Empty if not present.
fn field<'a>(low: &'a [u8], name: &[u8]) -> &'a [u8] {
    let mut at = 0;
    while at < low.len() {
        let end = low[at..]
            .iter()
            .position(|&c| c == b'\n')
            .map_or(low.len(), |p| at + p + 1);
        if low[at..].starts_with(name) {
            // The value runs to the next line that is not folded.
            let mut stop = end;
            while stop < low.len() && matches!(low[stop], b' ' | b'\t') {
                stop = low[stop..]
                    .iter()
                    .position(|&c| c == b'\n')
                    .map_or(low.len(), |p| stop + p + 1);
            }
            return &low[at + name.len()..stop];
        }
        at = end;
    }
    &[]
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn flush_whole(out: &mut Vec<u8>, whole: &mut Vec<u8>, body: Body) {
    if matches!(body, Body::Whole(_)) && !whole.is_empty() {
        push(out, whole);
    }
    whole.clear();
}

/// Add bytes to the copy, if the copy stays inside `TOTAL_LIMIT`.
fn push(out: &mut Vec<u8>, b: &[u8]) {
    if out.len() + b.len() <= TOTAL_LIMIT {
        out.extend_from_slice(b);
    }
}

fn trim_end(l: &[u8]) -> &[u8] {
    let l = l.strip_suffix(b"\n").unwrap_or(l);
    l.strip_suffix(b"\r").unwrap_or(l)
}

/// Read one line with its end into `line`, at most `LINE_KEEP` bytes: the
/// rest of a longer line is read and dropped. False at the end of `src`.
fn next_line(src: &mut impl BufRead, line: &mut Vec<u8>) -> Result<bool> {
    line.clear();
    let mut any = false;
    loop {
        let buf = src.fill_buf()?;
        if buf.is_empty() {
            return Ok(any);
        }
        any = true;
        let (take, done) = match buf.iter().position(|&c| c == b'\n') {
            Some(p) => (p + 1, true),
            None => (buf.len(), false),
        };
        let room = LINE_KEEP.saturating_sub(line.len());
        line.extend_from_slice(&buf[..take.min(room)]);
        src.consume(take);
        if done {
            return Ok(true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn copy(raw: &str) -> String {
        String::from_utf8(read(raw.as_bytes()).unwrap()).unwrap()
    }

    #[test]
    fn keeps_text_and_drops_an_image_body() {
        let raw = "Content-Type: multipart/mixed; boundary=\"z\"\n\n--z\nContent-Type: image/png\n\nIMAGEBYTES\n--z\nContent-Type: text/plain\n\nHello\n--z--\n";
        let c = copy(raw);
        assert!(!c.contains("IMAGEBYTES"));
        assert!(c.contains("Hello"));
        assert!(c.contains("Content-Type: image/png"));
        assert!(c.ends_with("--z--\n"));
    }

    #[test]
    fn keeps_a_small_pdf_and_drops_a_large_one() {
        let small = "Content-Type: multipart/mixed; boundary=z\n\n--z\nContent-Type: application/pdf\n\nPDFDATA\n--z--\n";
        assert!(copy(small).contains("PDFDATA"));
        let big = format!(
            "Content-Type: multipart/mixed; boundary=z\n\n--z\nContent-Type: application/pdf\n\n{}--z\nContent-Type: text/plain\n\nAfter\n--z--\n",
            "QUFB\n".repeat(PDF_KEEP / 5 + 10)
        );
        let c = copy(&big);
        assert!(!c.contains("QUFB"));
        assert!(c.contains("After"));
    }

    #[test]
    fn the_copy_and_a_long_line_stay_bounded() {
        let line = "A".repeat(3 * LINE_KEEP);
        let raw = format!("Subject: x\n\n{line}\nend\n");
        let c = copy(&raw);
        assert!(c.len() < 2 * LINE_KEEP);
        assert!(c.contains("end"));
        let many = format!(
            "Content-Type: multipart/mixed; boundary=z\n\n{}--z--\n",
            format!("--z\nContent-Type: text/plain\n\n{}\n", "B".repeat(60_000)).repeat(400)
        );
        assert!(read(many.as_bytes()).unwrap().len() <= TOTAL_LIMIT);
    }
}

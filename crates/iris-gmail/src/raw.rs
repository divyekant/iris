//! Read a `messages.get` answer with `format=raw` as a stream.
//!
//! The answer is JSON with the raw message as one base64url string. The
//! reader decodes that string into a sink while it reads, through a fixed
//! buffer, so it never holds the full message (spec 1 section 9). It keeps
//! the other fields, which are small, and gives them back as JSON with
//! `"raw": ""`.

use std::io::{Read, Write};

/// The size of the read buffer and of the output buffer.
pub const BUFFER: usize = 64 * 1024;
/// The fields other than `raw` must fit in this size.
const REST_LIMIT: usize = 1024 * 1024;

#[derive(Debug)]
pub enum SplitError {
    /// The read of the answer failed.
    Read(std::io::Error),
    /// The write to the sink failed.
    Write(std::io::Error),
    /// The answer is not in the expected shape.
    Format(&'static str),
}

/// Copy the decoded `raw` field of the JSON in `body` to `sink`. Return the
/// JSON without the data of `raw`, and the count of decoded bytes.
pub fn split<R: Read, W: Write>(body: R, sink: &mut W) -> Result<(Vec<u8>, u64), SplitError> {
    split_at(body, sink, 1)
}

/// As [`split`], for the `raw` field of the object `message` of a draft
/// (`users.drafts.get`): the key `raw` at the JSON depth 2.
pub fn split_draft<R: Read, W: Write>(body: R, sink: &mut W) -> Result<(Vec<u8>, u64), SplitError> {
    split_at(body, sink, 2)
}

fn split_at<R: Read, W: Write>(
    mut body: R,
    sink: &mut W,
    level: u32,
) -> Result<(Vec<u8>, u64), SplitError> {
    let mut buf = vec![0u8; BUFFER];
    let mut scan = Scanner {
        level,
        ..Scanner::default()
    };
    let mut decoder = Decoder::new();
    loop {
        let n = match body.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(SplitError::Read(e)),
        };
        for &b in &buf[..n] {
            if scan.in_raw {
                if b == b'"' {
                    scan.in_raw = false;
                    scan.found_raw = true;
                    scan.rest.push(b'"');
                    decoder.finish(sink)?;
                } else {
                    decoder.push(b, sink)?;
                }
            } else {
                scan.step(b)?;
            }
        }
    }
    if scan.in_raw || !scan.found_raw || scan.depth != 0 {
        return Err(SplitError::Format("no complete raw field"));
    }
    Ok((scan.rest, decoder.total))
}

/// A JSON scanner that knows only enough to find the key `raw` at the
/// depth `level` (1: the top level).
#[derive(Default)]
struct Scanner {
    level: u32,
    rest: Vec<u8>,
    depth: u32,
    in_string: bool,
    escape: bool,
    expect_key: bool,
    /// Bytes of the top-level key that is read now.
    key: Option<Vec<u8>>,
    last_key_is_raw: bool,
    raw_next: bool,
    in_raw: bool,
    found_raw: bool,
}

impl Scanner {
    fn step(&mut self, b: u8) -> Result<(), SplitError> {
        if self.rest.len() >= REST_LIMIT {
            return Err(SplitError::Format("fields other than raw are too large"));
        }
        if self.in_string {
            self.rest.push(b);
            if self.escape {
                self.escape = false;
            } else if b == b'\\' {
                self.escape = true;
            } else if b == b'"' {
                self.in_string = false;
                if let Some(k) = self.key.take() {
                    self.last_key_is_raw = k == b"raw";
                }
                return Ok(());
            }
            if let Some(k) = &mut self.key {
                k.push(b);
            }
            return Ok(());
        }
        if self.raw_next && !b.is_ascii_whitespace() {
            self.raw_next = false;
            if b == b'"' {
                if self.found_raw {
                    return Err(SplitError::Format("two raw fields"));
                }
                self.in_raw = true;
                self.rest.push(b);
                return Ok(());
            }
            return Err(SplitError::Format("raw is not a string"));
        }
        self.rest.push(b);
        match b {
            b'"' => {
                self.in_string = true;
                if self.depth == self.level && self.expect_key {
                    self.key = Some(Vec::new());
                }
            }
            b'{' | b'[' => {
                self.depth += 1;
                self.expect_key = self.depth == self.level && b == b'{';
            }
            b'}' | b']' => {
                self.depth = self
                    .depth
                    .checked_sub(1)
                    .ok_or(SplitError::Format("bad JSON nesting"))?;
            }
            b',' if self.depth == self.level => self.expect_key = true,
            b':' if self.depth == self.level => {
                self.expect_key = false;
                self.raw_next = self.last_key_is_raw;
                self.last_key_is_raw = false;
            }
            _ => {}
        }
        Ok(())
    }
}

/// A streamed base64 decoder. It takes the URL alphabet and the standard
/// alphabet, and ignores `=` padding.
struct Decoder {
    acc: u32,
    bits: u32,
    out: Vec<u8>,
    total: u64,
}

impl Decoder {
    fn new() -> Decoder {
        Decoder {
            acc: 0,
            bits: 0,
            out: Vec::with_capacity(BUFFER),
            total: 0,
        }
    }

    fn push<W: Write>(&mut self, c: u8, sink: &mut W) -> Result<(), SplitError> {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'-' | b'+' => 62,
            b'_' | b'/' => 63,
            b'=' => return Ok(()),
            _ => return Err(SplitError::Format("bad base64 in raw")),
        };
        self.acc = (self.acc << 6) | v as u32;
        self.bits += 6;
        if self.bits >= 8 {
            self.bits -= 8;
            self.out.push((self.acc >> self.bits) as u8);
            self.acc &= (1 << self.bits) - 1;
            if self.out.len() == BUFFER {
                self.flush(sink)?;
            }
        }
        Ok(())
    }

    fn finish<W: Write>(&mut self, sink: &mut W) -> Result<(), SplitError> {
        // 6 bits left over means a lone character: not valid base64.
        if self.bits >= 6 {
            return Err(SplitError::Format("bad base64 length in raw"));
        }
        self.flush(sink)
    }

    fn flush<W: Write>(&mut self, sink: &mut W) -> Result<(), SplitError> {
        sink.write_all(&self.out).map_err(SplitError::Write)?;
        self.total += self.out.len() as u64;
        self.out.clear();
        Ok(())
    }
}

/// Encode bytes as base64url with no padding, for `messages.send`.
pub fn encode_url(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, &b)| n | (b as u32) << (16 - 8 * i));
        for i in 0..=chunk.len() {
            out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A reader that gives at most `step` bytes for each read.
    struct Slow<'a>(&'a [u8], usize);

    impl Read for Slow<'_> {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let n = self.0.len().min(self.1).min(buf.len());
            buf[..n].copy_from_slice(&self.0[..n]);
            self.0 = &self.0[n..];
            Ok(n)
        }
    }

    #[test]
    fn splits_at_each_chunk_size() {
        let raw: Vec<u8> = (0..=255u8).cycle().take(1000).collect();
        let body = format!(
            r#"{{"id":"a","snippet":"say \"raw\": \"x\" {{ [","payload":{{"raw":"no"}},"raw" : "{}","sizeEstimate":3}}"#,
            encode_url(&raw)
        );
        for step in [1, 2, 3, 7, 64, 100_000] {
            let mut sink = Vec::new();
            let (rest, n) = split(Slow(body.as_bytes(), step), &mut sink).unwrap();
            assert_eq!(sink, raw, "step {step}");
            assert_eq!(n, 1000);
            let v: serde_json::Value = serde_json::from_slice(&rest).unwrap();
            assert_eq!(v["raw"], "");
            assert_eq!(v["payload"]["raw"], "no");
            assert_eq!(v["sizeEstimate"], 3);
        }
    }

    /// A draft answer in any field order, with any white space and any
    /// chunk size, gives the raw of `message` and nothing else. A decoy
    /// `raw` at another depth is not the raw.
    #[test]
    fn a_draft_answer_splits_in_any_form() {
        let mut seed: u64 = 0x2545_f491_4f6c_dd1d;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for _ in 0..500 {
            let len = (next() % 3000) as usize;
            let raw: Vec<u8> = (0..len).map(|_| next() as u8).collect();
            let ws = [" ", "", "\n", "\t "][(next() % 4) as usize];
            let mut fields = vec![
                format!(r#""id"{ws}:{ws}"m1""#),
                format!(r#""threadId":"t1""#),
                format!(r#""labelIds":["DRAFT",{{"raw":"QQ"}}]"#),
                format!(r#""raw"{ws}:{ws}"{}""#, encode_url(&raw)),
            ];
            let k = (next() % 4) as usize;
            fields.swap(k, 3);
            let body = format!(
                r#"{{"raw":"decoy",{ws}"id":"r1","message":{ws}{{{}}}}}"#,
                fields.join(&format!(",{ws}"))
            );
            let mut sink = Vec::new();
            let step = 1 + (next() % 200) as usize;
            let (rest, n) = split_draft(Slow(body.as_bytes(), step), &mut sink).unwrap();
            assert_eq!((sink.as_slice(), n), (raw.as_slice(), len as u64));
            let v: serde_json::Value = serde_json::from_slice(&rest).unwrap();
            assert_eq!(
                (v["id"].as_str(), v["message"]["id"].as_str()),
                (Some("r1"), Some("m1"))
            );
        }
    }

    #[test]
    fn padding_and_both_alphabets_decode() {
        let mut sink = Vec::new();
        split(&br#"{"raw":"+/8="}"#[..], &mut sink).unwrap();
        assert_eq!(sink, [0xfb, 0xff]);
        assert_eq!(encode_url(&[0xfb, 0xff]), "-_8");
        assert_eq!(encode_url(b"Hello"), "SGVsbG8");
    }

    #[test]
    fn refuses_bad_answers() {
        for body in [
            &br#"{"id":"a"}"#[..],
            br#"{"raw":"AB*C"}"#,
            br#"{"raw":"A"}"#,
            br#"{"raw":"QQ"#,
            br#"{"raw":5}"#,
            br#"{"raw":"QQ","raw":"QQ"}"#,
        ] {
            assert!(split(body, &mut Vec::new()).is_err(), "{:?}", body);
        }
    }
}

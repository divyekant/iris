//! Charsets and base64 prefixes. The `mail-parser` crate reads the MIME
//! structure, the headers and the transfer encodings.

use encoding_rs::{Encoding, UTF_8, WINDOWS_1252};

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
        if is_b64(b) {
            n += 1;
        }
    }
    body.len()
}

/// The count of base64 alphabet characters (padding and white space do not count).
pub fn base64_chars(body: &[u8]) -> usize {
    body.iter().filter(|&&b| is_b64(b)).count()
}

pub fn is_b64(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'+' || b == b'/'
}

/// The most source bytes for one byte of decoded text: 2 for UTF-16, else 1.
pub fn bytes_per_byte(label: Option<&str>) -> usize {
    let enc = label.and_then(|l| Encoding::for_label(l.trim().as_bytes()));
    if enc == Some(encoding_rs::UTF_16LE) || enc == Some(encoding_rs::UTF_16BE) {
        2
    } else {
        1
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn charset_labels_that_lie() {
        assert_eq!(decode("Grüße".as_bytes(), Some("us-ascii")), "Grüße");
        assert_eq!(decode(b"Caf\xe9", Some("iso-8859-1")), "Café");
        assert_eq!(decode(b"Caf\xe9", None), "Café");
        assert_eq!(decode(b"\xcf\xf0\xe8", Some("windows-1251")), "При");
        assert_eq!(decode(b"x", Some("no-such-charset")), "x");
    }

    #[test]
    fn base64_prefix_skips_line_ends() {
        assert_eq!(base64_prefix(b"AB\r\nCD\r\nEF", 4), 6);
        assert_eq!(base64_prefix(b"AB", 4), 2);
    }
}

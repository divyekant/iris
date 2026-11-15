//! Base64 (RFC 4648), standard and URL-safe. Gmail uses the URL-safe form.

const STD: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const URL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

fn encode_with(data: &[u8], table: &[u8; 64], pad: bool) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let n = chunk.iter().fold(0u32, |n, b| n << 8 | *b as u32) << (8 * (3 - chunk.len()));
        for k in 0..=chunk.len() {
            out.push(table[(n >> (18 - 6 * k) & 63) as usize] as char);
        }
        if pad {
            for _ in chunk.len()..3 {
                out.push('=');
            }
        }
    }
    out
}

/// Standard alphabet with `=` padding (MIME bodies).
pub fn encode(data: &[u8]) -> String {
    encode_with(data, STD, true)
}

/// URL-safe alphabet with padding, as Gmail returns `raw`.
pub fn encode_url(data: &[u8]) -> String {
    encode_with(data, URL, true)
}

/// Decode either alphabet, with or without padding. `None` on a bad character.
pub fn decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0);
    for c in text.bytes().filter(|c| !matches!(c, b'=' | b'\r' | b'\n')) {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return None,
        };
        acc = acc << 6 | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_known_values() {
        assert_eq!(encode(b"foob"), "Zm9vYg==");
        assert_eq!(encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(encode_url(&[0xfb, 0xff]), "-_8=");
        for n in 0..300usize {
            let data: Vec<u8> = (0..n).map(|i| (i * 37 % 256) as u8).collect();
            assert_eq!(decode(&encode(&data)).unwrap(), data);
            assert_eq!(decode(&encode_url(&data)).unwrap(), data);
        }
        assert_eq!(decode("Zm9v*"), None);
    }
}

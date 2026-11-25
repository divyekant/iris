//! The standard security handler of PDF (ISO 32000-2 section 7.6.4), for a
//! file with an empty user password (issue #52). Many PDF files of mail
//! have only an owner password: a viewer opens them with no password, and
//! the owner password only limits print or copy. Iris reads such a file as
//! a viewer does. A file that needs a user password gives no text.
//!
//! Supported: RC4 of 40 to 128 bits (`/V 1`, `/V 2`, `/R 2` to `/R 4`),
//! AES-128 (`/V 4`, `/CFM /AESV2`) and AES-256 (`/V 5`, `/R 5` and `/R 6`).
//! Each check of the input fails closed: [`Setup::Locked`].
//!
//! Full PDF crates (`lopdf`, `pdf`) decrypt too, but they parse the whole
//! file and can panic on hostile input (the release build aborts on a
//! panic). So this module uses only the hash and block cipher crates of
//! RustCrypto (`md-5`, `sha2`, `aes`).

use aes::cipher::{BlockCipherDecrypt, BlockCipherEncrypt, KeyInit};
use md5::{Digest, Md5};
use sha2::{Sha256, Sha384, Sha512};

use super::{direct_name, find, get, hex, literal, reference, value, Lexer, Val};

/// The padding string of the password (Algorithm 2, step a).
const PAD: [u8; 32] = [
    0x28, 0xBF, 0x4E, 0x5E, 0x4E, 0x75, 0x8A, 0x41, 0x64, 0x00, 0x4E, 0x56, 0xFF, 0xFA, 0x01, 0x08,
    0x2E, 0x2E, 0x00, 0xB6, 0xD0, 0x68, 0x3E, 0x80, 0x2F, 0x0C, 0xA9, 0xFE, 0x64, 0x53, 0x69, 0x7A,
];

/// The most bytes of the encryption dictionary that the reader reads.
const DICT_BYTES: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cipher {
    /// The streams are not encrypted (`/StmF /Identity`).
    None,
    Rc4,
    Aes128,
    Aes256,
}

/// The decryption of the streams of one PDF.
#[derive(Debug)]
pub(super) struct Crypt {
    cipher: Cipher,
    key: Vec<u8>,
}

/// The result of the read of the encryption of a PDF.
#[derive(Debug)]
pub(super) enum Setup {
    /// The file has no encryption dictionary.
    Plain,
    /// The file opens with the empty user password.
    Open(Crypt),
    /// The file needs a password, or Iris cannot read its encryption.
    Locked,
}

/// The encryption of `pdf`, from the trailer that the last `startxref`
/// names: a `trailer` dictionary after an `xref` table, or the dictionary of
/// a cross-reference stream. A viewer reads the same trailer. `/Encrypt`
/// text in other places (a content stream, a comment) does not count. A
/// file whose trailer Iris cannot find reads as a file with no encryption:
/// encrypted streams then give no readable text.
pub(super) fn setup(pdf: &[u8]) -> Setup {
    let Some(trailer) = trailer(pdf) else {
        return Setup::Plain;
    };
    let dict = match get(trailer, b"/Encrypt") {
        None => return Setup::Plain,
        Some(Val::Dict(d)) => Some(d),
        Some(Val::Ref(_)) => {
            reference(trailer, b"/Encrypt").and_then(|(n, g)| object_dict(pdf, n, g))
        }
        Some(_) => None,
    };
    let Some(dict) = dict else {
        return Setup::Locked;
    };
    // Only the standard handler. Another filter needs its own software.
    if get(dict, b"/Filter") != Some(Val::Name(b"/Standard")) {
        return Setup::Locked;
    }
    let id = match get(trailer, b"/ID") {
        Some(Val::Array(a)) => string(&a[1..]).unwrap_or_default(),
        _ => Vec::new(),
    };
    match open(dict, &id) {
        Some(c) => Setup::Open(c),
        None => Setup::Locked,
    }
}

/// The bytes at the end of the file where `startxref` must be.
const TAIL: usize = 1024;

/// The trailer dictionary that the last `startxref` of the last
/// [`TAIL`] bytes names. Each step reads a bounded part of the file.
fn trailer(pdf: &[u8]) -> Option<&[u8]> {
    let tail_at = pdf.len().saturating_sub(TAIL);
    let at = tail_at + rfind(&pdf[tail_at..], b"startxref")?;
    let mut lx = Lexer::new(&pdf[at + 9..]);
    let Some(Val::Int(offset)) = value(&mut lx) else {
        return None;
    };
    let offset = usize::try_from(offset).ok().filter(|&o| o < pdf.len())?;
    let rest = &pdf[offset..];
    let first = rest.iter().position(|b| !b.is_ascii_whitespace())?;
    let rest = &rest[first..];
    if rest.starts_with(b"xref") {
        // A classic table: the trailer comes after the entries of the
        // table, about 20 bytes for each object.
        let t = find(rest, b"trailer")?;
        let after = &rest[t + 7..rest.len().min(t + 7 + DICT_BYTES)];
        return match value(&mut Lexer::new(after))? {
            Val::Dict(d) => Some(d),
            _ => None,
        };
    }
    // A cross-reference stream: `N G obj << /Type /XRef ... >>`.
    let mut lx = Lexer::new(&rest[..rest.len().min(DICT_BYTES)]);
    for _ in 0..3 {
        lx.next()?;
    }
    match value(&mut lx)? {
        Val::Dict(d) if get(d, b"/Type") == Some(Val::Name(b"/XRef")) => Some(d),
        _ => None,
    }
}

/// The dictionary of the last object `n g obj` of the file, with any white
/// space between the tokens. Each step reads back from the last match, so
/// the search reads each byte of the file at most a bounded count of times.
fn object_dict(pdf: &[u8], n: u32, g: u32) -> Option<&[u8]> {
    let mut end = pdf.len();
    let at = loop {
        let at = rfind(&pdf[..end], b"obj")?;
        end = at;
        let after = pdf.get(at + 3);
        if after.is_some_and(|b| !b.is_ascii_whitespace() && !b"<[(/%".contains(b)) {
            continue;
        }
        if header_before(&pdf[..at]) == Some((n, g)) {
            break at;
        }
    };
    let body = &pdf[at + 3..pdf.len().min(at + 3 + DICT_BYTES)];
    match value(&mut Lexer::new(body))? {
        Val::Dict(d) => Some(d),
        _ => None,
    }
}

/// The most white space bytes between two tokens of an object header.
const HEADER_SPACE: usize = 32;

/// The object number and generation of `<n> <g> ` at the end of `b`, the
/// bytes before an `obj` token.
fn header_before(b: &[u8]) -> Option<(u32, u32)> {
    let mut i = b.len();
    let mut back = |need_space: bool| -> Option<u32> {
        let ws = b[..i]
            .iter()
            .rev()
            .take(HEADER_SPACE + 1)
            .take_while(|c| c.is_ascii_whitespace())
            .count();
        if ws > HEADER_SPACE || (need_space && ws == 0) {
            return None;
        }
        i -= ws;
        let digits = b[..i]
            .iter()
            .rev()
            .take(11)
            .take_while(|c| c.is_ascii_digit())
            .count();
        if digits == 0 || digits > 10 {
            return None;
        }
        i -= digits;
        std::str::from_utf8(&b[i..i + digits]).ok()?.parse().ok()
    };
    let g = back(true)?;
    let n = back(true)?;
    // The number starts a token.
    (i == 0 || b[i - 1].is_ascii_whitespace()).then_some((n, g))
}

fn rfind(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).rposition(|w| w == needle)
}

/// The first string of `b` after white space: `(...)` or `<...>`.
fn string(b: &[u8]) -> Option<Vec<u8>> {
    let s = b.iter().position(|&c| !c.is_ascii_whitespace())?;
    match b[s] {
        b'(' => Some(literal(&b[s..]).0),
        b'<' if b.get(s + 1) != Some(&b'<') => {
            let e = b[s..].iter().position(|&c| c == b'>')?;
            Some(hex(&b[s + 1..s + e]))
        }
        _ => None,
    }
}

/// The string value of `key` in the dictionary `d`.
fn string_of(d: &[u8], key: &[u8]) -> Option<Vec<u8>> {
    let at = direct_name(d, key, true)?;
    string(&d[at + key.len()..])
}

fn int_of(d: &[u8], key: &[u8]) -> Option<i64> {
    match get(d, key)? {
        Val::Int(n) => Some(n),
        _ => None,
    }
}

/// The stream cipher of a `/V 4` or `/V 5` dictionary: the `/CFM` of the
/// crypt filter that `/StmF` names.
fn stream_filter(d: &[u8]) -> Option<Cipher> {
    let name = match get(d, b"/StmF") {
        None => return Some(Cipher::None),
        Some(Val::Name(n)) => n,
        _ => return None,
    };
    if name == b"/Identity" {
        return Some(Cipher::None);
    }
    let Some(Val::Dict(cf)) = get(d, b"/CF") else {
        return None;
    };
    let Some(Val::Dict(f)) = get(cf, name) else {
        return None;
    };
    match get(f, b"/CFM") {
        Some(Val::Name(b"/V2")) => Some(Cipher::Rc4),
        Some(Val::Name(b"/AESV2")) => Some(Cipher::Aes128),
        Some(Val::Name(b"/AESV3")) => Some(Cipher::Aes256),
        Some(Val::Name(b"/None")) => Some(Cipher::None),
        _ => None,
    }
}

/// The file key for the empty user password, or `None` if the password
/// check fails or the dictionary has a form that Iris does not read.
fn open(d: &[u8], id: &[u8]) -> Option<Crypt> {
    let v = int_of(d, b"/V")?;
    let r = int_of(d, b"/R")?;
    let u = string_of(d, b"/U")?;
    if v == 5 {
        let ue = string_of(d, b"/UE")?;
        if (r != 5 && r != 6)
            || stream_filter(d)? != Cipher::Aes256 && stream_filter(d)? != Cipher::None
        {
            return None;
        }
        let key = open_aes256(r, &u, &ue)?;
        return Some(Crypt {
            cipher: stream_filter(d)?,
            key,
        });
    }
    let o = string_of(d, b"/O")?;
    let p = int_of(d, b"/P")?;
    let (cipher, bytes) = match v {
        1 => (Cipher::Rc4, 5),
        2 => (
            Cipher::Rc4,
            (int_of(d, b"/Length").unwrap_or(40) / 8) as usize,
        ),
        4 => {
            let c = stream_filter(d)?;
            let bytes = match c {
                Cipher::Rc4 => (int_of(d, b"/Length").unwrap_or(128) / 8) as usize,
                _ => 16,
            };
            (c, bytes)
        }
        _ => return None,
    };
    if !(5..=16).contains(&bytes) || o.len() < 32 || u.len() < 32 || !(2..=4).contains(&r) {
        return None;
    }
    if cipher == Cipher::Aes256 {
        return None;
    }
    let metadata = get(d, b"/EncryptMetadata").is_none()
        || direct_name(d, b"/EncryptMetadata", true)
            .and_then(|at| d.get(at + 16..))
            .is_some_and(|rest| rest.trim_ascii_start().starts_with(b"true"));
    // Algorithm 2: the file key.
    let mut h = Md5::new();
    h.update(PAD);
    h.update(&o[..32]);
    h.update((p as i32).to_le_bytes());
    h.update(id);
    if r >= 4 && !metadata {
        h.update([0xFF; 4]);
    }
    let mut key = h.finalize().to_vec();
    if r >= 3 {
        for _ in 0..50 {
            key = Md5::digest(&key[..bytes]).to_vec();
        }
    }
    key.truncate(bytes);
    // Algorithms 4 and 5: the check of the user password.
    let ok = if r == 2 {
        rc4(&key, &PAD) == u[..32]
    } else {
        let mut h = Md5::new();
        h.update(PAD);
        h.update(id);
        let mut x = rc4(&key, &h.finalize());
        for i in 1..=19u8 {
            let k: Vec<u8> = key.iter().map(|b| b ^ i).collect();
            x = rc4(&k, &x);
        }
        x[..] == u[..16]
    };
    ok.then_some(Crypt { cipher, key })
}

/// Algorithms 2.A and 11 for the empty password: the file key of AES-256.
fn open_aes256(r: i64, u: &[u8], ue: &[u8]) -> Option<Vec<u8>> {
    if u.len() < 48 || ue.len() < 32 {
        return None;
    }
    let hash = |salt: &[u8]| -> [u8; 32] {
        if r == 5 {
            Sha256::digest(salt).into()
        } else {
            hash_2b(salt)
        }
    };
    if hash(&u[32..40]) != u[..32] {
        return None;
    }
    let k = hash(&u[40..48]);
    // AES-256 with no padding and an IV of zeros: CBC of 2 blocks.
    let aes = aes::Aes256::new_from_slice(&k).ok()?;
    let mut prev = [0u8; 16];
    let mut out = Vec::with_capacity(32);
    for chunk in ue[..32].chunks(16) {
        let mut block = aes::Block::try_from(chunk).ok()?;
        aes.decrypt_block(&mut block);
        out.extend(block.iter().zip(prev).map(|(a, b)| a ^ b));
        prev.copy_from_slice(chunk);
    }
    Some(out)
}

/// Algorithm 2.B (`/R 6`) for the empty password and no user key data.
fn hash_2b(salt: &[u8]) -> [u8; 32] {
    let mut k: Vec<u8> = Sha256::digest(salt).to_vec();
    let mut round = 0u32;
    loop {
        // The password is empty, so K1 is 64 copies of K.
        let k1: Vec<u8> = k.repeat(64);
        let aes = aes::Aes128::new_from_slice(&k[..16]).expect("16 bytes");
        let mut e = Vec::with_capacity(k1.len());
        let mut prev = [0u8; 16];
        prev.copy_from_slice(&k[16..32]);
        for chunk in k1.chunks(16) {
            let x: Vec<u8> = chunk.iter().zip(prev).map(|(a, b)| a ^ b).collect();
            let mut block = aes::Block::try_from(&x[..]).expect("16 bytes");
            aes.encrypt_block(&mut block);
            prev.copy_from_slice(&block);
            e.extend_from_slice(&block);
        }
        let sum: u32 = e[..16].iter().map(|&b| b as u32).sum();
        k = match sum % 3 {
            0 => Sha256::digest(&e).to_vec(),
            1 => Sha384::digest(&e).to_vec(),
            _ => Sha512::digest(&e).to_vec(),
        };
        round += 1;
        if round >= 64 && u32::from(*e.last().expect("not empty")) <= round - 32 {
            break;
        }
    }
    k[..32].try_into().expect("32 bytes")
}

/// RC4 of `data` with `key` (1 to 32 bytes).
fn rc4(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut out = data.to_vec();
    rc4_in_place(key, &mut out);
    out
}

/// RC4 of `data` with `key` (1 to 32 bytes), in place.
fn rc4_in_place(key: &[u8], data: &mut [u8]) {
    let mut s: [u8; 256] = std::array::from_fn(|i| i as u8);
    let mut j = 0u8;
    for i in 0..256 {
        j = j.wrapping_add(s[i]).wrapping_add(key[i % key.len()]);
        s.swap(i, j as usize);
    }
    let (mut i, mut j) = (0u8, 0u8);
    for b in data {
        i = i.wrapping_add(1);
        j = j.wrapping_add(s[i as usize]);
        s.swap(i as usize, j as usize);
        *b ^= s[s[i as usize].wrapping_add(s[j as usize]) as usize];
    }
}

impl Crypt {
    /// Decrypts the data of the stream of object `num`, generation `gen`,
    /// in place, with no copy. Gives the range of the plain data in `data`,
    /// or `None` for AES data that is not complete blocks or has bad
    /// padding.
    pub(super) fn stream(
        &self,
        num: u32,
        gen: u16,
        data: &mut [u8],
    ) -> Option<std::ops::Range<usize>> {
        match self.cipher {
            Cipher::None => Some(0..data.len()),
            Cipher::Aes256 => aes_cbc(&self.key, data),
            c => {
                // Algorithm 1: the key of the object.
                let mut h = Md5::new();
                h.update(&self.key);
                h.update(&num.to_le_bytes()[..3]);
                h.update(gen.to_le_bytes());
                if c == Cipher::Aes128 {
                    h.update(b"sAlT");
                }
                let k = h.finalize();
                let k = &k[..(self.key.len() + 5).min(16)];
                match c {
                    Cipher::Rc4 => {
                        rc4_in_place(k, data);
                        Some(0..data.len())
                    }
                    _ => aes_cbc(k, data),
                }
            }
        }
    }
}

/// AES-CBC decryption in place, with the IV in the first block and PKCS#7
/// padding. Block `i` of the plain data stays at block `i` of `data`, so
/// the plain data is `16..` less the padding.
fn aes_cbc(key: &[u8], data: &mut [u8]) -> Option<std::ops::Range<usize>> {
    if data.len() < 32 || !data.len().is_multiple_of(16) {
        return None;
    }
    let mut prev: [u8; 16] = data[..16].try_into().ok()?;
    let mut run = |dec: &dyn Fn(&mut aes::Block)| {
        for chunk in data[16..].chunks_mut(16) {
            let cipher: [u8; 16] = (&*chunk).try_into().expect("16 bytes");
            let mut block = aes::Block::from(cipher);
            dec(&mut block);
            for ((o, b), p) in chunk.iter_mut().zip(block.iter()).zip(prev) {
                *o = b ^ p;
            }
            prev = cipher;
        }
    };
    match key.len() {
        16 => {
            let a = aes::Aes128::new_from_slice(key).ok()?;
            run(&|b| a.decrypt_block(b));
        }
        32 => {
            let a = aes::Aes256::new_from_slice(key).ok()?;
            run(&|b| a.decrypt_block(b));
        }
        _ => return None,
    }
    let pad = *data.last()? as usize;
    let end = data.len().checked_sub(pad)?;
    if pad == 0 || pad > 16 || data[end..].iter().any(|&b| b as usize != pad) {
        return None;
    }
    Some(16..end)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Invariant: an `/Encrypt` reference resolves only to the object with
    /// the same number and generation, with any white space between the
    /// tokens (review finding on #1148).
    #[test]
    fn the_encrypt_reference_resolves_its_number_and_generation() {
        let base = include_bytes!("../../tests/pdf/rc4-128.pdf");
        let swap = |pdf: &[u8], from: &str, to: &str| -> Vec<u8> {
            let at = pdf
                .windows(from.len())
                .position(|w| w == from.as_bytes())
                .unwrap();
            let out = [&pdf[..at], to.as_bytes(), &pdf[at + from.len()..]].concat();
            // The swap moves the bytes after it, so it moves the offset that
            // `startxref` gives too when the swap is before the xref table.
            let sx = rfind(&out, b"startxref").unwrap() + 9;
            let text = std::str::from_utf8(&out[sx..]).unwrap();
            let old: usize = text.split_whitespace().next().unwrap().parse().unwrap();
            let moved = if at < old {
                old + to.len() - from.len()
            } else {
                old
            };
            let tail = text.replacen(&old.to_string(), &moved.to_string(), 1);
            [&out[..sx], tail.as_bytes()].concat()
        };
        for gen in ["0", "1", "7", "65535"] {
            for sep in [" ", "\n", "  \r\n"] {
                let head = format!("6{sep}{gen}{sep}obj");
                let pdf = swap(base, "6 0 obj", &head);
                let pdf = swap(&pdf, "/Encrypt 6 0 R", &format!("/Encrypt 6 {gen} R"));
                assert!(matches!(setup(&pdf), Setup::Open(_)), "{gen} {sep:?}");
                // A reference with another generation does not resolve.
                let other = if gen == "0" { "2" } else { "0" };
                let pdf = swap(
                    &pdf,
                    &format!("/Encrypt 6 {gen} R"),
                    &format!("/Encrypt 6 {other} R"),
                );
                assert!(matches!(setup(&pdf), Setup::Locked), "{gen} {sep:?}");
            }
        }
        // `16 0 obj` is not `6 0 obj`.
        assert!(matches!(
            setup(&swap(base, "6 0 obj", "16 0 obj")),
            Setup::Locked
        ));
    }

    #[test]
    fn rc4_gives_the_known_vector() {
        // The test vector "Key" / "Plaintext" of RFC 6229 tables and Wikipedia.
        assert_eq!(
            rc4(b"Key", b"Plaintext"),
            [0xBB, 0xF3, 0x16, 0xE8, 0xD9, 0x40, 0xAF, 0x0A, 0xD3]
        );
    }

    /// A file whose trailer has `dict` as its value of `/Encrypt`.
    fn with_trailer(entry: &str) -> Vec<u8> {
        let mut pdf = b"%PDF-1.4\n".to_vec();
        let at = pdf.len();
        pdf.extend_from_slice(
            format!("xref\n0 1\n0000000000 65535 f \ntrailer\n<< /Size 1 {entry} >>\n").as_bytes(),
        );
        pdf.extend_from_slice(format!("startxref\n{at}\n%%EOF\n").as_bytes());
        pdf
    }

    #[test]
    fn bad_input_fails_closed() {
        for entry in [
            "/Encrypt 9 0 R",
            "/Encrypt << /Filter /Other >>",
            "/Encrypt << /Filter /Standard /V 2 /R 3 >>",
            "/Encrypt << /Filter /Standard /V 5 /R 6 /U <00> /UE <00> >>",
            "/Encrypt << /Filter /Standard /V 9 /R 9 /O <00> /U <00> /P 0 >>",
            "/Encrypt (x)",
        ] {
            assert!(
                matches!(setup(&with_trailer(entry)), Setup::Locked),
                "{entry}"
            );
        }
        assert!(matches!(
            setup(&with_trailer("/EncryptMetadata false")),
            Setup::Plain
        ));
    }

    /// Only the trailer counts: `/Encrypt` text in a content stream or in a
    /// file with no `startxref` does not make the file encrypted.
    #[test]
    fn encrypt_text_outside_the_trailer_does_not_count() {
        let pdf = b"%PDF-1.4\n1 0 obj\n<< /Length 40 >>\nstream\nBT (x /Encrypt 2 0 R y) Tj ET\nendstream\nendobj\n";
        assert!(matches!(setup(pdf), Setup::Plain));
        let mut pdf = with_trailer("");
        pdf.splice(9..9, b"% /Encrypt << /Filter /Other >>\n".iter().copied());
        assert!(matches!(setup(&pdf), Setup::Plain));
    }

    /// Many near matches cost one bounded scan: the setup reads the tail
    /// and the trailer only.
    #[test]
    fn many_near_matches_are_linear() {
        let mut pdf = b"%PDF-1.4\n".to_vec();
        pdf.extend("/EncryptMetadata 1 0 obj ".repeat(400_000).bytes());
        let start = std::time::Instant::now();
        assert!(matches!(setup(&pdf), Setup::Plain));
        let mut pdf = with_trailer("/Encrypt 1 0 R");
        pdf.extend("x1 0 obj ".repeat(1_000_000).bytes());
        let _ = setup(&pdf);
        assert!(
            start.elapsed() < std::time::Duration::from_secs(5),
            "{:?}",
            start.elapsed()
        );
    }
}

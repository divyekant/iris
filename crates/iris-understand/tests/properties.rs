//! Property tests of the parsers of mail bytes: many generated messages,
//! with a fixed seed so that a failure repeats.

use iris_understand::{Head, RAW_LIMIT};

/// A small xorshift generator with a fixed seed.
struct Rng(u64);

impl Rng {
    fn stream(seed: u64, a: u64, _: u64, b: u64) -> Rng {
        Rng((seed + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (a << 32) ^ b)
    }
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
    fn chance(&mut self, per_mille: u64) -> bool {
        self.below(1000) < per_mille
    }
    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len() as u64) as usize]
    }
}

/// A random multipart message. Gives the bytes and, for each part, its
/// type and a marker word in its body.
fn message(rng: &mut Rng) -> (Vec<u8>, Vec<(&'static str, String)>) {
    let types = [
        "text/plain",
        "text/html",
        "text/calendar",
        "image/png",
        "application/zip",
        "application/pdf",
    ];
    let mut raw = String::from("From: a@example.org\r\nSubject: s\r\n");
    raw.push_str("Content-Type: multipart/mixed; boundary=\"b0\"\r\n\r\npre\r\n");
    let mut parts = Vec::new();
    for i in 0..(1 + rng.below(8)) {
        let ty = *rng.pick(&types);
        let marker = format!("mark{i}x{}", rng.below(1_000_000));
        raw.push_str(&format!("--b0\r\nContent-Type: {ty}\r\n\r\n{marker}\r\n"));
        // Sometimes a large body after the marker.
        if rng.chance(300) {
            raw.push_str(&"QUFBQUFB\r\n".repeat(rng.below(120_000) as usize));
        }
        parts.push((ty, marker));
    }
    raw.push_str("--b0--\r\n");
    (raw.into_bytes(), parts)
}

#[test]
fn the_copy_keeps_each_decoded_part_and_stays_bounded() {
    for seed in 0..100 {
        let mut rng = Rng::stream(seed, 0, 0, 7);
        let (raw, parts) = message(&mut rng);
        let copy = iris_understand::bounded_copy(&raw).unwrap();
        assert!(copy.len() as u64 <= RAW_LIMIT, "seed {seed}");
        let text = String::from_utf8_lossy(&copy);
        for (ty, marker) in &parts {
            let kept = text.contains(marker.as_str());
            match *ty {
                t if t.starts_with("text/") => assert!(kept, "seed {seed}: {ty} {marker}"),
                "image/png" | "application/zip" => assert!(!kept, "seed {seed}: {ty}"),
                // A PDF is kept whole when it is small enough.
                _ => {}
            }
        }
        // The copy of the copy is the same: nothing more goes out.
        assert_eq!(
            iris_understand::bounded_copy(&copy).unwrap(),
            copy,
            "seed {seed}"
        );
    }
}

#[test]
fn random_bytes_never_stop_the_parsers() {
    for seed in 0..300 {
        let mut rng = Rng::stream(seed, 1, 0, 9);
        let alphabet: &[u8] =
            b"-:;=\"\r\n \tabcBOUNDARYboundary=Content-Type:text/calendarMETHOD:REQUEST\x00\xff";
        let n = rng.below(4_000) as usize;
        let raw: Vec<u8> = (0..n).map(|_| *rng.pick(alphabet)).collect();
        let copy = iris_understand::bounded_copy(&raw).unwrap();
        assert!(copy.len() <= raw.len(), "seed {seed}");
        let _ = Head::parse(&raw);
        let _ = Head::parse(&copy);
        let _ = iris_clean::clean(&copy);
    }
}

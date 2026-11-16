//! Property tests of the header parser and of `analyze` on random bytes,
//! with a fixed seed so that a failure repeats.

use iris_understand::{analyze, Context, Head, Kind};

/// A small xorshift generator.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Rng {
        Rng((seed + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15))
    }
    fn below(&mut self, n: u64) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 % n.max(1)
    }
    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len() as u64) as usize]
    }
}

const PIECES: &[&str] = &[
    "From: ",
    "To: ",
    "Cc: ",
    "Date: ",
    "Subject: ",
    "List-Id: <a.b>",
    "Content-Type: ",
    "multipart/mixed; boundary=",
    "text/calendar; method=REQUEST",
    "text/plain",
    "Content-Transfer-Encoding: base64",
    "Authentication-Results: mx.google.com; ",
    "dkim=pass header.d=",
    "dmarc=fail header.from=",
    "=?utf-8?B?QW5h?=",
    "=?x?Q?=FF?=",
    "<a@b.c>",
    "\"Lee, Ana\" <A@B.C>",
    "--",
    "z",
    "METHOD:REQUEST",
    "\r\n",
    "\n",
    " ",
    "\t",
    ";",
    ",",
    "\"",
    "(",
    ")",
    "@",
    "\u{e9}",
    "\u{0}",
    "31 Dec 2025 09:00:00 +0000",
    "1 Jan 99999999999 00:00 +9999",
];

#[test]
fn random_mail_never_stops_the_parsers() {
    for seed in 0..500 {
        let mut rng = Rng::new(seed);
        let n = rng.below(120) as usize;
        let mut raw = Vec::new();
        for _ in 0..n {
            raw.extend_from_slice(rng.pick(PIECES).as_bytes());
        }
        if rng.below(4) == 0 {
            raw.push(0xff);
        }
        let h = Head::parse(&raw);
        // Each address is lowercase, as the store keeps it.
        for a in h.to.iter().chain(&h.cc).chain(h.from_addr.iter()) {
            assert_eq!(a, &a.to_ascii_lowercase(), "seed {seed}");
        }
        let lookup = |_: &str| vec!["a@b.c".to_string()];
        let cx = Context {
            me: "owner@example.com",
            kind: Kind::Stranger,
            labels: &[],
            internal_date: None,
            now_ms: 0,
            known_person: &lookup,
            existing: &[],
            thread: &[],
            same_sender: &[],
            known: &[],
        };
        let a = analyze(&raw, &cx);
        let mut sorted = a.marks.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), a.marks.len(), "seed {seed}: a mark twice");
    }
}

/// One decoder for display names: the names of the sender rows
/// (`iris-senders`) and of the header rows (`iris-understand`) agree.
#[test]
fn both_address_parsers_give_the_same_decoded_name() {
    for seed in 0..2000 {
        let mut rng = Rng::new(seed);
        let n = rng.below(12) as usize;
        let mut v = String::new();
        for _ in 0..n {
            v.push_str(rng.pick(PIECES));
        }
        let ours = iris_understand::head::addresses(&v);
        for (a, name) in iris_senders::head::addresses(&v) {
            let Some(name) = name else { continue };
            assert!(!name.contains("=?utf-8?B?QW5h?="), "seed {seed}: {v:?}");
            if ours.iter().any(|(b, _)| *b == a) {
                let pair = (a, Some(name));
                assert!(ours.contains(&pair), "seed {seed}: {v:?} {pair:?}");
            }
        }
    }
}

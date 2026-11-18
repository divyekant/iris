//! Short IDs (specification 1, section 5.7): a prefix and the row ID, as `m-42`.

/// The object that an ID names. `Commitment` to `Signal` are rows of the
/// table `item`, with the form of the same name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Message,
    Thread,
    Sender,
    Commitment,
    Conversation,
    Record,
    Feed,
    Signal,
    /// A row of the table `watch` (card R5-07).
    Watch,
    /// A row of the table `matter` (card R6-01).
    Matter,
    /// A row of the table `notice` (card R5-08).
    Notice,
    /// A draft that Iris made at Gmail (table `draft`).
    Draft,
    /// A mailbox (table `account`).
    Account,
}

const PREFIXES: [(Kind, &str); 13] = [
    (Kind::Message, "m"),
    (Kind::Thread, "t"),
    (Kind::Sender, "p"),
    (Kind::Commitment, "c"),
    (Kind::Conversation, "v"),
    (Kind::Record, "r"),
    (Kind::Feed, "f"),
    (Kind::Signal, "s"),
    (Kind::Watch, "w"),
    (Kind::Matter, "g"),
    (Kind::Notice, "n"),
    (Kind::Draft, "d"),
    (Kind::Account, "a"),
];

pub fn format_id(kind: Kind, row: i64) -> String {
    let (_, p) = PREFIXES
        .iter()
        .find(|(k, _)| *k == kind)
        .expect("each kind has a prefix");
    format!("{p}-{row}")
}

/// Parse an ID such as `m-42`. Gives `None` for an unknown prefix, or a
/// number that is not a positive decimal row ID.
pub fn parse_id(text: &str) -> Option<(Kind, i64)> {
    let (p, n) = text.split_once('-')?;
    let (kind, _) = PREFIXES.iter().find(|(_, q)| *q == p)?;
    // Only digits: `i64::from_str` also takes a sign, and "+5" is not an ID.
    if n.is_empty() || n.starts_with('0') || !n.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let row: i64 = n.parse().ok()?;
    (row > 0).then_some((*kind, row))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        for (kind, _) in PREFIXES {
            assert_eq!(parse_id(&format_id(kind, 42)), Some((kind, 42)));
        }
        assert_eq!(format_id(Kind::Sender, 7), "p-7");
    }

    /// Property: a text parses only if it is the canonical form that
    /// `format_id` gives for its row (#309).
    #[test]
    fn only_the_canonical_form_parses() {
        let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
        let alphabet = b"mtp-0123456789+ x";
        for _ in 0..20_000 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            let len = (x % 8) as usize;
            let text: String = (0..len)
                .map(|i| alphabet[((x >> (i * 5)) % alphabet.len() as u64) as usize] as char)
                .collect();
            match parse_id(&text) {
                Some((kind, row)) => assert_eq!(format_id(kind, row), text),
                None => assert!(PREFIXES.iter().all(|(k, _)| {
                    text.split_once('-')
                        .and_then(|(_, n)| n.parse::<i64>().ok())
                        .is_none_or(|n| n <= 0 || format_id(*k, n) != text)
                })),
            }
        }
    }

    #[test]
    fn rejects_bad_ids() {
        for bad in [
            "",
            "m",
            "m-",
            "m-0",
            "m-+5",
            "m--5",
            "x-1",
            "m-1a",
            "M-1",
            "m-99999999999999999999",
            "m-01",
            "cs-007",
            "t-00",
        ] {
            assert_eq!(parse_id(bad), None, "{bad}");
        }
    }
}

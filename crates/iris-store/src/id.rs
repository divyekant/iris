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
}

const PREFIXES: [(Kind, &str); 10] = [
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
    if n.is_empty() || !n.bytes().all(|b| b.is_ascii_digit()) {
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
        ] {
            assert_eq!(parse_id(bad), None, "{bad}");
        }
    }
}

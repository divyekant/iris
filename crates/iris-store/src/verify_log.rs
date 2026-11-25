//! The text of a verify row of `sync_log`. `iris_sync::verify` writes it
//! with [`line`], and the measures read it with [`read`], so a change of
//! the text changes both (issue #473).

/// The counts of one verify row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Verify {
    /// The differences that the check found, or `None` if the text has no count.
    pub differences: Option<i64>,
    /// The messages that changed during the check and were not compared.
    pub skipped: Option<i64>,
    /// True if an error stopped the check.
    pub stopped: bool,
    /// True only with no error and 0 skipped messages: a skipped message
    /// was not compared.
    pub complete: bool,
}

/// The first line of the row: "N differences, M skipped", then
/// ", then error: E" when an error stopped the check. The caller adds the
/// listed differences on the next lines.
pub fn line(differences: u64, skipped: u64, error: Option<&str>) -> String {
    let mut text = format!("{differences} differences, {skipped} skipped");
    if let Some(e) = error {
        text.push_str(", then error: ");
        text.push_str(e);
    }
    text
}

/// The counts of the text of a verify row.
pub fn read(text: &str) -> Verify {
    let first = text.lines().next().unwrap_or("");
    let differences = first
        .split_once(" differences")
        .and_then(|(n, _)| n.parse().ok());
    let skipped = first
        .split_once(" skipped")
        .and_then(|(n, _)| n.rsplit(' ').next()?.parse::<i64>().ok());
    let none_skipped = skipped == Some(0) || !first.contains(" skipped");
    let stopped = first.contains(", then error");
    Verify {
        differences,
        skipped,
        stopped,
        complete: none_skipped && !stopped,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_gives_the_counts_of_each_text() {
        let v = |differences, skipped, stopped, complete| Verify {
            differences,
            skipped,
            stopped,
            complete,
        };
        assert_eq!(
            read("0 differences, 0 skipped"),
            v(Some(0), Some(0), false, true)
        );
        assert_eq!(
            read("0 differences, 2 skipped"),
            v(Some(0), Some(2), false, false)
        );
        assert_eq!(
            read("3 differences, 1 skipped\nmessage a: other labels"),
            v(Some(3), Some(1), false, false)
        );
        assert_eq!(
            read("0 differences, x skipped"),
            v(Some(0), None, false, false)
        );
        assert_eq!(
            read("2 differences, 0 skipped, then error: HTTP 401"),
            v(Some(2), Some(0), true, false)
        );
        assert_eq!(
            read("0 differences, 4 skipped, then error: HTTP 401"),
            v(Some(0), Some(4), true, false)
        );
        assert_eq!(read(""), v(None, None, false, true));
        assert_eq!(read("x differences"), v(None, None, false, true));
    }

    /// Property: [`read`] gives back the counts and the stop of each text
    /// that [`line`] writes, also with an error text that holds the words
    /// of the format, and with the listed differences on the next lines.
    #[test]
    fn read_gives_back_what_line_writes() {
        let errors = [
            None,
            Some("HTTP 401"),
            Some("7 differences, 9 skipped"),
            Some(""),
            Some("a\nb skipped"),
        ];
        let mut x: u64 = 0x9e37_79b9_7f4a_7c15;
        for i in 0..2_000u64 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            let d = if i % 3 == 0 { x % 10 } else { x >> 40 };
            let s = if i % 2 == 0 { 0 } else { (x >> 20) % 1_000 };
            let e = errors[(x % errors.len() as u64) as usize];
            let mut text = line(d, s, e);
            text.push_str("\nmessage m: 1 differences, 2 skipped");
            let got = read(&text);
            assert_eq!(got.differences, Some(d as i64), "{text}");
            assert_eq!(got.skipped, Some(s as i64), "{text}");
            assert_eq!(got.stopped, e.is_some(), "{text}");
            assert_eq!(got.complete, s == 0 && e.is_none(), "{text}");
        }
    }
}

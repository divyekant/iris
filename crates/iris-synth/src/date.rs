//! RFC 5322 dates from Unix seconds, in UTC, with no dependency.

const DAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// For example `Thu, 01 Jan 2026 00:00:00 +0000`.
pub fn rfc5322(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, m, d) = civil(days);
    format!(
        "{}, {:02} {} {} {:02}:{:02}:{:02} +0000",
        DAYS[days.rem_euclid(7) as usize],
        d,
        MONTHS[m as usize - 1],
        y,
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// For example `2026-01-01`.
pub fn ymd(secs: i64) -> String {
    let (y, m, d) = civil(secs.div_euclid(86_400));
    format!("{y:04}-{m:02}-{d:02}")
}

/// The first time after `secs`, on a later day, that falls on `weekday`
/// (0 is Monday). A Monday message that says "by Monday" means the next one.
pub fn next_weekday(secs: i64, weekday: i64) -> i64 {
    // Day 0 (1970-01-01) was a Thursday, so Monday is 4.
    let day = secs.div_euclid(86_400);
    let ahead = (weekday + 4 - day).rem_euclid(7);
    (day + if ahead == 0 { 7 } else { ahead }) * 86_400
}

/// Days since 1970-01-01 to (year, month, day). H. Hinnant's algorithm.
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

#[cfg(test)]
mod tests {
    #[test]
    fn known_dates() {
        assert_eq!(super::rfc5322(0), "Thu, 01 Jan 1970 00:00:00 +0000");
        assert_eq!(
            super::rfc5322(1_767_225_600),
            "Thu, 01 Jan 2026 00:00:00 +0000"
        );
        assert_eq!(
            super::rfc5322(1_709_210_096),
            "Thu, 29 Feb 2024 12:34:56 +0000"
        );
        assert_eq!(super::ymd(1_709_210_096), "2024-02-29");
        // Thursday 2024-02-29: the next Friday is 03-01, the next Thursday 03-07.
        assert_eq!(
            super::ymd(super::next_weekday(1_709_210_096, 4)),
            "2024-03-01"
        );
        assert_eq!(
            super::ymd(super::next_weekday(1_709_210_096, 3)),
            "2024-03-07"
        );
        assert_eq!(
            super::ymd(super::next_weekday(1_709_210_096, 0)),
            "2024-03-04"
        );
    }
}

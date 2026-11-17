//! UTC dates with no dependency, after Howard Hinnant's date algorithms.

/// Days since 1970-01-01 of a civil date.
pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

pub fn civil(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// True if the date and the time exist: the day is inside its month (with
/// leap years), the hour, the minute and the second (60 for a leap second)
/// are inside their range.
pub fn valid(y: i64, m: i64, d: i64, h: i64, mi: i64, s: i64) -> bool {
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let days = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    (1..=days).contains(&d)
        && (0..=23).contains(&h)
        && (0..=59).contains(&mi)
        && (0..=60).contains(&s)
}

/// RFC 3339 in UTC, for example `2026-11-02T10:00:00Z`, with milliseconds
/// when they are not zero (`2026-11-02T10:00:00.250Z`).
pub fn rfc3339(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let (y, m, d) = civil(secs.div_euclid(86_400));
    let r = secs.rem_euclid(86_400);
    let frac = match ms.rem_euclid(1000) {
        0 => String::new(),
        f => format!(".{f:03}"),
    };
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}{frac}Z",
        r / 3600,
        r % 3600 / 60,
        r % 60
    )
}

/// An RFC 3339 date-time, for example `2026-11-02T10:00:00Z` or
/// `2026-11-02T11:00:00.5+01:00`, in milliseconds since the Unix epoch.
pub fn parse_rfc3339(v: &str) -> Option<i64> {
    let b = v.as_bytes();
    let num = |r: std::ops::Range<usize>| -> Option<i64> {
        let s = v.get(r)?;
        s.bytes()
            .all(|c| c.is_ascii_digit())
            .then(|| s.parse().ok())?
    };
    if b.len() < 20
        || b[4] != b'-'
        || b[7] != b'-'
        || !matches!(b[10], b'T' | b't')
        || b[13] != b':'
        || b[16] != b':'
    {
        return None;
    }
    let (y, mo, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (h, mi, s) = (num(11..13)?, num(14..16)?, num(17..19)?);
    if !valid(y, mo, d, h, mi, s) {
        return None;
    }
    let mut i = 19;
    // Milliseconds from the first 3 digits of a fraction; more digits are
    // below a millisecond.
    let mut frac = 0;
    if b[i] == b'.' {
        i += 1;
        let start = i;
        while i < b.len() && b[i].is_ascii_digit() {
            if i - start < 3 {
                frac = frac * 10 + i64::from(b[i] - b'0');
            }
            i += 1;
        }
        if i == start {
            return None;
        }
        for _ in (i - start)..3 {
            frac *= 10;
        }
    }
    let off = match b.get(i)? {
        b'Z' | b'z' if i + 1 == b.len() => 0,
        c @ (b'+' | b'-') if i + 6 == b.len() && b[i + 3] == b':' => {
            let (oh, om) = (num(i + 1..i + 3)?, num(i + 4..i + 6)?);
            if oh > 23 || om > 59 {
                return None;
            }
            let m = oh * 60 + om;
            if *c == b'-' {
                -m
            } else {
                m
            }
        }
        _ => return None,
    };
    let days = days_from_civil(y, mo, d);
    Some((days * 86_400 + h * 3600 + mi * 60 + s - off * 60) * 1000 + frac)
}

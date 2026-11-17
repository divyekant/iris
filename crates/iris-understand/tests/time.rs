//! Tests of the RFC 3339 dates and of the calendar check.

use iris_understand::time;

#[test]
fn round_trip() {
    assert_eq!(time::rfc3339(0), "1970-01-01T00:00:00Z");
    assert_eq!(time::rfc3339(1_725_070_141_000), "2024-08-31T02:09:01Z");
    let d = time::days_from_civil(2024, 2, 29);
    assert_eq!(time::civil(d), (2024, 2, 29));
    assert_eq!(time::parse_rfc3339("1970-01-01T01:00:00+01:00"), Some(0));
    assert_eq!(
        time::parse_rfc3339("2024-08-31T02:09:01.25Z"),
        Some(1_725_070_141_250)
    );
    assert_eq!(time::rfc3339(1_725_070_141_250), "2024-08-31T02:09:01.250Z");
    assert_eq!(time::parse_rfc3339("2024-08-31T02:09:01.Z"), None);
    assert_eq!(time::parse_rfc3339("2024-08-31 02:09:01"), None);
    assert_eq!(time::parse_rfc3339("2025-02-31T00:00:00Z"), None);
    assert_eq!(time::parse_rfc3339("2023-02-29T00:00:00Z"), None);
    assert!(time::parse_rfc3339("2024-02-29T00:00:00Z").is_some());
    assert_eq!(time::parse_rfc3339("2025-01-01T00:00:00+24:00"), None);
    assert_eq!(time::parse_rfc3339("2025-01-01T00:00:00+01:60"), None);
}

/// Each time in milliseconds goes to text and back with no change.
#[test]
fn round_trip_property() {
    let mut x: u64 = 0x2545_F491_4F6C_DD1D;
    for _ in 0..10_000 {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        // Years 1 to 9999.
        let ms = (x % 315_537_897_600_000) as i64 - 62_135_596_800_000;
        assert_eq!(time::parse_rfc3339(&time::rfc3339(ms)), Some(ms), "{ms}");
    }
}

/// Each day number that `valid` accepts maps back to the same date, and
/// no impossible day is accepted.
#[test]
fn valid_agrees_with_the_calendar() {
    for y in [1900, 2000, 2023, 2024, 2100] {
        for m in 1..=12 {
            for d in 1..=31 {
                let ok = time::valid(y, m, d, 0, 0, 0);
                let back = time::civil(time::days_from_civil(y, m, d)) == (y, m, d);
                assert_eq!(ok, back, "{y}-{m}-{d}");
            }
        }
    }
}

//! Pure-integer proleptic-Gregorian calendar math.
//!
//! All arithmetic uses `i64` and wraps the same way PHP's timelib does for
//! extreme years, so out-of-range inputs (e.g. `10000-01-01`,
//! `-292277022657-...`) produce timestamps matching PHP. Algorithms are Howard
//! Hinnant's `days_from_civil` / `civil_from_days` (the same routine used by
//! the Go reference's `phpEpochDays`).

/// Days from the Unix epoch (1970-01-01) to the given civil date.
///
/// Month is 1..=12 here; callers normalize out-of-range months first. Works for
/// any year. Mirrors `phpEpochDays` in the Go reference.
pub const fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let m = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * m + 2) / 5 + day - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146097 + doe - 719468
}

/// Inverse of [`days_from_civil`]: returns `(year, month, day)` for a day count
/// since the Unix epoch. Month is 1..=12.
pub const fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let year = if m <= 2 { y + 1 } else { y };
    (year, m, d)
}

/// Unix timestamp (seconds) for a civil UTC date-time. Components may be any
/// `i64`; they are combined with two's-complement wrapping so extreme years
/// overflow exactly as PHP's `int64` arithmetic does (e.g. the documented
/// `i64::MIN` wrap-around cases).
pub const fn unix_from_civil(
    year: i64,
    month: i64,
    day: i64,
    hour: i64,
    minute: i64,
    second: i64,
) -> i64 {
    days_from_civil(year, month, day)
        .wrapping_mul(86400)
        .wrapping_add(hour.wrapping_mul(3600))
        .wrapping_add(minute.wrapping_mul(60))
        .wrapping_add(second)
}

/// Day of week for a day count since the epoch. 0 = Sunday .. 6 = Saturday
/// (matching Go's `time.Weekday` and PHP). 1970-01-01 was a Thursday.
pub const fn weekday_from_days(z: i64) -> i64 {
    (z + 4).rem_euclid(7)
}

/// Whether `year` is a leap year in the proleptic Gregorian calendar.
pub const fn is_leap_year(year: i64) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

/// Number of days in `month` (1..=12) of `year`. Returns 0 for out-of-range
/// months.
pub const fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap_year(year) {
                29
            } else {
                28
            }
        }
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_roundtrip() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(weekday_from_days(0), 4); // Thursday
    }

    #[test]
    fn known_dates() {
        // 2000-01-01 12:00:00 UTC = 946728000 (from the CSV).
        assert_eq!(unix_from_civil(2000, 1, 1, 12, 0, 0), 946728000);
        // 2000-01-01 was a Saturday.
        assert_eq!(weekday_from_days(days_from_civil(2000, 1, 1)), 6);
    }

    #[test]
    fn roundtrip_range() {
        for z in [-1_000_000i64, -719468, -1, 0, 1, 730000, 2_000_000] {
            let (y, m, d) = civil_from_days(z);
            assert_eq!(days_from_civil(y, m, d), z, "roundtrip for z={z}");
        }
    }

    #[test]
    fn leap_and_dim() {
        assert!(is_leap_year(2000));
        assert!(!is_leap_year(1900));
        assert!(is_leap_year(2024));
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2023, 2), 28);
        assert_eq!(days_in_month(2023, 4), 30);
    }
}

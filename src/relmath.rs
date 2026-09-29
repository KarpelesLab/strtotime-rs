//! Relative time arithmetic on a [`Moment`], matching PHP/Go semantics.
//!
//! Port of the arithmetic in the Go reference's `helpers.go`. Day/week additions
//! use wall-clock arithmetic but fall *forward* across DST gaps (PHP behavior);
//! hour/minute/second additions are wall-clock; month/year use calendar
//! arithmetic with day carry.

use crate::civil;
use crate::datetime::Civil;
use crate::lookups::Unit;
use crate::tz::Moment;

/// Add to the wall-clock date fields then re-resolve in the zone (Go's
/// `AddDate`). Day overflow carries (e.g. Jan 31 + 1 month → Mar 3).
fn add_date(m: Moment, dy: i64, dmo: i64, dd: i64) -> Moment {
    let w = m.wall();
    let c = Civil::new(
        w.year + dy,
        w.month as i64 + dmo,
        w.day as i64 + dd,
        w.hour as i64,
        w.minute as i64,
        w.second as i64,
    );
    Moment::from_civil_frac(m.tz, c, m.micros)
}

/// Add `secs` to the instant directly (duration arithmetic).
fn add_duration(m: Moment, secs: i64) -> Moment {
    Moment {
        unix: m.unix + secs,
        tz: m.tz,
        micros: m.micros,
    }
}

/// Add `n` calendar days with PHP DST handling: preserve wall-clock time, but if
/// the result lands in a spring-forward gap (wrong day or shifted clock), fall
/// forward using duration arithmetic. Mirrors `addDaysPHP`.
pub(crate) fn add_days_php(m: Moment, n: i64) -> Moment {
    let w = m.wall();
    let result = add_date(m, 0, 0, n);
    let rw = result.wall();

    // Expected calendar date had no DST interference.
    let want_days = civil::days_from_civil(w.year, w.month as i64, w.day as i64) + n;
    let (wy, wm, wd) = civil::civil_from_days(want_days);
    if rw.year != wy || rw.month as i64 != wm || rw.day as i64 != wd {
        return add_duration(m, n * 86400);
    }
    if rw.hour != w.hour || rw.minute != w.minute || rw.second != w.second {
        return add_duration(m, n * 86400);
    }
    result
}

/// Add `n` business days (Mon–Fri). From a weekend with `n == 0`, snap to the
/// next Monday. Mirrors `addWeekdays`.
pub(crate) fn add_weekdays(m: Moment, n: i64) -> Moment {
    let wd = m.wall().weekday();
    if n == 0 {
        return match wd {
            6 => add_date(m, 0, 0, 2), // Saturday → Monday
            0 => add_date(m, 0, 0, 1), // Sunday → Monday
            _ => m,
        };
    }

    add_date(m, 0, 0, weekday_delta(wd as i64, n))
}

/// Calendar days from a day with weekday `wd` (0 = Sunday) to the `n`-th
/// business day after it (before it, for negative `n`), `n != 0`.
///
/// Closed form, so the cost is independent of `n` (stepping day by day let a
/// short input like "99999999999 weekdays" run for hours). Resolving the wall
/// time once, at the target date, also matches PHP across DST gaps.
fn weekday_delta(wd: i64, n: i64) -> i64 {
    // Weekend starts count from the adjacent business day in the direction of
    // travel: the n-th business day after Sat/Sun is the n-th after Friday, the
    // n-th before Sat/Sun is the n-th before Monday.
    let (adjust, k) = match (wd, n > 0) {
        (6, true) => (-1, 4),
        (0, true) => (-2, 4),
        (6, false) => (2, 0),
        (0, false) => (1, 0),
        _ => (0, wd - 1), // Mon = 0 .. Fri = 4
    };
    let target = k + n; // business-day index from the Monday of `k`'s week
    adjust + target.div_euclid(5) * 7 + target.rem_euclid(5) - k
}

/// Apply `amount` units of `unit` to `m`. Mirrors `applyTimeOffset`.
pub(crate) fn apply_offset(m: Moment, amount: i64, unit: Unit) -> Moment {
    match unit {
        Unit::Day => add_days_php(m, amount),
        Unit::Week => add_days_php(m, amount * 7),
        Unit::Weekday => add_weekdays(m, amount),
        Unit::Month => add_date(m, 0, amount, 0),
        Unit::Year => add_date(m, amount, 0, 0),
        Unit::Hour => add_clock(m, amount, 0, 0),
        Unit::Minute => add_clock(m, 0, amount, 0),
        Unit::Second => add_clock(m, 0, 0, amount),
    }
}

/// Add to the wall-clock time fields then re-resolve (Go's hour/min/sec via
/// `time.Date`).
fn add_clock(m: Moment, dh: i64, dmi: i64, ds: i64) -> Moment {
    let w = m.wall();
    let c = Civil::new(
        w.year,
        w.month as i64,
        w.day as i64,
        w.hour as i64 + dh,
        w.minute as i64 + dmi,
        w.second as i64 + ds,
    );
    Moment::from_civil_frac(m.tz, c, m.micros)
}

#[cfg(test)]
mod tests {
    use crate::relmath::weekday_delta;

    /// The closed form agrees with stepping one day at a time.
    #[test]
    fn weekday_delta_matches_stepping() {
        for wd in 0..7 {
            for n in -40i64..=40 {
                if n == 0 {
                    continue;
                }
                let step = n.signum();
                let (mut day, mut left) = (0i64, n.abs());
                while left > 0 {
                    day += step;
                    if !matches!((wd + day).rem_euclid(7), 0 | 6) {
                        left -= 1;
                    }
                }
                assert_eq!(weekday_delta(wd, n), day, "wd={wd} n={n}");
            }
        }
    }
}

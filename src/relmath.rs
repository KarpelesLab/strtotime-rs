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
    Moment::from_civil(m.tz, c)
}

/// Add `secs` to the instant directly (duration arithmetic).
fn add_duration(m: Moment, secs: i64) -> Moment {
    Moment { unix: m.unix + secs, tz: m.tz }
}

/// Add `n` calendar days with PHP DST handling: preserve wall-clock time, but if
/// the result lands in a spring-forward gap (wrong day or shifted clock), fall
/// forward using duration arithmetic. Mirrors `addDaysPHP`.
pub fn add_days_php(m: Moment, n: i64) -> Moment {
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
pub fn add_weekdays(m: Moment, n: i64) -> Moment {
    let wd = m.wall().weekday();
    if n == 0 {
        return match wd {
            6 => add_date(m, 0, 0, 2), // Saturday → Monday
            0 => add_date(m, 0, 0, 1), // Sunday → Monday
            _ => m,
        };
    }

    let (step, count) = if n < 0 { (-1, -n) } else { (1, n) };
    let mut result = m;
    for _ in 0..count {
        result = add_date(result, 0, 0, step);
        while matches!(result.wall().weekday(), 0 | 6) {
            result = add_date(result, 0, 0, step);
        }
    }
    result
}

/// Apply `amount` units of `unit` to `m`. Mirrors `applyTimeOffset`.
pub fn apply_offset(m: Moment, amount: i64, unit: Unit) -> Moment {
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
    Moment::from_civil(m.tz, c)
}

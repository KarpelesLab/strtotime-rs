//! ISO 8601 formats — port of `iso8601.go`.
//!
//! Handles week dates (`2023-W03`, `2023-W03-1`, `2023W031`) and datetimes with
//! a `T` separator (`2023-01-15T14:30:00`, `20060212T231223`), including numeric
//! offsets and named-timezone suffixes.

use crate::civil;
use crate::parsers::formats::{atoi, is_all_digits, mk, mk_frac, parse_iso, parse_iso8601_time};
use crate::tz::{self, Moment, Tz};

/// Entry point: try week date, then `T` datetime. Mirrors `parseISO8601`.
pub fn parse_iso8601(s: &str, base: Moment) -> Option<Moment> {
    if let Some(m) = parse_iso_week_date(s, base) {
        return Some(m);
    }
    parse_iso8601_datetime(s, base)
}

/// `<date>T<time>[offset]`. Mirrors `parseISO8601DateTime`.
fn parse_iso8601_datetime(s: &str, base: Moment) -> Option<Moment> {
    let b = s.as_bytes();
    // Find a 'T'/'t' flanked by digits.
    let mut t_idx = None;
    let mut i = 1;
    while i + 1 < b.len() {
        if (b[i] == b't' || b[i] == b'T') && b[i - 1].is_ascii_digit() && b[i + 1].is_ascii_digit() {
            t_idx = Some(i);
            break;
        }
        i += 1;
    }
    let t_idx = t_idx?;
    let date_part = &s[..t_idx];
    let rest = &s[t_idx + 1..];

    let (year, month, day);
    if date_part.contains('-') {
        let d = parse_iso(date_part, base)?;
        let w = d.wall();
        year = w.year;
        month = w.month as i64;
        day = w.day as i64;
    } else if date_part.len() >= 8 && is_all_digits(date_part) {
        let n = date_part.len();
        year = atoi(&date_part[..n - 4]);
        month = atoi(&date_part[n - 4..n - 2]);
        day = atoi(&date_part[n - 2..]);
        if !crate::parsers::token_parser::is_valid_date(year, month, day) {
            return None;
        }
    } else {
        return None;
    }

    let (hour, minute, second, micros, consumed) = parse_iso8601_time(rest)?;

    let mut tz = base.tz;
    let tz_rest = rest[consumed..].trim_start_matches(' ');
    if !tz_rest.is_empty() {
        if let Some((off, c)) = tz::parse_numeric_offset(tz_rest) {
            if !tz_rest[c..].trim().is_empty() {
                return None;
            }
            tz = if off == 0 { Tz::Utc } else { Tz::Fixed(off) };
        } else if let Some(t) = tz::parse_timezone(tz_rest) {
            tz = t;
        } else {
            return None;
        }
    }

    if hour == 24 {
        return Some(mk_frac(tz, year, month, day + 1, 0, minute, second, micros));
    }
    Some(mk_frac(tz, year, month, day, hour, minute, second, micros))
}

/// `YYYY-Www`, `YYYY-Www-D`, `YYYYWww`, `YYYYWwwD`. Mirrors `parseISOWeekDate`.
fn parse_iso_week_date(s: &str, base: Moment) -> Option<Moment> {
    let b = s.as_bytes();
    // Find 'w'/'W' preceded by a digit, or by '-' that is itself preceded by a digit.
    let mut w_idx = None;
    let mut i = 1;
    while i < b.len() {
        if b[i] == b'w' || b[i] == b'W' {
            let prev = b[i - 1];
            if prev.is_ascii_digit() {
                w_idx = Some(i);
                break;
            }
            if prev == b'-' && i >= 2 && b[i - 2].is_ascii_digit() {
                w_idx = Some(i);
                break;
            }
        }
        i += 1;
    }
    let w_idx = w_idx?;

    let year_part = s[..w_idx].strip_suffix('-').unwrap_or(&s[..w_idx]);
    if !is_all_digits(year_part) {
        return None;
    }
    let year = atoi(year_part);
    if year < 1 {
        return None;
    }

    let rest = &s[w_idx + 1..];
    let rb = rest.as_bytes();
    // Week number: up to 2 digits, but PHP requires exactly 2.
    let mut k = 0;
    while k < rb.len() && k < 2 && rb[k].is_ascii_digit() {
        k += 1;
    }
    if k < 2 {
        return None;
    }
    let week = atoi(&rest[..k]);
    if !(1..=53).contains(&week) {
        return None;
    }

    // Optional day-of-week.
    let mut rest = &rest[k..];
    let mut day = 1i64; // Monday
    if !rest.is_empty() {
        if rest.as_bytes()[0] == b'-' {
            rest = &rest[1..];
        }
        let rb = rest.as_bytes();
        if !rb.is_empty() && (b'0'..=b'7').contains(&rb[0]) {
            day = (rb[0] - b'0') as i64;
            rest = &rest[1..];
        } else if !rb.is_empty() && rb[0].is_ascii_digit() {
            // Day >= 8: PHP treats the digit as a timezone offset (UTC-h).
            let h = (rb[0] - b'0') as i64;
            if rest.len() > 1 {
                return None;
            }
            let (ty, tmo, td) = iso_week_target(year, week, 1, base.tz);
            let tz = Tz::Fixed((-h * 3600) as i32);
            return Some(mk(tz, ty, tmo, td, 0, 0, 0));
        }
        if !rest.is_empty() {
            return None;
        }
    }

    let (ty, tmo, td) = iso_week_target(year, week, day, base.tz);
    Some(mk(base.tz, ty, tmo, td, 0, 0, 0))
}

/// Calendar date for ISO (year, week, day-of-week 1..7). Week 1 contains Jan 4.
fn iso_week_target(year: i64, week: i64, day: i64, _tz: Tz) -> (i64, i64, i64) {
    let jan4 = civil::days_from_civil(year, 1, 4);
    let mut iso_wd = civil::weekday_from_days(jan4); // 0=Sun..6=Sat
    if iso_wd == 0 {
        iso_wd = 7;
    }
    let week1_monday = jan4 - (iso_wd - 1);
    let target = week1_monday + (week - 1) * 7 + (day - 1);
    civil::civil_from_days(target)
}

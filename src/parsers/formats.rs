//! Format parsers tried before the token parser, and the pipeline that runs
//! them in order. Port of `date_formats.go` (with the remaining format files
//! filled in over later phases) and the `formatParsers` list in `strtotime.go`.
//!
//! Each parser takes the trimmed input and the base [`Moment`] (for the zone and
//! "now"), and returns `Some(moment)` on a match.

use crate::civil;
use crate::datetime::Civil;
use crate::lookups::{apply_ampm, two_digit_year};
use crate::parsers::token_parser::is_valid_date;
use crate::tz::{self, Moment, Tz};

/// Build a moment from civil wall fields in a zone.
fn mk(tz: Tz, y: i64, mo: i64, d: i64, h: i64, mi: i64, s: i64) -> Moment {
    Moment::from_civil(tz, Civil::new(y, mo, d, h, mi, s))
}

/// Non-empty and all ASCII digits.
fn is_all_digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

fn atoi(s: &str) -> i64 {
    let mut n = 0i64;
    for b in s.bytes() {
        n = n * 10 + (b - b'0') as i64;
    }
    n
}

/// Count occurrences of byte `sep` in `s`.
fn count(s: &str, sep: u8) -> usize {
    s.bytes().filter(|b| *b == sep).count()
}

/// Split `s` on `sep` into exactly three parts (requires exactly two separators).
fn split3(s: &str, sep: u8) -> Option<(&str, &str, &str)> {
    let mut it = s.match_indices(sep as char);
    let (a, _) = it.next()?;
    let (b, _) = it.next()?;
    if it.next().is_some() {
        return None;
    }
    Some((&s[..a], &s[a + 1..b], &s[b + 1..]))
}

// ---------------------------------------------------------------------------
// Pipeline
// ---------------------------------------------------------------------------

/// Run the ordered format parsers; returns the first match. Mirrors the
/// `formatParsers` list in `strtotime.go`. Parsers from later phases are added
/// in their correct positions as they land.
pub fn pipeline(s: &str, base: Moment) -> Option<Moment> {
    let first = s.as_bytes().first().copied().unwrap_or(0);
    let digit = first.is_ascii_digit();

    if digit {
        if let Some(m) = parse_european(s, base) {
            return Some(m);
        }
    }
    if s.starts_with("0000-00-00") {
        if let Some(m) = parse_zero_date(s, base) {
            return Some(m);
        }
    }
    if first == b'-' || first == b'+' {
        if let Some(m) = parse_signed_year(s, base) {
            return Some(m);
        }
    }
    if let Some(m) = parse_datetime(s, base) {
        return Some(m);
    }
    if let Some(m) = parse_iso(s, base) {
        return Some(m);
    }
    if digit {
        if let Some(m) = parse_large_year_as_time(s, base) {
            return Some(m);
        }
        if let Some(m) = parse_year_month(s, base) {
            return Some(m);
        }
        if let Some(m) = parse_slash(s, base) {
            return Some(m);
        }
        if let Some(m) = parse_us(s, base) {
            return Some(m);
        }
        if let Some(m) = parse_short_year_us_military(s, base) {
            return Some(m);
        }
    }

    None
}

// ---------------------------------------------------------------------------
// Individual parsers
// ---------------------------------------------------------------------------

/// `YYYY-MM-DD` or `D-M-YYYY` (and 2-digit-year variants). Mirrors `parseISOFormat`.
pub fn parse_iso(s: &str, base: Moment) -> Option<Moment> {
    if count(s, b'-') != 2 {
        return None;
    }
    let (p0, p1, p2) = split3(s, b'-')?;
    if !is_all_digits(p0) || !is_all_digits(p1) || !is_all_digits(p2) {
        return None;
    }
    let (first, second, third) = (atoi(p0), atoi(p1), atoi(p2));
    let (year, month, day);
    if p0.len() >= 4 {
        year = first;
        month = second;
        day = third;
        if year > 9999 {
            return None;
        }
    } else if p2.len() >= 4 {
        day = first;
        month = second;
        year = third;
    } else {
        year = if first < 100 { two_digit_year(first) } else { first };
        month = second;
        day = third;
    }
    if !is_valid_date(year, month, day) {
        return None;
    }
    Some(mk(base.tz, year, month, day, 0, 0, 0))
}

/// `YYYY/MM/DD`. Mirrors `parseSlashFormat`.
pub fn parse_slash(s: &str, base: Moment) -> Option<Moment> {
    if count(s, b'/') != 2 {
        return None;
    }
    let (p0, p1, p2) = split3(s, b'/')?;
    if p0.len() < 4 || !is_all_digits(p0) || !is_all_digits(p1) || !is_all_digits(p2) {
        return None;
    }
    let (year, month, day) = (atoi(p0), atoi(p1), atoi(p2));
    if !is_valid_date(year, month, day) {
        return None;
    }
    Some(mk(base.tz, year, month, day, 0, 0, 0))
}

/// `MM/DD/YYYY`. Mirrors `parseUSFormat`.
pub fn parse_us(s: &str, base: Moment) -> Option<Moment> {
    if count(s, b'/') != 2 {
        return None;
    }
    let (p0, p1, p2) = split3(s, b'/')?;
    if p2.len() < 4 || !is_all_digits(p0) || !is_all_digits(p1) || !is_all_digits(p2) {
        return None;
    }
    let (month, day, year) = (atoi(p0), atoi(p1), atoi(p2));
    if !is_valid_date(year, month, day) {
        return None;
    }
    Some(mk(base.tz, year, month, day, 0, 0, 0))
}

/// `DD.MM.YY` / `DD.MM.YYYY`. Mirrors `parseEuropeanFormat`.
pub fn parse_european(s: &str, base: Moment) -> Option<Moment> {
    if count(s, b'.') != 2 {
        return None;
    }
    let (p0, p1, p2) = split3(s, b'.')?;
    if !is_all_digits(p0) || !is_all_digits(p1) || !is_all_digits(p2) {
        return None;
    }
    let (day, month) = (atoi(p0), atoi(p1));
    let year = {
        let y = atoi(p2);
        if y < 100 {
            two_digit_year(y)
        } else {
            y
        }
    };
    if !is_valid_date(year, month, day) {
        return None;
    }
    Some(mk(base.tz, year, month, day, 0, 0, 0))
}

/// `YYYY-MM`, `YYYY-M`, or ISO ordinal `YYYY-DDD`. Mirrors `parseYearMonthFormat`.
pub fn parse_year_month(s: &str, base: Moment) -> Option<Moment> {
    if count(s, b'-') != 1 {
        return None;
    }
    let i = s.find('-')?;
    let (p0, p1) = (&s[..i], &s[i + 1..]);
    if !is_all_digits(p0) || !is_all_digits(p1) || p0.len() < 4 {
        return None;
    }
    let year = atoi(p0);
    let dom = atoi(p1);

    // YYYY-DDD ordinal day-of-year.
    if p1.len() == 3 && (1..=366).contains(&dom) {
        let days = civil::days_from_civil(year, 1, 1) + (dom - 1);
        let (y, _, _) = civil::civil_from_days(days);
        if y == year {
            return Some(mk(base.tz, year, 1, dom, 0, 0, 0)); // day carries
        }
        return None;
    }

    if !(1..=12).contains(&dom) {
        return None;
    }
    Some(mk(base.tz, year, dom, 1, 0, 0, 0))
}

/// `0000-00-00 ...` → PHP's -0001-11-30. Mirrors `parseZeroDate`.
pub fn parse_zero_date(s: &str, base: Moment) -> Option<Moment> {
    if !s.trim_start().starts_with("0000-00-00") {
        return None;
    }
    // time.Date(0,0,0,...) normalizes: month 0 → Dec of prev year, day 0 → last
    // day of prev month → -0001-11-30.
    Some(mk(base.tz, 0, 0, 0, 0, 0, 0))
}

/// `-YYYY-MM-DD [HH:MM:SS [TZ]]` / `+YYYY-MM-DD[T]...`. Mirrors `parseSignedYear`.
pub fn parse_signed_year(s: &str, base: Moment) -> Option<Moment> {
    let b = s.as_bytes();
    if b.len() < 2 {
        return None;
    }
    let sign: i64 = match b[0] {
        b'-' => -1,
        b'+' => 1,
        _ => return None,
    };
    let rest = &s[1..];

    let (date_part, time_tz): (&str, &str) = if let Some(sp) = rest.find(' ') {
        (&rest[..sp], rest[sp + 1..].trim())
    } else if sign > 0 {
        match rest.find(['t', 'T']) {
            Some(ti) => (&rest[..ti], &rest[ti + 1..]),
            None => (rest, ""),
        }
    } else {
        (rest, "")
    };

    if count(date_part, b'-') != 2 {
        return None;
    }
    let (p0, p1, p2) = split3(date_part, b'-')?;
    if !is_all_digits(p0) || !is_all_digits(p1) || !is_all_digits(p2) {
        return None;
    }
    if sign > 0 && p0.len() < 4 {
        return None;
    }
    let (year, month, day) = (atoi(p0), atoi(p1), atoi(p2));
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }

    let (mut h, mut mi, mut sec) = (0i64, 0i64, 0i64);
    let mut tz = base.tz;
    if !time_tz.is_empty() {
        let (hh, mm, ss, t) = parse_time_tz_suffix(time_tz, base.tz);
        h = hh;
        mi = mm;
        sec = ss;
        tz = t;
    }

    Some(mk(tz, sign * year, month, day, h, mi, sec))
}

/// `MM/DD/YY HHMM` (short year + military time). Mirrors
/// `parseShortYearUSDateWithMilitaryTime`.
pub fn parse_short_year_us_military(s: &str, base: Moment) -> Option<Moment> {
    let sp = s.find(' ')?;
    let date_part = &s[..sp];
    let time_part = s[sp + 1..].trim();
    if count(date_part, b'/') != 2 {
        return None;
    }
    let (p0, p1, p2) = split3(date_part, b'/')?;
    if !is_all_digits(p0) || !is_all_digits(p1) || !is_all_digits(p2) || p2.len() > 2 {
        return None;
    }
    let month = atoi(p0);
    let day = atoi(p1);
    let year = two_digit_year(atoi(p2));
    if time_part.len() != 4 || !is_all_digits(time_part) {
        return None;
    }
    let hour = atoi(&time_part[..2]);
    let minute = atoi(&time_part[2..4]);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    if !is_valid_time(hour, minute, 0) {
        return None;
    }
    Some(mk(base.tz, year, month, day, hour, minute, 0))
}

/// 5–6 digit "year" that PHP reinterprets as compact time + month/day. Mirrors
/// `parseLargeYearAsTime`.
pub fn parse_large_year_as_time(s: &str, base: Moment) -> Option<Moment> {
    if count(s, b'-') != 2 {
        return None;
    }
    let (digits, p1, p2) = split3(s, b'-')?;
    if !is_all_digits(digits) || digits.len() < 5 || digits.len() > 6 {
        return None;
    }
    if !is_all_digits(p1) || !is_all_digits(p2) {
        return None;
    }
    let now = base.wall();

    if digits.len() == 5 {
        let hour = atoi(&digits[0..2]);
        let minute = atoi(&digits[2..4]);
        let second = atoi(&digits[4..5]);
        let month = atoi(p1);
        let day = atoi(p2);
        if !is_valid_time(hour, minute, second) {
            return None;
        }
        if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
            return None;
        }
        return Some(mk(base.tz, now.year, month, day, hour, minute, second));
    }

    // 6 digits: HHMMSS, the first -NN is a timezone offset; today's date kept.
    let hour = atoi(&digits[0..2]);
    let minute = atoi(&digits[2..4]);
    let second = atoi(&digits[4..6]);
    if !is_valid_time(hour, minute, second) {
        return None;
    }
    let tz_offset = atoi(p1);
    if !(0..=14).contains(&tz_offset) {
        return None;
    }
    let tz = Tz::Fixed((-tz_offset * 3600) as i32);
    Some(mk(tz, now.year, now.month as i64, now.day as i64, hour, minute, second))
}

/// `YYYY-MM-DD HH:MM:SS [TZ]` (and month-name dates via the extended parser,
/// wired later). Mirrors `parseDateTimeFormat`.
pub fn parse_datetime(s: &str, base: Moment) -> Option<Moment> {
    let sp = s.find(' ')?;
    let date_part = &s[..sp];
    let mut rest = s[sp + 1..].trim();

    // Trailing AM/PM (attached, spaced, or dotted).
    let mut ampm = "";
    if rest.len() >= 4
        && (rest[rest.len() - 4..].eq_ignore_ascii_case("a.m.")
            || rest[rest.len() - 4..].eq_ignore_ascii_case("p.m."))
    {
        ampm = if rest.as_bytes()[rest.len() - 4].eq_ignore_ascii_case(&b'a') {
            "am"
        } else {
            "pm"
        };
        rest = rest[..rest.len() - 4].trim();
    } else if rest.len() >= 2
        && (rest[rest.len() - 2..].eq_ignore_ascii_case("am") || rest[rest.len() - 2..].eq_ignore_ascii_case("pm"))
    {
        ampm = if rest[rest.len() - 2..].eq_ignore_ascii_case("am") { "am" } else { "pm" };
        rest = rest[..rest.len() - 2].trim();
    }

    let (mut hour, minute, second, consumed) = parse_iso8601_time(rest)?;
    if !ampm.is_empty() {
        hour = apply_ampm(hour, ampm);
    }

    // Date part: ISO format (month-name date support added with extended formats).
    let date = parse_iso(date_part, base)?;
    let dw = date.wall();

    // Optional timezone after the time.
    let mut tz = base.tz;
    let tz_rest = rest[consumed..].trim();
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

    Some(mk(tz, dw.year, dw.month as i64, dw.day as i64, hour, minute, second))
}

// ---------------------------------------------------------------------------
// Shared time helpers
// ---------------------------------------------------------------------------

/// Validate a wall-clock time.
pub fn is_valid_time(h: i64, mi: i64, s: i64) -> bool {
    (0..=23).contains(&h) && (0..=59).contains(&mi) && (0..=59).contains(&s)
}

/// Parse an ISO 8601 time from the start of `s`. Returns `(hour, minute, second,
/// bytes_consumed)`. Sub-second digits are consumed but ignored (we return whole
/// seconds). Mirrors `parseISO8601Time`. Hour 24 is allowed (caller handles).
pub fn parse_iso8601_time(s: &str) -> Option<(i64, i64, i64, usize)> {
    let b = s.as_bytes();
    if b.is_empty() {
        return None;
    }

    let (hour, minute, second, mut consumed);
    if let Some((h, m, sec, c)) = tz::parse_flex_time(s) {
        hour = h as i64;
        minute = m as i64;
        second = sec as i64;
        consumed = c;
    } else if b.len() >= 6 && b[..6].iter().all(|c| c.is_ascii_digit()) {
        hour = atoi(&s[..2]);
        minute = atoi(&s[2..4]);
        second = atoi(&s[4..6]);
        consumed = 6;
    } else if b.len() >= 4 && b[..4].iter().all(|c| c.is_ascii_digit()) {
        hour = atoi(&s[..2]);
        minute = atoi(&s[2..4]);
        second = 0;
        consumed = 4;
    } else if b.len() >= 2 && b[0].is_ascii_digit() && b[1].is_ascii_digit() && (b.len() == 2 || !b[2].is_ascii_digit()) {
        hour = atoi(&s[..2]);
        minute = 0;
        second = 0;
        consumed = 2;
    } else if b[0].is_ascii_digit() && (b.len() == 1 || !b[1].is_ascii_digit()) {
        hour = atoi(&s[..1]);
        minute = 0;
        second = 0;
        consumed = 1;
    } else {
        return None;
    }

    if hour != 24 && !is_valid_time(hour, minute, second) {
        return None;
    }

    // Fractional seconds: consume digits after a '.'.
    if consumed < b.len() && b[consumed] == b'.' {
        consumed += 1;
        while consumed < b.len() && b[consumed].is_ascii_digit() {
            consumed += 1;
        }
    }

    Some((hour, minute, second, consumed))
}

/// Parse `HH:MM:SS[.frac] [TZ]` from a date suffix, returning
/// `(hour, minute, second, tz)`. Mirrors `parseTimeTzSuffix`.
pub fn parse_time_tz_suffix(s: &str, default_tz: Tz) -> (i64, i64, i64, Tz) {
    let Some((h, m, sec, consumed)) = tz::parse_flex_time(s) else {
        return (0, 0, 0, default_tz);
    };
    let mut remaining = &s[consumed..];

    if remaining.starts_with('.') {
        let mut end = 1;
        let rb = remaining.as_bytes();
        while end < rb.len() && rb[end].is_ascii_digit() {
            end += 1;
        }
        remaining = &remaining[end..];
    }

    let remaining = remaining.trim();
    let mut tz = default_tz;
    if !remaining.is_empty() {
        if let Some((off, _)) = tz::parse_numeric_offset(remaining) {
            tz = if off == 0 { Tz::Utc } else { Tz::Fixed(off) };
        } else if let Some(t) = tz::parse_timezone(remaining) {
            tz = t;
        }
    }

    (h as i64, m as i64, sec as i64, tz)
}

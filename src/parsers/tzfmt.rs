//! Date/time formats that carry a trailing timezone — port of
//! `date_with_timezone.go`.

use crate::lookups::month_by_name;
use crate::parsers::formats::{mk, parse_iso, tail_from};
use crate::tz::{self, Moment};

/// Entry point. Mirrors `parseWithTimezone`.
pub fn parse_with_timezone(s: &str, base: Moment) -> Option<Moment> {
    parse_full_datetime_with_tz(s, base)
        .or_else(|| parse_iso_datetime_with_tz(s, base))
        .or_else(|| parse_time_only_with_tz(s, base))
}

fn tz_chars_ok(s: &str) -> bool {
    s.bytes()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'/' | b'_' | b'.'))
}

/// Split "H:M:S" requiring exactly 3 colon parts; returns validated (h,m,s).
fn parse_hms3(s: &str) -> Option<(i64, i64, i64)> {
    let mut it = s.split(':');
    let h: i64 = it.next()?.parse().ok()?;
    let m: i64 = it.next()?.parse().ok()?;
    let sec: i64 = it.next()?.parse().ok()?;
    if it.next().is_some() {
        return None;
    }
    if !(0..=23).contains(&h) || !(0..=59).contains(&m) || !(0..=59).contains(&sec) {
        return None;
    }
    Some((h, m, sec))
}

/// `YYYY-M-D HH:MM:SS timezone`. Mirrors `parseISODateTimeWithTimezone`.
pub(crate) fn parse_iso_datetime_with_tz(s: &str, base: Moment) -> Option<Moment> {
    let sp = s.find(' ')?;
    let date_part = &s[..sp];
    if date_part.bytes().filter(|b| *b == b'-').count() != 2 {
        return None;
    }
    let rest = s[sp + 1..].trim();
    let tsp = rest.find(' ')?;
    let time_part = &rest[..tsp];
    let tz_string = rest[tsp + 1..].trim();

    let (hour, minute, second) = parse_hms3(time_part)?;
    if !tz_chars_ok(tz_string) {
        return None;
    }
    let date = parse_iso(date_part, base)?;
    let w = date.wall();
    let tz = tz::parse_timezone(tz_string)?;
    Some(mk(tz, w.year, w.month as i64, w.day as i64, hour, minute, second))
}

/// `HH:MM[:SS] timezone` (date taken from base). Mirrors `parseTimeOnlyWithTimezone`.
fn parse_time_only_with_tz(s: &str, base: Moment) -> Option<Moment> {
    let sp = s.rfind(' ')?;
    let time_part = s[..sp].trim();
    let tz_string = s[sp + 1..].trim();

    let mut it = time_part.split(':');
    let h: i64 = it.next()?.parse().ok()?;
    let m: i64 = it.next()?.parse().ok()?;
    let mut sec = 0i64;
    let mut count = 2;
    if let Some(s3) = it.next() {
        sec = s3.parse().ok()?;
        count = 3;
    }
    if it.next().is_some() {
        return None;
    }
    if !(0..=23).contains(&h) || !(0..=59).contains(&m) || (count == 3 && !(0..=59).contains(&sec)) {
        return None;
    }
    if !tz_chars_ok(tz_string) {
        return None;
    }
    let tz = tz::parse_timezone(tz_string)?;
    // Go uses time.Now(); we use the base date for determinism.
    let now = base.wall();
    Some(mk(tz, now.year, now.month as i64, now.day as i64, h, m, sec))
}

/// `MonthName Day Year [HH:MM[:SS]] Timezone`. Mirrors `parseFullDateTimeWithTimezone`.
fn parse_full_datetime_with_tz(s: &str, _base: Moment) -> Option<Moment> {
    let mut fields = s.split_whitespace();
    let month = month_by_name(fields.next()?)? as i64;

    let mut day_str = fields.next()?;
    for suf in ["st", "nd", "rd", "th"] {
        if let Some(stripped) = day_str.strip_suffix(suf) {
            day_str = stripped;
            break;
        }
    }
    let day: i64 = day_str.parse().ok()?;
    if !(1..=31).contains(&day) {
        return None;
    }

    let year: i64 = fields.next()?.parse().ok()?;
    if !(1..=9999).contains(&year) {
        return None;
    }
    if day > crate::civil::days_in_month(year, month) {
        return None;
    }

    let next = fields.next()?;
    let (mut hour, mut minute, mut second) = (0i64, 0i64, 0i64);
    let tz_first;
    if next.contains(':') {
        let mut it = next.split(':');
        hour = it.next()?.parse().ok()?;
        minute = it.next()?.parse().ok()?;
        if let Some(s3) = it.next() {
            second = s3.parse().ok()?;
        }
        if it.next().is_some() {
            return None;
        }
        if !(0..=23).contains(&hour) || !(0..=59).contains(&minute) || !(0..=59).contains(&second) {
            return None;
        }
        tz_first = fields.next()?;
    } else {
        tz_first = next;
    }

    // The timezone is the remainder of the string from this field onward.
    let tz_string = tail_from(s, tz_first).trim();
    let tz = tz::parse_timezone(tz_string)?;
    Some(mk(tz, year, month, day, hour, minute, second))
}

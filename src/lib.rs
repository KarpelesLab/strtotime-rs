//! `strtotime` — parse PHP-style date/time expressions into a Unix timestamp.
//!
//! This is a `#![no_std]`, allocation-free port of the Go library
//! [`strtotime`](https://github.com/KarpelesLab/strtotime), which mirrors PHP's
//! [`strtotime()`](https://www.php.net/manual/en/function.strtotime.php).
//!
//! # Quick start
//!
//! ```
//! use strtotime::{strtotime, Tz};
//!
//! // Absolute date in UTC.
//! let ts = strtotime("2000-01-01 12:00:00", 0, Tz::Utc).unwrap();
//! assert_eq!(ts, 946728000);
//!
//! // Relative to a base instant.
//! let base = 946728000; // 2000-01-01 12:00:00 UTC
//! let tomorrow = strtotime("tomorrow", base, Tz::Utc).unwrap();
//! assert_eq!(tomorrow, 946771200); // 2000-01-02 00:00:00 UTC
//! ```
//!
//! # Timezones
//!
//! [`Tz`] selects the zone a non-absolute expression is interpreted in. With the
//! default `iana` feature, [`Tz`] can hold any IANA zone (DST-aware) loaded from
//! the embedded `timezone-data` database. Without it, only [`Tz::Utc`] and
//! [`Tz::Fixed`] (plus abbreviations found in the input) are available.
//!
//! # Features
//!
//! - `iana` (default): full IANA timezone support via the `timezone-data` crate.
//! - `std`: convenience helpers using the system clock and `std::time`.

#![no_std]
#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]

#[cfg(feature = "std")]
extern crate std;

mod civil;
mod datetime;
mod error;
mod lookups;
mod parsers;
mod relmath;
mod tokenizer;
mod tz;

pub use datetime::DateTime;
pub use error::Error;
pub use tz::Tz;

use datetime::Civil;
use tz::Moment;

/// Parse `input` relative to `base_unix` (a Unix timestamp) in zone `tz`,
/// returning the resolved Unix timestamp.
///
/// `base_unix` is the reference point for relative expressions ("tomorrow",
/// "+2 days"); it is ignored for fully absolute inputs. Pass `0` (the epoch) if
/// the expression is absolute.
pub fn strtotime(input: &str, base_unix: i64, tz: Tz) -> Result<i64, Error> {
    eval(input, Moment { unix: base_unix, tz }).map(|m| m.unix)
}

/// Like [`strtotime`], but returns the resolved [`DateTime`] (broken-down civil
/// fields plus the UTC offset in effect) instead of a bare timestamp.
pub fn strtotime_civil(input: &str, base_unix: i64, tz: Tz) -> Result<DateTime, Error> {
    eval(input, Moment { unix: base_unix, tz }).map(|m| m.wall())
}

/// The current Unix timestamp from the system clock. Requires the `std` feature.
#[cfg(feature = "std")]
#[cfg_attr(docsrs, doc(cfg(feature = "std")))]
pub fn now_unix() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_secs() as i64,
        Err(e) => -(e.duration().as_secs() as i64),
    }
}

/// Convert a Unix timestamp to a [`std::time::SystemTime`]. Requires `std`.
#[cfg(feature = "std")]
#[cfg_attr(docsrs, doc(cfg(feature = "std")))]
pub fn system_time_from_unix(unix: i64) -> std::time::SystemTime {
    use std::time::{Duration, UNIX_EPOCH};
    if unix >= 0 {
        UNIX_EPOCH + Duration::from_secs(unix as u64)
    } else {
        UNIX_EPOCH - Duration::from_secs(unix.unsigned_abs())
    }
}

/// Parse `input` relative to the current system time, in zone `tz`. Requires `std`.
///
/// Convenience over [`strtotime`] that supplies [`now_unix`] as the base.
#[cfg(feature = "std")]
#[cfg_attr(docsrs, doc(cfg(feature = "std")))]
pub fn strtotime_now(input: &str, tz: Tz) -> Result<i64, Error> {
    strtotime(input, now_unix(), tz)
}

#[cfg(feature = "std")]
impl From<DateTime> for std::time::SystemTime {
    fn from(dt: DateTime) -> Self {
        system_time_from_unix(dt.unix())
    }
}

/// Core orchestration: mirrors the Go reference's `StrToTime`. Runs the unix-`@`
/// handler, keyword expressions, the ordered format pipeline, the
/// date+relative / weekday-prefix / compound / ordinal-date fallbacks, and
/// finally the token parser.
fn eval(input: &str, base: Moment) -> Result<Moment, Error> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(Error::EmptyInput);
    }

    // "@<unix>[.frac] [TZ]"
    if let Some(m) = try_unix_timestamp(trimmed, base.tz)? {
        return Ok(m);
    }

    // Keywords (case-insensitive).
    if let Some(m) = try_keyword(trimmed, base) {
        return Ok(m);
    }

    // Ordered format parsers.
    if let Some(m) = parsers::formats::pipeline(trimmed, base) {
        return Ok(m);
    }

    // Date followed by a relative adjustment: "2023-05-30 -1 month".
    if let Some(m) = parse_date_with_relative_time(trimmed, base) {
        return Ok(m);
    }

    // Leading weekday name stripped and reparsed: "Fri Aug 20 1993 23:59:59".
    if let Some(m) = weekday_prefix_reparse(trimmed, base) {
        return Ok(m);
    }

    // Compound expression: a part joined by + / - in the middle.
    if is_compound_expression(trimmed) {
        return parse_compound(trimmed, base);
    }

    // Ordinal date ("26th Nov").
    if let Some(m) = parsers::extended::parse_ordinal_date(trimmed, base) {
        return Ok(m);
    }

    // Token-based parser (relative expressions, weekdays, month names, times).
    let toks = tokenizer::tokenize(trimmed)?;
    let mut parser = parsers::token_parser::Parser::new(trimmed, toks.as_slice(), base);
    parser.parse()
}

// ---------------------------------------------------------------------------
// Orchestration helpers (recurse into `eval`)
// ---------------------------------------------------------------------------

/// Does `s` look like a date format (`A-B-C`, `A/B/C`, or `A.B.C`, all digits)?
fn looks_like_date(s: &str) -> bool {
    for sep in [b'-', b'/', b'.'] {
        if s.bytes().filter(|b| *b == sep).count() == 2 {
            let mut ok = true;
            let mut nonempty = 0;
            for part in s.split(sep as char) {
                if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
                    ok = false;
                    break;
                }
                nonempty += 1;
            }
            if ok && nonempty == 3 {
                return true;
            }
        }
    }
    false
}

/// Parse "DATE rest" where DATE is a recognized date and `rest` is a relative
/// adjustment. Mirrors `parseDateWithRelativeTime` + `splitDateAndRest`.
fn parse_date_with_relative_time(s: &str, base: Moment) -> Option<Moment> {
    let sp = s.find(' ')?;
    let date_part = &s[..sp];
    let rest = s[sp + 1..].trim();
    if rest.is_empty() || !looks_like_date(date_part) {
        return None;
    }

    let date = eval(date_part, base).ok()?;

    // Special case: subtracting one month from a month-end date clamps to the
    // previous month's last day.
    if rest.eq_ignore_ascii_case("-1 month") {
        let w = date.wall();
        if w.day as i64 == civil::days_in_month(w.year, w.month as i64) {
            let last_prev = civil::days_from_civil(w.year, w.month as i64, 1) - 1;
            let (py, pmo, pd) = civil::civil_from_days(last_prev);
            return Some(Moment::from_civil(
                base.tz,
                Civil::new(py, pmo, pd, w.hour as i64, w.minute as i64, w.second as i64),
            ));
        }
    }

    eval(rest, Moment { unix: date.unix, tz: base.tz }).ok()
}

/// Strip a leading weekday name and reparse the rest, advancing to the named
/// weekday if it doesn't match. Mirrors `tryWeekdayPrefixReparse` +
/// `stripWeekdayPrefix`.
fn weekday_prefix_reparse(s: &str, base: Moment) -> Option<Moment> {
    let (rest, day_num) = strip_weekday_prefix(s)?;
    let rt = rest.trim();
    let lower3 = |p: &str| rt.len() >= p.len() && rt[..p.len()].eq_ignore_ascii_case(p);
    if lower3("next ") || lower3("last ") || lower3("this ") {
        return None;
    }

    let mut t = eval(rest, base).ok()?;
    if day_num >= 0 {
        let wd = t.wall().weekday() as i64;
        if wd != day_num {
            let mut days = (day_num - wd + 7) % 7;
            if days == 0 {
                days = 7;
            }
            t = relmath::apply_offset(t, days, lookups::Unit::Day);
        }
    }
    Some(t)
}

/// Strip a leading weekday name (full or 3-letter), returning the remainder and
/// the day number (0=Sunday). Returns `None` if no weekday prefix.
pub(crate) fn strip_weekday_prefix(s: &str) -> Option<(&str, i64)> {
    const FULL: &[(&str, i64)] = &[
        ("sunday", 0),
        ("monday", 1),
        ("tuesday", 2),
        ("wednesday", 3),
        ("thursday", 4),
        ("friday", 5),
        ("saturday", 6),
    ];
    for (name, dn) in FULL {
        if s.len() > name.len() && s[..name.len()].eq_ignore_ascii_case(name) {
            let r = s[name.len()..].trim_start_matches([',', ' ']);
            if !r.is_empty() {
                return Some((r, *dn));
            }
        }
    }
    if s.len() > 3 {
        if let Some(dn) = lookups::day_of_week(&s[..3]) {
            let r = s[3..].trim_start_matches([',', ' ']);
            if !r.is_empty() {
                return Some((r, dn as i64));
            }
        }
    }
    None
}

/// Normalize spaces around `+`/`-` into `buf`, returning the normalized `&str`.
/// Mirrors the Go reference's `strings.NewReplacer(" + ","+", " - ","-", "+ ","+", "- ","-")`.
fn normalize_ops<'b>(s: &str, buf: &'b mut [u8]) -> Option<&'b str> {
    const PATS: &[(&str, u8)] = &[(" + ", b'+'), (" - ", b'-'), ("+ ", b'+'), ("- ", b'-')];
    let sb = s.as_bytes();
    let mut i = 0;
    let mut n = 0;
    'outer: while i < sb.len() {
        for (pat, rep) in PATS {
            if sb[i..].starts_with(pat.as_bytes()) {
                if n >= buf.len() {
                    return None;
                }
                buf[n] = *rep;
                n += 1;
                i += pat.len();
                continue 'outer;
            }
        }
        if n >= buf.len() {
            return None;
        }
        buf[n] = sb[i];
        n += 1;
        i += 1;
    }
    core::str::from_utf8(&buf[..n]).ok()
}

/// A compound expression contains `+`/`-` joining parts (not just a leading
/// sign). Mirrors `isCompoundExpression`.
fn is_compound_expression(s: &str) -> bool {
    let mut buf = [0u8; 512];
    let Some(n) = normalize_ops(s, &mut buf) else {
        return false;
    };
    let b = n.as_bytes();
    let has_plus = b.contains(&b'+');
    let has_minus = b.contains(&b'-');
    let pre_plus = b.first() == Some(&b'+');
    let pre_minus = b.first() == Some(&b'-');
    (has_plus && !pre_plus) || (has_minus && !pre_minus)
}

/// Evaluate a compound expression by chaining each `±part` onto the running
/// result. Mirrors `parseCompoundExpression`.
fn parse_compound(s: &str, base: Moment) -> Result<Moment, Error> {
    let mut buf = [0u8; 512];
    let n = normalize_ops(s, &mut buf).ok_or(Error::TooLong)?;
    let nb = n.as_bytes();
    let is_op = |c: u8| c == b'+' || c == b'-';

    // A trailing operator means an empty final operand, which PHP/Go reject
    // ("2023-", "next year +").
    if matches!(nb.last(), Some(&b'+') | Some(&b'-')) {
        return Err(Error::UnableToParse);
    }

    // First operator at index > 0 splits the leading part.
    let mut i = 1;
    while i < nb.len() && !is_op(nb[i]) {
        i += 1;
    }
    if i >= nb.len() {
        return Err(Error::UnableToParse);
    }

    let mut result = eval(&n[..i], base)?;
    let mut start = i;
    loop {
        let mut j = start + 1;
        while j < nb.len() && !is_op(nb[j]) {
            j += 1;
        }
        result = eval(&n[start..j], Moment { unix: result.unix, tz: base.tz })?;
        if j >= nb.len() {
            break;
        }
        start = j;
    }
    Ok(result)
}

/// Parse `@<unix>[.fraction] [timezone]`. Returns `Ok(None)` if the input is not
/// an `@` expression, `Err` if it is but is malformed.
fn try_unix_timestamp(s: &str, mut tz: Tz) -> Result<Option<Moment>, Error> {
    let Some(body) = s.strip_prefix('@') else {
        return Ok(None);
    };

    let (ts_part, tz_part) = match body.find(' ') {
        Some(i) => (&body[..i], body[i + 1..].trim()),
        None => (body, ""),
    };

    let int_str = match ts_part.find('.') {
        Some(i) => {
            // PHP rejects fractional seconds with more than 6 digits.
            if ts_part.len() - i - 1 > 6 {
                return Err(Error::InvalidNumber);
            }
            &ts_part[..i]
        }
        None => ts_part,
    };

    let unix: i64 = int_str.parse().map_err(|_| Error::InvalidNumber)?;

    if !tz_part.is_empty() {
        if let Some(t) = tz::parse_timezone(tz_part) {
            tz = t;
        }
    }

    Ok(Some(Moment { unix, tz }))
}

/// Handle the bare keyword expressions: now, today, midnight, tomorrow,
/// yesterday, noon. Case-insensitive.
fn try_keyword(s: &str, base: Moment) -> Option<Moment> {
    let day_at = |day_delta: i64, hour: i64| {
        let w = base.wall();
        let c = Civil::new(w.year, w.month as i64, w.day as i64 + day_delta, hour, 0, 0);
        Moment::from_civil(base.tz, c)
    };

    if s.eq_ignore_ascii_case("now") {
        Some(base)
    } else if s.eq_ignore_ascii_case("today") || s.eq_ignore_ascii_case("midnight") {
        Some(day_at(0, 0))
    } else if s.eq_ignore_ascii_case("tomorrow") {
        Some(day_at(1, 0))
    } else if s.eq_ignore_ascii_case("yesterday") {
        Some(day_at(-1, 0))
    } else if s.eq_ignore_ascii_case("noon") {
        Some(day_at(0, 12))
    } else {
        None
    }
}

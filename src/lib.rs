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

/// Core orchestration: mirrors the Go reference's `StrToTime`. Currently a stub
/// handling `@timestamp` and the keyword expressions; format parsers and the
/// token parser are wired in over subsequent phases.
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

    Err(Error::UnableToParse)
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

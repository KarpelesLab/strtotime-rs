//! Timezone resolution.
//!
//! [`Tz`] is the timezone a parse is performed in. It can be UTC, a fixed
//! numeric offset, or — with the `iana` feature — a full IANA zone backed by the
//! `timezone-data` crate (DST-aware).
//!
//! Two directions are needed:
//! - **instant → offset** ([`Tz::offset_at`]): the offset in effect at a UTC
//!   instant, used to break a timestamp into wall-clock fields.
//! - **wall-clock → instant** ([`Tz::resolve_local`]): the reverse, which for
//!   DST zones is ambiguous in folds and impossible in gaps; we follow PHP's
//!   fall-forward behavior.

use crate::datetime::{Civil, DateTime};

/// The timezone a time expression is interpreted in.
#[derive(Clone, Copy)]
pub enum Tz {
    /// Coordinated Universal Time (offset 0).
    Utc,
    /// A fixed offset from UTC, in seconds east of UTC.
    Fixed(i32),
    /// A named IANA zone with full transition/DST data.
    #[cfg(feature = "iana")]
    Iana(timezone_data::Zone),
}

impl core::fmt::Debug for Tz {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Tz::Utc => f.write_str("Utc"),
            Tz::Fixed(o) => write!(f, "Fixed({o})"),
            #[cfg(feature = "iana")]
            Tz::Iana(z) => write!(f, "Iana({})", z.name()),
        }
    }
}

impl Tz {
    /// The offset (seconds east of UTC) in effect at the given UTC instant.
    pub fn offset_at(&self, unix: i64) -> i32 {
        match self {
            Tz::Utc => 0,
            Tz::Fixed(o) => *o,
            #[cfg(feature = "iana")]
            Tz::Iana(z) => z.lookup(unix).offset,
        }
    }

    /// Convert wall-clock seconds (a civil time interpreted as if it were UTC)
    /// into a real `(unix, offset)` pair in this zone.
    ///
    /// For UTC/fixed this is exact. For IANA zones we use the standard two-pass
    /// estimate; gap/fold edge cases are refined later against the test corpus.
    pub fn resolve_local(&self, wall: i64) -> (i64, i32) {
        match self {
            Tz::Utc => (wall, 0),
            Tz::Fixed(o) => (wall.wrapping_sub(*o as i64), *o),
            #[cfg(feature = "iana")]
            Tz::Iana(z) => {
                // Resolve a local wall-clock time to an instant. For an
                // unambiguous time the offset is self-consistent. Near a
                // transition we try the second offset; if that is still not
                // self-consistent the wall time is in a spring-forward gap and
                // we fall *forward* (PHP behavior) by using the smaller offset.
                let o0 = z.lookup(wall).offset;
                let cand0 = wall.wrapping_sub(o0 as i64);
                let oa0 = z.lookup(cand0).offset;
                if oa0 == o0 {
                    return (cand0, o0);
                }
                let cand1 = wall.wrapping_sub(oa0 as i64);
                let oa1 = z.lookup(cand1).offset;
                if oa1 == oa0 {
                    return (cand1, oa1);
                }
                // Gap: no valid instant maps back to `wall`; fall forward.
                let off = o0.min(oa0);
                (wall.wrapping_sub(off as i64), off)
            }
        }
    }
}

/// An instant tagged with the zone it is being interpreted in. Mirrors Go's
/// `time.Time` (instant + location). Parsers carry this as their running result.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Moment {
    pub unix: i64,
    pub tz: Tz,
    /// Sub-second component in microseconds (0..=999_999). Timezone-invariant.
    pub micros: u32,
}

impl Moment {
    /// A moment at `unix` seconds in `tz` with no sub-second component.
    pub fn new(unix: i64, tz: Tz) -> Moment {
        Moment { unix, tz, micros: 0 }
    }

    /// The wall-clock representation in this moment's zone.
    pub fn wall(&self) -> DateTime {
        let off = self.tz.offset_at(self.unix);
        DateTime::from_unix_offset(self.unix, off, self.micros)
    }

    /// Build a moment from civil wall-clock fields in a zone (fields may be out
    /// of range; they normalize/carry). No sub-second component.
    pub fn from_civil(tz: Tz, c: Civil) -> Moment {
        Self::from_civil_frac(tz, c, 0)
    }

    /// Like [`Moment::from_civil`], carrying a microsecond component.
    pub fn from_civil_frac(tz: Tz, c: Civil, micros: u32) -> Moment {
        let (unix, _off) = tz.resolve_local(c.unix_utc());
        Moment { unix, tz, micros }
    }

    /// Same instant (and sub-second component), reinterpreted in a different zone.
    pub fn in_tz(self, tz: Tz) -> Moment {
        Moment { unix: self.unix, tz, micros: self.micros }
    }
}

// ---------------------------------------------------------------------------
// Timezone string parsing
// ---------------------------------------------------------------------------

fn is_valid_tz_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, b'/' | b'_' | b'-' | b'+' | b' ')
}

/// Parse a timezone abbreviation (EST, PST, CET, GMT, UTC, military letters)
/// into a fixed offset. Case-insensitive. Mirrors `timezoneAbbreviations` in
/// the Go reference (which uses fixed offsets to preserve the stated zone).
fn abbrev_to_tz(s: &str) -> Option<Tz> {
    const H: i32 = 3600;
    // (abbrev, offset_seconds)
    const TABLE: &[(&str, i32)] = &[
        ("est", -5 * H),
        ("edt", -4 * H),
        ("cst", -6 * H),
        ("cdt", -5 * H),
        ("mst", -7 * H),
        ("mdt", -6 * H),
        ("pst", -8 * H),
        ("pdt", -7 * H),
        ("akst", -9 * H),
        ("akdt", -8 * H),
        ("hst", -10 * H),
        ("gmt", 0),
        ("bst", H),
        ("iet", H),
        ("cet", H),
        ("cest", 2 * H),
        ("eet", 2 * H),
        ("eest", 3 * H),
        ("awst", 8 * H),
        ("acst", 9 * H + 30 * 60),
        ("aest", 10 * H),
        ("aedt", 11 * H),
        ("jst", 9 * H),
        ("ct", 8 * H),
        ("ist", 5 * H + 30 * 60),
        ("utc", 0),
        ("z", 0),
        // Military single-letter codes.
        ("a", H),
        ("b", 2 * H),
        ("c", 3 * H),
        ("d", 4 * H),
        ("e", 5 * H),
        ("f", 6 * H),
        ("g", 7 * H),
        ("h", 8 * H),
        ("i", 9 * H),
        ("k", 10 * H),
        ("l", 11 * H),
        ("m", 12 * H),
        ("n", -H),
        ("o", -2 * H),
        ("p", -3 * H),
        ("q", -4 * H),
        ("r", -5 * H),
        ("s", -6 * H),
        ("t", -7 * H),
        ("u", -8 * H),
        ("v", -9 * H),
        ("w", -10 * H),
        ("x", -11 * H),
        ("y", -12 * H),
    ];
    for (name, off) in TABLE {
        if s.eq_ignore_ascii_case(name) {
            return Some(if *off == 0 { Tz::Utc } else { Tz::Fixed(*off) });
        }
    }
    None
}

/// Map a spelled-out timezone name to its IANA identifier. Mirrors
/// `timezoneNames` in the Go reference.
#[cfg(feature = "iana")]
fn full_name_to_iana(s: &str) -> Option<&'static str> {
    const TABLE: &[(&str, &str)] = &[
        ("eastern time", "America/New_York"),
        ("et", "America/New_York"),
        ("eastern", "America/New_York"),
        ("central time", "America/Chicago"),
        ("ct", "America/Chicago"),
        ("central", "America/Chicago"),
        ("mountain time", "America/Denver"),
        ("mt", "America/Denver"),
        ("mountain", "America/Denver"),
        ("pacific time", "America/Los_Angeles"),
        ("pt", "America/Los_Angeles"),
        ("pacific", "America/Los_Angeles"),
        ("alaska time", "America/Anchorage"),
        ("alaska", "America/Anchorage"),
        ("hawaii time", "Pacific/Honolulu"),
        ("hawaii", "Pacific/Honolulu"),
        ("greenwich mean time", "Europe/London"),
        ("british time", "Europe/London"),
        ("british", "Europe/London"),
        ("western european time", "Europe/London"),
        ("central european time", "Europe/Paris"),
        ("eastern european time", "Europe/Helsinki"),
        ("australian western time", "Australia/Perth"),
        ("australian central time", "Australia/Adelaide"),
        ("australian eastern time", "Australia/Sydney"),
        ("japan time", "Asia/Tokyo"),
        ("china time", "Asia/Shanghai"),
        ("india time", "Asia/Kolkata"),
        ("india", "Asia/Kolkata"),
        ("universal time", "UTC"),
        ("universal coordinated time", "UTC"),
        ("zulu time", "UTC"),
        ("zulu", "UTC"),
    ];
    for (name, iana) in TABLE {
        if s.eq_ignore_ascii_case(name) {
            return Some(iana);
        }
    }
    None
}

/// Parse a timezone string: abbreviation, full name, or IANA identifier.
/// Returns `None` if unrecognized. Mirrors `tryParseTimezone` in the Go
/// reference. Numeric offsets are handled separately by [`parse_numeric_offset`].
pub fn parse_timezone(s: &str) -> Option<Tz> {
    if s.is_empty() {
        return None;
    }
    for c in s.bytes() {
        if !is_valid_tz_char(c) {
            return None;
        }
    }

    if let Some(tz) = abbrev_to_tz(s) {
        return Some(tz);
    }

    #[cfg(feature = "iana")]
    {
        if let Some(name) = full_name_to_iana(s) {
            if name == "UTC" {
                return Some(Tz::Utc);
            }
            if let Ok(z) = timezone_data::load(name) {
                return Some(Tz::Iana(z));
            }
        }
        if let Ok(z) = timezone_data::load_insensitive(s) {
            return Some(Tz::Iana(z));
        }
    }

    None
}

/// Parse a numeric timezone offset (`Z`, `+HH:MM`, `-HHMM`, `+HH`, flexible
/// `+H:M`). Returns `(offset_seconds, bytes_consumed)`. Mirrors
/// `parseNumericTimezoneOffset` in the Go reference.
pub fn parse_numeric_offset(s: &str) -> Option<(i32, usize)> {
    let b = s.as_bytes();
    if b.is_empty() {
        return None;
    }

    if b[0] == b'z' || b[0] == b'Z' {
        if b.len() == 1 || b[1] == b' ' {
            return Some((0, 1));
        }
        return None;
    }

    let sign: i32 = match b[0] {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let rest = &b[1..];

    let is_digit = |c: u8| c.is_ascii_digit();
    let two = |r: &[u8], i: usize| ((r[i] - b'0') as i32) * 10 + (r[i + 1] - b'0') as i32;

    // +HH:MM
    if rest.len() >= 5 && rest[2] == b':' && is_digit(rest[0]) && is_digit(rest[1]) && is_digit(rest[3]) && is_digit(rest[4]) {
        let h = two(rest, 0);
        let m = two(rest, 3);
        if h <= 14 && m <= 59 {
            return Some((sign * (h * 3600 + m * 60), 6));
        }
    }
    // +HH:M (single-digit minute)
    if rest.len() >= 4
        && rest[2] == b':'
        && is_digit(rest[0])
        && is_digit(rest[1])
        && is_digit(rest[3])
        && (rest.len() == 4 || !is_digit(rest[4]))
    {
        let h = two(rest, 0);
        let m = (rest[3] - b'0') as i32;
        if h <= 14 && m <= 59 {
            return Some((sign * (h * 3600 + m * 60), 5));
        }
    }
    // +HHMM
    if rest.len() >= 4 && is_digit(rest[0]) && is_digit(rest[1]) && is_digit(rest[2]) && is_digit(rest[3]) {
        let h = two(rest, 0);
        let m = two(rest, 2);
        if h <= 14 && m <= 59 {
            return Some((sign * (h * 3600 + m * 60), 5));
        }
    }
    // +HH
    if rest.len() >= 2 && is_digit(rest[0]) && is_digit(rest[1]) && (rest.len() == 2 || !is_digit(rest[2])) {
        let h = two(rest, 0);
        if h <= 14 {
            return Some((sign * h * 3600, 3));
        }
    }
    // flexible +H:M
    if !rest.is_empty() && is_digit(rest[0]) {
        if let Some((h, m, _s, consumed)) = parse_flex_time(core::str::from_utf8(rest).ok()?) {
            if h <= 14 && m <= 59 {
                return Some((sign * (h * 3600 + m * 60), consumed + 1));
            }
        }
    }

    None
}

/// Parse a flexible `H:M[:S]` time (1–2 digit components) from the start of `s`.
/// Returns `(hour, minute, second, bytes_consumed)`. Mirrors `parseFlexTime`.
pub fn parse_flex_time(s: &str) -> Option<(i32, i32, i32, usize)> {
    let b = s.as_bytes();
    let mut pos = 0;
    let h_start = pos;
    while pos < b.len() && b[pos].is_ascii_digit() {
        pos += 1;
    }
    if pos == h_start || pos - h_start > 2 {
        return None;
    }
    if pos >= b.len() || b[pos] != b':' {
        return None;
    }
    let hour = atoi(&b[h_start..pos]);
    pos += 1;

    let m_start = pos;
    while pos < b.len() && b[pos].is_ascii_digit() {
        pos += 1;
    }
    if pos == m_start || pos - m_start > 2 {
        return None;
    }
    let minute = atoi(&b[m_start..pos]);

    let mut second = 0;
    if pos < b.len() && b[pos] == b':' {
        pos += 1;
        let s_start = pos;
        while pos < b.len() && b[pos].is_ascii_digit() {
            pos += 1;
        }
        if pos == s_start || pos - s_start > 2 {
            return None;
        }
        second = atoi(&b[s_start..pos]);
    }

    Some((hour, minute, second, pos))
}

/// Parse an ASCII digit slice into an i32 (no sign, assumes valid digits).
fn atoi(b: &[u8]) -> i32 {
    let mut n = 0i32;
    for &c in b {
        n = n * 10 + (c - b'0') as i32;
    }
    n
}

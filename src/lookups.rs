//! Name/word lookups: months, weekdays, time units, ordinals.
//!
//! Port of the Go reference's `lookups.go`. All matching is ASCII
//! case-insensitive (the Go code lowercases first; we compare in place).

/// Canonical time units.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    Day,
    Week,
    Weekday,
    Month,
    Year,
    Hour,
    Minute,
    Second,
}

/// Month number (1..=12) for a month name, handling a trailing period
/// (e.g. "dec." → 12). Case-insensitive.
pub fn month_by_name(name: &str) -> Option<u8> {
    let name = name.strip_suffix('.').unwrap_or(name);
    const TABLE: &[(&str, u8)] = &[
        ("january", 1),
        ("jan", 1),
        ("february", 2),
        ("feb", 2),
        ("march", 3),
        ("mar", 3),
        ("april", 4),
        ("apr", 4),
        ("may", 5),
        ("june", 6),
        ("jun", 6),
        ("july", 7),
        ("jul", 7),
        ("august", 8),
        ("aug", 8),
        ("september", 9),
        ("sep", 9),
        ("october", 10),
        ("oct", 10),
        ("november", 11),
        ("nov", 11),
        ("december", 12),
        ("dec", 12),
    ];
    for (n, m) in TABLE {
        if name.eq_ignore_ascii_case(n) {
            return Some(*m);
        }
    }
    None
}

/// Day-of-week number (0 = Sunday .. 6 = Saturday) for a day name, or `None`.
/// Case-insensitive.
pub fn day_of_week(day: &str) -> Option<u8> {
    const TABLE: &[(&str, u8)] = &[
        ("sunday", 0),
        ("sun", 0),
        ("monday", 1),
        ("mon", 1),
        ("tuesday", 2),
        ("tue", 2),
        ("wednesday", 3),
        ("wed", 3),
        ("thursday", 4),
        ("thu", 4),
        ("friday", 5),
        ("fri", 5),
        ("saturday", 6),
        ("sat", 6),
    ];
    for (n, d) in TABLE {
        if day.eq_ignore_ascii_case(n) {
            return Some(*d);
        }
    }
    None
}

/// Normalize a time-unit token to its canonical [`Unit`]. Mirrors
/// `normalizeTimeUnit`: exact table, then strip a trailing "s", then known
/// prefixes. Case-insensitive.
pub fn normalize_unit(unit: &str) -> Option<Unit> {
    // Exact matches (including odd plurals / abbreviations from the Go map).
    const TABLE: &[(&str, Unit)] = &[
        ("d", Unit::Day),
        ("day", Unit::Day),
        ("days", Unit::Day),
        ("days.", Unit::Day),
        ("w", Unit::Week),
        ("wk", Unit::Week),
        ("wks", Unit::Week),
        ("wks.", Unit::Week),
        ("week", Unit::Week),
        ("weeks", Unit::Week),
        ("weekday", Unit::Weekday),
        ("weekdays", Unit::Weekday),
        ("m", Unit::Month),
        ("mon", Unit::Month),
        ("mons", Unit::Month),
        ("mons.", Unit::Month),
        ("month", Unit::Month),
        ("months", Unit::Month),
        ("y", Unit::Year),
        ("yr", Unit::Year),
        ("yrs", Unit::Year),
        ("yrs.", Unit::Year),
        ("year", Unit::Year),
        ("years", Unit::Year),
        ("h", Unit::Hour),
        ("hr", Unit::Hour),
        ("hrs", Unit::Hour),
        ("hrs.", Unit::Hour),
        ("hour", Unit::Hour),
        ("hours", Unit::Hour),
        ("hourss", Unit::Hour),
        ("min", Unit::Minute),
        ("mins", Unit::Minute),
        ("mins.", Unit::Minute),
        ("minute", Unit::Minute),
        ("minutes", Unit::Minute),
        ("sec", Unit::Second),
        ("secs", Unit::Second),
        ("secs.", Unit::Second),
        ("second", Unit::Second),
        ("seconds", Unit::Second),
    ];
    for (n, u) in TABLE {
        if unit.eq_ignore_ascii_case(n) {
            return Some(*u);
        }
    }

    // Strip a trailing "s" and retry the exact table.
    if let Some(trimmed) = strip_trailing_s(unit) {
        for (n, u) in TABLE {
            if trimmed.eq_ignore_ascii_case(n) {
                return Some(*u);
            }
        }
    }

    // Known prefixes (order matters: weekday before week, hr handled with hour).
    let lower_starts = |p: &str| unit.len() >= p.len() && unit[..p.len()].eq_ignore_ascii_case(p);
    if lower_starts("weekday") {
        Some(Unit::Weekday)
    } else if lower_starts("day") {
        Some(Unit::Day)
    } else if lower_starts("week") {
        Some(Unit::Week)
    } else if lower_starts("month") {
        Some(Unit::Month)
    } else if lower_starts("year") {
        Some(Unit::Year)
    } else if lower_starts("hour") || lower_starts("hr") {
        Some(Unit::Hour)
    } else if lower_starts("min") {
        Some(Unit::Minute)
    } else if lower_starts("sec") {
        Some(Unit::Second)
    } else {
        None
    }
}

fn strip_trailing_s(s: &str) -> Option<&str> {
    if s.len() > 1 && (s.ends_with('s') || s.ends_with('S')) {
        Some(&s[..s.len() - 1])
    } else {
        None
    }
}

/// Convert an ordinal word ("first".."twelfth") to its number (1..12), else 0.
/// Case-insensitive.
pub fn ordinal_word_to_number(word: &str) -> i64 {
    const TABLE: &[(&str, i64)] = &[
        ("first", 1),
        ("second", 2),
        ("third", 3),
        ("fourth", 4),
        ("fifth", 5),
        ("sixth", 6),
        ("seventh", 7),
        ("eighth", 8),
        ("ninth", 9),
        ("tenth", 10),
        ("eleventh", 11),
        ("twelfth", 12),
    ];
    for (n, v) in TABLE {
        if word.eq_ignore_ascii_case(n) {
            return *v;
        }
    }
    0
}

/// Expand a 2-digit year: 00–69 → 2000–2069, 70–99 → 1970–1999.
pub fn two_digit_year(year: i64) -> i64 {
    if year < 100 {
        if year < 70 {
            year + 2000
        } else {
            year + 1900
        }
    } else {
        year
    }
}

/// Convert an hour to 24-hour form given an "am"/"pm" indicator (case-insensitive).
pub fn apply_ampm(hour: i64, ampm: &str) -> i64 {
    if ampm.eq_ignore_ascii_case("am") {
        if hour == 12 {
            0
        } else {
            hour
        }
    } else if hour == 12 {
        12
    } else {
        hour + 12
    }
}

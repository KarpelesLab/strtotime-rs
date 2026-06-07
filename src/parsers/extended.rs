//! Extended/long-tail formats — port of `extended_formats.go`.

use crate::civil::{days_in_month, weekday_from_days, days_from_civil};
use crate::lookups::{apply_ampm, day_of_week, month_by_name, normalize_unit, two_digit_year, Unit};
use crate::parsers::formats::{
    atoi, collect_fields, is_all_digits, is_valid_time, mk, parse_iso, parse_iso8601_time, tail_from,
};
use crate::parsers::token_parser::is_valid_date;
use crate::relmath::apply_offset;
use crate::tz::{self, Moment, Tz};

const NF: usize = 24; // max fields we track

fn is_alpha(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphabetic())
}

fn fixed(off: i32) -> Tz {
    if off == 0 {
        Tz::Utc
    } else {
        Tz::Fixed(off)
    }
}

/// "26th" → "26", "1st" → "1"; otherwise unchanged. Mirrors `stripOrdinalSuffix`.
fn strip_ordinal_suffix(s: &str) -> &str {
    for suf in ["st", "nd", "rd", "th"] {
        if s.len() > suf.len() && s[s.len() - suf.len()..].eq_ignore_ascii_case(suf) {
            let prefix = &s[..s.len() - suf.len()];
            if is_all_digits(prefix) {
                return prefix;
            }
        }
    }
    s
}

/// Parse an am/pm marker (case-insensitive), returning `"am"`/`"pm"`.
fn ampm_of(s: &str) -> Option<&'static str> {
    if s.eq_ignore_ascii_case("am") {
        Some("am")
    } else if s.eq_ignore_ascii_case("pm") {
        Some("pm")
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Compact timestamps / times
// ---------------------------------------------------------------------------

/// `YYYYMMDD`, `YYYYMMDDhhmmss` (+ optional tz). Mirrors `parseCompactTimestamp`.
pub fn parse_compact_timestamp(s: &str, base: Moment) -> Option<Moment> {
    let (digits, tz_str) = match s.find(' ') {
        Some(i) => (&s[..i], s[i + 1..].trim()),
        None => (s, ""),
    };

    if digits.len() == 8 && is_all_digits(digits) {
        let year = atoi(&digits[0..4]);
        let month = atoi(&digits[4..6]);
        let day = atoi(&digits[6..8]);
        if (1..=12).contains(&month) && (1..=31).contains(&day) {
            return Some(mk(base.tz, year, month, day, 0, 0, 0));
        }
        return None;
    }

    if digits.len() != 14 || !is_all_digits(digits) {
        return None;
    }
    let year = atoi(&digits[0..4]);
    let month = atoi(&digits[4..6]);
    let day = atoi(&digits[6..8]);
    let hour = atoi(&digits[8..10]);
    let minute = atoi(&digits[10..12]);
    let second = atoi(&digits[12..14]);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || !is_valid_time(hour, minute, second) {
        return None;
    }
    let mut tz = base.tz;
    if !tz_str.is_empty() {
        if let Some(t) = tz::parse_timezone(tz_str) {
            tz = t;
        }
    }
    Some(mk(tz, year, month, day, hour, minute, second))
}

/// `tHHMM`, dotted `HH.MM.SS[.f][TZ]`, `HHMMSS`, `YYYYDDD`. Mirrors
/// `parseCompactTimeFormats`.
pub fn parse_compact_time_formats(s: &str, base: Moment) -> Option<Moment> {
    let b = s.as_bytes();
    let now = base.wall();

    // "tHHMM"
    if b.len() >= 5 && (b[0] == b't' || b[0] == b'T') && b[1..5].iter().all(|c| c.is_ascii_digit()) {
        let hour = atoi(&s[1..3]);
        let minute = atoi(&s[3..5]);
        if is_valid_time(hour, minute, 0) {
            return Some(mk(base.tz, now.year, now.month as i64, now.day as i64, hour, minute, 0));
        }
    }

    // Dotted "HH.MM.SS[.frac][TZ]"
    if b.len() >= 8 && b[2] == b'.' && b[5] == b'.' && b[0..2].iter().all(|c| c.is_ascii_digit())
        && b[3..5].iter().all(|c| c.is_ascii_digit())
        && b[6].is_ascii_digit()
    {
        let hour = atoi(&s[0..2]);
        let minute = atoi(&s[3..5]);
        let mut pos = 6;
        while pos < b.len() && b[pos].is_ascii_digit() {
            pos += 1;
        }
        let second = atoi(&s[6..pos]);
        if !is_valid_time(hour, minute, second) {
            return None;
        }
        if pos < b.len() && b[pos] == b'.' {
            pos += 1;
            while pos < b.len() && b[pos].is_ascii_digit() {
                pos += 1;
            }
        }
        let mut tz = base.tz;
        if pos < b.len() {
            let tz_str = s[pos..].trim();
            match tz::parse_timezone(tz_str) {
                Some(t) => tz = t,
                None => return None,
            }
        }
        return Some(mk(tz, now.year, now.month as i64, now.day as i64, hour, minute, second));
    }

    if !is_all_digits(s) {
        return None;
    }

    // "HHMMSS"
    if s.len() == 6 {
        let hour = atoi(&s[0..2]);
        let minute = atoi(&s[2..4]);
        let second = atoi(&s[4..6]);
        if is_valid_time(hour, minute, second) {
            return Some(mk(base.tz, now.year, now.month as i64, now.day as i64, hour, minute, second));
        }
    }

    // "YYYYDDD" day-of-year.
    if s.len() == 7 {
        let year = atoi(&s[0..4]);
        let doy = atoi(&s[4..7]);
        if year >= 1 && (1..=366).contains(&doy) {
            let days = days_from_civil(year, 1, 1) + (doy - 1);
            let (y, _, _) = crate::civil::civil_from_days(days);
            if y == year {
                return Some(mk(base.tz, year, 1, doy, 0, 0, 0));
            }
        }
    }

    None
}

// ---------------------------------------------------------------------------
// Month-name dash dates
// ---------------------------------------------------------------------------

/// `Jan-15-2006`, `2006-Jan-15`, `15-Jan-2006`. Mirrors `parseMonthNameFormat`.
pub fn parse_month_name_format(s: &str, base: Moment) -> Option<Moment> {
    let mut it = s.split('-');
    let p0 = it.next()?;
    let p1 = it.next()?;
    let p2 = it.next()?;
    if it.next().is_some() {
        return None;
    }

    // Jan-15-2006
    if is_alpha(p0) && p0.len() >= 3 {
        if let (Some(day), Some(year)) = (parse_int(p1), parse_int(p2)) {
            if let Some(m) = month_by_name(p0) {
                if is_valid_date(year, m as i64, day) {
                    return Some(mk(base.tz, year, m as i64, day, 0, 0, 0));
                }
            }
        }
    }
    // 2006-Jan-15
    if p0.len() == 4 && is_alpha(p1) && p1.len() >= 3 {
        if let (Some(year), Some(day)) = (parse_int(p0), parse_int(p2)) {
            if let Some(m) = month_by_name(p1) {
                if is_valid_date(year, m as i64, day) {
                    return Some(mk(base.tz, year, m as i64, day, 0, 0, 0));
                }
            }
        }
    }
    // 15-Jan-2006 (or 2-digit year)
    if is_all_digits(p0) && is_alpha(p1) && p1.len() >= 3 && is_all_digits(p2) {
        if let (Some(day), Some(mut year)) = (parse_int(p0), parse_int(p2)) {
            if year < 100 {
                year = two_digit_year(year);
            }
            if let Some(m) = month_by_name(p1) {
                if is_valid_date(year, m as i64, day) {
                    return Some(mk(base.tz, year, m as i64, day, 0, 0, 0));
                }
            }
        }
    }
    None
}

fn parse_int(s: &str) -> Option<i64> {
    s.parse::<i64>().ok()
}

// ---------------------------------------------------------------------------
// HTTP log
// ---------------------------------------------------------------------------

/// `DD/Mon/YYYY:HH:MM:SS +0000`. Mirrors `parseHTTPLogFormat`.
pub fn parse_http_log_format(s: &str, _base: Moment) -> Option<Moment> {
    let sp = s.find(' ')?;
    let date_part = &s[..sp];
    let tz_off = s[sp + 1..].trim();

    let slash1 = date_part.find('/')?;
    if !(1..=2).contains(&slash1) {
        return None;
    }
    let slash2_rel = date_part[slash1 + 1..].find('/')?;
    let slash2 = slash1 + 1 + slash2_rel;

    let month_str = &date_part[slash1 + 1..slash2];
    if month_str.len() != 3 || !is_alpha(month_str) {
        return None;
    }
    let rest = &date_part[slash2 + 1..];
    let colon1 = rest.find(':')?;
    let year_str = &rest[..colon1];
    if year_str.len() != 4 {
        return None;
    }
    let time_str = &rest[colon1 + 1..];
    let mut tp = time_str.split(':');
    let (t0, t1, t2) = (tp.next()?, tp.next()?, tp.next()?);
    if tp.next().is_some() || t0.len() != 2 || t1.len() != 2 || t2.len() != 2 {
        return None;
    }
    let day = parse_int(&date_part[..slash1])?;
    let year = parse_int(year_str)?;
    let hour = parse_int(t0)?;
    let minute = parse_int(t1)?;
    let second = parse_int(t2)?;
    let month = month_by_name(month_str)? as i64;
    if !is_valid_date(year, month, day) || !is_valid_time(hour, minute, second) {
        return None;
    }

    let ob = tz_off.as_bytes();
    if tz_off.len() != 5 || (ob[0] != b'+' && ob[0] != b'-') {
        return None;
    }
    let tzh = parse_int(&tz_off[1..3])?;
    let tzm = parse_int(&tz_off[3..5])?;
    if !(0..=23).contains(&tzh) || !(0..=59).contains(&tzm) {
        return None;
    }
    let mut off = (tzh * 3600 + tzm * 60) as i32;
    if ob[0] == b'-' {
        off = -off;
    }
    Some(mk(fixed(off), year, month, day, hour, minute, second))
}

// ---------------------------------------------------------------------------
// Day-month-year (spaced / dashed / compact)
// ---------------------------------------------------------------------------

/// `DD Mon YYYY [time] [tz]`, `DD-Mon-YYYY`, with weekday prefix/ordinal.
/// Mirrors `parseDayMonthYear`.
pub fn parse_day_month_year(s: &str, base: Moment) -> Option<Moment> {
    let (after_prefix, prefix_day) = match crate::strip_weekday_prefix(s) {
        Some((rest, dn)) => (rest, dn),
        None => (s, -1),
    };

    // Collect fields; if the first field is hyphen-joined (24-Jan-2019), expand it.
    let mut raw = [""; NF];
    let rawn = collect_fields(after_prefix, &mut raw);
    if rawn < 2 {
        return parse_day_month_year_compact(after_prefix, base);
    }

    let mut fields = raw;
    let mut n = rawn;
    if raw[0].contains('-') {
        let mut hp = raw[0].splitn(3, '-');
        let a = hp.next().unwrap_or("");
        let b = hp.next();
        let c = hp.next();
        if let (Some(b), Some(c)) = (b, c) {
            let mut nf = [""; NF];
            nf[0] = a;
            nf[1] = b;
            nf[2] = c;
            let mut fc = 3;
            for i in 1..rawn {
                if fc >= NF {
                    break;
                }
                nf[fc] = raw[i];
                fc += 1;
            }
            fields = nf;
            n = fc;
        }
    }

    let mut idx = 0;
    let day = match parse_int(strip_ordinal_suffix(fields[idx])) {
        Some(d) if (1..=31).contains(&d) => d,
        _ => return parse_day_month_year_compact(after_prefix, base),
    };
    idx += 1;

    if idx >= n {
        return None;
    }
    let month = month_by_name(fields[idx])? as i64;
    idx += 1;

    if idx >= n {
        return None;
    }
    let mut year = parse_int(fields[idx])?;
    if year < 0 {
        return None;
    }
    if year < 100 {
        year = two_digit_year(year);
    }
    idx += 1;

    let (mut hour, mut minute, mut second) = (0i64, 0i64, 0i64);
    if idx < n && fields[idx].contains(':') {
        if let Some((h, m, sec, consumed)) = tz::parse_flex_time(fields[idx]) {
            hour = h as i64;
            minute = m as i64;
            second = sec as i64;
            let timef = fields[idx];
            idx += 1;
            let remaining = &timef[consumed..];
            if let Some(ap) = ampm_of(remaining) {
                hour = apply_ampm(hour, ap);
            } else if idx < n {
                if let Some(ap) = ampm_of(fields[idx]) {
                    hour = apply_ampm(hour, ap);
                    idx += 1;
                }
            }
        }
    }

    let mut tz = base.tz;
    if idx < n {
        let tz_str = tail_from(s, fields[idx]).trim();
        if let Some((off, _)) = tz::parse_numeric_offset(tz_str) {
            tz = fixed(off);
        } else if let Some(t) = tz::parse_timezone(tz_str) {
            tz = t;
        }
    }

    if month > 0 && day > 0 {
        let mut result = mk(tz, year, month, day, hour, minute, second);
        if prefix_day >= 0 {
            let wd = result.wall().weekday() as i64;
            if wd != prefix_day {
                let mut days = (prefix_day - wd + 7) % 7;
                if days == 0 {
                    days = 7;
                }
                result = mk(tz, year, month, day + days, hour, minute, second);
            }
        }
        return Some(result);
    }
    None
}

/// `DDMonYYYY`, `DDMon YYYY`, or `DDMon`. Mirrors `parseDayMonthYearCompact`.
fn parse_day_month_year_compact(s: &str, base: Moment) -> Option<Moment> {
    let s = s.trim();
    let b = s.as_bytes();
    if b.len() < 4 {
        return None;
    }
    let mut day_end = 0;
    while day_end < b.len() && b[day_end].is_ascii_digit() {
        day_end += 1;
    }
    if day_end == 0 || day_end > 2 {
        return None;
    }
    let day = atoi(&s[..day_end]);

    let mut month_end = day_end;
    while month_end < b.len() && b[month_end].is_ascii_alphabetic() {
        month_end += 1;
    }
    if month_end - day_end < 3 {
        return None;
    }
    let month = month_by_name(&s[day_end..month_end])? as i64;

    let rest = s[month_end..].trim();
    if rest.is_empty() {
        if !(1..=31).contains(&day) {
            return None;
        }
        let now = base.wall();
        return Some(mk(base.tz, now.year, month, day, 0, 0, 0));
    }

    let mut fields = [""; NF];
    let n = collect_fields(rest, &mut fields);
    if n == 0 {
        return None;
    }
    let mut year = parse_int(fields[0])?;
    if year < 100 {
        year = two_digit_year(year);
    }

    let (mut hour, mut minute, mut second) = (0i64, 0i64, 0i64);
    if n > 1 && fields[1].contains(':') {
        if let Some((h, m, sec, consumed)) = tz::parse_flex_time(fields[1]) {
            hour = h as i64;
            minute = m as i64;
            second = sec as i64;
            let remaining = &fields[1][consumed..];
            if let Some(ap) = ampm_of(remaining) {
                hour = apply_ampm(hour, ap);
            } else if n > 2 {
                if let Some(ap) = ampm_of(fields[2]) {
                    hour = apply_ampm(hour, ap);
                }
            }
        }
    }
    if !(1..=31).contains(&day) {
        return None;
    }
    Some(mk(base.tz, year, month, day, hour, minute, second))
}

// ---------------------------------------------------------------------------
// Month + year only
// ---------------------------------------------------------------------------

/// `Oct 2001` or `2001 Oct`. Mirrors `parseMonthYearOnly`.
pub fn parse_month_year_only(s: &str, base: Moment) -> Option<Moment> {
    let mut fields = [""; NF];
    let n = collect_fields(s, &mut fields);
    if n != 2 {
        return None;
    }
    if let Some(m) = month_by_name(fields[0]) {
        if let Some(year) = parse_int(fields[1]) {
            if year >= 100 || fields[1].len() >= 4 {
                return Some(mk(base.tz, year, m as i64, 1, 0, 0, 0));
            }
        }
    }
    if let Some(year) = parse_int(fields[0]) {
        if year >= 100 || fields[0].len() >= 4 {
            if let Some(m) = month_by_name(fields[1]) {
                return Some(mk(base.tz, year, m as i64, 1, 0, 0, 0));
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Time before date
// ---------------------------------------------------------------------------

/// `19:30 Dec 17 2005`, `17:00 2004-01-01`, `1pm Aug 1 GMT 2007`. Mirrors
/// `parseTimeBeforeDate`.
pub fn parse_time_before_date(s: &str, base: Moment) -> Option<Moment> {
    let mut fields = [""; NF];
    let n = collect_fields(s, &mut fields);
    if n < 2 {
        return None;
    }

    let (mut hour, mut minute, mut second) = (0i64, 0i64, 0i64);
    let mut time_field_end = 1;

    if fields[0].contains(':') {
        let (h, m, sec, _) = tz::parse_flex_time(fields[0])?;
        hour = h as i64;
        minute = m as i64;
        second = sec as i64;
        if !is_valid_time(hour, minute, second) {
            return None;
        }
        // AM/PM after the time.
        let f1 = fields[1];
        let f1l = f1;
        let is_ap = f1l.eq_ignore_ascii_case("am")
            || f1l.eq_ignore_ascii_case("pm")
            || f1l.eq_ignore_ascii_case("a.m.")
            || f1l.eq_ignore_ascii_case("p.m.");
        if is_ap {
            if f1l.starts_with(['a', 'A']) {
                hour = apply_ampm(hour, "am");
            } else {
                hour = apply_ampm(hour, "pm");
            }
            time_field_end = 2;
        }
    } else {
        let f = fields[0];
        let (ampm, num) = if f.len() >= 2 && f[f.len() - 2..].eq_ignore_ascii_case("pm") {
            ("pm", &f[..f.len() - 2])
        } else if f.len() >= 2 && f[f.len() - 2..].eq_ignore_ascii_case("am") {
            ("am", &f[..f.len() - 2])
        } else {
            return None;
        };
        let h = parse_int(num)?;
        if !(1..=12).contains(&h) {
            return None;
        }
        hour = apply_ampm(h, ampm);
    }

    // Date is the remaining fields.
    let date_str = tail_from(s, fields[time_field_end]).trim();

    if let Some(t) = parse_iso(date_str, base) {
        let w = t.wall();
        return Some(mk(base.tz, w.year, w.month as i64, w.day as i64, hour, minute, second));
    }

    let mut df = [""; NF];
    let dn = collect_fields(date_str, &mut df);
    if dn >= 2 {
        if let Some(month) = month_by_name(df[0]) {
            let day_str = strip_ordinal_suffix(df[1].trim_end_matches(','));
            if let Some(day) = parse_int(day_str) {
                if (1..=31).contains(&day) {
                    let mut year = base.wall().year;
                    let mut tz = base.tz;
                    let mut fidx = 2;
                    while fidx < dn {
                        if let Some(y) = parse_int(df[fidx]) {
                            if y > 0 {
                                year = y;
                                fidx += 1;
                                continue;
                            }
                        }
                        if let Some(t) = tz::parse_timezone(df[fidx]) {
                            tz = t;
                            fidx += 1;
                            continue;
                        }
                        break;
                    }
                    return Some(mk(tz, year, month as i64, day, hour, minute, second));
                }
            }
        }
    }

    None
}

// ---------------------------------------------------------------------------
// US date + 12h time
// ---------------------------------------------------------------------------

/// `MM/DD/YYYY H:MM AM`. Mirrors `parseUSDateWithTime`.
pub fn parse_us_date_with_time(s: &str, base: Moment) -> Option<Moment> {
    let mut fields = [""; NF];
    let n = collect_fields(s, &mut fields);
    if n < 2 {
        return None;
    }
    let t = crate::parsers::formats::parse_us(fields[0], base)?;
    let w = t.wall();
    if fields[1].contains(':') {
        let (h, m, sec, consumed) = tz::parse_flex_time(fields[1])?;
        let mut hour = h as i64;
        let remaining = &fields[1][consumed..];
        if let Some(ap) = ampm_of(remaining) {
            hour = apply_ampm(hour, ap);
        } else if n >= 3 {
            if let Some(ap) = ampm_of(fields[2]) {
                hour = apply_ampm(hour, ap);
            }
        }
        return Some(mk(base.tz, w.year, w.month as i64, w.day as i64, hour, m as i64, sec as i64));
    }
    None
}

// ---------------------------------------------------------------------------
// first/last day of <month-context>
// ---------------------------------------------------------------------------

/// `first day of YYYY-MM`, `last day of next month`, etc. Mirrors
/// `parseFirstLastDayOfDate`.
pub fn parse_first_last_day_of_date(s: &str, base: Moment) -> Option<Moment> {
    let t = s.trim();
    let (is_first, rest) = if t.len() >= 13 && t[..13].eq_ignore_ascii_case("first day of ") {
        (true, t[13..].trim())
    } else if t.len() >= 12 && t[..12].eq_ignore_ascii_case("last day of ") {
        (false, t[12..].trim())
    } else {
        return None;
    };

    let now = base.wall();

    // "+N month/year"
    if rest.starts_with('+') || rest.starts_with('-') {
        let mut f = [""; NF];
        let n = collect_fields(rest, &mut f);
        if n == 2 {
            if let Some(amount) = parse_int(f[0]) {
                let unit = normalize_unit(f[1]);
                let refm = match unit {
                    Some(Unit::Month) => apply_offset(base, amount, Unit::Month),
                    Some(Unit::Year) => apply_offset(base, amount, Unit::Year),
                    _ => return None,
                };
                let rw = refm.wall();
                let day = if is_first { 1 } else { days_in_month(rw.year, rw.month as i64) };
                return Some(mk(base.tz, rw.year, rw.month as i64, day, now.hour as i64, now.minute as i64, now.second as i64));
            }
        }
    }

    // YYYY-MM
    if let Some(t) = crate::parsers::formats::parse_year_month(rest, base) {
        let w = t.wall();
        let day = if is_first { 1 } else { days_in_month(w.year, w.month as i64) };
        return Some(mk(base.tz, w.year, w.month as i64, day, 0, 0, 0));
    }

    // Month name [year] [time]
    let mut f = [""; NF];
    let n = collect_fields(rest, &mut f);
    if n >= 1 {
        if let Some(month) = month_by_name(f[0]) {
            let mut idx = 1;
            let mut year = now.year;
            if idx < n {
                if let Some(y) = parse_int(f[idx]) {
                    year = y;
                    idx += 1;
                }
            }
            let (mut hour, mut minute, mut second) = (0i64, 0i64, 0i64);
            if idx < n {
                if let Some((h, m, sec, consumed)) = tz::parse_flex_time(f[idx]) {
                    if consumed == f[idx].len() {
                        hour = h as i64;
                        minute = m as i64;
                        second = sec as i64;
                        idx += 1;
                    }
                }
            }
            if idx != n {
                return None;
            }
            let day = if is_first { 1 } else { days_in_month(year, month as i64) };
            return Some(mk(base.tz, year, month as i64, day, hour, minute, second));
        }
    }

    None
}

// ---------------------------------------------------------------------------
// Ordinal date ("26th Nov")
// ---------------------------------------------------------------------------

/// `26th Nov [YYYY] [time]`. Mirrors `parseOrdinalDate`.
pub fn parse_ordinal_date(s: &str, base: Moment) -> Option<Moment> {
    let mut f = [""; NF];
    let n = collect_fields(s, &mut f);
    if n < 2 {
        return None;
    }
    let day = parse_int(strip_ordinal_suffix(f[0]))?;
    if !(1..=31).contains(&day) {
        return None;
    }
    let month = month_by_name(f[1])? as i64;
    let mut year = base.wall().year;
    let mut idx = 2;
    if idx < n {
        if let Some(y) = parse_int(f[idx]) {
            year = y;
            idx += 1;
        }
    }
    let (mut hour, mut minute, mut second) = (0i64, 0i64, 0i64);
    if idx < n && f[idx].contains(':') {
        if let Some((h, m, sec, _)) = tz::parse_flex_time(f[idx]) {
            hour = h as i64;
            minute = m as i64;
            second = sec as i64;
        }
    }
    Some(mk(base.tz, year, month, day, hour, minute, second))
}

// ---------------------------------------------------------------------------
// Month day time year ("Dec 17 19:30 2005")
// ---------------------------------------------------------------------------

/// `Dec 17 19:30 2005`. Mirrors `parseMonthDayTimeYear`.
pub fn parse_month_day_time_year(s: &str, base: Moment) -> Option<Moment> {
    let mut f = [""; NF];
    let n = collect_fields(s, &mut f);
    if n != 4 {
        return None;
    }
    let month = month_by_name(f[0])? as i64;
    let day = parse_int(strip_ordinal_suffix(f[1].trim_end_matches(',')))?;
    if !(1..=31).contains(&day) {
        return None;
    }
    if !f[2].contains(':') {
        return None;
    }
    let (h, m, sec, _) = tz::parse_flex_time(f[2])?;
    let year = parse_int(f[3])?;
    Some(mk(base.tz, year, month, day, h as i64, m as i64, sec as i64))
}

// ---------------------------------------------------------------------------
// Date + tz + relative
// ---------------------------------------------------------------------------

/// `YYYY-MM-DD TZ +N unit ...`. Mirrors `parseDateTimeTZRelative`.
pub fn parse_datetime_tz_relative(s: &str, base: Moment) -> Option<Moment> {
    let mut rels: [(i64, Unit); NF] = [(0, Unit::Day); NF];
    let mut rn = 0;
    let mut remaining = s;

    loop {
        remaining = remaining.trim_end_matches(' ');
        if remaining.is_empty() {
            break;
        }
        let rb = remaining.as_bytes();
        let mut found = false;
        let mut i = rb.len();
        while i > 1 {
            i -= 1;
            if (rb[i] == b'+' || rb[i] == b'-') && rb[i - 1] == b' ' {
                let rel_part = &remaining[i..];
                let mut rf = [""; 4];
                let rc = collect_fields(rel_part, &mut rf);
                if rc != 2 {
                    continue;
                }
                let Some(amount) = parse_int(rf[0]) else { continue };
                let Some(unit) = normalize_unit(rf[1]) else { continue };
                if rn < NF {
                    rels[rn] = (amount, unit);
                    rn += 1;
                }
                remaining = remaining[..i].trim();
                found = true;
                break;
            }
        }
        if !found {
            break;
        }
    }

    if rn == 0 {
        return None;
    }

    let date_part = remaining;
    let mut t = crate::parsers::iso8601::parse_iso8601(date_part, base)
        .or_else(|| crate::parsers::formats::parse_datetime(date_part, base))
        .or_else(|| crate::parsers::tzfmt::parse_iso_datetime_with_tz(date_part, base))
        .or_else(|| parse_date_with_tz(date_part, base))
        .or_else(|| parse_iso(date_part, base))
        .or_else(|| parse_day_month_year(date_part, base))?;

    // Apply in reverse (innermost first).
    for k in (0..rn).rev() {
        t = apply_offset(t, rels[k].0, rels[k].1);
    }
    Some(t)
}

/// `YYYY-MM-DD TZname`. Mirrors `parseDateWithTZ`.
pub fn parse_date_with_tz(s: &str, base: Moment) -> Option<Moment> {
    let mut f = [""; NF];
    let n = collect_fields(s, &mut f);
    if n != 2 {
        return None;
    }
    let t = parse_iso(f[0], base)?;
    let w = t.wall();
    let tz = tz::parse_timezone(f[1])?;
    Some(mk(tz, w.year, w.month as i64, w.day as i64, 0, 0, 0))
}

// ---------------------------------------------------------------------------
// front/back of
// ---------------------------------------------------------------------------

/// `front of 7` (6:45), `back of 7` (7:15), with am/pm. Mirrors `parseFrontBackOf`.
pub fn parse_front_back_of(s: &str, base: Moment) -> Option<Moment> {
    let t = s.trim();
    let (is_front, mut rest) = if t.len() >= 9 && t[..9].eq_ignore_ascii_case("front of ") {
        (true, t[9..].trim())
    } else if t.len() >= 8 && t[..8].eq_ignore_ascii_case("back of ") {
        (false, t[8..].trim())
    } else {
        return None;
    };

    let mut ampm = "";
    if rest.len() >= 2 && rest[rest.len() - 2..].eq_ignore_ascii_case("am") {
        ampm = "am";
        rest = rest[..rest.len() - 2].trim();
    } else if rest.len() >= 2 && rest[rest.len() - 2..].eq_ignore_ascii_case("pm") {
        ampm = "pm";
        rest = rest[..rest.len() - 2].trim();
    }

    let mut hour = parse_int(rest)?;
    if !(0..=24).contains(&hour) {
        return None;
    }
    let now = base.wall();
    if is_front {
        if ampm == "pm" {
            hour += 12;
        }
        return Some(mk(base.tz, now.year, now.month as i64, now.day as i64, hour - 1, 45, 0));
    }
    if !ampm.is_empty() {
        hour = apply_ampm(hour, ampm);
    }
    Some(mk(base.tz, now.year, now.month as i64, now.day as i64, hour, 15, 0))
}

// ---------------------------------------------------------------------------
// Roman numeral months
// ---------------------------------------------------------------------------

fn roman_month(s: &str) -> Option<i64> {
    const T: &[(&str, i64)] = &[
        ("i", 1), ("ii", 2), ("iii", 3), ("iv", 4), ("v", 5), ("vi", 6),
        ("vii", 7), ("viii", 8), ("ix", 9), ("x", 10), ("xi", 11), ("xii", 12),
    ];
    for (n, m) in T {
        if s.eq_ignore_ascii_case(n) {
            return Some(*m);
        }
    }
    None
}

/// `20 VI. 2005`. Mirrors `parseRomanNumeralDate`.
pub fn parse_roman_numeral_date(s: &str, base: Moment) -> Option<Moment> {
    let mut f = [""; NF];
    let n = collect_fields(s, &mut f);
    if n < 3 {
        return None;
    }
    let day = parse_int(f[0])?;
    if !(1..=31).contains(&day) {
        return None;
    }
    let month = roman_month(f[1].trim_end_matches('.'))?;
    let year = parse_int(f[2])?;
    if !is_valid_date(year, month, day) {
        return None;
    }
    Some(mk(base.tz, year, month, day, 0, 0, 0))
}

// ---------------------------------------------------------------------------
// Numbered weekday
// ---------------------------------------------------------------------------

/// Parse an ordinal prefix from `fields[idx]`. Returns `(ordinal, is_word,
/// next_idx)`. Mirrors `parseOrdinalPrefix`.
fn parse_ordinal_prefix(fields: &[&str], idx: usize) -> Option<(i64, bool, usize)> {
    if idx >= fields.len() {
        return None;
    }
    if let Some(nv) = parse_int(fields[idx]) {
        if nv <= 0 || nv > 53 {
            return None;
        }
        return Some((nv, false, idx + 1));
    }
    const WORDS: &[(&str, i64)] = &[
        ("first", 1), ("1st", 1), ("second", 2), ("2nd", 2), ("third", 3), ("3rd", 3),
        ("fourth", 4), ("4th", 4), ("fifth", 5), ("5th", 5), ("sixth", 6), ("6th", 6),
        ("seventh", 7), ("7th", 7), ("eighth", 8), ("8th", 8), ("ninth", 9), ("9th", 9),
        ("tenth", 10), ("10th", 10), ("eleventh", 11), ("11th", 11), ("twelfth", 12), ("12th", 12),
    ];
    for (w, v) in WORDS {
        if fields[idx].eq_ignore_ascii_case(w) {
            return Some((*v, true, idx + 1));
        }
    }
    if fields[idx].eq_ignore_ascii_case("last") {
        return Some((-1, false, idx + 1));
    }
    if day_of_week(fields[idx]).is_some() {
        return Some((1, false, idx)); // don't advance
    }
    None
}

/// `1 Monday December 2008`, `second Monday December 2008`, `+1 week Thursday Nov 2007`.
/// Mirrors `parseNumberedWeekday`.
pub fn parse_numbered_weekday(s: &str, base: Moment) -> Option<Moment> {
    let mut f = [""; NF];
    let n = collect_fields(s, &mut f);
    if n < 3 {
        return None;
    }
    let fields = &f[..n];

    let (ordinal, mut is_word, mut idx) = parse_ordinal_prefix(fields, 0)?;

    // "+N week(s) ..." — skip the unit after a numeric ordinal.
    if idx < n {
        if normalize_unit(fields[idx]) == Some(Unit::Week) {
            is_word = true;
            idx += 1;
        }
    }

    if idx >= n {
        return None;
    }
    let mut is_day_of_month = false;
    let dow = day_of_week(fields[idx]).map(|d| d as i64);
    let dow = match dow {
        Some(d) => d,
        None => {
            if fields[idx].eq_ignore_ascii_case("day") {
                if ordinal != 1 && ordinal != -1 {
                    return None;
                }
                is_day_of_month = true;
                -1
            } else {
                return None;
            }
        }
    };
    idx += 1;

    let mut has_of = false;
    if idx < n && fields[idx].eq_ignore_ascii_case("of") {
        has_of = true;
        idx += 1;
    }

    if idx >= n {
        return None;
    }

    let now = base.wall();
    let mut month;
    let mut year;
    let mut relative_years = 0i64;

    if fields[idx].eq_ignore_ascii_case("next") || fields[idx].eq_ignore_ascii_case("last") {
        let next = fields[idx].eq_ignore_ascii_case("next");
        idx += 1;
        if idx >= n {
            return None;
        }
        let unit = normalize_unit(fields[idx]);
        idx += 1;
        match unit {
            Some(Unit::Month) => {
                let refm = apply_offset(base, if next { 1 } else { -1 }, Unit::Month).wall();
                month = refm.month as i64;
                year = refm.year;
            }
            Some(Unit::Year) => {
                month = now.month as i64;
                year = now.year;
                relative_years = if next { 1 } else { -1 };
            }
            _ => return None,
        }
    } else {
        month = month_by_name(fields[idx])? as i64;
        idx += 1;
        year = now.year;
        if idx < n {
            let y = parse_int(fields[idx])?;
            if !(1..=9999).contains(&y) {
                return None;
            }
            year = y;
            idx += 1;
        }
    }

    let (mut th, mut tm, mut ts) = (0i64, 0i64, 0i64);
    let mut has_time = false;
    if idx < n {
        if let Some((h, m, sec, consumed)) = tz::parse_flex_time(fields[idx]) {
            if consumed == fields[idx].len() {
                th = h as i64;
                tm = m as i64;
                ts = sec as i64;
                has_time = true;
                idx += 1;
            }
        }
    }

    if idx != n {
        return None;
    }

    let result_day;
    if is_day_of_month {
        let last = days_in_month(year, month);
        if ordinal > 0 {
            if ordinal > last {
                return None;
            }
            result_day = ordinal;
        } else if ordinal == -1 {
            result_day = last;
        } else {
            return None;
        }
    } else if ordinal > 0 {
        let first_dow = weekday_from_days(days_from_civil(year, month, 1));
        let days_until_first = (dow - first_dow + 7) % 7;
        if is_word && !has_of && days_until_first == 0 {
            result_day = 1 + days_until_first + ordinal * 7;
        } else {
            result_day = 1 + days_until_first + (ordinal - 1) * 7;
        }
    } else if ordinal == -1 {
        if has_of {
            let mut nm = month + 1;
            let mut nmy = year;
            if nm > 12 {
                nm = 1;
                nmy += 1;
            }
            let first_dow = weekday_from_days(days_from_civil(nmy, nm, 1));
            let days_until = (dow - first_dow + 7) % 7;
            result_day = days_in_month(year, month) + 1 + days_until - 7;
        } else {
            let first_dow = weekday_from_days(days_from_civil(year, month, 1));
            let mut days_back = (first_dow - dow + 7) % 7;
            if days_back == 0 {
                days_back = 7;
            }
            return Some(mk(base.tz, year, month, 1 - days_back, 0, 0, 0));
        }
    } else {
        return None;
    }

    let (mut h, mut mi, mut sec) = (0i64, 0i64, 0i64);
    if is_day_of_month {
        h = now.hour as i64;
        mi = now.minute as i64;
        sec = now.second as i64;
    }
    if has_time {
        h = th;
        mi = tm;
        sec = ts;
    }
    Some(mk(base.tz, year + relative_years, month, result_day, h, mi, sec))
}

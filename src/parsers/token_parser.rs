//! Token-stream parser — port of the `Parser` type in the Go reference's
//! `strtotime.go`.
//!
//! Each `try_*` method attempts to consume one expression at the current
//! position, returning `Some(moment)` and advancing on success, or `None`
//! (restoring position where the Go code does) on no-match. The driver
//! [`Parser::parse`] runs them in the same order as the Go `Parse` loop and
//! errors on any leftover non-whitespace token.

use crate::civil;
use crate::datetime::Civil;
use crate::error::Error;
use crate::lookups::{apply_ampm, day_of_week, month_by_name, normalize_unit, ordinal_word_to_number, Unit};
use crate::relmath::apply_offset;
use crate::tokenizer::{TokType, Token};
use crate::tz::{self, Moment, Tz};

pub(crate) struct Parser<'a> {
    input: &'a str,
    toks: &'a [Token<'a>],
    pos: usize,
    result: Moment,
    tz: Tz,
    tz_found: bool,
    month_found: bool,
}

impl<'a> Parser<'a> {
    pub fn new(input: &'a str, toks: &'a [Token<'a>], base: Moment) -> Parser<'a> {
        Parser {
            input,
            toks,
            pos: 0,
            result: base,
            tz: base.tz,
            tz_found: false,
            month_found: false,
        }
    }

    // --- token helpers -----------------------------------------------------

    fn tok(&self, i: usize) -> Option<&Token<'a>> {
        self.toks.get(i)
    }

    fn typ(&self, i: usize) -> Option<TokType> {
        self.toks.get(i).map(|t| t.typ)
    }

    fn is(&self, i: usize, t: TokType) -> bool {
        self.typ(i) == Some(t)
    }

    fn val(&self, i: usize) -> &'a str {
        self.toks.get(i).map(|t| t.val).unwrap_or("")
    }

    fn skip_ws(&mut self) {
        while self.is(self.pos, TokType::Whitespace) {
            self.pos += 1;
        }
    }

    /// Substring of the original input spanning tokens `a..=b` (contiguous), used
    /// to assemble timezone paths without allocation.
    fn span(&self, a: usize, b: usize) -> &'a str {
        let start = self.toks[a].pos;
        let end = self.toks[b].pos + self.toks[b].val.len();
        &self.input[start..end]
    }

    fn mk(&self, y: i64, mo: i64, d: i64, h: i64, mi: i64, s: i64) -> Moment {
        Moment::from_civil(self.tz, Civil::new(y, mo, d, h, mi, s))
    }

    // --- driver ------------------------------------------------------------

    pub fn parse(&mut self) -> Result<Moment, Error> {
        self.skip_ws();

        if let Some(t) = self.try_standard_date()? {
            return Ok(t);
        }

        while self.pos < self.toks.len() {
            self.skip_ws();
            if self.pos >= self.toks.len() {
                break;
            }

            let mut parsed = false;

            if !self.tz_found && self.try_timezone() {
                parsed = true;
            }
            if !parsed {
                if let Some(t) = self.try_first_last_day_of() {
                    self.result = t;
                    parsed = true;
                }
            }
            if !parsed {
                if let Some(t) = self.try_next_last() {
                    self.result = t;
                    parsed = true;
                }
            }
            if !parsed {
                if let Some(t) = self.try_bare_weekday() {
                    self.result = t;
                    parsed = true;
                }
            }
            if !parsed {
                if let Some(t) = self.try_relative_time() {
                    self.result = t;
                    parsed = true;
                }
            }
            if !parsed {
                if let Some(t) = self.try_implicit_relative_time() {
                    self.result = t;
                    parsed = true;
                }
            }
            if !parsed {
                if let Some(t) = self.try_weekday_ago() {
                    self.result = t;
                    parsed = true;
                }
            }
            if !parsed {
                if let Some(t) = self.try_ordinal_relative_time() {
                    self.result = t;
                    parsed = true;
                }
            }
            if !parsed {
                if let Some(t) = self.try_time_expression() {
                    self.result = t;
                    parsed = true;
                }
            }
            if !parsed {
                if let Some(t) = self.try_bare_hour_ampm() {
                    self.result = t;
                    parsed = true;
                }
            }
            if !parsed {
                if let Some(t) = self.try_time_keyword() {
                    self.result = t;
                    parsed = true;
                }
            }
            if !parsed {
                if let Some(t) = self.try_month_only() {
                    self.result = t;
                    self.month_found = true;
                    parsed = true;
                }
            }
            if !parsed {
                if let Some(t) = self.try_month_name() {
                    self.result = t;
                    self.month_found = true;
                    parsed = true;
                }
            }
            if !parsed {
                if let Some(t) = self.try_year_only() {
                    self.result = t;
                    parsed = true;
                }
            }

            if !parsed && self.pos < self.toks.len() {
                let t = self.toks[self.pos];
                self.pos += 1;
                if t.typ != TokType::Whitespace {
                    return Err(Error::UnableToParse);
                }
            }

            self.skip_ws();
        }

        Ok(self.result)
    }

    // --- standard date (num op num op num) ---------------------------------

    fn try_standard_date(&mut self) -> Result<Option<Moment>, Error> {
        if self.pos + 4 >= self.toks.len() {
            return Ok(None);
        }
        if !self.is(self.pos, TokType::Number)
            || !self.is(self.pos + 2, TokType::Number)
            || !self.is(self.pos + 4, TokType::Number)
        {
            return Ok(None);
        }
        if !self.is(self.pos + 1, TokType::Operator) || !self.is(self.pos + 3, TokType::Operator) {
            return Ok(None);
        }
        let sep = self.val(self.pos + 1);
        if sep != self.val(self.pos + 3) {
            return Ok(None);
        }

        let first = parse_i64(self.val(self.pos))?;
        let second = parse_i64(self.val(self.pos + 2))?;
        let third = parse_i64(self.val(self.pos + 4))?;
        let first_len = self.val(self.pos).len();
        let third_len = self.val(self.pos + 4).len();

        let (year, month, day);
        match sep {
            "-" => {
                if first_len >= 4 {
                    year = first;
                    month = second;
                    day = third;
                    if year > 9999 {
                        return Ok(None);
                    }
                } else if third_len >= 4 {
                    day = first;
                    month = second;
                    year = third;
                } else {
                    year = if first < 100 { crate::lookups::two_digit_year(first) } else { first };
                    month = second;
                    day = third;
                }
            }
            "/" => {
                if first_len >= 4 {
                    year = first;
                    month = second;
                    day = third;
                } else if third_len >= 4 {
                    month = first;
                    day = second;
                    year = third;
                } else {
                    return Ok(None);
                }
            }
            "." => {
                day = first;
                month = second;
                year = if third < 100 { crate::lookups::two_digit_year(third) } else { third };
            }
            _ => return Ok(None),
        }

        if !is_valid_date(year, month, day) {
            return Ok(None);
        }

        self.pos += 5;
        Ok(Some(self.mk(year, month, day, 0, 0, 0)))
    }

    // --- timezone ----------------------------------------------------------

    fn try_timezone(&mut self) -> bool {
        if !self.is(self.pos, TokType::Str) {
            return false;
        }
        let start = self.pos;

        // Single token.
        if let Some(loc) = tz::parse_timezone(self.val(self.pos)) {
            self.set_tz(loc);
            self.pos += 1;
            return true;
        }

        // Extend with '/' or '-' separators (timezone paths).
        let mut best: Option<(Tz, usize)> = None;
        let mut p = self.pos + 1;
        while p + 1 < self.toks.len() {
            let sep = self.toks[p];
            if sep.typ != TokType::Operator || (sep.val != "/" && sep.val != "-") {
                break;
            }
            if self.toks[p + 1].typ != TokType::Str {
                break;
            }
            p += 2;
            if let Some(loc) = tz::parse_timezone(self.span(start, p - 1)) {
                best = Some((loc, p));
            }
        }
        if let Some((loc, end)) = best {
            self.set_tz(loc);
            self.pos = end;
            return true;
        }

        // Multi-word ("eastern time").
        if self.pos + 2 < self.toks.len()
            && self.is(self.pos + 1, TokType::Whitespace)
            && self.is(self.pos + 2, TokType::Str)
        {
            if let Some(loc) = tz::parse_timezone(self.span(self.pos, self.pos + 2)) {
                self.set_tz(loc);
                self.pos += 3;
                return true;
            }
        }

        self.pos = start;
        false
    }

    fn set_tz(&mut self, loc: Tz) {
        self.tz = loc;
        self.tz_found = true;
        self.result = self.result.in_tz(loc);
    }

    // --- next / last / this ------------------------------------------------

    fn try_next_last(&mut self) -> Option<Moment> {
        if !self.is(self.pos, TokType::Str) {
            return None;
        }
        let word = self.val(self.pos);
        let (is_next, is_this) = if word.eq_ignore_ascii_case("next") {
            (true, false)
        } else if word.eq_ignore_ascii_case("last") {
            (false, false)
        } else if word.eq_ignore_ascii_case("this") {
            (false, true)
        } else {
            return None;
        };
        let start = self.pos;
        self.pos += 1;
        self.skip_ws();

        if !self.is(self.pos, TokType::Str) {
            self.pos = start;
            return None;
        }
        let unit = self.val(self.pos);
        self.pos += 1;

        let w = self.result.wall();
        let weekday = w.weekday() as i64;

        // "week"
        if unit.eq_ignore_ascii_case("week") {
            let days_since_monday = (weekday + 6) % 7;
            let delta = if is_next {
                7 - days_since_monday
            } else if is_this {
                -days_since_monday
            } else {
                -(days_since_monday + 7)
            };
            return Some(crate::relmath::apply_offset(self.result, delta, Unit::Day));
        }

        // weekday
        if let Some(dn) = day_of_week(unit) {
            let dn = dn as i64;
            let cur = weekday;
            let days = if is_this {
                (dn - cur + 7) % 7
            } else if is_next {
                let d = (dn - cur + 7) % 7;
                if d == 0 {
                    7
                } else {
                    d
                }
            } else {
                let d = (cur - dn + 7) % 7;
                let d = if d == 0 { 7 } else { d };
                -d
            };
            let target = crate::relmath::apply_offset(self.result, days, Unit::Day);
            let tw = target.wall();
            return Some(self.mk(tw.year, tw.month as i64, tw.day as i64, 0, 0, 0));
        }

        match normalize_unit(unit) {
            Some(Unit::Month) => Some(apply_offset(self.result, if is_next { 1 } else { -1 }, Unit::Month)),
            Some(Unit::Year) => Some(apply_offset(self.result, if is_next { 1 } else { -1 }, Unit::Year)),
            _ => {
                // Invalid unit after next/last: Go returns (false, err) → not matched.
                self.pos = start;
                None
            }
        }
    }

    // --- relative time "+1 day" --------------------------------------------

    fn try_relative_time(&mut self) -> Option<Moment> {
        if !self.is(self.pos, TokType::Operator) {
            return None;
        }
        let op = self.val(self.pos);
        if op.len() > 1 && !op.contains('-') {
            return None;
        }
        let mut sign = 1i64;
        for c in op.bytes() {
            match c {
                b'-' => sign = -sign,
                b'+' => {}
                _ => return None,
            }
        }
        let start = self.pos;
        self.pos += 1;

        if !self.is(self.pos, TokType::Number) {
            self.pos = start;
            return None;
        }
        let amount = match parse_i64(self.val(self.pos)) {
            Ok(n) => n * sign,
            Err(_) => {
                self.pos = start;
                return None;
            }
        };
        self.pos += 1;
        self.skip_ws();

        if !self.is(self.pos, TokType::Str) {
            self.pos = start;
            return None;
        }
        let Some(unit) = normalize_unit(self.val(self.pos)) else {
            self.pos = start;
            return None;
        };
        self.pos += 1;
        Some(apply_offset(self.result, amount, unit))
    }

    // --- implicit relative time "4 days" -----------------------------------

    fn try_implicit_relative_time(&mut self) -> Option<Moment> {
        if !self.is(self.pos, TokType::Number) {
            return None;
        }
        let start = self.pos;
        let mut amount = parse_i64(self.val(self.pos)).ok()?;
        self.pos += 1;
        self.skip_ws();

        if !self.is(self.pos, TokType::Str) {
            self.pos = start;
            return None;
        }
        let unit_str = self.val(self.pos);
        let Some(unit) = normalize_unit(unit_str) else {
            self.pos = start;
            return None;
        };
        self.pos += 1;

        // Optional "ago".
        let after_unit = self.pos;
        self.skip_ws();
        if self.is(self.pos, TokType::Str) && self.val(self.pos).eq_ignore_ascii_case("ago") {
            amount = -amount;
            self.pos += 1;
        } else {
            self.pos = after_unit;
        }

        Some(apply_offset(self.result, amount, unit))
    }

    // --- "N weekday ago" ---------------------------------------------------

    fn try_weekday_ago(&mut self) -> Option<Moment> {
        if !self.is(self.pos, TokType::Number) {
            return None;
        }
        let start = self.pos;
        let amount = parse_i64(self.val(self.pos)).ok()?;
        if amount <= 0 {
            return None;
        }
        self.pos += 1;
        self.skip_ws();

        if !self.is(self.pos, TokType::Str) {
            self.pos = start;
            return None;
        }
        let name = self.val(self.pos);
        let singular = name.strip_suffix('s').or_else(|| name.strip_suffix('S')).unwrap_or(name);
        let dn = day_of_week(singular).or_else(|| day_of_week(name));
        let Some(dn) = dn else {
            self.pos = start;
            return None;
        };
        self.pos += 1;
        self.skip_ws();

        if !(self.is(self.pos, TokType::Str) && self.val(self.pos).eq_ignore_ascii_case("ago")) {
            self.pos = start;
            return None;
        }
        self.pos += 1;

        let w = self.result.wall();
        let cur = w.weekday() as i64;
        let mut days_since = (cur - dn as i64 + 7) % 7;
        if days_since == 0 {
            days_since = 7;
        }
        let total = days_since + (amount - 1) * 7;
        Some(apply_offset(self.result, -total, Unit::Day))
    }

    // --- ordinal word + unit ("eighth day") --------------------------------

    fn try_ordinal_relative_time(&mut self) -> Option<Moment> {
        if !self.is(self.pos, TokType::Str) {
            return None;
        }
        let start = self.pos;
        let amount = ordinal_word_to_number(self.val(self.pos));
        if amount <= 0 {
            return None;
        }
        self.pos += 1;
        self.skip_ws();

        if !self.is(self.pos, TokType::Str) {
            self.pos = start;
            return None;
        }
        let Some(unit) = normalize_unit(self.val(self.pos)) else {
            self.pos = start;
            return None;
        };
        self.pos += 1;
        Some(apply_offset(self.result, amount, unit))
    }

    // --- standalone time "HH:MM[:SS]" --------------------------------------

    fn try_time_expression(&mut self) -> Option<Moment> {
        if self.pos + 2 >= self.toks.len() {
            return None;
        }
        if !self.is(self.pos, TokType::Number)
            || !(self.is(self.pos + 1, TokType::Operator) && self.val(self.pos + 1) == ":")
            || !self.is(self.pos + 2, TokType::Number)
        {
            return None;
        }
        let hour = parse_i64(self.val(self.pos)).ok()?;
        if !(0..=23).contains(&hour) {
            return None;
        }
        self.pos += 2;
        let minute = parse_i64(self.val(self.pos)).ok()?;
        if !(0..=59).contains(&minute) {
            return None;
        }
        self.pos += 1;

        let mut second = 0;
        if self.pos + 1 < self.toks.len()
            && self.is(self.pos, TokType::Operator)
            && self.val(self.pos) == ":"
            && self.is(self.pos + 1, TokType::Number)
        {
            self.pos += 1;
            if let Ok(s) = parse_i64(self.val(self.pos)) {
                if (0..=59).contains(&s) {
                    second = s;
                    self.pos += 1;
                }
            }
        }

        let w = self.result.wall();
        Some(self.mk(w.year, w.month as i64, w.day as i64, hour, minute, second))
    }

    // --- bare hour + am/pm "10am" ------------------------------------------

    fn try_bare_hour_ampm(&mut self) -> Option<Moment> {
        if !self.is(self.pos, TokType::Number) {
            return None;
        }
        let hour = parse_i64(self.val(self.pos)).ok()?;
        if !(1..=12).contains(&hour) {
            return None;
        }
        let mut next = self.pos + 1;
        if self.is(next, TokType::Whitespace) {
            next += 1;
        }
        if !self.is(next, TokType::Str) {
            return None;
        }
        let ap = self.val(next);
        if !(ap.eq_ignore_ascii_case("am") || ap.eq_ignore_ascii_case("pm")) {
            return None;
        }
        let hour = apply_ampm(hour, ap);
        self.pos = next + 1;
        let w = self.result.wall();
        Some(self.mk(w.year, w.month as i64, w.day as i64, hour, 0, 0))
    }

    // --- "midnight" / "noon" -----------------------------------------------

    fn try_time_keyword(&mut self) -> Option<Moment> {
        if !self.is(self.pos, TokType::Str) {
            return None;
        }
        let w = self.result.wall();
        let word = self.val(self.pos);
        if word.eq_ignore_ascii_case("midnight") {
            self.pos += 1;
            Some(self.mk(w.year, w.month as i64, w.day as i64, 0, 0, 0))
        } else if word.eq_ignore_ascii_case("noon") {
            self.pos += 1;
            Some(self.mk(w.year, w.month as i64, w.day as i64, 12, 0, 0))
        } else {
            None
        }
    }

    // --- first/last day of this/next/last month/year -----------------------

    fn try_first_last_day_of(&mut self) -> Option<Moment> {
        if !self.is(self.pos, TokType::Str) {
            return None;
        }
        let start = self.pos;
        let is_first = if self.val(self.pos).eq_ignore_ascii_case("first") {
            true
        } else if self.val(self.pos).eq_ignore_ascii_case("last") {
            false
        } else {
            return None;
        };
        self.pos += 1;
        self.skip_ws();

        if !(self.is(self.pos, TokType::Str) && self.val(self.pos).eq_ignore_ascii_case("day")) {
            self.pos = start;
            return None;
        }
        self.pos += 1;
        self.skip_ws();
        if !(self.is(self.pos, TokType::Str) && self.val(self.pos).eq_ignore_ascii_case("of")) {
            self.pos = start;
            return None;
        }
        self.pos += 1;
        self.skip_ws();
        if !self.is(self.pos, TokType::Str) {
            self.pos = start;
            return None;
        }
        let dir = self.val(self.pos);
        let (is_next, is_last) = if dir.eq_ignore_ascii_case("next") {
            (true, false)
        } else if dir.eq_ignore_ascii_case("last") {
            (false, true)
        } else if dir.eq_ignore_ascii_case("this") {
            (false, false)
        } else {
            self.pos = start;
            return None;
        };
        self.pos += 1;
        self.skip_ws();
        if !self.is(self.pos, TokType::Str) {
            self.pos = start;
            return None;
        }
        let unit = normalize_unit(self.val(self.pos));
        self.pos += 1;

        let w = self.result.wall();
        let (mut year, mut month) = (w.year, w.month as i64);
        match unit {
            Some(Unit::Month) => {
                if is_next {
                    let r = apply_offset(self.result, 1, Unit::Month).wall();
                    year = r.year;
                    month = r.month as i64;
                } else if is_last {
                    let r = apply_offset(self.result, -1, Unit::Month).wall();
                    year = r.year;
                    month = r.month as i64;
                }
            }
            Some(Unit::Year) => {
                if is_next {
                    year = w.year + 1;
                    month = 1;
                } else if is_last {
                    year = w.year - 1;
                    month = 12;
                }
            }
            _ => {
                self.pos = start;
                return None;
            }
        }

        let day = if is_first { 1 } else { civil::days_in_month(year, month) };
        Some(self.mk(year, month, day, w.hour as i64, w.minute as i64, w.second as i64))
    }

    // --- bare weekday & "weekday next/last week [time]" & "weekday month [year]"
    fn try_bare_weekday(&mut self) -> Option<Moment> {
        if !self.is(self.pos, TokType::Str) {
            return None;
        }
        let Some(dn) = day_of_week(self.val(self.pos)) else {
            return None;
        };
        let dn = dn as i64;
        self.pos += 1;
        self.skip_ws();

        if self.is(self.pos, TokType::Str) {
            let dir_word = self.val(self.pos);
            let is_dir = dir_word.eq_ignore_ascii_case("next")
                || dir_word.eq_ignore_ascii_case("last")
                || dir_word.eq_ignore_ascii_case("this");
            if is_dir {
                let saved = self.pos;
                self.pos += 1;
                self.skip_ws();
                if self.is(self.pos, TokType::Str) && self.val(self.pos).eq_ignore_ascii_case("week") {
                    self.pos += 1;
                    let w = self.result.wall();
                    let cur = w.weekday() as i64;
                    let days_since_monday = (cur + 6) % 7;
                    let monday_delta = if dir_word.eq_ignore_ascii_case("next") {
                        7 - days_since_monday
                    } else if dir_word.eq_ignore_ascii_case("this") {
                        -days_since_monday
                    } else {
                        -(days_since_monday + 7)
                    };
                    let target_offset = (dn + 6) % 7;
                    let result = apply_offset(self.result, monday_delta + target_offset, Unit::Day);
                    let rw = result.wall();
                    let (mut hour, mut minute) = (0i64, 0i64);

                    self.skip_ws();
                    if self.pos + 2 < self.toks.len()
                        && self.is(self.pos, TokType::Number)
                        && self.is(self.pos + 1, TokType::Operator)
                        && self.val(self.pos + 1) == ":"
                        && self.is(self.pos + 2, TokType::Number)
                    {
                        if let Ok(h) = parse_i64(self.val(self.pos)) {
                            if (0..=23).contains(&h) {
                                hour = h;
                                self.pos += 2;
                                if let Ok(m) = parse_i64(self.val(self.pos)) {
                                    if (0..=59).contains(&m) {
                                        minute = m;
                                        self.pos += 1;
                                    }
                                }
                            }
                        }
                    }
                    return Some(self.mk(rw.year, rw.month as i64, rw.day as i64, hour, minute, 0));
                }
                self.pos = saved;
            }

            // weekday + month [year]
            if let Some(m) = month_by_name(self.val(self.pos)) {
                self.pos += 1;
                self.skip_ws();
                let mut year = self.result.wall().year;
                if self.is(self.pos, TokType::Number) {
                    if let Ok(y) = parse_i64(self.val(self.pos)) {
                        if y > 0 {
                            year = y;
                            self.pos += 1;
                        }
                    }
                }
                let first_dow =
                    civil::weekday_from_days(civil::days_from_civil(year, m as i64, 1));
                let days_until = (dn - first_dow + 7) % 7;
                let result_day = 1 + days_until;
                return Some(self.mk(year, m as i64, result_day, 0, 0, 0));
            }
        }

        // Bare weekday.
        let w = self.result.wall();
        let cur = w.weekday() as i64;
        let days_until = (dn - cur + 7) % 7;
        let target = apply_offset(self.result, days_until, Unit::Day).wall();
        Some(self.mk(target.year, target.month as i64, target.day as i64, 0, 0, 0))
    }

    // --- month only ("January") --------------------------------------------

    fn try_month_only(&mut self) -> Option<Moment> {
        if !self.is(self.pos, TokType::Str) {
            return None;
        }
        // Defer to month-name format if followed by a day number (not a time).
        if self.pos + 1 < self.toks.len() {
            let nt = self.toks[self.pos + 1];
            if nt.typ == TokType::Number {
                let num_idx = self.pos + 1;
                let is_time = num_idx + 1 < self.toks.len()
                    && self.is(num_idx + 1, TokType::Operator)
                    && self.val(num_idx + 1) == ":";
                if !is_time {
                    return None;
                }
            }
            if nt.typ == TokType::Operator && nt.val == "." {
                return None;
            }
            if nt.typ == TokType::Whitespace
                && self.pos + 2 < self.toks.len()
                && self.is(self.pos + 2, TokType::Number)
            {
                let num_idx = self.pos + 2;
                let is_time = num_idx + 1 < self.toks.len()
                    && self.is(num_idx + 1, TokType::Operator)
                    && self.val(num_idx + 1) == ":";
                if !is_time {
                    return None;
                }
            }
        }

        let month = month_by_name(self.val(self.pos))? as i64;
        self.pos += 1;

        let w = self.result.wall();
        let year = w.year;
        let mut day = w.day as i64;
        let max = civil::days_in_month(year, month);
        if day > max {
            day = max;
        }
        Some(self.mk(year, month, day, 0, 0, 0))
    }

    // --- month name ("January 15 2023 [HH:MM:SS] [TZ]") --------------------

    fn try_month_name(&mut self) -> Option<Moment> {
        if !self.is(self.pos, TokType::Str) {
            return None;
        }
        let month = month_by_name(self.val(self.pos))? as i64;
        self.pos += 1;

        if self.is(self.pos, TokType::Operator) && self.val(self.pos) == "." {
            self.pos += 1;
        }
        self.skip_ws();

        if !self.is(self.pos, TokType::Number) {
            return None;
        }
        let day = parse_i64(self.val(self.pos)).ok()?;
        self.pos += 1;

        // ordinal suffix
        if self.is(self.pos, TokType::Str) {
            let suf = self.val(self.pos);
            if ["st", "nd", "rd", "th"].iter().any(|s| suf.eq_ignore_ascii_case(s)) {
                self.pos += 1;
            }
        }
        if self.is(self.pos, TokType::Punctuation) {
            self.pos += 1;
        }
        self.skip_ws();

        let mut year = self.result.wall().year;
        if self.is(self.pos, TokType::Number) {
            let is_time = self.is(self.pos + 1, TokType::Operator) && self.val(self.pos + 1) == ":";
            if !is_time {
                year = parse_i64(self.val(self.pos)).ok()?;
                self.pos += 1;
            }
        }

        if !is_valid_date(year, month, day) {
            return None;
        }

        let (mut hour, mut minute, mut second) = (0i64, 0i64, 0i64);
        self.skip_ws();
        if self.pos + 2 < self.toks.len()
            && self.is(self.pos, TokType::Number)
            && self.is(self.pos + 1, TokType::Operator)
            && self.val(self.pos + 1) == ":"
            && self.is(self.pos + 2, TokType::Number)
        {
            if let Ok(h) = parse_i64(self.val(self.pos)) {
                if (0..=23).contains(&h) {
                    hour = h;
                    self.pos += 2;
                    if let Ok(m) = parse_i64(self.val(self.pos)) {
                        if (0..=59).contains(&m) {
                            minute = m;
                            self.pos += 1;
                            if self.pos + 1 < self.toks.len()
                                && self.is(self.pos, TokType::Operator)
                                && self.val(self.pos) == ":"
                                && self.is(self.pos + 1, TokType::Number)
                            {
                                self.pos += 1;
                                if let Ok(s) = parse_i64(self.val(self.pos)) {
                                    if (0..=59).contains(&s) {
                                        second = s;
                                        self.pos += 1;
                                    }
                                }
                            }
                            self.skip_ws();
                            if self.is(self.pos, TokType::Str) {
                                let ap = self.val(self.pos);
                                if ap.eq_ignore_ascii_case("am") || ap.eq_ignore_ascii_case("pm") {
                                    hour = apply_ampm(hour, ap);
                                    self.pos += 1;
                                }
                            }
                        }
                    }
                }
            }
        }

        // optional timezone
        self.skip_ws();
        let tz_start = self.pos;
        if !self.try_timezone() {
            self.pos = tz_start;
        }

        Some(self.mk(year, month, day, hour, minute, second))
    }

    // --- bare 4-digit year -------------------------------------------------

    fn try_year_only(&mut self) -> Option<Moment> {
        if !self.is(self.pos, TokType::Number) {
            return None;
        }
        let v = self.val(self.pos);
        if v.len() != 4 {
            return None;
        }
        let num = parse_i64(v).ok()?;
        if num < 1 {
            return None;
        }
        // Only if it is the last non-whitespace token.
        let mut next = self.pos + 1;
        while self.is(next, TokType::Whitespace) {
            next += 1;
        }
        if next != self.toks.len() {
            return None;
        }
        self.pos += 1;

        let w = self.result.wall();
        if self.month_found {
            let hour = num / 100;
            let minute = num % 100;
            if hour <= 23 && minute <= 59 {
                return Some(self.mk(w.year, w.month as i64, w.day as i64, hour, minute, 0));
            }
        }
        Some(self.mk(num, w.month as i64, w.day as i64, w.hour as i64, w.minute as i64, w.second as i64))
    }
}

/// Parse a pure-digit token into an i64 (no sign; tokens never carry one).
fn parse_i64(s: &str) -> Result<i64, Error> {
    s.parse::<i64>().map_err(|_| Error::InvalidNumber)
}

/// Validate calendar date components (Gregorian), matching `IsValidDate`.
pub(crate) fn is_valid_date(year: i64, month: i64, day: i64) -> bool {
    if year < 1 || !(1..=12).contains(&month) || day < 1 {
        return false;
    }
    day <= civil::days_in_month(year, month)
}

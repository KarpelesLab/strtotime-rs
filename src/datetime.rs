//! The public [`DateTime`] result and the internal [`Civil`] working type.

use crate::civil;

/// A resolved date-time, broken down into civil fields plus the UTC offset that
/// was in effect.
///
/// This is what [`crate::strtotime_civil`] returns. Call [`DateTime::unix`] for
/// the Unix timestamp (what [`crate::strtotime`] returns directly).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DateTime {
    /// Proleptic-Gregorian year (may be negative or > 9999).
    pub year: i64,
    /// Month, 1..=12.
    pub month: u8,
    /// Day of month, 1..=31.
    pub day: u8,
    /// Hour, 0..=23.
    pub hour: u8,
    /// Minute, 0..=59.
    pub minute: u8,
    /// Second, 0..=59.
    pub second: u8,
    /// Offset from UTC in seconds, east positive.
    pub offset: i32,
}

impl DateTime {
    /// The Unix timestamp (seconds since 1970-01-01T00:00:00Z) for this instant.
    pub fn unix(&self) -> i64 {
        civil::unix_from_civil(
            self.year,
            self.month as i64,
            self.day as i64,
            self.hour as i64,
            self.minute as i64,
            self.second as i64,
        ) - self.offset as i64
    }

    /// Day of week, 0 = Sunday .. 6 = Saturday.
    pub fn weekday(&self) -> u8 {
        civil::weekday_from_days(civil::days_from_civil(
            self.year,
            self.month as i64,
            self.day as i64,
        )) as u8
    }

    /// Build a `DateTime` from a UTC instant and the offset in effect there.
    /// The wall-clock fields are the local representation `unix + offset`.
    pub(crate) fn from_unix_offset(unix: i64, offset: i32) -> DateTime {
        let local = unix + offset as i64;
        let days = local.div_euclid(86400);
        let secs = local.rem_euclid(86400);
        let (year, month, day) = civil::civil_from_days(days);
        DateTime {
            year,
            month: month as u8,
            day: day as u8,
            hour: (secs / 3600) as u8,
            minute: ((secs % 3600) / 60) as u8,
            second: (secs % 60) as u8,
            offset,
        }
    }
}

/// Internal wall-clock working value used by the parsers. Fields may be filled
/// out of range (e.g. day 32, month 0, hour 25); they carry linearly when
/// converted to an instant, matching Go's `time.Date` normalization.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Civil {
    pub y: i64,
    pub mo: i64,
    pub d: i64,
    pub h: i64,
    pub mi: i64,
    pub s: i64,
}

impl Civil {
    pub fn new(y: i64, mo: i64, d: i64, h: i64, mi: i64, s: i64) -> Civil {
        Civil { y, mo, d, h, mi, s }
    }

    /// Normalize the month into the year so the month lands in 1..=12. Day and
    /// clock fields stay as-is (they carry linearly in [`Civil::unix_utc`]).
    pub fn norm_month(mut self) -> Civil {
        let m0 = self.mo - 1;
        self.y += m0.div_euclid(12);
        self.mo = m0.rem_euclid(12) + 1;
        self
    }

    /// Seconds since the epoch if these wall fields are interpreted as UTC.
    /// (For zoned times, subtract the offset separately.)
    pub fn unix_utc(self) -> i64 {
        let n = self.norm_month();
        civil::unix_from_civil(n.y, n.mo, n.d, n.h, n.mi, n.s)
    }
}

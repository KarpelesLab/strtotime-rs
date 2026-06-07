//! Error type returned by the parser.

use core::fmt;

/// Errors that can occur while parsing a time expression.
///
/// The parser mirrors PHP's `strtotime()`: any input PHP rejects produces an
/// error here. The variants are intentionally coarse — PHP itself only reports
/// success or failure — but they carry enough detail to be useful.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// The input was empty (or only whitespace).
    EmptyInput,
    /// The input could not be parsed by any known format.
    UnableToParse,
    /// A numeric component could not be parsed or was out of range.
    InvalidNumber,
    /// The date components do not form a valid date.
    InvalidDate,
    /// The time components do not form a valid time.
    InvalidTime,
    /// A timezone specification was present but not recognized.
    InvalidTimezone,
    /// The input produced more tokens than the fixed-size buffer can hold.
    TooLong,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let msg = match self {
            Error::EmptyInput => "empty time string",
            Error::UnableToParse => "unable to parse time string",
            Error::InvalidNumber => "invalid number",
            Error::InvalidDate => "invalid date component",
            Error::InvalidTime => "invalid time component",
            Error::InvalidTimezone => "invalid timezone",
            Error::TooLong => "input too long",
        };
        f.write_str(msg)
    }
}

impl core::error::Error for Error {}

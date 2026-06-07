# strtotime

[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

A `#![no_std]`, allocation-free Rust library that parses PHP-style date/time
expressions into a Unix timestamp — a port of the Go library
[`strtotime`](https://github.com/KarpelesLab/strtotime), which mirrors PHP's
[`strtotime()`](https://www.php.net/manual/en/function.strtotime.php).

It is validated against a corpus of 669 cases captured from real PHP (plus a
rejection corpus), and matches PHP's output on **every** one — including
timezone- and DST-sensitive results and extreme-year `int64` overflow.

## Highlights

- **`no_std` + no `alloc`.** The parser never allocates; tokens borrow slices of
  the input and the working set lives on the stack.
- **Full IANA timezones** (DST-aware) via the no_std/no-alloc
  [`timezone-data`](https://github.com/KarpelesLab/timezone-data-rs) crate,
  behind the default `iana` feature.
- **PHP-faithful**: relative expressions, weekday math, month-name dates, ISO
  8601, compact timestamps, HTTP-log dates, ordinals, "first/last day of …",
  compound expressions, and more.

## Usage

```rust
use strtotime::{strtotime, strtotime_civil, Tz};

// Absolute date.
assert_eq!(strtotime("2000-01-01 12:00:00", 0, Tz::Utc).unwrap(), 946728000);

// Relative to a base timestamp.
let base = 946728000; // 2000-01-01 12:00:00 UTC
assert_eq!(strtotime("tomorrow", base, Tz::Utc).unwrap(), 946771200);
assert_eq!(strtotime("next year + 4 days", base, Tz::Utc).unwrap(), 978696000);

// A fixed numeric offset.
assert_eq!(strtotime("2023-01-15 09:00 EST", 0, Tz::Fixed(-5 * 3600)).is_ok(), true);

// Broken-down result with the UTC offset in effect.
let dt = strtotime_civil("2008-07-01 22:35:17", 0, Tz::Fixed(2 * 3600)).unwrap();
assert_eq!((dt.year, dt.month, dt.day, dt.hour), (2008, 7, 1, 22));
assert_eq!(dt.unix(), 1214944517);
```

With the `iana` feature (on by default) you can parse in a named zone — load one
with the `timezone-data` crate and wrap it in `Tz::Iana`.

### Signatures

```rust
fn strtotime(input: &str, base_unix: i64, tz: Tz) -> Result<i64, Error>;
fn strtotime_civil(input: &str, base_unix: i64, tz: Tz) -> Result<DateTime, Error>;
```

`base_unix` is the reference for relative expressions (`tomorrow`, `+2 days`);
it is ignored for fully absolute inputs.

## Features

- **`iana`** *(default)* — full IANA timezone support (DST-aware named zones such
  as `America/New_York`). Disable default features for a build that understands
  only UTC, fixed numeric offsets, and timezone abbreviations:
  ```toml
  strtotime = { version = "0.1", default-features = false }
  ```
- **`std`** — convenience helpers using the system clock: `now_unix()`,
  `strtotime_now(input, tz)`, `system_time_from_unix(unix)`, and
  `From<DateTime> for std::time::SystemTime`.

## Supported expressions

A non-exhaustive tour (see `testdata/strtotime_tests.csv` for the full corpus):

- **Keywords**: `now`, `today`, `tomorrow`, `yesterday`, `midnight`, `noon`.
- **Relative**: `+1 day`, `-3 weeks`, `4 days`, `90 minutes ago`, `eighth day`.
- **Weekdays**: `next Monday`, `last Friday`, `this week`, `2 thursdays ago`,
  `first Monday December 2008`.
- **Dates**: `2023-05-15`, `15.05.2023`, `05/15/2023`, `2023/05/15`,
  `January 15 2023`, `Jan 15, 2023`, `April 4th`, `Oct 2001`, `2006-Jan-15`,
  `26th Nov`, `20 VI. 2005`.
- **Times & ISO 8601**: `14:30`, `2pm`, `2023-01-15T14:30:00+05:30`, `2023-W03-1`,
  `20060212T231223`.
- **Timezones**: abbreviations (`EST`, `CET`), offsets (`+0100`, `-07:00`), and
  IANA names (`Europe/Paris`, `America/New_York`) in the input.
- **Compound**: `next year + 1 month + 1 week`, `2023-05-30 -1 month`.
- **`@` timestamps**: `@1234567890`.

## License

MIT. See [LICENSE](LICENSE).

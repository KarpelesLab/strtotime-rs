# strtotime

A `#![no_std]`, allocation-free Rust library that parses PHP-style date/time
expressions into a Unix timestamp — a port of the Go library
[`strtotime`](https://github.com/KarpelesLab/strtotime), which mirrors PHP's
[`strtotime()`](https://www.php.net/manual/en/function.strtotime.php).

> **Status:** work in progress. The parser is being ported incrementally and
> validated against a corpus of PHP-captured cases in `testdata/`.

## Usage

```rust
use strtotime::{strtotime, Tz};

// Absolute date.
assert_eq!(strtotime("2000-01-01 12:00:00", 0, Tz::Utc).unwrap(), 946728000);

// Relative to a base timestamp.
let base = 946728000; // 2000-01-01 12:00:00 UTC
assert_eq!(strtotime("tomorrow", base, Tz::Utc).unwrap(), 946771200);
```

`strtotime_civil` returns a broken-down [`DateTime`] instead of a bare
timestamp.

## Features

- **`iana`** (default): full IANA timezone support (DST-aware named zones such
  as `America/New_York`) via the no_std/no-alloc
  [`timezone-data`](https://github.com/KarpelesLab/timezone-data-rs) crate.
  Disable default features for a build that only understands UTC, fixed numeric
  offsets, and timezone abbreviations.
- **`std`**: convenience helpers built on the system clock and `std::time`.

## License

MIT. See [LICENSE](LICENSE).

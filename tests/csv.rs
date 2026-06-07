//! Conformance harness against PHP's `strtotime()`.
//!
//! `testdata/strtotime_tests.csv` holds `input,base_unix,tz,expected_unix` rows
//! captured from real PHP; `testdata/strtotime_invalid.csv` holds inputs PHP
//! rejects. We require the same unix-second result / rejection.
//!
//! While the port is in progress the success harness asserts a moving floor
//! (`MIN_PASS`) rather than 100%, so regressions fail the build but unfinished
//! formats don't. Run with `--nocapture` to see the live count and failures.
//!
//! These tests require the `iana` feature (default) to resolve named zones.
#![cfg(feature = "iana")]

use strtotime::{strtotime, Tz};

/// Minimum number of success rows that must pass. Raise as formats land.
const MIN_PASS: usize = 455;

fn resolve_tz(tz: &str) -> Tz {
    if tz.is_empty() || tz.eq_ignore_ascii_case("UTC") {
        return Tz::Utc;
    }
    // Offset form like "+05:00" / "-07:00".
    if let Some(off) = parse_offset(tz) {
        return Tz::Fixed(off);
    }
    match timezone_data::load_insensitive(tz) {
        Ok(z) => Tz::Iana(z),
        Err(_) => panic!("test harness: unknown timezone {tz:?}"),
    }
}

/// Parse "+HH:MM" / "-HHMM" into seconds east of UTC.
fn parse_offset(s: &str) -> Option<i32> {
    let sign = match s.as_bytes().first()? {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let digits: String = s[1..].chars().filter(|c| *c != ':').collect();
    if digits.len() < 4 {
        return None;
    }
    let h: i32 = digits[0..2].parse().ok()?;
    let m: i32 = digits[2..4].parse().ok()?;
    Some(sign * (h * 3600 + m * 60))
}

/// Parse a single CSV record into its fields, honoring double-quoted fields with
/// embedded commas and `""` escapes.
fn parse_csv_line(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut cur = String::new();
    let mut chars = line.chars().peekable();
    let mut in_quotes = false;
    while let Some(c) = chars.next() {
        if in_quotes {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    cur.push('"');
                    chars.next();
                } else {
                    in_quotes = false;
                }
            } else {
                cur.push(c);
            }
        } else if c == '"' {
            in_quotes = true;
        } else if c == ',' {
            fields.push(core::mem::take(&mut cur));
        } else {
            cur.push(c);
        }
    }
    fields.push(cur);
    fields
}

fn read_records(path: &str) -> Vec<Vec<String>> {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if i == 0 {
            continue; // header
        }
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        out.push(parse_csv_line(line));
    }
    out
}

#[test]
fn csv_success() {
    let records = read_records("testdata/strtotime_tests.csv");
    let total = records.len();
    let mut pass = 0usize;
    let mut shown = 0usize;

    for rec in &records {
        if rec.len() < 4 {
            continue;
        }
        let input = &rec[0];
        let base: i64 = rec[1].parse().unwrap_or(0);
        let tz = resolve_tz(&rec[2]);
        let expected: i64 = rec[3].parse().expect("expected_unix");

        match strtotime(input, base, tz) {
            Ok(got) if got == expected => pass += 1,
            other => {
                if shown < 40 {
                    match other {
                        Ok(got) => eprintln!(
                            "FAIL {input:?} (base={base}, tz={:?}) = {got}, want {expected} [diff={}]",
                            rec[2],
                            got - expected
                        ),
                        Err(e) => eprintln!(
                            "ERR  {input:?} (base={base}, tz={:?}): {e} (want {expected})",
                            rec[2]
                        ),
                    }
                    shown += 1;
                }
            }
        }
    }

    eprintln!("\nstrtotime CSV success: {pass}/{total} passing");
    assert!(
        pass >= MIN_PASS,
        "regression: only {pass}/{total} pass (floor {MIN_PASS})"
    );
}

#[test]
fn csv_invalid() {
    let records = read_records("testdata/strtotime_invalid.csv");
    let total = records.len();
    let mut correct = 0usize;
    let mut shown = 0usize;

    for rec in &records {
        if rec.len() < 3 {
            continue;
        }
        let input = &rec[0];
        let base: i64 = rec[1].parse().unwrap_or(0);
        let tz = resolve_tz(&rec[2]);

        match strtotime(input, base, tz) {
            Err(_) => correct += 1,
            Ok(got) => {
                if shown < 40 {
                    eprintln!("SHOULD-ERR {input:?} (base={base}, tz={:?}) = {got}", rec[2]);
                    shown += 1;
                }
            }
        }
    }

    eprintln!("\nstrtotime CSV invalid: {correct}/{total} correctly rejected");
    // No floor yet; tightened once parsing is complete.
}

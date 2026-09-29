//! Throughput benchmark over the PHP conformance corpus (no external deps).
//!
//! `cargo bench --bench corpus` prints the aggregate ns/parse and the slowest
//! inputs. Pass `-- --top N` to list more of them.

use std::hint::black_box;
use std::time::Instant;
use strtotime::{Tz, strtotime};

fn resolve_tz(tz: &str) -> Tz {
    if tz.is_empty() || tz.eq_ignore_ascii_case("UTC") {
        return Tz::Utc;
    }
    if let Some(sign) = tz
        .strip_prefix('+')
        .map(|r| (1, r))
        .or(tz.strip_prefix('-').map(|r| (-1, r)))
    {
        let d: String = sign.1.chars().filter(|c| *c != ':').collect();
        let h: i32 = d[0..2].parse().unwrap();
        let m: i32 = d[2..4].parse().unwrap();
        return Tz::Fixed(sign.0 * (h * 3600 + m * 60));
    }
    Tz::Iana(timezone_data::load_insensitive(tz).expect("tz"))
}

fn fields(line: &str) -> Vec<String> {
    let (mut out, mut cur, mut q) = (Vec::new(), String::new(), false);
    let mut it = line.chars().peekable();
    while let Some(c) = it.next() {
        match (q, c) {
            (true, '"') if it.peek() == Some(&'"') => {
                cur.push('"');
                it.next();
            }
            (true, '"') => q = false,
            (false, '"') => q = true,
            (false, ',') => out.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    out.push(cur);
    out
}

fn load(path: &str) -> Vec<(String, i64, Tz)> {
    let text = std::fs::read_to_string(path).expect("corpus");
    text.lines()
        .skip(1)
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .map(|l| {
            let f = fields(l);
            (f[0].clone(), f[1].parse().unwrap_or(0), resolve_tz(&f[2]))
        })
        .collect()
}

fn time_one(input: &str, base: i64, tz: Tz, iters: u32) -> f64 {
    let t = Instant::now();
    for _ in 0..iters {
        let _ = black_box(strtotime(black_box(input), black_box(base), tz));
    }
    t.elapsed().as_nanos() as f64 / iters as f64
}

fn main() {
    let top: usize = std::env::args()
        .skip_while(|a| a != "--top")
        .nth(1)
        .and_then(|n| n.parse().ok())
        .unwrap_or(15);
    let mut rows = load("testdata/strtotime_tests.csv");
    rows.extend(load("testdata/strtotime_invalid.csv"));

    // Aggregate throughput: whole corpus, repeated.
    // `--quick`: a short aggregate-only run, for profilers (callgrind etc.).
    let quick = std::env::args().any(|a| a == "--quick");
    let rounds = if quick { 5 } else { 200 };
    let t = Instant::now();
    for _ in 0..rounds {
        for (input, base, tz) in &rows {
            let _ = black_box(strtotime(black_box(input), *base, *tz));
        }
    }
    let total = t.elapsed();
    let n = rows.len() * rounds;
    println!(
        "{} inputs x {rounds} rounds: {:.0} ns/parse avg ({:.2} M parses/s)",
        rows.len(),
        total.as_nanos() as f64 / n as f64,
        n as f64 / total.as_secs_f64() / 1e6
    );

    if quick {
        return;
    }

    // Per-input timings, slowest first.
    let mut per: Vec<(f64, &str)> = rows
        .iter()
        .map(|(i, b, tz)| (time_one(i, *b, *tz, 2000), i.as_str()))
        .collect();
    per.sort_by(|a, b| b.0.total_cmp(&a.0));
    let mut sorted: Vec<f64> = per.iter().map(|p| p.0).collect();
    sorted.sort_by(f64::total_cmp);
    let pct = |p: f64| sorted[((sorted.len() - 1) as f64 * p) as usize];
    println!(
        "per-input ns: p50 {:.0}  p90 {:.0}  p99 {:.0}  max {:.0}",
        pct(0.5),
        pct(0.9),
        pct(0.99),
        pct(1.0)
    );
    println!("slowest {top}:");
    for (ns, input) in per.iter().take(top) {
        println!("  {ns:>9.0} ns  {input:?}");
    }
}

//! Public API smoke tests that don't depend on the IANA database, so they run
//! under any feature combination.

use strtotime::{strtotime, strtotime_civil, Tz};

#[test]
fn absolute_utc() {
    assert_eq!(strtotime("2000-01-01 12:00:00", 0, Tz::Utc).unwrap(), 946728000);
    assert_eq!(strtotime("@1234567890", 0, Tz::Utc).unwrap(), 1234567890);
    assert_eq!(strtotime("@-5", 0, Tz::Utc).unwrap(), -5);
}

#[test]
fn relative_to_base() {
    let base = 946728000; // 2000-01-01 12:00:00 UTC
    assert_eq!(strtotime("tomorrow", base, Tz::Utc).unwrap(), 946771200);
    assert_eq!(strtotime("+1 day", base, Tz::Utc).unwrap(), base + 86400);
    assert_eq!(strtotime("-2 hours", base, Tz::Utc).unwrap(), base - 7200);
    assert_eq!(strtotime("next year + 4 days", base, Tz::Utc).unwrap(), 978696000);
}

#[test]
fn fixed_offset_zone() {
    // A wall-clock date in UTC-5 is 5h later in absolute terms than in UTC.
    let utc = strtotime("2023-01-15 00:00:00", 0, Tz::Utc).unwrap();
    let est = strtotime("2023-01-15 00:00:00", 0, Tz::Fixed(-5 * 3600)).unwrap();
    assert_eq!(est - utc, 5 * 3600);
}

#[test]
fn civil_fields() {
    let dt = strtotime_civil("2008-07-01 22:35:17", 0, Tz::Fixed(2 * 3600)).unwrap();
    assert_eq!((dt.year, dt.month, dt.day), (2008, 7, 1));
    assert_eq!((dt.hour, dt.minute, dt.second), (22, 35, 17));
    assert_eq!(dt.offset, 2 * 3600);
    // unix() must round-trip back to the parsed timestamp.
    assert_eq!(dt.unix(), strtotime("2008-07-01 22:35:17", 0, Tz::Fixed(2 * 3600)).unwrap());
}

#[test]
fn invalid_inputs_error() {
    assert!(strtotime("", 0, Tz::Utc).is_err());
    assert!(strtotime("not-a-date", 0, Tz::Utc).is_err());
    assert!(strtotime("2023-", 0, Tz::Utc).is_err());
}

#[cfg(feature = "std")]
#[test]
fn std_helpers() {
    use std::time::{SystemTime, UNIX_EPOCH};

    // now_unix() is plausibly recent.
    assert!(strtotime::now_unix() > 1_700_000_000);

    // "now" parsed against the system clock matches now_unix() within a second.
    let now = strtotime::strtotime_now("now", Tz::Utc).unwrap();
    assert!((now - strtotime::now_unix()).abs() <= 1);

    // SystemTime conversion round-trips.
    let dt = strtotime_civil("2000-01-01 12:00:00", 0, Tz::Utc).unwrap();
    let st: SystemTime = dt.into();
    assert_eq!(st.duration_since(UNIX_EPOCH).unwrap().as_secs(), 946728000);
}

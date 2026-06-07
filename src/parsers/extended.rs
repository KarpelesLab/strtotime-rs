//! Extended/long-tail formats — port of `extended_formats.go`.
//!
//! (Filled in during Phase 5; functions are declared here so the pipeline can
//! reference them in their correct order.)

use crate::tz::Moment;

macro_rules! todo_parser {
    ($name:ident) => {
        #[allow(unused_variables)]
        pub fn $name(s: &str, base: Moment) -> Option<Moment> {
            None
        }
    };
}

todo_parser!(parse_front_back_of);
todo_parser!(parse_roman_numeral_date);
todo_parser!(parse_us_date_with_time);
todo_parser!(parse_compact_timestamp);
todo_parser!(parse_compact_time_formats);
todo_parser!(parse_month_name_format);
todo_parser!(parse_http_log_format);
todo_parser!(parse_datetime_tz_relative);
todo_parser!(parse_date_with_tz);
todo_parser!(parse_day_month_year);
todo_parser!(parse_month_year_only);
todo_parser!(parse_time_before_date);
todo_parser!(parse_month_day_time_year);
todo_parser!(parse_first_last_day_of_date);
todo_parser!(parse_numbered_weekday);

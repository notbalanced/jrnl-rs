use chrono::{Datelike, Local, NaiveDate, NaiveDateTime, NaiveTime, Weekday};

/// Parse a date/time expression used in -on/-from/-to and entry date prefixes.
/// Supports:
///  - "today", "yesterday", "tomorrow"
///  - weekday names ("monday", "last friday") -> most recent past occurrence
///  - "YYYY-MM-DD", "YYYY-MM-DD HH:MM"
///  - "MM/DD/YYYY", "MM-DD-YYYY"
///  - "Month Day, Year"
///  - any of the above followed by a time-of-day: "10am", "10:30pm", "22:00"
///    e.g. "yesterday 10pm", "2024-01-15 10am", "monday 9:30am"
///  - the word "at" between a date and time is ignored, e.g.
///    "yesterday at 9:15am", "friday at 6pm", "2026-05-23 at 17:30"
///  - a bare time-of-day on its own ("10am") -> today at that time
/// Returns the start-of-day NaiveDateTime if no time component is given.
/// Parse a date/time expression, applying `default_hour`/`default_minute`
/// when the input has a date but no explicit time component.
///
/// Used for entry creation only — search filters use `parse_date` directly
/// (which defaults to midnight) so that date-range boundaries are inclusive.
pub fn parse_date_with_defaults(
    input: &str,
    default_hour: u32,
    default_minute: u32,
) -> Option<NaiveDateTime> {
    let s = input.trim().to_lowercase();
    let s = s.replace(" at ", " ");
    let s = s.trim();
    let now = Local::now().naive_local();

    let (date_part, time_part) = split_trailing_time(s);

    if date_part.is_empty() {
        // Bare time with no date → today at that time (no defaulting needed).
        return time_part.map(|t| now.date().and_time(t));
    }

    let base_date = parse_date_part(date_part, now)?;
    match time_part {
        // User supplied an explicit time — use it as-is.
        Some(t) => Some(base_date.and_time(t)),
        // No time supplied — use the configured defaults instead of midnight.
        None => NaiveTime::from_hms_opt(default_hour, default_minute, 0)
            .map(|t| base_date.and_time(t)),
    }
}

pub fn parse_date(input: &str) -> Option<NaiveDateTime> {
    let s = input.trim().to_lowercase();
    // "at" is just a connector word between a date and a time
    // (e.g. "yesterday at 9am", "2026-05-23 at 17:30") -- drop it.
    let s = s.replace(" at ", " ");
    let s = s.trim();
    let now = Local::now().naive_local();

    // Split off a trailing time-of-day token, if present.
    let (date_part, time_part) = split_trailing_time(s);

    // A bare time with no date part -> today at that time.
    if date_part.is_empty() {
        return time_part.map(|t| now.date().and_time(t));
    }

    let base_date = parse_date_part(date_part, now)?;
    match time_part {
        Some(t) => Some(base_date.and_time(t)),
        None => Some(base_date.and_time(NaiveTime::MIN)),
    }
}

/// Try to split `s` into (date_part, Some(time)) by checking if the trailing
/// whitespace-delimited token (or the whole string) parses as a time-of-day.
/// Returns (date_part, None) if no trailing time token is found.
fn split_trailing_time(s: &str) -> (&str, Option<NaiveTime>) {
    // Try the whole string as a time first (handles bare "10am").
    if let Some(t) = parse_time_of_day(s) {
        return ("", Some(t));
    }

    // Otherwise, try splitting off the last whitespace-separated token.
    if let Some(idx) = s.rfind(' ') {
        let (head, tail) = (&s[..idx], s[idx + 1..].trim());
        if let Some(t) = parse_time_of_day(tail) {
            return (head.trim(), Some(t));
        }
    }

    (s, None)
}

/// Parse a single time-of-day token: "10am", "10pm", "10:30am", "10:30pm",
/// "22:00", "9:00". Returns None if the token doesn't look like a time.
fn parse_time_of_day(s: &str) -> Option<NaiveTime> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }

    // 12-hour with am/pm, optional minutes: "10am", "10:30pm"
    if let Some(meridiem_idx) = s.find(|c: char| c == 'a' || c == 'p') {
        // ensure the suffix is exactly "am" or "pm"
        let suffix = &s[meridiem_idx..];
        if suffix == "am" || suffix == "pm" {
            let time_str = &s[..meridiem_idx];
            let (hour, minute) = if let Some((h, m)) = time_str.split_once(':') {
                (h.parse::<u32>().ok()?, m.parse::<u32>().ok()?)
            } else {
                (time_str.parse::<u32>().ok()?, 0)
            };
            if hour == 0 || hour > 12 || minute > 59 {
                return None;
            }
            let hour24 = match (hour, suffix) {
                (12, "am") => 0,
                (12, "pm") => 12,
                (h, "am") => h,
                (h, "pm") => h + 12,
                _ => unreachable!(),
            };
            return NaiveTime::from_hms_opt(hour24, minute, 0);
        }
        return None;
    }

    // 24-hour "HH:MM"
    if let Some((h, m)) = s.split_once(':') {
        let hour = h.parse::<u32>().ok()?;
        let minute = m.parse::<u32>().ok()?;
        if hour > 23 || minute > 59 {
            return None;
        }
        return NaiveTime::from_hms_opt(hour, minute, 0);
    }

    None
}

/// Parse the date-only portion of an expression (everything except a
/// trailing time-of-day, already stripped by `split_trailing_time`).
fn parse_date_part(s: &str, now: NaiveDateTime) -> Option<NaiveDate> {
    match s {
        "today" => return Some(now.date()),
        "yesterday" => return Some(now.date() - chrono::Duration::days(1)),
        "tomorrow" => return Some(now.date() + chrono::Duration::days(1)),
        _ => {}
    }

    // "last <weekday>" or bare "<weekday>" -> most recent past occurrence (not today)
    let weekday_part = s.strip_prefix("last ").unwrap_or(s);
    if let Some(wd) = parse_weekday(weekday_part) {
        return Some(most_recent_weekday(now.date(), wd));
    }

    // "YYYY-MM-DD HH:MM" (24-hour datetime given directly, no separate time token)
    if let Ok(dt) = NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M") {
        return Some(dt.date());
    }

    // "YYYY-MM-DD"
    if let Ok(d) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        return Some(d);
    }

    // "MM/DD/YYYY"
    if let Ok(d) = NaiveDate::parse_from_str(s, "%m/%d/%Y") {
        return Some(d);
    }

    // "MM-DD-YYYY"
    if let Ok(d) = NaiveDate::parse_from_str(s, "%m-%d-%Y") {
        return Some(d);
    }

    // "Month Day, Year" e.g. "January 15, 2024"
    if let Ok(d) = NaiveDate::parse_from_str(s, "%B %d, %Y") {
        return Some(d);
    }

    None
}

fn parse_weekday(s: &str) -> Option<Weekday> {
    match s {
        "monday" | "mon" => Some(Weekday::Mon),
        "tuesday" | "tue" => Some(Weekday::Tue),
        "wednesday" | "wed" => Some(Weekday::Wed),
        "thursday" | "thu" => Some(Weekday::Thu),
        "friday" | "fri" => Some(Weekday::Fri),
        "saturday" | "sat" => Some(Weekday::Sat),
        "sunday" | "sun" => Some(Weekday::Sun),
        _ => None,
    }
}

fn most_recent_weekday(from: NaiveDate, target: Weekday) -> NaiveDate {
    let mut d = from - chrono::Duration::days(1);
    loop {
        if d.weekday() == target {
            return d;
        }
        d -= chrono::Duration::days(1);
    }
}

/// Like `split_date_prefix` but uses `default_hour`/`default_minute` when
/// the date expression has no explicit time component. Used for entry creation.
pub fn split_date_prefix_with_defaults(
    text: &str,
    default_hour: u32,
    default_minute: u32,
) -> (Option<NaiveDateTime>, &str) {
    let mut best: Option<(NaiveDateTime, &str)> = None;

    for (idx, _) in text.match_indices(':') {
        let candidate = &text[..idx];
        if let Some(date) = parse_date_with_defaults(candidate, default_hour, default_minute) {
            let rest = text[idx + 1..].trim_start();
            best = Some((date, rest));
        }
    }

    match best {
        Some((date, rest)) => (Some(date), rest),
        None => (None, text),
    }
}


/// "6/6/2026 10:00: Older entry.", split off a leading date expression
/// (followed by ':') if present. Returns (Option<date>, remaining_text).
///
/// Since date expressions can themselves contain colons (e.g. "10:00"),
/// every ':' position is tried as a possible split point, and the longest
/// (rightmost) candidate that parses as a valid date wins.
#[allow(dead_code)]
pub fn split_date_prefix(text: &str) -> (Option<NaiveDateTime>, &str) {
    let mut best: Option<(NaiveDateTime, &str)> = None;

    for (idx, _) in text.match_indices(':') {
        let candidate = &text[..idx];
        if let Some(date) = parse_date(candidate) {
            let rest = text[idx + 1..].trim_start();
            best = Some((date, rest));
        }
    }

    match best {
        Some((date, rest)) => (Some(date), rest),
        None => (None, text),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_iso_date() {
        let d = parse_date("2024-01-15").unwrap();
        assert_eq!(d.date(), NaiveDate::from_ymd_opt(2024, 1, 15).unwrap());
    }

    #[test]
    fn test_iso_datetime() {
        let d = parse_date("2024-01-15 09:30").unwrap();
        assert_eq!(d.time(), NaiveTime::from_hms_opt(9, 30, 0).unwrap());
    }

    #[test]
    fn test_us_date() {
        let d = parse_date("01/15/2024").unwrap();
        assert_eq!(d.date(), NaiveDate::from_ymd_opt(2024, 1, 15).unwrap());
    }

    #[test]
    fn test_today_yesterday() {
        let today = Local::now().naive_local().date();
        assert_eq!(parse_date("today").unwrap().date(), today);
        assert_eq!(parse_date("yesterday").unwrap().date(), today - chrono::Duration::days(1));
    }

    #[test]
    fn test_split_date_prefix_with_date() {
        let (date, rest) = split_date_prefix("2024-01-15: Went for a walk.");
        assert!(date.is_some());
        assert_eq!(rest, "Went for a walk.");
    }

    #[test]
    fn test_split_date_prefix_without_date() {
        let (date, rest) = split_date_prefix("Went for a walk: it was nice.");
        assert!(date.is_none());
        assert_eq!(rest, "Went for a walk: it was nice.");
    }

    #[test]
    fn test_month_name() {
        let d = parse_date("January 15, 2024").unwrap();
        assert_eq!(d.date(), NaiveDate::from_ymd_opt(2024, 1, 15).unwrap());
    }

    #[test]
    fn test_bare_time_am() {
        let d = parse_date("10am").unwrap();
        let today = Local::now().naive_local().date();
        assert_eq!(d.date(), today);
        assert_eq!(d.time(), NaiveTime::from_hms_opt(10, 0, 0).unwrap());
    }

    #[test]
    fn test_bare_time_pm() {
        let d = parse_date("10pm").unwrap();
        assert_eq!(d.time(), NaiveTime::from_hms_opt(22, 0, 0).unwrap());
    }

    #[test]
    fn test_bare_time_with_minutes() {
        let d = parse_date("10:30pm").unwrap();
        assert_eq!(d.time(), NaiveTime::from_hms_opt(22, 30, 0).unwrap());
    }

    #[test]
    fn test_12am_is_midnight() {
        let d = parse_date("12am").unwrap();
        assert_eq!(d.time(), NaiveTime::from_hms_opt(0, 0, 0).unwrap());
    }

    #[test]
    fn test_12pm_is_noon() {
        let d = parse_date("12pm").unwrap();
        assert_eq!(d.time(), NaiveTime::from_hms_opt(12, 0, 0).unwrap());
    }

    #[test]
    fn test_yesterday_with_time() {
        let d = parse_date("yesterday 10pm").unwrap();
        let yesterday = Local::now().naive_local().date() - chrono::Duration::days(1);
        assert_eq!(d.date(), yesterday);
        assert_eq!(d.time(), NaiveTime::from_hms_opt(22, 0, 0).unwrap());
    }

    #[test]
    fn test_iso_date_with_am_pm_time() {
        let d = parse_date("2024-01-15 10am").unwrap();
        assert_eq!(d.date(), NaiveDate::from_ymd_opt(2024, 1, 15).unwrap());
        assert_eq!(d.time(), NaiveTime::from_hms_opt(10, 0, 0).unwrap());
    }

    #[test]
    fn test_iso_datetime_still_works() {
        // "YYYY-MM-DD HH:MM" should still parse correctly now that the time
        // is split off and re-attached.
        let d = parse_date("2024-01-15 09:30").unwrap();
        assert_eq!(d.date(), NaiveDate::from_ymd_opt(2024, 1, 15).unwrap());
        assert_eq!(d.time(), NaiveTime::from_hms_opt(9, 30, 0).unwrap());
    }

    #[test]
    fn test_split_date_prefix_with_time() {
        let (date, rest) = split_date_prefix("yesterday 10pm: Did stuff.");
        assert!(date.is_some());
        assert_eq!(date.unwrap().time(), NaiveTime::from_hms_opt(22, 0, 0).unwrap());
        assert_eq!(rest, "Did stuff.");
    }

    #[test]
    fn test_split_date_prefix_bare_time() {
        let (date, rest) = split_date_prefix("10am: Morning thoughts.");
        assert!(date.is_some());
        assert_eq!(date.unwrap().time(), NaiveTime::from_hms_opt(10, 0, 0).unwrap());
        assert_eq!(rest, "Morning thoughts.");
    }

    #[test]
    fn test_invalid_time_not_misparsed() {
        // "25am" isn't a valid hour, shouldn't be parsed as a time.
        assert!(parse_date("25am").is_none());
        // "13pm" is out of 12-hour range.
        assert!(parse_date("13pm").is_none());
    }

    #[test]
    fn test_split_date_prefix_us_date_with_colon_time() {
        let (date, rest) = split_date_prefix("6/6/2026 10:00: Older entry.");
        assert!(date.is_some());
        let d = date.unwrap();
        assert_eq!(d.date(), NaiveDate::from_ymd_opt(2026, 6, 6).unwrap());
        assert_eq!(d.time(), NaiveTime::from_hms_opt(10, 0, 0).unwrap());
        assert_eq!(rest, "Older entry.");
    }

    #[test]
    fn test_split_date_prefix_us_date_padded_with_colon_time() {
        let (date, rest) = split_date_prefix("06/06/2026 10:00: Older entry.");
        assert!(date.is_some());
        let d = date.unwrap();
        assert_eq!(d.date(), NaiveDate::from_ymd_opt(2026, 6, 6).unwrap());
        assert_eq!(d.time(), NaiveTime::from_hms_opt(10, 0, 0).unwrap());
        assert_eq!(rest, "Older entry.");
    }

    #[test]
    fn test_split_date_prefix_iso_with_colon_time() {
        let (date, rest) = split_date_prefix("2024-01-15 09:30: Meeting notes.");
        assert!(date.is_some());
        let d = date.unwrap();
        assert_eq!(d.time(), NaiveTime::from_hms_opt(9, 30, 0).unwrap());
        assert_eq!(rest, "Meeting notes.");
    }

    #[test]
    fn test_yesterday_at_time() {
        let d = parse_date("yesterday at 9:15am").unwrap();
        let yesterday = Local::now().naive_local().date() - chrono::Duration::days(1);
        assert_eq!(d.date(), yesterday);
        assert_eq!(d.time(), NaiveTime::from_hms_opt(9, 15, 0).unwrap());
    }

    #[test]
    fn test_weekday_at_time() {
        let d = parse_date("friday at 6pm").unwrap();
        assert_eq!(d.date().weekday(), chrono::Weekday::Fri);
        assert_eq!(d.time(), NaiveTime::from_hms_opt(18, 0, 0).unwrap());
    }

    #[test]
    fn test_us_slash_date_at_time() {
        let d = parse_date("6/2/2026 at 4:30am").unwrap();
        assert_eq!(d.date(), NaiveDate::from_ymd_opt(2026, 6, 2).unwrap());
        assert_eq!(d.time(), NaiveTime::from_hms_opt(4, 30, 0).unwrap());
    }

    #[test]
    fn test_us_dash_date_with_time() {
        let d = parse_date("06-05-2025 09:30").unwrap();
        assert_eq!(d.date(), NaiveDate::from_ymd_opt(2025, 6, 5).unwrap());
        assert_eq!(d.time(), NaiveTime::from_hms_opt(9, 30, 0).unwrap());
    }

    #[test]
    fn test_iso_date_at_24h_time() {
        let d = parse_date("2026-05-23 at 17:30").unwrap();
        assert_eq!(d.date(), NaiveDate::from_ymd_opt(2026, 5, 23).unwrap());
        assert_eq!(d.time(), NaiveTime::from_hms_opt(17, 30, 0).unwrap());
    }

    #[test]
    fn test_split_date_prefix_all_user_examples() {
        let cases: Vec<(&str, &str)> = vec![
            ("yesterday at 9:15am: Note one.", "Note one."),
            ("friday at 6pm: Note two.", "Note two."),
            ("6/2/2026 at 4:30am: Note three.", "Note three."),
            ("06-05-2025 09:30: Note four.", "Note four."),
            ("2026-05-23 at 17:30: Note five.", "Note five."),
        ];
        for (input, expected_rest) in cases {
            let (date, rest) = split_date_prefix(input);
            assert!(date.is_some(), "failed to parse date for input: {}", input);
            assert_eq!(rest, expected_rest, "wrong remainder for input: {}", input);
        }
    }

    // ---------- parse_date_with_defaults ----------

    #[test]
    fn test_defaults_applied_when_no_time_in_prefix() {
        let d = parse_date_with_defaults("2026-06-24", 9, 0).unwrap();
        assert_eq!(d.date(), NaiveDate::from_ymd_opt(2026, 6, 24).unwrap());
        assert_eq!(d.time(), NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
            "default time should be 09:00");
    }

    #[test]
    fn test_defaults_not_applied_when_explicit_time_given() {
        let d = parse_date_with_defaults("2026-06-24 11pm", 9, 0).unwrap();
        assert_eq!(d.time(), NaiveTime::from_hms_opt(23, 0, 0).unwrap(),
            "explicit time should override default");
    }

    #[test]
    fn test_defaults_applied_to_relative_date() {
        let yesterday = Local::now().naive_local().date() - chrono::Duration::days(1);
        let d = parse_date_with_defaults("yesterday", 9, 30).unwrap();
        assert_eq!(d.date(), yesterday);
        assert_eq!(d.time(), NaiveTime::from_hms_opt(9, 30, 0).unwrap());
    }

    #[test]
    fn test_defaults_applied_to_weekday() {
        // "monday" with default 14:00 should give 14:00, not midnight.
        let d = parse_date_with_defaults("monday", 14, 0).unwrap();
        assert_eq!(d.time(), NaiveTime::from_hms_opt(14, 0, 0).unwrap());
    }

    #[test]
    fn test_explicit_time_overrides_default_am_pm() {
        let d = parse_date_with_defaults("yesterday at 11pm", 9, 0).unwrap();
        assert_eq!(d.time(), NaiveTime::from_hms_opt(23, 0, 0).unwrap(),
            "explicit 11pm should not be replaced by default 9am");
    }

    #[test]
    fn test_explicit_hhmm_overrides_default() {
        let d = parse_date_with_defaults("2026-06-24 09:30", 9, 0).unwrap();
        assert_eq!(d.time(), NaiveTime::from_hms_opt(9, 30, 0).unwrap());
    }

    // ---------- split_date_prefix_with_defaults ----------

    #[test]
    fn test_split_date_prefix_with_defaults_no_time() {
        let (date, rest) = split_date_prefix_with_defaults("yesterday: I went to the store.", 9, 0);
        let d = date.unwrap();
        let yesterday = Local::now().naive_local().date() - chrono::Duration::days(1);
        assert_eq!(d.date(), yesterday);
        assert_eq!(d.time(), NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
            "should use default 09:00 when no time in prefix");
        assert_eq!(rest, "I went to the store.");
    }

    #[test]
    fn test_split_date_prefix_with_defaults_explicit_time_wins() {
        let (date, rest) = split_date_prefix_with_defaults("yesterday at 11pm: I went to the store.", 9, 0);
        let d = date.unwrap();
        assert_eq!(d.time(), NaiveTime::from_hms_opt(23, 0, 0).unwrap(),
            "explicit 11pm should override default 9am");
        assert_eq!(rest, "I went to the store.");
    }

    #[test]
    fn test_split_date_prefix_with_defaults_no_prefix_returns_none() {
        let (date, rest) = split_date_prefix_with_defaults("Just a plain entry.", 9, 0);
        assert!(date.is_none(), "no date prefix should return None");
        assert_eq!(rest, "Just a plain entry.");
    }

    #[test]
    fn test_split_date_prefix_with_defaults_iso_date_no_time() {
        let (date, rest) = split_date_prefix_with_defaults("2026-06-24: Entry text.", 9, 0);
        let d = date.unwrap();
        assert_eq!(d.date(), NaiveDate::from_ymd_opt(2026, 6, 24).unwrap());
        assert_eq!(d.time(), NaiveTime::from_hms_opt(9, 0, 0).unwrap());
        assert_eq!(rest, "Entry text.");
    }
}
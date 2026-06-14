use chrono::{Datelike, Local, NaiveDate, NaiveDateTime, NaiveTime, Weekday};

/// Parse a date/time expression used in -on/-from/-to and entry date prefixes.
/// Supports:
///  - "today", "yesterday", "tomorrow"
///  - weekday names ("monday", "last friday") -> most recent past occurrence
///  - "YYYY-MM-DD", "YYYY-MM-DD HH:MM"
///  - "MM/DD/YYYY"
/// Returns the start-of-day NaiveDateTime unless a time component is given.
pub fn parse_date(input: &str) -> Option<NaiveDateTime> {
    let s = input.trim().to_lowercase();
    let now = Local::now().naive_local();

    match s.as_str() {
        "today" => return Some(now.date().and_time(NaiveTime::MIN)),
        "yesterday" => return Some((now.date() - chrono::Duration::days(1)).and_time(NaiveTime::MIN)),
        "tomorrow" => return Some((now.date() + chrono::Duration::days(1)).and_time(NaiveTime::MIN)),
        _ => {}
    }

    // "last <weekday>" or bare "<weekday>" -> most recent past occurrence (not today)
    let weekday_part = s.strip_prefix("last ").unwrap_or(&s);
    if let Some(wd) = parse_weekday(weekday_part) {
        return Some(most_recent_weekday(now.date(), wd).and_time(NaiveTime::MIN));
    }

    // Try "YYYY-MM-DD HH:MM"
    if let Ok(dt) = NaiveDateTime::parse_from_str(&s, "%Y-%m-%d %H:%M") {
        return Some(dt);
    }

    // Try "YYYY-MM-DD"
    if let Ok(d) = NaiveDate::parse_from_str(&s, "%Y-%m-%d") {
        return Some(d.and_time(NaiveTime::MIN));
    }

    // Try "MM/DD/YYYY"
    if let Ok(d) = NaiveDate::parse_from_str(&s, "%m/%d/%Y") {
        return Some(d.and_time(NaiveTime::MIN));
    }

    // Try "Month Day, Year" e.g. "January 15, 2024"
    if let Ok(d) = NaiveDate::parse_from_str(&s, "%B %d, %Y") {
        return Some(d.and_time(NaiveTime::MIN));
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

/// Given an entry text like "yesterday: Did some stuff." or just "Did some stuff.",
/// split off a leading date expression (followed by ':') if present.
/// Returns (Option<date>, remaining_text).
pub fn split_date_prefix(text: &str) -> (Option<NaiveDateTime>, &str) {
    if let Some(idx) = text.find(':') {
        let candidate = &text[..idx];
        if let Some(date) = parse_date(candidate) {
            let rest = text[idx + 1..].trim_start();
            return (Some(date), rest);
        }
    }
    (None, text)
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
}

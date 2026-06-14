use crate::cli::FormatType;
use crate::entry::Entry;
use serde::Serialize;
use std::collections::HashMap;

#[derive(Serialize)]
struct JsonEntry<'a> {
    date: String,
    starred: bool,
    title: &'a str,
    body: &'a str,
    tags: Vec<String>,
}

/// Render a list of entries according to the given format. Default is "text".
pub fn format_entries(entries: &[&Entry], format: Option<FormatType>, short: bool) -> String {
    if short {
        return entries.iter().map(|e| e.to_short()).collect::<Vec<_>>().join("\n");
    }

    match format {
        None | Some(FormatType::Text) | Some(FormatType::Txt) | Some(FormatType::Pretty) => {
            entries.iter().map(|e| e.to_text()).collect::<Vec<_>>().join("\n")
        }
        Some(FormatType::Short) => {
            entries.iter().map(|e| e.to_short()).collect::<Vec<_>>().join("\n")
        }
        Some(FormatType::Dates) => {
            entries.iter().map(|e| e.date.format("%Y-%m-%d %H:%M").to_string()).collect::<Vec<_>>().join("\n")
        }
        Some(FormatType::Markdown) | Some(FormatType::Md) => format_markdown(entries),
        Some(FormatType::Json) => format_json(entries),
        Some(FormatType::Tags) => format_tags(entries),
    }
}

fn format_markdown(entries: &[&Entry]) -> String {
    let mut out = String::new();
    let mut current_date = String::new();
    for e in entries {
        let day = e.date.format("%Y-%m-%d").to_string();
        if day != current_date {
            if !current_date.is_empty() {
                out.push('\n');
            }
            out.push_str(&format!("## {}\n\n", day));
            current_date = day;
        }
        let star = if e.starred { "⭐ " } else { "" };
        out.push_str(&format!("### {}{}\n", star, e.title));
        if !e.body.trim().is_empty() {
            out.push_str(&format!("{}\n", e.body.trim()));
        }
        out.push('\n');
    }
    out.trim_end().to_string()
}

fn format_json(entries: &[&Entry]) -> String {
    let json_entries: Vec<JsonEntry> = entries
        .iter()
        .map(|e| JsonEntry {
            date: e.date.format("%Y-%m-%d %H:%M").to_string(),
            starred: e.starred,
            title: &e.title,
            body: &e.body,
            tags: e.tags().into_iter().collect(),
        })
        .collect();
    serde_json::to_string_pretty(&json_entries).unwrap_or_default()
}

/// Returns a list of all tags and their occurrence counts, sorted by count desc.
fn format_tags(entries: &[&Entry]) -> String {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for e in entries {
        for tag in e.tags() {
            *counts.entry(tag).or_insert(0) += 1;
        }
    }
    let mut pairs: Vec<(String, usize)> = counts.into_iter().collect();
    pairs.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    pairs
        .into_iter()
        .map(|(tag, count)| format!("{:<20} : {}", tag, count))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDateTime;

    fn entry(date: &str, title: &str, body: &str) -> Entry {
        Entry::new(
            NaiveDateTime::parse_from_str(date, "%Y-%m-%d %H:%M").unwrap(),
            false,
            title.to_string(),
            body.to_string(),
        )
    }

    #[test]
    fn test_text_format() {
        let e = entry("2024-01-15 09:30", "Hello.", "World");
        let refs = vec![&e];
        let out = format_entries(&refs, None, false);
        assert!(out.contains("[2024-01-15 09:30] Hello."));
        assert!(out.contains("World"));
    }

    #[test]
    fn test_json_format() {
        let e = entry("2024-01-15 09:30", "Hello @world.", "Body");
        let refs = vec![&e];
        let out = format_entries(&refs, Some(FormatType::Json), false);
        assert!(out.contains("\"title\""));
        assert!(out.contains("@world"));
    }

    #[test]
    fn test_tags_format() {
        let e1 = entry("2024-01-15 09:30", "Met @bob.", "");
        let e2 = entry("2024-01-16 09:30", "Met @bob and @alice.", "");
        let refs = vec![&e1, &e2];
        let out = format_entries(&refs, Some(FormatType::Tags), false);
        assert!(out.contains("@bob"));
        assert!(out.contains("@alice"));
        assert!(out.contains("2"));
    }

    #[test]
    fn test_short_flag_overrides_format() {
        let e = entry("2024-01-15 09:30", "Hello.", "World body text");
        let refs = vec![&e];
        let out = format_entries(&refs, Some(FormatType::Json), true);
        assert!(!out.contains("World"));
        assert!(out.contains("Hello."));
    }
}

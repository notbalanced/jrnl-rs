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
/// `linewrap` is the maximum line width in characters, applied only when no
/// `--format` is given and `short` is false (i.e. the default display mode);
/// wrapping breaks only at word boundaries. Explicit `--format text/txt/pretty`
/// and `--short` are always shown unwrapped.
pub fn format_entries(entries: &[&Entry], format: Option<FormatType>, short: bool, linewrap: usize) -> String {
    if short {
        return entries.iter().map(|e| e.to_short()).collect::<Vec<_>>().join("\n");
    }

    match format {
        None => entries
            .iter()
            .map(|e| wrap_text(e.to_text().trim_end(), linewrap))
            .collect::<Vec<_>>()
            .join("\n\n"),
        Some(FormatType::Text) | Some(FormatType::Txt) | Some(FormatType::Pretty) => {
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

/// Wrap `text` so that no line exceeds `width` characters, breaking only at
/// word boundaries (spaces). Existing line breaks are preserved -- each line
/// is wrapped independently, so blank lines (paragraph breaks) stay intact.
/// A single word longer than `width` is left intact rather than being split
/// mid-word. `width == 0` disables wrapping entirely.
pub fn wrap_text(text: &str, width: usize) -> String {
    if width == 0 {
        return text.to_string();
    }
    text.lines().map(|line| wrap_line(line, width)).collect::<Vec<_>>().join("\n")
}

fn wrap_line(line: &str, width: usize) -> String {
    if line.chars().count() <= width {
        return line.to_string();
    }

    let mut out = String::new();
    let mut current_width = 0;

    for word in line.split(' ') {
        let word_width = word.chars().count();
        if current_width == 0 {
            out.push_str(word);
            current_width = word_width;
        } else if current_width + 1 + word_width <= width {
            out.push(' ');
            out.push_str(word);
            current_width += 1 + word_width;
        } else {
            out.push('\n');
            out.push_str(word);
            current_width = word_width;
        }
    }
    out
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
        let out = format_entries(&refs, None, false, 0);
        assert!(out.contains("[2024-01-15 09:30] Hello."));
        assert!(out.contains("World"));
    }

    #[test]
    fn test_json_format() {
        let e = entry("2024-01-15 09:30", "Hello @world.", "Body");
        let refs = vec![&e];
        let out = format_entries(&refs, Some(FormatType::Json), false, 0);
        assert!(out.contains("\"title\""));
        assert!(out.contains("@world"));
    }

    #[test]
    fn test_tags_format() {
        let e1 = entry("2024-01-15 09:30", "Met @bob.", "");
        let e2 = entry("2024-01-16 09:30", "Met @bob and @alice.", "");
        let refs = vec![&e1, &e2];
        let out = format_entries(&refs, Some(FormatType::Tags), false, 0);
        assert!(out.contains("@bob"));
        assert!(out.contains("@alice"));
        assert!(out.contains("2"));
    }

    #[test]
    fn test_short_flag_overrides_format() {
        let e = entry("2024-01-15 09:30", "Hello.", "World body text");
        let refs = vec![&e];
        let out = format_entries(&refs, Some(FormatType::Json), true, 0);
        assert!(!out.contains("World"));
        assert!(out.contains("Hello."));
    }

    #[test]
    fn test_wrap_text_disabled_when_zero() {
        let text = "This is a fairly long line of text that would normally wrap.";
        assert_eq!(wrap_text(text, 0), text);
    }

    #[test]
    fn test_wrap_text_breaks_at_word_boundary() {
        let text = "The quick brown fox jumps over the lazy dog";
        let wrapped = wrap_text(text, 10);
        for line in wrapped.lines() {
            assert!(line.chars().count() <= 10, "line too long: {:?}", line);
        }
        // Should not have split any word.
        let rejoined: String = wrapped.split('\n').collect::<Vec<_>>().join(" ");
        assert_eq!(rejoined, text);
    }

    #[test]
    fn test_wrap_text_preserves_blank_lines_between_paragraphs() {
        let text = "Short line.\n\nAnother short line.";
        let wrapped = wrap_text(text, 40);
        assert_eq!(wrapped, text);
    }

    #[test]
    fn test_wrap_text_does_not_split_long_word() {
        let text = "supercalifragilisticexpialidocious is long";
        let wrapped = wrap_text(text, 10);
        assert!(wrapped.lines().next().unwrap().chars().count() > 10);
    }

    #[test]
    fn test_format_entries_applies_linewrap_to_text() {
        let e = entry(
            "2024-01-15 09:30",
            "A reasonably long title that should wrap.",
            "And a body with several words that also needs wrapping.",
        );
        let refs = vec![&e];
        let out = format_entries(&refs, None, false, 20);
        for line in out.lines() {
            assert!(line.chars().count() <= 20, "line too long: {:?}", line);
        }
    }

    #[test]
    fn test_format_entries_no_wrap_by_default_param() {
        let e = entry(
            "2024-01-15 09:30",
            "A reasonably long title that would wrap if linewrap were set.",
            "",
        );
        let refs = vec![&e];
        let out = format_entries(&refs, None, false, 0);
        // With linewrap=0, the whole header line stays on one line.
        assert_eq!(out.lines().next().unwrap(), "[2024-01-15 09:30] A reasonably long title that would wrap if linewrap were set.");
    }

    #[test]
    fn test_format_entries_applies_linewrap_to_short() {
        let e = entry(
            "2024-01-15 09:30",
            "A reasonably long title that should wrap when short.",
            "Body is ignored in short format.",
        );
        let refs = vec![&e];
        let out = format_entries(&refs, None, true, 20);
        // --short is never wrapped, regardless of linewrap.
        assert_eq!(out, e.to_short());
    }

    #[test]
    fn test_explicit_text_format_not_wrapped() {
        let e = entry(
            "2024-01-15 09:30",
            "A reasonably long title that should not wrap when --format text is given.",
            "Nor should this body wrap.",
        );
        let refs = vec![&e];
        let out = format_entries(&refs, Some(FormatType::Text), false, 20);
        assert_eq!(out, e.to_text());
    }

    #[test]
    fn test_explicit_short_format_not_wrapped() {
        let e = entry(
            "2024-01-15 09:30",
            "A reasonably long title that should not wrap with --format short.",
            "",
        );
        let refs = vec![&e];
        let out = format_entries(&refs, Some(FormatType::Short), false, 20);
        assert_eq!(out, e.to_short());
    }

    #[test]
    fn test_default_format_has_blank_line_between_entries() {
        let e1 = entry("2024-01-15 09:30", "First entry.", "");
        let e2 = entry("2024-01-16 09:30", "Second entry.", "");
        let refs = vec![&e1, &e2];

        // With wrapping enabled (the bug: wrap_text strips trailing
        // newlines via .lines(), which collapsed the blank line).
        let out = format_entries(&refs, None, false, 40);
        assert!(out.contains("First entry.\n\n[2024-01-16"), "expected blank line between entries, got: {:?}", out);

        // And with wrapping disabled.
        let out_nowrap = format_entries(&refs, None, false, 0);
        assert!(out_nowrap.contains("First entry.\n\n[2024-01-16"), "expected blank line between entries, got: {:?}", out_nowrap);
    }
}

use crate::cli::{FormatType, TagSort};
use crate::config::Colors;
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
pub fn format_entries(
    entries: &[&Entry],
    format: Option<FormatType>,
    short: bool,
    linewrap: usize,
    tag_symbols: &str,
    tag_sort: TagSort,
    colors: &Colors,
    highlight: Option<&str>,
) -> String {
    let use_color = matches!(format, Some(FormatType::Pretty))
        || (format.is_none() && colors.any_enabled());

    if short {
        if use_color {
            return entries
                .iter()
                .map(|e| format_entry_short(e, colors, tag_symbols, highlight))
                .collect::<Vec<_>>()
                .join("\n");
        }
        return entries.iter().map(|e| e.to_short()).collect::<Vec<_>>().join("\n");
    }

    match format {
        None => {
            if use_color {
                format_pretty_entries(entries, colors, tag_symbols, linewrap, highlight)
            } else {
                entries
                    .iter()
                    .map(|e| wrap_text(e.to_text().trim_end(), linewrap))
                    .collect::<Vec<_>>()
                    .join("\n\n")
            }
        }
        Some(FormatType::Text) | Some(FormatType::Txt) => {
            entries.iter().map(|e| e.to_text()).collect::<Vec<_>>().join("\n")
        }
        Some(FormatType::Pretty) => format_pretty_entries(entries, colors, tag_symbols, linewrap, highlight),
        Some(FormatType::Short) => entries.iter().map(|e| e.to_short()).collect::<Vec<_>>().join("\n"),
        Some(FormatType::Dates) => entries
            .iter()
            .map(|e| e.date.format("%Y-%m-%d %H:%M").to_string())
            .collect::<Vec<_>>()
            .join("\n"),
        Some(FormatType::Markdown) | Some(FormatType::Md) => format_markdown(entries),
        Some(FormatType::Json) => format_json(entries, tag_symbols),
        Some(FormatType::Tags) => format_tags(entries, tag_symbols, tag_sort),
    }
}

fn format_pretty_entries(
    entries: &[&Entry],
    colors: &Colors,
    tag_symbols: &str,
    linewrap: usize,
    highlight: Option<&str>,
) -> String {
    entries
        .iter()
        .map(|e| format_pretty_entry(e, colors, tag_symbols, linewrap, highlight))
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn format_pretty_entry(
    entry: &Entry,
    colors: &Colors,
    tag_symbols: &str,
    linewrap: usize,
    highlight: Option<&str>,
) -> String {
    let date_text = format!("[{}]", entry.date.format("%Y-%m-%d %H:%M"));
    let title_text = highlight_contains(entry.title.trim(), highlight);
    let header_text = if entry.starred {
        format!("{} *{}", date_text, title_text)
    } else {
        format!("{} {}", date_text, title_text)
    };
    let header_text = if linewrap == 0 {
        header_text
    } else {
        wrap_text(&header_text, linewrap)
    };

    let header = header_text
        .lines()
        .map(|line| {
            if let Some(after_date) = line.strip_prefix(&format!("{} ", date_text)) {
                let mut colored = String::new();
                colored.push_str(&apply_color(&date_text, &colors.date));
                colored.push(' ');
                if entry.starred && after_date.starts_with('*') {
                    colored.push('*');
                    let title_text = apply_tag_colors(&after_date[1..], tag_symbols, &colors.tags, Some(&colors.title));
                    colored.push_str(&apply_color(&title_text, &colors.title));
                } else {
                    let title_text = apply_tag_colors(after_date, tag_symbols, &colors.tags, Some(&colors.title));
                    colored.push_str(&apply_color(&title_text, &colors.title));
                }
                replace_highlight_placeholders(&colored, &colors.contains, Some(&colors.title))
            } else {
                let title_line = apply_tag_colors(line, tag_symbols, &colors.tags, Some(&colors.title));
                replace_highlight_placeholders(&apply_color(&title_line, &colors.title), &colors.contains, Some(&colors.title))
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    let mut out = header;
    if !entry.body.trim().is_empty() {
        let body_input = highlight_contains(entry.body.trim(), highlight);
        let body = if linewrap == 0 {
            body_input
        } else {
            wrap_text(&body_input, linewrap)
        };
        let body = apply_tag_colors(&body, tag_symbols, &colors.tags, Some(&colors.body));
        let body = apply_color(&body, &colors.body);
        let body = replace_highlight_placeholders(&body, &colors.contains, Some(&colors.body));
        out.push('\n');
        out.push_str(&body);
    }
    out
}

fn format_entry_short(entry: &Entry, colors: &Colors, tag_symbols: &str, highlight: Option<&str>) -> String {
    let date = apply_color(&format!("[{}]", entry.date.format("%Y-%m-%d %H:%M")), &colors.date);
    let title_input = highlight_contains(&entry.title, highlight);
    let title = apply_tag_colors(&title_input, tag_symbols, &colors.tags, Some(&colors.title));
    let title = apply_color(&title, &colors.title);
    let title = replace_highlight_placeholders(&title, &colors.contains, Some(&colors.title));
    format!("{} {}{}", date, if entry.starred { "*" } else { "" }, title)
}

fn color_code(color: &str) -> Option<&'static str> {
    match color.trim().to_lowercase().as_str() {
        "black" => Some("\x1b[30m"),
        "red" => Some("\x1b[31m"),
        "green" => Some("\x1b[32m"),
        "yellow" => Some("\x1b[33m"),
        "blue" => Some("\x1b[34m"),
        "magenta" => Some("\x1b[35m"),
        "cyan" => Some("\x1b[36m"),
        "white" => Some("\x1b[37m"),
        _ => None,
    }
}

fn apply_color(text: &str, color: &str) -> String {
    match color_code(color) {
        None => text.to_string(),
        Some(code) => format!("{}{}\x1b[0m", code, text),
    }
}

const HIGHLIGHT_START: &str = "\u{E000}";
const HIGHLIGHT_END: &str = "\u{E001}";

fn replace_highlight_placeholders(text: &str, highlight_color: &str, restore_color: Option<&str>) -> String {
    let highlight_code = color_code(highlight_color).unwrap_or("\x1b[0m");
    let restore_code = restore_color.and_then(color_code).unwrap_or("\x1b[0m");
    text.replace(HIGHLIGHT_START, highlight_code).replace(HIGHLIGHT_END, restore_code)
}

fn highlight_contains(text: &str, needle: Option<&str>) -> String {
    let needle = match needle {
        Some(n) if !n.trim().is_empty() => n,
        _ => return text.to_string(),
    };
    let needle_lower = needle.to_lowercase();
    text.split_inclusive(char::is_whitespace)
        .map(|segment| {
            let mut out = String::new();
            let lower = segment.to_lowercase();
            let mut idx = 0;
            while let Some(pos) = lower[idx..].find(&needle_lower) {
                let start = idx + pos;
                out.push_str(&segment[idx..start]);
                out.push_str(HIGHLIGHT_START);
                out.push_str(&segment[start..start + needle_lower.len()]);
                out.push_str(HIGHLIGHT_END);
                idx = start + needle_lower.len();
            }
            out.push_str(&segment[idx..]);
            out
        })
        .collect::<Vec<_>>()
        .join("")
}

fn apply_tag_colors(text: &str, tag_symbols: &str, tag_color: &str, restore_color: Option<&str>) -> String {
    if tag_color.trim().eq_ignore_ascii_case("none") {
        return text.to_string();
    }

    let tag_code = match tag_color.trim().to_lowercase().as_str() {
        "black"   => "\x1b[30m",
        "red"     => "\x1b[31m",
        "green"   => "\x1b[32m",
        "yellow"  => "\x1b[33m",
        "blue"    => "\x1b[34m",
        "magenta" => "\x1b[35m",
        "cyan"    => "\x1b[36m",
        "white"   => "\x1b[37m",
        _ => return text.to_string(),
    };
    let restore_code = restore_color.and_then(color_code).unwrap_or("\x1b[0m");
    let reset = "\x1b[0m";

    // Process the text word-by-word, preserving inter-word whitespace.
    // We walk the string keeping track of whitespace spans explicitly so
    // the restore color is correctly re-applied after every tag.
    let mut out = String::new();
    let mut idx = 0;
    let bytes = text.as_bytes();
    let len = text.len();

    while idx < len {
        // Collect leading whitespace.
        let ws_start = idx;
        while idx < len && (bytes[idx] == b' ' || bytes[idx] == b'\t' || bytes[idx] == b'\n' || bytes[idx] == b'\r') {
            idx += 1;
        }
        out.push_str(&text[ws_start..idx]);

        if idx >= len { break; }

        // Collect a word (non-whitespace run).
        let word_start = idx;
        while idx < len && bytes[idx] != b' ' && bytes[idx] != b'\t' && bytes[idx] != b'\n' && bytes[idx] != b'\r' {
            idx += 1;
        }
        let word = &text[word_start..idx];

        // Check if this word is a tag: starts with a tagsymbol and has content after it.
        let first_char_len = word.chars().next().map(|c| c.len_utf8()).unwrap_or(0);
        let first_char = word.chars().next().unwrap_or(' ');
        if tag_symbols.contains(first_char) && word.len() > first_char_len {
            // Colored tag, then restore the surrounding context color.
            out.push_str(tag_code);
            out.push_str(word);
            out.push_str(reset);
            out.push_str(restore_code);
        } else {
            out.push_str(word);
        }
    }
    out
}

/// Render a tag summary for a list of entries.
/// The header line is "N tags found" where N is the total number of tag
/// occurrences across all entries (one entry with 5 tags contributes 5 to N;
/// a tag appearing twice in the same entry's title + body counts as 1 since
/// tags are deduplicated per entry via HashSet before summing).
/// Tags are sorted by `sort`: frequency descending (ties broken alphabetically)
/// or alphabetically ascending.
pub fn format_tags_output(entries: &[&Entry], tag_symbols: &str, sort: TagSort) -> String {
    format_tags(entries, tag_symbols, sort)
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

fn format_json(entries: &[&Entry], tag_symbols: &str) -> String {
    let json_entries: Vec<JsonEntry> = entries
        .iter()
        .map(|e| JsonEntry {
            date: e.date.format("%Y-%m-%d %H:%M").to_string(),
            starred: e.starred,
            title: &e.title,
            body: &e.body,
            tags: e.tags_with_symbols(tag_symbols).into_iter().collect(),
        })
        .collect();
    serde_json::to_string_pretty(&json_entries).unwrap_or_default()
}

/// Returns a list of all tags and their occurrence counts, sorted by count desc.
fn format_tags(entries: &[&Entry], tag_symbols: &str, sort: TagSort) -> String {
    // Count how many entries each unique tag appears in (for the per-tag counts),
    // and separately track total tag occurrences across all entries (for the header).
    let mut counts: HashMap<String, usize> = HashMap::new();
    let mut total_occurrences: usize = 0;
    for e in entries {
        // Use a set per entry so a tag appearing twice in one entry counts
        // as 1 in the per-tag count, but still adds 1 to total_occurrences.
        let entry_tags = e.tags_with_symbols(tag_symbols);
        total_occurrences += entry_tags.len();
        for tag in entry_tags {
            *counts.entry(tag).or_insert(0) += 1;
        }
    }

    let mut pairs: Vec<(String, usize)> = counts.into_iter().collect();

    match sort {
        TagSort::Freq => {
            // Descending by count, ties broken alphabetically.
            pairs.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        }
        TagSort::Alpha => {
            // Ascending alphabetically, ties broken by descending count.
            pairs.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| b.1.cmp(&a.1)));
        }
    }

    // Compute column width for the tag names so counts align neatly.
    let tag_col_width = pairs
        .iter()
        .map(|(tag, _)| tag.chars().count())
        .max()
        .unwrap_or(0)
        .max(10); // minimum column width

    let header = format!(
        "{} {} found",
        total_occurrences,
        if total_occurrences == 1 { "tag" } else { "tags" }
    );

    if pairs.is_empty() {
        return format!("{}\n(no tags found)", header);
    }
    let mut out = header;
    out.push('\n');
    out.push_str(
        &pairs
            .into_iter()
            .map(|(tag, count)| format!("{:<width$} : {}", tag, count, width = tag_col_width + 1))
            .collect::<Vec<_>>()
            .join("\n"),
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::TagSort;
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
        let out = format_entries(&refs, None, false, 0, "#@", TagSort::Freq, &Colors::default(), None);
        assert!(out.contains("[2024-01-15 09:30] Hello."));
        assert!(out.contains("World"));
    }

    #[test]
    fn test_json_format() {
        let e = entry("2024-01-15 09:30", "Hello @world.", "Body");
        let refs = vec![&e];
        let out = format_entries(&refs, Some(FormatType::Json), false, 0, "#@", TagSort::Freq, &Colors::default(), None);
        assert!(out.contains("\"title\""));
        assert!(out.contains("@world"));
    }

    #[test]
    fn test_tags_format() {
        let e1 = entry("2024-01-15 09:30", "Met @bob.", "");
        let e2 = entry("2024-01-16 09:30", "Met @bob and @alice.", "");
        let refs = vec![&e1, &e2];
        let out = format_entries(&refs, Some(FormatType::Tags), false, 0, "#@", TagSort::Freq, &Colors::default(), None);
        assert!(out.starts_with("3 tags found"), "got: {:?}", out);
        assert!(out.contains("@bob"));
        assert!(out.contains("@alice"));
        assert!(out.contains("2"));
    }

    #[test]
    fn test_short_flag_overrides_format() {
        let e = entry("2024-01-15 09:30", "Hello.", "World body text");
        let refs = vec![&e];
        let out = format_entries(&refs, Some(FormatType::Json), true, 0, "#@", TagSort::Freq, &Colors::default(), None);
        assert!(!out.contains("World"));
        assert!(out.contains("Hello."));
    }

    fn strip_ansi_codes(text: &str) -> String {
        let mut out = String::new();
        let mut chars = text.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch == '\x1b' && chars.peek() == Some(&'[') {
                chars.next();
                while let Some(&next) = chars.peek() {
                    chars.next();
                    if next == 'm' {
                        break;
                    }
                }
                continue;
            }
            out.push(ch);
        }
        out
    }

    #[test]
    fn test_wrap_text_disabled_when_zero() {
        let text = "This is a fairly long line of text that would normally wrap.";
        assert_eq!(wrap_text(text, 0), text);
    }

    #[test]
    fn test_pretty_format_wraps_header_and_body() {
        let e = entry(
            "2024-01-15 09:30",
            "A title long enough to require wrapping across multiple pretty lines.",
            "And a body with several words that also needs wrapping in pretty mode.",
        );
        let refs = vec![&e];
        let out = format_entries(&refs, Some(FormatType::Pretty), false, 20, "#@", TagSort::Freq, &Colors::default(), None);
        for line in out.lines() {
            assert!(line.chars().count() <= 20, "line too long: {:?}", line);
        }
    }

    #[test]
    fn test_pretty_format_wraps_with_colors() {
        let colors = Colors {
            body: "green".to_string(),
            date: "cyan".to_string(),
            tags: "yellow".to_string(),
            title: "magenta".to_string(),
            contains: "none".to_string(),
        };
        let e = entry(
            "2024-01-15 09:30",
            "A title long enough to require wrapping across multiple pretty lines.",
            "And a body with several words that also needs wrapping in pretty mode.",
        );
        let refs = vec![&e];
        let out = format_entries(&refs, Some(FormatType::Pretty), false, 20, "#@", TagSort::Freq, &colors, None);
        for line in strip_ansi_codes(&out).lines() {
            assert!(line.chars().count() <= 20, "colored line too long: {:?}", line);
        }
    }

    #[test]
    fn test_contains_highlight_uses_contains_color() {
        let colors = Colors {
            body: "none".to_string(),
            date: "none".to_string(),
            tags: "yellow".to_string(),
            title: "none".to_string(),
            contains: "red".to_string(),
        };
        let e = entry("2024-01-15 09:30", "Shoes were found.", "The shoes are red.");
        let refs = vec![&e];
        let out = format_entries(&refs, Some(FormatType::Pretty), false, 0, "#@", TagSort::Freq, &colors, Some("shoes"));
        assert!(out.contains("\x1b[31mShoes\x1b[0m") || out.contains("\x1b[31mshoes\x1b[0m"));
    }

    #[test]
    fn test_contains_highlight_matches_tag_value() {
        let colors = Colors {
            body: "none".to_string(),
            date: "none".to_string(),
            tags: "yellow".to_string(),
            title: "none".to_string(),
            contains: "red".to_string(),
        };
        let e = entry("2024-01-15 09:30", "#run fast", "");
        let refs = vec![&e];
        let out = format_entries(&refs, Some(FormatType::Pretty), false, 0, "#@", TagSort::Freq, &colors, Some("#run"));
        assert!(out.contains("\x1b[31m#run\x1b[0m"));
    }

    #[test]
    fn test_title_color_survives_tag_segments() {
        let colors = Colors {
            body: "none".to_string(),
            date: "none".to_string(),
            tags: "yellow".to_string(),
            title: "magenta".to_string(),
            contains: "none".to_string(),
        };
        let e = entry("2024-01-15 09:30", "#run fast", "");
        let refs = vec![&e];
        let out = format_entries(&refs, Some(FormatType::Pretty), false, 0, "#@", TagSort::Freq, &colors, None);
        // Tag colored yellow, rest of title colored magenta, restore back to magenta after tag.
        assert!(out.contains("\x1b[33m#run\x1b[0m\x1b[35m"), "expected tag colored, then magenta restored: {:?}", out);
        assert!(out.contains("fast"), "should contain non-tag word: {:?}", out);
    }

    #[test]
    fn test_body_color_survives_tag_segments() {
        let colors = Colors {
            body: "green".to_string(),
            date: "none".to_string(),
            tags: "yellow".to_string(),
            title: "none".to_string(),
            contains: "none".to_string(),
        };
        let e = entry("2024-01-15 09:30", "Title", "Body with #run tag.");
        let refs = vec![&e];
        let out = format_entries(&refs, Some(FormatType::Pretty), false, 0, "#@", TagSort::Freq, &colors, None);
        // #run should be yellow; text before and after should be green.
        assert!(out.contains("\x1b[33m#run\x1b[0m\x1b[32m"), "expected tag yellow then green restore: {:?}", out);
        assert!(out.contains("\x1b[32m"), "expected body color green: {:?}", out);
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
        let out = format_entries(&refs, None, false, 20, "#@", TagSort::Freq, &Colors::default(), None);
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
        let out = format_entries(&refs, None, false, 0, "#@", TagSort::Freq, &Colors::default(), None);
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
        let out = format_entries(&refs, None, true, 20, "#@", TagSort::Freq, &Colors::default(), None);
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
        let out = format_entries(&refs, Some(FormatType::Text), false, 20, "#@", TagSort::Freq, &Colors::default(), None);
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
        let out = format_entries(&refs, Some(FormatType::Short), false, 20, "#@", TagSort::Freq, &Colors::default(), None);
        assert_eq!(out, e.to_short());
    }

    #[test]
    fn test_default_format_has_blank_line_between_entries() {
        let e1 = entry("2024-01-15 09:30", "First entry.", "");
        let e2 = entry("2024-01-16 09:30", "Second entry.", "");
        let refs = vec![&e1, &e2];

        // With wrapping enabled (the bug: wrap_text strips trailing
        // newlines via .lines(), which collapsed the blank line).
        let out = format_entries(&refs, None, false, 40, "#@", TagSort::Freq, &Colors::default(), None);
        assert!(out.contains("First entry.\n\n[2024-01-16"), "expected blank line between entries, got: {:?}", out);

        // And with wrapping disabled.
        let out_nowrap = format_entries(&refs, None, false, 0, "#@", TagSort::Freq, &Colors::default(), None);
        assert!(out_nowrap.contains("First entry.\n\n[2024-01-16"), "expected blank line between entries, got: {:?}", out_nowrap);
    }

    #[test]
    fn test_format_tags_counts_occurrences_in_header() {
        // @bob appears in both title and body of one entry — counts as 1 occurrence
        // (deduplicated per entry via HashSet), so header should say "1 tag found".
        let e = entry("2024-01-15 09:30", "Met @bob today.", "Talked to @bob about plans.");
        let refs = vec![&e];
        let out = format_tags_output(&refs, "#@", TagSort::Freq);
        assert!(out.starts_with("1 tag found"), "expected '1 tag found', got: {:?}", out);
        assert!(out.contains("@bob"));
        assert!(out.contains(": 1"));
    }

    #[test]
    fn test_format_tags_total_occurrences_header() {
        // 2 entries with #run = 2 occurrences; 1 entry with no tags = 0 occurrences.
        // Total = 2 tags found.
        let e1 = entry("2024-01-15 09:30", "First #run entry.", "");
        let e2 = entry("2024-01-16 09:30", "Second #run entry.", "");
        let e3 = entry("2024-01-17 09:30", "No tags here.", "");
        let refs = vec![&e1, &e2, &e3];
        let out = format_tags_output(&refs, "#@", TagSort::Freq);
        assert!(out.starts_with("2 tags found"), "expected '2 tags found', got: {:?}", out);
        assert!(out.contains("#run"));
        assert!(out.contains(": 2"));
    }

    #[test]
    fn test_format_tags_multi_tag_entry_sums_correctly() {
        // 3 entries: e1 has 1 tag, e2 has 2 tags, e3 has 3 tags = 6 total.
        let e1 = entry("2024-01-15 09:30", "Entry with #run.", "");
        let e2 = entry("2024-01-16 09:30", "Entry with #run and #shoes.", "");
        let e3 = entry("2024-01-17 09:30", "Entry with #run and #shoes and #food.", "");
        let refs = vec![&e1, &e2, &e3];
        let out = format_tags_output(&refs, "#@", TagSort::Freq);
        assert!(out.starts_with("6 tags found"), "expected '6 tags found', got: {:?}", out);
        let lines: Vec<&str> = out.lines().collect();
        // #run (3) should come before #shoes (2) which should come before #food (1)
        let run_pos = lines.iter().position(|l| l.contains("#run")).unwrap();
        let shoes_pos = lines.iter().position(|l| l.contains("#shoes")).unwrap();
        let food_pos = lines.iter().position(|l| l.contains("#food")).unwrap();
        assert!(run_pos < shoes_pos, "#run should appear before #shoes");
        assert!(shoes_pos < food_pos, "#shoes should appear before #food");
    }

    #[test]
    fn test_format_tags_sort_alphabetically() {
        let e1 = entry("2024-01-15 09:30", "Entry with #run.", "");
        let e2 = entry("2024-01-16 09:30", "Entry with #food.", "");
        let e3 = entry("2024-01-17 09:30", "Entry with #beer.", "");
        let refs = vec![&e1, &e2, &e3];
        let out = format_tags_output(&refs, "#@", TagSort::Alpha);
        let lines: Vec<&str> = out.lines().collect();
        let beer_pos = lines.iter().position(|l| l.contains("#beer")).unwrap();
        let food_pos = lines.iter().position(|l| l.contains("#food")).unwrap();
        let run_pos = lines.iter().position(|l| l.contains("#run")).unwrap();
        assert!(beer_pos < food_pos, "#beer should come before #food alphabetically");
        assert!(food_pos < run_pos, "#food should come before #run alphabetically");
    }

    #[test]
    fn test_format_tags_custom_symbol_hash_only() {
        let e = entry("2024-01-15 09:30", "A #run with @bob.", "");
        let refs = vec![&e];
        // With '#' only as tag symbol, @bob should not appear
        let out = format_tags_output(&refs, "#", TagSort::Freq);
        assert!(out.starts_with("1 tag found"), "got: {:?}", out);
        assert!(out.contains("#run"));
        assert!(!out.contains("@bob"));
    }

    #[test]
    fn test_format_tags_no_tags_message() {
        let e = entry("2024-01-15 09:30", "Plain entry with no tags.", "");
        let refs = vec![&e];
        let out = format_tags_output(&refs, "#@", TagSort::Freq);
        assert!(out.starts_with("0 tags found"), "got: {:?}", out);
        assert!(out.contains("no tags found"));
    }

    #[test]
    fn test_format_tags_singular_tag() {
        let e = entry("2024-01-15 09:30", "One #run.", "");
        let refs = vec![&e];
        let out = format_tags_output(&refs, "#@", TagSort::Freq);
        assert!(out.starts_with("1 tag found"), "got: {:?}", out);
    }

    // ---------- colors::any_enabled ----------

    #[test]
    fn test_colors_any_enabled_all_none() {
        assert!(!Colors::default().any_enabled());
    }

    #[test]
    fn test_colors_any_enabled_one_set() {
        let c = Colors { date: "cyan".to_string(), ..Colors::default() };
        assert!(c.any_enabled());
    }

    #[test]
    fn test_colors_any_enabled_case_insensitive_none() {
        let c = Colors {
            body: "NONE".to_string(), date: "None".to_string(),
            tags: "NoNe".to_string(), title: "none".to_string(),
            contains: "none".to_string(),
        };
        assert!(!c.any_enabled());
    }

    // ---------- default format with colors active ----------

    #[test]
    fn test_default_format_uses_colors_when_any_enabled() {
        // When at least one color is set, the default format (no --format flag)
        // should apply color codes, same as --format pretty.
        let colors = Colors { date: "cyan".to_string(), ..Colors::default() };
        let e = entry("2024-01-15 09:30", "Hello.", "");
        let refs = vec![&e];
        let plain = format_entries(&refs, None, false, 0, "#@", TagSort::Freq, &Colors::default(), None);
        let colored = format_entries(&refs, None, false, 0, "#@", TagSort::Freq, &colors, None);
        // Plain output has no ANSI; colored output should have cyan for date.
        assert!(!plain.contains("\x1b["), "plain output should have no ANSI codes");
        assert!(colored.contains("\x1b[36m"), "colored output should contain cyan ANSI code");
    }

    #[test]
    fn test_default_format_equals_pretty_when_colors_active() {
        let colors = Colors { date: "cyan".to_string(), title: "magenta".to_string(), ..Colors::default() };
        let e = entry("2024-01-15 09:30", "Hello.", "Body text.");
        let refs = vec![&e];
        let default_out = format_entries(&refs, None, false, 0, "#@", TagSort::Freq, &colors, None);
        let pretty_out = format_entries(&refs, Some(FormatType::Pretty), false, 0, "#@", TagSort::Freq, &colors, None);
        assert_eq!(default_out, pretty_out, "default format and --format pretty should be identical when colors are active");
    }

    // ---------- apply_tag_colors whitespace ----------

    #[test]
    fn test_apply_tag_colors_preserves_surrounding_spaces() {
        // "Body with #run tag." — the space before and after #run should survive.
        let result = apply_tag_colors("Body with #run tag.", "#@", "yellow", Some("green"));
        let stripped = strip_ansi_codes(&result);
        assert_eq!(stripped, "Body with #run tag.", "text content should be unchanged after stripping ANSI");
    }

    #[test]
    fn test_apply_tag_colors_multiple_tags() {
        let result = apply_tag_colors("#run and #shoes", "#@", "yellow", None);
        let stripped = strip_ansi_codes(&result);
        assert_eq!(stripped, "#run and #shoes");
        // Both tags should be colored.
        assert!(result.contains("\x1b[33m#run\x1b[0m"));
        assert!(result.contains("\x1b[33m#shoes\x1b[0m"));
    }

    #[test]
    fn test_apply_tag_colors_none_is_passthrough() {
        let text = "Hello #run world.";
        assert_eq!(apply_tag_colors(text, "#@", "none", None), text);
    }

    #[test]
    fn test_apply_tag_colors_unknown_color_is_passthrough() {
        let text = "Hello #run world.";
        assert_eq!(apply_tag_colors(text, "#@", "ultraviolet", None), text);
    }
}

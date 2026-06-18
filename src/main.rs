mod args;
mod cli;
mod config;
mod date_parser;
mod editor;
mod entry;
mod filter;
mod formatter;
mod journal;
mod storage;

use anyhow::{anyhow, Context, Result};
use chrono::Local;
use clap::Parser;
use cli::{Cli, FormatType};
use config::Config;
use entry::Entry;
use filter::Filter;
use journal::Journal;

fn main() {
    if let Err(e) = run() {
        eprintln!("Error: {:#}", e);
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let raw_args: Vec<String> = std::env::args().collect();
    let prog = raw_args.first().cloned().unwrap_or_else(|| "jrnl".to_string());
    let rest = &raw_args[1.min(raw_args.len())..];

    // Load a lightweight config first (honoring --config-file if present)
    // just to know which journal names are configured, so we can recognize
    // a journal-name argument no matter where it appears relative to flags
    // (e.g. `jrnl work --from 2026-01-01 --edit` as well as
    // `jrnl --from 2026-01-01 --edit work`).
    let config_file_arg = args::find_config_file_arg(rest);
    let detection_config = Config::load(config_file_arg.as_deref())?;
    let journal_names: std::collections::HashSet<String> =
        detection_config.journals.keys().cloned().collect();

    let (modified_args, extracted_journal) = args::extract_journal_name(rest, &journal_names);

    let mut full_args = vec![prog];
    full_args.extend(modified_args);
    let cli = Cli::parse_from(full_args);

    // Load config (with optional override path).
    let mut config = Config::load(cli.config_file.as_deref())?;

    // Apply one-off --config-override KEY VALUE.
    if let Some(kvs) = &cli.config_override {
        for pair in kvs.chunks(2) {
            if pair.len() == 2 {
                config.apply_override(&pair[0], &pair[1])?;
            }
        }
    }

    // --list: list configured journals and exit.
    if cli.list {
        return cmd_list_journals(&config, cli.format);
    }

    // Determine which journal to use: prefer the journal name extracted from
    // argv above (works regardless of flag order); otherwise fall back to
    // checking the first word of the entry text (covers the simple
    // `jrnl work ...` case if it wasn't already caught above).
    let (journal_name, text_args) = match extracted_journal {
        Some(name) => (name, cli.text.clone()),
        None => resolve_journal_name(&config, cli.text.clone()),
    };

    let journal_cfg = config.get_journal(&journal_name)?;
    let journal = Journal::from_config(journal_cfg);

    if cli.is_search_mode() {
        cmd_search(&cli, &config, &journal)
    } else if !text_args.is_empty() {
        cmd_add(&cli, &journal, &text_args.join(" "))
    } else {
        // No text and no search flags: prompt for input on stdin.
        cmd_compose(&config, &journal)
    }
}

/// If the first element of `text` matches a configured journal name
/// (e.g. `jrnl work ...`), return that journal's name and the remaining
/// text with the name stripped. Otherwise return "default" and the
/// text unchanged.
fn resolve_journal_name(config: &Config, mut text: Vec<String>) -> (String, Vec<String>) {
    if let Some(first) = text.first() {
        if config.journals.contains_key(first) {
            let name = first.clone();
            text.remove(0);
            return (name, text);
        }
    }
    ("default".to_string(), text)
}

/// --list: print configured journal names and their paths.
fn cmd_list_journals(config: &Config, format: Option<FormatType>) -> Result<()> {
    match format {
        Some(FormatType::Json) => {
            let map: std::collections::HashMap<&String, &std::path::Path> = config
                .journals
                .iter()
                .map(|(k, v)| (k, v.path.as_path()))
                .collect();
            println!("{}", serde_json::to_string_pretty(&map)?);
        }
        _ => {
            for (name, jcfg) in &config.journals {
                println!("{}: {}", name, jcfg.path.display());
            }
        }
    }
    Ok(())
}

/// Parse free-form entry text (as typed on the command line, via stdin, or
/// written into a blank editor buffer) into an Entry. Handles an optional
/// leading date/time expression (e.g. "yesterday 10pm:", "6/1/2026 10am:"),
/// an optional leading "*" for starred entries, and splits the remainder
/// into title (up to and including the first '.', '?', or '!') and body.
fn parse_free_text_entry(text: &str) -> Entry {
    let (date, rest) = date_parser::split_date_prefix(text);
    let date = date.unwrap_or_else(|| Local::now().naive_local());

    let (starred, rest) = match rest.strip_prefix('*') {
        Some(r) => (true, r),
        None => (false, rest),
    };

    let (title, body) = split_title_body(rest);
    Entry::new(date, starred, title, body)
}

/// Add a new entry. Splits an optional date prefix (e.g. "yesterday: text")
/// and a leading "*" for starred entries.
fn cmd_add(cli: &Cli, journal: &Journal, text: &str) -> Result<()> {
    let entry = if let Some(template_path) = &cli.template {
        let (date, rest) = date_parser::split_date_prefix(text);
        let date = date.unwrap_or_else(|| Local::now().naive_local());
        let (starred, rest) = match rest.strip_prefix('*') {
            Some(r) => (true, r),
            None => (false, rest),
        };
        let (title, body) = split_title_body(rest);
        apply_template(template_path, date, starred, &title, &body)?
    } else {
        parse_free_text_entry(text)
    };

    journal.add_entry(&entry)?;
    println!("Entry added to journal.");
    Ok(())
}

/// Split entry text into a title (up to and including the first '.', '?', or '!')
/// and a body (the remainder).
fn split_title_body(text: &str) -> (String, String) {
    let text = text.trim();

    if let Some((title, body)) = text.split_once('\n') {
        return (title.trim().to_string(), body.trim().to_string());
    }

    for (i, c) in text.char_indices() {
        if (c == '.' || c == '?' || c == '!') && should_split_title_at(text, i) {
            let split_at = i + c.len_utf8();
            let title = text[..split_at].trim().to_string();
            let body = text[split_at..].trim().to_string();
            return (title, body);
        }
    }
    (text.to_string(), String::new())
}

fn should_split_title_at(text: &str, index: usize) -> bool {
    let c = text[index..].chars().next().unwrap_or_default();

    let remainder = &text[index + c.len_utf8()..];
    let remainder = remainder.trim_start_matches(char::is_whitespace);
    if remainder.is_empty() {
        return false;
    }

    if c == '.' {
        let prev = text[..index].chars().next_back();
        let next = remainder.chars().next();

        if prev.map(|ch| ch.is_ascii_digit()).unwrap_or(false)
            && next.map(|ch| ch.is_ascii_digit()).unwrap_or(false)
        {
            return false;
        }

        let next_char = remainder.chars().next().unwrap();
        return next_char.is_ascii_uppercase()
            || matches!(next_char, '"' | '\'' | '(' | '[' | '{');
    }

    let next_char = remainder.chars().next().unwrap();
    matches!(c, '?' | '!') && (next_char.is_ascii_uppercase() || matches!(next_char, '"' | '\'' | '(' | '[' | '{'))
}

fn apply_template(
    path: &str,
    date: chrono::NaiveDateTime,
    starred: bool,
    title: &str,
    body: &str,
) -> Result<Entry> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| anyhow!("Failed to read template '{}': {}", path, e))?;
    let filled = content
        .replace("{{title}}", title)
        .replace("{{body}}", body)
        .replace("{{date}}", &date.format("%Y-%m-%d %H:%M").to_string());

    let (file_title, file_body) = split_title_body(&filled);
    let (final_title, final_body) = if file_title.is_empty() {
        (title.to_string(), file_body)
    } else {
        (file_title, file_body)
    };

    Ok(Entry::new(date, starred, final_title, final_body))
}

/// No text and no search flags: jrnl's "composing mode".
/// If an editor is configured (via config, $VISUAL, or $EDITOR), open it
/// with a blank temp file and use whatever the user writes as the new
/// entry. Otherwise, fall back to reading free-form text from stdin.
fn cmd_compose(config: &Config, journal: &Journal) -> Result<()> {
    if config.has_editor_configured() {
        let editor_cmd = config.resolve_editor();
        let (raw, written) = editor::edit_entries(&editor_cmd, &[])?;

        if !written.is_empty() {
            // User wrote one or more properly-headered "[date] title" entries.
            for e in &written {
                journal.add_entry(e)?;
            }
            println!(
                "{} entr{} added.",
                written.len(),
                if written.len() == 1 { "y" } else { "ies" }
            );
            return Ok(());
        }

        // No recognizable "[date] ..." header -- treat the whole file as a
        // single free-form entry, dated now.
        if raw.trim().is_empty() {
            println!("No input given, nothing saved.");
            return Ok(());
        }

        let entry = parse_free_text_entry(raw.trim());
        journal.add_entry(&entry)?;
        println!("Entry added to journal.");
        return Ok(());
    }

    // No editor configured: prompt for input on stdin (jrnl's other fallback).
    use std::io::Read;
    println!("Composing new entry. Press Ctrl-D (Linux/Mac) or Ctrl-Z then Enter (Windows) to finish.");
    let mut buf = String::new();
    std::io::stdin().read_to_string(&mut buf)?;
    if buf.trim().is_empty() {
        println!("No input given, nothing saved.");
        return Ok(());
    }
    let entry = parse_free_text_entry(buf.trim());
    journal.add_entry(&entry)?;
    println!("Entry added to journal.");
    Ok(())
}

/// Handle all search/filter/action flags.
fn cmd_search(cli: &Cli, config: &Config, journal: &Journal) -> Result<()> {
    // --last is a special case: we look up the cookie file and then only
    // load that one day's file (or the single journal file). We must handle
    // it before the full load_entries() call below so we never read all
    // 1800 day files just to show the most recently added entry.
    if cli.last {
        return cmd_last(cli, config, journal);
    }

    let entries = {
        // Extract calendar-date bounds from the CLI flags so we can tell
        // FolderStore which day files to skip entirely. This is purely a
        // loading optimisation; the Filter still applies the full datetime
        // comparison afterwards so results are identical to loading everything.
        let from_date = cli.on.as_deref()
            .map(parse_required_date)
            .transpose()?
            .or_else(|| cli.from.as_deref().map(parse_required_date).and_then(|r| r.ok()))
            .map(|dt| dt.date());

        let to_date = cli.on.as_deref()
            .map(parse_required_date)
            .transpose()?
            .or_else(|| cli.to.as_deref().map(parse_required_date).and_then(|r| r.ok()))
            .map(|dt| dt.date());

        if from_date.is_some() || to_date.is_some() {
            journal.load_entries_in_range(from_date, to_date)?
        } else {
            journal.load_entries()?
        }
    };

    let filter = build_filter(cli, config)?;
    let matched_refs = filter.apply(&entries);
    let matched: Vec<Entry> = matched_refs.iter().map(|e| (*e).clone()).collect();

    if matched.is_empty() {
        println!("No entries found.");
        // --edit/--delete with no matches just exits cleanly.
        if !cli.edit && !cli.delete {
            return Ok(());
        }
    }

    if cli.delete {
        return cmd_delete(journal, &entries, &matched);
    }

    if cli.edit {
        return cmd_edit(config, journal, &entries, &matched);
    }

    if cli.tags {
        // When --tags is used alone (no date/search filters), show tags across
        // the entire journal. When combined with filters (e.g. --from, --starred),
        // show tags only within the matched set.
        let tag_entries: Vec<&Entry> = if has_search_filters(cli) {
            matched.iter().collect()
        } else {
            entries.iter().collect()
        };
        let out = formatter::format_tags_output(&tag_entries, &config.tagsymbols, cli.sort);
        println!("{}", out);
        return Ok(());
    }

    let refs: Vec<&Entry> = matched.iter().collect();
    let out = formatter::format_entries(&refs, cli.format, cli.short, config.linewrap, &config.tagsymbols, cli.sort);

    if let Some(file_path) = &cli.file {
        std::fs::write(file_path, &out)
            .map_err(|e| anyhow!("Failed to write output file '{}': {}", file_path, e))?;
        println!("Output written to {}", file_path);
    } else {
        println!("{}", out);
    }

    Ok(())
}

/// True if the CLI has any date/content/attribute filter flags set (i.e. the
/// user explicitly limited the set of entries). Used to decide whether
/// `--tags` should show tags for the whole journal or only the matched subset.
/// Handle --last: display the most recently *added* entry without loading
/// the entire journal. Reads the cookie file for the entry's date, then
/// uses load_entries_for_date() to read only that single day file (folder
/// mode) rather than all 1800 files.
fn cmd_last(cli: &Cli, config: &Config, journal: &Journal) -> Result<()> {
    let cookie_path = journal.cookie_path();
    if !cookie_path.exists() {
        println!("No entries found (no entry has been added yet, or the cookie file is missing).");
        return Ok(());
    }

    // Parse the cookie file to get the entry's identity and date.
    let cookie_content = std::fs::read_to_string(cookie_path)
        .with_context(|| format!("Failed to read cookie file {}", cookie_path.display()))?;
    let cookie_entries = entry::parse_entries(&cookie_content);
    let cookie_entry = match cookie_entries.into_iter().last() {
        Some(e) => e,
        None => {
            println!("No entries found (cookie file is empty or malformed).");
            return Ok(());
        }
    };
    
    // Load only the day file that could contain this entry — O(1) files
    // instead of O(N) for the whole journal.
    let day_entries = journal.load_entries_for_date(cookie_entry.date)?;
    let found = day_entries.iter().find(|e| {
        e.date == cookie_entry.date
            && e.title.trim() == cookie_entry.title.trim()
            && e.body.trim() == cookie_entry.body.trim()
    });

    match found {
        None => {
            println!("Last entry no longer found (it may have been edited or deleted).");
        }
        Some(e) => {
            let out = if cli.short {
                e.to_short()
            } else {
                formatter::wrap_text(e.to_text().trim_end(), config.linewrap)
            };
            println!("{}", out);
        }
    }
    Ok(())
}

fn has_search_filters(cli: &Cli) -> bool {
    cli.on.is_some()
        || cli.from.is_some()
        || cli.to.is_some()
        || cli.contains.is_some()
        || cli.starred
        || cli.tagged
        || cli.not.is_some()
        || cli.n.is_some()
}

fn build_filter(cli: &Cli, config: &Config) -> Result<Filter> {
    let mut filter = Filter {
        and: cli.and,
        starred: cli.starred,
        tagged: cli.tagged,
        contains: cli.contains.clone(),
        not: cli.not.clone(),
        limit: cli.n,
        tag_symbols: config.tagsymbols.clone(),
        ..Default::default()
    };

    if let Some(s) = &cli.on {
        filter.on = Some(parse_required_date(s)?);
    }
    if let Some(s) = &cli.from {
        filter.from = Some(parse_required_date(s)?);
    }
    if let Some(s) = &cli.to {
        // Treat -to as inclusive of the whole day by setting time to 23:59.
        let mut d = parse_required_date(s)?;
        d = d.date().and_hms_opt(23, 59, 59).unwrap();
        filter.to = Some(d);
    }

    Ok(filter)
}

fn parse_required_date(s: &str) -> Result<chrono::NaiveDateTime> {
    date_parser::parse_date(s).ok_or_else(|| anyhow!("Could not parse date: '{}'", s))
}

/// Interactively delete matched entries (with confirmation).
fn cmd_delete(journal: &Journal, all_entries: &[Entry], matched: &[Entry]) -> Result<()> {
    use std::io::{self, Write};

    if matched.is_empty() {
        return Ok(());
    }

    let mut to_delete: Vec<bool> = vec![false; matched.len()];

    for (i, e) in matched.iter().enumerate() {
        print!("Delete entry?\n{}\n[y/N] ", e.to_text());
        io::stdout().flush()?;
        let mut answer = String::new();
        io::stdin().read_line(&mut answer)?;
        if answer.trim().eq_ignore_ascii_case("y") {
            to_delete[i] = true;
        }
    }

    let keys_to_delete: Vec<(chrono::NaiveDateTime, String, String)> = matched
        .iter()
        .zip(to_delete.iter())
        .filter(|(_, &del)| del)
        .map(|(e, _)| (e.date, e.title.clone(), e.body.clone()))
        .collect();

    if keys_to_delete.is_empty() {
        println!("No entries deleted.");
        return Ok(());
    }

    let remaining: Vec<Entry> = all_entries
        .iter()
        .filter(|e| {
            let key = (e.date, e.title.clone(), e.body.clone());
            !keys_to_delete.contains(&key)
        })
        .cloned()
        .collect();

    journal.save_all(&remaining)?;
    println!("{} entr{} deleted.", keys_to_delete.len(), if keys_to_delete.len() == 1 { "y" } else { "ies" });
    Ok(())
}

/// Open matched entries in the configured editor and reconcile changes.
fn cmd_edit(config: &Config, journal: &Journal, all_entries: &[Entry], matched: &[Entry]) -> Result<()> {
    if matched.is_empty() {
        return Ok(());
    }

    let editor_cmd = config.resolve_editor();
    let (_, edited) = editor::edit_entries(&editor_cmd, matched)?;

    let updated_all = journal.reconcile(all_entries, matched, &edited);
    journal.save_all(&updated_all)?;

    let delta = edited.len() as i64 - matched.len() as i64;
    match delta.cmp(&0) {
        std::cmp::Ordering::Less => println!("Entries updated. {} entr{} removed.", -delta, if -delta == 1 { "y" } else { "ies" }),
        std::cmp::Ordering::Greater => println!("Entries updated. {} entr{} added.", delta, if delta == 1 { "y" } else { "ies" }),
        std::cmp::Ordering::Equal => println!("Entries updated."),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config_with_work_journal() -> Config {
        let mut config = Config::default();
        config.journals.insert(
            "work".to_string(),
            config::JournalConfig {
                path: std::path::PathBuf::from("/tmp/work.txt"),
                storage: config::StorageMode::File,
            },
        );
        config
    }

    #[test]
    fn test_resolve_journal_name_named_journal() {
        let config = test_config_with_work_journal();
        let (name, rest) = resolve_journal_name(&config, vec!["work".to_string(), "Note.".to_string()]);
        assert_eq!(name, "work");
        assert_eq!(rest, vec!["Note.".to_string()]);
    }

    #[test]
    fn test_resolve_journal_name_default_when_no_match() {
        let config = test_config_with_work_journal();
        let (name, rest) = resolve_journal_name(&config, vec!["Just".to_string(), "a".to_string(), "note.".to_string()]);
        assert_eq!(name, "default");
        assert_eq!(rest, vec!["Just".to_string(), "a".to_string(), "note.".to_string()]);
    }

    #[test]
    fn test_resolve_journal_name_bare_journal_name_for_compose() {
        let config = test_config_with_work_journal();
        let (name, rest) = resolve_journal_name(&config, vec!["work".to_string()]);
        assert_eq!(name, "work");
        assert!(rest.is_empty());
    }

    #[test]
    fn test_resolve_journal_name_empty_text() {
        let config = test_config_with_work_journal();
        let (name, rest) = resolve_journal_name(&config, vec![]);
        assert_eq!(name, "default");
        assert!(rest.is_empty());
    }

    #[test]
    fn test_resolve_journal_name_date_prefix_not_mistaken_for_journal() {
        let config = test_config_with_work_journal();
        // "yesterday:" is one token (no space before the colon), shouldn't
        // match any journal name.
        let (name, rest) = resolve_journal_name(&config, vec!["yesterday:".to_string(), "Did stuff.".to_string()]);
        assert_eq!(name, "default");
        assert_eq!(rest, vec!["yesterday:".to_string(), "Did stuff.".to_string()]);
    }

    #[test]
    fn test_parse_free_text_entry_with_us_date_and_am_pm() {
        let entry = parse_free_text_entry("6/1/2026 10am: Test entry.\nOlder entry.");
        assert_eq!(entry.date.date(), chrono::NaiveDate::from_ymd_opt(2026, 6, 1).unwrap());
        assert_eq!(entry.date.time(), chrono::NaiveTime::from_hms_opt(10, 0, 0).unwrap());
        assert_eq!(entry.title, "Test entry.");
        assert_eq!(entry.body, "Older entry.");
    }

    #[test]
    fn test_parse_free_text_entry_no_date_prefix() {
        let entry = parse_free_text_entry("Just a note. With a body.");
        assert_eq!(entry.title, "Just a note.");
        assert_eq!(entry.body, "With a body.");
    }

    #[test]
    fn test_parse_free_text_entry_starred() {
        let entry = parse_free_text_entry("yesterday: *Big news! Got it.");
        assert!(entry.starred);
        assert_eq!(entry.title, "Big news!");
    }

    #[test]
    fn test_split_title_body_basic() {
        let (title, body) = split_title_body("Went for a walk. It was nice.");
        assert_eq!(title, "Went for a walk.");
        assert_eq!(body, "It was nice.");
    }

    #[test]
    fn test_split_title_body_no_punctuation() {
        let (title, body) = split_title_body("Just a quick note");
        assert_eq!(title, "Just a quick note");
        assert_eq!(body, "");
    }

    #[test]
    fn test_split_title_body_question() {
        let (title, body) = split_title_body("Did it work? Yes it did.");
        assert_eq!(title, "Did it work?");
        assert_eq!(body, "Yes it did.");
    }

    #[test]
    fn test_split_title_body_keeps_numeric_title_text_intact() {
        let (title, body) = split_title_body(
            "Here's another test - 1.2 plus 2.3 mi. equals 5.1.1 time 3. Here's where the body starts.",
        );
        assert_eq!(
            title,
            "Here's another test - 1.2 plus 2.3 mi. equals 5.1.1 time 3."
        );
        assert_eq!(body, "Here's where the body starts.");
    }

    #[test]
    fn test_split_title_body_uses_first_line_as_title_for_multiline_input() {
        let (title, body) = split_title_body(
            "Here's another test - 1.2 plus 2.3 mi. equals 5.1.1 time 3.\nHere's where the body starts.",
        );
        assert_eq!(
            title,
            "Here's another test - 1.2 plus 2.3 mi. equals 5.1.1 time 3."
        );
        assert_eq!(body, "Here's where the body starts.");
    }

    #[test]
    fn test_split_title_body_ignores_decimal_points() {
        let (title, body) = split_title_body("Run Walk with the Boys - 1.07 mi. / 00:20:18 18:58 pace.");
        assert_eq!(title, "Run Walk with the Boys - 1.07 mi. / 00:20:18 18:58 pace.");
        assert_eq!(body, "");
    }
}

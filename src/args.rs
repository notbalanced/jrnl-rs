use std::collections::HashSet;

/// Long-form flags that take exactly one value, either as a separate
/// argument ("--from 2026-01-01") or via "--flag=value".
const ONE_VALUE_LONG_FLAGS: &[&str] = &[
    "--template",
    "--on",
    "--from",
    "--to",
    "--until",
    "--contains",
    "--not",
    "--limit",
    "--format",
    "--file",
    "--config-file",
];

/// Short flags that take one value, either as a separate argument
/// ("-n 5") or attached ("-n5").
const ONE_VALUE_SHORT_FLAGS: &[&str] = &["-n"];

/// Long-form flags that take exactly two values as separate arguments.
const TWO_VALUE_LONG_FLAGS: &[&str] = &["--config-override"];

/// Boolean (no-value) flags, long and short forms.
const BOOL_FLAGS: &[&str] = &[
    "--debug",
    "--list",
    "--and",
    "--starred",
    "--tagged",
    "--edit",
    "--delete",
    "--tags",
    "--last",
    "--short",
    "--help",
    "--version",
    "-h",
    "-V",
];

/// Scan `args` (the program's arguments, not including argv[0]) for the
/// first "bare" positional token -- i.e. a token that isn't a recognized
/// flag and isn't consumed as a flag's value. If that token matches a name
/// in `journal_names`, remove it from the argument list and return it
/// separately, so the caller can pass the remaining arguments to clap
/// without that token confusing positional parsing.
///
/// This allows `jrnl work --from 2026-01-01 --edit` to work the same as
/// `jrnl --from 2026-01-01 --edit work`, by extracting "work" up front
/// regardless of where it appears relative to other flags.
///
/// If no bare positional token is found before a non-journal-name token
/// (which marks the start of free-form entry text), or if it doesn't match
/// a configured journal name, `args` is returned unchanged with `None`.
pub fn extract_journal_name(
    args: &[String],
    journal_names: &HashSet<String>,
) -> (Vec<String>, Option<String>) {
    let mut i = 0;

    while i < args.len() {
        let arg = &args[i];

        if arg == "--" {
            // No special meaning for us; stop scanning.
            break;
        }

        // Self-contained "--flag=value" form.
        if arg.starts_with("--") && arg.contains('=') {
            i += 1;
            continue;
        }

        if BOOL_FLAGS.contains(&arg.as_str()) {
            i += 1;
            continue;
        }

        if ONE_VALUE_LONG_FLAGS.contains(&arg.as_str()) {
            i += 2; // flag + its value
            continue;
        }

        if TWO_VALUE_LONG_FLAGS.contains(&arg.as_str()) {
            i += 3; // flag + two values
            continue;
        }

        // "-n5" (attached value) vs "-n" (separate value) vs unrelated "-x...".
        if let Some(matched) = ONE_VALUE_SHORT_FLAGS
            .iter()
            .find(|f| arg.starts_with(*f))
        {
            if arg.len() > matched.len() {
                // "-n5" -- value attached, this token is self-contained.
                i += 1;
            } else {
                // "-n" -- value is the next token.
                i += 2;
            }
            continue;
        }

        if arg.starts_with('-') {
            // Some other flag we don't have in our table (e.g. an
            // unrecognized option). Skip just this token; clap will report
            // an error for it later if it's truly invalid.
            i += 1;
            continue;
        }

        // First bare positional token: this is either the journal name or
        // the start of free-form entry text.
        if journal_names.contains(arg) {
            let mut new_args = args.to_vec();
            let name = new_args.remove(i);
            return (new_args, Some(name));
        }
        break;
    }

    (args.to_vec(), None)
}

/// Best-effort scan for a `--config-file <path>` or `--config-file=<path>`
/// argument, used to load the config early (before full clap parsing) so
/// `extract_journal_name` knows which journal names are configured.
pub fn find_config_file_arg(args: &[String]) -> Option<String> {
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if let Some(v) = arg.strip_prefix("--config-file=") {
            return Some(v.to_string());
        }
        if arg == "--config-file" {
            return args.get(i + 1).cloned();
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> HashSet<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn v(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn test_journal_name_first_with_flags_after() {
        let journals = names(&["default", "work"]);
        let args = v(&["work", "--from", "2026-01-01", "--edit"]);
        let (rest, name) = extract_journal_name(&args, &journals);
        assert_eq!(name, Some("work".to_string()));
        assert_eq!(rest, v(&["--from", "2026-01-01", "--edit"]));
    }

    #[test]
    fn test_journal_name_last_after_flags() {
        let journals = names(&["default", "work"]);
        let args = v(&["--from", "2026-01-01", "--edit", "work"]);
        let (rest, name) = extract_journal_name(&args, &journals);
        assert_eq!(name, Some("work".to_string()));
        assert_eq!(rest, v(&["--from", "2026-01-01", "--edit"]));
    }

    #[test]
    fn test_journal_name_with_entry_text() {
        let journals = names(&["default", "work"]);
        let args = v(&["work", "Some", "text."]);
        let (rest, name) = extract_journal_name(&args, &journals);
        assert_eq!(name, Some("work".to_string()));
        assert_eq!(rest, v(&["Some", "text."]));
    }

    #[test]
    fn test_no_journal_name_plain_text() {
        let journals = names(&["default", "work"]);
        let args = v(&["Some", "text."]);
        let (rest, name) = extract_journal_name(&args, &journals);
        assert_eq!(name, None);
        assert_eq!(rest, args);
    }

    #[test]
    fn test_journal_name_word_used_as_flag_value_is_preserved() {
        let journals = names(&["default", "work"]);
        // "work" here is the *value* of --contains, not a journal selector.
        let args = v(&["--contains", "work", "--starred"]);
        let (rest, name) = extract_journal_name(&args, &journals);
        assert_eq!(name, None);
        assert_eq!(rest, args);
    }

    #[test]
    fn test_config_override_two_values_skipped() {
        let journals = names(&["default", "work"]);
        let args = v(&[
            "--config-override",
            "journals.default.path",
            "/tmp/x.txt",
            "work",
            "Entry text.",
        ]);
        let (rest, name) = extract_journal_name(&args, &journals);
        assert_eq!(name, Some("work".to_string()));
        assert_eq!(
            rest,
            v(&[
                "--config-override",
                "journals.default.path",
                "/tmp/x.txt",
                "Entry text.",
            ])
        );
    }

    #[test]
    fn test_attached_short_flag_value() {
        let journals = names(&["default", "work"]);
        let args = v(&["-n5", "work", "--starred"]);
        let (rest, name) = extract_journal_name(&args, &journals);
        assert_eq!(name, Some("work".to_string()));
        assert_eq!(rest, v(&["-n5", "--starred"]));
    }

    #[test]
    fn test_separate_short_flag_value() {
        let journals = names(&["default", "work"]);
        let args = v(&["-n", "5", "work", "--starred"]);
        let (rest, name) = extract_journal_name(&args, &journals);
        assert_eq!(name, Some("work".to_string()));
        assert_eq!(rest, v(&["-n", "5", "--starred"]));
    }

    #[test]
    fn test_equals_form_flag_skipped() {
        let journals = names(&["default", "work"]);
        let args = v(&["--format=json", "work", "--starred"]);
        let (rest, name) = extract_journal_name(&args, &journals);
        assert_eq!(name, Some("work".to_string()));
        assert_eq!(rest, v(&["--format=json", "--starred"]));
    }

    #[test]
    fn test_no_match_returns_unchanged() {
        let journals = names(&["default", "work"]);
        let args = v(&["--from", "2026-01-01", "--edit"]);
        let (rest, name) = extract_journal_name(&args, &journals);
        assert_eq!(name, None);
        assert_eq!(rest, args);
    }

    #[test]
    fn test_find_config_file_arg_separate() {
        let args = v(&["--config-file", "/tmp/c.yaml", "work"]);
        assert_eq!(find_config_file_arg(&args), Some("/tmp/c.yaml".to_string()));
    }

    #[test]
    fn test_find_config_file_arg_equals() {
        let args = v(&["--config-file=/tmp/c.yaml", "work"]);
        assert_eq!(find_config_file_arg(&args), Some("/tmp/c.yaml".to_string()));
    }

    #[test]
    fn test_find_config_file_arg_absent() {
        let args = v(&["work", "--starred"]);
        assert_eq!(find_config_file_arg(&args), None);
    }
}

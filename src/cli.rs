use clap::{Parser, ValueEnum};

#[derive(Parser, Debug)]
#[command(
    name = "jrnl-rs",
    version,
    about = "Collect your thoughts and notes without leaving the command line",
    disable_help_subcommand = true
)]
pub struct Cli {
    /// Print information useful for troubleshooting
    #[arg(long, global = true)]
    pub debug: bool,

    /// List all configured journals
    #[arg(long)]
    pub list: bool,

    /// Create a default config file at the expected location
    #[arg(long)]
    pub init: bool,

    // ---- Writing ----
    /// Path to template file
    #[arg(long)]
    pub template: Option<String>,

    /// The entry text to add (date prefix optional, e.g. "yesterday: text")
    #[arg(allow_hyphen_values = false, num_args = 0.., trailing_var_arg = true)]
    pub text: Vec<String>,

    // ---- Searching ----
    /// Show entries on this date
    #[arg(long = "on")]
    pub on: Option<String>,

    /// Show entries after, or on, this date
    #[arg(long = "from")]
    pub from: Option<String>,

    /// Show entries before, or on, this date (alias: -until)
    #[arg(long = "to", visible_alias = "until")]
    pub to: Option<String>,

    /// Show entries containing specific text
    #[arg(long = "contains")]
    pub contains: Option<String>,

    /// Show only entries that match all conditions (default: OR)
    #[arg(long = "and")]
    pub and: bool,

    /// Show only starred entries
    #[arg(long = "starred")]
    pub starred: bool,

    /// Show only entries that have at least one tag
    #[arg(long = "tagged")]
    pub tagged: bool,

    /// Show a maximum of NUMBER entries
    #[arg(short = 'n', long = "limit")]
    pub n: Option<usize>,

    /// Exclude entries with this tag (or 'starred'/'tagged')
    #[arg(long = "not")]
    pub not: Option<String>,

    // ---- Searching Options / Actions ----
    /// Opens the selected entries in your configured editor
    #[arg(long = "edit")]
    pub edit: bool,

    /// Interactively deletes selected entries
    #[arg(long = "delete")]
    pub delete: bool,

    /// Display selected entries in an alternate format
    #[arg(long = "format")]
    pub format: Option<FormatType>,

    /// Write formatted output to file instead of stdout
    #[arg(long = "file")]
    pub file: Option<String>,

    /// Alias for '--format tags'
    #[arg(long = "tags")]
    pub tags: bool,

    /// Sort order for --tags output: "freq" (by frequency, default) or "alpha" (alphabetical)
    #[arg(long = "sort", default_value = "freq")]
    pub sort: TagSort,

    /// Show the entry that was most recently added to the journal
    #[arg(long = "last")]
    pub last: bool,

    /// Show only titles or line containing the search tags
    #[arg(long = "short")]
    pub short: bool,

    // ---- Config ----
    /// Override configured key-value pair for this invocation only
    #[arg(long = "config-override", num_args = 2, value_names = ["KEY", "VALUE"], action = clap::ArgAction::Append)]
    pub config_override: Option<Vec<String>>,

    /// Override default config file for this command only
    #[arg(long = "config-file")]
    pub config_file: Option<String>,
}

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
#[value(rename_all = "lowercase")]
pub enum FormatType {
    Text,
    Txt,
    Short,
    Json,
    Markdown,
    Md,
    Tags,
    Dates,
    Pretty,
}

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq, Default)]
#[value(rename_all = "lowercase")]
pub enum TagSort {
    /// Sort by frequency of use, most common first (default)
    #[default]
    Freq,
    /// Sort alphabetically
    Alpha,
}

impl Cli {
    /// True if this invocation is a "search/action" command rather than a plain write.
    pub fn is_search_mode(&self) -> bool {
        self.on.is_some()
            || self.from.is_some()
            || self.to.is_some()
            || self.contains.is_some()
            || self.starred
            || self.tagged
            || self.not.is_some()
            || self.n.is_some()
            || self.edit
            || self.delete
            || self.format.is_some()
            || self.tags
            || self.last
            || self.short
    }
}

# jrnl-rs
## Command line Options
```
Collect your thoughts and notes without leaving the command line

Usage: jrnl [OPTIONS] [TEXT]...

Arguments:
  [TEXT]...  The entry text to add (date prefix optional, e.g. "yesterday: text")

Options:
      --debug                          Print information useful for troubleshooting
      --list                           List all configured journals
      --template <TEMPLATE>            Path to template file
      --on <ON>                        Show entries on this date
      --from <FROM>                    Show entries after, or on, this date
      --to <TO>                        Show entries before, or on, this date (alias: -until) [aliases: until]
      --contains <CONTAINS>            Show entries containing specific text
      --and                            Show only entries that match all conditions (default: OR)
      --starred                        Show only starred entries
      --tagged                         Show only entries that have at least one tag
  -n, --limit <N>                      Show a maximum of NUMBER entries
      --not <NOT>                      Exclude entries with this tag (or 'starred'/'tagged')
      --edit                           Opens the selected entries in your configured editor
      --delete                         Interactively deletes selected entries
      --format <FORMAT>                Display selected entries in an alternate format [possible values: text, txt, short, json, markdown, md, tags, dates, pretty]
      --file <FILE>                    Write formatted output to file instead of stdout
      --tags                           Alias for '--format tags'
      --short                          Show only titles or line containing the search tags
      --config-override <KEY> <VALUE>  Override configured key-value pair for this invocation only
      --config-file <CONFIG_FILE>      Override default config file for this command only
  -h, --help                           Print help
```

**Build native release**

`cargo build --release`

**Build windows release**

`cargo build --release --target x86_64-pc-windows-gnu`

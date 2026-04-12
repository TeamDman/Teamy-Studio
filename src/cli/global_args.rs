//! Global arguments that apply to all commands.

use arbitrary::Arbitrary;
use facet::Facet;
use figue::{self as args};

use crate::cli::output::OutputFormat;

/// Global arguments that apply to all commands.
/// tool[impl cli.global.debug]
/// tool[impl cli.global.log-filter]
/// tool[impl cli.global.log-file]
/// tool[impl cli.global.output-format]
#[derive(Facet, Arbitrary, Debug, Default, PartialEq)]
#[facet(rename_all = "kebab-case")]
pub struct GlobalArgs {
    /// tool[impl cli.global.debug]
    /// Enable debug logging, including backtraces on panics.
    #[facet(args::named, default)]
    pub debug: bool,

    /// tool[impl cli.global.log-filter]
    /// Log level filter directive.
    #[facet(args::named)]
    pub log_filter: Option<String>,

    /// tool[impl cli.global.log-file]
    /// Write structured ndjson logs.
    ///
    /// If a file path is provided, logs are written to that file.
    /// If a directory path is provided, a filename like `log_<timestamp>.ndjson`
    /// is generated in that directory.
    /// If omitted, no JSON log file is written.
    #[facet(args::named)]
    pub log_file: Option<String>,

    /// tool[impl cli.global.output-format]
    /// Render command output as `text`, `json`, or `csv`.
    ///
    /// If omitted, Teamy Studio uses `text` for interactive terminals and `json`
    /// when stdout is redirected.
    #[facet(args::named)]
    pub output_format: Option<OutputFormat>,
}

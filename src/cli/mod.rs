pub mod facet_shape;
pub mod global_args;
pub mod self_test;
pub mod terminal;

use crate::cli::global_args::GlobalArgs;
use crate::cli::self_test::SelfTestArgs;
use crate::cli::terminal::TerminalArgs;
use arbitrary::Arbitrary;
use facet::Facet;
use figue::FigueBuiltins;
use figue::{self as args};

/// Teamy Studio opens a terminal window by default and exposes terminal and self-test commands.
/// tool[impl cli.help.describes-behavior]
/// tool[impl cli.help.describes-terminal]
/// tool[impl cli.help.describes-self-test]
/// tool[impl cli.help.describes-environment]
/// tool[impl cli.help.describes-argv]
// cli[impl parser.args-consistent]
// cli[impl parser.roundtrip]
///
/// Environment variables:
/// - `TEAMY_STUDIO_HOME_DIR` overrides the resolved application home directory.
/// - `TEAMY_STUDIO_CACHE_DIR` overrides the resolved cache directory.
/// - `RUST_LOG` provides a tracing filter when `--log-filter` is omitted.
#[derive(Facet, Arbitrary, Debug)]
pub struct Cli {
    /// Global arguments (`debug`, `log_filter`, `log_file`).
    #[facet(flatten)]
    pub global_args: GlobalArgs,

    /// Standard CLI options (help, version, completions).
    #[facet(flatten)]
    #[arbitrary(default)]
    pub builtins: FigueBuiltins,

    /// The command to run.
    #[facet(args::subcommand)]
    pub command: Option<Command>,
}

impl PartialEq for Cli {
    fn eq(&self, other: &Self) -> bool {
        // Ignore builtins in comparison since FigueBuiltins doesn't implement PartialEq
        self.global_args == other.global_args && self.command == other.command
    }
}

impl Cli {
    /// # Errors
    ///
    /// This function will return an error if the command fails.
    pub fn invoke(self) -> eyre::Result<()> {
        let app_home = crate::paths::APP_HOME.clone();
        let cache_home = crate::paths::CACHE_DIR.clone();
        match self.command {
            Some(command) => command.invoke(&app_home, &cache_home),
            None => crate::app::run(&app_home),
        }
    }
}

/// Teamy Studio commands.
/// tool[impl cli.surface.terminal]
/// tool[impl cli.surface.self-test]
#[derive(Facet, Arbitrary, Debug, PartialEq)]
#[repr(u8)]
pub enum Command {
    // cli[impl command.surface.terminal]
    /// Open and enumerate terminal windows.
    Terminal(TerminalArgs),
    // cli[impl command.surface.self-test]
    /// Run reproducible self-tests.
    SelfTest(SelfTestArgs),
}

impl Command {
    // cli[impl command.surface.core]
    /// # Errors
    ///
    /// This function will return an error if the subcommand fails.
    pub fn invoke(
        self,
        app_home: &crate::paths::AppHome,
        cache_home: &crate::paths::CacheHome,
    ) -> eyre::Result<()> {
        match self {
            Command::Terminal(args) => args.invoke(app_home, cache_home),
            Command::SelfTest(args) => args.invoke(app_home, cache_home),
        }
    }
}

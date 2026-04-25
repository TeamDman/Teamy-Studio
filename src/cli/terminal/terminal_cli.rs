use crate::cli::terminal::default_shell::TerminalDefaultShellArgs;
use crate::cli::terminal::list::TerminalListArgs;
use crate::cli::terminal::open::TerminalOpenArgs;
use arbitrary::Arbitrary;
use facet::Facet;
use figue as args;

use crate::cli::output::CliOutput;

/// Terminal commands.
// cli[impl command.surface.terminal]
/// tool[impl cli.surface.terminal]
/// tool[impl cli.help.describes-terminal]
/// tool[impl cli.help.describes-shell]
#[derive(Facet, Arbitrary, Debug, PartialEq)]
pub struct TerminalArgs {
    /// The terminal subcommand to run.
    #[facet(args::subcommand)]
    pub command: TerminalCommand,
}

/// Terminal-definition subcommands.
#[derive(Facet, Arbitrary, Debug, PartialEq)]
#[repr(u8)]
pub enum TerminalCommand {
    // cli[impl command.surface.terminal-default-shell]
    /// Show or change the configured default shell.
    DefaultShell(TerminalDefaultShellArgs),
    // cli[impl command.surface.terminal-list]
    /// Enumerate live Teamy Studio terminal windows.
    List(TerminalListArgs),
    // cli[impl command.surface.terminal-open]
    /// Open a new terminal window.
    Open(TerminalOpenArgs),
}

impl TerminalArgs {
    /// # Errors
    ///
    /// This function will return an error if the terminal action fails.
    pub fn invoke(
        self,
        app_home: &crate::paths::AppHome,
        cache_home: &crate::paths::CacheHome,
    ) -> eyre::Result<CliOutput> {
        match self.command {
            TerminalCommand::DefaultShell(args) => args.invoke(app_home, cache_home),
            TerminalCommand::List(args) => args.invoke(app_home, cache_home),
            TerminalCommand::Open(args) => args.invoke(app_home, cache_home),
        }
    }
}

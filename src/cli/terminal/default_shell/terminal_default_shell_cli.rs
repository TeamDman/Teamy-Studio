use crate::cli::terminal::default_shell::set::TerminalDefaultShellSetArgs;
use crate::cli::terminal::default_shell::show::TerminalDefaultShellShowArgs;
use arbitrary::Arbitrary;
use eyre::Result;
use facet::Facet;
use figue as args;

/// Default-shell commands.
// cli[impl command.surface.terminal-default-shell]
#[derive(Facet, Arbitrary, Debug, PartialEq)]
pub struct TerminalDefaultShellArgs {
    /// The default-shell subcommand to run.
    #[facet(args::subcommand)]
    pub command: TerminalDefaultShellCommand,
}

/// Default-shell subcommands.
#[derive(Facet, Arbitrary, Debug, PartialEq)]
#[repr(u8)]
pub enum TerminalDefaultShellCommand {
    // cli[impl command.surface.terminal-default-shell-set]
    /// Persist the default shell command.
    Set(TerminalDefaultShellSetArgs),
    // cli[impl command.surface.terminal-default-shell-show]
    /// Show the effective default shell command.
    Show(TerminalDefaultShellShowArgs),
}

impl TerminalDefaultShellArgs {
    /// # Errors
    ///
    /// This function will return an error if the subcommand fails.
    pub fn invoke(
        self,
        app_home: &crate::paths::AppHome,
        cache_home: &crate::paths::CacheHome,
    ) -> Result<()> {
        match self.command {
            TerminalDefaultShellCommand::Set(args) => args.invoke(app_home, cache_home)?,
            TerminalDefaultShellCommand::Show(_) => {
                TerminalDefaultShellShowArgs::invoke(app_home, cache_home)?;
            }
        }

        Ok(())
    }
}

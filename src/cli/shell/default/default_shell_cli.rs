use crate::cli::shell::default::set::ShellDefaultSetArgs;
use crate::cli::shell::default::show::ShellDefaultShowArgs;
use arbitrary::Arbitrary;
use eyre::Result;
use facet::Facet;
use figue as args;

/// Default shell commands.
/// cli[impl command.surface.shell-default]
#[derive(Facet, Arbitrary, Debug, PartialEq)]
pub struct ShellDefaultArgs {
    /// The default-shell subcommand to run.
    #[facet(args::subcommand)]
    pub command: ShellDefaultCommand,
}

/// Default shell subcommands.
#[derive(Facet, Arbitrary, Debug, PartialEq)]
#[repr(u8)]
pub enum ShellDefaultCommand {
    /// cli[impl command.surface.shell-default-set]
    /// Persist the default shell command.
    Set(ShellDefaultSetArgs),
    /// cli[impl command.surface.shell-default-show]
    /// Show the effective default shell command.
    Show(ShellDefaultShowArgs),
}

impl ShellDefaultArgs {
    /// # Errors
    ///
    /// This function will return an error if the subcommand fails.
    pub async fn invoke(
        self,
        app_home: &crate::paths::AppHome,
        cache_home: &crate::paths::CacheHome,
    ) -> Result<()> {
        match self.command {
            ShellDefaultCommand::Set(args) => args.invoke(app_home, cache_home).await?,
            ShellDefaultCommand::Show(args) => args.invoke(app_home, cache_home).await?,
        }

        Ok(())
    }
}

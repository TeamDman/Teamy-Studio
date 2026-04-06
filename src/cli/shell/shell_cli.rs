use crate::cli::shell::default::ShellDefaultArgs;
use arbitrary::Arbitrary;
use eyre::Result;
use facet::Facet;
use figue as args;

/// Shell-related commands.
/// cli[impl command.surface.shell]
/// tool[impl cli.surface.shell]
/// tool[impl cli.help.describes-shell]
#[derive(Facet, Arbitrary, Debug, PartialEq)]
pub struct ShellArgs {
    /// The shell subcommand to run.
    #[facet(args::subcommand)]
    pub command: Option<ShellCommand>,
}

/// Shell subcommands.
#[derive(Facet, Arbitrary, Debug, PartialEq)]
#[repr(u8)]
pub enum ShellCommand {
    /// cli[impl command.surface.shell-default]
    /// Show or change the default shell.
    Default(ShellDefaultArgs),
}

impl ShellArgs {
    /// cli[impl shell.inline.launches-configured-default]
    /// # Errors
    ///
    /// This function will return an error if the shell action fails.
    pub async fn invoke(self, app_home: &crate::paths::AppHome) -> Result<()> {
        match self.command {
            Some(ShellCommand::Default(args)) => args.invoke(app_home).await,
            None => crate::app::run_inline_shell(app_home),
        }
    }
}

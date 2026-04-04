use crate::cli::window::show::WindowShowArgs;
use arbitrary::Arbitrary;
use facet::Facet;
use figue as args;

/// Window-related commands.
#[derive(Facet, Arbitrary, Debug, PartialEq)]
pub struct WindowArgs {
    /// The window subcommand to run.
    #[facet(args::subcommand)]
    pub command: WindowCommand,
}

/// Window subcommands.
#[derive(Facet, Arbitrary, Debug, PartialEq)]
#[repr(u8)]
pub enum WindowCommand {
    /// Show the main Teamy Studio window.
    Show(WindowShowArgs),
}

impl WindowArgs {
    /// # Errors
    ///
    /// This function will return an error if the subcommand fails.
    pub async fn invoke(self) -> eyre::Result<()> {
        match self.command {
            WindowCommand::Show(args) => args.invoke().await,
        }
    }
}

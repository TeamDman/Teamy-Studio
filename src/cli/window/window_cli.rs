use crate::cli::window::show::WindowShowArgs;
use arbitrary::Arbitrary;
use facet::Facet;
use figue as args;

/// Window-related commands.
/// cli[impl command.surface.window-show]
/// tool[impl cli.surface.window]
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
    /// cli[impl command.surface.window-show]
    /// Show the main Teamy Studio window.
    Show(WindowShowArgs),
}

impl WindowArgs {
    /// # Errors
    ///
    /// This function will return an error if the subcommand fails.
    pub async fn invoke(
        self,
        app_home: &crate::paths::AppHome,
        cache_home: &crate::paths::CacheHome,
    ) -> eyre::Result<()> {
        match self.command {
            WindowCommand::Show(args) => args.invoke(app_home, cache_home).await,
        }
    }
}

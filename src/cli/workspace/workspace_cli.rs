use crate::cli::workspace::create::WorkspaceCreateArgs;
use crate::cli::workspace::list::WorkspaceListArgs;
use crate::cli::workspace::run::WorkspaceRunArgs;
use crate::cli::workspace::show::WorkspaceShowArgs;
use arbitrary::Arbitrary;
use facet::Facet;
use figue as args;

/// Workspace-related commands.
/// cli[impl command.surface.workspace]
/// tool[impl cli.surface.workspace]
/// tool[impl cli.help.describes-workspace]
#[derive(Facet, Arbitrary, Debug, PartialEq)]
pub struct WorkspaceArgs {
    /// The workspace subcommand to run.
    #[facet(args::subcommand)]
    pub command: WorkspaceCommand,
}

/// Workspace subcommands.
#[derive(Facet, Arbitrary, Debug, PartialEq)]
#[repr(u8)]
pub enum WorkspaceCommand {
    /// cli[impl command.surface.workspace-list]
    /// List available workspaces.
    List(WorkspaceListArgs),
    /// cli[impl command.surface.workspace-show]
    /// Show a workspace by id or exact name.
    Show(WorkspaceShowArgs),
    /// cli[impl command.surface.workspace-create]
    /// Create a new workspace.
    Create(WorkspaceCreateArgs),
    /// cli[impl command.surface.workspace-run]
    /// Run an existing workspace or create a new one.
    Run(WorkspaceRunArgs),
}

impl WorkspaceArgs {
    /// # Errors
    ///
    /// This function will return an error if the workspace action fails.
    pub async fn invoke(
        self,
        app_home: &crate::paths::AppHome,
        cache_home: &crate::paths::CacheHome,
    ) -> eyre::Result<()> {
        match self.command {
            WorkspaceCommand::List(args) => args.invoke(app_home, cache_home).await,
            WorkspaceCommand::Show(args) => args.invoke(app_home, cache_home).await,
            WorkspaceCommand::Create(args) => args.invoke(app_home, cache_home).await,
            WorkspaceCommand::Run(args) => args.invoke(app_home, cache_home).await,
        }
    }
}

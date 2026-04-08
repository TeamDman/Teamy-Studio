use arbitrary::Arbitrary;
use facet::Facet;
use figue as args;

/// Run an existing workspace or create a new one.
/// cli[impl command.surface.workspace-run]
/// cli[impl workspace.run.no-target-creates-workspace]
/// cli[impl workspace.run.target-by-id-or-name]
#[derive(Facet, Arbitrary, Debug, PartialEq)]
pub struct WorkspaceRunArgs {
    /// Optional workspace id or exact workspace name.
    #[facet(args::positional)]
    pub target: Option<String>,
}

impl WorkspaceRunArgs {
    /// # Errors
    ///
    /// This function will return an error if the workspace cannot be launched.
    pub fn invoke(
        self,
        app_home: &crate::paths::AppHome,
        cache_home: &crate::paths::CacheHome,
    ) -> eyre::Result<()> {
        crate::app::run_workspace(app_home, cache_home, self.target.as_deref())
    }
}

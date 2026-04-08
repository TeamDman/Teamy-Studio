use arbitrary::Arbitrary;
use facet::Facet;
use figue as args;

/// Show a workspace by id or exact name.
/// cli[impl command.surface.workspace-show]
/// cli[impl workspace.show.bails-when-missing]
/// cli[impl workspace.show.prints-id-name-cell-count]
#[derive(Facet, Arbitrary, Debug, PartialEq)]
pub struct WorkspaceShowArgs {
    /// Workspace id or exact workspace name.
    #[facet(args::positional)]
    pub target: String,
}

impl WorkspaceShowArgs {
    /// # Errors
    ///
    /// This function will return an error if the workspace cannot be found or read.
    pub fn invoke(
        self,
        app_home: &crate::paths::AppHome,
        cache_home: &crate::paths::CacheHome,
    ) -> eyre::Result<()> {
        let _ = app_home;
        let workspace = crate::workspace::show_workspace(cache_home, &self.target)?;
        println!("id: {}", workspace.id.as_str());
        println!("name: {}", workspace.name);
        println!("cells: {}", workspace.cell_count);
        Ok(())
    }
}

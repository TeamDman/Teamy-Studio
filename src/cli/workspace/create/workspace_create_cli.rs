use arbitrary::Arbitrary;
use facet::Facet;
use figue as args;

/// Create a new workspace.
/// cli[impl command.surface.workspace-create]
/// cli[impl workspace.create.name-optional]
#[derive(Facet, Arbitrary, Debug, PartialEq)]
pub struct WorkspaceCreateArgs {
    /// Optional display name for the new workspace.
    #[facet(args::positional)]
    pub name: Option<String>,
}

impl WorkspaceCreateArgs {
    /// # Errors
    ///
    /// This function will return an error if the workspace cannot be created.
    pub fn invoke(
        self,
        app_home: &crate::paths::AppHome,
        cache_home: &crate::paths::CacheHome,
    ) -> eyre::Result<()> {
        let _ = app_home;
        let workspace = crate::workspace::create_workspace(cache_home, self.name.as_deref())?;
        println!("id: {}", workspace.workspace.id.as_str());
        println!("name: {}", workspace.workspace.name);
        println!("cells: {}", workspace.workspace.cell_count);
        Ok(())
    }
}

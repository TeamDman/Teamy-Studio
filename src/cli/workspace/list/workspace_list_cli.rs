use arbitrary::Arbitrary;
use facet::Facet;

/// List available workspaces.
/// cli[impl command.surface.workspace-list]
/// cli[impl workspace.list.prints-id-name-cell-count]
#[derive(Facet, Arbitrary, Debug, PartialEq)]
pub struct WorkspaceListArgs;

impl WorkspaceListArgs {
    /// # Errors
    ///
    /// This function will return an error if the workspaces cannot be listed.
    #[expect(clippy::unused_async)]
    pub async fn invoke(
        self,
        app_home: &crate::paths::AppHome,
        cache_home: &crate::paths::CacheHome,
    ) -> eyre::Result<()> {
        let _ = app_home;
        for workspace in crate::workspace::list_workspaces(cache_home)? {
            println!(
                "{}\t{}\t{}",
                workspace.id.as_str(),
                workspace.name,
                workspace.cell_count
            );
        }
        Ok(())
    }
}

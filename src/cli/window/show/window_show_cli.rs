use arbitrary::Arbitrary;
use facet::Facet;

/// Show the main Teamy Studio window.
/// cli[impl command.surface.window-show]
#[derive(Facet, Arbitrary, Debug, PartialEq)]
pub struct WindowShowArgs;

impl WindowShowArgs {
    /// # Errors
    ///
    /// This function will return an error if the application window cannot be created.
    #[expect(clippy::unused_async)]
    pub async fn invoke(self, app_home: &crate::paths::AppHome) -> eyre::Result<()> {
        crate::app::run(app_home)
    }
}

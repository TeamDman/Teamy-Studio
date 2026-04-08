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
    pub fn invoke(
        app_home: &crate::paths::AppHome,
        cache_home: &crate::paths::CacheHome,
    ) -> eyre::Result<()> {
        let _ = cache_home;
        crate::app::run(app_home)
    }
}

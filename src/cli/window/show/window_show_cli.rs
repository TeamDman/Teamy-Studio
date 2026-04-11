use arbitrary::Arbitrary;
use facet::Facet;
use figue::{self as args};

#[derive(Facet, Arbitrary, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[facet(rename_all = "kebab-case")]
#[repr(u8)]
pub enum WindowShowVtEngine {
    #[default]
    Ghostty,
    Teamy,
}

impl From<WindowShowVtEngine> for crate::app::VtEngineChoice {
    fn from(value: WindowShowVtEngine) -> Self {
        match value {
            WindowShowVtEngine::Ghostty => Self::Ghostty,
            WindowShowVtEngine::Teamy => Self::Teamy,
        }
    }
}

/// Show the main Teamy Studio window.
/// cli[impl command.surface.window-show]
#[derive(Facet, Arbitrary, Debug, PartialEq)]
#[facet(rename_all = "kebab-case")]
pub struct WindowShowArgs {
    /// cli[impl window.show.vt-engine-flag]
    /// Select which VT engine backs the live window.
    #[facet(args::named)]
    pub vt_engine: Option<WindowShowVtEngine>,
}

impl WindowShowArgs {
    /// # Errors
    ///
    /// This function will return an error if the application window cannot be created.
    pub fn invoke(
        self,
        app_home: &crate::paths::AppHome,
        cache_home: &crate::paths::CacheHome,
    ) -> eyre::Result<()> {
        let _ = cache_home;
        crate::app::run_with_vt_engine(app_home, self.vt_engine.unwrap_or_default().into())
    }
}

use arbitrary::Arbitrary;
use facet::Facet;
use figue::{self as args};
use std::path::Path;

/// Run a headless offscreen terminal render self-test.
/// cli[impl command.surface.self-test-render-offscreen]
#[derive(Facet, Arbitrary, Debug, PartialEq)]
#[facet(rename_all = "kebab-case")]
pub struct SelfTestRenderOffscreenArgs {
    /// cli[impl self-test.render-offscreen.artifact-output]
    /// Optional artifact output path for the rendered image or report.
    #[facet(args::named)]
    pub artifact_output: Option<String>,
}

impl SelfTestRenderOffscreenArgs {
    /// # Errors
    ///
    /// This function will return an error if the offscreen render self-test fails.
    pub fn invoke(
        self,
        app_home: &crate::paths::AppHome,
        cache_home: &crate::paths::CacheHome,
    ) -> eyre::Result<()> {
        crate::app::run_render_offscreen_self_test(
            app_home,
            cache_home,
            self.artifact_output.as_deref().map(Path::new),
        )
    }
}

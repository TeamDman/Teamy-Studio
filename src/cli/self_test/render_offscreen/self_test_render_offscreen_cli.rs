use arbitrary::Arbitrary;
use facet::Facet;
use figue::{self as args};
use std::path::Path;

use crate::cli::output::CliOutput;

/// Run a headless offscreen terminal render self-test.
// cli[impl command.surface.self-test-render-offscreen]
#[derive(Facet, Arbitrary, Debug, PartialEq)]
#[facet(rename_all = "kebab-case")]
pub struct SelfTestRenderOffscreenArgs {
    // cli[impl self-test.render-offscreen.fixture-flag]
    /// Optional built-in fixture name to run.
    #[facet(args::named)]
    pub fixture: Option<String>,
    // cli[impl self-test.render-offscreen.list-fixtures-flag]
    /// List the available built-in render fixtures without executing one.
    #[facet(args::named, default)]
    pub list_fixtures: bool,
    // cli[impl self-test.render-offscreen.update-expected-flag]
    /// Update the expected render image for the selected fixture.
    #[facet(args::named, default)]
    pub update_expected: bool,
    // cli[impl self-test.render-offscreen.artifact-output]
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
    ) -> eyre::Result<CliOutput> {
        if self.list_fixtures {
            return Ok(CliOutput::facet(
                crate::app::list_render_offscreen_self_test_fixtures(),
            ));
        }

        Ok(CliOutput::facet(
            crate::app::run_render_offscreen_self_test(
                app_home,
                cache_home,
                self.fixture.as_deref(),
                self.artifact_output.as_deref().map(Path::new),
                self.update_expected,
            )?,
        ))
    }
}

use arbitrary::Arbitrary;
use facet::Facet;
use figue::{self as args};
use std::path::Path;

use crate::cli::output::CliOutput;

#[derive(Facet, Arbitrary, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[facet(rename_all = "kebab-case")]
#[repr(u8)]
pub enum SelfTestKeyboardInputVtEngine {
    #[default]
    Ghostty,
    Teamy,
}

impl From<SelfTestKeyboardInputVtEngine> for crate::app::VtEngineChoice {
    fn from(value: SelfTestKeyboardInputVtEngine) -> Self {
        match value {
            SelfTestKeyboardInputVtEngine::Ghostty => Self::Ghostty,
            SelfTestKeyboardInputVtEngine::Teamy => Self::Teamy,
        }
    }
}

/// Run the keyboard input self-test harness.
// cli[impl command.surface.self-test-keyboard-input]
#[derive(Facet, Arbitrary, Debug, PartialEq)]
#[facet(rename_all = "kebab-case")]
pub struct SelfTestKeyboardInputArgs {
    // cli[impl self-test.keyboard-input.scenario-optional]
    /// Optional reproduction scenario to run from the outer harness.
    #[facet(args::positional)]
    pub scenario: Option<String>,

    // cli[impl self-test.keyboard-input.inside-flag]
    /// Run the terminal-side probe instead of the outer harness.
    #[facet(args::named, default)]
    pub inside: bool,

    // cli[impl self-test.keyboard-input.artifact-output]
    /// Optional artifact output path for the captured transcript.
    #[facet(args::named)]
    pub artifact_output: Option<String>,

    // cli[impl self-test.keyboard-input.vt-engine-flag]
    /// Select which VT engine backs the keyboard-input self-test session.
    #[facet(args::named)]
    pub vt_engine: Option<SelfTestKeyboardInputVtEngine>,
}

impl SelfTestKeyboardInputArgs {
    /// # Errors
    ///
    /// This function will return an error if the keyboard input self-test fails.
    pub fn invoke(
        self,
        app_home: &crate::paths::AppHome,
        cache_home: &crate::paths::CacheHome,
    ) -> eyre::Result<CliOutput> {
        let _ = cache_home;
        Ok(CliOutput::facet(crate::app::run_keyboard_input_self_test(
            app_home,
            self.inside,
            self.scenario.as_deref(),
            self.artifact_output.as_deref().map(Path::new),
            self.vt_engine.unwrap_or_default().into(),
        )?))
    }
}

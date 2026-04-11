use arbitrary::Arbitrary;
use facet::Facet;
use figue::{self as args};
use std::path::Path;

/// Run a headless terminal transcript replay self-test.
/// cli[impl command.surface.self-test-terminal-replay]
#[derive(Facet, Arbitrary, Debug, PartialEq)]
#[facet(rename_all = "kebab-case")]
pub struct SelfTestTerminalReplayArgs {
    /// Replay fixture path.
    #[facet(args::named)]
    pub fixture: String,

    /// cli[impl self-test.terminal-replay.artifact-output]
    /// Optional artifact output path for the replay report.
    #[facet(args::named)]
    pub artifact_output: Option<String>,

    /// Number of replay samples to run.
    #[facet(args::named)]
    pub samples: Option<usize>,
}

impl SelfTestTerminalReplayArgs {
    /// # Errors
    ///
    /// This function will return an error if the replay self-test fails.
    pub fn invoke(
        self,
        app_home: &crate::paths::AppHome,
        _cache_home: &crate::paths::CacheHome,
    ) -> eyre::Result<()> {
        let _ = app_home;
        crate::app::run_terminal_replay_self_test(
            Path::new(&self.fixture),
            self.artifact_output.as_deref().map(Path::new),
            self.samples.unwrap_or(1).max(1),
        )
    }
}

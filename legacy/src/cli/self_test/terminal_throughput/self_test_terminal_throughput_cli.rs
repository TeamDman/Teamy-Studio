use arbitrary::Arbitrary;
use facet::Facet;
use figue::{self as args};

use crate::cli::output::CliOutput;

/// Terminal throughput benchmark modes.
#[derive(Facet, Arbitrary, Clone, Copy, Debug, PartialEq, Eq)]
#[facet(rename_all = "kebab-case")]
#[repr(u8)]
pub enum SelfTestTerminalThroughputMode {
    MeasureCommandOutHost,
    StreamSmallBatches,
    WideLines,
    ScrollFlood,
    PromptBursts,
    ResizeDuringOutput,
}

impl From<SelfTestTerminalThroughputMode> for crate::app::TerminalThroughputBenchmarkMode {
    fn from(value: SelfTestTerminalThroughputMode) -> Self {
        match value {
            SelfTestTerminalThroughputMode::MeasureCommandOutHost => Self::MeasureCommandOutHost,
            SelfTestTerminalThroughputMode::StreamSmallBatches => Self::StreamSmallBatches,
            SelfTestTerminalThroughputMode::WideLines => Self::WideLines,
            SelfTestTerminalThroughputMode::ScrollFlood => Self::ScrollFlood,
            SelfTestTerminalThroughputMode::PromptBursts => Self::PromptBursts,
            SelfTestTerminalThroughputMode::ResizeDuringOutput => Self::ResizeDuringOutput,
        }
    }
}

/// Run the terminal throughput self-test benchmark.
// cli[impl command.surface.self-test-terminal-throughput]
#[derive(Facet, Arbitrary, Debug, PartialEq)]
#[facet(rename_all = "kebab-case")]
pub struct SelfTestTerminalThroughputArgs {
    // cli[impl self-test.terminal-throughput.mode-optional]
    /// Optional benchmark mode to run.
    #[facet(args::positional)]
    pub mode: Option<SelfTestTerminalThroughputMode>,

    // cli[impl self-test.terminal-throughput.line-count-flag]
    /// Number of lines to emit through `Out-Host`.
    #[facet(args::named)]
    pub line_count: Option<usize>,

    // cli[impl self-test.terminal-throughput.samples-flag]
    /// Number of benchmark samples to run before reporting results.
    #[facet(args::named)]
    pub samples: Option<usize>,
}

impl SelfTestTerminalThroughputArgs {
    /// # Errors
    ///
    /// This function will return an error if the throughput self-test fails.
    pub fn invoke(
        self,
        app_home: &crate::paths::AppHome,
        cache_home: &crate::paths::CacheHome,
    ) -> eyre::Result<CliOutput> {
        Ok(CliOutput::facet(
            crate::app::run_terminal_throughput_self_test(
                app_home,
                cache_home,
                self.mode.map(Into::into),
                self.line_count.unwrap_or(10_000),
                self.samples.unwrap_or(1).max(1),
            )?,
        ))
    }
}

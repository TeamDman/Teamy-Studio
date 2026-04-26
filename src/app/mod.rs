mod audio_transcription;
mod cell_grid;
mod render_verification;
mod spatial;
pub mod teamy_terminal_engine;
mod vt_types;
mod windows_app;
mod windows_audio;
mod windows_audio_input;
mod windows_cursor_info;
mod windows_d3d12_renderer;
mod windows_demo_mode;
mod windows_dialogs;
mod windows_scene;
mod windows_terminal;
#[cfg(feature = "ghostty")]
mod windows_terminal_engine;
mod windows_terminal_replay;
mod windows_terminal_self_test;

use std::path::Path;

use crate::paths::{AppHome, CacheHome};
use eyre::Context;
use facet::Facet;

pub use audio_transcription::{
    AudioTranscriptionControlRequest, AudioTranscriptionControlResult,
    AudioTranscriptionDaemonStatusReport, AudioTranscriptionQueuedRequest,
    AudioTranscriptionSharedMemorySlotPool, AudioTranscriptionSharedMemorySlotPoolStatus,
    WhisperLogMel80x3000, audio_transcription_control_request_for_queued_request,
    audio_transcription_daemon_status, audio_transcription_daemon_status_with_pool_status,
    decode_audio_transcription_control_result_line,
    encode_audio_transcription_control_request_line,
};
pub use render_verification::{RenderOffscreenFixtureListReport, RenderOffscreenSelfTestReport};
pub use windows_app::TerminalThroughputBenchmarkResultsReport;
pub use windows_audio_input::{
    AudioInputDeviceListReport, AudioInputDeviceSummary, list_active_audio_input_devices,
};
pub use windows_cursor_info::{CursorInfoConfig, CursorInfoPixelSize, CursorInfoRenderMode};
pub use windows_terminal_replay::TerminalReplayReport;
pub use windows_terminal_self_test::KeyboardInputSelfTestReport;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalThroughputBenchmarkMode {
    MeasureCommandOutHost,
    StreamSmallBatches,
    WideLines,
    ScrollFlood,
    PromptBursts,
    ResizeDuringOutput,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VtEngineChoice {
    Ghostty,
    #[default]
    Teamy,
}

impl VtEngineChoice {
    pub const CURRENT_TERMINAL_VT_ENGINE_ENV_VAR: &str = "TEAMY_STUDIO_CURRENT_TERMINAL_VT_ENGINE";

    #[must_use]
    pub const fn current_terminal_vt_engine_env_value(self) -> &'static str {
        match self {
            Self::Ghostty => "ghostty",
            Self::Teamy => "teamy",
        }
    }
}

#[derive(Clone, Debug, Facet, PartialEq, Eq)]
pub struct TerminalWindowSummary {
    pub hwnd: usize,
    pub pid: u32,
    pub title: String,
}

/// Run the Teamy Studio application shell.
// cli[impl command.surface.core]
/// windowing[impl launcher.startup.default]
///
/// # Errors
///
/// This function will return an error if the platform-specific window cannot be launched.
pub fn run(app_home: &AppHome) -> eyre::Result<()> {
    windows_app::run_launcher(app_home, VtEngineChoice::default())
}

/// Run the standalone cursor-info diagnostic TUI.
///
/// # Errors
///
/// This function will return an error if the terminal UI cannot be initialized or the
/// Windows snapshot backend fails.
pub fn run_cursor_info(app_home: &AppHome, config: CursorInfoConfig) -> eyre::Result<()> {
    let _ = app_home;
    windows_cursor_info::run(config)
}

/// Run the Teamy Studio application shell with an explicit VT engine.
///
/// # Errors
///
/// This function will return an error if the platform-specific window cannot be launched.
pub fn run_with_vt_engine(app_home: &AppHome, vt_engine: VtEngineChoice) -> eyre::Result<()> {
    windows_app::run_launcher(app_home, vt_engine)
}

/// Open a terminal window from an explicit command argv.
// cli[impl command.surface.terminal-open]
// cli[impl terminal.open.stdin-flag]
// cli[impl terminal.open.title-flag]
// cli[impl terminal.open.vt-engine-flag]
// cli[impl terminal.open.current-vt-engine-env]
///
/// # Errors
///
/// This function will return an error if the platform-specific window cannot be launched.
pub fn open_terminal_window(
    app_home: &AppHome,
    command_argv: Option<&[String]>,
    initial_stdin: Option<&str>,
    title: Option<&str>,
    vt_engine: VtEngineChoice,
) -> eyre::Result<()> {
    open_terminal_window_with_vt_engine(app_home, command_argv, initial_stdin, title, vt_engine)
        .map_err(|error| {
            error.wrap_err(format!(
                "failed to open terminal window{}",
                title.map_or_else(String::new, |value| format!(" `{value}`"))
            ))
        })
}

fn open_terminal_window_with_vt_engine(
    app_home: &AppHome,
    command_argv: Option<&[String]>,
    initial_stdin: Option<&str>,
    title: Option<&str>,
    vt_engine: VtEngineChoice,
) -> eyre::Result<()> {
    let working_dir =
        std::env::current_dir().wrap_err("failed to resolve the current working directory")?;
    windows_app::run(
        app_home,
        &working_dir,
        command_argv,
        initial_stdin,
        title,
        vt_engine,
    )
}

/// Enumerate live Teamy Studio terminal windows from the OS window list.
// cli[impl command.surface.terminal-list]
// cli[impl terminal.list.enumerates-live-windows]
///
/// # Errors
///
/// This function will return an error if window enumeration fails.
pub fn list_terminal_windows() -> eyre::Result<Vec<TerminalWindowSummary>> {
    windows_app::list_terminal_windows()
}

/// Run the keyboard input self-test harness.
///
/// # Errors
///
/// This function will return an error if the Windows-only self-test cannot be launched.
pub fn run_keyboard_input_self_test(
    app_home: &AppHome,
    inside: bool,
    scenario: Option<&str>,
    artifact_output: Option<&Path>,
    vt_engine: VtEngineChoice,
) -> eyre::Result<KeyboardInputSelfTestReport> {
    windows_terminal_self_test::run(app_home, inside, scenario, artifact_output, vt_engine)
}

/// Run the terminal throughput self-test benchmark.
/// cli[impl command.surface.self-test-terminal-throughput]
/// tool[impl cli.surface.self-test-terminal-throughput]
///
/// # Errors
///
/// This function will return an error if the Windows-only benchmark cannot be launched.
pub fn run_terminal_throughput_self_test(
    app_home: &AppHome,
    cache_home: &CacheHome,
    mode: Option<TerminalThroughputBenchmarkMode>,
    line_count: usize,
    samples: usize,
) -> eyre::Result<TerminalThroughputBenchmarkResultsReport> {
    windows_app::run_terminal_throughput_self_test(app_home, cache_home, mode, line_count, samples)
}

/// Run a headless terminal replay self-test.
// cli[impl command.surface.self-test-terminal-replay]
// cli[impl self-test.terminal-replay.artifact-output]
/// tool[impl cli.surface.self-test]
/// behavior[impl window.interaction.rendering.headless-verification]
/// tool[impl tests.headless.required-for-terminal-engine]
/// tool[impl tests.performance.terminal-throughput-replay]
///
/// # Errors
///
/// This function will return an error if the replay self-test fails.
pub fn run_terminal_replay_self_test(
    fixture_path: &Path,
    artifact_output: Option<&Path>,
    samples: usize,
) -> eyre::Result<TerminalReplayReport> {
    windows_terminal_replay::run_terminal_replay_self_test(fixture_path, artifact_output, samples)
}

/// Run a headless offscreen render self-test.
// cli[impl command.surface.self-test-render-offscreen]
// cli[impl self-test.render-offscreen.artifact-output]
// cli[impl self-test.render-offscreen.fixture-flag]
// cli[impl self-test.render-offscreen.update-expected-flag]
/// tool[impl cli.surface.self-test]
/// behavior[impl window.interaction.rendering.headless-verification]
/// tool[impl tests.headless.required-for-terminal-engine]
///
/// # Errors
///
/// This function will return an error if the offscreen render self-test fails.
pub fn run_render_offscreen_self_test(
    app_home: &AppHome,
    cache_home: &CacheHome,
    fixture: Option<&str>,
    artifact_output: Option<&Path>,
    update_expected: bool,
) -> eyre::Result<RenderOffscreenSelfTestReport> {
    let _ = app_home;
    let _ = cache_home;

    render_verification::run_render_offscreen_fixture(fixture, artifact_output, update_expected)
}

/// cli[impl self-test.render-offscreen.list-fixtures-flag]
#[must_use]
pub fn list_render_offscreen_self_test_fixtures() -> RenderOffscreenFixtureListReport {
    render_verification::list_render_offscreen_fixtures()
}

/// os[impl os.windows.rendering.direct3d12.offscreen-terminal-verification]
///
/// # Errors
///
/// This function will return an error if the selected offscreen render fixture fails to render,
/// does not match its expected image, or cannot write requested artifacts.
pub fn run_render_offscreen_verification_fixture(
    fixture: Option<&str>,
    artifact_output: Option<&Path>,
    update_expected: bool,
) -> eyre::Result<RenderOffscreenSelfTestReport> {
    render_verification::run_render_offscreen_fixture(fixture, artifact_output, update_expected)
}

/// Write a PNG snapshot for a single slug glyph.
///
/// # Errors
///
/// This function will return an error if the glyph snapshot cannot be rendered or written.
pub fn write_slug_snapshot_png(
    character: char,
    font_size_px: u32,
    image_width: u32,
    image_height: u32,
    output_path: &Path,
) -> eyre::Result<()> {
    windows_d3d12_renderer::write_slug_snapshot_png(
        character,
        font_size_px,
        image_width,
        image_height,
        output_path,
    )
}

/// Write a PNG sheet containing multiple slug glyph snapshots plus an index file.
///
/// # Errors
///
/// This function will return an error if the snapshot sheet or index cannot be produced.
pub fn write_slug_snapshot_sheet_png(
    font_size_px: u32,
    cell_size_px: u32,
    columns: u32,
    output_path: &Path,
    index_output_path: &Path,
) -> eyre::Result<()> {
    windows_d3d12_renderer::write_slug_snapshot_sheet_png(
        font_size_px,
        cell_size_px,
        columns,
        output_path,
        index_output_path,
    )
}

#[cfg(windows)]
mod spatial;
pub mod teamy_terminal_engine;
#[cfg(windows)]
mod windows_app;
#[cfg(windows)]
mod windows_d3d12_renderer;
#[cfg(windows)]
mod windows_dialogs;
#[cfg(windows)]
mod windows_terminal;
#[cfg(windows)]
mod windows_terminal_engine;
#[cfg(windows)]
mod windows_terminal_replay;
#[cfg(windows)]
mod windows_terminal_self_test;

use std::path::Path;
#[cfg(windows)]
use std::sync::Arc;

#[cfg(windows)]
use crate::app::spatial::TerminalCellPoint;
use crate::paths::{AppHome, CacheHome, CellId};
use crate::workspace::{WorkspaceLaunch, WorkspaceSummary};

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
    #[default]
    Ghostty,
    Teamy,
}

#[derive(Clone, Debug)]
pub struct WorkspaceWindowState {
    pub cache_home: CacheHome,
    pub workspace: WorkspaceSummary,
    pub cell_id: CellId,
    pub cell_number: usize,
}

/// Run the Teamy Studio application shell.
/// cli[impl command.surface.core]
///
/// # Errors
///
/// This function will return an error if the platform-specific window cannot be launched.
pub fn run(app_home: &AppHome) -> eyre::Result<()> {
    run_in_dir_with_vt_engine(app_home, None, None, VtEngineChoice::Ghostty)
}

/// Run the Teamy Studio application shell with an explicit VT engine.
/// cli[impl window.show.vt-engine-flag]
///
/// # Errors
///
/// This function will return an error if the platform-specific window cannot be launched.
pub fn run_with_vt_engine(app_home: &AppHome, vt_engine: VtEngineChoice) -> eyre::Result<()> {
    run_in_dir_with_vt_engine(app_home, None, None, vt_engine)
}

/// Run the Teamy Studio application shell with an explicit starting directory.
/// behavior[impl window.appearance.shell-starts-in-workspace-cell-dir]
///
/// # Errors
///
/// This function will return an error if the platform-specific window cannot be launched.
pub fn run_in_dir(
    app_home: &AppHome,
    working_dir: Option<&Path>,
    workspace_window: Option<WorkspaceWindowState>,
) -> eyre::Result<()> {
    run_in_dir_with_vt_engine(
        app_home,
        working_dir,
        workspace_window,
        VtEngineChoice::Ghostty,
    )
}

/// Run the Teamy Studio application shell with an explicit starting directory and VT engine.
///
/// # Errors
///
/// This function will return an error if the platform-specific window cannot be launched.
pub fn run_in_dir_with_vt_engine(
    app_home: &AppHome,
    working_dir: Option<&Path>,
    workspace_window: Option<WorkspaceWindowState>,
    vt_engine: VtEngineChoice,
) -> eyre::Result<()> {
    #[cfg(windows)]
    {
        windows_app::run(app_home, working_dir, workspace_window, vt_engine)
    }

    #[cfg(not(windows))]
    {
        let _ = app_home;
        let _ = working_dir;
        let _ = workspace_window;
        let _ = vt_engine;
        eyre::bail!("Teamy Studio currently only supports Windows")
    }
}

/// Run a notebook workspace, creating a new one when no target is provided.
/// cli[impl command.surface.core]
/// cli[impl workspace.run.no-target-creates-workspace]
/// cli[impl workspace.run.target-by-id-or-name]
///
/// # Errors
///
/// This function will return an error if the workspace cannot be resolved or the window cannot be launched.
pub fn run_workspace(
    app_home: &AppHome,
    cache_home: &CacheHome,
    target: Option<&str>,
) -> eyre::Result<()> {
    let launch = crate::workspace::open_workspace(cache_home, target)?;
    run_workspace_launch(app_home, cache_home, launch)
}

/// Run a resolved workspace launch in the application window.
///
/// # Errors
///
/// This function will return an error if the application window cannot be launched.
pub fn run_workspace_launch(
    app_home: &AppHome,
    cache_home: &CacheHome,
    launch: WorkspaceLaunch,
) -> eyre::Result<()> {
    run_in_dir(
        app_home,
        Some(&launch.first_cell_dir),
        Some(WorkspaceWindowState {
            cache_home: cache_home.clone(),
            workspace: launch.workspace,
            cell_id: launch.first_cell_id,
            cell_number: launch.cell_number,
        }),
    )
}

/// Run the configured default shell inline in the current console.
/// cli[impl shell.inline.launches-configured-default]
///
/// # Errors
///
/// This function will return an error if the shell cannot be launched.
pub fn run_inline_shell(app_home: &AppHome) -> eyre::Result<()> {
    #[cfg(windows)]
    {
        use eyre::Context;
        use tracing::info;

        let command_argv = crate::shell_default::load_effective_argv(app_home)?;
        let (program, args) = command_argv
            .split_first()
            .ok_or_else(|| eyre::eyre!("default shell command cannot be empty"))?;

        info!(program, args = ?args, "starting Teamy Studio inline shell");
        let status = std::process::Command::new(program)
            .args(args)
            .status()
            .wrap_err_with(|| format!("failed to launch inline shell `{program}`"))?;
        info!(?status, "Teamy Studio inline shell exited");
        Ok(())
    }

    #[cfg(not(windows))]
    {
        let _ = app_home;
        eyre::bail!("Teamy Studio currently only supports Windows")
    }
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
) -> eyre::Result<()> {
    #[cfg(windows)]
    {
        windows_terminal_self_test::run(app_home, inside, scenario, artifact_output, vt_engine)
    }

    #[cfg(not(windows))]
    {
        let _ = app_home;
        let _ = inside;
        let _ = scenario;
        let _ = artifact_output;
        let _ = vt_engine;
        eyre::bail!("Teamy Studio keyboard self-test currently only supports Windows")
    }
}

/// Run the terminal throughput self-test benchmark.
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
) -> eyre::Result<()> {
    #[cfg(windows)]
    {
        windows_app::run_terminal_throughput_self_test(
            app_home, cache_home, mode, line_count, samples,
        )
    }

    #[cfg(not(windows))]
    {
        let _ = app_home;
        let _ = cache_home;
        let _ = mode;
        let _ = line_count;
        let _ = samples;
        eyre::bail!("Teamy Studio terminal throughput self-test currently only supports Windows")
    }
}

/// Run a headless terminal replay self-test.
/// cli[impl command.surface.self-test-terminal-replay]
/// cli[impl self-test.terminal-replay.artifact-output]
/// tool[impl cli.surface.self-test]
///
/// # Errors
///
/// This function will return an error if the replay self-test fails.
pub fn run_terminal_replay_self_test(
    fixture_path: &Path,
    artifact_output: Option<&Path>,
    samples: usize,
) -> eyre::Result<()> {
    #[cfg(windows)]
    {
        windows_terminal_replay::run_terminal_replay_self_test(
            fixture_path,
            artifact_output,
            samples,
        )
    }

    #[cfg(not(windows))]
    {
        let _ = fixture_path;
        let _ = artifact_output;
        let _ = samples;
        eyre::bail!("Teamy Studio terminal replay self-test currently only supports Windows")
    }
}

/// Run a headless offscreen render self-test.
/// cli[impl command.surface.self-test-render-offscreen]
/// cli[impl self-test.render-offscreen.artifact-output]
/// tool[impl cli.surface.self-test]
///
/// # Errors
///
/// This function will return an error if the offscreen render self-test fails.
pub fn run_render_offscreen_self_test(
    app_home: &AppHome,
    cache_home: &CacheHome,
    artifact_output: Option<&Path>,
) -> eyre::Result<()> {
    #[cfg(windows)]
    {
        let _ = app_home;
        let _ = cache_home;

        let frame = build_offscreen_render_self_test_frame();

        if let Some(output_path) = artifact_output {
            windows_d3d12_renderer::write_render_frame_model_offscreen_png(&frame, output_path)?;
            println!("artifact_path: {}", output_path.display());
        }

        let image = windows_d3d12_renderer::render_frame_model_offscreen_image(&frame)?;
        let (non_transparent_pixels, bright_pixels) = summarize_offscreen_image(&image);
        println!(
            "image_width: {}\nimage_height: {}\nnon_transparent_pixels: {}\nbright_pixels: {}",
            image.width(),
            image.height(),
            non_transparent_pixels,
            bright_pixels,
        );
        if non_transparent_pixels == 0 || bright_pixels == 0 {
            eyre::bail!("offscreen render produced an empty image")
        }
        Ok(())
    }

    #[cfg(not(windows))]
    {
        let _ = app_home;
        let _ = cache_home;
        let _ = artifact_output;
        eyre::bail!("headless offscreen render self-test currently only supports Windows")
    }
}

#[cfg(windows)]
fn build_offscreen_render_self_test_frame() -> windows_d3d12_renderer::RenderFrameModel {
    let layout = windows_terminal::TerminalLayout {
        client_width: 1040,
        client_height: 680,
        cell_width: 8,
        cell_height: 16,
    };
    let terminal_display = Arc::new(windows_terminal::TerminalDisplayState {
        rows: vec![
            build_offscreen_render_row(0, "echo offscreen", [0.92, 0.94, 0.98, 1.0], true),
            build_offscreen_render_row(1, "headless renderer", [0.96, 0.90, 0.70, 1.0], false),
        ],
        dirty_rows: vec![0, 1],
        cursor: Some(windows_terminal::TerminalDisplayCursor {
            cell: TerminalCellPoint::new(8, 1),
            color: [0.96, 0.45, 1.0, 1.0],
            style: windows_terminal::TerminalDisplayCursorStyle::Block,
        }),
        scrollbar: Some(windows_terminal::TerminalDisplayScrollbar {
            total: 100,
            offset: 40,
            visible: 24,
        }),
    });

    windows_d3d12_renderer::RenderFrameModel {
        layout,
        cell_number: 7,
        output_text: "offscreen render self-test".to_owned(),
        output_cell_width: 8,
        output_cell_height: 16,
        terminal_cell_width: 8,
        terminal_cell_height: 16,
        terminal_display,
        terminal_visual_state: windows_d3d12_renderer::RendererTerminalVisualState {
            track_hovered: true,
            thumb_hovered: true,
            thumb_grabbed: false,
        },
    }
}

#[cfg(windows)]
fn build_offscreen_render_row(
    row: i32,
    text: &str,
    color: [f32; 4],
    include_background: bool,
) -> windows_terminal::TerminalDisplayRow {
    windows_terminal::TerminalDisplayRow {
        row,
        backgrounds: if include_background {
            vec![windows_terminal::TerminalDisplayBackground {
                cell: TerminalCellPoint::new(0, row),
                color: [0.18, 0.18, 0.24, 1.0],
            }]
        } else {
            Vec::new()
        },
        glyphs: text
            .chars()
            .enumerate()
            .map(
                |(column, character)| windows_terminal::TerminalDisplayGlyph {
                    cell: TerminalCellPoint::new(i32::try_from(column).unwrap_or_default(), row),
                    character,
                    color,
                },
            )
            .collect(),
    }
}

#[cfg(windows)]
fn summarize_offscreen_image(
    image: &image::ImageBuffer<image::Rgba<u8>, Vec<u8>>,
) -> (usize, usize) {
    let non_transparent_pixels = image.pixels().filter(|pixel| pixel[3] > 0).count();
    let bright_pixels = image
        .pixels()
        .filter(|pixel| u16::from(pixel[0]) + u16::from(pixel[1]) + u16::from(pixel[2]) > 64)
        .count();
    (non_transparent_pixels, bright_pixels)
}

#[cfg(windows)]
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

#[cfg(windows)]
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

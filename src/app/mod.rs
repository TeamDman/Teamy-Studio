#[cfg(windows)]
mod spatial;
#[cfg(windows)]
mod windows_app;
#[cfg(windows)]
mod windows_d3d12_renderer;
#[cfg(windows)]
mod windows_terminal;
#[cfg(windows)]
mod windows_terminal_self_test;

use std::path::Path;

use crate::paths::{AppHome, CacheHome, CellId};
use crate::workspace::{WorkspaceLaunch, WorkspaceSummary};

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
    run_in_dir(app_home, None, None)
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
    #[cfg(windows)]
    {
        windows_app::run(app_home, working_dir, workspace_window)
    }

    #[cfg(not(windows))]
    {
        let _ = app_home;
        let _ = working_dir;
        let _ = workspace_window;
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
pub fn run_keyboard_input_self_test(app_home: &AppHome, inside: bool) -> eyre::Result<()> {
    #[cfg(windows)]
    {
        windows_terminal_self_test::run(app_home, inside)
    }

    #[cfg(not(windows))]
    {
        let _ = app_home;
        let _ = inside;
        eyre::bail!("Teamy Studio keyboard self-test currently only supports Windows")
    }
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

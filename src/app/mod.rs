#[cfg(windows)]
mod windows_app;
#[cfg(windows)]
mod windows_terminal;
#[cfg(windows)]
mod windows_terminal_self_test;

use crate::paths::AppHome;

/// Run the Teamy Studio application shell.
/// cli[impl command.surface.core]
///
/// # Errors
///
/// This function will return an error if the platform-specific window cannot be launched.
pub fn run(app_home: &AppHome) -> eyre::Result<()> {
    #[cfg(windows)]
    {
        windows_app::run(app_home)
    }

    #[cfg(not(windows))]
    {
        let _ = app_home;
        eyre::bail!("Teamy Studio currently only supports Windows")
    }
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

        let argv = crate::shell_default::load_effective_argv(app_home)?;
        let (program, args) = argv
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

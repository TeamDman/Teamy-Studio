#[cfg(windows)]
mod windows_app;
#[cfg(windows)]
mod windows_terminal;
#[cfg(windows)]
mod windows_terminal_self_test;

/// Run the Teamy Studio application shell.
///
/// # Errors
///
/// This function will return an error if the platform-specific window cannot be launched.
pub fn run() -> eyre::Result<()> {
    #[cfg(windows)]
    {
        windows_app::run()
    }

    #[cfg(not(windows))]
    {
        eyre::bail!("Teamy Studio currently only supports Windows")
    }
}

/// Run the keyboard input self-test harness.
///
/// # Errors
///
/// This function will return an error if the Windows-only self-test cannot be launched.
pub fn run_keyboard_input_self_test(inside: bool) -> eyre::Result<()> {
    #[cfg(windows)]
    {
        windows_terminal_self_test::run(inside)
    }

    #[cfg(not(windows))]
    {
        let _ = inside;
        eyre::bail!("Teamy Studio keyboard self-test currently only supports Windows")
    }
}

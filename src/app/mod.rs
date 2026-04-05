#[cfg(windows)]
mod windows_app;
#[cfg(windows)]
mod windows_terminal;

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

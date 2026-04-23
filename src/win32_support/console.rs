use eyre::Context;
use windows::Win32::System::Console::{
    CONSOLE_MODE, ENABLE_VIRTUAL_TERMINAL_PROCESSING, GetConsoleMode, GetStdHandle,
    STD_OUTPUT_HANDLE, SetConsoleMode,
};

/// Enable ANSI escape sequence support on the active console output handle.
///
/// # Errors
///
/// Returns an error when stdout has no console handle attached or the console
/// mode cannot be queried or updated.
pub fn enable_ansi_support() -> eyre::Result<()> {
    // Safety: retrieving the current process stdout handle requires no additional invariants.
    let handle = unsafe { GetStdHandle(STD_OUTPUT_HANDLE) }
        .wrap_err("failed to get standard output handle")?;
    if handle.is_invalid() {
        return Err(windows::core::Error::from_thread()).wrap_err("STD_OUTPUT_HANDLE is invalid");
    }

    let mut mode = CONSOLE_MODE::default();
    // Safety: `mode` is a valid out-pointer for the queried console handle.
    unsafe { GetConsoleMode(handle, &raw mut mode) }.wrap_err("failed to get console mode")?;
    // Safety: updating the console mode for the current stdout handle is valid.
    unsafe { SetConsoleMode(handle, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING) }
        .wrap_err("failed to set console mode")?;
    Ok(())
}

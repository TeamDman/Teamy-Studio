#[cfg(windows)]
use std::io::Write;

#[cfg(windows)]
fn main() -> eyre::Result<()> {
    use std::io::{self};

    use eyre::Context;
    use windows::Win32::System::Console::{
        CONSOLE_MODE, ENABLE_ECHO_INPUT, ENABLE_LINE_INPUT, ENABLE_QUICK_EDIT_MODE,
        ENABLE_WINDOW_INPUT, GetConsoleMode, GetStdHandle, STD_INPUT_HANDLE, SetConsoleMode,
    };

    let event_limit = std::env::var("TEAMY_KEY_PROBE_EVENT_LIMIT")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(16);

    // Safety: querying the standard input handle does not require additional invariants.
    let input =
        unsafe { GetStdHandle(STD_INPUT_HANDLE) }.wrap_err("failed to get console input handle")?;
    let mut original_mode = CONSOLE_MODE(0);
    // Safety: `original_mode` is a valid out-pointer for `GetConsoleMode`.
    unsafe { GetConsoleMode(input, &raw mut original_mode) }
        .wrap_err("failed to read console mode")?;

    let raw_mode = (original_mode | ENABLE_WINDOW_INPUT)
        & !(ENABLE_LINE_INPUT | ENABLE_ECHO_INPUT | ENABLE_QUICK_EDIT_MODE);
    // Safety: `input` is a valid console input handle and `raw_mode` is derived from the current mode bits.
    unsafe { SetConsoleMode(input, raw_mode) }
        .wrap_err("failed to enable windows_key_probe console mode")?;
    let _mode_guard = ConsoleModeGuard {
        input,
        original_mode,
    };

    print!("\x1b[?9001h");
    io::stdout().flush()?;

    println!("WINDOWS_KEY_PROBE_READY");
    io::stdout().flush()?;

    let mut events_seen = 0_usize;
    while events_seen < event_limit {
        let event = read_next_key_event(input)?;
        if should_skip_event(&event) {
            continue;
        }

        events_seen += 1;
        println!("EVENT {}", format_key_event(events_seen, &event));
        io::stdout().flush()?;
    }

    Ok(())
}

#[cfg(not(windows))]
fn main() -> eyre::Result<()> {
    eyre::bail!("windows-key-probe only supports Windows")
}

#[cfg(windows)]
struct ConsoleModeGuard {
    input: windows::Win32::Foundation::HANDLE,
    original_mode: windows::Win32::System::Console::CONSOLE_MODE,
}

#[cfg(windows)]
impl Drop for ConsoleModeGuard {
    fn drop(&mut self) {
        print!("\x1b[?9001l");
        let _ = std::io::stdout().flush();
        // Safety: this restores the previously read console mode on the same input handle.
        let _ = unsafe {
            windows::Win32::System::Console::SetConsoleMode(self.input, self.original_mode)
        };
    }
}

#[cfg(windows)]
fn read_next_key_event(
    input: windows::Win32::Foundation::HANDLE,
) -> eyre::Result<windows::Win32::System::Console::KEY_EVENT_RECORD> {
    use eyre::Context;
    use windows::Win32::System::Console::{INPUT_RECORD, KEY_EVENT, ReadConsoleInputW};

    loop {
        let mut records = [INPUT_RECORD::default()];
        let mut records_read = 0;
        // Safety: both output buffers live for the duration of the call and point to valid writable memory.
        unsafe { ReadConsoleInputW(input, &mut records, &raw mut records_read) }
            .wrap_err("failed to read console input")?;

        if records_read == 0 {
            continue;
        }

        let record = records[0];
        if u32::from(record.EventType) != KEY_EVENT {
            continue;
        }

        // Safety: `EventType == KEY_EVENT`, so reading the `KeyEvent` union field is valid.
        return Ok(unsafe { record.Event.KeyEvent });
    }
}

#[cfg(windows)]
fn should_skip_event(event: &windows::Win32::System::Console::KEY_EVENT_RECORD) -> bool {
    matches!(event.wVirtualKeyCode, 0x10..=0x12)
}

#[cfg(windows)]
fn format_key_event(
    index: usize,
    event: &windows::Win32::System::Console::KEY_EVENT_RECORD,
) -> String {
    use windows::Win32::System::Console::{
        LEFT_ALT_PRESSED, LEFT_CTRL_PRESSED, RIGHT_ALT_PRESSED, RIGHT_CTRL_PRESSED, SHIFT_PRESSED,
    };

    let control_state = event.dwControlKeyState;
    let direction = if event.bKeyDown.as_bool() {
        "DOWN"
    } else {
        "UP"
    };
    // Safety: reading the Unicode union field is valid for console key events.
    let unicode = unsafe { event.uChar.UnicodeChar };

    format!(
        "E{index:02} {direction} VK={:02X} SC={:02X} CH={unicode:04X} CTRL={} LCTRL={} RCTRL={} ALT={} SHIFT={}",
        event.wVirtualKeyCode,
        event.wVirtualScanCode,
        flag(control_state, LEFT_CTRL_PRESSED | RIGHT_CTRL_PRESSED),
        flag(control_state, LEFT_CTRL_PRESSED),
        flag(control_state, RIGHT_CTRL_PRESSED),
        flag(control_state, LEFT_ALT_PRESSED | RIGHT_ALT_PRESSED),
        flag(control_state, SHIFT_PRESSED),
    )
}

#[cfg(windows)]
fn flag(bits: u32, mask: u32) -> u8 {
    u8::from((bits & mask) != 0)
}

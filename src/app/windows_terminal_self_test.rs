use eyre::Context;
use libghostty_vt::key;
use std::io::{self, Write};
use std::thread;
use std::time::{Duration, Instant};
use windows::Win32::System::Console::{
    CONSOLE_MODE, ENABLE_ECHO_INPUT, ENABLE_LINE_INPUT, ENABLE_QUICK_EDIT_MODE,
    ENABLE_WINDOW_INPUT, GetConsoleMode, GetStdHandle, INPUT_RECORD, KEY_EVENT, KEY_EVENT_RECORD,
    LEFT_ALT_PRESSED, LEFT_CTRL_PRESSED, RIGHT_ALT_PRESSED, RIGHT_CTRL_PRESSED, ReadConsoleInputW,
    SHIFT_PRESSED, STD_INPUT_HANDLE, SetConsoleMode,
};

use crate::paths::AppHome;

use super::windows_terminal::{TerminalLayout, TerminalSession};

const SELF_TEST_READY: &str = "KEYBOARD_SELF_TEST_READY";
const SELF_TEST_DONE: &str = "KEYBOARD_SELF_TEST_DONE";
const CROSSTERM_KEY_PROBE_READY: &str = "CROSSTERM_KEY_PROBE_READY";
const RATATUI_KEY_DEBUG_TITLE: &str = "Key Events (Hit Esc 3 times to exit)";
const WINDOWS_KEY_PROBE_READY: &str = "WINDOWS_KEY_PROBE_READY";
const WIN32_INPUT_MODE_UNSUPPORTED: &str = "WIN32_INPUT_MODE_UNSUPPORTED";
const DEFAULT_RATATUI_KEY_DEBUG_PATH: &str =
    "g:\\Programming\\Repos\\ratatui-key-debug\\target\\debug\\ratatui_key_debug.exe";
const WAIT_TIMEOUT: Duration = Duration::from_secs(5);
const PROBE_DETECTION_TIMEOUT: Duration = Duration::from_millis(250);
const POLL_INTERVAL: Duration = Duration::from_millis(20);

pub fn run(app_home: &AppHome, inside: bool) -> eyre::Result<()> {
    if inside {
        run_inside()
    } else {
        run_outside(app_home)
    }
}

fn run_outside(app_home: &AppHome) -> eyre::Result<()> {
    let mut terminal = TerminalSession::new(app_home, None)?;
    terminal.resize(TerminalLayout {
        client_width: 1600,
        client_height: 900,
        cell_width: 8,
        cell_height: 16,
    })?;

    if std::env::var("TEAMY_KEYBOARD_SELF_TEST_CASE").as_deref() == Ok("default-cmd-enter") {
        return run_default_cmd_enter_reproduction(&mut terminal);
    }

    if std::env::var("TEAMY_KEYBOARD_SELF_TEST_CASE").as_deref()
        == Ok("default-cmd-ratatui-key-debug")
    {
        return run_default_cmd_ratatui_key_debug_reproduction(&mut terminal);
    }

    if wait_for_screen(
        &mut terminal,
        CROSSTERM_KEY_PROBE_READY,
        PROBE_DETECTION_TIMEOUT,
    )
    .is_ok()
    {
        return run_crossterm_key_probe_reproduction(&mut terminal);
    }

    if wait_for_screen(
        &mut terminal,
        RATATUI_KEY_DEBUG_TITLE,
        PROBE_DETECTION_TIMEOUT,
    )
    .is_ok()
    {
        return run_ratatui_key_debug_reproduction(&mut terminal);
    }

    if wait_for_screen(
        &mut terminal,
        WINDOWS_KEY_PROBE_READY,
        PROBE_DETECTION_TIMEOUT,
    )
    .is_ok()
    {
        return run_windows_key_probe_reproduction(&mut terminal);
    }

    let _ = terminal.take_input_trace();

    send_keydown(&mut terminal, 0x41, 0x1E, key::Mods::empty())?;
    let a_keydown = terminal.take_input_trace();

    terminal.handle_char(u32::from('a'), char_lparam(0x1E))?;
    let a_char = terminal.take_input_trace();

    send_keyup(&mut terminal, 0x41, 0x1E, key::Mods::empty())?;
    let a_keyup = terminal.take_input_trace();

    let ctrl_mods = key::Mods::CTRL | key::Mods::CTRL_SIDE;
    send_keydown(&mut terminal, 0x11, 0x1D, ctrl_mods)?;
    let ctrl_keydown = terminal.take_input_trace();

    send_keydown(&mut terminal, 0x08, 0x0E, ctrl_mods)?;
    let backspace_keydown = terminal.take_input_trace();

    send_keyup(&mut terminal, 0x08, 0x0E, ctrl_mods)?;
    let backspace_keyup = terminal.take_input_trace();

    send_keyup(&mut terminal, 0x11, 0x1D, ctrl_mods)?;
    let ctrl_keyup = terminal.take_input_trace();

    let transcript = format!(
        "plain_a_keydown: {}\nplain_a_char: {}\nplain_a_keyup: {}\nctrl_keydown: {}\nctrl_backspace_keydown: {}\nctrl_backspace_keyup: {}\nctrl_keyup: {}",
        format_chunks(&a_keydown),
        format_chunks(&a_char),
        format_chunks(&a_keyup),
        format_chunks(&ctrl_keydown),
        format_chunks(&backspace_keydown),
        format_chunks(&backspace_keyup),
        format_chunks(&ctrl_keyup),
    );
    assert_trace(
        &a_keydown,
        &a_char,
        &a_keyup,
        &ctrl_keydown,
        &backspace_keydown,
        &backspace_keyup,
        &ctrl_keyup,
        &transcript,
    )?;

    println!("{transcript}");
    Ok(())
}

fn run_default_cmd_enter_reproduction(terminal: &mut TerminalSession) -> eyre::Result<()> {
    let _ = wait_for_quiet_screen(terminal, Duration::from_millis(150), WAIT_TIMEOUT)?;

    type_text(terminal, "echo hi")?;
    press_enter(terminal)?;

    let screen = wait_for_count(terminal, "hi", 2, WAIT_TIMEOUT)?;
    if screen.matches("hi").count() < 2 {
        eyre::bail!("default cmd Enter did not run `echo hi`\n\n{screen}");
    }

    println!("=== default_cmd_enter ===\n{screen}");
    Ok(())
}

fn run_default_cmd_ratatui_key_debug_reproduction(
    terminal: &mut TerminalSession,
) -> eyre::Result<()> {
    let ratatui_path = std::env::var("TEAMY_KEYBOARD_SELF_TEST_RATATUI_PATH")
        .unwrap_or_else(|_| DEFAULT_RATATUI_KEY_DEBUG_PATH.to_owned());

    let _ = wait_for_quiet_screen(terminal, Duration::from_millis(150), WAIT_TIMEOUT)?;

    type_text(terminal, &ratatui_path)?;
    press_enter(terminal)?;
    let launched_screen = wait_for_screen(terminal, RATATUI_KEY_DEBUG_TITLE, WAIT_TIMEOUT)?;
    if !launched_screen.contains(RATATUI_KEY_DEBUG_TITLE) {
        eyre::bail!("failed to launch ratatui_key_debug.exe from cmd.exe\n\n{launched_screen}");
    }

    let (initial_flags, after_a_release, after_ctrl_backspace_press) =
        exercise_ratatui_key_debug(terminal)?;

    let ctrl_mods = key::Mods::CTRL | key::Mods::CTRL_SIDE;
    send_keyup(terminal, 0x08, 0x0E, ctrl_mods)?;
    send_keyup(terminal, 0x11, 0x1D, ctrl_mods)?;

    for _ in 0..3 {
        send_keydown(terminal, 0x1B, 0x01, key::Mods::empty())?;
        thread::sleep(POLL_INTERVAL);
    }

    let after_ratatui_exit =
        wait_for_quiet_screen(terminal, Duration::from_millis(200), WAIT_TIMEOUT)?;
    type_text(terminal, "exit")?;
    press_enter(terminal)?;
    let final_screen = wait_for_child_exit(terminal, WAIT_TIMEOUT)?;

    let transcript = format!(
        "launched: {}\nkitty_flags: {:?}\n\n=== after_a_release ===\n{}\n\n=== after_ctrl_backspace_press ===\n{}\n\n=== after_ratatui_exit ===\n{}\n\n=== final_screen ===\n{}",
        ratatui_path,
        initial_flags,
        after_a_release,
        after_ctrl_backspace_press,
        after_ratatui_exit,
        final_screen,
    );

    println!("{transcript}");
    Ok(())
}

fn run_ratatui_key_debug_reproduction(terminal: &mut TerminalSession) -> eyre::Result<()> {
    let (initial_flags, after_a_release, after_ctrl_backspace_press) =
        exercise_ratatui_key_debug(terminal)?;

    let ctrl_mods = key::Mods::CTRL | key::Mods::CTRL_SIDE;

    send_keyup(terminal, 0x08, 0x0E, ctrl_mods)?;
    send_keyup(terminal, 0x11, 0x1D, ctrl_mods)?;

    for _ in 0..3 {
        send_keydown(terminal, 0x1B, 0x01, key::Mods::empty())?;
        thread::sleep(POLL_INTERVAL);
    }

    let final_screen = wait_for_child_exit(terminal, WAIT_TIMEOUT)?;
    let transcript = format!(
        "kitty_flags: {:?}\n\n=== after_a_release ===\n{}\n\n=== after_ctrl_backspace_press ===\n{}\n\n=== final_screen ===\n{}",
        initial_flags, after_a_release, after_ctrl_backspace_press, final_screen,
    );

    println!("{transcript}");
    Ok(())
}

fn exercise_ratatui_key_debug(
    terminal: &mut TerminalSession,
) -> eyre::Result<(key::KittyKeyFlags, String, String)> {
    let initial_flags = wait_for_kitty_flags(terminal, Duration::from_millis(250))
        .unwrap_or_else(|_| key::KittyKeyFlags::empty());

    let before = collect_screen(terminal)?;
    if !before.contains(RATATUI_KEY_DEBUG_TITLE) {
        eyre::bail!("ratatui_key_debug title disappeared before reproduction\n\n{before}");
    }

    send_keydown(terminal, 0x41, 0x1E, key::Mods::empty())?;
    let _ = terminal.handle_char(u32::from('a'), char_lparam(0x1E))?;
    let after_a_press = wait_for_screen_line(terminal, "Char('a')", WAIT_TIMEOUT)?;
    let a_press_lines = matching_lines(&after_a_press, "Char('a')");
    if a_press_lines
        .iter()
        .any(|line| line.contains("kind: Release"))
    {
        eyre::bail!("plain A press already produced a release before WM_KEYUP\n\n{after_a_press}");
    }
    if a_press_lines.len() != 1 {
        eyre::bail!(
            "plain A press produced {} matching lines before WM_KEYUP\n\n{after_a_press}",
            a_press_lines.len()
        );
    }

    send_keyup(terminal, 0x41, 0x1E, key::Mods::empty())?;
    let after_a_release = wait_for_count(terminal, "Char('a')", 2, WAIT_TIMEOUT)?;

    let ctrl_mods = key::Mods::CTRL | key::Mods::CTRL_SIDE;
    send_keydown(terminal, 0x11, 0x1D, ctrl_mods)?;
    send_keydown(terminal, 0x08, 0x0E, ctrl_mods)?;
    let _ = terminal.handle_char(0x7F, char_lparam(0x0E))?;
    let after_ctrl_backspace_press = wait_for_screen_line(terminal, "Backspace", WAIT_TIMEOUT)?;
    let backspace_press_lines = matching_lines(&after_ctrl_backspace_press, "Backspace");
    if after_ctrl_backspace_press.contains("Char('h')") {
        eyre::bail!("Ctrl+Backspace reproduced as Ctrl+H\n\n{after_ctrl_backspace_press}");
    }
    if backspace_press_lines.is_empty() {
        eyre::bail!("Ctrl+Backspace did not produce Backspace\n\n{after_ctrl_backspace_press}");
    }
    if backspace_press_lines
        .iter()
        .any(|line| line.contains("kind: Release"))
    {
        eyre::bail!(
            "Ctrl+Backspace press already produced a release before WM_KEYUP\n\n{after_ctrl_backspace_press}"
        );
    }
    if backspace_press_lines.len() != 1 {
        eyre::bail!(
            "Ctrl+Backspace press produced {} matching lines before WM_KEYUP\n\n{after_ctrl_backspace_press}",
            backspace_press_lines.len()
        );
    }

    Ok((initial_flags, after_a_release, after_ctrl_backspace_press))
}

fn run_crossterm_key_probe_reproduction(terminal: &mut TerminalSession) -> eyre::Result<()> {
    let before = collect_screen(terminal)?;
    if !before.contains(CROSSTERM_KEY_PROBE_READY) {
        eyre::bail!("crossterm_key_probe marker disappeared before reproduction\n\n{before}");
    }

    send_keydown(terminal, 0x41, 0x1E, key::Mods::empty())?;
    let _ = terminal.handle_char(u32::from('a'), char_lparam(0x1E))?;
    send_keyup(terminal, 0x41, 0x1E, key::Mods::empty())?;
    let after_a_release = wait_for_count(terminal, "code: Char('a')", 2, WAIT_TIMEOUT)?;
    assert_key_event_count(&after_a_release, "code: Char('a')", 2, "plain A tap")?;

    let ctrl_mods = key::Mods::CTRL | key::Mods::CTRL_SIDE;
    send_keydown(terminal, 0x11, 0x1D, ctrl_mods)?;
    send_keydown(terminal, 0x08, 0x0E, ctrl_mods)?;
    let backspace_char_consumed = terminal.handle_char(0x7F, char_lparam(0x0E))?;
    let after_ctrl_backspace_press =
        wait_for_quiet_screen(terminal, Duration::from_millis(150), WAIT_TIMEOUT)?;
    assert_key_event_count(
        &after_ctrl_backspace_press,
        "code: Backspace",
        1,
        "Ctrl+Backspace press before keyup",
    )?;
    if !backspace_char_consumed {
        eyre::bail!(
            "Ctrl+Backspace WM_CHAR was not consumed after legacy keydown\n\n{after_ctrl_backspace_press}"
        );
    }

    send_keyup(terminal, 0x08, 0x0E, ctrl_mods)?;
    send_keyup(terminal, 0x11, 0x1D, ctrl_mods)?;

    send_keydown(terminal, 0x42, 0x30, key::Mods::empty())?;
    let _ = terminal.handle_char(u32::from('b'), char_lparam(0x30))?;
    send_keyup(terminal, 0x42, 0x30, key::Mods::empty())?;

    let final_screen = wait_for_child_exit(terminal, WAIT_TIMEOUT)?;
    let transcript = format!(
        "=== after_a_release ===\n{}\n\n=== after_ctrl_backspace_press ===\n{}\n\n=== final_screen ===\n{}",
        after_a_release, after_ctrl_backspace_press, final_screen,
    );

    println!("{transcript}");
    Ok(())
}

fn run_windows_key_probe_reproduction(terminal: &mut TerminalSession) -> eyre::Result<()> {
    let before = collect_screen(terminal)?;
    if !before.contains(WINDOWS_KEY_PROBE_READY) {
        eyre::bail!("windows_key_probe marker disappeared before reproduction\n\n{before}");
    }

    wait_for_win32_input_mode(terminal, WAIT_TIMEOUT)?;

    send_keydown(terminal, 0x41, 0x1E, key::Mods::empty())?;
    let _ = terminal.handle_char(u32::from('a'), char_lparam(0x1E))?;
    if wait_for_screen(terminal, "EVENT E01", WAIT_TIMEOUT).is_err() {
        let trace = format_chunks(&terminal.take_input_trace());
        let screen = collect_screen(terminal)?;
        println!("{WIN32_INPUT_MODE_UNSUPPORTED}\ntrace: {trace}\n\n{screen}");
        return Ok(());
    }
    let after_a_press = wait_for_quiet_screen(terminal, Duration::from_millis(150), WAIT_TIMEOUT)?;
    assert_probe_event_state(
        &after_a_press,
        "VK=41 SC=1E",
        "DOWN VK=41 SC=1E CH=0061 CTRL=0",
        "UP VK=41 SC=1E",
        1,
        false,
        "plain A press",
    )?;

    send_keyup(terminal, 0x41, 0x1E, key::Mods::empty())?;
    let after_a_release = wait_for_count(terminal, "VK=41 SC=1E", 2, WAIT_TIMEOUT)?;
    assert_probe_event_state(
        &after_a_release,
        "VK=41 SC=1E",
        "DOWN VK=41 SC=1E CH=0061 CTRL=0",
        "UP VK=41 SC=1E",
        2,
        true,
        "plain A release",
    )?;

    let ctrl_mods = key::Mods::CTRL | key::Mods::CTRL_SIDE;
    send_keydown(terminal, 0x11, 0x1D, ctrl_mods)?;
    send_keydown(terminal, 0x08, 0x0E, ctrl_mods)?;
    let _ = terminal.handle_char(0x7F, char_lparam(0x0E))?;
    let _ = wait_for_screen(terminal, "VK=08 SC=0E", WAIT_TIMEOUT)?;
    let after_ctrl_backspace_press =
        wait_for_quiet_screen(terminal, Duration::from_millis(150), WAIT_TIMEOUT)?;
    assert_probe_event_state_with_press_variants(
        &after_ctrl_backspace_press,
        "VK=08 SC=0E",
        &[
            "DOWN VK=08 SC=0E CH=0008 CTRL=1",
            "DOWN VK=08 SC=0E CH=007F CTRL=1",
        ],
        "UP VK=08 SC=0E",
        1,
        false,
        "Ctrl+Backspace press",
    )?;

    send_keyup(terminal, 0x08, 0x0E, ctrl_mods)?;
    send_keyup(terminal, 0x11, 0x1D, ctrl_mods)?;
    let after_ctrl_backspace_release = wait_for_count(terminal, "VK=08 SC=0E", 2, WAIT_TIMEOUT)?;
    assert_probe_event_state_with_press_variants(
        &after_ctrl_backspace_release,
        "VK=08 SC=0E",
        &[
            "DOWN VK=08 SC=0E CH=0008 CTRL=1",
            "DOWN VK=08 SC=0E CH=007F CTRL=1",
        ],
        "UP VK=08 SC=0E",
        2,
        true,
        "Ctrl+Backspace release",
    )?;

    let transcript = format!(
        "=== after_a_press ===\n{}\n\n=== after_a_release ===\n{}\n\n=== after_ctrl_backspace_press ===\n{}\n\n=== after_ctrl_backspace_release ===\n{}",
        after_a_press, after_a_release, after_ctrl_backspace_press, after_ctrl_backspace_release,
    );
    println!("{transcript}");
    Ok(())
}

fn run_inside() -> eyre::Result<()> {
    let input =
        unsafe { GetStdHandle(STD_INPUT_HANDLE) }.wrap_err("failed to get console input handle")?;
    let mut original_mode = CONSOLE_MODE(0);
    unsafe { GetConsoleMode(input, &mut original_mode) }.wrap_err("failed to read console mode")?;

    let raw_mode = (original_mode | ENABLE_WINDOW_INPUT)
        & !(ENABLE_LINE_INPUT | ENABLE_ECHO_INPUT | ENABLE_QUICK_EDIT_MODE);
    unsafe { SetConsoleMode(input, raw_mode) }
        .wrap_err("failed to enable keyboard self-test console mode")?;
    let _mode_guard = ConsoleModeGuard {
        input,
        original_mode,
    };

    println!("{SELF_TEST_READY}");
    io::stdout()
        .flush()
        .wrap_err("failed to flush self-test ready banner")?;

    let mut index = 0_usize;
    loop {
        let event = read_next_key_event(input)?;
        index += 1;
        println!("{}", format_key_event(index, &event));
        io::stdout()
            .flush()
            .wrap_err("failed to flush self-test key event")?;

        if event.bKeyDown.as_bool() && event.wVirtualKeyCode == 0x7B {
            println!("{SELF_TEST_DONE}");
            io::stdout()
                .flush()
                .wrap_err("failed to flush self-test completion banner")?;
            break;
        }
    }

    Ok(())
}

struct ConsoleModeGuard {
    input: windows::Win32::Foundation::HANDLE,
    original_mode: CONSOLE_MODE,
}

impl Drop for ConsoleModeGuard {
    fn drop(&mut self) {
        let _ = unsafe { SetConsoleMode(self.input, self.original_mode) };
    }
}

fn read_next_key_event(
    input: windows::Win32::Foundation::HANDLE,
) -> eyre::Result<KEY_EVENT_RECORD> {
    loop {
        let mut records = [INPUT_RECORD::default()];
        let mut records_read = 0;
        unsafe { ReadConsoleInputW(input, &mut records, &mut records_read) }
            .wrap_err("failed to read console input")?;

        if records_read == 0 {
            continue;
        }

        let record = records[0];
        if u32::from(record.EventType) != KEY_EVENT {
            continue;
        }

        return Ok(unsafe { record.Event.KeyEvent });
    }
}

fn format_key_event(index: usize, event: &KEY_EVENT_RECORD) -> String {
    let control_state = event.dwControlKeyState;
    let direction = if event.bKeyDown.as_bool() {
        "DOWN"
    } else {
        "UP"
    };
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

fn flag(bits: u32, mask: u32) -> u8 {
    u8::from((bits & mask) != 0)
}

fn send_keydown(
    terminal: &mut TerminalSession,
    vkey: u32,
    scancode: u8,
    mods: key::Mods,
) -> eyre::Result<()> {
    terminal.handle_key_event(vkey, key_lparam(scancode, false, false), false, false, mods)?;
    Ok(())
}

fn send_keyup(
    terminal: &mut TerminalSession,
    vkey: u32,
    scancode: u8,
    mods: key::Mods,
) -> eyre::Result<()> {
    terminal.handle_key_event(vkey, key_lparam(scancode, false, true), false, true, mods)?;
    Ok(())
}

fn press_enter(terminal: &mut TerminalSession) -> eyre::Result<()> {
    send_keydown(terminal, 0x0D, 0x1C, key::Mods::empty())?;
    let _ = terminal.handle_char(u32::from('\r'), char_lparam(0x1C))?;
    send_keyup(terminal, 0x0D, 0x1C, key::Mods::empty())?;
    Ok(())
}

fn type_text(terminal: &mut TerminalSession, text: &str) -> eyre::Result<()> {
    for character in text.chars() {
        send_text_character(terminal, character)?;
    }
    Ok(())
}

fn send_text_character(terminal: &mut TerminalSession, character: char) -> eyre::Result<()> {
    let Some((vkey, scancode, mods)) = text_character_key(character) else {
        let code_unit = u32::from(character);
        let _ = terminal.handle_char(code_unit, 0)?;
        return Ok(());
    };

    send_keydown(terminal, vkey, scancode, mods)?;
    let _ = terminal.handle_char(u32::from(character), char_lparam(scancode))?;
    send_keyup(terminal, vkey, scancode, mods)?;
    Ok(())
}

fn text_character_key(character: char) -> Option<(u32, u8, key::Mods)> {
    let shift = key::Mods::SHIFT | key::Mods::SHIFT_SIDE;
    let alpha = match character {
        'a'..='z' => Some((
            character.to_ascii_uppercase() as u32,
            letter_scancode(character),
            key::Mods::empty(),
        )),
        'A'..='Z' => {
            let lower = character.to_ascii_lowercase();
            Some((
                lower.to_ascii_uppercase() as u32,
                letter_scancode(lower),
                shift,
            ))
        }
        '0' => Some((0x30, 0x0B, key::Mods::empty())),
        '1' => Some((0x31, 0x02, key::Mods::empty())),
        '2' => Some((0x32, 0x03, key::Mods::empty())),
        '3' => Some((0x33, 0x04, key::Mods::empty())),
        '4' => Some((0x34, 0x05, key::Mods::empty())),
        '5' => Some((0x35, 0x06, key::Mods::empty())),
        '6' => Some((0x36, 0x07, key::Mods::empty())),
        '7' => Some((0x37, 0x08, key::Mods::empty())),
        '8' => Some((0x38, 0x09, key::Mods::empty())),
        '9' => Some((0x39, 0x0A, key::Mods::empty())),
        ' ' => Some((0x20, 0x39, key::Mods::empty())),
        ':' => Some((0xBA, 0x27, shift)),
        '\\' => Some((0xDC, 0x2B, key::Mods::empty())),
        '-' => Some((0xBD, 0x0C, key::Mods::empty())),
        '_' => Some((0xBD, 0x0C, shift)),
        '.' => Some((0xBE, 0x34, key::Mods::empty())),
        '/' => Some((0xBF, 0x35, key::Mods::empty())),
        _ => None,
    };

    alpha
}

fn letter_scancode(character: char) -> u8 {
    match character {
        'a' => 0x1E,
        'b' => 0x30,
        'c' => 0x2E,
        'd' => 0x20,
        'e' => 0x12,
        'f' => 0x21,
        'g' => 0x22,
        'h' => 0x23,
        'i' => 0x17,
        'j' => 0x24,
        'k' => 0x25,
        'l' => 0x26,
        'm' => 0x32,
        'n' => 0x31,
        'o' => 0x18,
        'p' => 0x19,
        'q' => 0x10,
        'r' => 0x13,
        's' => 0x1F,
        't' => 0x14,
        'u' => 0x16,
        'v' => 0x2F,
        'w' => 0x11,
        'x' => 0x2D,
        'y' => 0x15,
        'z' => 0x2C,
        _ => 0,
    }
}

fn key_lparam(scancode: u8, extended: bool, previous_state: bool) -> isize {
    let mut value = 1_isize | (isize::from(scancode) << 16);
    if extended {
        value |= 1_isize << 24;
    }
    if previous_state {
        value |= 1_isize << 30;
        value |= 1_isize << 31;
    }
    value
}

fn char_lparam(scancode: u8) -> isize {
    1_isize | (isize::from(scancode) << 16)
}

fn format_chunks(chunks: &[Vec<u8>]) -> String {
    if chunks.is_empty() {
        return "<none>".to_owned();
    }

    chunks
        .iter()
        .map(|chunk| {
            if chunk.is_empty() {
                "<empty>".to_owned()
            } else {
                chunk
                    .iter()
                    .map(|byte| format!("{byte:02X}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            }
        })
        .collect::<Vec<_>>()
        .join(" | ")
}

fn assert_trace(
    a_keydown: &[Vec<u8>],
    a_char: &[Vec<u8>],
    a_keyup: &[Vec<u8>],
    ctrl_keydown: &[Vec<u8>],
    backspace_keydown: &[Vec<u8>],
    backspace_keyup: &[Vec<u8>],
    ctrl_keyup: &[Vec<u8>],
    transcript: &str,
) -> eyre::Result<()> {
    if !a_keydown.is_empty() {
        eyre::bail!("plain A keydown unexpectedly wrote PTY input\n\n{transcript}");
    }

    if a_char != [b"a".to_vec()] {
        eyre::bail!("plain A text routing changed unexpectedly\n\n{transcript}");
    }

    if !a_keyup.is_empty() {
        eyre::bail!("plain A keyup unexpectedly wrote PTY input\n\n{transcript}");
    }

    if !ctrl_keydown.is_empty() {
        eyre::bail!("Ctrl keydown unexpectedly wrote PTY input\n\n{transcript}");
    }

    if backspace_keydown != [vec![0x7F]] {
        eyre::bail!("Ctrl+Backspace keydown should produce DEL (0x7F)\n\n{transcript}");
    }

    if !backspace_keyup.is_empty() {
        eyre::bail!("Ctrl+Backspace keyup unexpectedly wrote PTY input\n\n{transcript}");
    }

    if !ctrl_keyup.is_empty() {
        eyre::bail!("Ctrl keyup unexpectedly wrote PTY input\n\n{transcript}");
    }

    Ok(())
}

fn wait_for_kitty_flags(
    terminal: &mut TerminalSession,
    timeout: Duration,
) -> eyre::Result<key::KittyKeyFlags> {
    let started = Instant::now();
    loop {
        let _ = terminal.pump()?;
        let flags = terminal.current_kitty_keyboard_flags()?;
        if !flags.is_empty() {
            return Ok(flags);
        }
        if started.elapsed() >= timeout {
            let screen = collect_screen(terminal)?;
            eyre::bail!("timed out waiting for kitty keyboard flags\n\n{screen}");
        }
        thread::sleep(POLL_INTERVAL);
    }
}

fn wait_for_screen(
    terminal: &mut TerminalSession,
    needle: &str,
    timeout: Duration,
) -> eyre::Result<String> {
    let started = Instant::now();
    loop {
        let screen = collect_screen(terminal)?;
        if screen.contains(needle) {
            return Ok(screen);
        }
        if started.elapsed() >= timeout {
            eyre::bail!("timed out waiting for `{needle}`\n\n{screen}");
        }
        thread::sleep(POLL_INTERVAL);
    }
}

fn wait_for_screen_line(
    terminal: &mut TerminalSession,
    needle: &str,
    timeout: Duration,
) -> eyre::Result<String> {
    wait_for_screen(terminal, needle, timeout)
}

fn wait_for_count(
    terminal: &mut TerminalSession,
    needle: &str,
    expected: usize,
    timeout: Duration,
) -> eyre::Result<String> {
    let started = Instant::now();
    loop {
        let screen = collect_screen(terminal)?;
        if screen.matches(needle).count() >= expected {
            return Ok(screen);
        }
        if started.elapsed() >= timeout {
            eyre::bail!("timed out waiting for {expected} matches of `{needle}`\n\n{screen}");
        }
        thread::sleep(POLL_INTERVAL);
    }
}

fn wait_for_quiet_screen(
    terminal: &mut TerminalSession,
    quiet_period: Duration,
    timeout: Duration,
) -> eyre::Result<String> {
    let started = Instant::now();
    let mut last_screen = String::new();
    let mut last_change = Instant::now();

    loop {
        let screen = collect_screen(terminal)?;
        if screen != last_screen {
            last_screen = screen;
            last_change = Instant::now();
        }

        if !last_screen.is_empty() && last_change.elapsed() >= quiet_period {
            return Ok(last_screen);
        }

        if started.elapsed() >= timeout {
            eyre::bail!("timed out waiting for quiet screen\n\n{last_screen}");
        }
        thread::sleep(POLL_INTERVAL);
    }
}

fn wait_for_win32_input_mode(
    terminal: &mut TerminalSession,
    timeout: Duration,
) -> eyre::Result<()> {
    let started = Instant::now();
    loop {
        let _ = terminal.pump()?;
        if terminal.win32_input_mode_enabled() {
            return Ok(());
        }
        if started.elapsed() >= timeout {
            let screen = terminal.visible_text()?;
            eyre::bail!("timed out waiting for ConPTY win32-input-mode\n\n{screen}");
        }
        thread::sleep(POLL_INTERVAL);
    }
}

fn assert_probe_event_state(
    screen: &str,
    event_prefix: &str,
    expected_press: &str,
    expected_release: &str,
    expected_count: usize,
    require_release: bool,
    label: &str,
) -> eyre::Result<()> {
    let count = screen.matches(event_prefix).count();
    if count != expected_count {
        eyre::bail!(
            "{label} expected {expected_count} matching events but saw {count}\n\n{screen}"
        );
    }

    if !screen.contains(expected_press) {
        eyre::bail!("{label} did not contain the expected press event\n\n{screen}");
    }

    let has_release = screen.contains(expected_release);
    if require_release && !has_release {
        eyre::bail!("{label} expected a release event\n\n{screen}");
    }
    if !require_release && has_release {
        eyre::bail!("{label} unexpectedly contained a release event before keyup\n\n{screen}");
    }

    Ok(())
}

fn assert_probe_event_state_with_press_variants(
    screen: &str,
    event_prefix: &str,
    expected_press_variants: &[&str],
    expected_release: &str,
    expected_count: usize,
    require_release: bool,
    label: &str,
) -> eyre::Result<()> {
    let count = screen.matches(event_prefix).count();
    if count != expected_count {
        eyre::bail!(
            "{label} expected {expected_count} matching events but saw {count}\n\n{screen}"
        );
    }

    if !expected_press_variants
        .iter()
        .any(|expected_press| screen.contains(expected_press))
    {
        eyre::bail!("{label} did not contain any expected press event variant\n\n{screen}");
    }

    let has_release = screen.contains(expected_release);
    if require_release && !has_release {
        eyre::bail!("{label} expected a release event\n\n{screen}");
    }
    if !require_release && has_release {
        eyre::bail!("{label} unexpectedly contained a release event before keyup\n\n{screen}");
    }

    Ok(())
}

fn assert_key_event_count(
    screen: &str,
    needle: &str,
    expected_count: usize,
    label: &str,
) -> eyre::Result<()> {
    let count = screen.matches(needle).count();
    if count != expected_count {
        eyre::bail!(
            "{label} expected {expected_count} matching key events but saw {count}\n\n{screen}"
        );
    }

    Ok(())
}

fn matching_lines(screen: &str, needle: &str) -> Vec<String> {
    screen
        .lines()
        .filter(|line| line.contains(needle))
        .map(std::borrow::ToOwned::to_owned)
        .collect()
}

fn wait_for_child_exit(terminal: &mut TerminalSession, timeout: Duration) -> eyre::Result<String> {
    let started = Instant::now();
    loop {
        let result = terminal.pump()?;
        let screen = terminal.visible_text()?;
        if result.should_close {
            return Ok(screen);
        }
        if started.elapsed() >= timeout {
            eyre::bail!("timed out waiting for child exit\n\n{screen}");
        }
        thread::sleep(POLL_INTERVAL);
    }
}

fn collect_screen(terminal: &mut TerminalSession) -> eyre::Result<String> {
    let _ = terminal.pump()?;
    terminal.visible_text()
}

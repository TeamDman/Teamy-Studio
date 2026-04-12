use eyre::Context;
use libghostty_vt::key;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
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
const CTRL_D_EXIT_COMMAND: &[u8] = b"exit\r";
const PWSH_CTRL_D_AT_PROMPT_CASE: &str = "pwsh-ctrl-d-at-prompt";
const PWSH_NESTED_CTRL_D_CASE: &str = "pwsh-nested-ctrl-d";
const PWSH_CTRL_D_AFTER_TYPED_INPUT_CASE: &str = "pwsh-ctrl-d-after-typed-input";
const PWSH_CTRL_L_REDRAW_CASE: &str = "pwsh-ctrl-l-redraw";
const PWSH_TYPED_INPUT_SCROLLS_CARET_INTO_VIEW_CASE: &str =
    "pwsh-typed-input-scrolls-caret-into-view";
const PWSH_NOPROFILE_RESIZE_RESTORES_PROMPT_CASE: &str = "pwsh-noprofile-resize-restores-prompt";
const RESIZE_RESTORE_PROMPT_TOP_MARKER: &str = "TEAMY_RESIZE_TOP";
const RESIZE_RESTORE_PROMPT_BOTTOM_MARKER: &str = "TEAMY_RESIZE_BOTTOM>";
const WAIT_TIMEOUT: Duration = Duration::from_secs(5);
const PROBE_DETECTION_TIMEOUT: Duration = Duration::from_millis(250);
const POLL_INTERVAL: Duration = Duration::from_millis(20);
const MAX_CTRL_L_RESPONSE_LATENCY_MS: f64 = 1000.0;
const MAX_CTRL_L_PRESENT_LATENCY_MS: f64 = 1000.0;

pub fn run(
    app_home: &AppHome,
    inside: bool,
    scenario: Option<&str>,
    artifact_output: Option<&Path>,
    vt_engine: super::VtEngineChoice,
) -> eyre::Result<()> {
    if inside {
        run_inside()
    } else {
        run_outside(app_home, scenario, artifact_output, vt_engine)
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "the outer keyboard self-test harness keeps scenario setup and probe detection in one place"
)]
fn run_outside(
    app_home: &AppHome,
    scenario: Option<&str>,
    artifact_output: Option<&Path>,
    vt_engine: super::VtEngineChoice,
) -> eyre::Result<()> {
    let scenario = scenario
        .map(std::borrow::ToOwned::to_owned)
        .or_else(|| std::env::var("TEAMY_KEYBOARD_SELF_TEST_CASE").ok());

    let mut terminal = match scenario.as_deref() {
        Some(
            PWSH_CTRL_D_AT_PROMPT_CASE
            | PWSH_NESTED_CTRL_D_CASE
            | PWSH_CTRL_D_AFTER_TYPED_INPUT_CASE,
        ) => {
            let command = crate::shell_default::command_builder_from_argv(&[
                "pwsh.exe".to_owned(),
                "-NoLogo".to_owned(),
            ])?;
            TerminalSession::new_with_command(command, vt_engine)?
        }
        Some(PWSH_NOPROFILE_RESIZE_RESTORES_PROMPT_CASE) => {
            let command = crate::shell_default::command_builder_from_argv(&[
                "pwsh.exe".to_owned(),
                "-NoLogo".to_owned(),
                "-NoProfile".to_owned(),
            ])?;
            TerminalSession::new_with_command(command, vt_engine)?
        }
        Some(PWSH_CTRL_L_REDRAW_CASE | PWSH_TYPED_INPUT_SCROLLS_CARET_INTO_VIEW_CASE) => {
            let command = crate::shell_default::command_builder_from_argv(&[
                "pwsh.exe".to_owned(),
                "-NoLogo".to_owned(),
                "-NoExit".to_owned(),
                "-Command".to_owned(),
                "1..80 | ForEach-Object { 'teamy' }".to_owned(),
            ])?;
            TerminalSession::new_with_command(command, vt_engine)?
        }
        _ => TerminalSession::new(app_home, None, vt_engine)?,
    };
    terminal.resize(TerminalLayout {
        client_width: 1600,
        client_height: 900,
        cell_width: 8,
        cell_height: 16,
    })?;

    if let Some(result) =
        try_run_named_scenario(&mut terminal, scenario.as_deref(), artifact_output)?
    {
        return Ok(result);
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
    assert_trace(&TraceSnapshot {
        a_keydown: &a_keydown,
        a_char: &a_char,
        a_keyup: &a_keyup,
        ctrl_keydown: &ctrl_keydown,
        backspace_keydown: &backspace_keydown,
        backspace_keyup: &backspace_keyup,
        ctrl_keyup: &ctrl_keyup,
        transcript: &transcript,
    })?;

    emit_transcript(&transcript, artifact_output)
}

fn try_run_named_scenario(
    terminal: &mut TerminalSession,
    scenario: Option<&str>,
    artifact_output: Option<&Path>,
) -> eyre::Result<Option<()>> {
    let Some(scenario) = scenario else {
        return Ok(None);
    };

    let result = match scenario {
        "default-cmd-enter" => run_default_cmd_enter_reproduction(terminal, artifact_output),
        "default-cmd-ratatui-key-debug" => {
            run_default_cmd_ratatui_key_debug_reproduction(terminal, artifact_output)
        }
        PWSH_CTRL_D_AT_PROMPT_CASE => {
            run_pwsh_ctrl_d_at_prompt_reproduction(terminal, artifact_output)
        }
        PWSH_NESTED_CTRL_D_CASE => run_pwsh_nested_ctrl_d_reproduction(terminal, artifact_output),
        PWSH_CTRL_D_AFTER_TYPED_INPUT_CASE => {
            run_pwsh_ctrl_d_after_typed_input_reproduction(terminal, artifact_output)
        }
        PWSH_CTRL_L_REDRAW_CASE => run_pwsh_ctrl_l_redraw_reproduction(terminal, artifact_output),
        PWSH_TYPED_INPUT_SCROLLS_CARET_INTO_VIEW_CASE => {
            run_pwsh_typed_input_scrolls_caret_into_view_reproduction(terminal, artifact_output)
        }
        PWSH_NOPROFILE_RESIZE_RESTORES_PROMPT_CASE => {
            run_pwsh_noprofile_resize_restores_prompt_reproduction(terminal, artifact_output)
        }
        _ => return Ok(None),
    };

    result.map(Some)
}

fn run_default_cmd_enter_reproduction(
    terminal: &mut TerminalSession,
    artifact_output: Option<&Path>,
) -> eyre::Result<()> {
    let _ = wait_for_quiet_screen(terminal, Duration::from_millis(150), WAIT_TIMEOUT)?;

    type_text(terminal, "echo hi")?;
    press_enter(terminal)?;

    let screen = wait_for_count(terminal, "hi", 2, WAIT_TIMEOUT)?;
    if screen.matches("hi").count() < 2 {
        eyre::bail!("default cmd Enter did not run `echo hi`\n\n{screen}");
    }

    emit_transcript(
        &format!("=== default_cmd_enter ===\n{screen}"),
        artifact_output,
    )
}

fn run_default_cmd_ratatui_key_debug_reproduction(
    terminal: &mut TerminalSession,
    artifact_output: Option<&Path>,
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
        "launched: {ratatui_path}\nkitty_flags: {initial_flags:?}\n\n=== after_a_release ===\n{after_a_release}\n\n=== after_ctrl_backspace_press ===\n{after_ctrl_backspace_press}\n\n=== after_ratatui_exit ===\n{after_ratatui_exit}\n\n=== final_screen ===\n{final_screen}"
    );

    emit_transcript(&transcript, artifact_output)
}

fn run_pwsh_ctrl_d_at_prompt_reproduction(
    terminal: &mut TerminalSession,
    artifact_output: Option<&Path>,
) -> eyre::Result<()> {
    let initial_screen = wait_for_semantic_prompt(terminal, WAIT_TIMEOUT)?;
    let _ = terminal.take_input_trace();

    press_ctrl_d(terminal)?;
    let input_trace = terminal.take_input_trace();
    let final_screen = wait_for_child_exit(terminal, WAIT_TIMEOUT)?;
    let (markers_observed, at_shell_prompt, awaiting_input) = terminal.semantic_prompt_state();

    if final_screen.contains("warning(stream): invalid C0 character, ignoring: 0x4") {
        eyre::bail!(
            "Ctrl+D at the prompt still leaked 0x04 into Ghostty\n\n=== initial_screen ===\n{initial_screen}\n\n=== input_trace ===\n{}\n\n=== final_screen ===\n{final_screen}",
            format_chunks(&input_trace),
        );
    }

    let transcript = format!(
        "scenario: {PWSH_CTRL_D_AT_PROMPT_CASE}\nmarkers_observed: {markers_observed}\nat_shell_prompt: {at_shell_prompt}\nawaiting_input: {awaiting_input}\n\n=== initial_screen ===\n{initial_screen}\n\n=== input_trace ===\n{}\n\n=== final_screen ===\n{final_screen}",
        format_chunks(&input_trace),
    );

    emit_transcript(&transcript, artifact_output)
}

fn run_pwsh_nested_ctrl_d_reproduction(
    terminal: &mut TerminalSession,
    artifact_output: Option<&Path>,
) -> eyre::Result<()> {
    let initial_screen = wait_for_semantic_prompt(terminal, WAIT_TIMEOUT)?;

    type_text(terminal, "pwsh")?;
    press_enter(terminal)?;
    let nested_screen = wait_for_quiet_screen(terminal, Duration::from_millis(300), WAIT_TIMEOUT)?;

    press_ctrl_d(terminal)?;
    let after_ctrl_d = assert_stays_open(terminal, Duration::from_millis(600))?;

    clear_echoed_ctrl_d_marker(terminal)?;

    for _ in 0..2 {
        type_text(terminal, "exit")?;
        press_enter(terminal)?;
        if terminal.pump()?.should_close {
            break;
        }
        let _ = wait_for_quiet_screen(terminal, Duration::from_millis(150), WAIT_TIMEOUT)?;
    }

    let final_screen = wait_for_child_exit(terminal, WAIT_TIMEOUT)?;
    let transcript = format!(
        "scenario: {PWSH_NESTED_CTRL_D_CASE}\n\n=== initial_screen ===\n{initial_screen}\n\n=== nested_screen ===\n{nested_screen}\n\n=== after_ctrl_d ===\n{after_ctrl_d}\n\n=== final_screen ===\n{final_screen}"
    );

    emit_transcript(&transcript, artifact_output)
}

fn run_pwsh_ctrl_d_after_typed_input_reproduction(
    terminal: &mut TerminalSession,
    artifact_output: Option<&Path>,
) -> eyre::Result<()> {
    let initial_screen = wait_for_semantic_prompt(terminal, WAIT_TIMEOUT)?;
    let _ = terminal.take_input_trace();

    send_text_character(terminal, 'a')?;
    press_backspace(terminal)?;
    let _ = wait_for_quiet_screen(terminal, Duration::from_millis(150), WAIT_TIMEOUT)?;
    let _ = terminal.take_input_trace();

    press_ctrl_d(terminal)?;
    let input_trace = terminal.take_input_trace();
    let after_ctrl_d = assert_stays_open(terminal, Duration::from_millis(600))?;

    if input_trace.iter().any(|chunk| chunk == CTRL_D_EXIT_COMMAND) {
        eyre::bail!(
            "Ctrl+D after prior prompt input should not translate to exit\\r\n\n=== initial_screen ===\n{initial_screen}\n\n=== input_trace ===\n{}\n\n=== after_ctrl_d ===\n{after_ctrl_d}",
            format_chunks(&input_trace),
        );
    }

    if input_trace.is_empty() {
        eyre::bail!(
            "Ctrl+D after prior prompt input should produce PTY input without translating to exit\n\n=== initial_screen ===\n{initial_screen}\n\n=== input_trace ===\n{}\n\n=== after_ctrl_d ===\n{after_ctrl_d}",
            format_chunks(&input_trace),
        );
    }

    clear_echoed_ctrl_d_marker(terminal)?;
    type_text(terminal, "exit")?;
    press_enter(terminal)?;
    let final_screen = wait_for_child_exit(terminal, WAIT_TIMEOUT)?;
    let transcript = format!(
        "scenario: {PWSH_CTRL_D_AFTER_TYPED_INPUT_CASE}\n\n=== initial_screen ===\n{initial_screen}\n\n=== input_trace ===\n{}\n\n=== after_ctrl_d ===\n{after_ctrl_d}\n\n=== final_screen ===\n{final_screen}",
        format_chunks(&input_trace),
    );

    emit_transcript(&transcript, artifact_output)
}

fn run_pwsh_ctrl_l_redraw_reproduction(
    terminal: &mut TerminalSession,
    artifact_output: Option<&Path>,
) -> eyre::Result<()> {
    const CLEAR_ME_MARKER: &str = "TEAMY_CTRL_L_CLEAR_ME";

    let initial_screen = wait_for_pwsh_visible_prompt(terminal)?;
    let _ = terminal.take_input_trace();
    seed_pwsh_scrollback_for_input_jump(terminal)?;

    type_text(terminal, &format!("Write-Host {CLEAR_ME_MARKER}"))?;
    press_enter(terminal)?;
    let _ = wait_for_screen(terminal, CLEAR_ME_MARKER, WAIT_TIMEOUT)?;
    let _ = wait_for_pwsh_visible_prompt(terminal)?;

    type_text(terminal, "echo hello")?;
    let _ = wait_for_quiet_screen(terminal, Duration::from_millis(150), WAIT_TIMEOUT)?;
    let scrolled_offset = scroll_terminal_viewport_up_for_input_jump(terminal)?;
    let before_ctrl_l = wait_for_quiet_screen(terminal, Duration::from_millis(150), WAIT_TIMEOUT)?;
    let latency_before_ctrl_l = terminal.performance_snapshot()?;
    let _ = terminal.take_input_trace();

    press_ctrl_l(terminal)?;
    let input_trace = terminal.take_input_trace();
    let after_ctrl_l = wait_for_quiet_screen(terminal, Duration::from_millis(250), WAIT_TIMEOUT)?;
    let latency_after_ctrl_l = terminal.performance_snapshot()?;
    let viewport_after_ctrl_l = terminal.viewport_metrics()?;
    let (markers_observed, at_shell_prompt, awaiting_input) = terminal.semantic_prompt_state();

    assert_prompt_markers_do_not_leak_to_screen("initial_screen", &initial_screen)?;
    assert_prompt_markers_do_not_leak_to_screen("before_ctrl_l", &before_ctrl_l)?;
    assert_prompt_markers_do_not_leak_to_screen("after_ctrl_l", &after_ctrl_l)?;

    let ctrl_l_response_latency_ms = delta_latency_ms(
        latency_before_ctrl_l.total_input_response_latency_us,
        latency_after_ctrl_l.total_input_response_latency_us,
        latency_before_ctrl_l.input_response_latency_observations,
        latency_after_ctrl_l.input_response_latency_observations,
    )
    .ok_or_else(|| {
        eyre::eyre!(
            "Ctrl+L redraw did not record any input-response latency sample\n\n=== before_ctrl_l ===\n{before_ctrl_l}\n\n=== after_ctrl_l ===\n{after_ctrl_l}"
        )
    })?;
    let ctrl_l_present_latency_ms = delta_latency_ms(
        latency_before_ctrl_l.total_input_present_latency_us,
        latency_after_ctrl_l.total_input_present_latency_us,
        latency_before_ctrl_l.input_present_latency_observations,
        latency_after_ctrl_l.input_present_latency_observations,
    )
    .ok_or_else(|| {
        eyre::eyre!(
            "Ctrl+L redraw did not record any input-present latency sample\n\n=== before_ctrl_l ===\n{before_ctrl_l}\n\n=== after_ctrl_l ===\n{after_ctrl_l}"
        )
    })?;

    if input_trace.is_empty() {
        eyre::bail!(
            "Ctrl+L should produce PTY input for pwsh redraw investigation\n\n=== initial_screen ===\n{initial_screen}\n\n=== before_ctrl_l ===\n{before_ctrl_l}\n\n=== after_ctrl_l ===\n{after_ctrl_l}"
        );
    }

    if after_ctrl_l.contains(CLEAR_ME_MARKER) {
        eyre::bail!(
            "Ctrl+L should clear the previously printed marker from the visible screen\n\n=== before_ctrl_l ===\n{before_ctrl_l}\n\n=== after_ctrl_l ===\n{after_ctrl_l}"
        );
    }

    if ctrl_l_response_latency_ms > MAX_CTRL_L_RESPONSE_LATENCY_MS {
        eyre::bail!(
            "Ctrl+L response latency exceeded threshold\nresponse_latency_ms: {ctrl_l_response_latency_ms:.3}\nthreshold_ms: {MAX_CTRL_L_RESPONSE_LATENCY_MS:.3}\n\n=== input_trace ===\n{}\n\n=== after_ctrl_l ===\n{after_ctrl_l}",
            format_chunks(&input_trace),
        );
    }

    if ctrl_l_present_latency_ms > MAX_CTRL_L_PRESENT_LATENCY_MS {
        eyre::bail!(
            "Ctrl+L present latency exceeded threshold\npresent_latency_ms: {ctrl_l_present_latency_ms:.3}\nthreshold_ms: {MAX_CTRL_L_PRESENT_LATENCY_MS:.3}\n\n=== input_trace ===\n{}\n\n=== after_ctrl_l ===\n{after_ctrl_l}",
            format_chunks(&input_trace),
        );
    }

    let max_offset_after_ctrl_l = viewport_after_ctrl_l
        .total
        .saturating_sub(viewport_after_ctrl_l.visible);
    if viewport_after_ctrl_l.offset != max_offset_after_ctrl_l {
        eyre::bail!(
            "Ctrl+L should jump the viewport back to the active caret\nscrolled_offset: {scrolled_offset}\nviewport_after_ctrl_l: {:?}\n\n=== before_ctrl_l ===\n{before_ctrl_l}\n\n=== after_ctrl_l ===\n{after_ctrl_l}",
            viewport_after_ctrl_l,
        );
    }

    let transcript = format!(
        "scenario: {PWSH_CTRL_L_REDRAW_CASE}\nmarkers_observed: {markers_observed}\nat_shell_prompt: {at_shell_prompt}\nawaiting_input: {awaiting_input}\nscrolled_offset: {scrolled_offset}\nviewport_after_ctrl_l: {:?}\nctrl_l_response_latency_ms: {ctrl_l_response_latency_ms:.3}\nctrl_l_present_latency_ms: {ctrl_l_present_latency_ms:.3}\n\n=== initial_screen ===\n{initial_screen}\n\n=== before_ctrl_l ===\n{before_ctrl_l}\n\n=== input_trace ===\n{}\n\n=== after_ctrl_l ===\n{after_ctrl_l}",
        viewport_after_ctrl_l,
        format_chunks(&input_trace),
    );

    emit_transcript(&transcript, artifact_output)
}

fn run_pwsh_typed_input_scrolls_caret_into_view_reproduction(
    terminal: &mut TerminalSession,
    artifact_output: Option<&Path>,
) -> eyre::Result<()> {
    let initial_screen = wait_for_pwsh_visible_prompt(terminal)?;
    seed_pwsh_scrollback_for_input_jump(terminal)?;
    let scrolled_offset = scroll_terminal_viewport_up_for_input_jump(terminal)?;
    let before_input = wait_for_quiet_screen(terminal, Duration::from_millis(150), WAIT_TIMEOUT)?;
    let _ = terminal.take_input_trace();

    send_text_character(terminal, 'a')?;
    let input_trace = terminal.take_input_trace();
    let after_input = wait_for_quiet_screen(terminal, Duration::from_millis(150), WAIT_TIMEOUT)?;
    let viewport_after_input = terminal.viewport_metrics()?;

    if !after_input.contains('a') {
        eyre::bail!(
            "Typing while scrolled up should bring the edited prompt back into view\n\n=== initial_screen ===\n{initial_screen}\n\n=== before_input ===\n{before_input}\n\n=== after_input ===\n{after_input}"
        );
    }

    let max_offset_after_input = viewport_after_input
        .total
        .saturating_sub(viewport_after_input.visible);
    if viewport_after_input.offset != max_offset_after_input {
        eyre::bail!(
            "Typing while scrolled up should jump the viewport back to the active caret\nscrolled_offset: {scrolled_offset}\nviewport_after_input: {:?}\n\n=== before_input ===\n{before_input}\n\n=== after_input ===\n{after_input}",
            viewport_after_input,
        );
    }

    press_backspace(terminal)?;
    let _ = wait_for_quiet_screen(terminal, Duration::from_millis(100), WAIT_TIMEOUT)?;
    type_text(terminal, "exit")?;
    press_enter(terminal)?;
    let final_screen = wait_for_child_exit(terminal, WAIT_TIMEOUT)?;

    let transcript = format!(
        "scenario: {PWSH_TYPED_INPUT_SCROLLS_CARET_INTO_VIEW_CASE}\nscrolled_offset: {scrolled_offset}\nviewport_after_input: {:?}\n\n=== initial_screen ===\n{initial_screen}\n\n=== before_input ===\n{before_input}\n\n=== input_trace ===\n{}\n\n=== after_input ===\n{after_input}\n\n=== final_screen ===\n{final_screen}",
        viewport_after_input,
        format_chunks(&input_trace),
    );

    emit_transcript(&transcript, artifact_output)
}

fn run_pwsh_noprofile_resize_restores_prompt_reproduction(
    terminal: &mut TerminalSession,
    artifact_output: Option<&Path>,
) -> eyre::Result<()> {
    let initial_shell_prompt = wait_for_semantic_prompt(terminal, WAIT_TIMEOUT)?;

    terminal.handle_paste(&format!(
        "function global:prompt {{ \"{RESIZE_RESTORE_PROMPT_TOP_MARKER}`n{RESIZE_RESTORE_PROMPT_BOTTOM_MARKER} \" }}"
    ))?;
    press_enter(terminal)?;

    let custom_prompt_screen =
        wait_for_screen(terminal, RESIZE_RESTORE_PROMPT_BOTTOM_MARKER, WAIT_TIMEOUT)?;
    if !custom_prompt_screen.contains(RESIZE_RESTORE_PROMPT_TOP_MARKER) {
        eyre::bail!(
            "custom multiline prompt did not render both lines before resize\n\n=== initial_shell_prompt ===\n{initial_shell_prompt}\n\n=== custom_prompt_screen ===\n{custom_prompt_screen}"
        );
    }

    shrink_terminal_until_one_row_visible(terminal)?;
    let after_shrink =
        wait_for_screen(terminal, RESIZE_RESTORE_PROMPT_BOTTOM_MARKER, WAIT_TIMEOUT)?;

    terminal.resize(TerminalLayout {
        client_width: 1600,
        client_height: 900,
        cell_width: 8,
        cell_height: 16,
    })?;

    let after_restore =
        wait_for_screen(terminal, RESIZE_RESTORE_PROMPT_BOTTOM_MARKER, WAIT_TIMEOUT)?;
    if !after_restore.contains(RESIZE_RESTORE_PROMPT_TOP_MARKER) {
        eyre::bail!(
            "restoring the terminal height should recover the hidden prompt line from scrollback\n\n=== initial_shell_prompt ===\n{initial_shell_prompt}\n\n=== custom_prompt_screen ===\n{custom_prompt_screen}\n\n=== after_shrink ===\n{after_shrink}\n\n=== after_restore ===\n{after_restore}"
        );
    }

    let transcript = format!(
        "scenario: {PWSH_NOPROFILE_RESIZE_RESTORES_PROMPT_CASE}\n\n=== initial_shell_prompt ===\n{initial_shell_prompt}\n\n=== custom_prompt_screen ===\n{custom_prompt_screen}\n\n=== after_shrink ===\n{after_shrink}\n\n=== after_restore ===\n{after_restore}"
    );
    emit_transcript(&transcript, artifact_output)
}

fn seed_pwsh_scrollback_for_input_jump(terminal: &mut TerminalSession) -> eyre::Result<()> {
    terminal.resize(TerminalLayout {
        client_width: 1600,
        client_height: 240,
        cell_width: 8,
        cell_height: 16,
    })?;
    let _ = wait_for_pwsh_visible_prompt(terminal)?;

    Ok(())
}

fn shrink_terminal_until_one_row_visible(terminal: &mut TerminalSession) -> eyre::Result<()> {
    for client_height in [96, 88, 80, 72, 64, 56, 48] {
        terminal.resize(TerminalLayout {
            client_width: 1600,
            client_height,
            cell_width: 8,
            cell_height: 16,
        })?;
        let viewport = terminal.viewport_metrics()?;
        if viewport.visible == 1 {
            return Ok(());
        }
    }

    eyre::bail!(
        "failed to shrink the terminal to a single visible row\n\nviewport: {:?}",
        terminal.viewport_metrics()?,
    )
}

fn wait_for_pwsh_visible_prompt(terminal: &mut TerminalSession) -> eyre::Result<String> {
    wait_for_screen(terminal, "❯", WAIT_TIMEOUT)
}

fn scroll_terminal_viewport_up_for_input_jump(terminal: &mut TerminalSession) -> eyre::Result<u64> {
    let viewport = terminal.viewport_metrics()?;
    let max_offset = viewport.total.saturating_sub(viewport.visible);
    if max_offset == 0 {
        eyre::bail!("expected scrollback before exercising input-jump behavior: {viewport:?}");
    }

    let target_offset = max_offset.saturating_sub(10).min(max_offset - 1);
    terminal.scroll_viewport_to_offset(target_offset)?;
    let viewport_after_scroll = terminal.viewport_metrics()?;
    if viewport_after_scroll.offset != target_offset {
        eyre::bail!(
            "failed to scroll viewport away from the active caret\nrequested_offset: {target_offset}\nviewport_after_scroll: {:?}",
            viewport_after_scroll,
        );
    }

    Ok(target_offset)
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
        "kitty_flags: {initial_flags:?}\n\n=== after_a_release ===\n{after_a_release}\n\n=== after_ctrl_backspace_press ===\n{after_ctrl_backspace_press}\n\n=== final_screen ===\n{final_screen}"
    );

    println!("{transcript}");
    Ok(())
}

fn emit_transcript(transcript: &str, artifact_output: Option<&Path>) -> eyre::Result<()> {
    println!("{transcript}");

    if let Some(artifact_output) = artifact_output {
        if let Some(parent) = artifact_output.parent() {
            fs::create_dir_all(parent).wrap_err_with(|| {
                format!(
                    "failed to create keyboard self-test artifact directory {}",
                    parent.display()
                )
            })?;
        }

        fs::write(artifact_output, transcript).wrap_err_with(|| {
            format!(
                "failed to write keyboard self-test artifact {}",
                artifact_output.display()
            )
        })?;
    }

    Ok(())
}

fn assert_prompt_markers_do_not_leak_to_screen(label: &str, screen: &str) -> eyre::Result<()> {
    if screen.contains("133;") {
        eyre::bail!(
            "{label} leaked raw OSC 133 prompt markers into the visible screen\n\n=== {label} ===\n{screen}"
        );
    }

    Ok(())
}

fn delta_latency_ms(
    total_before_us: u64,
    total_after_us: u64,
    observations_before: u64,
    observations_after: u64,
) -> Option<f64> {
    let delta_observations = observations_after.checked_sub(observations_before)?;
    if delta_observations == 0 {
        return None;
    }

    let delta_total_us = total_after_us.checked_sub(total_before_us)?;
    Some(u64_to_f64(delta_total_us) / u64_to_f64(delta_observations) / 1000.0)
}

fn u64_to_f64(value: u64) -> f64 {
    const TWO_POW_32: f64 = 4_294_967_296.0;

    let upper = u32::try_from(value >> 32).unwrap_or(u32::MAX);
    let lower = u32::try_from(value & u64::from(u32::MAX)).unwrap_or(u32::MAX);
    f64::from(upper) * TWO_POW_32 + f64::from(lower)
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
        "=== after_a_release ===\n{after_a_release}\n\n=== after_ctrl_backspace_press ===\n{after_ctrl_backspace_press}\n\n=== final_screen ===\n{final_screen}"
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
        "=== after_a_press ===\n{after_a_press}\n\n=== after_a_release ===\n{after_a_release}\n\n=== after_ctrl_backspace_press ===\n{after_ctrl_backspace_press}\n\n=== after_ctrl_backspace_release ===\n{after_ctrl_backspace_release}"
    );
    println!("{transcript}");
    Ok(())
}

fn run_inside() -> eyre::Result<()> {
    // Safety: querying the standard input handle does not require additional invariants.
    let input =
        unsafe { GetStdHandle(STD_INPUT_HANDLE) }.wrap_err("failed to get console input handle")?;
    let mut original_mode = CONSOLE_MODE(0);
    // Safety: `original_mode` is a valid out-pointer for `GetConsoleMode`.
    unsafe { GetConsoleMode(input, &raw mut original_mode) }
        .wrap_err("failed to read console mode")?;

    let raw_mode = (original_mode | ENABLE_WINDOW_INPUT)
        & !(ENABLE_LINE_INPUT | ENABLE_ECHO_INPUT | ENABLE_QUICK_EDIT_MODE);
    // Safety: `input` is a valid console handle and `raw_mode` is derived from the current mode bits.
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

struct TraceSnapshot<'a> {
    a_keydown: &'a [Vec<u8>],
    a_char: &'a [Vec<u8>],
    a_keyup: &'a [Vec<u8>],
    ctrl_keydown: &'a [Vec<u8>],
    backspace_keydown: &'a [Vec<u8>],
    backspace_keyup: &'a [Vec<u8>],
    ctrl_keyup: &'a [Vec<u8>],
    transcript: &'a str,
}

impl Drop for ConsoleModeGuard {
    fn drop(&mut self) {
        // Safety: this restores the saved console mode on the same input handle.
        let _ = unsafe { SetConsoleMode(self.input, self.original_mode) };
    }
}

fn read_next_key_event(
    input: windows::Win32::Foundation::HANDLE,
) -> eyre::Result<KEY_EVENT_RECORD> {
    loop {
        let mut records = [INPUT_RECORD::default()];
        let mut records_read = 0;
        // Safety: both output buffers are valid and writable for the duration of the call.
        unsafe { ReadConsoleInputW(input, &mut records, &raw mut records_read) }
            .wrap_err("failed to read console input")?;

        if records_read == 0 {
            continue;
        }

        let record = records[0];
        if u32::from(record.EventType) != KEY_EVENT {
            continue;
        }

        // Safety: `EventType == KEY_EVENT`, so accessing the `KeyEvent` union field is valid.
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

fn press_ctrl_d(terminal: &mut TerminalSession) -> eyre::Result<()> {
    let ctrl_mods = key::Mods::CTRL | key::Mods::CTRL_SIDE;
    send_keydown(terminal, 0x11, 0x1D, ctrl_mods)?;
    send_keydown(terminal, 0x44, 0x20, ctrl_mods)?;
    let _ = terminal.handle_char(0x04, char_lparam(0x20))?;
    send_keyup(terminal, 0x44, 0x20, ctrl_mods)?;
    send_keyup(terminal, 0x11, 0x1D, ctrl_mods)?;
    Ok(())
}

fn press_ctrl_l(terminal: &mut TerminalSession) -> eyre::Result<()> {
    let ctrl_mods = key::Mods::CTRL | key::Mods::CTRL_SIDE;
    send_keydown(terminal, 0x11, 0x1D, ctrl_mods)?;
    send_keydown(terminal, 0x4C, 0x26, ctrl_mods)?;
    let _ = terminal.handle_char(0x0C, char_lparam(0x26))?;
    send_keyup(terminal, 0x4C, 0x26, ctrl_mods)?;
    send_keyup(terminal, 0x11, 0x1D, ctrl_mods)?;
    Ok(())
}

fn clear_echoed_ctrl_d_marker(terminal: &mut TerminalSession) -> eyre::Result<()> {
    press_backspace(terminal)?;
    let _ = wait_for_quiet_screen(terminal, Duration::from_millis(150), WAIT_TIMEOUT)?;
    Ok(())
}

fn press_backspace(terminal: &mut TerminalSession) -> eyre::Result<()> {
    send_keydown(terminal, 0x08, 0x0E, key::Mods::empty())?;
    let _ = terminal.handle_char(u32::from('\u{8}'), char_lparam(0x0E))?;
    send_keyup(terminal, 0x08, 0x0E, key::Mods::empty())?;
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
    match character {
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
    }
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

fn assert_trace(snapshot: &TraceSnapshot<'_>) -> eyre::Result<()> {
    if !snapshot.a_keydown.is_empty() {
        eyre::bail!(
            "plain A keydown unexpectedly wrote PTY input\n\n{}",
            snapshot.transcript
        );
    }

    if snapshot.a_char != [b"a".to_vec()] {
        eyre::bail!(
            "plain A text routing changed unexpectedly\n\n{}",
            snapshot.transcript
        );
    }

    if !snapshot.a_keyup.is_empty() {
        eyre::bail!(
            "plain A keyup unexpectedly wrote PTY input\n\n{}",
            snapshot.transcript
        );
    }

    if !snapshot.ctrl_keydown.is_empty() {
        eyre::bail!(
            "Ctrl keydown unexpectedly wrote PTY input\n\n{}",
            snapshot.transcript
        );
    }

    if snapshot.backspace_keydown != [vec![0x7F]] {
        eyre::bail!(
            "Ctrl+Backspace keydown should produce DEL (0x7F)\n\n{}",
            snapshot.transcript
        );
    }

    if !snapshot.backspace_keyup.is_empty() {
        eyre::bail!(
            "Ctrl+Backspace keyup unexpectedly wrote PTY input\n\n{}",
            snapshot.transcript
        );
    }

    if !snapshot.ctrl_keyup.is_empty() {
        eyre::bail!(
            "Ctrl keyup unexpectedly wrote PTY input\n\n{}",
            snapshot.transcript
        );
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

fn wait_for_semantic_prompt(
    terminal: &mut TerminalSession,
    timeout: Duration,
) -> eyre::Result<String> {
    let started = Instant::now();
    loop {
        let screen = collect_screen(terminal)?;
        let (markers_observed, at_shell_prompt, awaiting_input) = terminal.semantic_prompt_state();
        if markers_observed && at_shell_prompt && awaiting_input {
            return Ok(screen);
        }
        if started.elapsed() >= timeout {
            eyre::bail!(
                "timed out waiting for semantic shell prompt markers\n\nmarkers_observed: {markers_observed}\nat_shell_prompt: {at_shell_prompt}\nawaiting_input: {awaiting_input}\n\n{screen}"
            );
        }
        thread::sleep(POLL_INTERVAL);
    }
}

fn assert_stays_open(terminal: &mut TerminalSession, duration: Duration) -> eyre::Result<String> {
    let started = Instant::now();
    let mut last_screen = String::new();
    while started.elapsed() < duration {
        let result = terminal.pump()?;
        last_screen = terminal.visible_text()?;
        if result.should_close {
            eyre::bail!("terminal closed unexpectedly\n\n{last_screen}");
        }
        thread::sleep(POLL_INTERVAL);
    }

    Ok(last_screen)
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
            let screen = collect_screen(terminal)?;
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
    let screen = terminal.visible_text()?;
    terminal.note_frame_presented();
    Ok(screen)
}

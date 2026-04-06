#![cfg(windows)]

use std::process::Command;

fn run_keyboard_self_test(probe_path: &str) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_teamy-studio"))
        .env("COMSPEC", probe_path)
        .env("TEAMY_KEY_PROBE_EVENT_LIMIT", "8")
    .env_remove("TEAMY_KEYBOARD_SELF_TEST_CASE")
    .env_remove("TEAMY_KEYBOARD_SELF_TEST_RATATUI_PATH")
        .args(["self-test", "keyboard-input"])
        .output()
        .expect("keyboard self-test should launch")
}

fn run_crossterm_keyboard_self_test() -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_teamy-studio"))
        .env("COMSPEC", env!("CARGO_BIN_EXE_crossterm_key_probe"))
        .env("TEAMY_KEY_PROBE_EVENT_LIMIT", "6")
    .env_remove("TEAMY_KEYBOARD_SELF_TEST_CASE")
    .env_remove("TEAMY_KEYBOARD_SELF_TEST_RATATUI_PATH")
        .args(["self-test", "keyboard-input"])
        .output()
        .expect("crossterm keyboard self-test should launch")
}

fn run_default_cmd_keyboard_self_test() -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_teamy-studio"))
        .env("COMSPEC", "cmd.exe")
        .env("TEAMY_KEYBOARD_SELF_TEST_CASE", "default-cmd-enter")
        .env_remove("TEAMY_KEYBOARD_SELF_TEST_RATATUI_PATH")
        .args(["self-test", "keyboard-input"])
        .output()
        .expect("default cmd keyboard self-test should launch")
}

fn run_default_cmd_ratatui_keyboard_self_test() -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_teamy-studio"))
        .env("COMSPEC", "cmd.exe")
        .env("TEAMY_KEYBOARD_SELF_TEST_CASE", "default-cmd-ratatui-key-debug")
        .env(
            "TEAMY_KEYBOARD_SELF_TEST_RATATUI_PATH",
            "g:\\Programming\\Repos\\ratatui-key-debug\\target\\debug\\ratatui_key_debug.exe",
        )
        .args(["self-test", "keyboard-input"])
        .output()
        .expect("default cmd ratatui keyboard self-test should launch")
}

#[test]
fn test_issue_keyboard_input_reduced_probe_press_release_ordering() {
    let output = run_keyboard_self_test(env!("CARGO_BIN_EXE_windows_key_probe"));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "expected reduced probe self-test to succeed\nstdout:\n{stdout}\n\nstderr:\n{stderr}"
    );
}

#[test]
fn test_issue_keyboard_input_reduced_probe_ctrl_backspace_modifier_and_release() {
    let output = run_keyboard_self_test(env!("CARGO_BIN_EXE_windows_key_probe"));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "expected reduced probe Ctrl+Backspace self-test to succeed\nstdout:\n{stdout}\n\nstderr:\n{stderr}"
    );
}

#[test]
fn test_issue_keyboard_input_crossterm_probe_ctrl_backspace_is_not_double_injected() {
    let output = run_crossterm_keyboard_self_test();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "expected crossterm probe Ctrl+Backspace self-test to succeed\nstdout:\n{stdout}\n\nstderr:\n{stderr}"
    );
}

#[test]
fn test_issue_keyboard_input_default_cmd_enter_runs_command() {
    let output = run_default_cmd_keyboard_self_test();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "expected default cmd Enter self-test to succeed\nstdout:\n{stdout}\n\nstderr:\n{stderr}"
    );
}

#[test]
fn test_issue_keyboard_input_default_cmd_ratatui_key_debug_reproduction() {
    let output = run_default_cmd_ratatui_keyboard_self_test();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "expected default cmd ratatui self-test to succeed\nstdout:\n{stdout}\n\nstderr:\n{stderr}"
    );
}
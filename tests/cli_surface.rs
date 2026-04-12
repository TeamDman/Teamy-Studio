#![cfg(windows)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

fn run_teamy_studio(args: &[&str], envs: &[(&str, &str)]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_teamy-studio"));
    command.args(args);
    for (key, value) in envs {
        command.env(key, value);
    }
    command
        .output()
        .expect("teamy-studio command should launch")
}

fn output_text(output: &Output) -> String {
    let mut text = String::new();
    text.push_str(&String::from_utf8_lossy(&output.stdout));
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    text
}

struct TempDirGuard {
    path: PathBuf,
}

impl TempDirGuard {
    fn new(prefix: &str) -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("{prefix}-{unique}"));
        std::fs::create_dir_all(&path).expect("temporary directory should be created");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

// tool[verify cli.version.includes-semver]
// tool[verify cli.version.includes-git-revision]
#[test]
fn test_version_includes_semver_and_git_revision() {
    let output = run_teamy_studio(&["--version"], &[]);
    let text = output_text(&output);

    assert!(output.status.success(), "version command failed:\n{text}");
    assert!(
        text.contains(env!("CARGO_PKG_VERSION")),
        "missing semver:\n{text}"
    );
    assert!(
        text.contains("(rev "),
        "missing git revision marker:\n{text}"
    );
}

// tool[verify cli.help.describes-behavior]
// tool[verify cli.help.describes-terminal]
// tool[verify cli.help.describes-self-test]
// tool[verify cli.help.describes-argv]
// tool[verify cli.help.describes-environment]
// tool[verify cli.global.debug]
// tool[verify cli.global.log-filter]
// tool[verify cli.global.log-file]
// tool[verify cli.surface.terminal]
// tool[verify cli.surface.self-test]
#[test]
fn test_root_help_describes_commands_args_and_environment() {
    let output = run_teamy_studio(&["--help"], &[]);
    let text = output_text(&output);

    assert!(output.status.success(), "help command failed:\n{text}");
    assert!(
        text.contains("terminal"),
        "missing terminal command in help:\n{text}"
    );
    assert!(
        text.contains("self-test"),
        "missing self-test command in help:\n{text}"
    );
    assert!(
        !text.contains("\n    workspace\n"),
        "workspace command should not appear in help:\n{text}"
    );
    assert!(
        !text.contains("\n    window\n"),
        "window command should not appear in help:\n{text}"
    );
    assert!(text.contains("--debug"), "missing --debug in help:\n{text}");
    assert!(
        text.contains("--log-filter"),
        "missing --log-filter in help:\n{text}"
    );
    assert!(
        text.contains("--log-file"),
        "missing --log-file in help:\n{text}"
    );
    assert!(
        text.contains("TEAMY_STUDIO_HOME_DIR"),
        "missing TEAMY_STUDIO_HOME_DIR in help:\n{text}"
    );
    assert!(
        text.contains("TEAMY_STUDIO_CACHE_DIR"),
        "missing TEAMY_STUDIO_CACHE_DIR in help:\n{text}"
    );
    assert!(
        text.contains("RUST_LOG"),
        "missing RUST_LOG in help:\n{text}"
    );
}

// tool[verify cli.help.position-independent]
// cli[verify command.surface.terminal]
// cli[verify command.surface.terminal-default-shell]
// cli[verify command.surface.terminal-list]
// cli[verify command.surface.terminal-open]
#[test]
fn test_terminal_help_is_available() {
    let output = run_teamy_studio(&["terminal", "--help"], &[]);
    let text = output_text(&output);

    assert!(output.status.success(), "terminal help failed:\n{text}");
    assert!(
        text.contains("default-shell"),
        "missing default-shell subcommand in help:\n{text}"
    );
    assert!(
        text.contains("list"),
        "missing list subcommand in help:\n{text}"
    );
    assert!(
        text.contains("open"),
        "missing open subcommand in help:\n{text}"
    );
    assert!(
        !text.contains("attach"),
        "attach subcommand should not appear in help:\n{text}"
    );
    assert!(
        !text.contains("create"),
        "create subcommand should not appear in help:\n{text}"
    );
    assert!(
        !text.contains("show-window"),
        "show-window subcommand should not appear in help:\n{text}"
    );
}

// cli[verify terminal.open.program-positional]
// cli[verify terminal.open.double-dash-trailing-args]
// cli[verify terminal.open.stdin-flag]
// cli[verify terminal.open.title-flag]
// cli[verify terminal.open.vt-engine-flag]
#[test]
fn test_terminal_open_help_is_available() {
    let output = run_teamy_studio(&["terminal", "open", "--help"], &[]);
    let text = output_text(&output);

    assert!(
        output.status.success(),
        "terminal open help failed:\n{text}"
    );
    assert!(
        text.contains("Program to launch"),
        "missing positional program description:\n{text}"
    );
    assert!(text.contains("--stdin"), "missing --stdin in help:\n{text}");
    assert!(text.contains("--title"), "missing --title in help:\n{text}");
    assert!(
        text.contains("--vt-engine"),
        "missing --vt-engine in help:\n{text}"
    );
    assert!(
        text.contains("ghostty"),
        "missing ghostty choice in help:\n{text}"
    );
    assert!(
        text.contains("teamy"),
        "missing teamy choice in help:\n{text}"
    );
}

// cli[verify terminal.list.enumerates-live-windows]
// cli[verify terminal.list.prints-hwnd-pid-and-title]
#[test]
fn test_terminal_list_help_and_command_succeed() {
    let help_output = run_teamy_studio(&["terminal", "list", "--help"], &[]);
    let help_text = output_text(&help_output);
    assert!(
        help_output.status.success(),
        "terminal list help failed:\n{help_text}"
    );

    let output = run_teamy_studio(&["terminal", "list"], &[]);
    let text = output_text(&output);
    assert!(output.status.success(), "terminal list failed:\n{text}");
}

// cli[verify command.surface.terminal-default-shell]
// cli[verify command.surface.terminal-default-shell-set]
// cli[verify command.surface.terminal-default-shell-show]
#[test]
fn test_terminal_default_shell_help_is_available() {
    let output = run_teamy_studio(&["terminal", "default-shell", "--help"], &[]);
    let text = output_text(&output);

    assert!(
        output.status.success(),
        "terminal default-shell help failed:\n{text}"
    );
    assert!(
        text.contains("set"),
        "missing set subcommand in help:\n{text}"
    );
    assert!(
        text.contains("show"),
        "missing show subcommand in help:\n{text}"
    );
}

// cli[verify shell.default.set.double-dash-trailing-args]
// cli[verify shell.default.show-effective]
// cli[verify shell.default.persisted-in-app-home]
#[test]
fn test_terminal_default_shell_set_and_show_roundtrip() {
    let app_home = TempDirGuard::new("teamy-studio-cli-app-home");
    let app_home_value = app_home.path().to_string_lossy().into_owned();
    let envs = [("TEAMY_STUDIO_HOME_DIR", app_home_value.as_str())];

    let set_output = run_teamy_studio(
        &[
            "terminal",
            "default-shell",
            "set",
            "pwsh",
            "--",
            "-NoProfile",
        ],
        &envs,
    );
    let set_text = output_text(&set_output);
    assert!(
        set_output.status.success(),
        "terminal default-shell set failed:\n{set_text}"
    );

    let show_output = run_teamy_studio(&["terminal", "default-shell", "show"], &envs);
    let show_text = output_text(&show_output);
    assert!(
        show_output.status.success(),
        "terminal default-shell show failed:\n{show_text}"
    );
    assert!(
        show_text.contains("pwsh"),
        "missing program in show output:\n{show_text}"
    );
    assert!(
        show_text.contains("-NoProfile"),
        "missing trailing argument in show output:\n{show_text}"
    );
}

#[test]
fn test_shell_surface_is_removed() {
    let output = run_teamy_studio(&["shell"], &[]);
    let text = output_text(&output);

    assert!(
        !output.status.success(),
        "shell invocation unexpectedly succeeded:\n{text}"
    );
    assert!(
        text.contains("unexpected argument: shell"),
        "removed shell command should be rejected explicitly:\n{text}"
    );
}

// cli[verify command.surface.self-test]
// cli[verify command.surface.self-test-keyboard-input]
// cli[verify command.surface.self-test-terminal-throughput]
// cli[verify command.surface.self-test-terminal-replay]
// cli[verify command.surface.self-test-render-offscreen]
#[test]
fn test_self_test_help_is_available() {
    let output = run_teamy_studio(&["self-test", "--help"], &[]);
    let text = output_text(&output);

    assert!(output.status.success(), "self-test help failed:\n{text}");
    assert!(
        text.contains("keyboard-input"),
        "missing keyboard-input subcommand in help:\n{text}"
    );
    assert!(
        text.contains("terminal-throughput"),
        "missing terminal-throughput subcommand in help:\n{text}"
    );
    assert!(
        text.contains("terminal-replay"),
        "missing terminal-replay subcommand in help:\n{text}"
    );
    assert!(
        text.contains("render-offscreen"),
        "missing render-offscreen subcommand in help:\n{text}"
    );
}

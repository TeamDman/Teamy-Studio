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
// tool[verify cli.help.describes-shell]
// tool[verify cli.help.describes-self-test]
// tool[verify cli.help.describes-argv]
// tool[verify cli.help.describes-environment]
// tool[verify cli.global.debug]
// tool[verify cli.global.log-filter]
// tool[verify cli.global.log-file]
// tool[verify cli.surface.window]
// tool[verify cli.surface.shell]
// tool[verify cli.surface.self-test]
#[test]
fn test_root_help_describes_commands_args_and_environment() {
    let output = run_teamy_studio(&["--help"], &[]);
    let text = output_text(&output);

    assert!(output.status.success(), "help command failed:\n{text}");
    assert!(
        text.contains("window"),
        "missing window command in help:\n{text}"
    );
    assert!(
        text.contains("shell"),
        "missing shell command in help:\n{text}"
    );
    assert!(
        text.contains("self-test"),
        "missing self-test command in help:\n{text}"
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
// cli[verify command.surface.shell-default]
// cli[verify command.surface.shell-default-set]
// cli[verify command.surface.shell-default-show]
#[test]
fn test_nested_shell_default_help_is_available() {
    let output = run_teamy_studio(&["shell", "default", "--help"], &[]);
    let text = output_text(&output);

    assert!(output.status.success(), "nested shell help failed:\n{text}");
    assert!(
        text.contains("set"),
        "missing set subcommand in help:\n{text}"
    );
    assert!(
        text.contains("show"),
        "missing show subcommand in help:\n{text}"
    );
}

// tool[verify cli.help.position-independent]
// cli[verify command.surface.self-test-keyboard-input]
// cli[verify self-test.keyboard-input.inside-flag]
#[test]
fn test_keyboard_input_help_shows_inside_flag() {
    let output = run_teamy_studio(&["self-test", "keyboard-input", "--help"], &[]);
    let text = output_text(&output);

    assert!(
        output.status.success(),
        "keyboard-input help failed:\n{text}"
    );
    assert!(
        text.contains("--inside"),
        "missing --inside in help:\n{text}"
    );
}

// cli[verify command.surface.shell-default-set]
// cli[verify command.surface.shell-default-show]
// cli[verify shell.default.persisted-in-app-home]
// cli[verify shell.default.show-effective]
// cli[verify shell.default.set.double-dash-trailing-args]
// cli[verify path.app-home.env-overrides-platform]
#[test]
fn test_shell_default_set_and_show_roundtrip_with_app_home_override() {
    let app_home = TempDirGuard::new("teamy-studio-cli-shell-home");
    let app_home_str = app_home.path().to_string_lossy().into_owned();

    let set_output = run_teamy_studio(
        &["shell", "default", "set", "--", "pwsh.exe", "-NoLogo"],
        &[("TEAMY_STUDIO_HOME_DIR", &app_home_str)],
    );
    let set_text = output_text(&set_output);
    assert!(
        set_output.status.success(),
        "shell default set failed:\n{set_text}"
    );

    let config_path = app_home.path().join("default-shell.txt");
    assert!(config_path.exists(), "default shell file was not created");

    let show_output = run_teamy_studio(
        &["shell", "default", "show"],
        &[("TEAMY_STUDIO_HOME_DIR", &app_home_str)],
    );
    let show_text = output_text(&show_output);
    assert!(
        show_output.status.success(),
        "shell default show failed:\n{show_text}"
    );
    assert!(
        show_text.contains("pwsh.exe -NoLogo"),
        "unexpected show output:\n{show_text}"
    );
}

// cli[verify shell.default.fallback.builtin]
// cli[verify shell.default.fallback.windows-comspec]
#[test]
fn test_shell_default_show_uses_builtin_fallback_when_unset() {
    let app_home = TempDirGuard::new("teamy-studio-cli-shell-fallback");
    let app_home_str = app_home.path().to_string_lossy().into_owned();

    let output = run_teamy_studio(
        &["shell", "default", "show"],
        &[
            ("TEAMY_STUDIO_HOME_DIR", &app_home_str),
            ("COMSPEC", "fallback-shell.exe"),
        ],
    );
    let text = output_text(&output);

    assert!(output.status.success(), "fallback show failed:\n{text}");
    assert!(
        text.contains("fallback-shell.exe /D"),
        "unexpected fallback output:\n{text}"
    );
}

// cli[verify shell.inline.launches-configured-default]
// cli[verify command.surface.shell]
#[test]
fn test_shell_runs_configured_default_inline() {
    let app_home = TempDirGuard::new("teamy-studio-cli-inline-shell");
    let app_home_str = app_home.path().to_string_lossy().into_owned();

    let set_output = run_teamy_studio(
        &["shell", "default", "set", "cmd.exe", "/C", "exit", "0"],
        &[("TEAMY_STUDIO_HOME_DIR", &app_home_str)],
    );
    let set_text = output_text(&set_output);
    assert!(
        set_output.status.success(),
        "inline shell setup failed:\n{set_text}"
    );

    let run_output = run_teamy_studio(&["shell"], &[("TEAMY_STUDIO_HOME_DIR", &app_home_str)]);
    let run_text = output_text(&run_output);
    assert!(
        run_output.status.success(),
        "inline shell failed:\n{run_text}"
    );
}

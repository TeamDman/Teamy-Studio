#[cfg(windows)]
use std::ffi::OsString;
use std::fs;
use std::io::ErrorKind;
#[cfg(windows)]
use std::path::Path;
use std::path::PathBuf;

use eyre::{Context, ensure};

use crate::paths::AppHome;

#[cfg(windows)]
use portable_pty::CommandBuilder;

const DEFAULT_SHELL_FILENAME: &str = "default-shell.txt";

#[must_use]
/// cli[impl shell.default.persisted-in-app-home]
pub fn default_shell_path(app_home: &AppHome) -> PathBuf {
    app_home.file_path(DEFAULT_SHELL_FILENAME)
}

/// cli[impl shell.default.fallback.builtin]
/// cli[impl shell.default.fallback.windows-comspec]
///
/// # Errors
///
/// This function will return an error if the configured shell cannot be read.
pub fn load_effective_argv(app_home: &AppHome) -> eyre::Result<Vec<String>> {
    Ok(load_configured_argv(app_home)?.unwrap_or_else(builtin_default_argv))
}

/// cli[impl shell.default.persisted-in-app-home]
///
/// # Errors
///
/// This function will return an error if the configured shell file cannot be read.
pub fn load_configured_argv(app_home: &AppHome) -> eyre::Result<Option<Vec<String>>> {
    let path = default_shell_path(app_home);
    match fs::read_to_string(&path) {
        Ok(contents) => {
            let argv = parse_shell_file(&contents);
            if argv.is_empty() {
                Ok(None)
            } else {
                Ok(Some(argv))
            }
        }
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).wrap_err_with(|| {
            format!(
                "failed to read default shell config from {}",
                path.display()
            )
        }),
    }
}

/// cli[impl shell.default.persisted-in-app-home]
///
/// # Errors
///
/// This function will return an error if the shell arguments are invalid or the configuration file cannot be written.
pub fn save_configured_argv(
    app_home: &AppHome,
    program: String,
    args: Vec<String>,
) -> eyre::Result<()> {
    ensure!(
        !program.contains(['\r', '\n']),
        "shell program cannot contain newlines"
    );
    ensure!(!program.is_empty(), "shell program cannot be empty");

    for argument in &args {
        ensure!(
            !argument.contains(['\r', '\n']),
            "shell arguments cannot contain newlines"
        );
    }

    let mut command_argv = Vec::with_capacity(args.len() + 1);
    command_argv.push(program);
    command_argv.extend(args);

    app_home.ensure_dir()?;
    let path = default_shell_path(app_home);
    fs::write(&path, serialize_shell_file(&command_argv))
        .wrap_err_with(|| format!("failed to write default shell config to {}", path.display()))?;
    Ok(())
}

#[cfg(windows)]
/// Build a Windows PTY command for the configured default shell.
///
/// # Errors
///
/// This function will return an error if the effective shell argv cannot be resolved.
pub fn load_effective_command_builder(app_home: &AppHome) -> eyre::Result<CommandBuilder> {
    command_builder_from_argv(&load_effective_argv(app_home)?)
}

#[cfg(windows)]
/// Build a Windows PTY command from a pre-resolved argv list.
///
/// # Errors
///
/// This function will return an error if the argv list is empty.
pub fn command_builder_from_argv(command_argv: &[String]) -> eyre::Result<CommandBuilder> {
    let (program, args) = command_argv
        .split_first()
        .ok_or_else(|| eyre::eyre!("default shell command cannot be empty"))?;
    let resolved_program = resolve_windows_program(program);
    let mut command = CommandBuilder::new(resolved_program);
    for argument in args {
        command.arg(argument);
    }
    Ok(command)
}

#[cfg(windows)]
/// cli[impl shell.default.windows-launch-resolves-program-on-path]
fn resolve_windows_program(program: &str) -> PathBuf {
    resolve_windows_program_on_path(program).unwrap_or_else(|| PathBuf::from(program))
}

#[cfg(windows)]
fn resolve_windows_program_on_path(program: &str) -> Option<PathBuf> {
    let candidate = Path::new(program);
    let has_path_separator = program.contains(['\\', '/']);
    let has_extension = candidate.extension().is_some();

    if candidate.is_absolute() || has_path_separator {
        return resolve_windows_program_candidate(candidate, has_extension);
    }

    let path = std::env::var_os("PATH")?;
    for directory in std::env::split_paths(&path) {
        let joined = directory.join(candidate);
        if let Some(resolved) = resolve_windows_program_candidate(&joined, has_extension) {
            return Some(resolved);
        }
    }

    None
}

#[cfg(windows)]
fn resolve_windows_program_candidate(candidate: &Path, has_extension: bool) -> Option<PathBuf> {
    if has_extension {
        return candidate.is_file().then(|| candidate.to_path_buf());
    }

    if candidate.is_file() {
        return Some(candidate.to_path_buf());
    }

    for extension in windows_program_extensions() {
        let mut program = candidate.as_os_str().to_os_string();
        program.push(extension);
        let resolved = PathBuf::from(program);
        if resolved.is_file() {
            return Some(resolved);
        }
    }

    None
}

#[cfg(windows)]
fn windows_program_extensions() -> Vec<OsString> {
    std::env::var_os("PATHEXT")
        .map(|value| {
            value
                .to_string_lossy()
                .split(';')
                .filter(|part| !part.is_empty())
                .map(OsString::from)
                .collect::<Vec<_>>()
        })
        .filter(|extensions| !extensions.is_empty())
        .unwrap_or_else(|| {
            vec![
                OsString::from(".COM"),
                OsString::from(".EXE"),
                OsString::from(".BAT"),
                OsString::from(".CMD"),
            ]
        })
}

#[must_use]
/// cli[impl shell.default.show-effective]
pub fn format_command_line(argv: &[String]) -> String {
    argv.iter()
        .map(|argument| quote_windows_command_argument(argument))
        .collect::<Vec<_>>()
        .join(" ")
}

fn builtin_default_argv() -> Vec<String> {
    #[cfg(windows)]
    {
        vec![
            std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_owned()),
            "/D".to_owned(),
        ]
    }

    #[cfg(not(windows))]
    {
        vec![std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_owned())]
    }
}

fn parse_shell_file(contents: &str) -> Vec<String> {
    contents.lines().map(ToOwned::to_owned).collect()
}

fn serialize_shell_file(argv: &[String]) -> String {
    let mut contents = argv.join("\n");
    contents.push('\n');
    contents
}

fn quote_windows_command_argument(argument: &str) -> String {
    if argument.is_empty() {
        return "\"\"".to_owned();
    }

    if !argument.contains([' ', '\t', '"']) {
        return argument.to_owned();
    }

    let mut quoted = String::from('"');
    let mut backslashes = 0_usize;

    for character in argument.chars() {
        match character {
            '\\' => backslashes += 1,
            '"' => {
                quoted.push_str(&"\\".repeat((backslashes * 2) + 1));
                quoted.push('"');
                backslashes = 0;
            }
            _ => {
                if backslashes > 0 {
                    quoted.push_str(&"\\".repeat(backslashes));
                    backslashes = 0;
                }
                quoted.push(character);
            }
        }
    }

    if backslashes > 0 {
        quoted.push_str(&"\\".repeat(backslashes * 2));
    }

    quoted.push('"');
    quoted
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{
        command_builder_from_argv, default_shell_path, format_command_line, load_configured_argv,
        parse_shell_file, save_configured_argv, serialize_shell_file,
    };
    use crate::paths::AppHome;

    struct TestHome {
        path: PathBuf,
    }

    impl TestHome {
        fn new() -> Self {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock should be after unix epoch")
                .as_nanos();
            let path =
                std::env::temp_dir().join(format!("teamy-studio-shell-default-test-{unique}"));
            Self { path }
        }

        fn app_home(&self) -> AppHome {
            AppHome(self.path.clone())
        }
    }

    impl Drop for TestHome {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn shell_file_roundtrip_preserves_arguments() {
        let argv = vec![
            "pwsh.exe".to_owned(),
            "-NoLogo".to_owned(),
            "C:\\Program Files\\PowerShell\\7\\pwsh.exe".to_owned(),
        ];

        assert_eq!(parse_shell_file(&serialize_shell_file(&argv)), argv);
    }

    #[test]
    fn format_command_line_quotes_whitespace() {
        let argv = vec![
            "C:\\Program Files\\PowerShell\\7\\pwsh.exe".to_owned(),
            "-Command".to_owned(),
            "Write-Host hi".to_owned(),
        ];

        assert_eq!(
            format_command_line(&argv),
            "\"C:\\Program Files\\PowerShell\\7\\pwsh.exe\" -Command \"Write-Host hi\""
        );
    }

    #[test]
    fn save_and_load_configured_argv_uses_supplied_app_home() {
        let test_home = TestHome::new();
        let app_home = test_home.app_home();
        let expected = vec!["pwsh.exe".to_owned(), "-NoLogo".to_owned()];

        save_configured_argv(&app_home, expected[0].clone(), expected[1..].to_vec())
            .expect("shell config should save successfully");

        assert_eq!(
            load_configured_argv(&app_home).expect("shell config should load successfully"),
            Some(expected)
        );
        assert!(default_shell_path(&app_home).exists());
    }

    #[cfg(windows)]
    #[test]
    // cli[verify shell.default.windows-launch-resolves-program-on-path]
    fn command_builder_resolves_bare_windows_program_names() {
        let command = command_builder_from_argv(&["cmd".to_owned(), "/D".to_owned()])
            .expect("command builder should resolve cmd through PATH");
        let argv = command.get_argv();
        let program = argv
            .first()
            .expect("resolved command should include a program")
            .to_string_lossy()
            .to_string();

        assert!(program.to_ascii_lowercase().ends_with("cmd.exe"));
    }
}

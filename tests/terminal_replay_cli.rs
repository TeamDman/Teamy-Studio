#![cfg(windows)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

fn run_teamy_studio(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_teamy-studio"))
        .args(args)
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

// cli[verify command.surface.self-test-terminal-replay]
// cli[verify self-test.terminal-replay.artifact-output]
#[test]
fn terminal_replay_command_runs_headlessly_and_writes_artifact() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("terminal-replay")
        .join("hello.json");
    let output_dir = TempDirGuard::new("teamy-studio-terminal-replay");
    let artifact = output_dir.path().join("replay-report.json");

    let output = run_teamy_studio(&[
        "self-test",
        "terminal-replay",
        "--fixture",
        fixture.to_string_lossy().as_ref(),
        "--artifact-output",
        artifact.to_string_lossy().as_ref(),
    ]);
    let text = output_text(&output);

    assert!(output.status.success(), "terminal replay failed:\n{text}");
    assert!(
        text.contains("median_apply_ms:"),
        "missing replay timing:\n{text}"
    );
    assert!(
        text.contains("teamy_matches_ghostty: true"),
        "missing Teamy comparison result:\n{text}"
    );
    assert!(
        text.contains("hello"),
        "missing replay screen text:\n{text}"
    );
    assert!(artifact.exists(), "replay artifact was not written");

    let artifact_text = std::fs::read_to_string(&artifact).expect("artifact should be readable");
    assert!(artifact_text.contains("final_screen"));
    assert!(artifact_text.contains("teamy_final_screen"));
    assert!(artifact_text.contains("teamy_matches_ghostty"));
    assert!(artifact_text.contains("teamy_display"));
    assert!(artifact_text.contains("teamy_trace"));
    assert!(artifact_text.contains("visible_rows"));
    assert!(artifact_text.contains("cursor"));
    assert!(artifact_text.contains("events"));
    assert!(artifact_text.contains("hello"));
}

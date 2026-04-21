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

// cli[verify command.surface.self-test-render-offscreen]
// cli[verify self-test.render-offscreen.artifact-output]
// os[verify os.windows.rendering.direct3d12.offscreen-terminal-verification]
#[test]
fn render_offscreen_command_runs_headlessly_and_writes_png_artifact() {
    let output_dir = TempDirGuard::new("teamy-studio-render-offscreen");
    let artifact = output_dir.path().join("offscreen-render.png");
    let scene_artifact = output_dir.path().join("offscreen-render-scene.txt");

    let output = run_teamy_studio(&[
        "self-test",
        "render-offscreen",
        "--fixture",
        "basic-terminal-frame",
        "--artifact-output",
        artifact.to_string_lossy().as_ref(),
    ]);
    let text = output_text(&output);

    assert!(output.status.success(), "render-offscreen failed:\n{text}");
    assert!(
        text.contains("\"image_width\":"),
        "missing image metrics:\n{text}"
    );
    assert!(
        text.contains("\"matched_expected\": true"),
        "render fixture did not report an expected-image match:\n{text}"
    );
    assert!(
        text.contains("\"matched_scene_snapshot\": true"),
        "render fixture did not report a scene-snapshot match:\n{text}"
    );
    assert!(artifact.exists(), "offscreen PNG artifact was not written");
    assert!(
        scene_artifact.exists(),
        "offscreen scene snapshot artifact was not written"
    );

    let image = image::open(&artifact)
        .expect("offscreen artifact should be readable")
        .into_rgba8();
    let scene_snapshot = std::fs::read_to_string(&scene_artifact)
        .expect("offscreen scene snapshot artifact should be readable");

    assert!(image.pixels().any(|pixel| pixel[3] > 0));
    assert!(
        image
            .pixels()
            .any(|pixel| { u16::from(pixel[0]) + u16::from(pixel[1]) + u16::from(pixel[2]) > 64 }),
        "offscreen image should contain visible content"
    );
    assert!(
        scene_snapshot.contains("fragment_count="),
        "scene snapshot should include fragment metadata"
    );
}

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

const FXC_PATH: &str = r"C:\Program Files (x86)\Windows Kits\10\bin\10.0.22621.0\x64\fxc.exe";

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR should be set"));
    let shader_dir = manifest_dir.join("..").join("..").join("legacy").join("src").join("app");
    let panel_shader = shader_dir.join("windows_panel_shaders.hlsl");
    let chrome_shader = shader_dir.join("windows_chrome_shaders.hlsl");
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR should be set"));

    println!("cargo:rerun-if-changed={}", panel_shader.display());
    println!("cargo:rerun-if-changed={}", chrome_shader.display());

    compile_shader(&panel_shader, &shader_dir, &out_dir.join("windows_panel_vs.cso"), "VSMain", "vs_5_0");
    compile_shader(&panel_shader, &shader_dir, &out_dir.join("windows_panel_ps.cso"), "PSMain", "ps_5_0");
}

fn compile_shader(input_path: &Path, include_dir: &Path, output_path: &Path, entry_point: &str, target: &str) {
    let mut command = Command::new(FXC_PATH);
    command
        .arg("/nologo")
        .arg("/T")
        .arg(target)
        .arg("/E")
        .arg(entry_point)
        .arg("/I")
        .arg(include_dir)
        .arg("/Fo")
        .arg(output_path)
        .arg(input_path);

    if matches!(env::var("PROFILE").as_deref(), Ok("debug")) {
        command.arg("/Zi").arg("/Od");
    }

    let output = command
        .output()
        .unwrap_or_else(|error| panic!("failed to start fxc at {FXC_PATH}: {error}"));

    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        panic!(
            "fxc failed for {} {}\nstdout:\n{}\nstderr:\n{}",
            entry_point,
            target,
            stdout.trim(),
            stderr.trim()
        );
    }
}
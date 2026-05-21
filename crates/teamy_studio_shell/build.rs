use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let manifest_dir =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR should be set"));
    let shader_dir = manifest_dir.join("src");
    let panel_shader = shader_dir.join("windows_panel_shaders.hlsl");
    let chrome_shader = shader_dir.join("windows_chrome_shaders.hlsl");
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR should be set"));

    println!("cargo:rerun-if-changed={}", panel_shader.display());
    println!("cargo:rerun-if-changed={}", chrome_shader.display());
    println!("cargo:rerun-if-env-changed=TEAMY_FXC_PATH");

    let fxc_path = find_fxc().unwrap_or_else(|| {
        panic!("failed to locate fxc.exe; set TEAMY_FXC_PATH to the Windows SDK fxc.exe path")
    });

    compile_shader(
        &fxc_path,
        &panel_shader,
        &shader_dir,
        &out_dir.join("windows_panel_vs.cso"),
        "VSMain",
        "vs_5_0",
    );
    compile_shader(
        &fxc_path,
        &panel_shader,
        &shader_dir,
        &out_dir.join("windows_panel_ps.cso"),
        "PSMain",
        "ps_5_0",
    );
}

fn find_fxc() -> Option<PathBuf> {
    env::var_os("TEAMY_FXC_PATH")
        .map(PathBuf::from)
        .filter(|path| path.exists())
        .or_else(find_fxc_in_windows_kits)
}

fn find_fxc_in_windows_kits() -> Option<PathBuf> {
    let kit_bin = PathBuf::from(r"C:\Program Files (x86)\Windows Kits\10\bin");
    let mut versions = std::fs::read_dir(kit_bin)
        .ok()?
        .flatten()
        .filter_map(|entry| {
            let path = entry.path().join("x64").join("fxc.exe");
            path.exists().then_some(path)
        })
        .collect::<Vec<_>>();
    versions.sort();
    versions.pop()
}

fn compile_shader(
    fxc_path: &Path,
    input_path: &Path,
    include_dir: &Path,
    output_path: &Path,
    entry_point: &str,
    target: &str,
) {
    let mut command = Command::new(fxc_path);
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
        .unwrap_or_else(|error| panic!("failed to start fxc at {}: {error}", fxc_path.display()));

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

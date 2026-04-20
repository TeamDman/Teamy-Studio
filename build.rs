use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    add_exe_resources();
    add_git_revision();
    if env::var_os("CARGO_FEATURE_GHOSTTY").is_some() {
        stage_ghostty_binaries();
    }
    stage_openconsole_binaries();
}

/// Embeds Windows resources (like application icon) into the executable.
fn add_exe_resources() {
    println!("cargo:rerun-if-changed=resources");

    embed_resource::compile("resources/app.rc", embed_resource::NONE)
        .manifest_required()
        .expect("failed to embed resources");
}

/// In your code you can now access git revision using
/// ```rust
/// let git_rev = option_env!("GIT_REVISION").unwrap_or("unknown");
/// ```
/// tool[impl cli.version.includes-git-revision]
fn add_git_revision() {
    // Try to get a short git revision; on failure, set to "unknown".
    let rev = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|o| o.status.success().then_some(o.stdout))
        .and_then(|v| String::from_utf8(v).ok())
        .map_or_else(|| "unknown".to_string(), |s| s.trim().to_string());

    println!("cargo:rustc-env=GIT_REVISION={rev}",);
}

fn stage_ghostty_binaries() {
    println!("cargo:rerun-if-env-changed=DEP_GHOSTTY_VT_INCLUDE");

    let Some(output_dir) = cargo_profile_dir() else {
        println!(
            "cargo:warning=skipping Ghostty staging because Cargo output directory could not be determined"
        );
        return;
    };

    let Some(source_dir) = ghostty_build_dir(&output_dir) else {
        return;
    };

    stage_binary(&source_dir, &output_dir, "ghostty-vt.dll", "Ghostty");
}

fn stage_openconsole_binaries() {
    println!("cargo:rerun-if-env-changed=TEAMY_OPENCONSOLE_BUILD_DIR");

    let Some(source_dir) = openconsole_build_dir() else {
        return;
    };

    let Some(output_dir) = cargo_profile_dir() else {
        println!(
            "cargo:warning=skipping OpenConsole staging because Cargo output directory could not be determined"
        );
        return;
    };

    for file_name in ["conpty.dll", "OpenConsole.exe"] {
        stage_binary(&source_dir, &output_dir, file_name, "OpenConsole");
    }
}

fn stage_binary(source_dir: &Path, output_dir: &Path, file_name: &str, label: &str) {
    let source = source_dir.join(file_name);
    if !source.exists() {
        return;
    }

    println!("cargo:rerun-if-changed={}", source.display());

    let destination = output_dir.join(file_name);
    if let Err(error) = fs::copy(&source, &destination) {
        println!(
            "cargo:warning=failed to stage {} from {} to {}: {}",
            label,
            source.display(),
            destination.display(),
            error,
        );
    }
}

fn ghostty_build_dir(output_dir: &Path) -> Option<PathBuf> {
    let env_dir = env::var_os("DEP_GHOSTTY_VT_INCLUDE")
        .map(PathBuf::from)
        .and_then(|include_dir| include_dir.parent().map(Path::to_path_buf))
        .map(|prefix| prefix.join("bin"))
        .filter(|path| path.join("ghostty-vt.dll").exists());
    if env_dir.is_some() {
        return env_dir;
    }

    let build_dir = output_dir.join("build");
    let entries = fs::read_dir(build_dir).ok()?;

    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if !name.starts_with("libghostty-vt-sys-") {
            continue;
        }

        let candidate = path.join("out").join("ghostty-install").join("bin");
        if candidate.join("ghostty-vt.dll").exists() {
            return Some(candidate);
        }
    }

    None
}

fn openconsole_build_dir() -> Option<PathBuf> {
    let env_dir = env::var_os("TEAMY_OPENCONSOLE_BUILD_DIR")
        .map(PathBuf::from)
        .filter(|path| path.join("conpty.dll").exists() && path.join("OpenConsole.exe").exists());
    if env_dir.is_some() {
        return env_dir;
    }

    let manifest_dir = env::var_os("CARGO_MANIFEST_DIR").map(PathBuf::from)?;
    let sibling_dir = manifest_dir
        .parent()
        .map(Path::to_path_buf)?
        .join("microsoft-terminal")
        .join("bin")
        .join("x64")
        .join("Release");

    (sibling_dir.join("conpty.dll").exists() && sibling_dir.join("OpenConsole.exe").exists())
        .then_some(sibling_dir)
}

fn cargo_profile_dir() -> Option<PathBuf> {
    let out_dir = env::var_os("OUT_DIR").map(PathBuf::from)?;
    out_dir.parent()?.parent()?.parent().map(Path::to_path_buf)
}

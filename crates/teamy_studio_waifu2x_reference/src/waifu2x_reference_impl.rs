use eyre::{Context, bail};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const WAIFU2X_REFERENCE_SOURCE_PARENT_DIR: &str = "python";
pub const WAIFU2X_REFERENCE_SOURCE_DIR_NAME: &str = "waifu2x-reference";
pub const DEFAULT_NUNIF_ROOT: &str = r"G:\Programming\Repos\nunif";
pub const DEFAULT_NUNIF_HOME: &str = r"G:\Programming\Caches\NUNIF_HOME";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Waifu2xReferenceCommandOutput {
    pub stdout: String,
    pub stderr: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Waifu2xReferenceImportReport {
    pub ok: bool,
    pub nunif_home: String,
    pub torch: String,
    pub torchvision: String,
    pub pillow: String,
    pub cuda: Waifu2xReferenceCudaReport,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Waifu2xReferenceCudaReport {
    pub available: bool,
    pub device_count: u32,
    pub device_name: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Waifu2xReferenceModelReport {
    pub ok: bool,
    pub model_type: String,
    pub method: String,
    pub tile_size: u32,
    pub batch_size: u32,
    pub torch_model_class: String,
    pub torch_model_name: String,
    pub i2i_scale: u32,
    pub i2i_offset: u32,
    pub i2i_blend_size: u32,
    pub parameter_count: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Waifu2xReferenceFixtureReport {
    pub ok: bool,
    pub fixture_path: String,
    pub pil_mode: String,
    pub pil_size: Vec<u32>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Waifu2xReferenceTensorReport {
    pub ok: bool,
    pub image_path: String,
    pub pil_mode: String,
    pub pil_size: Vec<u32>,
    pub blank_alpha: bool,
    pub tensors: Waifu2xReferenceTensorReports,
    pub dump_npz: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Waifu2xReferenceTensorReports {
    pub rgb: Waifu2xReferenceTensorSummary,
    pub alpha: Option<Waifu2xReferenceTensorSummary>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Waifu2xReferenceTensorSummary {
    pub shape: Vec<u32>,
    pub dtype: String,
    pub min: f32,
    pub max: f32,
    pub mean: f32,
    pub sum: f32,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Waifu2xReferenceLayerReport {
    pub ok: bool,
    pub model_type: String,
    pub method: String,
    pub device: String,
    pub input_size: u32,
    pub layers: std::collections::BTreeMap<String, Waifu2xReferenceTensorSummary>,
    pub dump_npz: Option<String>,
}

#[must_use]
pub fn waifu2x_reference_source_dir() -> PathBuf {
    crate::repo_root_dir()
        .join(WAIFU2X_REFERENCE_SOURCE_PARENT_DIR)
        .join(WAIFU2X_REFERENCE_SOURCE_DIR_NAME)
}

/// # Errors
///
/// This function will return an error if the Python reference harness cannot be launched or exits
/// unsuccessfully.
// image[impl self-test.reference-command]
pub fn run_waifu2x_reference_import_check() -> eyre::Result<Waifu2xReferenceCommandOutput> {
    run_waifu2x_reference([
        "--check-imports",
        "--nunif-root",
        DEFAULT_NUNIF_ROOT,
        "--nunif-home",
        DEFAULT_NUNIF_HOME,
    ])
}

/// # Errors
///
/// This function will return an error if the Python reference import check cannot be parsed.
// image[impl self-test.reference-command]
pub fn read_waifu2x_reference_import_report() -> eyre::Result<Waifu2xReferenceImportReport> {
    let output = run_waifu2x_reference_import_check()?;
    parse_reference_json(&output.stdout, "waifu2x reference import report")
}

/// # Errors
///
/// This function will return an error if the Python reference model report cannot be produced.
// image[impl self-test.reference-command]
pub fn run_waifu2x_reference_model_report(
    download_models: bool,
) -> eyre::Result<Waifu2xReferenceCommandOutput> {
    let mut args = vec![
        "--model-report",
        "--nunif-root",
        DEFAULT_NUNIF_ROOT,
        "--nunif-home",
        DEFAULT_NUNIF_HOME,
        "--device",
        "cpu",
        "--model-type",
        "art",
        "--method",
        "scale",
        "--tile-size",
        "256",
        "--batch-size",
        "4",
    ];
    if download_models {
        args.push("--download-models");
    }
    run_waifu2x_reference(args)
}

/// # Errors
///
/// This function will return an error if the Python reference model report cannot be parsed.
// image[impl self-test.reference-command]
pub fn read_waifu2x_reference_model_report(
    download_models: bool,
) -> eyre::Result<Waifu2xReferenceModelReport> {
    let output = run_waifu2x_reference_model_report(download_models)?;
    parse_reference_json(&output.stdout, "waifu2x reference model report")
}

/// # Errors
///
/// This function will return an error if the Python reference fixture cannot be written.
// image[impl self-test.reference-command]
pub fn write_waifu2x_reference_fixture(
    fixture_path: &Path,
) -> eyre::Result<Waifu2xReferenceFixtureReport> {
    let fixture_path = fixture_path.to_string_lossy().into_owned();
    let output = run_waifu2x_reference([
        "--write-fixture",
        fixture_path.as_str(),
        "--nunif-root",
        DEFAULT_NUNIF_ROOT,
        "--nunif-home",
        DEFAULT_NUNIF_HOME,
    ])?;
    parse_reference_json(&output.stdout, "waifu2x reference fixture report")
}

/// # Errors
///
/// This function will return an error if the Python reference tensor report cannot be parsed.
// image[impl self-test.reference-command]
pub fn read_waifu2x_reference_tensor_report(
    image_path: &Path,
    dump_npz_path: Option<&Path>,
) -> eyre::Result<Waifu2xReferenceTensorReport> {
    let image_path = image_path.to_string_lossy().into_owned();
    let dump_npz_path = dump_npz_path.map(|path| path.to_string_lossy().into_owned());
    let mut args = vec![
        "--tensor-report".to_owned(),
        image_path,
        "--nunif-root".to_owned(),
        DEFAULT_NUNIF_ROOT.to_owned(),
        "--nunif-home".to_owned(),
        DEFAULT_NUNIF_HOME.to_owned(),
    ];
    if let Some(dump_npz_path) = dump_npz_path {
        args.push("--dump-npz".to_owned());
        args.push(dump_npz_path);
    }
    let output = run_waifu2x_reference(args)?;
    parse_reference_json(&output.stdout, "waifu2x reference tensor report")
}

/// # Errors
///
/// This function will return an error if the Python reference layer report cannot be parsed.
// image[impl self-test.reference-command]
pub fn read_waifu2x_reference_layer_report(
    dump_npz_path: Option<&Path>,
) -> eyre::Result<Waifu2xReferenceLayerReport> {
    let dump_npz_path = dump_npz_path.map(|path| path.to_string_lossy().into_owned());
    let mut args = vec![
        "--layer-report".to_owned(),
        "--nunif-root".to_owned(),
        DEFAULT_NUNIF_ROOT.to_owned(),
        "--nunif-home".to_owned(),
        DEFAULT_NUNIF_HOME.to_owned(),
        "--device".to_owned(),
        "cpu".to_owned(),
        "--model-type".to_owned(),
        "art".to_owned(),
        "--method".to_owned(),
        "scale".to_owned(),
        "--tile-size".to_owned(),
        "256".to_owned(),
        "--batch-size".to_owned(),
        "4".to_owned(),
    ];
    if let Some(dump_npz_path) = dump_npz_path {
        args.push("--layer-dump-npz".to_owned());
        args.push(dump_npz_path);
    }
    let output = run_waifu2x_reference(args)?;
    parse_reference_json(&output.stdout, "waifu2x reference layer report")
}

fn run_waifu2x_reference<I, S>(args: I) -> eyre::Result<Waifu2xReferenceCommandOutput>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let source_dir = waifu2x_reference_source_dir();
    ensure_reference_source_dir(&source_dir)?;
    let output = Command::new("uv")
        .arg("run")
        .arg("teamy-waifu2x-reference")
        .args(args)
        .current_dir(&source_dir)
        .output()
        .wrap_err("failed to launch waifu2x reference harness through uv")?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    if !output.status.success() {
        bail!(
            "waifu2x reference harness exited with {}\nstdout:\n{}\nstderr:\n{}",
            output.status,
            stdout,
            stderr
        );
    }
    Ok(Waifu2xReferenceCommandOutput { stdout, stderr })
}

fn ensure_reference_source_dir(path: &Path) -> eyre::Result<()> {
    if !path.is_dir() {
        bail!(
            "waifu2x reference source directory does not exist: {}",
            path.display()
        );
    }
    Ok(())
}

fn parse_reference_json<T>(json: &str, label: &str) -> eyre::Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    serde_json::from_str(json).wrap_err_with(|| format!("failed to parse {label}"))
}

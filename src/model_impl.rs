#![expect(warnings, reason = "ported experimental Burn Whisper prototype")]

use burn_store::{BurnpackStore, ModuleSnapshot, PytorchStore, pytorch::PytorchReader};
use eyre::{WrapErr, bail};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::paths::{AppHome, CacheHome};
use crate::whisper::{WhisperCpuBackend, WhisperDims, WhisperModel, WhisperModelConfig};

pub const MODEL_DIRS_FILE_NAME: &str = "model-dirs.txt";
pub const MANAGED_MODELS_DIR_NAME: &str = "models";
pub const MANAGED_MODEL_DOWNLOADS_DIR_NAME: &str = "downloads";
pub const MANAGED_MODEL_CONVERSIONS_DIR_NAME: &str = "conversions";
pub const MODEL_PACKAGE_EXTENSION: &str = "zip";
pub const DEFAULT_MODEL_PACKAGE_URL_ENV_VAR: &str = "TEAMY_STUDIO_WHISPER_MODEL_PACKAGE_URL";
pub const TOKENIZER_FILE_NAME: &str = "tokenizer.json";
pub const MODEL_BURNPACK_FILE_NAME: &str = "model.bpk";
pub const MODEL_DIMS_FILE_NAME: &str = "dims.json";
pub const DEFAULT_TRANSCRIPTION_MODEL_NAME: &str = "tiny.en";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedNamedModel {
    pub managed_dir: PathBuf,
    pub artifacts: WhisperModelArtifacts,
    pub registered_model_dirs: Vec<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KnownWhisperModel {
    pub name: &'static str,
    pub checkpoint_url: &'static str,
    pub hugging_face_model_id: &'static str,
    pub parameter_count: &'static str,
    pub vram_estimate: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WhisperModelPreparationState {
    Missing,
    DownloadedUnprocessed,
    Compatible,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WhisperModelLocationStatus {
    pub label: String,
    pub path: PathBuf,
    pub exists: bool,
    pub compatible: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WhisperModelPreparationStatus {
    pub model_name: String,
    pub state: WhisperModelPreparationState,
    pub locations: Vec<WhisperModelLocationStatus>,
}

impl WhisperModelPreparationStatus {
    #[must_use]
    pub const fn is_compatible(&self) -> bool {
        matches!(self.state, WhisperModelPreparationState::Compatible)
    }
}

pub const KNOWN_WHISPER_MODELS: [KnownWhisperModel; 14] = [
    KnownWhisperModel {
        name: "tiny.en",
        checkpoint_url: "https://openaipublic.azureedge.net/main/whisper/models/d3dd57d32accea0b295c96e26691aa14d8822fac7d9d27d5dc00b4ca2826dd03/tiny.en.pt",
        hugging_face_model_id: "openai/whisper-tiny.en",
        parameter_count: "39M",
        vram_estimate: "~1 GiB",
    },
    KnownWhisperModel {
        name: "tiny",
        checkpoint_url: "https://openaipublic.azureedge.net/main/whisper/models/65147644a518d12f04e32d6f3b26facc3f8dd46e5390956a9424a650c0ce22b9/tiny.pt",
        hugging_face_model_id: "openai/whisper-tiny",
        parameter_count: "39M",
        vram_estimate: "~1 GiB",
    },
    KnownWhisperModel {
        name: "base.en",
        checkpoint_url: "https://openaipublic.azureedge.net/main/whisper/models/25a8566e1d0c1e2231d1c762132cd20e0f96a85d16145c3a00adf5d1ac670ead/base.en.pt",
        hugging_face_model_id: "openai/whisper-base.en",
        parameter_count: "74M",
        vram_estimate: "~1 GiB",
    },
    KnownWhisperModel {
        name: "base",
        checkpoint_url: "https://openaipublic.azureedge.net/main/whisper/models/ed3a0b6b1c0edf879ad9b11b1af5a0e6ab5db9205f891f668f8b0e6c6326e34e/base.pt",
        hugging_face_model_id: "openai/whisper-base",
        parameter_count: "74M",
        vram_estimate: "~1 GiB",
    },
    KnownWhisperModel {
        name: "small.en",
        checkpoint_url: "https://openaipublic.azureedge.net/main/whisper/models/f953ad0fd29cacd07d5a9eda5624af0f6bcf2258be67c92b79389873d91e0872/small.en.pt",
        hugging_face_model_id: "openai/whisper-small.en",
        parameter_count: "244M",
        vram_estimate: "~2 GiB",
    },
    KnownWhisperModel {
        name: "small",
        checkpoint_url: "https://openaipublic.azureedge.net/main/whisper/models/9ecf779972d90ba49c06d968637d720dd632c55bbf19d441fb42bf17a411e794/small.pt",
        hugging_face_model_id: "openai/whisper-small",
        parameter_count: "244M",
        vram_estimate: "~2 GiB",
    },
    KnownWhisperModel {
        name: "medium.en",
        checkpoint_url: "https://openaipublic.azureedge.net/main/whisper/models/d7440d1dc186f76616474e0ff0b3d6b879abc9d1a4926b7adfa41db2d497ab4f/medium.en.pt",
        hugging_face_model_id: "openai/whisper-medium.en",
        parameter_count: "769M",
        vram_estimate: "~5 GiB",
    },
    KnownWhisperModel {
        name: "medium",
        checkpoint_url: "https://openaipublic.azureedge.net/main/whisper/models/345ae4da62f9b3d59415adc60127b97c714f32e89e936602e85993674d08dcb1/medium.pt",
        hugging_face_model_id: "openai/whisper-medium",
        parameter_count: "769M",
        vram_estimate: "~5 GiB",
    },
    KnownWhisperModel {
        name: "large-v1",
        checkpoint_url: "https://openaipublic.azureedge.net/main/whisper/models/e4b87e7e0bf463eb8e6956e646f1e277e901512310def2c24bf0e11bd3c28e9a/large-v1.pt",
        hugging_face_model_id: "openai/whisper-large-v1",
        parameter_count: "1550M",
        vram_estimate: "~10 GiB",
    },
    KnownWhisperModel {
        name: "large-v2",
        checkpoint_url: "https://openaipublic.azureedge.net/main/whisper/models/81f7c96c852ee8fc832187b0132e569d6c3065a3252ed18e56effd0b6a73e524/large-v2.pt",
        hugging_face_model_id: "openai/whisper-large-v2",
        parameter_count: "1550M",
        vram_estimate: "~10 GiB",
    },
    KnownWhisperModel {
        name: "large-v3",
        checkpoint_url: "https://openaipublic.azureedge.net/main/whisper/models/e5b1a55b89c1367dacf97e3e19bfd829a01529dbfdeefa8caeb59b3f1b81dadb/large-v3.pt",
        hugging_face_model_id: "openai/whisper-large-v3",
        parameter_count: "1550M",
        vram_estimate: "~10 GiB",
    },
    KnownWhisperModel {
        name: "large",
        checkpoint_url: "https://openaipublic.azureedge.net/main/whisper/models/e5b1a55b89c1367dacf97e3e19bfd829a01529dbfdeefa8caeb59b3f1b81dadb/large-v3.pt",
        hugging_face_model_id: "openai/whisper-large-v3",
        parameter_count: "1550M",
        vram_estimate: "~10 GiB",
    },
    KnownWhisperModel {
        name: "large-v3-turbo",
        checkpoint_url: "https://openaipublic.azureedge.net/main/whisper/models/aff26ae408abcba5fbf8813c21e62b0941638c5f6eebfb145be0c9839262a19a/large-v3-turbo.pt",
        hugging_face_model_id: "openai/whisper-large-v3-turbo",
        parameter_count: "809M",
        vram_estimate: "~6 GiB",
    },
    KnownWhisperModel {
        name: "turbo",
        checkpoint_url: "https://openaipublic.azureedge.net/main/whisper/models/aff26ae408abcba5fbf8813c21e62b0941638c5f6eebfb145be0c9839262a19a/large-v3-turbo.pt",
        hugging_face_model_id: "openai/whisper-large-v3-turbo",
        parameter_count: "809M",
        vram_estimate: "~6 GiB",
    },
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedModelDir {
    pub managed_dir: PathBuf,
    pub artifacts: WhisperModelArtifacts,
    pub registered_model_dirs: Vec<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackagedModelArchive {
    pub archive_path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WhisperModelLayout {
    WhisperBurnNpy,
    BurnPack,
}

impl WhisperModelLayout {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            WhisperModelLayout::WhisperBurnNpy => "whisper-burn-npy",
            WhisperModelLayout::BurnPack => "burnpack",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TokenizerMetadata {
    pub path: PathBuf,
    pub vocab_size: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WhisperModelArtifacts {
    pub root: PathBuf,
    pub layout: WhisperModelLayout,
    pub tokenizer: TokenizerMetadata,
    pub encoder_dir: Option<PathBuf>,
    pub decoder_dir: Option<PathBuf>,
    pub burnpack_path: Option<PathBuf>,
    pub dims_path: Option<PathBuf>,
    pub dims: Option<WhisperDims>,
}

/// Discover a locally converted Whisper model directory.
///
/// # Errors
///
/// This function will return an error if the directory does not exist, the tokenizer cannot be
/// loaded, or the expected whisper-burn artifact layout is incomplete.
pub fn inspect_model_dir(root: &Path) -> eyre::Result<WhisperModelArtifacts> {
    ensure_existing_dir(root)?;

    let tokenizer_path = root.join(TOKENIZER_FILE_NAME);
    if !tokenizer_path.is_file() {
        bail!(
            "Model directory is missing {}: {}",
            TOKENIZER_FILE_NAME,
            tokenizer_path.display()
        );
    }

    let tokenizer = tokenizers::Tokenizer::from_file(&tokenizer_path).map_err(|error| {
        eyre::eyre!(
            "Failed to load tokenizer from {}: {}",
            tokenizer_path.display(),
            error
        )
    })?;

    let tokenizer = TokenizerMetadata {
        path: tokenizer_path,
        vocab_size: tokenizer.get_vocab_size(true),
    };

    let encoder_dir = root.join("encoder");
    let decoder_dir = root.join("decoder");
    if encoder_dir.is_dir() && decoder_dir.is_dir() {
        ensure_whisper_burn_dir(&encoder_dir, "encoder")?;
        ensure_whisper_burn_dir(&decoder_dir, "decoder")?;

        let mut artifacts = WhisperModelArtifacts {
            root: root.to_path_buf(),
            layout: WhisperModelLayout::WhisperBurnNpy,
            tokenizer,
            encoder_dir: Some(encoder_dir),
            decoder_dir: Some(decoder_dir),
            burnpack_path: None,
            dims_path: None,
            dims: None,
        };
        artifacts.dims = crate::whisper::infer_dims_from_artifacts(&artifacts).ok();
        return Ok(artifacts);
    }

    let burnpack_path = root.join(MODEL_BURNPACK_FILE_NAME);
    let dims_path = root.join(MODEL_DIMS_FILE_NAME);
    if burnpack_path.is_file() && dims_path.is_file() {
        let dims = read_dims_file(&dims_path)?;
        return Ok(WhisperModelArtifacts {
            root: root.to_path_buf(),
            layout: WhisperModelLayout::BurnPack,
            tokenizer,
            encoder_dir: None,
            decoder_dir: None,
            burnpack_path: Some(burnpack_path),
            dims_path: Some(dims_path),
            dims: Some(dims),
        });
    }

    bail!(
        "Model directory {} is not a supported Teamy Studio Whisper layout. Expected either legacy encoder/decoder directories or {} + {} alongside {}.",
        root.display(),
        MODEL_BURNPACK_FILE_NAME,
        MODEL_DIMS_FILE_NAME,
        TOKENIZER_FILE_NAME,
    )
}

#[must_use]
pub fn render_model_report(artifacts: &WhisperModelArtifacts) -> String {
    let mut lines = vec![
        format!("Model root: {}", artifacts.root.display()),
        format!("Layout: {}", artifacts.layout.as_str()),
        format!("Tokenizer: {}", artifacts.tokenizer.path.display()),
        format!("Vocabulary size: {}", artifacts.tokenizer.vocab_size),
    ];

    match artifacts.layout {
        WhisperModelLayout::WhisperBurnNpy => {
            if let Some(path) = &artifacts.encoder_dir {
                lines.push(format!("Encoder dir: {}", path.display()));
            }
            if let Some(path) = &artifacts.decoder_dir {
                lines.push(format!("Decoder dir: {}", path.display()));
            }
        }
        WhisperModelLayout::BurnPack => {
            if let Some(path) = &artifacts.burnpack_path {
                lines.push(format!("Burnpack weights: {}", path.display()));
            }
            if let Some(path) = &artifacts.dims_path {
                lines.push(format!("Dims file: {}", path.display()));
            }
        }
    }

    lines.extend(artifacts.dims.as_ref().map_or_else(
        || vec!["Inferred dims: unavailable".to_owned()],
        |dims| dims.render_lines(),
    ));
    lines.join("\n")
}

/// Prepare a model directory or package archive into managed local storage and auto-register it.
///
/// # Errors
///
/// This function will return an error if the source cannot be read, unpacked, validated, or
/// registered.
pub fn prepare_model_source(
    app_home: &AppHome,
    cache_home: &CacheHome,
    source: &Path,
    install_name: Option<&str>,
    overwrite: bool,
) -> eyre::Result<PreparedModelDir> {
    if !source.exists() {
        bail!("Model source does not exist: {}", source.display());
    }

    let managed_root = managed_models_dir(cache_home);
    std::fs::create_dir_all(&managed_root).wrap_err_with(|| {
        format!(
            "Failed to create managed model storage {}",
            managed_root.display()
        )
    })?;

    let name = sanitized_install_name(source, install_name)?;
    let destination = managed_root.join(name);
    if destination.exists() {
        if overwrite {
            std::fs::remove_dir_all(&destination).wrap_err_with(|| {
                format!(
                    "Failed to replace existing managed model {}",
                    destination.display()
                )
            })?;
        } else {
            bail!(
                "Managed model directory already exists: {}. Re-run with --overwrite to replace it.",
                destination.display()
            );
        }
    }

    if source.is_dir() {
        copy_dir_recursive(source, &destination)?;
    } else if is_supported_model_package(source) {
        unpack_model_archive(source, &destination)?;
    } else {
        bail!(
            "Unsupported model source {}. Expected a model directory or .{} archive.",
            source.display(),
            MODEL_PACKAGE_EXTENSION
        );
    }

    let artifacts = validate_prepared_model_dir(&destination)?;
    let registered_model_dirs = add_registered_model_dir(app_home, &destination)?;

    Ok(PreparedModelDir {
        managed_dir: destination,
        artifacts,
        registered_model_dirs,
    })
}

/// Prepare a remotely-hosted model package into managed local storage and auto-register it.
///
/// # Errors
///
/// This function will return an error if the package cannot be downloaded, unpacked, validated,
/// or registered.
pub fn prepare_model_url(
    app_home: &AppHome,
    cache_home: &CacheHome,
    url: &str,
    install_name: Option<&str>,
    overwrite: bool,
) -> eyre::Result<PreparedModelDir> {
    let downloaded_archive = download_model_package(cache_home, url, install_name)?;
    prepare_model_source(
        app_home,
        cache_home,
        &downloaded_archive,
        install_name,
        overwrite,
    )
}

/// Package an existing model directory into a portable archive.
///
/// # Errors
///
/// This function will return an error if the source is not a compatible model directory or if the
/// archive cannot be written.
pub fn package_model_dir(source: &Path, output: &Path) -> eyre::Result<PackagedModelArchive> {
    let canonical_source = dunce::canonicalize(source).wrap_err_with(|| {
        format!(
            "Failed to canonicalize model directory {}",
            source.display()
        )
    })?;
    let _artifacts = validate_prepared_model_dir(&canonical_source)?;

    let entry_count = walkdir::WalkDir::new(&canonical_source)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.path() != canonical_source)
        .count();
    println!(
        "Packaging {} filesystem entries from {}",
        entry_count,
        canonical_source.display()
    );

    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent).wrap_err_with(|| {
            format!("Failed to create archive parent dir {}", parent.display())
        })?;
    }

    let file = std::fs::File::create(output)
        .wrap_err_with(|| format!("Failed to create model archive {}", output.display()))?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    let mut processed_entries = 0usize;
    for entry in walkdir::WalkDir::new(&canonical_source) {
        let entry = entry.wrap_err_with(|| {
            format!(
                "Failed to walk model directory {}",
                canonical_source.display()
            )
        })?;
        let path = entry.path();
        if path == canonical_source {
            continue;
        }

        let relative = path.strip_prefix(&canonical_source).wrap_err_with(|| {
            format!(
                "Failed to derive relative archive path for {}",
                path.display()
            )
        })?;
        let archive_name = relative.to_string_lossy().replace('\\', "/");

        if entry.file_type().is_dir() {
            zip.add_directory(format!("{archive_name}/"), options)
                .wrap_err_with(|| format!("Failed to add directory {} to archive", archive_name))?;
            processed_entries += 1;
            if processed_entries.is_multiple_of(100) || processed_entries == entry_count {
                println!("Packaging progress: {processed_entries}/{entry_count}");
            }
            continue;
        }

        zip.start_file(&archive_name, options)
            .wrap_err_with(|| format!("Failed to start archive file {}", archive_name))?;
        let mut input = std::fs::File::open(path)
            .wrap_err_with(|| format!("Failed to open source file {}", path.display()))?;
        std::io::copy(&mut input, &mut zip)
            .wrap_err_with(|| format!("Failed to write archive file {}", archive_name))?;
        processed_entries += 1;
        if processed_entries.is_multiple_of(100) || processed_entries == entry_count {
            println!("Packaging progress: {processed_entries}/{entry_count}");
        }
    }

    println!("Finalizing model archive: {}", output.display());
    zip.finish()
        .wrap_err_with(|| format!("Failed to finalize model archive {}", output.display()))?;

    println!("Created model archive: {}", output.display());

    Ok(PackagedModelArchive {
        archive_path: output.to_path_buf(),
    })
}

/// Convert a raw Whisper checkpoint into a packaged Teamy Studio Whisper archive.
///
/// # Errors
///
/// This function will return an error if the checkpoint cannot be converted, tokenizers cannot be
/// resolved, or the packaged archive cannot be written.
pub fn convert_checkpoint_to_model_package(
    cache_home: &CacheHome,
    checkpoint: &Path,
    output: &Path,
    model_id: Option<&str>,
    tokenizer: Option<&Path>,
    tokenizer_url: Option<&str>,
    overwrite: bool,
) -> eyre::Result<PackagedModelArchive> {
    if !checkpoint.is_file() {
        bail!(
            "Checkpoint does not exist or is not a file: {}",
            checkpoint.display()
        );
    }

    if output.exists() {
        if overwrite {
            if output.is_dir() {
                bail!(
                    "Refusing to overwrite output directory with a model archive: {}",
                    output.display()
                );
            }
            std::fs::remove_file(output).wrap_err_with(|| {
                format!(
                    "Failed to remove existing output archive {}",
                    output.display()
                )
            })?;
        } else {
            bail!(
                "Output archive already exists: {}. Re-run with --overwrite to replace it.",
                output.display()
            );
        }
    }

    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent).wrap_err_with(|| {
            format!("Failed to create archive parent dir {}", parent.display())
        })?;
    }

    let staging_dir = managed_model_conversion_dir(cache_home, checkpoint)?;
    if staging_dir.exists() {
        std::fs::remove_dir_all(&staging_dir).wrap_err_with(|| {
            format!(
                "Failed to replace staging conversion dir {}",
                staging_dir.display()
            )
        })?;
    }
    std::fs::create_dir_all(&staging_dir).wrap_err_with(|| {
        format!(
            "Failed to create staging conversion dir {}",
            staging_dir.display()
        )
    })?;

    println!("Converting checkpoint: {}", checkpoint.display());
    println!("Conversion staging dir: {}", staging_dir.display());

    convert_checkpoint_to_model_dir(checkpoint, &staging_dir, model_id, tokenizer, tokenizer_url)?;

    println!("Packaging converted model: {}", output.display());
    package_model_dir(&staging_dir, output)
}

/// Convert a raw Whisper checkpoint into a local Teamy Studio Whisper model directory.
///
/// # Errors
///
/// This function will return an error if the checkpoint, tokenizer, or Burnpack save step fails.
pub fn convert_checkpoint_to_model_dir(
    checkpoint: &Path,
    output_dir: &Path,
    model_id: Option<&str>,
    tokenizer: Option<&Path>,
    tokenizer_url: Option<&str>,
) -> eyre::Result<WhisperModelArtifacts> {
    std::fs::create_dir_all(output_dir)
        .wrap_err_with(|| format!("Failed to create model dir {}", output_dir.display()))?;

    let dims = load_checkpoint_dims(checkpoint)?;
    println!("Loaded checkpoint dims from {}", checkpoint.display());

    let normalized_checkpoint = normalize_checkpoint_for_burn_store(checkpoint, output_dir)?;
    let mut model = import_whisper_model_from_checkpoint(&normalized_checkpoint, &dims)?;
    let burnpack_path = output_dir.join(MODEL_BURNPACK_FILE_NAME);
    save_model_burnpack(&mut model, &burnpack_path, &dims)?;
    let _ = std::fs::remove_file(&normalized_checkpoint);
    write_dims_file(&output_dir.join(MODEL_DIMS_FILE_NAME), &dims)?;
    install_tokenizer_file(output_dir, checkpoint, model_id, tokenizer, tokenizer_url)?;

    inspect_model_dir(output_dir)
}

fn normalize_checkpoint_for_burn_store(
    checkpoint: &Path,
    output_dir: &Path,
) -> eyre::Result<PathBuf> {
    let normalized_checkpoint = output_dir.join("checkpoint.contiguous-fp32.pt");
    let script = r"
import sys, torch
source, destination = sys.argv[1], sys.argv[2]
checkpoint = torch.load(source, map_location='cpu')
state = checkpoint.get('model_state_dict')
if state is None:
    raise RuntimeError('checkpoint is missing model_state_dict')
checkpoint['model_state_dict'] = {
    key: value.detach().float().contiguous() if torch.is_tensor(value) and value.is_floating_point() else value
    for key, value in state.items()
}
torch.save(checkpoint, destination)
";
    let output = Command::new("uv")
        .arg("run")
        .arg("--no-project")
        .arg("--index")
        .arg("https://download.pytorch.org/whl/cu128")
        .arg("--with")
        .arg("torch")
        .arg("python")
        .arg("-c")
        .arg(script)
        .arg(checkpoint)
        .arg(&normalized_checkpoint)
        .output()
        .wrap_err("failed to run Python checkpoint normalization")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "Failed to normalize Whisper checkpoint {} for Burn import: {}",
            checkpoint.display(),
            stderr.trim()
        );
    }

    Ok(normalized_checkpoint)
}

#[must_use]
pub fn default_model_package_path(source: &Path) -> PathBuf {
    let base_name = source
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .filter(|name| !name.is_empty())
        .unwrap_or("model");
    source.with_file_name(format!(
        "{base_name}.teamy-studio-whisper-model.{MODEL_PACKAGE_EXTENSION}"
    ))
}

#[must_use]
pub fn default_checkpoint_package_path(checkpoint: &Path) -> PathBuf {
    let base_name = checkpoint
        .file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .filter(|name| !name.is_empty())
        .unwrap_or("model");
    checkpoint.with_file_name(format!(
        "{base_name}.teamy-studio-whisper-model.{MODEL_PACKAGE_EXTENSION}"
    ))
}

#[must_use]
pub fn default_remote_model_url() -> Option<String> {
    std::env::var(DEFAULT_MODEL_PACKAGE_URL_ENV_VAR)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

/// Register a Whisper model directory for later reuse.
///
/// New entries become the default entry used by `transcribe` when `--model-dir` is omitted.
///
/// # Errors
///
/// This function will return an error if the model directory cannot be canonicalized, inspected,
/// or persisted under the application home directory.
pub fn add_registered_model_dir(app_home: &AppHome, root: &Path) -> eyre::Result<Vec<PathBuf>> {
    let canonical_root = dunce::canonicalize(root)
        .wrap_err_with(|| format!("Failed to canonicalize model directory {}", root.display()))?;
    let _artifacts = inspect_model_dir(&canonical_root)?;

    let mut model_dirs = list_registered_model_dirs(app_home)?;
    model_dirs.retain(|existing| existing != &canonical_root);
    model_dirs.insert(0, canonical_root);
    write_registered_model_dirs(app_home, &model_dirs)?;
    Ok(model_dirs)
}

/// Remove a previously-registered Whisper model directory.
///
/// # Errors
///
/// This function will return an error if the registry cannot be updated.
pub fn remove_registered_model_dir(app_home: &AppHome, root: &Path) -> eyre::Result<Vec<PathBuf>> {
    let target = normalize_model_dir_for_lookup(root)?;
    let mut model_dirs = list_registered_model_dirs(app_home)?;
    model_dirs.retain(|existing| existing != &target);
    write_registered_model_dirs(app_home, &model_dirs)?;
    Ok(model_dirs)
}

/// List all registered Whisper model directories.
///
/// The first entry is treated as the default model directory.
///
/// # Errors
///
/// This function will return an error if the registry file cannot be read.
pub fn list_registered_model_dirs(app_home: &AppHome) -> eyre::Result<Vec<PathBuf>> {
    let registry_path = model_dirs_file_path(app_home);
    if !registry_path.is_file() {
        return Ok(Vec::new());
    }

    let contents = std::fs::read_to_string(&registry_path)
        .wrap_err_with(|| format!("Failed to read model registry {}", registry_path.display()))?;
    Ok(contents
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(PathBuf::from)
        .collect())
}

/// Resolve the default registered Whisper model directory, if any.
///
/// # Errors
///
/// This function will return an error if the registry cannot be read.
pub fn resolve_default_model_dir(app_home: &AppHome) -> eyre::Result<Option<PathBuf>> {
    Ok(list_registered_model_dirs(app_home)?.into_iter().next())
}

#[must_use]
pub fn managed_model_dir(cache_home: &CacheHome, model_name: &str) -> PathBuf {
    managed_models_dir(cache_home).join(model_name)
}

#[must_use]
pub fn known_model_checkpoint_path(cache_home: &CacheHome, model_name: &str) -> PathBuf {
    managed_model_downloads_dir(cache_home).join(format!("{model_name}.pt"))
}

#[must_use]
pub fn inspect_whisper_model_preparation(
    app_home: &AppHome,
    cache_home: &CacheHome,
    model_name: &str,
) -> WhisperModelPreparationStatus {
    let managed_dir = managed_model_dir(cache_home, model_name);
    let checkpoint_path = known_model_checkpoint_path(cache_home, model_name);
    let compatible = inspect_model_dir(&managed_dir).is_ok();
    let checkpoint_exists = checkpoint_path.is_file();
    let mut locations = vec![
        WhisperModelLocationStatus {
            label: "Compatible Teamy model directory".to_owned(),
            path: managed_dir,
            exists: compatible || managed_model_dir(cache_home, model_name).exists(),
            compatible,
        },
        WhisperModelLocationStatus {
            label: "Downloaded Python checkpoint".to_owned(),
            path: checkpoint_path,
            exists: checkpoint_exists,
            compatible: false,
        },
    ];
    if let Ok(registered) = list_registered_model_dirs(app_home) {
        locations.extend(registered.into_iter().map(|path| {
            let compatible = inspect_model_dir(&path).is_ok();
            WhisperModelLocationStatus {
                label: "Registered model directory".to_owned(),
                exists: path.exists(),
                path,
                compatible,
            }
        }));
    }
    let state = if compatible {
        WhisperModelPreparationState::Compatible
    } else if checkpoint_exists {
        WhisperModelPreparationState::DownloadedUnprocessed
    } else {
        WhisperModelPreparationState::Missing
    };
    WhisperModelPreparationStatus {
        model_name: model_name.to_owned(),
        state,
        locations,
    }
}

/// Resolve the default model path for transcription.
///
/// # Errors
///
/// This function will return an error if the registry cannot be read.
pub fn resolve_transcription_model_dir(
    app_home: &AppHome,
    cache_home: &CacheHome,
    model_name: Option<&str>,
    explicit_model_dir: Option<&Path>,
) -> eyre::Result<PathBuf> {
    if let Some(model_dir) = explicit_model_dir {
        return Ok(model_dir.to_path_buf());
    }
    if let Some(model_name) = model_name.filter(|value| !value.trim().is_empty()) {
        return Ok(managed_model_dir(cache_home, model_name.trim()));
    }
    if let Some(registered) = resolve_default_model_dir(app_home)? {
        return Ok(registered);
    }
    Ok(managed_model_dir(
        cache_home,
        DEFAULT_TRANSCRIPTION_MODEL_NAME,
    ))
}

/// Prepare one known OpenAI Whisper checkpoint into Teamy's managed cache model directory.
///
/// # Errors
///
/// This function will return an error if the model name is unknown, the checkpoint cannot be
/// downloaded, the checkpoint cannot be converted, or the converted model cannot be registered.
pub fn prepare_known_whisper_model(
    app_home: &AppHome,
    cache_home: &CacheHome,
    model_name: &str,
    overwrite: bool,
) -> eyre::Result<PreparedNamedModel> {
    let known = known_whisper_model(model_name).ok_or_else(|| {
        eyre::eyre!(
            "Unknown Whisper model `{model_name}`. Known models: {}",
            KNOWN_WHISPER_MODELS
                .iter()
                .map(|model| model.name)
                .collect::<Vec<_>>()
                .join(", ")
        )
    })?;
    let checkpoint = download_known_whisper_checkpoint(cache_home, known)?;
    let managed_dir = managed_model_dir(cache_home, known.name);
    if managed_dir.exists() {
        if overwrite {
            std::fs::remove_dir_all(&managed_dir).wrap_err_with(|| {
                format!("Failed to replace managed model {}", managed_dir.display())
            })?;
        } else {
            let artifacts = inspect_model_dir(&managed_dir)?;
            let registered_model_dirs = add_registered_model_dir(app_home, &managed_dir)?;
            return Ok(PreparedNamedModel {
                managed_dir,
                artifacts,
                registered_model_dirs,
            });
        }
    }

    let artifacts = convert_checkpoint_to_model_dir(
        &checkpoint,
        &managed_dir,
        Some(known.hugging_face_model_id),
        None,
        None,
    )?;
    let registered_model_dirs = add_registered_model_dir(app_home, &managed_dir)?;
    Ok(PreparedNamedModel {
        managed_dir,
        artifacts,
        registered_model_dirs,
    })
}

#[must_use]
pub fn render_registered_model_dirs(model_dirs: &[PathBuf]) -> String {
    if model_dirs.is_empty() {
        return "No registered model directories.".to_owned();
    }

    model_dirs
        .iter()
        .enumerate()
        .map(|(index, path)| {
            if index == 0 {
                format!("* {} (default)", path.display())
            } else {
                format!("* {}", path.display())
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[must_use]
pub fn managed_models_dir(cache_home: &CacheHome) -> PathBuf {
    cache_home.0.join(MANAGED_MODELS_DIR_NAME)
}

#[must_use]
pub fn managed_model_downloads_dir(cache_home: &CacheHome) -> PathBuf {
    managed_models_dir(cache_home).join(MANAGED_MODEL_DOWNLOADS_DIR_NAME)
}

#[must_use]
pub fn managed_model_conversions_dir(cache_home: &CacheHome) -> PathBuf {
    managed_models_dir(cache_home).join(MANAGED_MODEL_CONVERSIONS_DIR_NAME)
}

fn known_whisper_model(model_name: &str) -> Option<&'static KnownWhisperModel> {
    let requested = model_name.trim();
    KNOWN_WHISPER_MODELS
        .iter()
        .find(|model| model.name.eq_ignore_ascii_case(requested))
}

fn download_known_whisper_checkpoint(
    cache_home: &CacheHome,
    known: &KnownWhisperModel,
) -> eyre::Result<PathBuf> {
    let downloads_dir = managed_model_downloads_dir(cache_home);
    std::fs::create_dir_all(&downloads_dir).wrap_err_with(|| {
        format!(
            "Failed to create managed model downloads dir {}",
            downloads_dir.display()
        )
    })?;
    let checkpoint = downloads_dir.join(format!("{}.pt", known.name));
    if checkpoint.is_file() {
        return Ok(checkpoint);
    }
    download_to_file(known.checkpoint_url, &checkpoint)?;
    Ok(checkpoint)
}

fn validate_prepared_model_dir(root: &Path) -> eyre::Result<WhisperModelArtifacts> {
    let artifacts = inspect_model_dir(root)?;
    if artifacts.dims.is_none() {
        bail!(
            "Model directory is inspectable but not transcribe-ready because Whisper dimension inference failed: {}",
            root.display()
        );
    }
    let _prompt = crate::whisper::default_decoder_prompt_token_ids(&artifacts)?;
    if matches!(artifacts.layout, WhisperModelLayout::BurnPack) {
        let _model = crate::whisper::load_whisper_model_from_artifacts(&artifacts)?;
    }
    Ok(artifacts)
}

fn sanitized_install_name(source: &Path, requested_name: Option<&str>) -> eyre::Result<String> {
    let raw = requested_name
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            source
                .file_stem()
                .and_then(std::ffi::OsStr::to_str)
                .map(ToOwned::to_owned)
        })
        .or_else(|| {
            source
                .file_name()
                .and_then(std::ffi::OsStr::to_str)
                .map(ToOwned::to_owned)
        })
        .ok_or_else(|| {
            eyre::eyre!(
                "Could not derive a managed model name from {}",
                source.display()
            )
        })?;

    let sanitized = raw
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_owned();
    if sanitized.is_empty() {
        bail!(
            "Managed model name resolved to an empty string for {}",
            source.display()
        );
    }
    Ok(sanitized)
}

fn is_supported_model_package(path: &Path) -> bool {
    path.extension()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|ext| ext.eq_ignore_ascii_case(MODEL_PACKAGE_EXTENSION))
}

fn managed_model_conversion_dir(
    cache_home: &CacheHome,
    checkpoint: &Path,
) -> eyre::Result<PathBuf> {
    let conversions_dir = managed_model_conversions_dir(cache_home);
    std::fs::create_dir_all(&conversions_dir).wrap_err_with(|| {
        format!(
            "Failed to create managed model conversions dir {}",
            conversions_dir.display()
        )
    })?;

    let name = sanitized_install_name(checkpoint, None)?;
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .wrap_err("System clock is before the Unix epoch")?
        .as_millis();
    Ok(conversions_dir.join(format!("{name}-{timestamp}-{}", std::process::id())))
}

#[derive(Debug, Deserialize)]
struct WhisperCheckpointDims {
    n_mels: usize,
    n_vocab: usize,
    n_audio_ctx: usize,
    n_audio_state: usize,
    n_audio_head: usize,
    n_audio_layer: usize,
    n_text_ctx: usize,
    n_text_state: usize,
    n_text_head: usize,
    n_text_layer: usize,
}

impl WhisperCheckpointDims {
    fn into_whisper_dims(self) -> WhisperDims {
        WhisperDims {
            audio: crate::whisper::AudioEncoderDims {
                n_mels: self.n_mels,
                n_audio_ctx: self.n_audio_ctx,
                n_audio_state: self.n_audio_state,
                n_audio_head: self.n_audio_head,
                n_audio_layer: self.n_audio_layer,
            },
            text: crate::whisper::TextDecoderDims {
                n_vocab: self.n_vocab,
                n_text_ctx: self.n_text_ctx,
                n_text_state: self.n_text_state,
                n_text_head: self.n_text_head,
                n_text_layer: self.n_text_layer,
            },
        }
    }
}

fn load_checkpoint_dims(checkpoint: &Path) -> eyre::Result<WhisperDims> {
    let dims: WhisperCheckpointDims = PytorchReader::load_config(checkpoint, Some("dims"))
        .wrap_err_with(|| {
            format!(
                "Failed to read Whisper checkpoint dims from {}",
                checkpoint.display()
            )
        })?;
    Ok(dims.into_whisper_dims())
}

fn import_whisper_model_from_checkpoint(
    checkpoint: &Path,
    dims: &WhisperDims,
) -> eyre::Result<WhisperModel<WhisperCpuBackend>> {
    let device = Default::default();
    let config = WhisperModelConfig {
        audio: crate::whisper::WhisperAudioEncoderConfig::from_dims(&dims.audio),
        text: crate::whisper::WhisperTextDecoderConfig::from_dims(&dims.text),
    };
    let mut model = config.init::<WhisperCpuBackend>(&device);

    let mut store = PytorchStore::from_file(checkpoint)
        .with_top_level_key("model_state_dict")
        .with_key_remapping(
            r"^encoder\.blocks\.(\d+)\.mlp\.0\.",
            "encoder.blocks.$1.mlp.lin1.",
        )
        .with_key_remapping(
            r"^encoder\.blocks\.(\d+)\.mlp\.2\.",
            "encoder.blocks.$1.mlp.lin2.",
        )
        .with_key_remapping(
            r"^decoder\.blocks\.(\d+)\.mlp\.0\.",
            "decoder.blocks.$1.mlp.lin1.",
        )
        .with_key_remapping(
            r"^decoder\.blocks\.(\d+)\.mlp\.2\.",
            "decoder.blocks.$1.mlp.lin2.",
        )
        .with_key_remapping(r"^(.*\.attn_ln)\.weight$", "$1.gamma")
        .with_key_remapping(r"^(.*\.attn_ln)\.bias$", "$1.beta")
        .with_key_remapping(r"^(.*\.cross_attn_ln)\.weight$", "$1.gamma")
        .with_key_remapping(r"^(.*\.cross_attn_ln)\.bias$", "$1.beta")
        .with_key_remapping(r"^(.*\.mlp_ln)\.weight$", "$1.gamma")
        .with_key_remapping(r"^(.*\.mlp_ln)\.bias$", "$1.beta")
        .with_key_remapping(r"^encoder\.ln_post\.weight$", "encoder.ln_post.gamma")
        .with_key_remapping(r"^encoder\.ln_post\.bias$", "encoder.ln_post.beta")
        .with_key_remapping(r"^decoder\.ln\.weight$", "decoder.ln.gamma")
        .with_key_remapping(r"^decoder\.ln\.bias$", "decoder.ln.beta")
        .allow_partial(true);

    println!("Importing weights from checkpoint into Burn model");
    let result = model.load_from(&mut store).wrap_err_with(|| {
        format!(
            "Failed to import Whisper checkpoint weights from {}",
            checkpoint.display()
        )
    })?;

    if !result.errors.is_empty() {
        bail!(
            "Checkpoint import from {} reported tensor errors after remapping: {:?}",
            checkpoint.display(),
            result.errors,
        );
    }

    let allowed_missing = ["decoder.mask"];
    let unexpected_missing = result
        .missing
        .iter()
        .filter(|path| !allowed_missing.iter().any(|allowed| path == allowed))
        .cloned()
        .collect::<Vec<_>>();
    if !unexpected_missing.is_empty() {
        bail!(
            "Checkpoint import from {} left unexpected missing tensors: {:?}",
            checkpoint.display(),
            unexpected_missing,
        );
    }
    if !result.unused.is_empty() {
        bail!(
            "Checkpoint import from {} left unused tensors after remapping: {:?}",
            checkpoint.display(),
            result.unused,
        );
    }

    Ok(model)
}

fn save_model_burnpack(
    model: &mut WhisperModel<WhisperCpuBackend>,
    burnpack_path: &Path,
    dims: &WhisperDims,
) -> eyre::Result<()> {
    let mut store = BurnpackStore::from_file(burnpack_path)
        .overwrite(true)
        .metadata("whisper.audio.n_mels", dims.audio.n_mels.to_string())
        .metadata(
            "whisper.audio.n_audio_ctx",
            dims.audio.n_audio_ctx.to_string(),
        )
        .metadata(
            "whisper.audio.n_audio_state",
            dims.audio.n_audio_state.to_string(),
        )
        .metadata(
            "whisper.audio.n_audio_head",
            dims.audio.n_audio_head.to_string(),
        )
        .metadata(
            "whisper.audio.n_audio_layer",
            dims.audio.n_audio_layer.to_string(),
        )
        .metadata("whisper.text.n_vocab", dims.text.n_vocab.to_string())
        .metadata("whisper.text.n_text_ctx", dims.text.n_text_ctx.to_string())
        .metadata(
            "whisper.text.n_text_state",
            dims.text.n_text_state.to_string(),
        )
        .metadata(
            "whisper.text.n_text_head",
            dims.text.n_text_head.to_string(),
        )
        .metadata(
            "whisper.text.n_text_layer",
            dims.text.n_text_layer.to_string(),
        );
    println!("Saving Burnpack weights: {}", burnpack_path.display());
    model
        .save_into(&mut store)
        .wrap_err_with(|| format!("Failed to write Burnpack model {}", burnpack_path.display()))
}

fn write_dims_file(path: &Path, dims: &WhisperDims) -> eyre::Result<()> {
    let json =
        serde_json::to_string_pretty(dims).wrap_err("Failed to serialize Whisper dims to JSON")?;
    std::fs::write(path, json)
        .wrap_err_with(|| format!("Failed to write dims file {}", path.display()))
}

fn read_dims_file(path: &Path) -> eyre::Result<WhisperDims> {
    let json = std::fs::read_to_string(path)
        .wrap_err_with(|| format!("Failed to read dims file {}", path.display()))?;
    serde_json::from_str(&json)
        .wrap_err_with(|| format!("Failed to parse dims file {}", path.display()))
}

fn install_tokenizer_file(
    output_dir: &Path,
    checkpoint_path: &Path,
    model_id: Option<&str>,
    tokenizer_path: Option<&Path>,
    tokenizer_url: Option<&str>,
) -> eyre::Result<()> {
    let output_tokenizer = output_dir.join(TOKENIZER_FILE_NAME);
    if let Some(tokenizer_path) = tokenizer_path {
        println!("Copying tokenizer: {}", tokenizer_path.display());
        std::fs::copy(tokenizer_path, &output_tokenizer).wrap_err_with(|| {
            format!(
                "Failed to copy tokenizer {} to {}",
                tokenizer_path.display(),
                output_tokenizer.display()
            )
        })?;
        return Ok(());
    }

    let resolved_url = if let Some(url) = tokenizer_url.filter(|value| !value.trim().is_empty()) {
        url.trim().to_owned()
    } else {
        let inferred_model_id = model_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| infer_whisper_model_id(checkpoint_path));
        format!(
            "https://huggingface.co/{}/resolve/main/{}",
            inferred_model_id, TOKENIZER_FILE_NAME
        )
    };

    println!("Downloading tokenizer: {resolved_url}");
    download_to_file(&resolved_url, &output_tokenizer)
}

fn infer_whisper_model_id(checkpoint_path: &Path) -> String {
    let stem = checkpoint_path
        .file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    let known = [
        ("tiny", "openai/whisper-tiny"),
        ("tiny.en", "openai/whisper-tiny.en"),
        ("base", "openai/whisper-base"),
        ("base.en", "openai/whisper-base.en"),
        ("small", "openai/whisper-small"),
        ("small.en", "openai/whisper-small.en"),
        ("medium", "openai/whisper-medium"),
        ("medium.en", "openai/whisper-medium.en"),
        ("large-v3-turbo", "openai/whisper-large-v3-turbo"),
        ("large-v3", "openai/whisper-large-v3"),
        ("large-v2", "openai/whisper-large-v2"),
        ("large-v1", "openai/whisper-large-v1"),
        ("large", "openai/whisper-large"),
    ];

    for (name, model_id) in known {
        if stem == name || stem.contains(name) {
            return model_id.to_owned();
        }
    }

    format!("openai/whisper-{stem}")
}

fn download_to_file(url: &str, destination: &Path) -> eyre::Result<()> {
    let client = reqwest::blocking::Client::builder()
        .build()
        .wrap_err("Failed to build HTTP client")?;
    let mut response = client
        .get(url)
        .send()
        .wrap_err_with(|| format!("Failed to download {url}"))?
        .error_for_status()
        .wrap_err_with(|| format!("Download returned an error for {url}"))?;
    let mut output = std::fs::File::create(destination)
        .wrap_err_with(|| format!("Failed to create {}", destination.display()))?;
    std::io::copy(&mut response, &mut output)
        .wrap_err_with(|| format!("Failed to write {}", destination.display()))?;
    Ok(())
}

fn download_model_package(
    cache_home: &CacheHome,
    url: &str,
    install_name: Option<&str>,
) -> eyre::Result<PathBuf> {
    let downloads_dir = managed_model_downloads_dir(cache_home);
    std::fs::create_dir_all(&downloads_dir).wrap_err_with(|| {
        format!(
            "Failed to create managed model downloads dir {}",
            downloads_dir.display()
        )
    })?;

    let file_name = downloaded_archive_file_name(url, install_name);
    let destination = downloads_dir.join(file_name);

    let client = reqwest::blocking::Client::builder()
        .build()
        .wrap_err("Failed to build HTTP client for model download")?;
    let mut response = client
        .get(url)
        .send()
        .wrap_err_with(|| format!("Failed to download model package from {url}"))?
        .error_for_status()
        .wrap_err_with(|| format!("Model package download returned an error for {url}"))?;
    let mut output = std::fs::File::create(&destination).wrap_err_with(|| {
        format!(
            "Failed to create downloaded model archive {}",
            destination.display()
        )
    })?;
    std::io::copy(&mut response, &mut output).wrap_err_with(|| {
        format!(
            "Failed to write downloaded model archive {}",
            destination.display()
        )
    })?;

    Ok(destination)
}

fn downloaded_archive_file_name(url: &str, install_name: Option<&str>) -> String {
    if let Some(name) = install_name.filter(|name| !name.trim().is_empty()) {
        return format!("{}.{}", name.trim(), MODEL_PACKAGE_EXTENSION);
    }

    let last_segment = url
        .rsplit('/')
        .next()
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .unwrap_or("model-package");
    if last_segment.ends_with(&format!(".{MODEL_PACKAGE_EXTENSION}")) {
        last_segment.to_owned()
    } else {
        format!("{last_segment}.{MODEL_PACKAGE_EXTENSION}")
    }
}

fn unpack_model_archive(source: &Path, destination: &Path) -> eyre::Result<()> {
    std::fs::create_dir_all(destination).wrap_err_with(|| {
        format!(
            "Failed to create model install dir {}",
            destination.display()
        )
    })?;
    let file = std::fs::File::open(source)
        .wrap_err_with(|| format!("Failed to open model archive {}", source.display()))?;
    let mut archive = zip::ZipArchive::new(file)
        .wrap_err_with(|| format!("Failed to open ZIP archive {}", source.display()))?;

    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).wrap_err_with(|| {
            format!("Failed to read ZIP entry {index} from {}", source.display())
        })?;
        let enclosed_name = entry.enclosed_name().ok_or_else(|| {
            eyre::eyre!(
                "ZIP archive {} contains an invalid path at entry {}",
                source.display(),
                index
            )
        })?;
        let output_path = destination.join(enclosed_name);

        if entry.name().ends_with('/') {
            std::fs::create_dir_all(&output_path).wrap_err_with(|| {
                format!(
                    "Failed to create directory {} while unpacking",
                    output_path.display()
                )
            })?;
            continue;
        }

        if let Some(parent) = output_path.parent() {
            std::fs::create_dir_all(parent).wrap_err_with(|| {
                format!(
                    "Failed to create parent dir {} while unpacking",
                    parent.display()
                )
            })?;
        }

        let mut output = std::fs::File::create(&output_path).wrap_err_with(|| {
            format!("Failed to create unpacked file {}", output_path.display())
        })?;
        std::io::copy(&mut entry, &mut output)
            .wrap_err_with(|| format!("Failed to write unpacked file {}", output_path.display()))?;
    }

    Ok(())
}

fn copy_dir_recursive(source: &Path, destination: &Path) -> eyre::Result<()> {
    std::fs::create_dir_all(destination).wrap_err_with(|| {
        format!(
            "Failed to create destination model dir {}",
            destination.display()
        )
    })?;

    for entry in walkdir::WalkDir::new(source) {
        let entry =
            entry.wrap_err_with(|| format!("Failed to walk source dir {}", source.display()))?;
        let path = entry.path();
        let relative = path
            .strip_prefix(source)
            .wrap_err_with(|| format!("Failed to derive relative path for {}", path.display()))?;
        if relative.as_os_str().is_empty() {
            continue;
        }
        let target = destination.join(relative);

        if entry.file_type().is_dir() {
            std::fs::create_dir_all(&target)
                .wrap_err_with(|| format!("Failed to create dir {}", target.display()))?;
        } else {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).wrap_err_with(|| {
                    format!("Failed to create parent dir {}", parent.display())
                })?;
            }
            std::fs::copy(path, &target).wrap_err_with(|| {
                format!("Failed to copy {} to {}", path.display(), target.display())
            })?;
        }
    }

    Ok(())
}

fn write_registered_model_dirs(app_home: &AppHome, model_dirs: &[PathBuf]) -> eyre::Result<()> {
    app_home.ensure_dir()?;
    let registry_path = model_dirs_file_path(app_home);

    if model_dirs.is_empty() {
        if registry_path.exists() {
            std::fs::remove_file(&registry_path).wrap_err_with(|| {
                format!(
                    "Failed to remove empty model registry {}",
                    registry_path.display()
                )
            })?;
        }
        return Ok(());
    }

    let contents = model_dirs
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&registry_path, format!("{contents}\n"))
        .wrap_err_with(|| format!("Failed to write model registry {}", registry_path.display()))
}

fn model_dirs_file_path(app_home: &AppHome) -> PathBuf {
    app_home.file_path(MODEL_DIRS_FILE_NAME)
}

fn normalize_model_dir_for_lookup(path: &Path) -> eyre::Result<PathBuf> {
    if path.exists() {
        return dunce::canonicalize(path).wrap_err_with(|| {
            format!("Failed to canonicalize model directory {}", path.display())
        });
    }

    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .wrap_err("Failed to resolve current working directory for model removal")?
            .join(path)
    };
    Ok(dunce::simplified(&absolute).to_path_buf())
}

fn ensure_existing_dir(path: &Path) -> eyre::Result<()> {
    if !path.exists() {
        bail!("Model directory does not exist: {}", path.display());
    }
    if !path.is_dir() {
        bail!("Model path is not a directory: {}", path.display());
    }
    Ok(())
}

fn ensure_whisper_burn_dir(path: &Path, name: &str) -> eyre::Result<()> {
    if !path.is_dir() {
        bail!(
            "Model directory is missing the `{}` subdirectory required by the whisper-burn layout: {}",
            name,
            path.display()
        );
    }

    let entries = std::fs::read_dir(path)
        .wrap_err_with(|| format!("Failed to read {} directory {}", name, path.display()))?;
    let has_any_entries = entries.into_iter().next().transpose()?.is_some();
    if !has_any_entries {
        bail!(
            "Model directory has an empty `{}` subdirectory: {}",
            name,
            path.display()
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        MODEL_BURNPACK_FILE_NAME, MODEL_DIMS_FILE_NAME, WhisperModelLayout,
        add_registered_model_dir, inspect_model_dir, list_registered_model_dirs,
        remove_registered_model_dir, resolve_default_model_dir,
    };
    use crate::paths::AppHome;
    use crate::whisper::{AudioEncoderDims, TextDecoderDims, WhisperDims};
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tokenizers::{Tokenizer, models::bpe::BPE};

    fn unique_temp_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("teamy-studio-whisper-{name}-{nanos}"))
    }

    #[test]
    fn inspect_model_dir_accepts_minimal_whisper_burn_layout() {
        let root = unique_temp_dir("model-inspect");
        create_minimal_model_dir(&root);

        let artifacts = inspect_model_dir(&root).expect("layout should inspect successfully");
        assert_eq!(artifacts.layout, WhisperModelLayout::WhisperBurnNpy);
        assert_eq!(artifacts.tokenizer.vocab_size, 0);

        std::fs::remove_dir_all(&root).expect("temp dir should be removable");
    }

    #[test]
    fn inspect_model_dir_accepts_minimal_burnpack_layout() {
        let root = unique_temp_dir("model-burnpack-inspect");
        create_minimal_burnpack_model_dir(&root);

        let artifacts =
            inspect_model_dir(&root).expect("burnpack layout should inspect successfully");
        assert_eq!(artifacts.layout, WhisperModelLayout::BurnPack);
        assert_eq!(artifacts.tokenizer.vocab_size, 0);
        assert_eq!(
            artifacts
                .dims
                .as_ref()
                .expect("dims should exist")
                .audio
                .n_mels,
            80
        );

        std::fs::remove_dir_all(&root).expect("temp dir should be removable");
    }

    #[test]
    fn registered_model_dirs_roundtrip_and_default_to_most_recent() {
        let home_root = unique_temp_dir("model-registry-home");
        let app_home = AppHome(home_root.clone());
        let model_a = unique_temp_dir("model-a");
        let model_b = unique_temp_dir("model-b");
        create_minimal_model_dir(&model_a);
        create_minimal_model_dir(&model_b);

        let registered =
            add_registered_model_dir(&app_home, &model_a).expect("first model should register");
        assert_eq!(registered.len(), 1);

        let registered =
            add_registered_model_dir(&app_home, &model_b).expect("second model should register");
        assert_eq!(registered.len(), 2);
        assert_eq!(
            registered[0],
            dunce::canonicalize(&model_b).expect("model b should canonicalize")
        );

        let listed = list_registered_model_dirs(&app_home).expect("list should read registry");
        assert_eq!(listed, registered);

        let default = resolve_default_model_dir(&app_home).expect("default should resolve");
        assert_eq!(default, Some(registered[0].clone()));

        let remaining =
            remove_registered_model_dir(&app_home, &model_b).expect("remove should succeed");
        assert_eq!(remaining.len(), 1);
        assert_eq!(
            remaining[0],
            dunce::canonicalize(&model_a).expect("model a should canonicalize")
        );

        std::fs::remove_dir_all(&model_a).expect("temp model a should be removable");
        std::fs::remove_dir_all(&model_b).expect("temp model b should be removable");
        std::fs::remove_dir_all(&home_root).expect("temp home should be removable");
    }

    fn create_minimal_model_dir(root: &std::path::Path) {
        let encoder = root.join("encoder");
        let decoder = root.join("decoder");
        std::fs::create_dir_all(&encoder).expect("encoder dir should be creatable");
        std::fs::create_dir_all(&decoder).expect("decoder dir should be creatable");

        std::fs::write(encoder.join("placeholder.npy"), b"x").expect("encoder file should write");
        std::fs::write(decoder.join("placeholder.npy"), b"x").expect("decoder file should write");
        Tokenizer::new(BPE::default())
            .save(root.join("tokenizer.json"), false)
            .expect("tokenizer fixture should save");
    }

    fn create_minimal_burnpack_model_dir(root: &std::path::Path) {
        std::fs::create_dir_all(root).expect("burnpack root should be creatable");
        std::fs::write(root.join(MODEL_BURNPACK_FILE_NAME), b"not-a-real-burnpack")
            .expect("burnpack placeholder should write");
        std::fs::write(
            root.join(MODEL_DIMS_FILE_NAME),
            serde_json::to_string_pretty(&WhisperDims {
                audio: AudioEncoderDims {
                    n_mels: 80,
                    n_audio_ctx: 1500,
                    n_audio_state: 768,
                    n_audio_head: 12,
                    n_audio_layer: 12,
                },
                text: TextDecoderDims {
                    n_vocab: 51865,
                    n_text_ctx: 448,
                    n_text_state: 768,
                    n_text_head: 12,
                    n_text_layer: 12,
                },
            })
            .expect("dims fixture should serialize"),
        )
        .expect("dims fixture should write");
        Tokenizer::new(BPE::default())
            .save(root.join("tokenizer.json"), false)
            .expect("tokenizer fixture should save");
    }
}

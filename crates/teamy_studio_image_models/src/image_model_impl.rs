use crate::paths::CacheHome;
use burn::{
    backend::{Cuda, NdArray, cuda::CudaDevice},
    module::{Module, Param, ParamId},
    nn::{
        Linear, LinearConfig,
        conv::{Conv2d, Conv2dConfig},
    },
    tensor::{
        Int, Tensor, TensorData,
        activation::{gelu, leaky_relu, softmax},
        backend::Backend,
    },
};
use burn_store::pytorch::PytorchReader;
use burn_store::{BurnpackStore, ModuleSnapshot, PytorchStore};
use eyre::{WrapErr, bail};
use facet::Facet;
use serde::Serialize;
use std::path::{Component, Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::{Duration, Instant};

pub const IMAGE_MODELS_DIR_NAME: &str = "image";
pub const DEFAULT_IMAGE_MODEL_NAME: &str = "waifu2x-art-2x";
pub const IMAGE_MODEL_METADATA_FILE_NAME: &str = "model-metadata.json";
pub const IMAGE_MODEL_BURNPACK_FILE_NAME: &str = "model.bpk";
pub const IMAGE_MODEL_SOURCE_DIR_NAME: &str = "source";
pub const NUNIF_WAIFU2X_MODEL_ARCHIVE_VERSION: &str = "20250502";
pub const NUNIF_WAIFU2X_MODEL_ARCHIVE_URL: &str = "https://github.com/nagadomi/nunif/releases/download/0.0.0/waifu2x_pretrained_models_20250502.zip";
pub const IMAGE_MODEL_RUNTIME_STATUS_IMPLEMENTED: &str = "implemented";
pub const IMAGE_MODEL_RUNTIME_STATUS_INVENTORY_ONLY: &str = "inventory-only";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KnownImageModel {
    pub name: &'static str,
    pub family: &'static str,
    pub style: &'static str,
    pub method: &'static str,
    pub noise_level: Option<u8>,
    pub scale: u8,
    pub native_scale: u8,
    pub architecture: &'static str,
    pub source_archive_url: &'static str,
    pub source_archive_version: &'static str,
    pub source_checkpoint_path: &'static str,
    pub model_offset: u32,
    pub blend_size: u32,
    pub default_tile_size: u32,
    pub default_batch_size: u32,
    pub input_channels: u32,
    pub output_channels: u32,
    pub parameter_count: Option<u64>,
    pub alpha_behavior: &'static str,
    pub teamy_runtime_status: &'static str,
    pub teamy_runtime_notes: &'static str,
}

#[expect(
    clippy::too_many_arguments,
    reason = "image model inventory rows are declared inline as explicit constants for readability"
)]
const fn known_swin_unet_model(
    style: &'static str,
    name: &'static str,
    method: &'static str,
    noise_level: Option<u8>,
    scale: u8,
    native_scale: u8,
    architecture: &'static str,
    source_checkpoint_path: &'static str,
    parameter_count: Option<u64>,
    teamy_runtime_status: &'static str,
    teamy_runtime_notes: &'static str,
) -> KnownImageModel {
    known_swin_unet_model_with_tiling(
        style,
        name,
        method,
        noise_level,
        scale,
        native_scale,
        architecture,
        source_checkpoint_path,
        16,
        8,
        parameter_count,
        teamy_runtime_status,
        teamy_runtime_notes,
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "image model inventory rows are declared inline as explicit constants for readability"
)]
const fn known_swin_unet_model_with_tiling(
    style: &'static str,
    name: &'static str,
    method: &'static str,
    noise_level: Option<u8>,
    scale: u8,
    native_scale: u8,
    architecture: &'static str,
    source_checkpoint_path: &'static str,
    model_offset: u32,
    blend_size: u32,
    parameter_count: Option<u64>,
    teamy_runtime_status: &'static str,
    teamy_runtime_notes: &'static str,
) -> KnownImageModel {
    KnownImageModel {
        name,
        family: "waifu2x",
        style,
        method,
        noise_level,
        scale,
        native_scale,
        architecture,
        source_archive_url: NUNIF_WAIFU2X_MODEL_ARCHIVE_URL,
        source_archive_version: NUNIF_WAIFU2X_MODEL_ARCHIVE_VERSION,
        source_checkpoint_path,
        model_offset,
        blend_size,
        default_tile_size: 256,
        default_batch_size: 4,
        input_channels: 3,
        output_channels: 3,
        parameter_count,
        alpha_behavior: "nunif-compatible-alpha-border-padding-and-alpha-upscale",
        teamy_runtime_status,
        teamy_runtime_notes,
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "the art inventory helper forwards explicit checkpoint metadata into the shared constructor"
)]
const fn known_swin_unet_art_model(
    name: &'static str,
    method: &'static str,
    noise_level: Option<u8>,
    scale: u8,
    native_scale: u8,
    architecture: &'static str,
    source_checkpoint_path: &'static str,
    parameter_count: Option<u64>,
    teamy_runtime_status: &'static str,
    teamy_runtime_notes: &'static str,
) -> KnownImageModel {
    known_swin_unet_model(
        "art",
        name,
        method,
        noise_level,
        scale,
        native_scale,
        architecture,
        source_checkpoint_path,
        parameter_count,
        teamy_runtime_status,
        teamy_runtime_notes,
    )
}

const fn known_swin_unet_art_native_4x_model(
    name: &'static str,
    method: &'static str,
    noise_level: Option<u8>,
    source_checkpoint_path: &'static str,
    parameter_count: Option<u64>,
    teamy_runtime_status: &'static str,
    teamy_runtime_notes: &'static str,
) -> KnownImageModel {
    known_swin_unet_model_with_tiling(
        "art",
        name,
        method,
        noise_level,
        4,
        4,
        "waifu2x.swin_unet_4x",
        source_checkpoint_path,
        32,
        16,
        parameter_count,
        teamy_runtime_status,
        teamy_runtime_notes,
    )
}

pub const KNOWN_IMAGE_MODELS: [KnownImageModel; 16] = [
    known_swin_unet_art_model(
        DEFAULT_IMAGE_MODEL_NAME,
        "scale2x",
        None,
        2,
        2,
        "waifu2x.swin_unet_2x",
        "pretrained_models/swin_unet/art/scale2x.pth",
        Some(3_758_304),
        IMAGE_MODEL_RUNTIME_STATUS_IMPLEMENTED,
        "Current Teamy Burn prep and runtime support this art 2x checkpoint end to end.",
    ),
    known_swin_unet_model(
        "photo",
        "waifu2x-photo-2x",
        "scale2x",
        None,
        2,
        2,
        "waifu2x.swin_unet_4x",
        "pretrained_models/swin_unet/photo/scale4x.pth",
        None,
        IMAGE_MODEL_RUNTIME_STATUS_IMPLEMENTED,
        "Teamy prepares the upstream photo 4x checkpoint and downscales each 4x tile back to the logical 2x output, matching nunif's derived 2x wrapper.",
    ),
    known_swin_unet_model(
        "art_scan",
        "waifu2x-art-scan-2x",
        "scale2x",
        None,
        2,
        2,
        "waifu2x.swin_unet_4x",
        "pretrained_models/swin_unet/art_scan/scale4x.pth",
        None,
        IMAGE_MODEL_RUNTIME_STATUS_IMPLEMENTED,
        "Teamy prepares the upstream art_scan 4x checkpoint and downscales each 4x tile back to the logical 2x output, matching nunif's derived 2x wrapper.",
    ),
    known_swin_unet_art_native_4x_model(
        "waifu2x-art-4x",
        "scale4x",
        None,
        "pretrained_models/swin_unet/art/scale4x.pth",
        None,
        IMAGE_MODEL_RUNTIME_STATUS_IMPLEMENTED,
        "Current Teamy Burn prep and runtime support this native art 4x checkpoint end to end.",
    ),
    known_swin_unet_art_model(
        "waifu2x-art-denoise-0",
        "noise",
        Some(0),
        1,
        1,
        "waifu2x.swin_unet_1x",
        "pretrained_models/swin_unet/art/noise0.pth",
        None,
        IMAGE_MODEL_RUNTIME_STATUS_INVENTORY_ONLY,
        "Inventory only for now; Teamy has not implemented denoise-only Burn prep/runtime yet.",
    ),
    known_swin_unet_art_model(
        "waifu2x-art-denoise-1",
        "noise",
        Some(1),
        1,
        1,
        "waifu2x.swin_unet_1x",
        "pretrained_models/swin_unet/art/noise1.pth",
        None,
        IMAGE_MODEL_RUNTIME_STATUS_INVENTORY_ONLY,
        "Inventory only for now; Teamy has not implemented denoise-only Burn prep/runtime yet.",
    ),
    known_swin_unet_art_model(
        "waifu2x-art-denoise-2",
        "noise",
        Some(2),
        1,
        1,
        "waifu2x.swin_unet_1x",
        "pretrained_models/swin_unet/art/noise2.pth",
        None,
        IMAGE_MODEL_RUNTIME_STATUS_INVENTORY_ONLY,
        "Inventory only for now; Teamy has not implemented denoise-only Burn prep/runtime yet.",
    ),
    known_swin_unet_art_model(
        "waifu2x-art-denoise-3",
        "noise",
        Some(3),
        1,
        1,
        "waifu2x.swin_unet_1x",
        "pretrained_models/swin_unet/art/noise3.pth",
        None,
        IMAGE_MODEL_RUNTIME_STATUS_INVENTORY_ONLY,
        "Inventory only for now; Teamy has not implemented denoise-only Burn prep/runtime yet.",
    ),
    known_swin_unet_art_model(
        "waifu2x-art-denoise-0-2x",
        "noise_scale2x",
        Some(0),
        2,
        2,
        "waifu2x.swin_unet_2x",
        "pretrained_models/swin_unet/art/noise0_scale2x.pth",
        Some(3_758_304),
        IMAGE_MODEL_RUNTIME_STATUS_IMPLEMENTED,
        "Current Teamy Burn prep and runtime support this art denoise+2x checkpoint end to end.",
    ),
    known_swin_unet_art_model(
        "waifu2x-art-denoise-1-2x",
        "noise_scale2x",
        Some(1),
        2,
        2,
        "waifu2x.swin_unet_2x",
        "pretrained_models/swin_unet/art/noise1_scale2x.pth",
        Some(3_758_304),
        IMAGE_MODEL_RUNTIME_STATUS_IMPLEMENTED,
        "Current Teamy Burn prep and runtime support this art denoise+2x checkpoint end to end.",
    ),
    known_swin_unet_art_model(
        "waifu2x-art-denoise-2-2x",
        "noise_scale2x",
        Some(2),
        2,
        2,
        "waifu2x.swin_unet_2x",
        "pretrained_models/swin_unet/art/noise2_scale2x.pth",
        Some(3_758_304),
        IMAGE_MODEL_RUNTIME_STATUS_IMPLEMENTED,
        "Current Teamy Burn prep and runtime support this art denoise+2x checkpoint end to end.",
    ),
    known_swin_unet_art_model(
        "waifu2x-art-denoise-3-2x",
        "noise_scale2x",
        Some(3),
        2,
        2,
        "waifu2x.swin_unet_2x",
        "pretrained_models/swin_unet/art/noise3_scale2x.pth",
        Some(3_758_304),
        IMAGE_MODEL_RUNTIME_STATUS_IMPLEMENTED,
        "Current Teamy Burn prep and runtime support this art denoise+2x checkpoint end to end.",
    ),
    known_swin_unet_art_native_4x_model(
        "waifu2x-art-denoise-0-4x",
        "noise_scale4x",
        Some(0),
        "pretrained_models/swin_unet/art/noise0_scale4x.pth",
        None,
        IMAGE_MODEL_RUNTIME_STATUS_IMPLEMENTED,
        "Current Teamy Burn prep and runtime support this native art denoise+4x checkpoint end to end.",
    ),
    known_swin_unet_art_native_4x_model(
        "waifu2x-art-denoise-1-4x",
        "noise_scale4x",
        Some(1),
        "pretrained_models/swin_unet/art/noise1_scale4x.pth",
        None,
        IMAGE_MODEL_RUNTIME_STATUS_IMPLEMENTED,
        "Current Teamy Burn prep and runtime support this native art denoise+4x checkpoint end to end.",
    ),
    known_swin_unet_art_native_4x_model(
        "waifu2x-art-denoise-2-4x",
        "noise_scale4x",
        Some(2),
        "pretrained_models/swin_unet/art/noise2_scale4x.pth",
        None,
        IMAGE_MODEL_RUNTIME_STATUS_IMPLEMENTED,
        "Current Teamy Burn prep and runtime support this native art denoise+4x checkpoint end to end.",
    ),
    known_swin_unet_art_native_4x_model(
        "waifu2x-art-denoise-3-4x",
        "noise_scale4x",
        Some(3),
        "pretrained_models/swin_unet/art/noise3_scale4x.pth",
        None,
        IMAGE_MODEL_RUNTIME_STATUS_IMPLEMENTED,
        "Current Teamy Burn prep and runtime support this native art denoise+4x checkpoint end to end.",
    ),
];

#[derive(Clone, Debug, Facet, PartialEq, Serialize)]
#[facet(rename_all = "snake_case")]
pub struct ImageModelMetadata {
    pub model_name: String,
    pub family: String,
    pub style: String,
    pub method: String,
    pub noise_level: Option<u8>,
    pub scale: u8,
    pub native_scale: u8,
    pub architecture: String,
    pub source_archive_url: String,
    pub source_archive_version: String,
    pub source_checkpoint_path: String,
    pub model_offset: u32,
    pub blend_size: u32,
    pub default_tile_size: u32,
    pub default_batch_size: u32,
    pub input_channels: u32,
    pub output_channels: u32,
    pub parameter_count: Option<u64>,
    pub alpha_behavior: String,
    pub teamy_runtime_status: String,
    pub teamy_runtime_notes: String,
}

impl From<&KnownImageModel> for ImageModelMetadata {
    fn from(model: &KnownImageModel) -> Self {
        Self {
            model_name: model.name.to_owned(),
            family: model.family.to_owned(),
            style: model.style.to_owned(),
            method: model.method.to_owned(),
            noise_level: model.noise_level,
            scale: model.scale,
            native_scale: model.native_scale,
            architecture: model.architecture.to_owned(),
            source_archive_url: model.source_archive_url.to_owned(),
            source_archive_version: model.source_archive_version.to_owned(),
            source_checkpoint_path: model.source_checkpoint_path.to_owned(),
            model_offset: model.model_offset,
            blend_size: model.blend_size,
            default_tile_size: model.default_tile_size,
            default_batch_size: model.default_batch_size,
            input_channels: model.input_channels,
            output_channels: model.output_channels,
            parameter_count: model.parameter_count,
            alpha_behavior: model.alpha_behavior.to_owned(),
            teamy_runtime_status: model.teamy_runtime_status.to_owned(),
            teamy_runtime_notes: model.teamy_runtime_notes.to_owned(),
        }
    }
}

#[derive(Clone, Copy, Debug, Facet, PartialEq, Eq)]
#[repr(u8)]
pub enum ImageModelPreparationState {
    Missing,
    MetadataOnly,
    Prepared,
}

#[derive(Clone, Debug, Facet, PartialEq, Eq)]
pub struct ImageModelStatusReport {
    pub name: String,
    pub managed_dir: String,
    pub metadata_path: String,
    pub burnpack_path: String,
    pub state: ImageModelPreparationState,
    pub metadata_exists: bool,
    pub burnpack_exists: bool,
}

#[derive(Clone, Debug, Facet, PartialEq, Eq)]
pub struct ImageModelSourceArtifactsReport {
    pub source_root: String,
    pub archive_path: String,
    pub checkpoint_path: String,
    pub archive_exists: bool,
    pub checkpoint_exists: bool,
    pub archive_size_bytes: Option<u64>,
    pub checkpoint_size_bytes: Option<u64>,
}

#[derive(Clone, Debug, Facet, PartialEq, Eq)]
pub struct ImageModelCheckpointTensorPreview {
    pub key: String,
    pub dtype: String,
    pub shape: Vec<u64>,
    pub data_size_bytes: u64,
}

#[derive(Clone, Debug, Facet, PartialEq, Eq)]
pub struct ImageModelCheckpointReport {
    pub checkpoint_path: String,
    pub state_dict_top_level_key: String,
    pub checkpoint_name: Option<String>,
    pub nunif_model: Option<bool>,
    pub kwargs_in_channels: Option<u32>,
    pub kwargs_out_channels: Option<u32>,
    pub pytorch_format: String,
    pub pytorch_version: Option<String>,
    pub tensor_count: u64,
    pub total_tensor_data_bytes: u64,
    pub sample_keys: Vec<String>,
    pub sample_tensors: Vec<ImageModelCheckpointTensorPreview>,
}

#[derive(Clone, Debug, Facet, PartialEq, Eq)]
pub struct ImageModelCheckpointBurnLoadProbeReport {
    pub module_name: String,
    pub output_scale: u64,
    pub checkpoint_filter_regex: String,
    pub matched_checkpoint_keys: Vec<String>,
    pub patch0_weight_shape: Vec<u64>,
    pub patch2_weight_shape: Vec<u64>,
    pub down1_conv_weight_shape: Vec<u64>,
    pub down2_conv_weight_shape: Vec<u64>,
    pub swin1_block0_qkv_weight_shape: Vec<u64>,
    pub swin1_block0_relative_position_bias_table_shape: Vec<u64>,
    pub swin1_block0_relative_position_index_shape: Vec<u64>,
    pub swin1_block1_qkv_weight_shape: Vec<u64>,
    pub swin1_block1_relative_position_bias_table_shape: Vec<u64>,
    pub swin1_block1_relative_position_index_shape: Vec<u64>,
    pub swin2_block0_qkv_weight_shape: Vec<u64>,
    pub swin2_block0_relative_position_bias_table_shape: Vec<u64>,
    pub swin2_block0_relative_position_index_shape: Vec<u64>,
    pub swin2_block1_qkv_weight_shape: Vec<u64>,
    pub swin2_block1_relative_position_bias_table_shape: Vec<u64>,
    pub swin2_block1_relative_position_index_shape: Vec<u64>,
    pub swin3_block0_qkv_weight_shape: Vec<u64>,
    pub swin3_block0_relative_position_bias_table_shape: Vec<u64>,
    pub swin3_block0_relative_position_index_shape: Vec<u64>,
    pub swin3_block1_qkv_weight_shape: Vec<u64>,
    pub swin3_block1_relative_position_bias_table_shape: Vec<u64>,
    pub swin3_block1_relative_position_index_shape: Vec<u64>,
    pub swin3_block2_qkv_weight_shape: Vec<u64>,
    pub swin3_block2_relative_position_bias_table_shape: Vec<u64>,
    pub swin3_block2_relative_position_index_shape: Vec<u64>,
    pub swin3_block3_qkv_weight_shape: Vec<u64>,
    pub swin3_block3_relative_position_bias_table_shape: Vec<u64>,
    pub swin3_block3_relative_position_index_shape: Vec<u64>,
    pub swin3_block4_qkv_weight_shape: Vec<u64>,
    pub swin3_block4_relative_position_bias_table_shape: Vec<u64>,
    pub swin3_block4_relative_position_index_shape: Vec<u64>,
    pub swin3_block5_qkv_weight_shape: Vec<u64>,
    pub swin3_block5_relative_position_bias_table_shape: Vec<u64>,
    pub swin3_block5_relative_position_index_shape: Vec<u64>,
    pub swin4_block0_qkv_weight_shape: Vec<u64>,
    pub swin4_block0_relative_position_bias_table_shape: Vec<u64>,
    pub swin4_block0_relative_position_index_shape: Vec<u64>,
    pub swin4_block1_qkv_weight_shape: Vec<u64>,
    pub swin4_block1_relative_position_bias_table_shape: Vec<u64>,
    pub swin4_block1_relative_position_index_shape: Vec<u64>,
    pub swin5_block0_qkv_weight_shape: Vec<u64>,
    pub swin5_block0_relative_position_bias_table_shape: Vec<u64>,
    pub swin5_block0_relative_position_index_shape: Vec<u64>,
    pub swin5_block1_qkv_weight_shape: Vec<u64>,
    pub swin5_block1_relative_position_bias_table_shape: Vec<u64>,
    pub swin5_block1_relative_position_index_shape: Vec<u64>,
    pub proj2_weight_shape: Option<Vec<u64>>,
    pub up1_proj_weight_shape: Vec<u64>,
    pub up2_proj_weight_shape: Vec<u64>,
    pub to_image_proj_weight_shape: Vec<u64>,
    pub applied: Vec<String>,
    pub missing: Vec<String>,
    pub unused: Vec<String>,
    pub errors: Vec<String>,
}

#[derive(Clone, Debug, Facet, PartialEq)]
pub struct ImageModelDetailsReport {
    pub status: ImageModelStatusReport,
    pub metadata: ImageModelMetadata,
    pub source_artifacts: ImageModelSourceArtifactsReport,
    pub checkpoint: Option<ImageModelCheckpointReport>,
    pub burn_load_probe: Option<ImageModelCheckpointBurnLoadProbeReport>,
}

#[derive(Clone, Debug, Facet, PartialEq, Eq)]
pub struct ImageModelInventoryEntry {
    pub status: ImageModelStatusReport,
    pub family: String,
    pub style: String,
    pub method: String,
    pub noise_level: Option<u8>,
    pub native_scale: u8,
    pub architecture: String,
    pub source_checkpoint_path: String,
    pub teamy_runtime_status: String,
}

#[derive(Clone, Debug, Facet, PartialEq, Eq)]
pub struct ImageModelListReport {
    pub managed_root: String,
    pub models: Vec<ImageModelInventoryEntry>,
}

#[derive(Clone, Debug, Facet, PartialEq)]
pub struct PreparedImageModelReport {
    pub status: ImageModelStatusReport,
    pub metadata: ImageModelMetadata,
    pub source_artifacts: ImageModelSourceArtifactsReport,
    pub checkpoint: Option<ImageModelCheckpointReport>,
    pub burn_load_probe: Option<ImageModelCheckpointBurnLoadProbeReport>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageModelSelectionRequest<'a> {
    pub style: &'a str,
    pub method: &'a str,
    pub noise_level: Option<u8>,
}

pub type Waifu2xCpuBackend = NdArray<f32>;
pub type Waifu2xInferenceBackend = Cuda<f32, i32>;
type Waifu2xProbeBackend = Waifu2xCpuBackend;
type Waifu2xUpscaledImage = (Vec<f32>, Option<Vec<f32>>, u32, u32, u32);

const WAIFU2X_TTA_TRANSFORM_COUNT: usize = 8;
const WAIFU2X_TTA_TRANSFORM_COUNT_F32: f32 = 8.0;
#[cfg(test)]
const TEAMY_STUDIO_FULL_CHECK_ENV_VAR: &str = "TEAMY_STUDIO_FULL_CHECK";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Waifu2xTtaTransform {
    Identity,
    Rotate90,
    Rotate180,
    Rotate270,
    FlipHorizontal,
    FlipVertical,
    Transpose,
    Transverse,
}

const WAIFU2X_TTA_TRANSFORMS: [Waifu2xTtaTransform; WAIFU2X_TTA_TRANSFORM_COUNT] = [
    Waifu2xTtaTransform::Identity,
    Waifu2xTtaTransform::Rotate90,
    Waifu2xTtaTransform::Rotate180,
    Waifu2xTtaTransform::Rotate270,
    Waifu2xTtaTransform::FlipHorizontal,
    Waifu2xTtaTransform::FlipVertical,
    Waifu2xTtaTransform::Transpose,
    Waifu2xTtaTransform::Transverse,
];

impl Waifu2xTtaTransform {
    #[must_use]
    const fn inverse(self) -> Self {
        match self {
            Self::Identity => Self::Identity,
            Self::Rotate90 => Self::Rotate270,
            Self::Rotate180 => Self::Rotate180,
            Self::Rotate270 => Self::Rotate90,
            Self::FlipHorizontal => Self::FlipHorizontal,
            Self::FlipVertical => Self::FlipVertical,
            Self::Transpose => Self::Transpose,
            Self::Transverse => Self::Transverse,
        }
    }
}

#[must_use]
pub fn waifu2x_inference_device() -> CudaDevice {
    CudaDevice::default()
}

const WAIFU2X_WINDOW_SIZE: usize = 6;
const WAIFU2X_WINDOW_AREA: usize = WAIFU2X_WINDOW_SIZE * WAIFU2X_WINDOW_SIZE;
const WAIFU2X_SHIFT_SIZE: usize = WAIFU2X_WINDOW_SIZE / 2;
const WAIFU2X_NUM_HEADS: usize = 6;

#[derive(Module, Debug)]
pub struct Waifu2xPatchStem<B: Backend> {
    patch0: Conv2d<B>,
    patch2: Conv2d<B>,
    down1_conv: Conv2d<B>,
    down2_conv: Conv2d<B>,
    block0: Waifu2xSwinBlockProbe<B>,
    block1: Waifu2xSwinBlockProbe<B>,
    stage2_block0: Waifu2xSwinBlockProbe<B>,
    stage2_block1: Waifu2xSwinBlockProbe<B>,
    stage3_block0: Waifu2xSwinBlockProbe<B>,
    stage3_block1: Waifu2xSwinBlockProbe<B>,
    stage3_block2: Waifu2xSwinBlockProbe<B>,
    stage3_block3: Waifu2xSwinBlockProbe<B>,
    stage3_block4: Waifu2xSwinBlockProbe<B>,
    stage3_block5: Waifu2xSwinBlockProbe<B>,
    stage4_block0: Waifu2xSwinBlockProbe<B>,
    stage4_block1: Waifu2xSwinBlockProbe<B>,
    stage5_block0: Waifu2xSwinBlockProbe<B>,
    stage5_block1: Waifu2xSwinBlockProbe<B>,
    proj2: Option<Linear<B>>,
    up1_proj: Linear<B>,
    up2_proj: Linear<B>,
    to_image_proj: Linear<B>,
    output_scale: usize,
}

#[derive(Module, Debug)]
struct Waifu2xSwinBlockProbe<B: Backend> {
    attn: Waifu2xSwinAttentionProbe<B>,
    mlp: Waifu2xSwinMlpProbe<B>,
}

#[derive(Module, Debug)]
struct Waifu2xSwinAttentionProbe<B: Backend> {
    qkv: Linear<B>,
    proj: Linear<B>,
    relative_position_bias_table: Param<Tensor<B, 2>>,
    relative_position_index: Param<Tensor<B, 1, Int>>,
}

#[derive(Module, Debug)]
struct Waifu2xSwinMlpProbe<B: Backend> {
    lin0: Linear<B>,
    lin3: Linear<B>,
}

impl<B: Backend> Waifu2xPatchStem<B> {
    fn forward(&self, input: Tensor<B, 4>) -> Tensor<B, 4> {
        let x2 = self.forward_patch(input);
        let x3 = self.forward_swin_stage(x2.clone(), [&self.block0, &self.block1]);
        let x4 = self.forward_down_stage(x3.clone(), &self.down1_conv);
        let x4 = self.forward_swin_stage(x4, [&self.stage2_block0, &self.stage2_block1]);
        let x5 = self.forward_down_stage(x4.clone(), &self.down2_conv);
        let x5 = self.forward_swin_stage(
            x5,
            [
                &self.stage3_block0,
                &self.stage3_block1,
                &self.stage3_block2,
                &self.stage3_block3,
                &self.stage3_block4,
                &self.stage3_block5,
            ],
        );
        let x5 = self.forward_patch_up(x5, &self.up2_proj);
        let x = x5 + x4;
        let x = self.forward_swin_stage(x, [&self.stage4_block0, &self.stage4_block1]);
        let x = self.forward_patch_up(x, &self.up1_proj);
        let x = x + self.forward_proj2(x3);
        let x = self.forward_swin_stage(x, [&self.stage5_block0, &self.stage5_block1]);
        self.forward_to_image(x)
    }

    fn forward_patch(&self, input: Tensor<B, 4>) -> Tensor<B, 4> {
        let x = leaky_relu(self.patch0.forward(input), 0.1);
        let x = leaky_relu(self.patch2.forward(x), 0.1);
        let [batch, channels, height, width] = x.dims();
        assert!(
            height > 12 && width > 12,
            "waifu2x patch stem expects spatial dimensions larger than 12"
        );
        x.slice([0..batch, 0..channels, 6..height - 6, 6..width - 6])
            .permute([0, 2, 3, 1])
    }

    #[expect(
        clippy::unused_self,
        reason = "kept as a method for symmetry with the other stage helpers"
    )]
    fn forward_down_stage(&self, input: Tensor<B, 4>, conv: &Conv2d<B>) -> Tensor<B, 4> {
        conv.forward(input.permute([0, 3, 1, 2]))
            .permute([0, 2, 3, 1])
    }

    #[expect(
        clippy::unused_self,
        reason = "kept as a method for symmetry with the other stage helpers"
    )]
    fn forward_patch_up(&self, input: Tensor<B, 4>, proj: &Linear<B>) -> Tensor<B, 4> {
        let x = proj.forward(input).permute([0, 3, 1, 2]);
        pixel_shuffle_2x(x).permute([0, 2, 3, 1])
    }

    fn forward_proj2(&self, input: Tensor<B, 4>) -> Tensor<B, 4> {
        if let Some(proj2) = &self.proj2 {
            proj2.forward(input)
        } else {
            input
        }
    }

    fn forward_to_image(&self, input: Tensor<B, 4>) -> Tensor<B, 4> {
        let x = self.to_image_proj.forward(input).permute([0, 3, 1, 2]);
        pixel_shuffle(x, self.output_scale)
    }

    #[expect(
        clippy::unused_self,
        reason = "kept as a method for symmetry with the other stage helpers"
    )]
    fn forward_swin_stage<const N: usize>(
        &self,
        mut input: Tensor<B, 4>,
        blocks: [&Waifu2xSwinBlockProbe<B>; N],
    ) -> Tensor<B, 4> {
        for (index, block) in blocks.into_iter().enumerate() {
            input = block.forward(input, index % 2 == 1);
        }
        input
    }
}

impl<B: Backend> Waifu2xSwinBlockProbe<B> {
    fn forward(&self, input: Tensor<B, 4>, shifted: bool) -> Tensor<B, 4> {
        let x = input.clone() + self.attn.forward(input, shifted);
        x.clone() + self.mlp.forward(x)
    }
}

impl<B: Backend> Waifu2xSwinAttentionProbe<B> {
    #[expect(
        clippy::too_many_lines,
        reason = "the Swin attention probe mirrors the reference tensor flow step by step"
    )]
    #[expect(
        clippy::cast_precision_loss,
        reason = "attention scaling uses a small head dimension and Burn tensors are f32"
    )]
    fn forward(&self, input: Tensor<B, 4>, shifted: bool) -> Tensor<B, 4> {
        let [batch, height, width, channels] = input.dims();
        assert_eq!(
            channels % WAIFU2X_NUM_HEADS,
            0,
            "waifu2x attention expects channels divisible by the fixed head count"
        );
        assert_eq!(
            height % WAIFU2X_WINDOW_SIZE,
            0,
            "waifu2x attention expects height divisible by the window size"
        );
        assert_eq!(
            width % WAIFU2X_WINDOW_SIZE,
            0,
            "waifu2x attention expects width divisible by the window size"
        );

        let shift = if shifted { WAIFU2X_SHIFT_SIZE } else { 0 };
        let shift = isize::try_from(shift).expect("waifu2x shift size must fit in isize");
        let head_dim = channels / WAIFU2X_NUM_HEADS;
        let windows_h = height / WAIFU2X_WINDOW_SIZE;
        let windows_w = width / WAIFU2X_WINDOW_SIZE;
        let num_windows = windows_h * windows_w;
        let x = if shift > 0 {
            input.roll(&[-shift, -shift], &[1, 2])
        } else {
            input
        };
        let x = x
            .reshape([
                batch,
                windows_h,
                WAIFU2X_WINDOW_SIZE,
                windows_w,
                WAIFU2X_WINDOW_SIZE,
                channels,
            ])
            .permute([0, 1, 3, 2, 4, 5])
            .reshape([batch * num_windows, WAIFU2X_WINDOW_AREA, channels]);
        let qkv = self
            .qkv
            .forward(x.clone())
            .reshape([
                batch * num_windows,
                WAIFU2X_WINDOW_AREA,
                3,
                WAIFU2X_NUM_HEADS,
                head_dim,
            ])
            .permute([2, 0, 3, 1, 4]);
        let q = qkv
            .clone()
            .slice([
                0..1,
                0..batch * num_windows,
                0..WAIFU2X_NUM_HEADS,
                0..WAIFU2X_WINDOW_AREA,
                0..head_dim,
            ])
            .squeeze_dim(0)
            * ((head_dim as f32).powf(-0.5));
        let k = qkv
            .clone()
            .slice([
                1..2,
                0..batch * num_windows,
                0..WAIFU2X_NUM_HEADS,
                0..WAIFU2X_WINDOW_AREA,
                0..head_dim,
            ])
            .squeeze_dim(0);
        let v = qkv
            .slice([
                2..3,
                0..batch * num_windows,
                0..WAIFU2X_NUM_HEADS,
                0..WAIFU2X_WINDOW_AREA,
                0..head_dim,
            ])
            .squeeze_dim(0);
        let attn = q.matmul(k.swap_dims(2, 3)) + self.relative_position_bias();

        let attn = if shift > 0 {
            let mask = shifted_window_attention_mask::<B>(height, width, &attn.device());
            (attn.reshape([
                batch,
                num_windows,
                WAIFU2X_NUM_HEADS,
                WAIFU2X_WINDOW_AREA,
                WAIFU2X_WINDOW_AREA,
            ]) + mask)
                .reshape([
                    batch * num_windows,
                    WAIFU2X_NUM_HEADS,
                    WAIFU2X_WINDOW_AREA,
                    WAIFU2X_WINDOW_AREA,
                ])
        } else {
            attn
        };

        let x = softmax(attn, 3).matmul(v).swap_dims(1, 2).reshape([
            batch * num_windows,
            WAIFU2X_WINDOW_AREA,
            channels,
        ]);
        let x = self.proj.forward(x);
        let x = x
            .reshape([
                batch,
                windows_h,
                windows_w,
                WAIFU2X_WINDOW_SIZE,
                WAIFU2X_WINDOW_SIZE,
                channels,
            ])
            .permute([0, 1, 3, 2, 4, 5])
            .reshape([batch, height, width, channels]);

        if shift > 0 {
            x.roll(&[shift, shift], &[1, 2])
        } else {
            x
        }
    }

    fn relative_position_bias(&self) -> Tensor<B, 4> {
        let table = self.relative_position_bias_table.val();
        let index = self.relative_position_index.val();
        let gathered = table.gather(
            0,
            index
                .unsqueeze_dim::<2>(1)
                .expand([WAIFU2X_WINDOW_AREA * WAIFU2X_WINDOW_AREA, WAIFU2X_NUM_HEADS]),
        );
        gathered
            .reshape([WAIFU2X_WINDOW_AREA, WAIFU2X_WINDOW_AREA, WAIFU2X_NUM_HEADS])
            .permute([2, 0, 1])
            .unsqueeze_dim(0)
    }
}

impl<B: Backend> Waifu2xSwinMlpProbe<B> {
    fn forward(&self, input: Tensor<B, 4>) -> Tensor<B, 4> {
        self.lin3.forward(gelu(self.lin0.forward(input)))
    }
}

fn pixel_shuffle_2x<B: Backend>(input: Tensor<B, 4>) -> Tensor<B, 4> {
    pixel_shuffle(input, 2)
}

fn pixel_shuffle<B: Backend>(input: Tensor<B, 4>, upscale_factor: usize) -> Tensor<B, 4> {
    let [batch, channels, height, width] = input.dims();
    let upscale_area = upscale_factor * upscale_factor;
    assert_eq!(
        channels % upscale_area,
        0,
        "pixel shuffle expects the channel dimension to be divisible by the upscale area"
    );
    let out_channels = channels / upscale_area;
    input
        .reshape([
            batch,
            out_channels,
            upscale_factor,
            upscale_factor,
            height,
            width,
        ])
        .permute([0, 1, 4, 2, 5, 3])
        .reshape([
            batch,
            out_channels,
            height * upscale_factor,
            width * upscale_factor,
        ])
}

fn shifted_window_attention_mask<B: Backend>(
    height: usize,
    width: usize,
    device: &B::Device,
) -> Tensor<B, 5> {
    let windows_h = height / WAIFU2X_WINDOW_SIZE;
    let windows_w = width / WAIFU2X_WINDOW_SIZE;
    let num_windows = windows_h * windows_w;
    let mut region_ids = vec![0_i64; height * width];
    let h_slices = [
        (0, height - WAIFU2X_WINDOW_SIZE),
        (height - WAIFU2X_WINDOW_SIZE, height - WAIFU2X_SHIFT_SIZE),
        (height - WAIFU2X_SHIFT_SIZE, height),
    ];
    let w_slices = [
        (0, width - WAIFU2X_WINDOW_SIZE),
        (width - WAIFU2X_WINDOW_SIZE, width - WAIFU2X_SHIFT_SIZE),
        (width - WAIFU2X_SHIFT_SIZE, width),
    ];
    let mut count = 0_i64;
    for (h_start, h_end) in h_slices {
        for (w_start, w_end) in w_slices {
            for row in h_start..h_end {
                for column in w_start..w_end {
                    region_ids[row * width + column] = count;
                }
            }
            count += 1;
        }
    }

    let mut window_labels = vec![0_i64; num_windows * WAIFU2X_WINDOW_AREA];
    let mut cursor = 0;
    for window_row in 0..windows_h {
        for window_column in 0..windows_w {
            for row in 0..WAIFU2X_WINDOW_SIZE {
                for column in 0..WAIFU2X_WINDOW_SIZE {
                    let source_row = window_row * WAIFU2X_WINDOW_SIZE + row;
                    let source_column = window_column * WAIFU2X_WINDOW_SIZE + column;
                    window_labels[cursor] = region_ids[source_row * width + source_column];
                    cursor += 1;
                }
            }
        }
    }

    let mut mask = vec![0.0_f32; num_windows * WAIFU2X_WINDOW_AREA * WAIFU2X_WINDOW_AREA];
    for window_index in 0..num_windows {
        let base = window_index * WAIFU2X_WINDOW_AREA;
        let mask_base = window_index * WAIFU2X_WINDOW_AREA * WAIFU2X_WINDOW_AREA;
        for left in 0..WAIFU2X_WINDOW_AREA {
            for right in 0..WAIFU2X_WINDOW_AREA {
                if window_labels[base + left] != window_labels[base + right] {
                    mask[mask_base + left * WAIFU2X_WINDOW_AREA + right] = -100.0;
                }
            }
        }
    }

    Tensor::from_data(
        TensorData::new(
            mask,
            [1, num_windows, 1, WAIFU2X_WINDOW_AREA, WAIFU2X_WINDOW_AREA],
        ),
        device,
    )
}

#[must_use]
// image[impl model.cache-layout]
pub fn image_models_root(cache_home: &CacheHome) -> PathBuf {
    crate::model::managed_models_dir(cache_home).join(IMAGE_MODELS_DIR_NAME)
}

#[must_use]
// image[impl model.cache-layout]
pub fn managed_image_model_dir(cache_home: &CacheHome, model_name: &str) -> PathBuf {
    image_models_root(cache_home).join(model_name)
}

#[must_use]
pub fn image_model_metadata_path(model_dir: &Path) -> PathBuf {
    model_dir.join(IMAGE_MODEL_METADATA_FILE_NAME)
}

#[must_use]
pub fn image_model_burnpack_path(model_dir: &Path) -> PathBuf {
    model_dir.join(IMAGE_MODEL_BURNPACK_FILE_NAME)
}

#[must_use]
pub fn image_model_source_dir(model_dir: &Path) -> PathBuf {
    model_dir.join(IMAGE_MODEL_SOURCE_DIR_NAME)
}

#[must_use]
pub fn known_image_model(model_name: &str) -> Option<&'static KnownImageModel> {
    let requested = model_name.trim();
    KNOWN_IMAGE_MODELS
        .iter()
        .find(|model| model.name.eq_ignore_ascii_case(requested))
}

#[must_use]
pub fn inspect_image_model(cache_home: &CacheHome, model_name: &str) -> ImageModelStatusReport {
    let managed_dir = managed_image_model_dir(cache_home, model_name);
    inspect_image_model_dir(model_name, &managed_dir)
}

#[must_use]
pub fn inspect_image_model_dir(model_name: &str, model_dir: &Path) -> ImageModelStatusReport {
    let metadata_path = image_model_metadata_path(model_dir);
    let burnpack_path = image_model_burnpack_path(model_dir);
    let metadata_exists = metadata_path.is_file();
    let burnpack_exists = burnpack_path.is_file();
    let state = match (metadata_exists, burnpack_exists) {
        (_, true) => ImageModelPreparationState::Prepared,
        (true, false) => ImageModelPreparationState::MetadataOnly,
        (false, false) => ImageModelPreparationState::Missing,
    };
    ImageModelStatusReport {
        name: model_name.to_owned(),
        managed_dir: model_dir.display().to_string(),
        metadata_path: metadata_path.display().to_string(),
        burnpack_path: burnpack_path.display().to_string(),
        state,
        metadata_exists,
        burnpack_exists,
    }
}

/// # Errors
///
/// This function returns an error if the model name is unknown, or metadata cannot be read.
pub fn image_model_details(
    cache_home: &CacheHome,
    model_name: &str,
    explicit_model_dir: Option<&Path>,
) -> eyre::Result<ImageModelDetailsReport> {
    let known = known_image_model(model_name).ok_or_else(|| unknown_model_error(model_name))?;
    let model_dir = explicit_model_dir.map_or_else(
        || managed_image_model_dir(cache_home, known.name),
        Path::to_path_buf,
    );
    let status = inspect_image_model_dir(known.name, &model_dir);
    let metadata = if status.metadata_exists {
        read_image_model_metadata(&image_model_metadata_path(&model_dir))?
    } else {
        ImageModelMetadata::from(known)
    };
    let source_artifacts = inspect_image_model_source_artifacts(&model_dir, &metadata)?;
    let checkpoint = inspect_image_model_checkpoint(&source_artifacts)?;
    let burn_load_probe = inspect_image_model_burn_load_probe(&source_artifacts)?;
    Ok(ImageModelDetailsReport {
        status,
        metadata,
        source_artifacts,
        checkpoint,
        burn_load_probe,
    })
}

// image[impl cli.model-list]
#[must_use]
pub fn list_image_models(cache_home: &CacheHome) -> ImageModelListReport {
    ImageModelListReport {
        managed_root: image_models_root(cache_home).display().to_string(),
        models: KNOWN_IMAGE_MODELS
            .iter()
            .map(|model| ImageModelInventoryEntry {
                status: inspect_image_model(cache_home, model.name),
                family: model.family.to_owned(),
                style: model.style.to_owned(),
                method: model.method.to_owned(),
                noise_level: model.noise_level,
                native_scale: model.native_scale,
                architecture: model.architecture.to_owned(),
                source_checkpoint_path: model.source_checkpoint_path.to_owned(),
                teamy_runtime_status: model.teamy_runtime_status.to_owned(),
            })
            .collect(),
    }
}

#[must_use]
pub fn default_upscale_model_name() -> &'static str {
    DEFAULT_IMAGE_MODEL_NAME
}

#[must_use]
pub fn default_upscale_method() -> &'static str {
    "scale2x"
}

/// # Errors
///
/// This function returns an error if no known managed image model matches the requested
/// style, method, and optional noise-level combination.
pub fn resolve_image_model_for_request(
    request: ImageModelSelectionRequest<'_>,
) -> eyre::Result<&'static KnownImageModel> {
    let style = normalize_image_model_style(request.style);
    KNOWN_IMAGE_MODELS
        .iter()
        .find(|model| {
            model.style.eq_ignore_ascii_case(style)
                && model.method.eq_ignore_ascii_case(request.method)
                && model.noise_level == request.noise_level
        })
        .ok_or_else(|| {
            let noise_suffix = request
                .noise_level
                .map(|value| format!(", noise-level {value}"))
                .unwrap_or_default();
            eyre::eyre!(
                "no known image model matches style `{}` with method `{}`{}",
                request.style,
                request.method,
                noise_suffix
            )
        })
}

fn normalize_image_model_style(style: &str) -> &str {
    if style.eq_ignore_ascii_case("scan") {
        "art_scan"
    } else {
        style
    }
}

/// # Errors
///
/// This function returns an error if the model name is unknown or metadata/source artifacts cannot be prepared.
// image[impl cli.model-prepare]
// image[impl model.cache-layout]
// image[impl runtime.rust-only]
pub fn prepare_image_model(
    cache_home: &CacheHome,
    model_name: &str,
    overwrite: bool,
) -> eyre::Result<PreparedImageModelReport> {
    let known = known_image_model(model_name).ok_or_else(|| unknown_model_error(model_name))?;
    ensure_image_model_runtime_is_implemented(known, "prepare")?;
    let managed_dir = managed_image_model_dir(cache_home, known.name);
    let metadata_path = image_model_metadata_path(&managed_dir);
    std::fs::create_dir_all(&managed_dir)
        .wrap_err_with(|| format!("failed to create image model dir {}", managed_dir.display()))?;
    let metadata = if metadata_path.is_file() {
        read_image_model_metadata(&metadata_path)?
    } else {
        ImageModelMetadata::from(known)
    };
    if overwrite || !metadata_path.is_file() {
        write_image_model_metadata(&metadata_path, &metadata)?;
    }
    ensure_image_model_source_artifacts(&managed_dir, &metadata, overwrite)?;
    let source_artifacts = inspect_image_model_source_artifacts(&managed_dir, &metadata)?;
    ensure_image_model_burnpack(&managed_dir, &metadata, &source_artifacts, overwrite)?;
    let checkpoint = inspect_image_model_checkpoint(&source_artifacts)?;
    let burn_load_probe = inspect_image_model_burn_load_probe(&source_artifacts)?;

    Ok(PreparedImageModelReport {
        status: inspect_image_model_dir(known.name, &managed_dir),
        metadata,
        source_artifacts,
        checkpoint,
        burn_load_probe,
    })
}

fn inspect_image_model_checkpoint(
    source_artifacts: &ImageModelSourceArtifactsReport,
) -> eyre::Result<Option<ImageModelCheckpointReport>> {
    if !source_artifacts.checkpoint_exists {
        return Ok(None);
    }

    let checkpoint_path = Path::new(&source_artifacts.checkpoint_path);
    let reader =
        PytorchReader::with_top_level_key(checkpoint_path, "state_dict").wrap_err_with(|| {
            format!(
                "failed to read waifu2x state_dict from {}",
                checkpoint_path.display()
            )
        })?;
    let tensor_count = u64::try_from(reader.len())
        .wrap_err("waifu2x checkpoint tensor count does not fit in u64")?;
    let total_tensor_data_bytes = reader.tensors().values().try_fold(0_u64, |total, tensor| {
        let size = u64::try_from(tensor.data_len()).map_err(|error| eyre::eyre!(error))?;
        total
            .checked_add(size)
            .ok_or_else(|| eyre::eyre!("waifu2x checkpoint byte size overflowed u64"))
    })?;

    let mut sample_keys = reader.keys();
    sample_keys.sort_unstable();
    sample_keys.truncate(16);
    let sample_tensors = sample_keys
        .iter()
        .map(|key| checkpoint_tensor_preview(reader.get(key), key))
        .collect::<eyre::Result<Vec<_>>>()?;

    let checkpoint_name = load_optional_checkpoint_config::<String>(checkpoint_path, "name")?;
    let nunif_model = load_optional_checkpoint_config::<u64>(checkpoint_path, "nunif_model")?
        .map(|value| value != 0);
    let kwargs =
        load_optional_checkpoint_config::<ImageModelCheckpointKwargs>(checkpoint_path, "kwargs")?;

    Ok(Some(ImageModelCheckpointReport {
        checkpoint_path: source_artifacts.checkpoint_path.clone(),
        state_dict_top_level_key: "state_dict".to_owned(),
        checkpoint_name,
        nunif_model,
        kwargs_in_channels: kwargs
            .as_ref()
            .map(|kwargs| u32::try_from(kwargs.in_channels))
            .transpose()
            .wrap_err("waifu2x checkpoint kwargs.in_channels does not fit in u32")?,
        kwargs_out_channels: kwargs
            .as_ref()
            .map(|kwargs| u32::try_from(kwargs.out_channels))
            .transpose()
            .wrap_err("waifu2x checkpoint kwargs.out_channels does not fit in u32")?,
        pytorch_format: format!("{:?}", reader.metadata().format_type),
        pytorch_version: reader.metadata().pytorch_version.clone(),
        tensor_count,
        total_tensor_data_bytes,
        sample_keys,
        sample_tensors,
    }))
}

fn ensure_image_model_burnpack(
    model_dir: &Path,
    metadata: &ImageModelMetadata,
    source_artifacts: &ImageModelSourceArtifactsReport,
    overwrite: bool,
) -> eyre::Result<()> {
    if !source_artifacts.checkpoint_exists {
        bail!(
            "cannot create image model Burnpack because checkpoint is missing at {}",
            source_artifacts.checkpoint_path
        );
    }

    let burnpack_path = image_model_burnpack_path(model_dir);
    if burnpack_path.is_file() && !overwrite {
        return Ok(());
    }

    let checkpoint_path = Path::new(&source_artifacts.checkpoint_path);
    let model = load_waifu2x_probe_model_from_checkpoint(checkpoint_path)?;
    let mut store = BurnpackStore::from_file(&burnpack_path)
        .overwrite(true)
        .metadata("image.model_name", metadata.model_name.clone())
        .metadata("image.family", metadata.family.clone())
        .metadata("image.style", metadata.style.clone())
        .metadata("image.scale", metadata.scale.to_string())
        .metadata("image.architecture", metadata.architecture.clone())
        .metadata("image.model_offset", metadata.model_offset.to_string())
        .metadata("image.blend_size", metadata.blend_size.to_string())
        .metadata(
            "image.default_tile_size",
            metadata.default_tile_size.to_string(),
        )
        .metadata(
            "image.default_batch_size",
            metadata.default_batch_size.to_string(),
        )
        .metadata("image.input_channels", metadata.input_channels.to_string())
        .metadata(
            "image.output_channels",
            metadata.output_channels.to_string(),
        );
    if let Some(parameter_count) = metadata.parameter_count {
        store = store.metadata("image.parameter_count", parameter_count.to_string());
    }
    println!(
        "Saving image model Burnpack weights: {}",
        burnpack_path.display()
    );
    model.save_into(&mut store).wrap_err_with(|| {
        format!(
            "failed to write image model Burnpack {}",
            burnpack_path.display()
        )
    })
}

#[expect(
    clippy::too_many_lines,
    reason = "checkpoint import is intentionally explicit so each required tensor name stays local"
)]
fn load_waifu2x_probe_model_from_checkpoint(
    checkpoint_path: &Path,
) -> eyre::Result<Waifu2xPatchStem<Waifu2xProbeBackend>> {
    let reader =
        PytorchReader::with_top_level_key(checkpoint_path, "state_dict").wrap_err_with(|| {
            format!(
                "failed to read waifu2x state_dict from {} for Burnpack import",
                checkpoint_path.display()
            )
        })?;
    let patch0 = required_checkpoint_snapshot(&reader, "unet.patch.0.weight")?;
    let patch2 = required_checkpoint_snapshot(&reader, "unet.patch.2.weight")?;
    let down1_conv = required_checkpoint_snapshot(&reader, "unet.down1.conv.weight")?;
    let down2_conv = required_checkpoint_snapshot(&reader, "unet.down2.conv.weight")?;
    let block0_qkv = required_checkpoint_snapshot(&reader, "unet.swin1.block.0.attn.qkv.weight")?;
    let block0_proj = required_checkpoint_snapshot(&reader, "unet.swin1.block.0.attn.proj.weight")?;
    let block0_mlp0 = required_checkpoint_snapshot(&reader, "unet.swin1.block.0.mlp.0.weight")?;
    let block0_mlp3 = required_checkpoint_snapshot(&reader, "unet.swin1.block.0.mlp.3.weight")?;
    let block0_bias_table = required_checkpoint_snapshot(
        &reader,
        "unet.swin1.block.0.attn.relative_position_bias_table",
    )?;
    let block0_index =
        required_checkpoint_snapshot(&reader, "unet.swin1.block.0.attn.relative_position_index")?;
    let block1_qkv = required_checkpoint_snapshot(&reader, "unet.swin1.block.1.attn.qkv.weight")?;
    let block1_proj = required_checkpoint_snapshot(&reader, "unet.swin1.block.1.attn.proj.weight")?;
    let block1_mlp0 = required_checkpoint_snapshot(&reader, "unet.swin1.block.1.mlp.0.weight")?;
    let block1_mlp3 = required_checkpoint_snapshot(&reader, "unet.swin1.block.1.mlp.3.weight")?;
    let block1_bias_table = required_checkpoint_snapshot(
        &reader,
        "unet.swin1.block.1.attn.relative_position_bias_table",
    )?;
    let block1_index =
        required_checkpoint_snapshot(&reader, "unet.swin1.block.1.attn.relative_position_index")?;
    let stage2_block0_qkv =
        required_checkpoint_snapshot(&reader, "unet.swin2.block.0.attn.qkv.weight")?;
    let stage2_block0_proj =
        required_checkpoint_snapshot(&reader, "unet.swin2.block.0.attn.proj.weight")?;
    let stage2_block0_mlp0 =
        required_checkpoint_snapshot(&reader, "unet.swin2.block.0.mlp.0.weight")?;
    let stage2_block0_mlp3 =
        required_checkpoint_snapshot(&reader, "unet.swin2.block.0.mlp.3.weight")?;
    let stage2_block0_bias_table = required_checkpoint_snapshot(
        &reader,
        "unet.swin2.block.0.attn.relative_position_bias_table",
    )?;
    let stage2_block0_index =
        required_checkpoint_snapshot(&reader, "unet.swin2.block.0.attn.relative_position_index")?;
    let stage2_block1_qkv =
        required_checkpoint_snapshot(&reader, "unet.swin2.block.1.attn.qkv.weight")?;
    let stage2_block1_proj =
        required_checkpoint_snapshot(&reader, "unet.swin2.block.1.attn.proj.weight")?;
    let stage2_block1_mlp0 =
        required_checkpoint_snapshot(&reader, "unet.swin2.block.1.mlp.0.weight")?;
    let stage2_block1_mlp3 =
        required_checkpoint_snapshot(&reader, "unet.swin2.block.1.mlp.3.weight")?;
    let stage2_block1_bias_table = required_checkpoint_snapshot(
        &reader,
        "unet.swin2.block.1.attn.relative_position_bias_table",
    )?;
    let stage2_block1_index =
        required_checkpoint_snapshot(&reader, "unet.swin2.block.1.attn.relative_position_index")?;
    let stage3_block0_qkv =
        required_checkpoint_snapshot(&reader, "unet.swin3.block.0.attn.qkv.weight")?;
    let stage3_block0_proj =
        required_checkpoint_snapshot(&reader, "unet.swin3.block.0.attn.proj.weight")?;
    let stage3_block0_mlp0 =
        required_checkpoint_snapshot(&reader, "unet.swin3.block.0.mlp.0.weight")?;
    let stage3_block0_mlp3 =
        required_checkpoint_snapshot(&reader, "unet.swin3.block.0.mlp.3.weight")?;
    let stage3_block0_bias_table = required_checkpoint_snapshot(
        &reader,
        "unet.swin3.block.0.attn.relative_position_bias_table",
    )?;
    let stage3_block0_index =
        required_checkpoint_snapshot(&reader, "unet.swin3.block.0.attn.relative_position_index")?;
    let stage3_block1_qkv =
        required_checkpoint_snapshot(&reader, "unet.swin3.block.1.attn.qkv.weight")?;
    let stage3_block1_proj =
        required_checkpoint_snapshot(&reader, "unet.swin3.block.1.attn.proj.weight")?;
    let stage3_block1_mlp0 =
        required_checkpoint_snapshot(&reader, "unet.swin3.block.1.mlp.0.weight")?;
    let stage3_block1_mlp3 =
        required_checkpoint_snapshot(&reader, "unet.swin3.block.1.mlp.3.weight")?;
    let stage3_block1_bias_table = required_checkpoint_snapshot(
        &reader,
        "unet.swin3.block.1.attn.relative_position_bias_table",
    )?;
    let stage3_block1_index =
        required_checkpoint_snapshot(&reader, "unet.swin3.block.1.attn.relative_position_index")?;
    let stage3_block2_qkv =
        required_checkpoint_snapshot(&reader, "unet.swin3.block.2.attn.qkv.weight")?;
    let stage3_block2_proj =
        required_checkpoint_snapshot(&reader, "unet.swin3.block.2.attn.proj.weight")?;
    let stage3_block2_mlp0 =
        required_checkpoint_snapshot(&reader, "unet.swin3.block.2.mlp.0.weight")?;
    let stage3_block2_mlp3 =
        required_checkpoint_snapshot(&reader, "unet.swin3.block.2.mlp.3.weight")?;
    let stage3_block2_bias_table = required_checkpoint_snapshot(
        &reader,
        "unet.swin3.block.2.attn.relative_position_bias_table",
    )?;
    let stage3_block2_index =
        required_checkpoint_snapshot(&reader, "unet.swin3.block.2.attn.relative_position_index")?;
    let stage3_block3_qkv =
        required_checkpoint_snapshot(&reader, "unet.swin3.block.3.attn.qkv.weight")?;
    let stage3_block3_proj =
        required_checkpoint_snapshot(&reader, "unet.swin3.block.3.attn.proj.weight")?;
    let stage3_block3_mlp0 =
        required_checkpoint_snapshot(&reader, "unet.swin3.block.3.mlp.0.weight")?;
    let stage3_block3_mlp3 =
        required_checkpoint_snapshot(&reader, "unet.swin3.block.3.mlp.3.weight")?;
    let stage3_block3_bias_table = required_checkpoint_snapshot(
        &reader,
        "unet.swin3.block.3.attn.relative_position_bias_table",
    )?;
    let stage3_block3_index =
        required_checkpoint_snapshot(&reader, "unet.swin3.block.3.attn.relative_position_index")?;
    let stage3_block4_qkv =
        required_checkpoint_snapshot(&reader, "unet.swin3.block.4.attn.qkv.weight")?;
    let stage3_block4_proj =
        required_checkpoint_snapshot(&reader, "unet.swin3.block.4.attn.proj.weight")?;
    let stage3_block4_mlp0 =
        required_checkpoint_snapshot(&reader, "unet.swin3.block.4.mlp.0.weight")?;
    let stage3_block4_mlp3 =
        required_checkpoint_snapshot(&reader, "unet.swin3.block.4.mlp.3.weight")?;
    let stage3_block4_bias_table = required_checkpoint_snapshot(
        &reader,
        "unet.swin3.block.4.attn.relative_position_bias_table",
    )?;
    let stage3_block4_index =
        required_checkpoint_snapshot(&reader, "unet.swin3.block.4.attn.relative_position_index")?;
    let stage3_block5_qkv =
        required_checkpoint_snapshot(&reader, "unet.swin3.block.5.attn.qkv.weight")?;
    let stage3_block5_proj =
        required_checkpoint_snapshot(&reader, "unet.swin3.block.5.attn.proj.weight")?;
    let stage3_block5_mlp0 =
        required_checkpoint_snapshot(&reader, "unet.swin3.block.5.mlp.0.weight")?;
    let stage3_block5_mlp3 =
        required_checkpoint_snapshot(&reader, "unet.swin3.block.5.mlp.3.weight")?;
    let stage3_block5_bias_table = required_checkpoint_snapshot(
        &reader,
        "unet.swin3.block.5.attn.relative_position_bias_table",
    )?;
    let stage3_block5_index =
        required_checkpoint_snapshot(&reader, "unet.swin3.block.5.attn.relative_position_index")?;
    let stage4_block0_qkv =
        required_checkpoint_snapshot(&reader, "unet.swin4.block.0.attn.qkv.weight")?;
    let stage4_block0_proj =
        required_checkpoint_snapshot(&reader, "unet.swin4.block.0.attn.proj.weight")?;
    let stage4_block0_mlp0 =
        required_checkpoint_snapshot(&reader, "unet.swin4.block.0.mlp.0.weight")?;
    let stage4_block0_mlp3 =
        required_checkpoint_snapshot(&reader, "unet.swin4.block.0.mlp.3.weight")?;
    let stage4_block0_bias_table = required_checkpoint_snapshot(
        &reader,
        "unet.swin4.block.0.attn.relative_position_bias_table",
    )?;
    let stage4_block0_index =
        required_checkpoint_snapshot(&reader, "unet.swin4.block.0.attn.relative_position_index")?;
    let stage4_block1_qkv =
        required_checkpoint_snapshot(&reader, "unet.swin4.block.1.attn.qkv.weight")?;
    let stage4_block1_proj =
        required_checkpoint_snapshot(&reader, "unet.swin4.block.1.attn.proj.weight")?;
    let stage4_block1_mlp0 =
        required_checkpoint_snapshot(&reader, "unet.swin4.block.1.mlp.0.weight")?;
    let stage4_block1_mlp3 =
        required_checkpoint_snapshot(&reader, "unet.swin4.block.1.mlp.3.weight")?;
    let stage4_block1_bias_table = required_checkpoint_snapshot(
        &reader,
        "unet.swin4.block.1.attn.relative_position_bias_table",
    )?;
    let stage4_block1_index =
        required_checkpoint_snapshot(&reader, "unet.swin4.block.1.attn.relative_position_index")?;
    let stage5_block0_qkv =
        required_checkpoint_snapshot(&reader, "unet.swin5.block.0.attn.qkv.weight")?;
    let stage5_block0_proj =
        required_checkpoint_snapshot(&reader, "unet.swin5.block.0.attn.proj.weight")?;
    let stage5_block0_mlp0 =
        required_checkpoint_snapshot(&reader, "unet.swin5.block.0.mlp.0.weight")?;
    let stage5_block0_mlp3 =
        required_checkpoint_snapshot(&reader, "unet.swin5.block.0.mlp.3.weight")?;
    let stage5_block0_bias_table = required_checkpoint_snapshot(
        &reader,
        "unet.swin5.block.0.attn.relative_position_bias_table",
    )?;
    let stage5_block0_index =
        required_checkpoint_snapshot(&reader, "unet.swin5.block.0.attn.relative_position_index")?;
    let stage5_block1_qkv =
        required_checkpoint_snapshot(&reader, "unet.swin5.block.1.attn.qkv.weight")?;
    let stage5_block1_proj =
        required_checkpoint_snapshot(&reader, "unet.swin5.block.1.attn.proj.weight")?;
    let stage5_block1_mlp0 =
        required_checkpoint_snapshot(&reader, "unet.swin5.block.1.mlp.0.weight")?;
    let stage5_block1_mlp3 =
        required_checkpoint_snapshot(&reader, "unet.swin5.block.1.mlp.3.weight")?;
    let stage5_block1_bias_table = required_checkpoint_snapshot(
        &reader,
        "unet.swin5.block.1.attn.relative_position_bias_table",
    )?;
    let stage5_block1_index =
        required_checkpoint_snapshot(&reader, "unet.swin5.block.1.attn.relative_position_index")?;
    let output_scale = waifu2x_output_scale_from_checkpoint_reader(&reader);
    let proj2 = if output_scale == 4 {
        Some(required_checkpoint_snapshot(
            &reader,
            WAIFU2X_PROJ2_WEIGHT_KEY,
        )?)
    } else {
        None
    };
    let up1_proj = required_checkpoint_snapshot(&reader, "unet.up1.proj.weight")?;
    let up2_proj = required_checkpoint_snapshot(&reader, "unet.up2.proj.weight")?;
    let to_image_proj = required_checkpoint_snapshot(&reader, "unet.to_image.proj.weight")?;

    let device = burn::backend::ndarray::NdArrayDevice::default();
    let mut model = Waifu2xPatchStem::<Waifu2xProbeBackend> {
        patch0: conv2d_from_weight_snapshot(patch0, &device)?,
        patch2: conv2d_from_weight_snapshot(patch2, &device)?,
        down1_conv: downsample_conv2d_from_weight_snapshot(down1_conv, &device)?,
        down2_conv: downsample_conv2d_from_weight_snapshot(down2_conv, &device)?,
        block0: build_swin_block(
            &device,
            block0_qkv,
            block0_proj,
            block0_bias_table,
            block0_index,
            block0_mlp0,
            block0_mlp3,
        )?,
        block1: build_swin_block(
            &device,
            block1_qkv,
            block1_proj,
            block1_bias_table,
            block1_index,
            block1_mlp0,
            block1_mlp3,
        )?,
        stage2_block0: build_swin_block(
            &device,
            stage2_block0_qkv,
            stage2_block0_proj,
            stage2_block0_bias_table,
            stage2_block0_index,
            stage2_block0_mlp0,
            stage2_block0_mlp3,
        )?,
        stage2_block1: build_swin_block(
            &device,
            stage2_block1_qkv,
            stage2_block1_proj,
            stage2_block1_bias_table,
            stage2_block1_index,
            stage2_block1_mlp0,
            stage2_block1_mlp3,
        )?,
        stage3_block0: build_swin_block(
            &device,
            stage3_block0_qkv,
            stage3_block0_proj,
            stage3_block0_bias_table,
            stage3_block0_index,
            stage3_block0_mlp0,
            stage3_block0_mlp3,
        )?,
        stage3_block1: build_swin_block(
            &device,
            stage3_block1_qkv,
            stage3_block1_proj,
            stage3_block1_bias_table,
            stage3_block1_index,
            stage3_block1_mlp0,
            stage3_block1_mlp3,
        )?,
        stage3_block2: build_swin_block(
            &device,
            stage3_block2_qkv,
            stage3_block2_proj,
            stage3_block2_bias_table,
            stage3_block2_index,
            stage3_block2_mlp0,
            stage3_block2_mlp3,
        )?,
        stage3_block3: build_swin_block(
            &device,
            stage3_block3_qkv,
            stage3_block3_proj,
            stage3_block3_bias_table,
            stage3_block3_index,
            stage3_block3_mlp0,
            stage3_block3_mlp3,
        )?,
        stage3_block4: build_swin_block(
            &device,
            stage3_block4_qkv,
            stage3_block4_proj,
            stage3_block4_bias_table,
            stage3_block4_index,
            stage3_block4_mlp0,
            stage3_block4_mlp3,
        )?,
        stage3_block5: build_swin_block(
            &device,
            stage3_block5_qkv,
            stage3_block5_proj,
            stage3_block5_bias_table,
            stage3_block5_index,
            stage3_block5_mlp0,
            stage3_block5_mlp3,
        )?,
        stage4_block0: build_swin_block(
            &device,
            stage4_block0_qkv,
            stage4_block0_proj,
            stage4_block0_bias_table,
            stage4_block0_index,
            stage4_block0_mlp0,
            stage4_block0_mlp3,
        )?,
        stage4_block1: build_swin_block(
            &device,
            stage4_block1_qkv,
            stage4_block1_proj,
            stage4_block1_bias_table,
            stage4_block1_index,
            stage4_block1_mlp0,
            stage4_block1_mlp3,
        )?,
        stage5_block0: build_swin_block(
            &device,
            stage5_block0_qkv,
            stage5_block0_proj,
            stage5_block0_bias_table,
            stage5_block0_index,
            stage5_block0_mlp0,
            stage5_block0_mlp3,
        )?,
        stage5_block1: build_swin_block(
            &device,
            stage5_block1_qkv,
            stage5_block1_proj,
            stage5_block1_bias_table,
            stage5_block1_index,
            stage5_block1_mlp0,
            stage5_block1_mlp3,
        )?,
        proj2: proj2
            .map(|weight| linear_from_weight_snapshot(weight, &device))
            .transpose()?,
        up1_proj: linear_from_weight_snapshot(up1_proj, &device)?,
        up2_proj: linear_from_weight_snapshot(up2_proj, &device)?,
        to_image_proj: linear_from_weight_snapshot(to_image_proj, &device)?,
        output_scale,
    };

    let mut store = build_waifu2x_checkpoint_store(checkpoint_path);
    let result = model.load_from(&mut store).wrap_err_with(|| {
        format!(
            "failed to load waifu2x Burnpack model skeleton from {}",
            checkpoint_path.display()
        )
    })?;
    if !result.errors.is_empty() {
        bail!(
            "waifu2x Burnpack import reported tensor errors for {}: {:?}",
            checkpoint_path.display(),
            result.errors
        );
    }
    if !result.missing.is_empty() || !result.unused.is_empty() {
        bail!(
            "waifu2x Burnpack import did not consume the full checkpoint for {}: missing={:?} unused={:?}",
            checkpoint_path.display(),
            result.missing,
            result.unused
        );
    }
    Ok(model)
}

fn build_swin_block<B: Backend>(
    device: &B::Device,
    qkv: &burn_store::TensorSnapshot,
    proj: &burn_store::TensorSnapshot,
    bias_table: &burn_store::TensorSnapshot,
    index: &burn_store::TensorSnapshot,
    mlp0: &burn_store::TensorSnapshot,
    mlp3: &burn_store::TensorSnapshot,
) -> eyre::Result<Waifu2xSwinBlockProbe<B>> {
    Ok(Waifu2xSwinBlockProbe {
        attn: Waifu2xSwinAttentionProbe {
            qkv: linear_from_weight_snapshot(qkv, device)?,
            proj: linear_from_weight_snapshot(proj, device)?,
            relative_position_bias_table: float_param_2d_from_snapshot_shape(bias_table, device)?,
            relative_position_index: int_param_1d_from_snapshot_shape(index, device)?,
        },
        mlp: Waifu2xSwinMlpProbe {
            lin0: linear_from_weight_snapshot(mlp0, device)?,
            lin3: linear_from_weight_snapshot(mlp3, device)?,
        },
    })
}

fn init_swin_block_from_dims<B: Backend>(
    device: &B::Device,
    hidden_size: usize,
    mlp_hidden_size: usize,
) -> Waifu2xSwinBlockProbe<B> {
    Waifu2xSwinBlockProbe {
        attn: Waifu2xSwinAttentionProbe {
            qkv: LinearConfig::new(hidden_size, hidden_size * 3).init(device),
            proj: LinearConfig::new(hidden_size, hidden_size).init(device),
            relative_position_bias_table: Param::from_tensor(Tensor::<B, 2>::zeros(
                [121, 6],
                device,
            )),
            relative_position_index: Param::initialized(
                ParamId::new(),
                Tensor::<B, 1, Int>::zeros([1296], device),
            ),
        },
        mlp: Waifu2xSwinMlpProbe {
            lin0: LinearConfig::new(hidden_size, mlp_hidden_size).init(device),
            lin3: LinearConfig::new(mlp_hidden_size, hidden_size).init(device),
        },
    }
}

const WAIFU2X_PROJ2_WEIGHT_KEY: &str = "unet.proj2.weight";
const WAIFU2X_CHECKPOINT_FILTER_REGEX: &str = r"^(unet\.patch\.(0|2)|unet\.down(1|2)\.conv)\.(weight|bias)$|^unet\.swin(1|2)\.block\.(0|1)\.(attn\.(qkv|proj)|mlp\.(0|3))\.(weight|bias)$|^unet\.swin(1|2)\.block\.(0|1)\.attn\.relative_position_(bias_table|index)$|^unet\.swin3\.block\.[0-5]\.(attn\.(qkv|proj)|mlp\.(0|3))\.(weight|bias)$|^unet\.swin3\.block\.[0-5]\.attn\.relative_position_(bias_table|index)$|^unet\.swin4\.block\.(0|1)\.(attn\.(qkv|proj)|mlp\.(0|3))\.(weight|bias)$|^unet\.swin4\.block\.(0|1)\.attn\.relative_position_(bias_table|index)$|^unet\.swin5\.block\.(0|1)\.(attn\.(qkv|proj)|mlp\.(0|3))\.(weight|bias)$|^unet\.swin5\.block\.(0|1)\.attn\.relative_position_(bias_table|index)$|^unet\.proj2\.(weight|bias)$|^(unet\.(up1|up2|to_image)\.proj)\.(weight|bias)$";

fn waifu2x_output_scale_for_architecture(architecture: &str) -> eyre::Result<usize> {
    match architecture.trim() {
        "waifu2x.swin_unet_2x" => Ok(2),
        "waifu2x.swin_unet_4x" => Ok(4),
        other => bail!("unsupported waifu2x architecture `{other}` for Burn runtime"),
    }
}

fn waifu2x_output_scale_from_checkpoint_keys<'a>(keys: impl IntoIterator<Item = &'a str>) -> usize {
    if keys.into_iter().any(|key| key == WAIFU2X_PROJ2_WEIGHT_KEY) {
        4
    } else {
        2
    }
}

fn waifu2x_output_scale_from_checkpoint_reader(reader: &PytorchReader) -> usize {
    waifu2x_output_scale_from_checkpoint_keys(reader.keys().iter().map(String::as_str))
}

#[expect(
    clippy::too_many_lines,
    reason = "the waifu2x checkpoint store remapping stays explicit so checkpoint tensor routing remains local"
)]
fn configure_waifu2x_checkpoint_store(store: PytorchStore) -> PytorchStore {
    store
        .with_top_level_key("state_dict")
        .with_regex(WAIFU2X_CHECKPOINT_FILTER_REGEX)
        .with_key_remapping(r"^unet\.patch\.0\.", "patch0.")
        .with_key_remapping(r"^unet\.patch\.2\.", "patch2.")
        .with_key_remapping(r"^unet\.down1\.conv\.", "down1_conv.")
        .with_key_remapping(r"^unet\.down2\.conv\.", "down2_conv.")
        .with_key_remapping(r"^unet\.swin1\.block\.0\.attn\.", "block0.attn.")
        .with_key_remapping(r"^unet\.swin1\.block\.0\.mlp\.0\.", "block0.mlp.lin0.")
        .with_key_remapping(r"^unet\.swin1\.block\.0\.mlp\.3\.", "block0.mlp.lin3.")
        .with_key_remapping(r"^unet\.swin1\.block\.1\.attn\.", "block1.attn.")
        .with_key_remapping(r"^unet\.swin1\.block\.1\.mlp\.0\.", "block1.mlp.lin0.")
        .with_key_remapping(r"^unet\.swin1\.block\.1\.mlp\.3\.", "block1.mlp.lin3.")
        .with_key_remapping(r"^unet\.swin2\.block\.0\.attn\.", "stage2_block0.attn.")
        .with_key_remapping(
            r"^unet\.swin2\.block\.0\.mlp\.0\.",
            "stage2_block0.mlp.lin0.",
        )
        .with_key_remapping(
            r"^unet\.swin2\.block\.0\.mlp\.3\.",
            "stage2_block0.mlp.lin3.",
        )
        .with_key_remapping(r"^unet\.swin2\.block\.1\.attn\.", "stage2_block1.attn.")
        .with_key_remapping(
            r"^unet\.swin2\.block\.1\.mlp\.0\.",
            "stage2_block1.mlp.lin0.",
        )
        .with_key_remapping(
            r"^unet\.swin2\.block\.1\.mlp\.3\.",
            "stage2_block1.mlp.lin3.",
        )
        .with_key_remapping(r"^unet\.swin3\.block\.0\.attn\.", "stage3_block0.attn.")
        .with_key_remapping(
            r"^unet\.swin3\.block\.0\.mlp\.0\.",
            "stage3_block0.mlp.lin0.",
        )
        .with_key_remapping(
            r"^unet\.swin3\.block\.0\.mlp\.3\.",
            "stage3_block0.mlp.lin3.",
        )
        .with_key_remapping(r"^unet\.swin3\.block\.1\.attn\.", "stage3_block1.attn.")
        .with_key_remapping(
            r"^unet\.swin3\.block\.1\.mlp\.0\.",
            "stage3_block1.mlp.lin0.",
        )
        .with_key_remapping(
            r"^unet\.swin3\.block\.1\.mlp\.3\.",
            "stage3_block1.mlp.lin3.",
        )
        .with_key_remapping(r"^unet\.swin3\.block\.2\.attn\.", "stage3_block2.attn.")
        .with_key_remapping(
            r"^unet\.swin3\.block\.2\.mlp\.0\.",
            "stage3_block2.mlp.lin0.",
        )
        .with_key_remapping(
            r"^unet\.swin3\.block\.2\.mlp\.3\.",
            "stage3_block2.mlp.lin3.",
        )
        .with_key_remapping(r"^unet\.swin3\.block\.3\.attn\.", "stage3_block3.attn.")
        .with_key_remapping(
            r"^unet\.swin3\.block\.3\.mlp\.0\.",
            "stage3_block3.mlp.lin0.",
        )
        .with_key_remapping(
            r"^unet\.swin3\.block\.3\.mlp\.3\.",
            "stage3_block3.mlp.lin3.",
        )
        .with_key_remapping(r"^unet\.swin3\.block\.4\.attn\.", "stage3_block4.attn.")
        .with_key_remapping(
            r"^unet\.swin3\.block\.4\.mlp\.0\.",
            "stage3_block4.mlp.lin0.",
        )
        .with_key_remapping(
            r"^unet\.swin3\.block\.4\.mlp\.3\.",
            "stage3_block4.mlp.lin3.",
        )
        .with_key_remapping(r"^unet\.swin3\.block\.5\.attn\.", "stage3_block5.attn.")
        .with_key_remapping(
            r"^unet\.swin3\.block\.5\.mlp\.0\.",
            "stage3_block5.mlp.lin0.",
        )
        .with_key_remapping(
            r"^unet\.swin3\.block\.5\.mlp\.3\.",
            "stage3_block5.mlp.lin3.",
        )
        .with_key_remapping(r"^unet\.swin4\.block\.0\.attn\.", "stage4_block0.attn.")
        .with_key_remapping(
            r"^unet\.swin4\.block\.0\.mlp\.0\.",
            "stage4_block0.mlp.lin0.",
        )
        .with_key_remapping(
            r"^unet\.swin4\.block\.0\.mlp\.3\.",
            "stage4_block0.mlp.lin3.",
        )
        .with_key_remapping(r"^unet\.swin4\.block\.1\.attn\.", "stage4_block1.attn.")
        .with_key_remapping(
            r"^unet\.swin4\.block\.1\.mlp\.0\.",
            "stage4_block1.mlp.lin0.",
        )
        .with_key_remapping(
            r"^unet\.swin4\.block\.1\.mlp\.3\.",
            "stage4_block1.mlp.lin3.",
        )
        .with_key_remapping(r"^unet\.swin5\.block\.0\.attn\.", "stage5_block0.attn.")
        .with_key_remapping(
            r"^unet\.swin5\.block\.0\.mlp\.0\.",
            "stage5_block0.mlp.lin0.",
        )
        .with_key_remapping(
            r"^unet\.swin5\.block\.0\.mlp\.3\.",
            "stage5_block0.mlp.lin3.",
        )
        .with_key_remapping(r"^unet\.swin5\.block\.1\.attn\.", "stage5_block1.attn.")
        .with_key_remapping(
            r"^unet\.swin5\.block\.1\.mlp\.0\.",
            "stage5_block1.mlp.lin0.",
        )
        .with_key_remapping(
            r"^unet\.swin5\.block\.1\.mlp\.3\.",
            "stage5_block1.mlp.lin3.",
        )
        .with_key_remapping(r"^unet\.proj2\.", "proj2.")
        .with_key_remapping(r"^unet\.up1\.proj\.", "up1_proj.")
        .with_key_remapping(r"^unet\.up2\.proj\.", "up2_proj.")
        .with_key_remapping(r"^unet\.to_image\.proj\.", "to_image_proj.")
        .allow_partial(true)
}

fn init_waifu2x_probe_model<B: Backend>(
    device: &B::Device,
    output_scale: usize,
) -> Waifu2xPatchStem<B> {
    let (proj2, up1_proj, stage5_block0, stage5_block1, to_image_proj) = match output_scale {
        2 => (
            None,
            LinearConfig::new(192, 384).init(device),
            init_swin_block_from_dims(device, 96, 192),
            init_swin_block_from_dims(device, 96, 192),
            LinearConfig::new(96, 12).init(device),
        ),
        4 => (
            Some(LinearConfig::new(96, 192).init(device)),
            LinearConfig::new(192, 768).init(device),
            init_swin_block_from_dims(device, 192, 384),
            init_swin_block_from_dims(device, 192, 384),
            LinearConfig::new(192, 48).init(device),
        ),
        _ => panic!("unsupported waifu2x output scale {output_scale}"),
    };

    Waifu2xPatchStem {
        patch0: Conv2dConfig::new([3, 48], [3, 3]).init(device),
        patch2: Conv2dConfig::new([48, 96], [3, 3]).init(device),
        down1_conv: Conv2dConfig::new([96, 192], [2, 2])
            .with_stride([2, 2])
            .init(device),
        down2_conv: Conv2dConfig::new([192, 192], [2, 2])
            .with_stride([2, 2])
            .init(device),
        block0: init_swin_block_from_dims(device, 96, 192),
        block1: init_swin_block_from_dims(device, 96, 192),
        stage2_block0: init_swin_block_from_dims(device, 192, 384),
        stage2_block1: init_swin_block_from_dims(device, 192, 384),
        stage3_block0: init_swin_block_from_dims(device, 192, 384),
        stage3_block1: init_swin_block_from_dims(device, 192, 384),
        stage3_block2: init_swin_block_from_dims(device, 192, 384),
        stage3_block3: init_swin_block_from_dims(device, 192, 384),
        stage3_block4: init_swin_block_from_dims(device, 192, 384),
        stage3_block5: init_swin_block_from_dims(device, 192, 384),
        stage4_block0: init_swin_block_from_dims(device, 192, 384),
        stage4_block1: init_swin_block_from_dims(device, 192, 384),
        stage5_block0,
        stage5_block1,
        proj2,
        up1_proj,
        up2_proj: LinearConfig::new(192, 768).init(device),
        to_image_proj,
        output_scale,
    }
}

fn load_managed_image_model_burnpack_with_device<B: Backend>(
    cache_home: &CacheHome,
    model_name: &str,
    device: &B::Device,
) -> eyre::Result<Waifu2xPatchStem<B>> {
    let known = known_image_model(model_name).ok_or_else(|| unknown_model_error(model_name))?;
    ensure_image_model_runtime_is_implemented(known, "load")?;
    let model_dir = managed_image_model_dir(cache_home, known.name);
    let status = inspect_image_model_dir(known.name, &model_dir);
    if !matches!(status.state, ImageModelPreparationState::Prepared) {
        bail!(
            "image model `{}` is not prepared; expected Burnpack at {}",
            known.name,
            image_model_burnpack_path(&model_dir).display()
        );
    }
    let burnpack_path = image_model_burnpack_path(&model_dir);
    let output_scale = waifu2x_output_scale_for_architecture(known.architecture)?;
    let mut model = init_waifu2x_probe_model::<B>(device, output_scale);
    let mut store = BurnpackStore::from_file(&burnpack_path).allow_partial(true);
    let result = model.load_from(&mut store).wrap_err_with(|| {
        format!(
            "failed to load waifu2x Burnpack model from {}",
            burnpack_path.display()
        )
    })?;
    if !result.errors.is_empty() {
        bail!(
            "waifu2x Burnpack load reported tensor errors for {}: {:?}",
            burnpack_path.display(),
            result.errors
        );
    }
    if !result.missing.is_empty() || !result.unused.is_empty() {
        bail!(
            "waifu2x Burnpack load did not consume the full Burnpack for {}: missing={:?} unused={:?}",
            burnpack_path.display(),
            result.missing,
            result.unused
        );
    }
    Ok(model)
}

pub fn load_managed_image_model_burnpack(
    cache_home: &CacheHome,
    model_name: &str,
) -> eyre::Result<Waifu2xPatchStem<Waifu2xProbeBackend>> {
    let device = burn::backend::ndarray::NdArrayDevice::default();
    load_managed_image_model_burnpack_with_device::<Waifu2xProbeBackend>(
        cache_home, model_name, &device,
    )
}

pub fn validate_managed_image_model_burnpack_load(
    cache_home: &CacheHome,
    model_name: &str,
) -> eyre::Result<()> {
    let _model = load_managed_image_model_burnpack(cache_home, model_name)?;
    Ok(())
}

pub fn validate_managed_image_model_burnpack_load_cuda(
    cache_home: &CacheHome,
    model_name: &str,
    device: &CudaDevice,
) -> eyre::Result<()> {
    let _model = load_managed_image_model_burnpack_with_device::<Waifu2xInferenceBackend>(
        cache_home, model_name, device,
    )?;
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Waifu2xTiledRenderConfig {
    y_h: usize,
    y_w: usize,
    y_buffer_h: usize,
    y_buffer_w: usize,
    h_blocks: usize,
    w_blocks: usize,
    input_tile_step: usize,
    output_tile_step: usize,
    input_offset: usize,
    input_h: usize,
    input_w: usize,
    tile_size: usize,
    model_output_size: usize,
    pad_left: usize,
    pad_right: usize,
    pad_top: usize,
    pad_bottom: usize,
}

#[expect(
    clippy::too_many_arguments,
    reason = "this helper currently threads the full CLI/runtime request without an extra staging struct"
)]
fn upscale_managed_image_model_tiled_rgba_with_device<B: Backend>(
    cache_home: &CacheHome,
    model_name: &str,
    rgb_chw: &[f32],
    alpha_hw: Option<&[f32]>,
    blank_alpha: bool,
    width: u32,
    height: u32,
    requested_tile_size: u32,
    batch_size: u32,
    tta_enabled: bool,
    device: &B::Device,
) -> eyre::Result<Waifu2xUpscaledImage> {
    let known = known_image_model(model_name).ok_or_else(|| unknown_model_error(model_name))?;
    let model = load_managed_image_model_burnpack_with_device::<B>(cache_home, model_name, device)?;
    let actual_tile_size = choose_waifu2x_tile_size(
        requested_tile_size,
        width,
        height,
        u32::from(known.scale),
        known.model_offset,
        known.blend_size,
    )?;
    let prepared_rgb = match alpha_hw {
        Some(alpha_hw) if !blank_alpha => apply_waifu2x_alpha_border_padding(
            rgb_chw,
            alpha_hw,
            width,
            height,
            known.model_offset,
        )?,
        _ => rgb_chw.to_vec(),
    };
    let (rgb, output_width, output_height) = upscale_waifu2x_tiled_rgb_with_model(
        &model,
        device,
        &prepared_rgb,
        width,
        height,
        actual_tile_size,
        batch_size,
        u32::from(known.scale),
        known.model_offset,
        known.blend_size,
        tta_enabled,
        "rgb",
    )?;
    let alpha = match alpha_hw {
        Some(alpha_hw) => Some(upscale_waifu2x_tiled_alpha_with_model(
            &model,
            device,
            alpha_hw,
            blank_alpha,
            width,
            height,
            actual_tile_size,
            batch_size,
            tta_enabled,
            u32::from(known.scale),
            known.model_offset,
            known.blend_size,
        )?),
        None => None,
    };
    Ok((rgb, alpha, output_width, output_height, actual_tile_size))
}

#[expect(
    clippy::too_many_arguments,
    reason = "public image upscale entrypoint mirrors the CLI request shape"
)]
pub fn upscale_managed_image_model_tiled_rgba(
    cache_home: &CacheHome,
    model_name: &str,
    rgb_chw: &[f32],
    alpha_hw: Option<&[f32]>,
    blank_alpha: bool,
    width: u32,
    height: u32,
    requested_tile_size: u32,
    batch_size: u32,
    tta_enabled: bool,
) -> eyre::Result<Waifu2xUpscaledImage> {
    let device = burn::backend::ndarray::NdArrayDevice::default();
    upscale_managed_image_model_tiled_rgba_with_device::<Waifu2xCpuBackend>(
        cache_home,
        model_name,
        rgb_chw,
        alpha_hw,
        blank_alpha,
        width,
        height,
        requested_tile_size,
        batch_size,
        tta_enabled,
        &device,
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "public CUDA entrypoint mirrors the CLI request shape plus device"
)]
pub fn upscale_managed_image_model_tiled_rgba_cuda(
    cache_home: &CacheHome,
    model_name: &str,
    rgb_chw: &[f32],
    alpha_hw: Option<&[f32]>,
    blank_alpha: bool,
    width: u32,
    height: u32,
    requested_tile_size: u32,
    batch_size: u32,
    tta_enabled: bool,
    device: &CudaDevice,
) -> eyre::Result<Waifu2xUpscaledImage> {
    upscale_managed_image_model_tiled_rgba_with_device::<Waifu2xInferenceBackend>(
        cache_home,
        model_name,
        rgb_chw,
        alpha_hw,
        blank_alpha,
        width,
        height,
        requested_tile_size,
        batch_size,
        tta_enabled,
        device,
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "alpha upscale reuses the RGB tiled runtime inputs directly"
)]
fn upscale_waifu2x_tiled_alpha_with_model<B: Backend>(
    model: &Waifu2xPatchStem<B>,
    device: &B::Device,
    alpha_hw: &[f32],
    blank_alpha: bool,
    width: u32,
    height: u32,
    tile_size: u32,
    batch_size: u32,
    tta_enabled: bool,
    scale: u32,
    offset: u32,
    blend_size: u32,
) -> eyre::Result<Vec<f32>> {
    let width_usize =
        usize::try_from(width).wrap_err("waifu2x tiled alpha width does not fit in usize")?;
    let height_usize =
        usize::try_from(height).wrap_err("waifu2x tiled alpha height does not fit in usize")?;
    let expected_len = width_usize
        .checked_mul(height_usize)
        .ok_or_else(|| eyre::eyre!("waifu2x tiled alpha size overflowed usize"))?;
    if alpha_hw.len() != expected_len {
        bail!(
            "waifu2x tiled alpha expected {} values for {}x{}, got {}",
            expected_len,
            width,
            height,
            alpha_hw.len()
        );
    }

    let output_width = width_usize
        .checked_mul(
            usize::try_from(scale).wrap_err("waifu2x tiled alpha scale does not fit in usize")?,
        )
        .ok_or_else(|| eyre::eyre!("waifu2x tiled alpha output width overflowed usize"))?;
    let output_height = height_usize
        .checked_mul(
            usize::try_from(scale).wrap_err("waifu2x tiled alpha scale does not fit in usize")?,
        )
        .ok_or_else(|| eyre::eyre!("waifu2x tiled alpha output height overflowed usize"))?;
    if blank_alpha {
        return Ok(vec![1.0_f32; output_width * output_height]);
    }

    let alpha_rgb = expand_alpha_hw_to_rgb_chw(alpha_hw);
    let (alpha_rgb, actual_output_width, actual_output_height) =
        upscale_waifu2x_tiled_rgb_with_model(
            model,
            device,
            &alpha_rgb,
            width,
            height,
            tile_size,
            batch_size,
            scale,
            offset,
            blend_size,
            tta_enabled,
            "alpha",
        )?;
    let actual_output_width = usize::try_from(actual_output_width)
        .wrap_err("waifu2x tiled alpha output width does not fit in usize")?;
    let actual_output_height = usize::try_from(actual_output_height)
        .wrap_err("waifu2x tiled alpha output height does not fit in usize")?;
    collapse_rgb_chw_to_alpha_hw_mean(&alpha_rgb, actual_output_width, actual_output_height)
}

#[expect(
    clippy::too_many_arguments,
    reason = "the tiled RGB runtime needs the full render configuration at the callsite"
)]
#[expect(
    clippy::too_many_lines,
    reason = "the tiled RGB runtime keeps the batching loop and progress accounting together"
)]
fn upscale_waifu2x_tiled_rgb_with_model<B: Backend>(
    model: &Waifu2xPatchStem<B>,
    device: &B::Device,
    rgb_chw: &[f32],
    width: u32,
    height: u32,
    tile_size: u32,
    batch_size: u32,
    scale: u32,
    offset: u32,
    blend_size: u32,
    tta_enabled: bool,
    phase_label: &str,
) -> eyre::Result<(Vec<f32>, u32, u32)> {
    let width_usize =
        usize::try_from(width).wrap_err("waifu2x tiled input width does not fit in usize")?;
    let height_usize =
        usize::try_from(height).wrap_err("waifu2x tiled input height does not fit in usize")?;
    let expected_len = width_usize
        .checked_mul(height_usize)
        .and_then(|value| value.checked_mul(3))
        .ok_or_else(|| eyre::eyre!("waifu2x tiled input size overflowed usize"))?;
    if rgb_chw.len() != expected_len {
        bail!(
            "waifu2x tiled input expected {} RGB values for {}x{}, got {}",
            expected_len,
            width,
            height,
            rgb_chw.len()
        );
    }
    let batch_size =
        usize::try_from(batch_size).wrap_err("waifu2x tiled batch size does not fit in usize")?;
    if batch_size == 0 {
        bail!("waifu2x tiled batch size must be greater than zero");
    }

    let config =
        create_waifu2x_tiled_render_config(width, height, scale, offset, tile_size, blend_size)?;
    let padded = pad_rgb_replicate_chw(
        rgb_chw,
        width_usize,
        height_usize,
        config.pad_left,
        config.pad_right,
        config.pad_top,
        config.pad_bottom,
    );
    let output_channels = 3_usize;
    let buffer_len = output_channels
        .checked_mul(config.y_buffer_h)
        .and_then(|value| value.checked_mul(config.y_buffer_w))
        .ok_or_else(|| eyre::eyre!("waifu2x tiled output buffer overflowed usize"))?;
    let mut pixels = vec![0.0_f32; buffer_len];
    let mut weights = (blend_size > 0).then(|| vec![0.0_f32; buffer_len]);
    let blend_filter = if blend_size > 0 {
        Some(create_waifu2x_blend_filter(
            output_channels,
            config.model_output_size,
            usize::try_from(blend_size).unwrap_or_default(),
        )?)
    } else {
        None
    };

    let tile_len = output_channels
        .checked_mul(config.tile_size)
        .and_then(|value| value.checked_mul(config.tile_size))
        .ok_or_else(|| eyre::eyre!("waifu2x tiled input tile size overflowed usize"))?;
    let mut minibatch_tiles = Vec::with_capacity(tile_len * batch_size);
    let mut minibatch_indexes = Vec::with_capacity(batch_size);
    let total_tiles = config.h_blocks * config.w_blocks;
    let progress_step = total_tiles.max(1).div_ceil(10);
    let mut processed_tiles = 0_usize;
    let mut next_progress_mark = progress_step;

    tracing::info!(
        phase = phase_label,
        width,
        height,
        tile_size,
        batch_size,
        total_tiles,
        tile_grid_h = config.h_blocks,
        tile_grid_w = config.w_blocks,
        "waifu2x tiled inference started"
    );

    for h_block in 0..config.h_blocks {
        for w_block in 0..config.w_blocks {
            let input_row = h_block * config.input_tile_step;
            let input_column = w_block * config.input_tile_step;
            minibatch_tiles.extend(extract_rgb_tile_chw(
                &padded,
                config.input_w,
                config.input_h,
                input_row,
                input_column,
                config.tile_size,
            ));
            minibatch_indexes.push((h_block, w_block));

            if minibatch_indexes.len() == batch_size {
                let flushed_tiles = minibatch_indexes.len();
                flush_waifu2x_tile_batch(
                    phase_label,
                    model,
                    device,
                    &mut minibatch_tiles,
                    &mut minibatch_indexes,
                    &config,
                    tta_enabled,
                    &mut pixels,
                    weights.as_mut(),
                    blend_filter.as_deref(),
                )?;
                processed_tiles += flushed_tiles;
                log_waifu2x_tile_progress(
                    phase_label,
                    processed_tiles,
                    total_tiles,
                    &mut next_progress_mark,
                    progress_step,
                );
            }
        }
    }

    if !minibatch_indexes.is_empty() {
        let flushed_tiles = minibatch_indexes.len();
        flush_waifu2x_tile_batch(
            phase_label,
            model,
            device,
            &mut minibatch_tiles,
            &mut minibatch_indexes,
            &config,
            tta_enabled,
            &mut pixels,
            weights.as_mut(),
            blend_filter.as_deref(),
        )?;
        processed_tiles += flushed_tiles;
        log_waifu2x_tile_progress(
            phase_label,
            processed_tiles,
            total_tiles,
            &mut next_progress_mark,
            progress_step,
        );
    }

    Ok((
        crop_rgb_chw(&pixels, config.y_buffer_w, config.y_h, config.y_w),
        u32::try_from(config.y_w).wrap_err("waifu2x tiled output width does not fit in u32")?,
        u32::try_from(config.y_h).wrap_err("waifu2x tiled output height does not fit in u32")?,
    ))
}

fn log_waifu2x_tile_progress(
    phase_label: &str,
    processed_tiles: usize,
    total_tiles: usize,
    next_progress_mark: &mut usize,
    progress_step: usize,
) {
    if processed_tiles < *next_progress_mark && processed_tiles < total_tiles {
        return;
    }
    #[expect(
        clippy::cast_precision_loss,
        reason = "progress logging only needs approximate percentage output"
    )]
    let percent = if total_tiles == 0 {
        100.0
    } else {
        (processed_tiles as f32 / total_tiles as f32) * 100.0
    };
    tracing::info!(
        phase = phase_label,
        processed_tiles,
        total_tiles,
        percent = percent,
        "waifu2x tiled inference progress"
    );
    while *next_progress_mark <= processed_tiles {
        *next_progress_mark += progress_step;
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "tile flush needs the active buffers, blend state, and render config together"
)]
#[expect(
    clippy::too_many_lines,
    reason = "the flush path keeps heartbeat, forward pass, and writeback in one place"
)]
fn flush_waifu2x_tile_batch<B: Backend>(
    phase_label: &str,
    model: &Waifu2xPatchStem<B>,
    device: &B::Device,
    minibatch_tiles: &mut Vec<f32>,
    minibatch_indexes: &mut Vec<(usize, usize)>,
    config: &Waifu2xTiledRenderConfig,
    tta_enabled: bool,
    pixels: &mut [f32],
    weights: Option<&mut Vec<f32>>,
    blend_filter: Option<&[f32]>,
) -> eyre::Result<()> {
    let batch_len = minibatch_indexes.len();
    let model_batch_len = if tta_enabled {
        batch_len
            .checked_mul(WAIFU2X_TTA_TRANSFORM_COUNT)
            .ok_or_else(|| eyre::eyre!("waifu2x TTA batch size overflowed usize"))?
    } else {
        batch_len
    };
    let input_values = if tta_enabled {
        let expanded = expand_waifu2x_tta_batch(minibatch_tiles, batch_len, config.tile_size)?;
        minibatch_tiles.clear();
        expanded
    } else {
        std::mem::take(minibatch_tiles)
    };
    let input = Tensor::<B, 4>::from_data(
        TensorData::new(
            input_values,
            [model_batch_len, 3, config.tile_size, config.tile_size],
        ),
        device,
    );
    tracing::info!(
        phase = phase_label,
        batch_tiles = batch_len,
        model_batch_tiles = model_batch_len,
        tile_size = config.tile_size,
        tta_enabled,
        "waifu2x tile batch started"
    );
    let started = Instant::now();
    let done = Arc::new(AtomicBool::new(false));
    let done_for_heartbeat = Arc::clone(&done);
    let heartbeat_phase = phase_label.to_owned();
    let heartbeat = thread::spawn(move || {
        while !done_for_heartbeat.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_secs(5));
            if done_for_heartbeat.load(Ordering::Relaxed) {
                break;
            }
            tracing::info!(
                phase = heartbeat_phase.as_str(),
                batch_tiles = batch_len,
                model_batch_tiles = model_batch_len,
                elapsed_seconds = started.elapsed().as_secs_f32(),
                "waifu2x tile batch still running"
            );
        }
    });
    let output = model.forward(input);
    done.store(true, Ordering::Relaxed);
    let _ = heartbeat.join();
    tracing::info!(
        phase = phase_label,
        batch_tiles = batch_len,
        model_batch_tiles = model_batch_len,
        elapsed_seconds = started.elapsed().as_secs_f32(),
        "waifu2x tile batch completed"
    );
    let [actual_batch, channels, output_height, output_width] = output.dims();
    if actual_batch != model_batch_len || channels != 3 {
        bail!(
            "waifu2x tiled inference expected output dims [batch, 3, H, W], got {:?}",
            [actual_batch, channels, output_height, output_width]
        );
    }
    eyre::ensure!(
        config
            .output_tile_step
            .is_multiple_of(config.input_tile_step),
        "waifu2x tiled inference expected output tile step {} to be a multiple of input tile step {}",
        config.output_tile_step,
        config.input_tile_step
    );
    let logical_scale = config.output_tile_step / config.input_tile_step;
    eyre::ensure!(
        model.output_scale >= logical_scale && model.output_scale.is_multiple_of(logical_scale),
        "waifu2x tiled inference expected model output scale {} to be a multiple of logical scale {}",
        model.output_scale,
        logical_scale
    );
    let downscale_factor = model.output_scale / logical_scale;
    let runtime_output_size = config
        .model_output_size
        .checked_mul(downscale_factor)
        .ok_or_else(|| eyre::eyre!("waifu2x tiled runtime output size overflowed usize"))?;
    if output_height != runtime_output_size || output_width != runtime_output_size {
        bail!(
            "waifu2x tiled inference expected per-tile output {}x{}, got {}x{}",
            runtime_output_size,
            runtime_output_size,
            output_width,
            output_height
        );
    }

    let mut output_values = output
        .to_data()
        .to_vec::<f32>()
        .map_err(|error| eyre::eyre!("{error:?}"))
        .wrap_err("waifu2x tiled output tensor was not f32")?;
    if tta_enabled {
        output_values =
            merge_waifu2x_tta_output_tiles(&output_values, batch_len, runtime_output_size)?;
    }
    if downscale_factor > 1 {
        output_values = downscale_rgb_chw_tile_batch(
            &output_values,
            batch_len,
            runtime_output_size,
            runtime_output_size,
            downscale_factor,
        )?;
    }
    let tile_output_len = 3_usize
        .checked_mul(config.model_output_size)
        .and_then(|value| value.checked_mul(config.model_output_size))
        .ok_or_else(|| eyre::eyre!("waifu2x tiled output tile size overflowed usize"))?;

    match weights {
        Some(weights) => {
            let blend_filter = blend_filter.ok_or_else(|| {
                eyre::eyre!("waifu2x tiled blending expected a blend filter when weights exist")
            })?;
            for (batch_index, &(h_block, w_block)) in minibatch_indexes.iter().enumerate() {
                let start = batch_index * tile_output_len;
                let end = start + tile_output_len;
                accumulate_waifu2x_tile(
                    &output_values[start..end],
                    blend_filter,
                    config,
                    pixels,
                    weights,
                    h_block,
                    w_block,
                );
            }
        }
        None => {
            for (batch_index, &(h_block, w_block)) in minibatch_indexes.iter().enumerate() {
                let start = batch_index * tile_output_len;
                let end = start + tile_output_len;
                write_waifu2x_tile_without_blending(
                    &output_values[start..end],
                    config,
                    pixels,
                    h_block,
                    w_block,
                );
            }
        }
    }

    minibatch_indexes.clear();
    Ok(())
}

fn expand_waifu2x_tta_batch(
    minibatch_tiles: &[f32],
    batch_len: usize,
    tile_size: usize,
) -> eyre::Result<Vec<f32>> {
    let tile_len = 3_usize
        .checked_mul(tile_size)
        .and_then(|value| value.checked_mul(tile_size))
        .ok_or_else(|| eyre::eyre!("waifu2x TTA tile size overflowed usize"))?;
    let expected_len = batch_len
        .checked_mul(tile_len)
        .ok_or_else(|| eyre::eyre!("waifu2x TTA minibatch size overflowed usize"))?;
    if minibatch_tiles.len() != expected_len {
        bail!(
            "waifu2x TTA expected {} input tile values, got {}",
            expected_len,
            minibatch_tiles.len()
        );
    }
    let expanded_len = expected_len
        .checked_mul(WAIFU2X_TTA_TRANSFORM_COUNT)
        .ok_or_else(|| eyre::eyre!("waifu2x TTA expanded batch size overflowed usize"))?;
    let mut expanded = Vec::with_capacity(expanded_len);
    for batch_index in 0..batch_len {
        let tile_start = batch_index * tile_len;
        let tile_end = tile_start + tile_len;
        let tile = &minibatch_tiles[tile_start..tile_end];
        for transform in WAIFU2X_TTA_TRANSFORMS {
            expanded.extend(transform_rgb_chw_square_tile(tile, tile_size, transform)?);
        }
    }
    Ok(expanded)
}

fn merge_waifu2x_tta_output_tiles(
    output_values: &[f32],
    batch_len: usize,
    tile_size: usize,
) -> eyre::Result<Vec<f32>> {
    let tile_len = 3_usize
        .checked_mul(tile_size)
        .and_then(|value| value.checked_mul(tile_size))
        .ok_or_else(|| eyre::eyre!("waifu2x TTA output tile size overflowed usize"))?;
    let expected_len = batch_len
        .checked_mul(WAIFU2X_TTA_TRANSFORM_COUNT)
        .and_then(|value| value.checked_mul(tile_len))
        .ok_or_else(|| eyre::eyre!("waifu2x TTA output batch size overflowed usize"))?;
    if output_values.len() != expected_len {
        bail!(
            "waifu2x TTA expected {} output tile values, got {}",
            expected_len,
            output_values.len()
        );
    }
    let merged_len = batch_len
        .checked_mul(tile_len)
        .ok_or_else(|| eyre::eyre!("waifu2x TTA merged output size overflowed usize"))?;
    let mut merged = vec![0.0_f32; merged_len];
    for batch_index in 0..batch_len {
        let merged_start = batch_index * tile_len;
        let merged_end = merged_start + tile_len;
        let merged_tile = &mut merged[merged_start..merged_end];
        let tta_start = batch_index * WAIFU2X_TTA_TRANSFORM_COUNT * tile_len;
        for (transform_index, transform) in WAIFU2X_TTA_TRANSFORMS.into_iter().enumerate() {
            let tile_start = tta_start + transform_index * tile_len;
            let tile_end = tile_start + tile_len;
            let restored = transform_rgb_chw_square_tile(
                &output_values[tile_start..tile_end],
                tile_size,
                transform.inverse(),
            )?;
            for (merged_value, restored_value) in merged_tile.iter_mut().zip(restored) {
                *merged_value += restored_value;
            }
        }
        for value in merged_tile {
            *value /= WAIFU2X_TTA_TRANSFORM_COUNT_F32;
        }
    }
    Ok(merged)
}

fn transform_rgb_chw_square_tile(
    tile_rgb: &[f32],
    tile_size: usize,
    transform: Waifu2xTtaTransform,
) -> eyre::Result<Vec<f32>> {
    let plane_len = tile_size
        .checked_mul(tile_size)
        .ok_or_else(|| eyre::eyre!("waifu2x TTA plane size overflowed usize"))?;
    let expected_len = plane_len
        .checked_mul(3)
        .ok_or_else(|| eyre::eyre!("waifu2x TTA tile length overflowed usize"))?;
    if tile_rgb.len() != expected_len {
        bail!(
            "waifu2x TTA expected square RGB tile length {}, got {}",
            expected_len,
            tile_rgb.len()
        );
    }
    let mut transformed = vec![0.0_f32; expected_len];
    for channel in 0..3 {
        let channel_base = channel * plane_len;
        for row in 0..tile_size {
            for column in 0..tile_size {
                let (source_row, source_column) =
                    map_waifu2x_tta_coords(transform, tile_size, row, column);
                let dest_index = channel_base + row * tile_size + column;
                let source_index = channel_base + source_row * tile_size + source_column;
                transformed[dest_index] = tile_rgb[source_index];
            }
        }
    }
    Ok(transformed)
}

fn map_waifu2x_tta_coords(
    transform: Waifu2xTtaTransform,
    tile_size: usize,
    row: usize,
    column: usize,
) -> (usize, usize) {
    let last = tile_size - 1;
    match transform {
        Waifu2xTtaTransform::Identity => (row, column),
        Waifu2xTtaTransform::Rotate90 => (last - column, row),
        Waifu2xTtaTransform::Rotate180 => (last - row, last - column),
        Waifu2xTtaTransform::Rotate270 => (column, last - row),
        Waifu2xTtaTransform::FlipHorizontal => (row, last - column),
        Waifu2xTtaTransform::FlipVertical => (last - row, column),
        Waifu2xTtaTransform::Transpose => (column, row),
        Waifu2xTtaTransform::Transverse => (last - column, last - row),
    }
}

#[cfg(test)]
fn is_full_check_enabled() -> bool {
    std::env::var(TEAMY_STUDIO_FULL_CHECK_ENV_VAR)
        .ok()
        .is_some_and(|value| value == "1")
}

fn accumulate_waifu2x_tile(
    tile_rgb: &[f32],
    blend_filter: &[f32],
    config: &Waifu2xTiledRenderConfig,
    pixels: &mut [f32],
    weights: &mut [f32],
    h_block: usize,
    w_block: usize,
) {
    let row_base = h_block * config.output_tile_step;
    let column_base = w_block * config.output_tile_step;
    let tile_plane_len = config.model_output_size * config.model_output_size;
    let buffer_plane_len = config.y_buffer_h * config.y_buffer_w;
    for channel in 0..3 {
        let tile_channel_base = channel * tile_plane_len;
        let buffer_channel_base = channel * buffer_plane_len;
        for row in 0..config.model_output_size {
            for column in 0..config.model_output_size {
                let tile_index = tile_channel_base + row * config.model_output_size + column;
                let buffer_index = buffer_channel_base
                    + (row_base + row) * config.y_buffer_w
                    + (column_base + column);
                let old_weight = weights[buffer_index];
                let next_weight = old_weight + blend_filter[tile_index];
                let old_ratio = if next_weight > 0.0 {
                    old_weight / next_weight
                } else {
                    0.0
                };
                let new_ratio = 1.0 - old_ratio;
                pixels[buffer_index] =
                    pixels[buffer_index] * old_ratio + tile_rgb[tile_index] * new_ratio;
                weights[buffer_index] = next_weight;
            }
        }
    }
}

fn write_waifu2x_tile_without_blending(
    tile_rgb: &[f32],
    config: &Waifu2xTiledRenderConfig,
    pixels: &mut [f32],
    h_block: usize,
    w_block: usize,
) {
    let row_base = h_block * config.output_tile_step;
    let column_base = w_block * config.output_tile_step;
    let tile_plane_len = config.model_output_size * config.model_output_size;
    let buffer_plane_len = config.y_buffer_h * config.y_buffer_w;
    for channel in 0..3 {
        let tile_channel_base = channel * tile_plane_len;
        let buffer_channel_base = channel * buffer_plane_len;
        for row in 0..config.model_output_size {
            for column in 0..config.model_output_size {
                let tile_index = tile_channel_base + row * config.model_output_size + column;
                let buffer_index = buffer_channel_base
                    + (row_base + row) * config.y_buffer_w
                    + (column_base + column);
                pixels[buffer_index] = tile_rgb[tile_index];
            }
        }
    }
}

fn create_waifu2x_tiled_render_config(
    width: u32,
    height: u32,
    scale: u32,
    offset: u32,
    tile_size: u32,
    blend_size: u32,
) -> eyre::Result<Waifu2xTiledRenderConfig> {
    let width = usize::try_from(width).wrap_err("waifu2x tiled width does not fit in usize")?;
    let height = usize::try_from(height).wrap_err("waifu2x tiled height does not fit in usize")?;
    let scale = usize::try_from(scale).wrap_err("waifu2x tiled scale does not fit in usize")?;
    let offset = usize::try_from(offset).wrap_err("waifu2x tiled offset does not fit in usize")?;
    let tile_size =
        usize::try_from(tile_size).wrap_err("waifu2x tiled tile size does not fit in usize")?;
    let blend_size =
        usize::try_from(blend_size).wrap_err("waifu2x tiled blend size does not fit in usize")?;
    let input_offset = offset.div_ceil(scale);
    let input_blend_size = blend_size.div_ceil(scale);
    let input_tile_step = tile_size
        .checked_sub(input_offset * 2 + input_blend_size)
        .ok_or_else(|| {
            eyre::eyre!("waifu2x tiled tile size {tile_size} is too small for offset/blend")
        })?;
    if input_tile_step == 0 {
        bail!("waifu2x tiled tile size produced a zero tile step");
    }

    let mut h_blocks = 0_usize;
    let mut input_h = 0_usize;
    while input_h < height + input_offset * 2 {
        input_h = h_blocks * input_tile_step + tile_size;
        h_blocks += 1;
    }
    let mut w_blocks = 0_usize;
    let mut input_w = 0_usize;
    while input_w < width + input_offset * 2 {
        input_w = w_blocks * input_tile_step + tile_size;
        w_blocks += 1;
    }

    let output_tile_step = input_tile_step * scale;
    let y_buffer_h = input_h * scale;
    let y_buffer_w = input_w * scale;
    let y_h = height * scale;
    let y_w = width * scale;
    let model_output_size = tile_size
        .checked_mul(scale)
        .and_then(|value| value.checked_sub(offset * 2))
        .ok_or_else(|| eyre::eyre!("waifu2x tiled model output size underflowed"))?;

    Ok(Waifu2xTiledRenderConfig {
        y_h,
        y_w,
        y_buffer_h,
        y_buffer_w,
        h_blocks,
        w_blocks,
        input_tile_step,
        output_tile_step,
        input_offset,
        input_h,
        input_w,
        tile_size,
        model_output_size,
        pad_left: input_offset,
        pad_right: input_w - (width + input_offset),
        pad_top: input_offset,
        pad_bottom: input_h - (height + input_offset),
    })
}

#[expect(
    clippy::cast_precision_loss,
    reason = "blend weights are computed in f32 because the output filter is f32"
)]
fn create_waifu2x_blend_filter(
    channels: usize,
    model_output_size: usize,
    blend_size: usize,
) -> eyre::Result<Vec<f32>> {
    let plane_len = model_output_size
        .checked_mul(model_output_size)
        .ok_or_else(|| eyre::eyre!("waifu2x blend filter plane size overflowed usize"))?;
    let mut filter = vec![1.0_f32; channels * plane_len];
    if blend_size == 0 {
        return Ok(filter);
    }
    let denominator = (blend_size + 1) as f32;
    for row in 0..model_output_size {
        for column in 0..model_output_size {
            let distance = row
                .min(column)
                .min(model_output_size - 1 - row)
                .min(model_output_size - 1 - column);
            let weight = if distance >= blend_size {
                1.0
            } else {
                (distance + 1) as f32 / denominator
            };
            for channel in 0..channels {
                filter[channel * plane_len + row * model_output_size + column] = weight;
            }
        }
    }
    Ok(filter)
}

fn pad_rgb_replicate_chw(
    rgb_chw: &[f32],
    width: usize,
    height: usize,
    pad_left: usize,
    pad_right: usize,
    pad_top: usize,
    pad_bottom: usize,
) -> Vec<f32> {
    let padded_width = width + pad_left + pad_right;
    let padded_height = height + pad_top + pad_bottom;
    let plane_len = width * height;
    let padded_plane_len = padded_width * padded_height;
    let mut padded = vec![0.0_f32; 3 * padded_plane_len];
    for channel in 0..3 {
        let input_channel_base = channel * plane_len;
        let padded_channel_base = channel * padded_plane_len;
        for row in 0..padded_height {
            let src_row = row.saturating_sub(pad_top).min(height - 1);
            for column in 0..padded_width {
                let src_column = column.saturating_sub(pad_left).min(width - 1);
                padded[padded_channel_base + row * padded_width + column] =
                    rgb_chw[input_channel_base + src_row * width + src_column];
            }
        }
    }
    padded
}

fn extract_rgb_tile_chw(
    padded_rgb: &[f32],
    padded_width: usize,
    padded_height: usize,
    start_row: usize,
    start_column: usize,
    tile_size: usize,
) -> Vec<f32> {
    let padded_plane_len = padded_width * padded_height;
    let mut tile = vec![0.0_f32; 3 * tile_size * tile_size];
    for channel in 0..3 {
        let padded_channel_base = channel * padded_plane_len;
        let tile_channel_base = channel * tile_size * tile_size;
        for row in 0..tile_size {
            for column in 0..tile_size {
                tile[tile_channel_base + row * tile_size + column] = padded_rgb[padded_channel_base
                    + (start_row + row) * padded_width
                    + (start_column + column)];
            }
        }
    }
    tile
}

fn crop_rgb_chw(
    pixels: &[f32],
    source_width: usize,
    crop_height: usize,
    crop_width: usize,
) -> Vec<f32> {
    let source_plane_len = pixels.len() / 3;
    let mut cropped = vec![0.0_f32; 3 * crop_height * crop_width];
    for channel in 0..3 {
        let source_channel_base = channel * source_plane_len;
        let cropped_channel_base = channel * crop_height * crop_width;
        for row in 0..crop_height {
            for column in 0..crop_width {
                cropped[cropped_channel_base + row * crop_width + column] =
                    pixels[source_channel_base + row * source_width + column];
            }
        }
    }
    cropped
}

fn apply_waifu2x_alpha_border_padding(
    rgb_chw: &[f32],
    alpha_hw: &[f32],
    width: u32,
    height: u32,
    offset: u32,
) -> eyre::Result<Vec<f32>> {
    let width =
        usize::try_from(width).wrap_err("waifu2x alpha border width does not fit in usize")?;
    let height =
        usize::try_from(height).wrap_err("waifu2x alpha border height does not fit in usize")?;
    let pixel_count = width
        .checked_mul(height)
        .ok_or_else(|| eyre::eyre!("waifu2x alpha border pixel count overflowed usize"))?;
    if rgb_chw.len() != pixel_count * 3 {
        bail!(
            "waifu2x alpha border expected {} RGB values for {}x{}, got {}",
            pixel_count * 3,
            width,
            height,
            rgb_chw.len()
        );
    }
    if alpha_hw.len() != pixel_count {
        bail!(
            "waifu2x alpha border expected {} alpha values for {}x{}, got {}",
            pixel_count,
            width,
            height,
            alpha_hw.len()
        );
    }

    let mut rgb = rgb_chw.to_vec();
    let mut mask = vec![0.0_f32; pixel_count];
    for index in 0..pixel_count {
        if alpha_hw[index] > 0.0 {
            mask[index] = 1.0;
        } else {
            rgb[index] = 0.0;
            rgb[pixel_count + index] = 0.0;
            rgb[pixel_count * 2 + index] = 0.0;
        }
    }

    for _ in
        0..usize::try_from(offset).wrap_err("waifu2x alpha border offset does not fit in usize")?
    {
        let mask_weight = channelwise_sum_hw(&mask, width, height);
        let border = channelwise_sum_rgb_chw(&rgb, width, height);
        for index in 0..pixel_count {
            if mask[index] < 1.0 {
                let weight = mask_weight[index] + 1e-7;
                rgb[index] = (border[index] / weight).clamp(0.0, 1.0);
                rgb[pixel_count + index] = (border[pixel_count + index] / weight).clamp(0.0, 1.0);
                rgb[pixel_count * 2 + index] =
                    (border[pixel_count * 2 + index] / weight).clamp(0.0, 1.0);
            }
        }
        for index in 0..pixel_count {
            mask[index] = if mask_weight[index] > 0.0 { 1.0 } else { 0.0 };
        }
    }

    Ok(rgb)
}

fn channelwise_sum_hw(values: &[f32], width: usize, height: usize) -> Vec<f32> {
    let mut output = vec![0.0_f32; values.len()];
    for row in 0..height {
        for column in 0..width {
            let row_start = row.saturating_sub(1);
            let row_end = (row + 1).min(height - 1);
            let column_start = column.saturating_sub(1);
            let column_end = (column + 1).min(width - 1);
            let mut sum = 0.0_f32;
            for kernel_row in row_start..=row_end {
                for kernel_column in column_start..=column_end {
                    sum += values[kernel_row * width + kernel_column];
                }
            }
            output[row * width + column] = sum;
        }
    }
    output
}

fn channelwise_sum_rgb_chw(values: &[f32], width: usize, height: usize) -> Vec<f32> {
    let plane_len = width * height;
    let mut output = vec![0.0_f32; values.len()];
    for channel in 0..3 {
        let channel_base = channel * plane_len;
        for row in 0..height {
            for column in 0..width {
                let row_start = row.saturating_sub(1);
                let row_end = (row + 1).min(height - 1);
                let column_start = column.saturating_sub(1);
                let column_end = (column + 1).min(width - 1);
                let mut sum = 0.0_f32;
                for kernel_row in row_start..=row_end {
                    for kernel_column in column_start..=column_end {
                        sum += values[channel_base + kernel_row * width + kernel_column];
                    }
                }
                output[channel_base + row * width + column] = sum;
            }
        }
    }
    output
}

fn expand_alpha_hw_to_rgb_chw(alpha_hw: &[f32]) -> Vec<f32> {
    let mut rgb = Vec::with_capacity(alpha_hw.len() * 3);
    rgb.extend_from_slice(alpha_hw);
    rgb.extend_from_slice(alpha_hw);
    rgb.extend_from_slice(alpha_hw);
    rgb
}

fn collapse_rgb_chw_to_alpha_hw_mean(
    rgb_chw: &[f32],
    width: usize,
    height: usize,
) -> eyre::Result<Vec<f32>> {
    let pixel_count = width
        .checked_mul(height)
        .ok_or_else(|| eyre::eyre!("waifu2x alpha collapse pixel count overflowed usize"))?;
    if rgb_chw.len() != pixel_count * 3 {
        bail!(
            "waifu2x alpha collapse expected {} RGB values for {}x{}, got {}",
            pixel_count * 3,
            width,
            height,
            rgb_chw.len()
        );
    }
    let mut alpha = vec![0.0_f32; pixel_count];
    for index in 0..pixel_count {
        alpha[index] =
            ((rgb_chw[index] + rgb_chw[pixel_count + index] + rgb_chw[pixel_count * 2 + index])
                / 3.0)
                .clamp(0.0, 1.0);
    }
    Ok(alpha)
}

fn downscale_rgb_chw_tile(
    rgb_chw: &[f32],
    width: usize,
    height: usize,
    downscale_factor: usize,
) -> eyre::Result<Vec<f32>> {
    eyre::ensure!(
        downscale_factor > 0,
        "waifu2x tile downscale factor must be greater than zero"
    );
    eyre::ensure!(
        width.is_multiple_of(downscale_factor) && height.is_multiple_of(downscale_factor),
        "waifu2x tile downscale expected {width}x{height} to be divisible by {downscale_factor}"
    );
    let pixel_count = width
        .checked_mul(height)
        .ok_or_else(|| eyre::eyre!("waifu2x tile downscale pixel count overflowed usize"))?;
    if rgb_chw.len() != pixel_count * 3 {
        bail!(
            "waifu2x tile downscale expected {} RGB values for {}x{}, got {}",
            pixel_count * 3,
            width,
            height,
            rgb_chw.len()
        );
    }
    let mut interleaved = vec![0.0_f32; rgb_chw.len()];
    for row in 0..height {
        for column in 0..width {
            let pixel_index = row * width + column;
            let interleaved_base = pixel_index * 3;
            interleaved[interleaved_base] = rgb_chw[pixel_index];
            interleaved[interleaved_base + 1] = rgb_chw[pixel_count + pixel_index];
            interleaved[interleaved_base + 2] = rgb_chw[pixel_count * 2 + pixel_index];
        }
    }

    let width_u32 = u32::try_from(width).wrap_err("waifu2x tile width does not fit in u32")?;
    let height_u32 = u32::try_from(height).wrap_err("waifu2x tile height does not fit in u32")?;
    let image = image::ImageBuffer::<image::Rgb<f32>, Vec<f32>>::from_raw(
        width_u32,
        height_u32,
        interleaved,
    )
    .ok_or_else(|| eyre::eyre!("failed to assemble waifu2x RGB tile for downscale"))?;

    let target_width = width / downscale_factor;
    let target_height = height / downscale_factor;
    let resized = image::imageops::resize(
        &image,
        u32::try_from(target_width).wrap_err("waifu2x target tile width does not fit in u32")?,
        u32::try_from(target_height).wrap_err("waifu2x target tile height does not fit in u32")?,
        image::imageops::FilterType::CatmullRom,
    );
    let resized_raw = resized.into_raw();
    let target_pixel_count = target_width
        .checked_mul(target_height)
        .ok_or_else(|| eyre::eyre!("waifu2x target tile pixel count overflowed usize"))?;
    let mut output = vec![0.0_f32; target_pixel_count * 3];
    for row in 0..target_height {
        for column in 0..target_width {
            let pixel_index = row * target_width + column;
            let interleaved_base = pixel_index * 3;
            output[pixel_index] = resized_raw[interleaved_base];
            output[target_pixel_count + pixel_index] = resized_raw[interleaved_base + 1];
            output[target_pixel_count * 2 + pixel_index] = resized_raw[interleaved_base + 2];
        }
    }
    Ok(output)
}

fn downscale_rgb_chw_tile_batch(
    batch_rgb_chw: &[f32],
    batch_len: usize,
    width: usize,
    height: usize,
    downscale_factor: usize,
) -> eyre::Result<Vec<f32>> {
    let tile_len = width
        .checked_mul(height)
        .and_then(|value| value.checked_mul(3))
        .ok_or_else(|| eyre::eyre!("waifu2x tile batch length overflowed usize"))?;
    if batch_rgb_chw.len() != tile_len * batch_len {
        bail!(
            "waifu2x tile batch downscale expected {} values for {} tiles of size {}x{}, got {}",
            tile_len * batch_len,
            batch_len,
            width,
            height,
            batch_rgb_chw.len()
        );
    }

    let target_tile_len = (width / downscale_factor)
        .checked_mul(height / downscale_factor)
        .and_then(|value| value.checked_mul(3))
        .ok_or_else(|| eyre::eyre!("waifu2x target tile batch length overflowed usize"))?;
    let mut output = Vec::with_capacity(target_tile_len * batch_len);
    for batch_index in 0..batch_len {
        let start = batch_index * tile_len;
        let end = start + tile_len;
        output.extend(downscale_rgb_chw_tile(
            &batch_rgb_chw[start..end],
            width,
            height,
            downscale_factor,
        )?);
    }
    Ok(output)
}

fn find_valid_waifu2x_tile_size(base_tile_size: u32) -> eyre::Result<u32> {
    let mut tile_size = base_tile_size;
    while tile_size > 0 {
        if is_valid_waifu2x_tile_size(tile_size) {
            return Ok(tile_size);
        }
        tile_size -= 1;
    }
    bail!("could not find valid waifu2x tile size from requested base {base_tile_size}")
}

fn is_valid_waifu2x_tile_size(tile_size: u32) -> bool {
    tile_size > 16 && (tile_size - 16).is_multiple_of(12) && (tile_size - 16).is_multiple_of(16)
}

fn choose_waifu2x_tile_size(
    requested_tile_size: u32,
    width: u32,
    height: u32,
    scale: u32,
    offset: u32,
    blend_size: u32,
) -> eyre::Result<u32> {
    let requested_tile_size = find_valid_waifu2x_tile_size(requested_tile_size)?;
    let mut candidate = 17_u32;
    let mut best = requested_tile_size;
    while candidate <= requested_tile_size {
        if is_valid_waifu2x_tile_size(candidate) {
            let config = create_waifu2x_tiled_render_config(
                width, height, scale, offset, candidate, blend_size,
            )?;
            if config.h_blocks == 1 && config.w_blocks == 1 {
                best = candidate;
                break;
            }
        }
        candidate += 1;
    }
    Ok(best)
}

fn build_waifu2x_checkpoint_store(checkpoint_path: &Path) -> PytorchStore {
    configure_waifu2x_checkpoint_store(PytorchStore::from_file(checkpoint_path))
}

fn checkpoint_tensor_preview(
    snapshot: Option<&burn_store::TensorSnapshot>,
    key: &str,
) -> eyre::Result<ImageModelCheckpointTensorPreview> {
    let snapshot = snapshot.ok_or_else(|| {
        eyre::eyre!("waifu2x checkpoint preview key `{key}` disappeared during inspection")
    })?;
    Ok(ImageModelCheckpointTensorPreview {
        key: key.to_owned(),
        dtype: format!("{:?}", snapshot.dtype),
        shape: snapshot
            .shape
            .iter()
            .copied()
            .map(|dimension| {
                u64::try_from(dimension)
                    .wrap_err("waifu2x checkpoint tensor dimension does not fit in u64")
            })
            .collect::<eyre::Result<Vec<_>>>()?,
        data_size_bytes: u64::try_from(snapshot.data_len())
            .wrap_err("waifu2x checkpoint tensor byte size does not fit in u64")?,
    })
}

#[expect(
    clippy::too_many_lines,
    reason = "the burn load probe intentionally enumerates every required checkpoint tensor for diagnostics"
)]
fn inspect_image_model_burn_load_probe(
    source_artifacts: &ImageModelSourceArtifactsReport,
) -> eyre::Result<Option<ImageModelCheckpointBurnLoadProbeReport>> {
    if !source_artifacts.checkpoint_exists {
        return Ok(None);
    }

    let checkpoint_path = Path::new(&source_artifacts.checkpoint_path);
    let reader =
        PytorchReader::with_top_level_key(checkpoint_path, "state_dict").wrap_err_with(|| {
            format!(
                "failed to read waifu2x state_dict from {} for Burn probe",
                checkpoint_path.display()
            )
        })?;
    let patch0 = required_checkpoint_snapshot(&reader, "unet.patch.0.weight")?;
    let patch2 = required_checkpoint_snapshot(&reader, "unet.patch.2.weight")?;
    let down1_conv = required_checkpoint_snapshot(&reader, "unet.down1.conv.weight")?;
    let down2_conv = required_checkpoint_snapshot(&reader, "unet.down2.conv.weight")?;
    let block0_qkv = required_checkpoint_snapshot(&reader, "unet.swin1.block.0.attn.qkv.weight")?;
    let block0_proj = required_checkpoint_snapshot(&reader, "unet.swin1.block.0.attn.proj.weight")?;
    let block0_mlp0 = required_checkpoint_snapshot(&reader, "unet.swin1.block.0.mlp.0.weight")?;
    let block0_mlp3 = required_checkpoint_snapshot(&reader, "unet.swin1.block.0.mlp.3.weight")?;
    let block0_bias_table = required_checkpoint_snapshot(
        &reader,
        "unet.swin1.block.0.attn.relative_position_bias_table",
    )?;
    let block0_index =
        required_checkpoint_snapshot(&reader, "unet.swin1.block.0.attn.relative_position_index")?;
    let block1_qkv = required_checkpoint_snapshot(&reader, "unet.swin1.block.1.attn.qkv.weight")?;
    let block1_proj = required_checkpoint_snapshot(&reader, "unet.swin1.block.1.attn.proj.weight")?;
    let block1_mlp0 = required_checkpoint_snapshot(&reader, "unet.swin1.block.1.mlp.0.weight")?;
    let block1_mlp3 = required_checkpoint_snapshot(&reader, "unet.swin1.block.1.mlp.3.weight")?;
    let block1_bias_table = required_checkpoint_snapshot(
        &reader,
        "unet.swin1.block.1.attn.relative_position_bias_table",
    )?;
    let block1_index =
        required_checkpoint_snapshot(&reader, "unet.swin1.block.1.attn.relative_position_index")?;
    let stage2_block0_qkv =
        required_checkpoint_snapshot(&reader, "unet.swin2.block.0.attn.qkv.weight")?;
    let stage2_block0_proj =
        required_checkpoint_snapshot(&reader, "unet.swin2.block.0.attn.proj.weight")?;
    let stage2_block0_mlp0 =
        required_checkpoint_snapshot(&reader, "unet.swin2.block.0.mlp.0.weight")?;
    let stage2_block0_mlp3 =
        required_checkpoint_snapshot(&reader, "unet.swin2.block.0.mlp.3.weight")?;
    let stage2_block0_bias_table = required_checkpoint_snapshot(
        &reader,
        "unet.swin2.block.0.attn.relative_position_bias_table",
    )?;
    let stage2_block0_index =
        required_checkpoint_snapshot(&reader, "unet.swin2.block.0.attn.relative_position_index")?;
    let stage2_block1_qkv =
        required_checkpoint_snapshot(&reader, "unet.swin2.block.1.attn.qkv.weight")?;
    let stage2_block1_proj =
        required_checkpoint_snapshot(&reader, "unet.swin2.block.1.attn.proj.weight")?;
    let stage2_block1_mlp0 =
        required_checkpoint_snapshot(&reader, "unet.swin2.block.1.mlp.0.weight")?;
    let stage2_block1_mlp3 =
        required_checkpoint_snapshot(&reader, "unet.swin2.block.1.mlp.3.weight")?;
    let stage2_block1_bias_table = required_checkpoint_snapshot(
        &reader,
        "unet.swin2.block.1.attn.relative_position_bias_table",
    )?;
    let stage2_block1_index =
        required_checkpoint_snapshot(&reader, "unet.swin2.block.1.attn.relative_position_index")?;
    let stage3_block0_qkv =
        required_checkpoint_snapshot(&reader, "unet.swin3.block.0.attn.qkv.weight")?;
    let stage3_block0_proj =
        required_checkpoint_snapshot(&reader, "unet.swin3.block.0.attn.proj.weight")?;
    let stage3_block0_mlp0 =
        required_checkpoint_snapshot(&reader, "unet.swin3.block.0.mlp.0.weight")?;
    let stage3_block0_mlp3 =
        required_checkpoint_snapshot(&reader, "unet.swin3.block.0.mlp.3.weight")?;
    let stage3_block0_bias_table = required_checkpoint_snapshot(
        &reader,
        "unet.swin3.block.0.attn.relative_position_bias_table",
    )?;
    let stage3_block0_index =
        required_checkpoint_snapshot(&reader, "unet.swin3.block.0.attn.relative_position_index")?;
    let stage3_block1_qkv =
        required_checkpoint_snapshot(&reader, "unet.swin3.block.1.attn.qkv.weight")?;
    let stage3_block1_proj =
        required_checkpoint_snapshot(&reader, "unet.swin3.block.1.attn.proj.weight")?;
    let stage3_block1_mlp0 =
        required_checkpoint_snapshot(&reader, "unet.swin3.block.1.mlp.0.weight")?;
    let stage3_block1_mlp3 =
        required_checkpoint_snapshot(&reader, "unet.swin3.block.1.mlp.3.weight")?;
    let stage3_block1_bias_table = required_checkpoint_snapshot(
        &reader,
        "unet.swin3.block.1.attn.relative_position_bias_table",
    )?;
    let stage3_block1_index =
        required_checkpoint_snapshot(&reader, "unet.swin3.block.1.attn.relative_position_index")?;
    let stage3_block2_qkv =
        required_checkpoint_snapshot(&reader, "unet.swin3.block.2.attn.qkv.weight")?;
    let stage3_block2_proj =
        required_checkpoint_snapshot(&reader, "unet.swin3.block.2.attn.proj.weight")?;
    let stage3_block2_mlp0 =
        required_checkpoint_snapshot(&reader, "unet.swin3.block.2.mlp.0.weight")?;
    let stage3_block2_mlp3 =
        required_checkpoint_snapshot(&reader, "unet.swin3.block.2.mlp.3.weight")?;
    let stage3_block2_bias_table = required_checkpoint_snapshot(
        &reader,
        "unet.swin3.block.2.attn.relative_position_bias_table",
    )?;
    let stage3_block2_index =
        required_checkpoint_snapshot(&reader, "unet.swin3.block.2.attn.relative_position_index")?;
    let stage3_block3_qkv =
        required_checkpoint_snapshot(&reader, "unet.swin3.block.3.attn.qkv.weight")?;
    let stage3_block3_proj =
        required_checkpoint_snapshot(&reader, "unet.swin3.block.3.attn.proj.weight")?;
    let stage3_block3_mlp0 =
        required_checkpoint_snapshot(&reader, "unet.swin3.block.3.mlp.0.weight")?;
    let stage3_block3_mlp3 =
        required_checkpoint_snapshot(&reader, "unet.swin3.block.3.mlp.3.weight")?;
    let stage3_block3_bias_table = required_checkpoint_snapshot(
        &reader,
        "unet.swin3.block.3.attn.relative_position_bias_table",
    )?;
    let stage3_block3_index =
        required_checkpoint_snapshot(&reader, "unet.swin3.block.3.attn.relative_position_index")?;
    let stage3_block4_qkv =
        required_checkpoint_snapshot(&reader, "unet.swin3.block.4.attn.qkv.weight")?;
    let stage3_block4_proj =
        required_checkpoint_snapshot(&reader, "unet.swin3.block.4.attn.proj.weight")?;
    let stage3_block4_mlp0 =
        required_checkpoint_snapshot(&reader, "unet.swin3.block.4.mlp.0.weight")?;
    let stage3_block4_mlp3 =
        required_checkpoint_snapshot(&reader, "unet.swin3.block.4.mlp.3.weight")?;
    let stage3_block4_bias_table = required_checkpoint_snapshot(
        &reader,
        "unet.swin3.block.4.attn.relative_position_bias_table",
    )?;
    let stage3_block4_index =
        required_checkpoint_snapshot(&reader, "unet.swin3.block.4.attn.relative_position_index")?;
    let stage3_block5_qkv =
        required_checkpoint_snapshot(&reader, "unet.swin3.block.5.attn.qkv.weight")?;
    let stage3_block5_proj =
        required_checkpoint_snapshot(&reader, "unet.swin3.block.5.attn.proj.weight")?;
    let stage3_block5_mlp0 =
        required_checkpoint_snapshot(&reader, "unet.swin3.block.5.mlp.0.weight")?;
    let stage3_block5_mlp3 =
        required_checkpoint_snapshot(&reader, "unet.swin3.block.5.mlp.3.weight")?;
    let stage3_block5_bias_table = required_checkpoint_snapshot(
        &reader,
        "unet.swin3.block.5.attn.relative_position_bias_table",
    )?;
    let stage3_block5_index =
        required_checkpoint_snapshot(&reader, "unet.swin3.block.5.attn.relative_position_index")?;
    let stage4_block0_qkv =
        required_checkpoint_snapshot(&reader, "unet.swin4.block.0.attn.qkv.weight")?;
    let stage4_block0_proj =
        required_checkpoint_snapshot(&reader, "unet.swin4.block.0.attn.proj.weight")?;
    let stage4_block0_mlp0 =
        required_checkpoint_snapshot(&reader, "unet.swin4.block.0.mlp.0.weight")?;
    let stage4_block0_mlp3 =
        required_checkpoint_snapshot(&reader, "unet.swin4.block.0.mlp.3.weight")?;
    let stage4_block0_bias_table = required_checkpoint_snapshot(
        &reader,
        "unet.swin4.block.0.attn.relative_position_bias_table",
    )?;
    let stage4_block0_index =
        required_checkpoint_snapshot(&reader, "unet.swin4.block.0.attn.relative_position_index")?;
    let stage4_block1_qkv =
        required_checkpoint_snapshot(&reader, "unet.swin4.block.1.attn.qkv.weight")?;
    let stage4_block1_proj =
        required_checkpoint_snapshot(&reader, "unet.swin4.block.1.attn.proj.weight")?;
    let stage4_block1_mlp0 =
        required_checkpoint_snapshot(&reader, "unet.swin4.block.1.mlp.0.weight")?;
    let stage4_block1_mlp3 =
        required_checkpoint_snapshot(&reader, "unet.swin4.block.1.mlp.3.weight")?;
    let stage4_block1_bias_table = required_checkpoint_snapshot(
        &reader,
        "unet.swin4.block.1.attn.relative_position_bias_table",
    )?;
    let stage4_block1_index =
        required_checkpoint_snapshot(&reader, "unet.swin4.block.1.attn.relative_position_index")?;
    let stage5_block0_qkv =
        required_checkpoint_snapshot(&reader, "unet.swin5.block.0.attn.qkv.weight")?;
    let stage5_block0_proj =
        required_checkpoint_snapshot(&reader, "unet.swin5.block.0.attn.proj.weight")?;
    let stage5_block0_mlp0 =
        required_checkpoint_snapshot(&reader, "unet.swin5.block.0.mlp.0.weight")?;
    let stage5_block0_mlp3 =
        required_checkpoint_snapshot(&reader, "unet.swin5.block.0.mlp.3.weight")?;
    let stage5_block0_bias_table = required_checkpoint_snapshot(
        &reader,
        "unet.swin5.block.0.attn.relative_position_bias_table",
    )?;
    let stage5_block0_index =
        required_checkpoint_snapshot(&reader, "unet.swin5.block.0.attn.relative_position_index")?;
    let stage5_block1_qkv =
        required_checkpoint_snapshot(&reader, "unet.swin5.block.1.attn.qkv.weight")?;
    let stage5_block1_proj =
        required_checkpoint_snapshot(&reader, "unet.swin5.block.1.attn.proj.weight")?;
    let stage5_block1_mlp0 =
        required_checkpoint_snapshot(&reader, "unet.swin5.block.1.mlp.0.weight")?;
    let stage5_block1_mlp3 =
        required_checkpoint_snapshot(&reader, "unet.swin5.block.1.mlp.3.weight")?;
    let stage5_block1_bias_table = required_checkpoint_snapshot(
        &reader,
        "unet.swin5.block.1.attn.relative_position_bias_table",
    )?;
    let stage5_block1_index =
        required_checkpoint_snapshot(&reader, "unet.swin5.block.1.attn.relative_position_index")?;
    let up1_proj = required_checkpoint_snapshot(&reader, "unet.up1.proj.weight")?;
    let up2_proj = required_checkpoint_snapshot(&reader, "unet.up2.proj.weight")?;
    let to_image_proj = required_checkpoint_snapshot(&reader, "unet.to_image.proj.weight")?;

    let patch0_weight_shape = tensor_shape_u64(patch0, "unet.patch.0.weight")?;
    let patch2_weight_shape = tensor_shape_u64(patch2, "unet.patch.2.weight")?;
    let down1_conv_weight_shape = tensor_shape_u64(down1_conv, "unet.down1.conv.weight")?;
    let down2_conv_weight_shape = tensor_shape_u64(down2_conv, "unet.down2.conv.weight")?;
    let swin1_block0_qkv_weight_shape =
        tensor_shape_u64(block0_qkv, "unet.swin1.block.0.attn.qkv.weight")?;
    let swin1_block0_relative_position_bias_table_shape = tensor_shape_u64(
        block0_bias_table,
        "unet.swin1.block.0.attn.relative_position_bias_table",
    )?;
    let swin1_block0_relative_position_index_shape = tensor_shape_u64(
        block0_index,
        "unet.swin1.block.0.attn.relative_position_index",
    )?;
    let swin1_block1_qkv_weight_shape =
        tensor_shape_u64(block1_qkv, "unet.swin1.block.1.attn.qkv.weight")?;
    let swin1_block1_relative_position_bias_table_shape = tensor_shape_u64(
        block1_bias_table,
        "unet.swin1.block.1.attn.relative_position_bias_table",
    )?;
    let swin1_block1_relative_position_index_shape = tensor_shape_u64(
        block1_index,
        "unet.swin1.block.1.attn.relative_position_index",
    )?;
    let swin2_block0_qkv_weight_shape =
        tensor_shape_u64(stage2_block0_qkv, "unet.swin2.block.0.attn.qkv.weight")?;
    let swin2_block0_relative_position_bias_table_shape = tensor_shape_u64(
        stage2_block0_bias_table,
        "unet.swin2.block.0.attn.relative_position_bias_table",
    )?;
    let swin2_block0_relative_position_index_shape = tensor_shape_u64(
        stage2_block0_index,
        "unet.swin2.block.0.attn.relative_position_index",
    )?;
    let swin2_block1_qkv_weight_shape =
        tensor_shape_u64(stage2_block1_qkv, "unet.swin2.block.1.attn.qkv.weight")?;
    let swin2_block1_relative_position_bias_table_shape = tensor_shape_u64(
        stage2_block1_bias_table,
        "unet.swin2.block.1.attn.relative_position_bias_table",
    )?;
    let swin2_block1_relative_position_index_shape = tensor_shape_u64(
        stage2_block1_index,
        "unet.swin2.block.1.attn.relative_position_index",
    )?;
    let swin3_block0_qkv_weight_shape =
        tensor_shape_u64(stage3_block0_qkv, "unet.swin3.block.0.attn.qkv.weight")?;
    let swin3_block0_relative_position_bias_table_shape = tensor_shape_u64(
        stage3_block0_bias_table,
        "unet.swin3.block.0.attn.relative_position_bias_table",
    )?;
    let swin3_block0_relative_position_index_shape = tensor_shape_u64(
        stage3_block0_index,
        "unet.swin3.block.0.attn.relative_position_index",
    )?;
    let swin3_block1_qkv_weight_shape =
        tensor_shape_u64(stage3_block1_qkv, "unet.swin3.block.1.attn.qkv.weight")?;
    let swin3_block1_relative_position_bias_table_shape = tensor_shape_u64(
        stage3_block1_bias_table,
        "unet.swin3.block.1.attn.relative_position_bias_table",
    )?;
    let swin3_block1_relative_position_index_shape = tensor_shape_u64(
        stage3_block1_index,
        "unet.swin3.block.1.attn.relative_position_index",
    )?;
    let swin3_block2_qkv_weight_shape =
        tensor_shape_u64(stage3_block2_qkv, "unet.swin3.block.2.attn.qkv.weight")?;
    let swin3_block2_relative_position_bias_table_shape = tensor_shape_u64(
        stage3_block2_bias_table,
        "unet.swin3.block.2.attn.relative_position_bias_table",
    )?;
    let swin3_block2_relative_position_index_shape = tensor_shape_u64(
        stage3_block2_index,
        "unet.swin3.block.2.attn.relative_position_index",
    )?;
    let swin3_block3_qkv_weight_shape =
        tensor_shape_u64(stage3_block3_qkv, "unet.swin3.block.3.attn.qkv.weight")?;
    let swin3_block3_relative_position_bias_table_shape = tensor_shape_u64(
        stage3_block3_bias_table,
        "unet.swin3.block.3.attn.relative_position_bias_table",
    )?;
    let swin3_block3_relative_position_index_shape = tensor_shape_u64(
        stage3_block3_index,
        "unet.swin3.block.3.attn.relative_position_index",
    )?;
    let swin3_block4_qkv_weight_shape =
        tensor_shape_u64(stage3_block4_qkv, "unet.swin3.block.4.attn.qkv.weight")?;
    let swin3_block4_relative_position_bias_table_shape = tensor_shape_u64(
        stage3_block4_bias_table,
        "unet.swin3.block.4.attn.relative_position_bias_table",
    )?;
    let swin3_block4_relative_position_index_shape = tensor_shape_u64(
        stage3_block4_index,
        "unet.swin3.block.4.attn.relative_position_index",
    )?;
    let swin3_block5_qkv_weight_shape =
        tensor_shape_u64(stage3_block5_qkv, "unet.swin3.block.5.attn.qkv.weight")?;
    let swin3_block5_relative_position_bias_table_shape = tensor_shape_u64(
        stage3_block5_bias_table,
        "unet.swin3.block.5.attn.relative_position_bias_table",
    )?;
    let swin3_block5_relative_position_index_shape = tensor_shape_u64(
        stage3_block5_index,
        "unet.swin3.block.5.attn.relative_position_index",
    )?;
    let swin4_block0_qkv_weight_shape =
        tensor_shape_u64(stage4_block0_qkv, "unet.swin4.block.0.attn.qkv.weight")?;
    let swin4_block0_relative_position_bias_table_shape = tensor_shape_u64(
        stage4_block0_bias_table,
        "unet.swin4.block.0.attn.relative_position_bias_table",
    )?;
    let swin4_block0_relative_position_index_shape = tensor_shape_u64(
        stage4_block0_index,
        "unet.swin4.block.0.attn.relative_position_index",
    )?;
    let swin4_block1_qkv_weight_shape =
        tensor_shape_u64(stage4_block1_qkv, "unet.swin4.block.1.attn.qkv.weight")?;
    let swin4_block1_relative_position_bias_table_shape = tensor_shape_u64(
        stage4_block1_bias_table,
        "unet.swin4.block.1.attn.relative_position_bias_table",
    )?;
    let swin4_block1_relative_position_index_shape = tensor_shape_u64(
        stage4_block1_index,
        "unet.swin4.block.1.attn.relative_position_index",
    )?;
    let swin5_block0_qkv_weight_shape =
        tensor_shape_u64(stage5_block0_qkv, "unet.swin5.block.0.attn.qkv.weight")?;
    let swin5_block0_relative_position_bias_table_shape = tensor_shape_u64(
        stage5_block0_bias_table,
        "unet.swin5.block.0.attn.relative_position_bias_table",
    )?;
    let swin5_block0_relative_position_index_shape = tensor_shape_u64(
        stage5_block0_index,
        "unet.swin5.block.0.attn.relative_position_index",
    )?;
    let swin5_block1_qkv_weight_shape =
        tensor_shape_u64(stage5_block1_qkv, "unet.swin5.block.1.attn.qkv.weight")?;
    let swin5_block1_relative_position_bias_table_shape = tensor_shape_u64(
        stage5_block1_bias_table,
        "unet.swin5.block.1.attn.relative_position_bias_table",
    )?;
    let swin5_block1_relative_position_index_shape = tensor_shape_u64(
        stage5_block1_index,
        "unet.swin5.block.1.attn.relative_position_index",
    )?;
    let output_scale = waifu2x_output_scale_from_checkpoint_reader(&reader);
    let proj2 = if output_scale == 4 {
        Some(required_checkpoint_snapshot(
            &reader,
            WAIFU2X_PROJ2_WEIGHT_KEY,
        )?)
    } else {
        None
    };
    let proj2_weight_shape = proj2
        .map(|snapshot| tensor_shape_u64(snapshot, WAIFU2X_PROJ2_WEIGHT_KEY))
        .transpose()?;
    let up1_proj_weight_shape = tensor_shape_u64(up1_proj, "unet.up1.proj.weight")?;
    let up2_proj_weight_shape = tensor_shape_u64(up2_proj, "unet.up2.proj.weight")?;
    let to_image_proj_weight_shape = tensor_shape_u64(to_image_proj, "unet.to_image.proj.weight")?;
    let device = burn::backend::ndarray::NdArrayDevice::default();
    let mut probe = Waifu2xPatchStem::<Waifu2xProbeBackend> {
        patch0: conv2d_from_weight_snapshot(patch0, &device)?,
        patch2: conv2d_from_weight_snapshot(patch2, &device)?,
        down1_conv: conv2d_from_weight_snapshot(down1_conv, &device)?,
        down2_conv: conv2d_from_weight_snapshot(down2_conv, &device)?,
        block0: Waifu2xSwinBlockProbe {
            attn: Waifu2xSwinAttentionProbe {
                qkv: linear_from_weight_snapshot(block0_qkv, &device)?,
                proj: linear_from_weight_snapshot(block0_proj, &device)?,
                relative_position_bias_table: float_param_2d_from_snapshot_shape(
                    block0_bias_table,
                    &device,
                )?,
                relative_position_index: int_param_1d_from_snapshot_shape(block0_index, &device)?,
            },
            mlp: Waifu2xSwinMlpProbe {
                lin0: linear_from_weight_snapshot(block0_mlp0, &device)?,
                lin3: linear_from_weight_snapshot(block0_mlp3, &device)?,
            },
        },
        block1: Waifu2xSwinBlockProbe {
            attn: Waifu2xSwinAttentionProbe {
                qkv: linear_from_weight_snapshot(block1_qkv, &device)?,
                proj: linear_from_weight_snapshot(block1_proj, &device)?,
                relative_position_bias_table: float_param_2d_from_snapshot_shape(
                    block1_bias_table,
                    &device,
                )?,
                relative_position_index: int_param_1d_from_snapshot_shape(block1_index, &device)?,
            },
            mlp: Waifu2xSwinMlpProbe {
                lin0: linear_from_weight_snapshot(block1_mlp0, &device)?,
                lin3: linear_from_weight_snapshot(block1_mlp3, &device)?,
            },
        },
        stage2_block0: Waifu2xSwinBlockProbe {
            attn: Waifu2xSwinAttentionProbe {
                qkv: linear_from_weight_snapshot(stage2_block0_qkv, &device)?,
                proj: linear_from_weight_snapshot(stage2_block0_proj, &device)?,
                relative_position_bias_table: float_param_2d_from_snapshot_shape(
                    stage2_block0_bias_table,
                    &device,
                )?,
                relative_position_index: int_param_1d_from_snapshot_shape(
                    stage2_block0_index,
                    &device,
                )?,
            },
            mlp: Waifu2xSwinMlpProbe {
                lin0: linear_from_weight_snapshot(stage2_block0_mlp0, &device)?,
                lin3: linear_from_weight_snapshot(stage2_block0_mlp3, &device)?,
            },
        },
        stage2_block1: Waifu2xSwinBlockProbe {
            attn: Waifu2xSwinAttentionProbe {
                qkv: linear_from_weight_snapshot(stage2_block1_qkv, &device)?,
                proj: linear_from_weight_snapshot(stage2_block1_proj, &device)?,
                relative_position_bias_table: float_param_2d_from_snapshot_shape(
                    stage2_block1_bias_table,
                    &device,
                )?,
                relative_position_index: int_param_1d_from_snapshot_shape(
                    stage2_block1_index,
                    &device,
                )?,
            },
            mlp: Waifu2xSwinMlpProbe {
                lin0: linear_from_weight_snapshot(stage2_block1_mlp0, &device)?,
                lin3: linear_from_weight_snapshot(stage2_block1_mlp3, &device)?,
            },
        },
        stage3_block0: Waifu2xSwinBlockProbe {
            attn: Waifu2xSwinAttentionProbe {
                qkv: linear_from_weight_snapshot(stage3_block0_qkv, &device)?,
                proj: linear_from_weight_snapshot(stage3_block0_proj, &device)?,
                relative_position_bias_table: float_param_2d_from_snapshot_shape(
                    stage3_block0_bias_table,
                    &device,
                )?,
                relative_position_index: int_param_1d_from_snapshot_shape(
                    stage3_block0_index,
                    &device,
                )?,
            },
            mlp: Waifu2xSwinMlpProbe {
                lin0: linear_from_weight_snapshot(stage3_block0_mlp0, &device)?,
                lin3: linear_from_weight_snapshot(stage3_block0_mlp3, &device)?,
            },
        },
        stage3_block1: Waifu2xSwinBlockProbe {
            attn: Waifu2xSwinAttentionProbe {
                qkv: linear_from_weight_snapshot(stage3_block1_qkv, &device)?,
                proj: linear_from_weight_snapshot(stage3_block1_proj, &device)?,
                relative_position_bias_table: float_param_2d_from_snapshot_shape(
                    stage3_block1_bias_table,
                    &device,
                )?,
                relative_position_index: int_param_1d_from_snapshot_shape(
                    stage3_block1_index,
                    &device,
                )?,
            },
            mlp: Waifu2xSwinMlpProbe {
                lin0: linear_from_weight_snapshot(stage3_block1_mlp0, &device)?,
                lin3: linear_from_weight_snapshot(stage3_block1_mlp3, &device)?,
            },
        },
        stage3_block2: Waifu2xSwinBlockProbe {
            attn: Waifu2xSwinAttentionProbe {
                qkv: linear_from_weight_snapshot(stage3_block2_qkv, &device)?,
                proj: linear_from_weight_snapshot(stage3_block2_proj, &device)?,
                relative_position_bias_table: float_param_2d_from_snapshot_shape(
                    stage3_block2_bias_table,
                    &device,
                )?,
                relative_position_index: int_param_1d_from_snapshot_shape(
                    stage3_block2_index,
                    &device,
                )?,
            },
            mlp: Waifu2xSwinMlpProbe {
                lin0: linear_from_weight_snapshot(stage3_block2_mlp0, &device)?,
                lin3: linear_from_weight_snapshot(stage3_block2_mlp3, &device)?,
            },
        },
        stage3_block3: Waifu2xSwinBlockProbe {
            attn: Waifu2xSwinAttentionProbe {
                qkv: linear_from_weight_snapshot(stage3_block3_qkv, &device)?,
                proj: linear_from_weight_snapshot(stage3_block3_proj, &device)?,
                relative_position_bias_table: float_param_2d_from_snapshot_shape(
                    stage3_block3_bias_table,
                    &device,
                )?,
                relative_position_index: int_param_1d_from_snapshot_shape(
                    stage3_block3_index,
                    &device,
                )?,
            },
            mlp: Waifu2xSwinMlpProbe {
                lin0: linear_from_weight_snapshot(stage3_block3_mlp0, &device)?,
                lin3: linear_from_weight_snapshot(stage3_block3_mlp3, &device)?,
            },
        },
        stage3_block4: Waifu2xSwinBlockProbe {
            attn: Waifu2xSwinAttentionProbe {
                qkv: linear_from_weight_snapshot(stage3_block4_qkv, &device)?,
                proj: linear_from_weight_snapshot(stage3_block4_proj, &device)?,
                relative_position_bias_table: float_param_2d_from_snapshot_shape(
                    stage3_block4_bias_table,
                    &device,
                )?,
                relative_position_index: int_param_1d_from_snapshot_shape(
                    stage3_block4_index,
                    &device,
                )?,
            },
            mlp: Waifu2xSwinMlpProbe {
                lin0: linear_from_weight_snapshot(stage3_block4_mlp0, &device)?,
                lin3: linear_from_weight_snapshot(stage3_block4_mlp3, &device)?,
            },
        },
        stage3_block5: Waifu2xSwinBlockProbe {
            attn: Waifu2xSwinAttentionProbe {
                qkv: linear_from_weight_snapshot(stage3_block5_qkv, &device)?,
                proj: linear_from_weight_snapshot(stage3_block5_proj, &device)?,
                relative_position_bias_table: float_param_2d_from_snapshot_shape(
                    stage3_block5_bias_table,
                    &device,
                )?,
                relative_position_index: int_param_1d_from_snapshot_shape(
                    stage3_block5_index,
                    &device,
                )?,
            },
            mlp: Waifu2xSwinMlpProbe {
                lin0: linear_from_weight_snapshot(stage3_block5_mlp0, &device)?,
                lin3: linear_from_weight_snapshot(stage3_block5_mlp3, &device)?,
            },
        },
        stage4_block0: Waifu2xSwinBlockProbe {
            attn: Waifu2xSwinAttentionProbe {
                qkv: linear_from_weight_snapshot(stage4_block0_qkv, &device)?,
                proj: linear_from_weight_snapshot(stage4_block0_proj, &device)?,
                relative_position_bias_table: float_param_2d_from_snapshot_shape(
                    stage4_block0_bias_table,
                    &device,
                )?,
                relative_position_index: int_param_1d_from_snapshot_shape(
                    stage4_block0_index,
                    &device,
                )?,
            },
            mlp: Waifu2xSwinMlpProbe {
                lin0: linear_from_weight_snapshot(stage4_block0_mlp0, &device)?,
                lin3: linear_from_weight_snapshot(stage4_block0_mlp3, &device)?,
            },
        },
        stage4_block1: Waifu2xSwinBlockProbe {
            attn: Waifu2xSwinAttentionProbe {
                qkv: linear_from_weight_snapshot(stage4_block1_qkv, &device)?,
                proj: linear_from_weight_snapshot(stage4_block1_proj, &device)?,
                relative_position_bias_table: float_param_2d_from_snapshot_shape(
                    stage4_block1_bias_table,
                    &device,
                )?,
                relative_position_index: int_param_1d_from_snapshot_shape(
                    stage4_block1_index,
                    &device,
                )?,
            },
            mlp: Waifu2xSwinMlpProbe {
                lin0: linear_from_weight_snapshot(stage4_block1_mlp0, &device)?,
                lin3: linear_from_weight_snapshot(stage4_block1_mlp3, &device)?,
            },
        },
        stage5_block0: Waifu2xSwinBlockProbe {
            attn: Waifu2xSwinAttentionProbe {
                qkv: linear_from_weight_snapshot(stage5_block0_qkv, &device)?,
                proj: linear_from_weight_snapshot(stage5_block0_proj, &device)?,
                relative_position_bias_table: float_param_2d_from_snapshot_shape(
                    stage5_block0_bias_table,
                    &device,
                )?,
                relative_position_index: int_param_1d_from_snapshot_shape(
                    stage5_block0_index,
                    &device,
                )?,
            },
            mlp: Waifu2xSwinMlpProbe {
                lin0: linear_from_weight_snapshot(stage5_block0_mlp0, &device)?,
                lin3: linear_from_weight_snapshot(stage5_block0_mlp3, &device)?,
            },
        },
        stage5_block1: Waifu2xSwinBlockProbe {
            attn: Waifu2xSwinAttentionProbe {
                qkv: linear_from_weight_snapshot(stage5_block1_qkv, &device)?,
                proj: linear_from_weight_snapshot(stage5_block1_proj, &device)?,
                relative_position_bias_table: float_param_2d_from_snapshot_shape(
                    stage5_block1_bias_table,
                    &device,
                )?,
                relative_position_index: int_param_1d_from_snapshot_shape(
                    stage5_block1_index,
                    &device,
                )?,
            },
            mlp: Waifu2xSwinMlpProbe {
                lin0: linear_from_weight_snapshot(stage5_block1_mlp0, &device)?,
                lin3: linear_from_weight_snapshot(stage5_block1_mlp3, &device)?,
            },
        },
        proj2: proj2
            .map(|weight| linear_from_weight_snapshot(weight, &device))
            .transpose()?,
        up1_proj: linear_from_weight_snapshot(up1_proj, &device)?,
        up2_proj: linear_from_weight_snapshot(up2_proj, &device)?,
        to_image_proj: linear_from_weight_snapshot(to_image_proj, &device)?,
        output_scale,
    };

    let checkpoint_filter_regex = WAIFU2X_CHECKPOINT_FILTER_REGEX.to_owned();
    let mut store = configure_waifu2x_checkpoint_store(PytorchStore::from_file(checkpoint_path));
    let result = probe.load_from(&mut store).wrap_err_with(|| {
        format!(
            "failed to run waifu2x Burn patch/down1/swin-block load probe from {}",
            checkpoint_path.display()
        )
    })?;

    let mut matched_checkpoint_keys = reader
        .keys()
        .into_iter()
        .filter(|key| {
            matches!(
                key.as_str(),
                "unet.patch.0.weight"
                    | "unet.patch.0.bias"
                    | "unet.patch.2.weight"
                    | "unet.patch.2.bias"
                    | "unet.down1.conv.weight"
                    | "unet.down1.conv.bias"
                    | "unet.down2.conv.weight"
                    | "unet.down2.conv.bias"
                    | "unet.swin1.block.0.attn.qkv.weight"
                    | "unet.swin1.block.0.attn.qkv.bias"
                    | "unet.swin1.block.0.attn.proj.weight"
                    | "unet.swin1.block.0.attn.proj.bias"
                    | "unet.swin1.block.0.attn.relative_position_bias_table"
                    | "unet.swin1.block.0.attn.relative_position_index"
                    | "unet.swin1.block.0.mlp.0.weight"
                    | "unet.swin1.block.0.mlp.0.bias"
                    | "unet.swin1.block.0.mlp.3.weight"
                    | "unet.swin1.block.0.mlp.3.bias"
                    | "unet.swin1.block.1.attn.qkv.weight"
                    | "unet.swin1.block.1.attn.qkv.bias"
                    | "unet.swin1.block.1.attn.proj.weight"
                    | "unet.swin1.block.1.attn.proj.bias"
                    | "unet.swin1.block.1.attn.relative_position_bias_table"
                    | "unet.swin1.block.1.attn.relative_position_index"
                    | "unet.swin1.block.1.mlp.0.weight"
                    | "unet.swin1.block.1.mlp.0.bias"
                    | "unet.swin1.block.1.mlp.3.weight"
                    | "unet.swin1.block.1.mlp.3.bias"
                    | "unet.swin2.block.0.attn.qkv.weight"
                    | "unet.swin2.block.0.attn.qkv.bias"
                    | "unet.swin2.block.0.attn.proj.weight"
                    | "unet.swin2.block.0.attn.proj.bias"
                    | "unet.swin2.block.0.attn.relative_position_bias_table"
                    | "unet.swin2.block.0.attn.relative_position_index"
                    | "unet.swin2.block.0.mlp.0.weight"
                    | "unet.swin2.block.0.mlp.0.bias"
                    | "unet.swin2.block.0.mlp.3.weight"
                    | "unet.swin2.block.0.mlp.3.bias"
                    | "unet.swin2.block.1.attn.qkv.weight"
                    | "unet.swin2.block.1.attn.qkv.bias"
                    | "unet.swin2.block.1.attn.proj.weight"
                    | "unet.swin2.block.1.attn.proj.bias"
                    | "unet.swin2.block.1.attn.relative_position_bias_table"
                    | "unet.swin2.block.1.attn.relative_position_index"
                    | "unet.swin2.block.1.mlp.0.weight"
                    | "unet.swin2.block.1.mlp.0.bias"
                    | "unet.swin2.block.1.mlp.3.weight"
                    | "unet.swin2.block.1.mlp.3.bias"
                    | "unet.swin3.block.0.attn.qkv.weight"
                    | "unet.swin3.block.0.attn.qkv.bias"
                    | "unet.swin3.block.0.attn.proj.weight"
                    | "unet.swin3.block.0.attn.proj.bias"
                    | "unet.swin3.block.0.attn.relative_position_bias_table"
                    | "unet.swin3.block.0.attn.relative_position_index"
                    | "unet.swin3.block.0.mlp.0.weight"
                    | "unet.swin3.block.0.mlp.0.bias"
                    | "unet.swin3.block.0.mlp.3.weight"
                    | "unet.swin3.block.0.mlp.3.bias"
                    | "unet.swin3.block.1.attn.qkv.weight"
                    | "unet.swin3.block.1.attn.qkv.bias"
                    | "unet.swin3.block.1.attn.proj.weight"
                    | "unet.swin3.block.1.attn.proj.bias"
                    | "unet.swin3.block.1.attn.relative_position_bias_table"
                    | "unet.swin3.block.1.attn.relative_position_index"
                    | "unet.swin3.block.1.mlp.0.weight"
                    | "unet.swin3.block.1.mlp.0.bias"
                    | "unet.swin3.block.1.mlp.3.weight"
                    | "unet.swin3.block.1.mlp.3.bias"
                    | "unet.swin3.block.2.attn.qkv.weight"
                    | "unet.swin3.block.2.attn.qkv.bias"
                    | "unet.swin3.block.2.attn.proj.weight"
                    | "unet.swin3.block.2.attn.proj.bias"
                    | "unet.swin3.block.2.attn.relative_position_bias_table"
                    | "unet.swin3.block.2.attn.relative_position_index"
                    | "unet.swin3.block.2.mlp.0.weight"
                    | "unet.swin3.block.2.mlp.0.bias"
                    | "unet.swin3.block.2.mlp.3.weight"
                    | "unet.swin3.block.2.mlp.3.bias"
                    | "unet.swin3.block.3.attn.qkv.weight"
                    | "unet.swin3.block.3.attn.qkv.bias"
                    | "unet.swin3.block.3.attn.proj.weight"
                    | "unet.swin3.block.3.attn.proj.bias"
                    | "unet.swin3.block.3.attn.relative_position_bias_table"
                    | "unet.swin3.block.3.attn.relative_position_index"
                    | "unet.swin3.block.3.mlp.0.weight"
                    | "unet.swin3.block.3.mlp.0.bias"
                    | "unet.swin3.block.3.mlp.3.weight"
                    | "unet.swin3.block.3.mlp.3.bias"
                    | "unet.swin3.block.4.attn.qkv.weight"
                    | "unet.swin3.block.4.attn.qkv.bias"
                    | "unet.swin3.block.4.attn.proj.weight"
                    | "unet.swin3.block.4.attn.proj.bias"
                    | "unet.swin3.block.4.attn.relative_position_bias_table"
                    | "unet.swin3.block.4.attn.relative_position_index"
                    | "unet.swin3.block.4.mlp.0.weight"
                    | "unet.swin3.block.4.mlp.0.bias"
                    | "unet.swin3.block.4.mlp.3.weight"
                    | "unet.swin3.block.4.mlp.3.bias"
                    | "unet.swin3.block.5.attn.qkv.weight"
                    | "unet.swin3.block.5.attn.qkv.bias"
                    | "unet.swin3.block.5.attn.proj.weight"
                    | "unet.swin3.block.5.attn.proj.bias"
                    | "unet.swin3.block.5.attn.relative_position_bias_table"
                    | "unet.swin3.block.5.attn.relative_position_index"
                    | "unet.swin3.block.5.mlp.0.weight"
                    | "unet.swin3.block.5.mlp.0.bias"
                    | "unet.swin3.block.5.mlp.3.weight"
                    | "unet.swin3.block.5.mlp.3.bias"
                    | "unet.swin4.block.0.attn.qkv.weight"
                    | "unet.swin4.block.0.attn.qkv.bias"
                    | "unet.swin4.block.0.attn.proj.weight"
                    | "unet.swin4.block.0.attn.proj.bias"
                    | "unet.swin4.block.0.attn.relative_position_bias_table"
                    | "unet.swin4.block.0.attn.relative_position_index"
                    | "unet.swin4.block.0.mlp.0.weight"
                    | "unet.swin4.block.0.mlp.0.bias"
                    | "unet.swin4.block.0.mlp.3.weight"
                    | "unet.swin4.block.0.mlp.3.bias"
                    | "unet.swin4.block.1.attn.qkv.weight"
                    | "unet.swin4.block.1.attn.qkv.bias"
                    | "unet.swin4.block.1.attn.proj.weight"
                    | "unet.swin4.block.1.attn.proj.bias"
                    | "unet.swin4.block.1.attn.relative_position_bias_table"
                    | "unet.swin4.block.1.attn.relative_position_index"
                    | "unet.swin4.block.1.mlp.0.weight"
                    | "unet.swin4.block.1.mlp.0.bias"
                    | "unet.swin4.block.1.mlp.3.weight"
                    | "unet.swin4.block.1.mlp.3.bias"
                    | "unet.swin5.block.0.attn.qkv.weight"
                    | "unet.swin5.block.0.attn.qkv.bias"
                    | "unet.swin5.block.0.attn.proj.weight"
                    | "unet.swin5.block.0.attn.proj.bias"
                    | "unet.swin5.block.0.attn.relative_position_bias_table"
                    | "unet.swin5.block.0.attn.relative_position_index"
                    | "unet.swin5.block.0.mlp.0.weight"
                    | "unet.swin5.block.0.mlp.0.bias"
                    | "unet.swin5.block.0.mlp.3.weight"
                    | "unet.swin5.block.0.mlp.3.bias"
                    | "unet.swin5.block.1.attn.qkv.weight"
                    | "unet.swin5.block.1.attn.qkv.bias"
                    | "unet.swin5.block.1.attn.proj.weight"
                    | "unet.swin5.block.1.attn.proj.bias"
                    | "unet.swin5.block.1.attn.relative_position_bias_table"
                    | "unet.swin5.block.1.attn.relative_position_index"
                    | "unet.swin5.block.1.mlp.0.weight"
                    | "unet.swin5.block.1.mlp.0.bias"
                    | "unet.swin5.block.1.mlp.3.weight"
                    | "unet.swin5.block.1.mlp.3.bias"
                    | "unet.proj2.weight"
                    | "unet.proj2.bias"
                    | "unet.up1.proj.weight"
                    | "unet.up1.proj.bias"
                    | "unet.up2.proj.weight"
                    | "unet.up2.proj.bias"
                    | "unet.to_image.proj.weight"
                    | "unet.to_image.proj.bias"
            )
        })
        .collect::<Vec<_>>();
    matched_checkpoint_keys.sort_unstable();

    Ok(Some(ImageModelCheckpointBurnLoadProbeReport {
        module_name: "waifu2x.full-checkpoint-probe".to_owned(),
        output_scale: output_scale as u64,
        checkpoint_filter_regex,
        matched_checkpoint_keys,
        patch0_weight_shape,
        patch2_weight_shape,
        down1_conv_weight_shape,
        down2_conv_weight_shape,
        swin1_block0_qkv_weight_shape,
        swin1_block0_relative_position_bias_table_shape,
        swin1_block0_relative_position_index_shape,
        swin1_block1_qkv_weight_shape,
        swin1_block1_relative_position_bias_table_shape,
        swin1_block1_relative_position_index_shape,
        swin2_block0_qkv_weight_shape,
        swin2_block0_relative_position_bias_table_shape,
        swin2_block0_relative_position_index_shape,
        swin2_block1_qkv_weight_shape,
        swin2_block1_relative_position_bias_table_shape,
        swin2_block1_relative_position_index_shape,
        swin3_block0_qkv_weight_shape,
        swin3_block0_relative_position_bias_table_shape,
        swin3_block0_relative_position_index_shape,
        swin3_block1_qkv_weight_shape,
        swin3_block1_relative_position_bias_table_shape,
        swin3_block1_relative_position_index_shape,
        swin3_block2_qkv_weight_shape,
        swin3_block2_relative_position_bias_table_shape,
        swin3_block2_relative_position_index_shape,
        swin3_block3_qkv_weight_shape,
        swin3_block3_relative_position_bias_table_shape,
        swin3_block3_relative_position_index_shape,
        swin3_block4_qkv_weight_shape,
        swin3_block4_relative_position_bias_table_shape,
        swin3_block4_relative_position_index_shape,
        swin3_block5_qkv_weight_shape,
        swin3_block5_relative_position_bias_table_shape,
        swin3_block5_relative_position_index_shape,
        swin4_block0_qkv_weight_shape,
        swin4_block0_relative_position_bias_table_shape,
        swin4_block0_relative_position_index_shape,
        swin4_block1_qkv_weight_shape,
        swin4_block1_relative_position_bias_table_shape,
        swin4_block1_relative_position_index_shape,
        swin5_block0_qkv_weight_shape,
        swin5_block0_relative_position_bias_table_shape,
        swin5_block0_relative_position_index_shape,
        swin5_block1_qkv_weight_shape,
        swin5_block1_relative_position_bias_table_shape,
        swin5_block1_relative_position_index_shape,
        proj2_weight_shape,
        up1_proj_weight_shape,
        up2_proj_weight_shape,
        to_image_proj_weight_shape,
        applied: result.applied,
        missing: result.missing.into_iter().map(|(path, _)| path).collect(),
        unused: result.unused,
        errors: result
            .errors
            .into_iter()
            .map(|error| error.to_string())
            .collect(),
    }))
}

fn required_checkpoint_snapshot<'a>(
    reader: &'a PytorchReader,
    key: &str,
) -> eyre::Result<&'a burn_store::TensorSnapshot> {
    reader
        .get(key)
        .ok_or_else(|| eyre::eyre!("waifu2x checkpoint Burn probe requires missing tensor `{key}`"))
}

fn tensor_shape_u64(snapshot: &burn_store::TensorSnapshot, key: &str) -> eyre::Result<Vec<u64>> {
    snapshot
        .shape
        .iter()
        .copied()
        .map(|dimension| {
            u64::try_from(dimension).wrap_err_with(|| {
                format!("waifu2x checkpoint tensor `{key}` dimension does not fit in u64")
            })
        })
        .collect()
}

fn conv2d_from_weight_snapshot<B: Backend>(
    snapshot: &burn_store::TensorSnapshot,
    device: &B::Device,
) -> eyre::Result<Conv2d<B>> {
    let [channels_out, channels_in, kernel_height, kernel_width] =
        weight_shape_2d(snapshot.shape.as_slice())?;
    Ok(Conv2dConfig::new([channels_in, channels_out], [kernel_height, kernel_width]).init(device))
}

fn downsample_conv2d_from_weight_snapshot<B: Backend>(
    snapshot: &burn_store::TensorSnapshot,
    device: &B::Device,
) -> eyre::Result<Conv2d<B>> {
    let [channels_out, channels_in, kernel_height, kernel_width] =
        weight_shape_2d(snapshot.shape.as_slice())?;
    Ok(
        Conv2dConfig::new([channels_in, channels_out], [kernel_height, kernel_width])
            .with_stride([2, 2])
            .init(device),
    )
}

fn linear_from_weight_snapshot<B: Backend>(
    snapshot: &burn_store::TensorSnapshot,
    device: &B::Device,
) -> eyre::Result<Linear<B>> {
    let [features_out, features_in] = weight_shape_1d_or_linear(snapshot.shape.as_slice())?;
    Ok(LinearConfig::new(features_in, features_out).init(device))
}

fn float_param_2d_from_snapshot_shape<B: Backend>(
    snapshot: &burn_store::TensorSnapshot,
    device: &B::Device,
) -> eyre::Result<Param<Tensor<B, 2>>> {
    let [rows, columns] = weight_shape_1d_or_linear(snapshot.shape.as_slice())?;
    Ok(Param::from_tensor(Tensor::<B, 2>::zeros(
        [rows, columns],
        device,
    )))
}

fn int_param_1d_from_snapshot_shape<B: Backend>(
    snapshot: &burn_store::TensorSnapshot,
    device: &B::Device,
) -> eyre::Result<Param<Tensor<B, 1, Int>>> {
    let len = single_dimension(snapshot.shape.as_slice())?;
    Ok(Param::initialized(
        ParamId::new(),
        Tensor::<B, 1, Int>::zeros([len], device),
    ))
}

fn weight_shape_2d(shape: &[usize]) -> eyre::Result<[usize; 4]> {
    match shape {
        [channels_out, channels_in, kernel_height, kernel_width] => {
            Ok([*channels_out, *channels_in, *kernel_height, *kernel_width])
        }
        _ => bail!(
            "waifu2x Burn probe expected a 4D conv weight tensor, got shape {:?}",
            shape
        ),
    }
}

fn weight_shape_1d_or_linear(shape: &[usize]) -> eyre::Result<[usize; 2]> {
    match shape {
        [dim0, dim1] => Ok([*dim0, *dim1]),
        _ => bail!(
            "waifu2x Burn probe expected a 2D tensor shape, got shape {:?}",
            shape
        ),
    }
}

fn single_dimension(shape: &[usize]) -> eyre::Result<usize> {
    match shape {
        [dim] => Ok(*dim),
        _ => bail!(
            "waifu2x Burn probe expected a 1D tensor shape, got shape {:?}",
            shape
        ),
    }
}

fn load_optional_checkpoint_config<D>(
    checkpoint_path: &Path,
    top_level_key: &str,
) -> eyre::Result<Option<D>>
where
    D: serde::de::DeserializeOwned,
{
    match PytorchReader::load_config(checkpoint_path, Some(top_level_key)) {
        Ok(value) => Ok(Some(value)),
        Err(burn_store::pytorch::PytorchError::KeyNotFound(_)) => Ok(None),
        Err(error) => Err(eyre::eyre!(error)).wrap_err_with(|| {
            format!(
                "failed to read waifu2x checkpoint config `{top_level_key}` from {}",
                checkpoint_path.display()
            )
        }),
    }
}

fn write_image_model_metadata(path: &Path, metadata: &ImageModelMetadata) -> eyre::Result<()> {
    let json = serde_json::to_string_pretty(metadata)
        .wrap_err("failed to serialize image model metadata")?;
    std::fs::write(path, format!("{json}\n"))
        .wrap_err_with(|| format!("failed to write image model metadata {}", path.display()))?;
    Ok(())
}

fn ensure_image_model_runtime_is_implemented(
    known: &KnownImageModel,
    action: &str,
) -> eyre::Result<()> {
    if known.teamy_runtime_status == IMAGE_MODEL_RUNTIME_STATUS_IMPLEMENTED {
        return Ok(());
    }

    bail!(
        "image model `{}` is {} and cannot be {} yet: {}",
        known.name,
        known.teamy_runtime_status,
        action,
        known.teamy_runtime_notes
    )
}

fn ensure_image_model_source_artifacts(
    model_dir: &Path,
    metadata: &ImageModelMetadata,
    overwrite: bool,
) -> eyre::Result<()> {
    let source_dir = image_model_source_dir(model_dir);
    std::fs::create_dir_all(&source_dir).wrap_err_with(|| {
        format!(
            "failed to create image model source dir {}",
            source_dir.display()
        )
    })?;

    let archive_path = image_model_source_archive_path(model_dir, metadata)?;
    if overwrite || !archive_path.is_file() {
        download_to_file(&metadata.source_archive_url, &archive_path)?;
    }

    let checkpoint_path = image_model_source_checkpoint_path(model_dir, metadata)?;
    if overwrite || !checkpoint_path.is_file() {
        extract_checkpoint_from_archive(&archive_path, &checkpoint_path, metadata)?;
    }

    Ok(())
}

fn inspect_image_model_source_artifacts(
    model_dir: &Path,
    metadata: &ImageModelMetadata,
) -> eyre::Result<ImageModelSourceArtifactsReport> {
    let source_root = image_model_source_dir(model_dir);
    let archive_path = image_model_source_archive_path(model_dir, metadata)?;
    let checkpoint_path = image_model_source_checkpoint_path(model_dir, metadata)?;
    let archive_exists = archive_path.is_file();
    let checkpoint_exists = checkpoint_path.is_file();
    Ok(ImageModelSourceArtifactsReport {
        source_root: source_root.display().to_string(),
        archive_path: archive_path.display().to_string(),
        checkpoint_path: checkpoint_path.display().to_string(),
        archive_exists,
        checkpoint_exists,
        archive_size_bytes: file_size_bytes(&archive_path),
        checkpoint_size_bytes: file_size_bytes(&checkpoint_path),
    })
}

fn image_model_source_archive_path(
    model_dir: &Path,
    metadata: &ImageModelMetadata,
) -> eyre::Result<PathBuf> {
    let file_name = archive_file_name_from_url(&metadata.source_archive_url)?;
    Ok(image_model_source_dir(model_dir).join(file_name))
}

fn image_model_source_checkpoint_path(
    model_dir: &Path,
    metadata: &ImageModelMetadata,
) -> eyre::Result<PathBuf> {
    let relative = validated_relative_path(&metadata.source_checkpoint_path)?;
    Ok(image_model_source_dir(model_dir).join(relative))
}

fn archive_file_name_from_url(url: &str) -> eyre::Result<&str> {
    url.rsplit('/')
        .next()
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .ok_or_else(|| eyre::eyre!("failed to derive image model archive file name from {url}"))
}

fn validated_relative_path(path: &str) -> eyre::Result<PathBuf> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        bail!("image model source path must not be empty");
    }
    let relative = Path::new(trimmed);
    if relative.is_absolute() {
        bail!("image model source path must be relative: {trimmed}");
    }
    for component in relative.components() {
        match component {
            Component::Normal(_) | Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                bail!("image model source path must stay within the model dir: {trimmed}");
            }
        }
    }
    Ok(relative.to_path_buf())
}

fn extract_checkpoint_from_archive(
    archive_path: &Path,
    checkpoint_path: &Path,
    metadata: &ImageModelMetadata,
) -> eyre::Result<()> {
    let relative_checkpoint = validated_relative_path(&metadata.source_checkpoint_path)?;
    let file = std::fs::File::open(archive_path).wrap_err_with(|| {
        format!(
            "failed to open image model archive {}",
            archive_path.display()
        )
    })?;
    let mut archive = zip::ZipArchive::new(file).wrap_err_with(|| {
        format!(
            "failed to open ZIP image model archive {}",
            archive_path.display()
        )
    })?;

    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).wrap_err_with(|| {
            format!(
                "failed to read ZIP entry {index} from {}",
                archive_path.display()
            )
        })?;
        let Some(enclosed_name) = entry.enclosed_name() else {
            continue;
        };
        if enclosed_name != relative_checkpoint {
            continue;
        }

        if let Some(parent) = checkpoint_path.parent() {
            std::fs::create_dir_all(parent).wrap_err_with(|| {
                format!(
                    "failed to create checkpoint parent dir {}",
                    parent.display()
                )
            })?;
        }

        let mut output = std::fs::File::create(checkpoint_path).wrap_err_with(|| {
            format!(
                "failed to create extracted image model checkpoint {}",
                checkpoint_path.display()
            )
        })?;
        std::io::copy(&mut entry, &mut output).wrap_err_with(|| {
            format!(
                "failed to write extracted image model checkpoint {}",
                checkpoint_path.display()
            )
        })?;
        return Ok(());
    }

    bail!(
        "image model archive {} does not contain checkpoint {}",
        archive_path.display(),
        metadata.source_checkpoint_path
    )
}

fn download_to_file(url: &str, destination: &Path) -> eyre::Result<()> {
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent).wrap_err_with(|| {
            format!(
                "failed to create image model download dir {}",
                parent.display()
            )
        })?;
    }
    let client = reqwest::blocking::Client::builder()
        .build()
        .wrap_err("failed to build HTTP client for image model download")?;
    let mut response = client
        .get(url)
        .send()
        .wrap_err_with(|| format!("failed to download image model archive from {url}"))?
        .error_for_status()
        .wrap_err_with(|| format!("image model archive download returned an error for {url}"))?;
    let mut output = std::fs::File::create(destination).wrap_err_with(|| {
        format!(
            "failed to create downloaded image model archive {}",
            destination.display()
        )
    })?;
    std::io::copy(&mut response, &mut output).wrap_err_with(|| {
        format!(
            "failed to write downloaded image model archive {}",
            destination.display()
        )
    })?;
    Ok(())
}

fn file_size_bytes(path: &Path) -> Option<u64> {
    std::fs::metadata(path).ok().map(|metadata| metadata.len())
}

fn read_image_model_metadata(path: &Path) -> eyre::Result<ImageModelMetadata> {
    let json = std::fs::read_to_string(path)
        .wrap_err_with(|| format!("failed to read image model metadata {}", path.display()))?;
    let value: serde_json::Value = serde_json::from_str(&json)
        .wrap_err_with(|| format!("failed to parse image model metadata {}", path.display()))?;
    Ok(ImageModelMetadata {
        model_name: read_string_field(&value, "model_name")?,
        family: read_string_field(&value, "family")?,
        style: read_string_field(&value, "style")?,
        method: read_string_field(&value, "method")?,
        noise_level: read_optional_u8_field(&value, "noise_level")?,
        scale: read_u8_field(&value, "scale")?,
        native_scale: read_u8_field(&value, "native_scale")?,
        architecture: read_string_field(&value, "architecture")?,
        source_archive_url: read_string_field(&value, "source_archive_url")?,
        source_archive_version: read_string_field(&value, "source_archive_version")?,
        source_checkpoint_path: read_string_field(&value, "source_checkpoint_path")?,
        model_offset: read_u32_field(&value, "model_offset")?,
        blend_size: read_u32_field(&value, "blend_size")?,
        default_tile_size: read_u32_field(&value, "default_tile_size")?,
        default_batch_size: read_u32_field(&value, "default_batch_size")?,
        input_channels: read_u32_field(&value, "input_channels")?,
        output_channels: read_u32_field(&value, "output_channels")?,
        parameter_count: read_optional_u64_field(&value, "parameter_count")?,
        alpha_behavior: read_string_field(&value, "alpha_behavior")?,
        teamy_runtime_status: read_string_field(&value, "teamy_runtime_status")?,
        teamy_runtime_notes: read_string_field(&value, "teamy_runtime_notes")?,
    })
}

#[derive(Clone, Debug, serde::Deserialize, PartialEq, Eq)]
struct ImageModelCheckpointKwargs {
    in_channels: u64,
    out_channels: u64,
}

fn read_string_field(value: &serde_json::Value, field: &str) -> eyre::Result<String> {
    value
        .get(field)
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
        .ok_or_else(|| eyre::eyre!("image model metadata field `{field}` must be a string"))
}

fn read_u8_field(value: &serde_json::Value, field: &str) -> eyre::Result<u8> {
    let number = read_u64_field(value, field)?;
    u8::try_from(number)
        .wrap_err_with(|| format!("image model metadata field `{field}` must fit in u8"))
}

fn read_optional_u8_field(value: &serde_json::Value, field: &str) -> eyre::Result<Option<u8>> {
    read_optional_u64_field(value, field)?
        .map(|number| {
            u8::try_from(number)
                .wrap_err_with(|| format!("image model metadata field `{field}` must fit in u8"))
        })
        .transpose()
}

fn read_u32_field(value: &serde_json::Value, field: &str) -> eyre::Result<u32> {
    let number = read_u64_field(value, field)?;
    u32::try_from(number)
        .wrap_err_with(|| format!("image model metadata field `{field}` must fit in u32"))
}

fn read_u64_field(value: &serde_json::Value, field: &str) -> eyre::Result<u64> {
    value
        .get(field)
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| eyre::eyre!("image model metadata field `{field}` must be an integer"))
}

fn read_optional_u64_field(value: &serde_json::Value, field: &str) -> eyre::Result<Option<u64>> {
    match value.get(field) {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(number) => number.as_u64().map(Some).ok_or_else(|| {
            eyre::eyre!("image model metadata field `{field}` must be an integer or null")
        }),
    }
}

fn unknown_model_error(model_name: &str) -> eyre::Report {
    eyre::eyre!(
        "unknown image model `{model_name}`; known models: {}",
        KNOWN_IMAGE_MODELS
            .iter()
            .map(|model| model.name)
            .collect::<Vec<_>>()
            .join(", ")
    )
}

#[cfg(test)]
#[expect(
    clippy::assertions_on_result_states,
    reason = "image-model tests intentionally assert only the success state for validation helpers"
)]
#[expect(
    clippy::cast_precision_loss,
    reason = "probe-model tests generate bounded synthetic float inputs from small integer ranges"
)]
#[expect(
    clippy::default_trait_access,
    reason = "device fixtures use Default::default for concise test setup"
)]
#[expect(
    clippy::uninlined_format_args,
    reason = "the skip message is kept literal-first to match the surrounding test output style"
)]
mod tests {
    use super::*;

    #[test]
    fn known_image_models_include_art_denoise_inventory() {
        let model = known_image_model("waifu2x-art-denoise-3-4x").expect("known model");

        assert_eq!(model.family, "waifu2x");
        assert_eq!(model.style, "art");
        assert_eq!(model.method, "noise_scale4x");
        assert_eq!(model.noise_level, Some(3));
        assert_eq!(model.scale, 4);
        assert_eq!(model.native_scale, 4);
        assert_eq!(model.architecture, "waifu2x.swin_unet_4x");
        assert_eq!(
            model.source_checkpoint_path,
            "pretrained_models/swin_unet/art/noise3_scale4x.pth"
        );
        assert_eq!(
            model.teamy_runtime_status,
            IMAGE_MODEL_RUNTIME_STATUS_IMPLEMENTED
        );
        assert_eq!(model.model_offset, 32);
        assert_eq!(model.blend_size, 16);
    }

    #[test]
    fn prepare_rejects_inventory_only_models_before_touching_cache() {
        let temp = tempfile::tempdir().expect("tempdir");
        let cache_home = CacheHome(temp.path().to_path_buf());

        let error = prepare_image_model(&cache_home, "waifu2x-art-denoise-0", false)
            .expect_err("inventory-only model should not prepare yet");
        let error_text = format!("{error:#}");

        assert!(error_text.contains("inventory-only"));
        assert!(error_text.contains("cannot be prepare yet"));
        assert!(!managed_image_model_dir(&cache_home, "waifu2x-art-denoise-0").exists());
    }

    #[test]
    fn validated_relative_path_rejects_escape_components() {
        assert!(validated_relative_path("../scale2x.pth").is_err());
        assert!(validated_relative_path("C:/temp/scale2x.pth").is_err());
    }

    #[test]
    fn checkpoint_path_stays_under_source_dir() {
        let temp = tempfile::tempdir().expect("tempdir");
        let metadata = ImageModelMetadata::from(&KNOWN_IMAGE_MODELS[0]);
        let checkpoint_path =
            image_model_source_checkpoint_path(temp.path(), &metadata).expect("checkpoint path");
        assert!(checkpoint_path.starts_with(temp.path().join(IMAGE_MODEL_SOURCE_DIR_NAME)));
        assert!(checkpoint_path.ends_with(Path::new(&metadata.source_checkpoint_path)));
    }

    #[test]
    fn waifu2x_tile_size_falls_back_to_the_nearest_valid_value() {
        assert_eq!(find_valid_waifu2x_tile_size(256).expect("tile size"), 256);
        assert_eq!(find_valid_waifu2x_tile_size(255).expect("tile size"), 208);
    }

    #[test]
    fn waifu2x_tile_size_shrinks_for_small_images() {
        assert_eq!(
            choose_waifu2x_tile_size(256, 64, 64, 2, 16, 8).expect("tile size"),
            112
        );
        assert_eq!(
            choose_waifu2x_tile_size(256, 100, 100, 2, 16, 8).expect("tile size"),
            160
        );
    }

    #[test]
    fn waifu2x_checkpoint_scale_detection_distinguishes_2x_and_4x() {
        assert_eq!(
            waifu2x_output_scale_from_checkpoint_keys([
                "unet.patch.0.weight",
                "unet.to_image.proj.weight",
            ]),
            2
        );
        assert_eq!(
            waifu2x_output_scale_from_checkpoint_keys([
                "unet.patch.0.weight",
                WAIFU2X_PROJ2_WEIGHT_KEY,
                "unet.to_image.proj.weight",
            ]),
            4
        );
    }

    #[test]
    fn waifu2x_forward_preserves_the_known_2x_shape_contract() {
        let device = Default::default();
        let model = init_waifu2x_probe_model::<Waifu2xProbeBackend>(&device, 2);
        let values = (0..(64 * 64 * 3))
            .map(|value| (value % 257) as f32 / 256.0)
            .collect::<Vec<_>>();
        let input = Tensor::<Waifu2xProbeBackend, 4>::from_data(
            TensorData::new(values, [1, 3, 64, 64]),
            &device,
        );

        let output = model.forward(input);

        assert_eq!(output.dims(), [1, 3, 96, 96]);
    }

    #[test]
    fn waifu2x_forward_preserves_the_known_4x_shape_contract() {
        let device = Default::default();
        let model = init_waifu2x_probe_model::<Waifu2xProbeBackend>(&device, 4);
        let values = (0..(64 * 64 * 3))
            .map(|value| (value % 257) as f32 / 256.0)
            .collect::<Vec<_>>();
        let input = Tensor::<Waifu2xProbeBackend, 4>::from_data(
            TensorData::new(values, [1, 3, 64, 64]),
            &device,
        );

        let output = model.forward(input);

        assert_eq!(output.dims(), [1, 3, 192, 192]);
    }

    #[test]
    fn waifu2x_tiled_inference_handles_sizes_the_untiled_path_rejected() {
        let device = Default::default();
        let model = init_waifu2x_probe_model::<Waifu2xProbeBackend>(&device, 2);
        let values = (0..(100 * 100 * 3))
            .map(|value| (value % 257) as f32 / 256.0)
            .collect::<Vec<_>>();

        let (rgb, output_width, output_height) = upscale_waifu2x_tiled_rgb_with_model(
            &model, &device, &values, 100, 100, 64, 2, 2, 16, 8, false, "test-rgb",
        )
        .expect("tiled inference");

        assert_eq!(output_width, 200);
        assert_eq!(output_height, 200);
        assert_eq!(rgb.len(), 3 * 200 * 200);
    }

    #[test]
    fn waifu2x_tta_transform_inverse_roundtrip_restores_tile() {
        let tile_size = 4;
        let tile = (0..(3 * tile_size * tile_size))
            .map(|value| value as f32 / 10.0)
            .collect::<Vec<_>>();

        for transform in WAIFU2X_TTA_TRANSFORMS {
            let transformed =
                transform_rgb_chw_square_tile(&tile, tile_size, transform).expect("transform");
            let restored =
                transform_rgb_chw_square_tile(&transformed, tile_size, transform.inverse())
                    .expect("inverse transform");

            assert_eq!(restored, tile);
        }
    }

    #[test]
    fn waifu2x_tiled_inference_supports_tta() {
        if !is_full_check_enabled() {
            eprintln!(
                "skipping waifu2x_tiled_inference_supports_tta unless {}=1",
                TEAMY_STUDIO_FULL_CHECK_ENV_VAR
            );
            return;
        }
        let device = Default::default();
        let model = init_waifu2x_probe_model::<Waifu2xProbeBackend>(&device, 2);
        let values = (0..(64 * 64 * 3))
            .map(|value| (value % 257) as f32 / 256.0)
            .collect::<Vec<_>>();
        let tile_size = choose_waifu2x_tile_size(256, 64, 64, 2, 16, 8).expect("tile size");

        let (rgb, output_width, output_height) = upscale_waifu2x_tiled_rgb_with_model(
            &model,
            &device,
            &values,
            64,
            64,
            tile_size,
            1,
            2,
            16,
            8,
            true,
            "test-rgb-tta",
        )
        .expect("tiled inference with tta");

        assert_eq!(output_width, 128);
        assert_eq!(output_height, 128);
        assert_eq!(rgb.len(), 3 * 128 * 128);
    }

    #[test]
    fn waifu2x_alpha_border_padding_fills_transparent_neighbors_from_opaque_pixels() {
        let rgb = vec![
            0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
            0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
        ];
        let alpha = vec![0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0];

        let padded =
            apply_waifu2x_alpha_border_padding(&rgb, &alpha, 3, 3, 1).expect("alpha border");

        assert!(padded[0] > 0.99);
        assert!(padded[4] > 0.99);
        assert!(padded[8] > 0.99);
        assert!(padded[9] == 0.0);
        assert!(padded[18] == 0.0);
    }
}

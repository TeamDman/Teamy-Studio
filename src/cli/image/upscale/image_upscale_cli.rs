use crate::cli::output::CliOutput;
use arbitrary::Arbitrary;
use eyre::{WrapErr, bail, ensure};
use facet::Facet;
use figue as args;
use image::{ColorType, DynamicImage, ImageFormat, RgbImage, RgbaImage};
use std::path::{Path, PathBuf};

pub const DEFAULT_IMAGE_UPSCALE_STYLE: ImageUpscaleStyle = ImageUpscaleStyle::Art;
pub const DEFAULT_IMAGE_UPSCALE_SCALE: u8 = 2;
pub const DEFAULT_IMAGE_UPSCALE_TILE_SIZE: u32 = 256;
pub const DEFAULT_IMAGE_UPSCALE_BATCH_SIZE: u32 = 4;
pub const DEFAULT_IMAGE_UPSCALE_DEVICE: ImageUpscaleDevice = ImageUpscaleDevice::Cuda;
pub const DEFAULT_IMAGE_UPSCALE_OUTPUT_FORMAT: ImageOutputFormat = ImageOutputFormat::Auto;

/// Image upscale style.
#[derive(Facet, Arbitrary, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[facet(rename_all = "kebab-case")]
#[repr(u8)]
pub enum ImageUpscaleStyle {
    #[default]
    Art,
    Photo,
    Scan,
    ArtScan,
}

impl ImageUpscaleStyle {
    #[must_use]
    const fn as_model_style(self) -> &'static str {
        match self {
            Self::Art => "art",
            Self::Photo => "photo",
            Self::Scan | Self::ArtScan => "art_scan",
        }
    }
}

/// Image upscale execution device.
#[derive(Facet, Arbitrary, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[facet(rename_all = "kebab-case")]
#[repr(u8)]
pub enum ImageUpscaleDevice {
    #[default]
    Cuda,
    Cpu,
}

/// Image output file format.
#[derive(Facet, Arbitrary, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[facet(rename_all = "kebab-case")]
#[repr(u8)]
pub enum ImageOutputFormat {
    #[default]
    Auto,
    Png,
    Jpeg,
    Webp,
}

impl ImageOutputFormat {
    #[must_use]
    pub const fn extension(self) -> Option<&'static str> {
        match self {
            Self::Auto => None,
            Self::Png => Some("png"),
            Self::Jpeg => Some("jpg"),
            Self::Webp => Some("webp"),
        }
    }

    fn infer_from_path(path: &Path) -> eyre::Result<Self> {
        let extension = path
            .extension()
            .and_then(std::ffi::OsStr::to_str)
            .map(str::to_ascii_lowercase)
            .ok_or_else(|| {
                eyre::eyre!("output path must have an extension when --output-format auto is used")
            })?;
        match extension.as_str() {
            "png" => Ok(Self::Png),
            "jpg" | "jpeg" => Ok(Self::Jpeg),
            "webp" => Ok(Self::Webp),
            _ => bail!(
                "unsupported image output extension `.{extension}`; supported formats: png, jpg, jpeg, webp"
            ),
        }
    }
}

/// Resolved output target for an image upscale request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedImageOutput {
    pub path: PathBuf,
    pub format: ImageOutputFormat,
}

#[derive(Clone, Debug, Facet, PartialEq)]
pub struct ImageUpscaleReport {
    pub model_name: String,
    pub input_path: String,
    pub output_path: String,
    pub output_format: String,
    pub tile_size: u32,
    pub batch_size: u32,
    pub input_width: u32,
    pub input_height: u32,
    pub output_width: u32,
    pub output_height: u32,
    pub blank_alpha: bool,
    pub alpha_preserved: bool,
    pub runtime_mode: String,
}

#[derive(Clone, Debug, PartialEq)]
struct PreparedImageUpscaleInput {
    width: u32,
    height: u32,
    rgb_chw: Vec<f32>,
    alpha_hw: Option<Vec<f32>>,
    blank_alpha: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ImageUpscaleTensorSummary {
    pub shape: Vec<u32>,
    pub min: f32,
    pub max: f32,
    pub mean: f32,
    pub sum: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PreparedImageUpscaleInputSummary {
    pub pil_size: Vec<u32>,
    pub blank_alpha: bool,
    pub rgb: ImageUpscaleTensorSummary,
    pub alpha: Option<ImageUpscaleTensorSummary>,
}

/// Upscale one image with the Rust Burn waifu2x backend.
// image[impl cli.upscale-command]
#[derive(Facet, Arbitrary, Debug, PartialEq)]
#[facet(rename_all = "kebab-case")]
pub struct ImageUpscaleArgs {
    /// Input image path.
    #[facet(args::positional)]
    pub image_path: String,

    /// Output image path. When omitted, Teamy Studio writes beside the input.
    #[facet(args::positional)]
    pub output_path: Option<String>,

    /// Upscale style to use.
    // image[impl cli.upscale-defaults]
    #[facet(args::named, default = DEFAULT_IMAGE_UPSCALE_STYLE)]
    pub style: ImageUpscaleStyle,

    /// Upscale factor. Supports powers of two starting at 2.
    // image[impl cli.upscale-defaults]
    #[facet(args::named, default = DEFAULT_IMAGE_UPSCALE_SCALE)]
    pub scale: u8,

    /// Optional upstream-compatible noise level for denoise-aware model selection.
    #[facet(args::named)]
    pub noise_level: Option<u8>,

    /// Disable the default low-denoise art preset and use the scale-only art model instead.
    #[facet(args::named, default)]
    pub disable_denoise: bool,

    /// Nunif-compatible tile size.
    // image[impl cli.upscale-defaults]
    #[facet(args::named, default = DEFAULT_IMAGE_UPSCALE_TILE_SIZE)]
    pub tile_size: u32,

    /// Nunif-compatible tile batch size.
    // image[impl cli.upscale-defaults]
    #[facet(args::named, default = DEFAULT_IMAGE_UPSCALE_BATCH_SIZE)]
    pub batch_size: u32,

    /// Inference device.
    // image[impl cli.upscale-defaults]
    #[facet(args::named, default = DEFAULT_IMAGE_UPSCALE_DEVICE)]
    pub device: ImageUpscaleDevice,

    /// Output image format.
    // image[impl cli.upscale-defaults]
    #[facet(args::named, default = DEFAULT_IMAGE_UPSCALE_OUTPUT_FORMAT)]
    pub output_format: ImageOutputFormat,
}

impl ImageUpscaleArgs {
    /// # Errors
    ///
    /// This function will return an error if argument validation fails or the upscale pipeline fails.
    #[expect(
        clippy::too_many_lines,
        reason = "CLI flow keeps the user-facing image upscale sequence in one place"
    )]
    pub fn invoke(
        self,
        _app_home: &crate::paths::AppHome,
        cache_home: &crate::paths::CacheHome,
    ) -> eyre::Result<CliOutput> {
        let output = self.resolve_output()?;
        let resolved_model = self.resolve_model()?;
        self.validate_supported_mvp_arguments(resolved_model)?;
        // image[impl cli.auto-prepare-default-model]
        let model_status = crate::image_model::inspect_image_model(cache_home, resolved_model.name);
        if !matches!(
            model_status.state,
            crate::image_model::ImageModelPreparationState::Prepared
        ) {
            tracing::warn!(
                model = resolved_model.name,
                state = ?model_status.state,
                "resolved image model is not prepared; preparing managed image model Burnpack"
            );
            let _prepared =
                crate::image_model::prepare_image_model(cache_home, resolved_model.name, false)?;
        }
        let upscale_passes = self.upscale_pass_count_for_model(resolved_model.scale)?;
        let prepared_input = load_image_upscale_input(Path::new(&self.image_path))?;
        let ran_alpha_runtime = prepared_input.alpha_hw.is_some();
        let mut rgb_chw = prepared_input.rgb_chw;
        let mut alpha_hw = prepared_input.alpha_hw;
        let mut output_width = prepared_input.width;
        let mut output_height = prepared_input.height;
        let mut actual_tile_size = self.tile_size;

        match self.device {
            ImageUpscaleDevice::Cpu => {
                crate::image_model::validate_managed_image_model_burnpack_load(
                    cache_home,
                    resolved_model.name,
                )?;
                for pass_index in 0..upscale_passes {
                    tracing::info!(
                        device = "cpu",
                        scale = self.scale,
                        upscale_pass = pass_index + 1,
                        total_upscale_passes = upscale_passes,
                        "image upscale pass started"
                    );
                    let blank_alpha = alpha_hw
                        .as_deref()
                        .is_none_or(|values| values.iter().all(|value| *value >= 1.0));
                    let result = crate::image_model::upscale_managed_image_model_tiled_rgba(
                        cache_home,
                        resolved_model.name,
                        &rgb_chw,
                        alpha_hw.as_deref(),
                        blank_alpha,
                        output_width,
                        output_height,
                        self.tile_size,
                        self.batch_size,
                    )?;
                    (
                        rgb_chw,
                        alpha_hw,
                        output_width,
                        output_height,
                        actual_tile_size,
                    ) = result;
                }
            }
            ImageUpscaleDevice::Cuda => {
                let device = crate::image_model::waifu2x_inference_device();
                crate::image_model::validate_managed_image_model_burnpack_load_cuda(
                    cache_home,
                    resolved_model.name,
                    &device,
                )?;
                for pass_index in 0..upscale_passes {
                    tracing::info!(
                        device = "cuda",
                        scale = self.scale,
                        upscale_pass = pass_index + 1,
                        total_upscale_passes = upscale_passes,
                        "image upscale pass started"
                    );
                    let blank_alpha = alpha_hw
                        .as_deref()
                        .is_none_or(|values| values.iter().all(|value| *value >= 1.0));
                    let result = crate::image_model::upscale_managed_image_model_tiled_rgba_cuda(
                        cache_home,
                        resolved_model.name,
                        &rgb_chw,
                        alpha_hw.as_deref(),
                        blank_alpha,
                        output_width,
                        output_height,
                        self.tile_size,
                        self.batch_size,
                        &device,
                    )?;
                    (
                        rgb_chw,
                        alpha_hw,
                        output_width,
                        output_height,
                        actual_tile_size,
                    ) = result;
                }
            }
        }
        write_output_image(
            &output.path,
            output.format,
            output_width,
            output_height,
            &rgb_chw,
            alpha_hw.as_deref(),
        )?;
        let alpha_preserved = alpha_hw.is_some()
            && matches!(
                output.format,
                ImageOutputFormat::Png | ImageOutputFormat::Webp
            );
        tracing::info!(
            input = %self.image_path,
            output = %output.path.display(),
            format = ?output.format,
            model = resolved_model.name,
            scale = self.scale,
            width = prepared_input.width,
            height = prepared_input.height,
            tile_size = actual_tile_size,
            batch_size = self.batch_size,
            output_width,
            output_height,
            blank_alpha = prepared_input.blank_alpha,
            alpha_preserved,
            "image upscale tiled Burn inference succeeded"
        );
        Ok(CliOutput::facet(ImageUpscaleReport {
            model_name: resolved_model.name.to_owned(),
            input_path: self.image_path,
            output_path: output.path.display().to_string(),
            output_format: format!("{:?}", output.format),
            tile_size: actual_tile_size,
            batch_size: self.batch_size,
            input_width: prepared_input.width,
            input_height: prepared_input.height,
            output_width,
            output_height,
            blank_alpha: prepared_input.blank_alpha,
            alpha_preserved,
            runtime_mode: if ran_alpha_runtime {
                "tiled-rgba".to_owned()
            } else {
                "tiled-rgb-only".to_owned()
            },
        }))
    }

    /// # Errors
    ///
    /// This function returns an error if the output format and output path are incompatible.
    // image[impl cli.output-path-generation]
    // image[impl cli.output-format-inference]
    // image[impl cli.output-format-conflict]
    pub fn resolve_output(&self) -> eyre::Result<ResolvedImageOutput> {
        let input_path = Path::new(&self.image_path);
        let explicit_output = self.output_path.as_deref().map(Path::new);
        match (explicit_output, self.output_format) {
            (Some(output_path), ImageOutputFormat::Auto) => Ok(ResolvedImageOutput {
                path: output_path.to_path_buf(),
                format: ImageOutputFormat::infer_from_path(output_path)?,
            }),
            (Some(output_path), explicit_format) => {
                let inferred = ImageOutputFormat::infer_from_path(output_path)?;
                ensure!(
                    inferred == explicit_format,
                    "--output-format {explicit_format:?} conflicts with output path extension in {}",
                    output_path.display()
                );
                Ok(ResolvedImageOutput {
                    path: output_path.to_path_buf(),
                    format: explicit_format,
                })
            }
            (None, ImageOutputFormat::Auto) => Ok(ResolvedImageOutput {
                path: generated_output_path(input_path, ImageOutputFormat::Png, self.scale)?,
                format: ImageOutputFormat::Png,
            }),
            (None, explicit_format) => Ok(ResolvedImageOutput {
                path: generated_output_path(input_path, explicit_format, self.scale)?,
                format: explicit_format,
            }),
        }
    }

    // image[impl cli.model-selection]
    // image[impl cli.art-default-low-denoise]
    // image[impl cli.art-disable-denoise]
    // image[impl cli.unsupported-model-selection]
    fn resolve_model(&self) -> eyre::Result<&'static crate::image_model::KnownImageModel> {
        ensure!(
            self.noise_level.is_none_or(|value| value <= 3),
            "--noise-level must be between 0 and 3"
        );
        ensure!(
            !(self.disable_denoise && self.noise_level.is_some()),
            "--disable-denoise cannot be combined with --noise-level"
        );
        let effective_noise_level = self.effective_noise_level();
        let prefer_native_4x = self.prefers_native_4x_art_model();
        let method = if effective_noise_level.is_some() {
            if prefer_native_4x {
                "noise_scale4x"
            } else {
                "noise_scale2x"
            }
        } else if prefer_native_4x {
            "scale4x"
        } else {
            crate::image_model::default_upscale_method()
        };
        crate::image_model::resolve_image_model_for_request(
            crate::image_model::ImageModelSelectionRequest {
                style: self.style.as_model_style(),
                method,
                noise_level: effective_noise_level,
            },
        )
    }

    #[must_use]
    fn effective_noise_level(&self) -> Option<u8> {
        if self.disable_denoise {
            None
        } else {
            self.noise_level
                .or_else(|| (self.style == ImageUpscaleStyle::Art).then_some(0))
        }
    }

    fn validate_supported_mvp_arguments(
        &self,
        resolved_model: &crate::image_model::KnownImageModel,
    ) -> eyre::Result<()> {
        let _ = self.upscale_pass_count_for_model(resolved_model.scale)?;
        ensure!(self.tile_size > 0, "--tile-size must be greater than zero");
        ensure!(
            self.batch_size > 0,
            "--batch-size must be greater than zero"
        );
        Ok(())
    }

    fn upscale_pass_count_for_model(&self, model_scale: u8) -> eyre::Result<u8> {
        ensure!(
            self.scale >= 2 && self.scale.is_power_of_two(),
            "image upscale currently supports powers-of-two scales starting at 2"
        );
        ensure!(
            model_scale >= 2 && model_scale.is_power_of_two(),
            "resolved image model scale must be a power of two starting at 2"
        );
        let mut remaining_scale = self.scale;
        let mut passes = 0_u8;
        while remaining_scale > 1 {
            ensure!(
                remaining_scale.is_multiple_of(model_scale),
                "image upscale scale {} cannot be composed from repeated {}x passes of model `{}`",
                self.scale,
                model_scale,
                model_scale
            );
            remaining_scale /= model_scale;
            passes = passes
                .checked_add(1)
                .ok_or_else(|| eyre::eyre!("image upscale pass count overflowed u8"))?;
        }
        Ok(passes)
    }

    fn prefers_native_4x_art_model(&self) -> bool {
        self.style == ImageUpscaleStyle::Art
            && self.scale >= 4
            && self.scale.is_power_of_two()
            && self.scale.ilog2().is_multiple_of(2)
    }
}

fn generated_output_path(
    input_path: &Path,
    format: ImageOutputFormat,
    scale: u8,
) -> eyre::Result<PathBuf> {
    let extension = format
        .extension()
        .ok_or_else(|| eyre::eyre!("cannot generate output path for automatic output format"))?;
    let parent = input_path.parent().unwrap_or_else(|| Path::new(""));
    let stem = input_path
        .file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .ok_or_else(|| eyre::eyre!("input image path must have a valid file name"))?;
    Ok(parent.join(format!("{stem}.upscaled-{scale}x.{extension}")))
}

fn load_image_upscale_input(path: &Path) -> eyre::Result<PreparedImageUpscaleInput> {
    let image = image::open(path)
        .map_err(eyre::Report::from)
        .wrap_err_with(|| format!("failed to open input image {}", path.display()))?;
    let has_alpha = matches!(
        image.color(),
        ColorType::La8
            | ColorType::La16
            | ColorType::Rgba8
            | ColorType::Rgba16
            | ColorType::Rgba32F
    );
    let rgba = image.to_rgba8();
    let (width, height) = rgba.dimensions();
    let pixel_count = usize::try_from(u64::from(width) * u64::from(height))
        .wrap_err("input image is too large to fit in memory on this platform")?;

    let mut rgb_chw = vec![0.0_f32; pixel_count * 3];
    let mut alpha_hw = has_alpha.then(|| vec![1.0_f32; pixel_count]);
    let mut blank_alpha = true;

    for (index, pixel) in rgba.pixels().enumerate() {
        let r = f32::from(pixel[0]) / 255.0;
        let g = f32::from(pixel[1]) / 255.0;
        let b = f32::from(pixel[2]) / 255.0;
        rgb_chw[index] = r;
        rgb_chw[pixel_count + index] = g;
        rgb_chw[(pixel_count * 2) + index] = b;
        if let Some(alpha_hw) = alpha_hw.as_mut() {
            alpha_hw[index] = f32::from(pixel[3]) / 255.0;
            if pixel[3] != u8::MAX {
                blank_alpha = false;
            }
        }
    }

    Ok(PreparedImageUpscaleInput {
        width,
        height,
        rgb_chw,
        alpha_hw,
        blank_alpha,
    })
}

pub(crate) fn summarize_image_upscale_input(
    path: &Path,
) -> eyre::Result<PreparedImageUpscaleInputSummary> {
    let prepared = load_image_upscale_input(path)?;
    Ok(PreparedImageUpscaleInputSummary {
        pil_size: vec![prepared.width, prepared.height],
        blank_alpha: prepared.blank_alpha,
        rgb: summarize_chw_tensor(&prepared.rgb_chw, 3, prepared.width, prepared.height)?,
        alpha: prepared
            .alpha_hw
            .as_deref()
            .map(|alpha_hw| summarize_hw_tensor(alpha_hw, prepared.width, prepared.height))
            .transpose()?,
    })
}

fn summarize_chw_tensor(
    values: &[f32],
    channels: u32,
    width: u32,
    height: u32,
) -> eyre::Result<ImageUpscaleTensorSummary> {
    let expected_len = usize::try_from(u64::from(channels) * u64::from(width) * u64::from(height))
        .wrap_err("image upscale summary tensor length does not fit in usize")?;
    ensure!(
        values.len() == expected_len,
        "image upscale summary expected {} values for {}x{}x{}, got {}",
        expected_len,
        channels,
        height,
        width,
        values.len()
    );
    summarize_tensor(values, vec![channels, height, width])
}

fn summarize_hw_tensor(
    values: &[f32],
    width: u32,
    height: u32,
) -> eyre::Result<ImageUpscaleTensorSummary> {
    let expected_len = usize::try_from(u64::from(width) * u64::from(height))
        .wrap_err("image upscale summary alpha length does not fit in usize")?;
    ensure!(
        values.len() == expected_len,
        "image upscale summary expected {} alpha values for {}x{}, got {}",
        expected_len,
        width,
        height,
        values.len()
    );
    summarize_tensor(values, vec![1, height, width])
}

fn summarize_tensor(values: &[f32], shape: Vec<u32>) -> eyre::Result<ImageUpscaleTensorSummary> {
    let (first, rest) = values
        .split_first()
        .ok_or_else(|| eyre::eyre!("image upscale summary requires at least one value"))?;
    let mut min = *first;
    let mut max = *first;
    let mut sum = *first;
    for value in rest {
        min = min.min(*value);
        max = max.max(*value);
        sum += *value;
    }
    Ok(ImageUpscaleTensorSummary {
        shape,
        min,
        max,
        #[expect(
            clippy::cast_precision_loss,
            reason = "mean is reported as f32 alongside f32 tensor stats"
        )]
        mean: sum / values.len() as f32,
        sum,
    })
}

fn write_output_image(
    path: &Path,
    format: ImageOutputFormat,
    width: u32,
    height: u32,
    rgb_chw: &[f32],
    alpha_hw: Option<&[f32]>,
) -> eyre::Result<()> {
    match format {
        ImageOutputFormat::Auto => {
            bail!("image upscale output format must be resolved before writing")
        }
        ImageOutputFormat::Png => {
            if let Some(alpha_hw) = alpha_hw {
                write_rgba_image(path, width, height, rgb_chw, alpha_hw, ImageFormat::Png)
            } else {
                write_rgb_image(path, width, height, rgb_chw, ImageFormat::Png)
            }
        }
        ImageOutputFormat::Webp => {
            if let Some(alpha_hw) = alpha_hw {
                write_rgba_image(path, width, height, rgb_chw, alpha_hw, ImageFormat::WebP)
            } else {
                write_rgb_image(path, width, height, rgb_chw, ImageFormat::WebP)
            }
        }
        ImageOutputFormat::Jpeg => {
            if let Some(alpha_hw) = alpha_hw {
                let flattened = flatten_rgba_over_white(width, height, rgb_chw, alpha_hw)?;
                write_rgb_image(path, width, height, &flattened, ImageFormat::Jpeg)
            } else {
                write_rgb_image(path, width, height, rgb_chw, ImageFormat::Jpeg)
            }
        }
    }
}

fn write_rgb_image(
    path: &Path,
    width: u32,
    height: u32,
    rgb_chw: &[f32],
    format: ImageFormat,
) -> eyre::Result<()> {
    let width_usize = usize::try_from(width).wrap_err("output width does not fit in usize")?;
    let height_usize = usize::try_from(height).wrap_err("output height does not fit in usize")?;
    let pixel_count = width_usize
        .checked_mul(height_usize)
        .ok_or_else(|| eyre::eyre!("output image pixel count overflowed usize"))?;
    let expected_len = pixel_count
        .checked_mul(3)
        .ok_or_else(|| eyre::eyre!("output RGB buffer length overflowed usize"))?;
    ensure!(
        rgb_chw.len() == expected_len,
        "output RGB buffer expected {} values for {}x{}, got {}",
        expected_len,
        width,
        height,
        rgb_chw.len()
    );

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .wrap_err_with(|| format!("failed to create output directory {}", parent.display()))?;
    }

    let mut image = RgbImage::new(width, height);
    for y in 0..height {
        for x in 0..width {
            let index = usize::try_from(u64::from(y) * u64::from(width) + u64::from(x))
                .wrap_err("output pixel index does not fit in usize")?;
            let red = quantize_u8(rgb_chw[index]);
            let green = quantize_u8(rgb_chw[pixel_count + index]);
            let blue = quantize_u8(rgb_chw[(pixel_count * 2) + index]);
            image.put_pixel(x, y, image::Rgb([red, green, blue]));
        }
    }
    DynamicImage::ImageRgb8(image)
        .save_with_format(path, format)
        .wrap_err_with(|| format!("failed to write output image {}", path.display()))?;
    Ok(())
}

fn write_rgba_image(
    path: &Path,
    width: u32,
    height: u32,
    rgb_chw: &[f32],
    alpha_hw: &[f32],
    format: ImageFormat,
) -> eyre::Result<()> {
    let width_usize = usize::try_from(width).wrap_err("output width does not fit in usize")?;
    let height_usize = usize::try_from(height).wrap_err("output height does not fit in usize")?;
    let pixel_count = width_usize
        .checked_mul(height_usize)
        .ok_or_else(|| eyre::eyre!("output image pixel count overflowed usize"))?;
    ensure!(
        rgb_chw.len() == pixel_count * 3,
        "output RGBA writer expected {} RGB values for {}x{}, got {}",
        pixel_count * 3,
        width,
        height,
        rgb_chw.len()
    );
    ensure!(
        alpha_hw.len() == pixel_count,
        "output RGBA writer expected {} alpha values for {}x{}, got {}",
        pixel_count,
        width,
        height,
        alpha_hw.len()
    );

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .wrap_err_with(|| format!("failed to create output directory {}", parent.display()))?;
    }

    let mut image = RgbaImage::new(width, height);
    for y in 0..height {
        for x in 0..width {
            let index = usize::try_from(u64::from(y) * u64::from(width) + u64::from(x))
                .wrap_err("output pixel index does not fit in usize")?;
            let red = quantize_u8(rgb_chw[index]);
            let green = quantize_u8(rgb_chw[pixel_count + index]);
            let blue = quantize_u8(rgb_chw[(pixel_count * 2) + index]);
            let alpha = quantize_u8(alpha_hw[index]);
            image.put_pixel(x, y, image::Rgba([red, green, blue, alpha]));
        }
    }
    DynamicImage::ImageRgba8(image)
        .save_with_format(path, format)
        .wrap_err_with(|| format!("failed to write output image {}", path.display()))?;
    Ok(())
}

fn flatten_rgba_over_white(
    width: u32,
    height: u32,
    rgb_chw: &[f32],
    alpha_hw: &[f32],
) -> eyre::Result<Vec<f32>> {
    let width_usize = usize::try_from(width).wrap_err("flatten width does not fit in usize")?;
    let height_usize = usize::try_from(height).wrap_err("flatten height does not fit in usize")?;
    let pixel_count = width_usize
        .checked_mul(height_usize)
        .ok_or_else(|| eyre::eyre!("flatten image pixel count overflowed usize"))?;
    ensure!(
        rgb_chw.len() == pixel_count * 3,
        "flatten expected {} RGB values for {}x{}, got {}",
        pixel_count * 3,
        width,
        height,
        rgb_chw.len()
    );
    ensure!(
        alpha_hw.len() == pixel_count,
        "flatten expected {} alpha values for {}x{}, got {}",
        pixel_count,
        width,
        height,
        alpha_hw.len()
    );
    let mut flattened = vec![0.0_f32; rgb_chw.len()];
    for index in 0..pixel_count {
        let alpha = alpha_hw[index].clamp(0.0, 1.0);
        flattened[index] = rgb_chw[index] * alpha + (1.0 - alpha);
        flattened[pixel_count + index] = rgb_chw[pixel_count + index] * alpha + (1.0 - alpha);
        flattened[pixel_count * 2 + index] =
            rgb_chw[pixel_count * 2 + index] * alpha + (1.0 - alpha);
    }
    Ok(flattened)
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "value is clamped into the 0..=255 byte range before conversion"
)]
#[expect(
    clippy::cast_sign_loss,
    reason = "value is clamped to a non-negative range before conversion"
)]
fn quantize_u8(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_args(scale: u8) -> ImageUpscaleArgs {
        ImageUpscaleArgs {
            image_path: String::from(r"C:\images\input.png"),
            output_path: None,
            style: DEFAULT_IMAGE_UPSCALE_STYLE,
            scale,
            noise_level: None,
            disable_denoise: false,
            tile_size: DEFAULT_IMAGE_UPSCALE_TILE_SIZE,
            batch_size: DEFAULT_IMAGE_UPSCALE_BATCH_SIZE,
            device: ImageUpscaleDevice::Cuda,
            output_format: ImageOutputFormat::Auto,
        }
    }

    #[test]
    fn image_upscale_accepts_scale_4() {
        let args = sample_args(4);
        args.validate_supported_mvp_arguments(args.resolve_model().expect("model"))
            .expect("scale 4 should be accepted");
    }

    #[test]
    fn image_upscale_accepts_scale_8() {
        let args = sample_args(8);
        args.validate_supported_mvp_arguments(args.resolve_model().expect("model"))
            .expect("scale 8 should be accepted");
    }

    #[test]
    fn image_upscale_rejects_non_power_of_two_scale() {
        let args = sample_args(6);
        let error = args
            .validate_supported_mvp_arguments(args.resolve_model().expect("model"))
            .expect_err("scale 6 should be rejected");

        assert!(
            error
                .to_string()
                .contains("supports powers-of-two scales starting at 2")
        );
    }

    #[test]
    fn image_upscale_generates_4x_output_path() {
        let output = sample_args(4).resolve_output().expect("resolved output");

        assert_eq!(
            output.path,
            PathBuf::from(r"C:\images\input.upscaled-4x.png")
        );
        assert_eq!(output.format, ImageOutputFormat::Png);
    }

    #[test]
    fn image_upscale_resolves_default_art_model() {
        let model = sample_args(2).resolve_model().expect("model");

        assert_eq!(model.name, "waifu2x-art-denoise-0-2x");
    }

    #[test]
    fn image_upscale_resolves_native_art_4x_model_for_scale_4() {
        let model = sample_args(4).resolve_model().expect("model");

        assert_eq!(model.name, "waifu2x-art-denoise-0-4x");
    }

    #[test]
    fn image_upscale_disable_denoise_restores_scale_only_art_model() {
        let mut args = sample_args(2);
        args.disable_denoise = true;

        let model = args.resolve_model().expect("model");

        assert_eq!(model.name, crate::image_model::DEFAULT_IMAGE_MODEL_NAME);
    }

    #[test]
    fn image_upscale_disable_denoise_restores_native_scale_only_art_4x_model() {
        let mut args = sample_args(4);
        args.disable_denoise = true;

        let model = args.resolve_model().expect("model");

        assert_eq!(model.name, "waifu2x-art-4x");
    }

    #[test]
    fn image_upscale_resolves_native_art_denoise_4x_model_for_scale_4() {
        let mut args = sample_args(4);
        args.noise_level = Some(3);

        let model = args.resolve_model().expect("model");

        assert_eq!(model.name, "waifu2x-art-denoise-3-4x");
    }

    #[test]
    fn image_upscale_resolves_art_scale_8_to_repeatable_2x_model() {
        let model = sample_args(8).resolve_model().expect("model");

        assert_eq!(model.name, "waifu2x-art-denoise-0-2x");
    }

    #[test]
    fn image_upscale_resolves_photo_model() {
        let mut args = sample_args(2);
        args.style = ImageUpscaleStyle::Photo;

        let model = args.resolve_model().expect("model");

        assert_eq!(model.name, "waifu2x-photo-2x");
    }

    #[test]
    fn image_upscale_resolves_scan_alias_to_art_scan_model() {
        let mut args = sample_args(2);
        args.style = ImageUpscaleStyle::Scan;

        let model = args.resolve_model().expect("model");

        assert_eq!(model.name, "waifu2x-art-scan-2x");
    }

    #[test]
    fn image_upscale_resolves_art_noise_model_request() {
        let mut args = sample_args(2);
        args.noise_level = Some(3);

        let model = args.resolve_model().expect("model");

        assert_eq!(model.name, "waifu2x-art-denoise-3-2x");
    }

    #[test]
    fn image_upscale_uses_one_pass_for_native_4x_model() {
        let args = sample_args(4);
        let model = args.resolve_model().expect("model");

        assert_eq!(
            args.upscale_pass_count_for_model(model.scale)
                .expect("passes"),
            1
        );
    }

    #[test]
    fn image_upscale_uses_two_passes_for_native_4x_scale_16() {
        let args = sample_args(16);
        let model = args.resolve_model().expect("model");

        assert_eq!(model.name, "waifu2x-art-denoise-0-4x");
        assert_eq!(
            args.upscale_pass_count_for_model(model.scale)
                .expect("passes"),
            2
        );
    }

    #[test]
    fn image_upscale_rejects_out_of_range_noise_level() {
        let mut args = sample_args(2);
        args.noise_level = Some(4);

        let error = args
            .resolve_model()
            .expect_err("noise level 4 should be rejected");

        assert!(
            error
                .to_string()
                .contains("--noise-level must be between 0 and 3")
        );
    }

    #[test]
    fn image_upscale_rejects_disable_denoise_with_explicit_noise_level() {
        let mut args = sample_args(2);
        args.noise_level = Some(0);
        args.disable_denoise = true;

        let error = args
            .resolve_model()
            .expect_err("disable-denoise should reject explicit noise level");

        assert!(
            error
                .to_string()
                .contains("--disable-denoise cannot be combined with --noise-level")
        );
    }
}

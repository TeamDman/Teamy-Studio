use crate::cli::output::CliOutput;
use arbitrary::Arbitrary;
use eyre::{WrapErr, ensure};
use facet::Facet;
use figue as args;
use image::RgbImage;
use std::path::{Path, PathBuf};

/// Compare the Rust waifu2x Burn port against the Python/nunif reference harness.
// image[impl self-test.reference-command]
#[derive(Facet, Arbitrary, Debug, PartialEq)]
#[facet(rename_all = "kebab-case")]
pub struct SelfTestImageUpscaleReferenceArgs {
    /// Create or use a uv environment for the Python/nunif reference harness.
    #[facet(args::named, default)]
    pub use_uv: bool,

    /// Download nunif waifu2x models into the configured `NUNIF_HOME` if missing.
    #[facet(args::named, default)]
    pub download_models: bool,

    /// Optional real image path to inspect with the nunif tensor loader.
    #[facet(args::named)]
    pub reference_image_path: Option<String>,

    /// Optional `.npz` path for dumping tensors from `--reference-image-path`.
    #[facet(args::named)]
    pub reference_dump_npz_path: Option<String>,
}

impl SelfTestImageUpscaleReferenceArgs {
    /// # Errors
    ///
    /// This function will return an error if the reference harness cannot run.
    pub fn invoke(
        self,
        _app_home: &crate::paths::AppHome,
        _cache_home: &crate::paths::CacheHome,
    ) -> eyre::Result<CliOutput> {
        let _ = self.use_uv;
        let imports = crate::waifu2x_reference::read_waifu2x_reference_import_report()?;
        ensure!(
            imports.ok,
            "waifu2x reference import check reported failure"
        );
        println!(
            "waifu2x reference imports: torch {}, torchvision {}, Pillow {}, CUDA available: {} ({})",
            imports.torch,
            imports.torchvision,
            imports.pillow,
            imports.cuda.available,
            imports
                .cuda
                .device_name
                .as_deref()
                .unwrap_or("no CUDA device")
        );

        let model =
            crate::waifu2x_reference::read_waifu2x_reference_model_report(self.download_models)?;
        ensure!(model.ok, "waifu2x reference model report reported failure");
        println!(
            "waifu2x reference model: {} {} class {} scale {} offset {} blend {} params {}",
            model.model_type,
            model.method,
            model.torch_model_class,
            model.i2i_scale,
            model.i2i_offset,
            model.i2i_blend_size,
            model.parameter_count
        );

        let fixture_path = reference_fixture_path();
        check_reference_fixture_tensors(&fixture_path)?;
        compare_rust_preprocessing_against_reference(&fixture_path, "transparent fixture")?;

        let opaque_fixture_path = reference_opaque_fixture_path();
        write_reference_opaque_fixture(&opaque_fixture_path)?;
        compare_rust_preprocessing_against_reference(&opaque_fixture_path, "opaque fixture")?;
        check_reference_layer_report()?;

        if let Some(reference_image_path) = self.reference_image_path.as_deref() {
            let reference_image_path = PathBuf::from(reference_image_path);
            let dump_npz_path = self.reference_dump_npz_path.as_deref().map(PathBuf::from);
            let real_image_report = crate::waifu2x_reference::read_waifu2x_reference_tensor_report(
                &reference_image_path,
                dump_npz_path.as_deref(),
            )?;
            ensure!(
                real_image_report.ok,
                "waifu2x reference real image tensor report reported failure"
            );
            println!(
                "waifu2x reference real image tensors: {} mode {} size {:?} RGB {:?} alpha {:?} blank_alpha {} dump {:?}",
                real_image_report.image_path,
                real_image_report.pil_mode,
                real_image_report.pil_size,
                real_image_report.tensors.rgb.shape,
                real_image_report
                    .tensors
                    .alpha
                    .as_ref()
                    .map(|alpha| alpha.shape.clone()),
                real_image_report.blank_alpha,
                real_image_report.dump_npz
            );
            compare_rust_preprocessing_against_reference(&reference_image_path, "real image")?;
        }
        Ok(CliOutput::none())
    }
}

fn reference_fixture_path() -> PathBuf {
    std::env::temp_dir().join("teamy-waifu2x-reference-fixture.png")
}

fn reference_opaque_fixture_path() -> PathBuf {
    std::env::temp_dir().join("teamy-waifu2x-reference-opaque-fixture.png")
}

fn check_reference_fixture_tensors(fixture_path: &std::path::Path) -> eyre::Result<()> {
    let fixture = crate::waifu2x_reference::write_waifu2x_reference_fixture(fixture_path)?;
    ensure!(
        fixture.ok,
        "waifu2x reference fixture report reported failure"
    );
    ensure!(fixture.pil_mode == "RGBA", "fixture should be RGBA");
    ensure!(fixture.pil_size == vec![4, 4], "fixture should be 4x4");

    let tensor_report =
        crate::waifu2x_reference::read_waifu2x_reference_tensor_report(fixture_path, None)?;
    ensure!(
        tensor_report.ok,
        "waifu2x reference tensor report reported failure"
    );
    ensure!(
        !tensor_report.blank_alpha,
        "fixture alpha should not be blank"
    );
    ensure!(
        tensor_report.tensors.rgb.shape == vec![3, 4, 4],
        "reference RGB tensor should be CHW 3x4x4"
    );
    let alpha = tensor_report
        .tensors
        .alpha
        .as_ref()
        .ok_or_else(|| eyre::eyre!("reference fixture should produce an alpha tensor"))?;
    ensure!(
        alpha.shape == vec![1, 4, 4],
        "reference alpha tensor should be CHW 1x4x4"
    );
    println!(
        "waifu2x reference fixture tensors: RGB {:?} alpha {:?} blank_alpha {}",
        tensor_report.tensors.rgb.shape, alpha.shape, tensor_report.blank_alpha
    );
    Ok(())
}

fn check_reference_layer_report() -> eyre::Result<()> {
    let layer_report = crate::waifu2x_reference::read_waifu2x_reference_layer_report(None)?;
    ensure!(
        layer_report.ok,
        "waifu2x reference layer report reported failure"
    );
    ensure!(
        layer_report.input_size == 64,
        "reference layer report should use deterministic 64x64 input"
    );
    let input = layer_report
        .layers
        .get("input")
        .ok_or_else(|| eyre::eyre!("reference layer report should include input"))?;
    ensure!(
        input.shape == vec![1, 3, 64, 64],
        "reference layer input should be NCHW 1x3x64x64"
    );
    let output = layer_report
        .layers
        .get("output")
        .ok_or_else(|| eyre::eyre!("reference layer report should include output"))?;
    ensure!(
        output.shape == vec![1, 3, 96, 96],
        "reference layer output should be offset-cropped NCHW 1x3x96x96"
    );
    ensure!(
        layer_report.layers.contains_key("unet.patch.0"),
        "reference layer report should include the first convolution output"
    );
    println!(
        "waifu2x reference layers: {} captured, input {:?}, output {:?}",
        layer_report.layers.len(),
        input.shape,
        output.shape
    );
    Ok(())
}

fn write_reference_opaque_fixture(path: &Path) -> eyre::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).wrap_err_with(|| {
            format!(
                "failed to create opaque reference fixture directory {}",
                parent.display()
            )
        })?;
    }
    let mut image = RgbImage::new(4, 4);
    let pixels = [
        [0, 0, 0],
        [255, 0, 0],
        [0, 255, 0],
        [0, 0, 255],
        [255, 255, 255],
        [64, 32, 16],
        [16, 32, 64],
        [128, 128, 0],
        [0, 128, 128],
        [128, 0, 128],
        [32, 64, 96],
        [96, 64, 32],
        [255, 128, 0],
        [0, 255, 128],
        [128, 0, 255],
        [255, 255, 0],
    ];
    for (index, pixel) in pixels.into_iter().enumerate() {
        let x = u32::try_from(index % 4).expect("x");
        let y = u32::try_from(index / 4).expect("y");
        image.put_pixel(x, y, image::Rgb(pixel));
    }
    image.save(path).wrap_err_with(|| {
        format!(
            "failed to write opaque reference fixture {}",
            path.display()
        )
    })?;
    Ok(())
}

fn compare_rust_preprocessing_against_reference(path: &Path, label: &str) -> eyre::Result<()> {
    let rust = crate::cli::image::upscale::summarize_image_upscale_input(path)?;
    let reference = crate::waifu2x_reference::read_waifu2x_reference_tensor_report(path, None)?;
    ensure!(
        reference.ok,
        "waifu2x reference tensor report reported failure"
    );
    ensure!(
        rust.pil_size == reference.pil_size,
        "{label} Rust preprocessing size {:?} did not match Python reference {:?}",
        rust.pil_size,
        reference.pil_size
    );
    ensure!(
        rust.blank_alpha == reference.blank_alpha,
        "{label} Rust blank_alpha {} did not match Python reference {}",
        rust.blank_alpha,
        reference.blank_alpha
    );
    ensure_tensor_summary_matches(label, "rgb", &rust.rgb, &reference.tensors.rgb)?;
    match (&rust.alpha, &reference.tensors.alpha) {
        (Some(rust_alpha), Some(reference_alpha)) => {
            ensure_tensor_summary_matches(label, "alpha", rust_alpha, reference_alpha)?;
        }
        (None, None) => {}
        _ => {
            return Err(eyre::eyre!(
                "{label} Rust alpha presence {:?} did not match Python reference {:?}",
                rust.alpha.as_ref().map(|summary| &summary.shape),
                reference
                    .tensors
                    .alpha
                    .as_ref()
                    .map(|summary| &summary.shape)
            ));
        }
    }
    println!(
        "waifu2x preprocessing parity: {label} size {:?} blank_alpha {} RGB {:?} alpha {:?}",
        rust.pil_size,
        rust.blank_alpha,
        rust.rgb.shape,
        rust.alpha.as_ref().map(|summary| summary.shape.clone())
    );
    Ok(())
}

fn ensure_tensor_summary_matches(
    label: &str,
    tensor_name: &str,
    rust: &crate::cli::image::upscale::ImageUpscaleTensorSummary,
    reference: &crate::waifu2x_reference::Waifu2xReferenceTensorSummary,
) -> eyre::Result<()> {
    ensure!(
        rust.shape == reference.shape,
        "{label} {tensor_name} shape {:?} did not match Python reference {:?}",
        rust.shape,
        reference.shape
    );
    ensure_close(label, tensor_name, "min", rust.min, reference.min)?;
    ensure_close(label, tensor_name, "max", rust.max, reference.max)?;
    ensure_close(label, tensor_name, "mean", rust.mean, reference.mean)?;
    ensure_close(label, tensor_name, "sum", rust.sum, reference.sum)?;
    Ok(())
}

fn ensure_close(
    label: &str,
    tensor_name: &str,
    field_name: &str,
    rust: f32,
    reference: f32,
) -> eyre::Result<()> {
    let delta = (rust - reference).abs();
    ensure!(
        delta <= 1e-6,
        "{label} {tensor_name} {field_name} Rust value {} differed from Python reference {} by {}",
        rust,
        reference,
        delta
    );
    Ok(())
}

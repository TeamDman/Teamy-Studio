use std::path::{Path, PathBuf};
use std::sync::Arc;

use eyre::Context;
use facet::Facet;
use image::{Rgba, RgbaImage};

use super::spatial::TerminalCellPoint;
use super::windows_d3d12_renderer::{
    self, RenderFrameModel, RendererTerminalVisualState, WindowChromeButtonsState,
};
use super::windows_terminal::{
    self, TerminalDisplayCursor, TerminalDisplayCursorStyle, TerminalDisplayRow,
};

const DEFAULT_RENDER_FIXTURE_ID: &str = "basic-terminal-frame";

struct BuiltInRenderFixture {
    id: &'static str,
    description: &'static str,
    build_frame: fn() -> RenderFrameModel,
}

const BUILT_IN_RENDER_FIXTURES: &[BuiltInRenderFixture] = &[BuiltInRenderFixture {
    id: DEFAULT_RENDER_FIXTURE_ID,
    description: "Smoke terminal frame with diagnostic chrome, two rows of slug text, a cursor, and a scrollbar.",
    build_frame: build_basic_terminal_frame_fixture,
}];

#[derive(Debug, Facet)]
pub struct RenderOffscreenFixtureSummary {
    fixture: String,
    description: String,
    expected_image_path: String,
    has_expected_image: bool,
    expected_scene_snapshot_path: String,
    has_expected_scene_snapshot: bool,
}

#[derive(Debug, Facet)]
pub struct RenderOffscreenFixtureListReport {
    default_fixture: String,
    fixtures: Vec<RenderOffscreenFixtureSummary>,
}

#[derive(Debug, Facet)]
pub struct RenderOffscreenSelfTestReport {
    fixture: String,
    backend: String,
    expected_image_path: String,
    expected_scene_snapshot_path: String,
    artifact_path: Option<String>,
    diff_path: Option<String>,
    scene_snapshot_artifact_path: Option<String>,
    image_width: u32,
    image_height: u32,
    non_transparent_pixels: usize,
    bright_pixels: usize,
    pixel_hash: String,
    scene_snapshot_hash: String,
    scene_snapshot_lines: usize,
    matched_expected: bool,
    matched_scene_snapshot: bool,
    updated_expected: bool,
}

struct RenderFixturePaths {
    expected_image: PathBuf,
    expected_scene_snapshot: PathBuf,
}

struct ExpectedRenderOutputs {
    image: Option<RgbaImage>,
    scene_snapshot: Option<String>,
}

struct OutputComparison {
    matched_expected: bool,
    matched_scene_snapshot: bool,
}

struct ArtifactWrites {
    png: Option<String>,
    diff: Option<String>,
    scene_snapshot: Option<String>,
}

/// cli[impl self-test.render-offscreen.list-fixtures-flag]
#[must_use]
pub fn list_render_offscreen_fixtures() -> RenderOffscreenFixtureListReport {
    RenderOffscreenFixtureListReport {
        default_fixture: DEFAULT_RENDER_FIXTURE_ID.to_owned(),
        fixtures: BUILT_IN_RENDER_FIXTURES
            .iter()
            .map(|fixture| {
                let expected_image_path = expected_image_path(fixture.id);
                let expected_scene_snapshot_path = expected_scene_snapshot_path(fixture.id);
                RenderOffscreenFixtureSummary {
                    fixture: fixture.id.to_owned(),
                    description: fixture.description.to_owned(),
                    expected_image_path: expected_image_path.display().to_string(),
                    has_expected_image: expected_image_path.exists(),
                    expected_scene_snapshot_path: expected_scene_snapshot_path
                        .display()
                        .to_string(),
                    has_expected_scene_snapshot: expected_scene_snapshot_path.exists(),
                }
            })
            .collect(),
    }
}

/// cli[impl self-test.render-offscreen.fixture-flag]
/// cli[impl self-test.render-offscreen.update-expected-flag]
/// cli[impl self-test.render-offscreen.artifact-output]
/// os[impl os.windows.rendering.direct3d12.offscreen-terminal-verification]
pub fn run_render_offscreen_fixture(
    fixture: Option<&str>,
    artifact_output: Option<&Path>,
    update_expected: bool,
) -> eyre::Result<RenderOffscreenSelfTestReport> {
    let fixture = resolve_fixture(fixture)?;
    let frame = (fixture.build_frame)();
    let image = windows_d3d12_renderer::render_frame_model_offscreen_image(&frame)?;
    let scene_snapshot = windows_d3d12_renderer::render_frame_model_scene_snapshot(&frame);
    let (non_transparent_pixels, bright_pixels) = summarize_offscreen_image(&image);
    if non_transparent_pixels == 0 || bright_pixels == 0 {
        eyre::bail!(
            "render fixture `{}` produced an empty or fully dark image",
            fixture.id
        )
    }

    let paths = render_fixture_paths(fixture.id);
    if update_expected {
        update_expected_outputs(fixture.id, &paths, &image, &scene_snapshot)?;
    }

    let expected = load_expected_outputs(fixture.id, &paths)?;
    let comparison = compare_expected_outputs(&expected, &image, &scene_snapshot);
    let artifacts = write_actual_outputs(
        fixture.id,
        artifact_output,
        &comparison,
        expected.image.as_ref(),
        &image,
        &scene_snapshot,
    )?;
    let failure_reasons = collect_failure_reasons(
        &paths,
        &expected,
        &comparison,
        &artifacts,
        update_expected,
        &scene_snapshot,
    );

    let report = RenderOffscreenSelfTestReport {
        fixture: fixture.id.to_owned(),
        backend: windows_d3d12_renderer::offscreen_render_backend_name().to_owned(),
        expected_image_path: paths.expected_image.display().to_string(),
        expected_scene_snapshot_path: paths.expected_scene_snapshot.display().to_string(),
        artifact_path: artifacts.png,
        diff_path: artifacts.diff,
        scene_snapshot_artifact_path: artifacts.scene_snapshot,
        image_width: image.width(),
        image_height: image.height(),
        non_transparent_pixels,
        bright_pixels,
        pixel_hash: pixel_hash(&image),
        scene_snapshot_hash: text_hash(&scene_snapshot),
        scene_snapshot_lines: scene_snapshot.lines().count(),
        matched_expected: comparison.matched_expected || update_expected,
        matched_scene_snapshot: comparison.matched_scene_snapshot || update_expected,
        updated_expected: update_expected,
    };

    if !update_expected && !failure_reasons.is_empty() {
        eyre::bail!(
            "render fixture `{}` mismatched expected outputs: {}",
            fixture.id,
            failure_reasons.join("; ")
        )
    }

    Ok(report)
}

fn render_fixture_paths(fixture: &str) -> RenderFixturePaths {
    RenderFixturePaths {
        expected_image: expected_image_path(fixture),
        expected_scene_snapshot: expected_scene_snapshot_path(fixture),
    }
}

fn update_expected_outputs(
    fixture: &str,
    paths: &RenderFixturePaths,
    image: &RgbaImage,
    scene_snapshot: &str,
) -> eyre::Result<()> {
    write_png(&paths.expected_image, image).wrap_err_with(|| {
        format!("failed to update expected render image for fixture `{fixture}`")
    })?;
    write_text(&paths.expected_scene_snapshot, scene_snapshot).wrap_err_with(|| {
        format!("failed to update expected scene snapshot for fixture `{fixture}`")
    })?;
    Ok(())
}

fn load_expected_outputs(
    fixture: &str,
    paths: &RenderFixturePaths,
) -> eyre::Result<ExpectedRenderOutputs> {
    let image = if paths.expected_image.exists() {
        Some(
            load_expected_image(&paths.expected_image).wrap_err_with(|| {
                format!("failed to load expected render image for fixture `{fixture}`")
            })?,
        )
    } else {
        None
    };

    let scene_snapshot = if paths.expected_scene_snapshot.exists() {
        Some(load_text(&paths.expected_scene_snapshot).wrap_err_with(|| {
            format!("failed to load expected scene snapshot for fixture `{fixture}`")
        })?)
    } else {
        None
    };

    Ok(ExpectedRenderOutputs {
        image,
        scene_snapshot,
    })
}

fn compare_expected_outputs(
    expected: &ExpectedRenderOutputs,
    image: &RgbaImage,
    scene_snapshot: &str,
) -> OutputComparison {
    OutputComparison {
        matched_expected: expected
            .image
            .as_ref()
            .is_some_and(|expected| images_match(expected, image)),
        matched_scene_snapshot: expected
            .scene_snapshot
            .as_deref()
            .is_some_and(|expected| texts_match(expected, scene_snapshot)),
    }
}

fn write_actual_outputs(
    fixture: &str,
    artifact_output: Option<&Path>,
    comparison: &OutputComparison,
    expected_image: Option<&RgbaImage>,
    image: &RgbaImage,
    scene_snapshot: &str,
) -> eyre::Result<ArtifactWrites> {
    let actual_artifact_path = artifact_output
        .map(Path::to_path_buf)
        .or_else(|| (!comparison.matched_expected).then(|| default_actual_artifact_path(fixture)));
    let artifact_path = write_png_artifact(fixture, actual_artifact_path.as_deref(), image)?;

    let actual_scene_snapshot_path = artifact_output
        .map(default_scene_snapshot_path_from_artifact)
        .or_else(|| {
            (!comparison.matched_scene_snapshot)
                .then(|| default_actual_scene_snapshot_artifact_path(fixture))
        });
    let scene_snapshot_artifact_path = write_scene_snapshot_artifact(
        fixture,
        actual_scene_snapshot_path.as_deref(),
        scene_snapshot,
    )?;

    let diff_path = write_diff_artifact(
        fixture,
        artifact_output,
        comparison.matched_expected,
        expected_image,
        image,
    )?;

    Ok(ArtifactWrites {
        png: artifact_path,
        diff: diff_path,
        scene_snapshot: scene_snapshot_artifact_path,
    })
}

fn write_png_artifact(
    fixture: &str,
    path: Option<&Path>,
    image: &RgbaImage,
) -> eyre::Result<Option<String>> {
    if let Some(path) = path {
        write_png(path, image).wrap_err_with(|| {
            format!("failed to write actual render artifact for fixture `{fixture}`")
        })?;
        return Ok(Some(path.display().to_string()));
    }
    Ok(None)
}

fn write_scene_snapshot_artifact(
    fixture: &str,
    path: Option<&Path>,
    scene_snapshot: &str,
) -> eyre::Result<Option<String>> {
    if let Some(path) = path {
        write_text(path, scene_snapshot).wrap_err_with(|| {
            format!("failed to write scene snapshot artifact for fixture `{fixture}`")
        })?;
        return Ok(Some(path.display().to_string()));
    }
    Ok(None)
}

fn write_diff_artifact(
    fixture: &str,
    artifact_output: Option<&Path>,
    matched_expected: bool,
    expected_image: Option<&RgbaImage>,
    image: &RgbaImage,
) -> eyre::Result<Option<String>> {
    let Some(expected_image) = expected_image else {
        return Ok(None);
    };
    if matched_expected {
        return Ok(None);
    }

    let path = artifact_output.map_or_else(
        || default_diff_artifact_path(fixture),
        default_diff_path_from_artifact,
    );
    let diff = build_diff_image(expected_image, image);
    write_png(&path, &diff).wrap_err_with(|| {
        format!("failed to write render diff artifact for fixture `{fixture}`")
    })?;
    Ok(Some(path.display().to_string()))
}

fn collect_failure_reasons(
    paths: &RenderFixturePaths,
    expected: &ExpectedRenderOutputs,
    comparison: &OutputComparison,
    artifacts: &ArtifactWrites,
    update_expected: bool,
    scene_snapshot: &str,
) -> Vec<String> {
    if update_expected {
        return Vec::new();
    }

    let mut failure_reasons = Vec::new();
    append_image_failure_reason(paths, expected, comparison, artifacts, &mut failure_reasons);
    append_scene_snapshot_failure_reason(
        paths,
        expected,
        comparison,
        artifacts,
        scene_snapshot,
        &mut failure_reasons,
    );
    failure_reasons
}

fn append_image_failure_reason(
    paths: &RenderFixturePaths,
    expected: &ExpectedRenderOutputs,
    comparison: &OutputComparison,
    artifacts: &ArtifactWrites,
    failure_reasons: &mut Vec<String>,
) {
    if expected.image.is_none() {
        let actual_path_text = artifacts.png.as_deref().unwrap_or("<not written>");
        failure_reasons.push(format!(
            "missing expected image `{}`; actual=`{actual_path_text}`",
            paths.expected_image.display()
        ));
        return;
    }

    if !comparison.matched_expected {
        let actual_path_text = artifacts.png.as_deref().unwrap_or("<not written>");
        let diff_path_text = artifacts.diff.as_deref().unwrap_or("<not written>");
        failure_reasons.push(format!(
            "image `{}` actual=`{actual_path_text}` diff=`{diff_path_text}`",
            paths.expected_image.display()
        ));
    }
}

fn append_scene_snapshot_failure_reason(
    paths: &RenderFixturePaths,
    expected: &ExpectedRenderOutputs,
    comparison: &OutputComparison,
    artifacts: &ArtifactWrites,
    scene_snapshot: &str,
    failure_reasons: &mut Vec<String>,
) {
    if expected.scene_snapshot.is_none() {
        let actual_path_text = artifacts
            .scene_snapshot
            .as_deref()
            .unwrap_or("<not written>");
        failure_reasons.push(format!(
            "missing expected scene snapshot `{}`; actual=`{actual_path_text}`",
            paths.expected_scene_snapshot.display()
        ));
        return;
    }

    if !comparison.matched_scene_snapshot {
        let actual_path_text = artifacts
            .scene_snapshot
            .as_deref()
            .unwrap_or("<not written>");
        let mismatch_summary = expected
            .scene_snapshot
            .as_deref()
            .map_or_else(String::new, |expected| {
                snapshot_mismatch_summary(expected, scene_snapshot)
            });
        failure_reasons.push(format!(
            "scene snapshot `{}` actual=`{actual_path_text}` {mismatch_summary}",
            paths.expected_scene_snapshot.display()
        ));
    }
}

fn resolve_fixture(fixture: Option<&str>) -> eyre::Result<&'static BuiltInRenderFixture> {
    let requested = fixture.unwrap_or(DEFAULT_RENDER_FIXTURE_ID);
    BUILT_IN_RENDER_FIXTURES
        .iter()
        .find(|fixture| fixture.id.eq_ignore_ascii_case(requested))
        .ok_or_else(|| {
            let available = BUILT_IN_RENDER_FIXTURES
                .iter()
                .map(|fixture| fixture.id)
                .collect::<Vec<_>>()
                .join(", ");
            eyre::eyre!("unknown render fixture `{requested}`; available fixtures: {available}")
        })
}

fn expected_image_path(fixture: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("render-offscreen")
        .join(format!("{fixture}.png"))
}

fn expected_scene_snapshot_path(fixture: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("render-offscreen")
        .join(format!("{fixture}.scene.txt"))
}

fn default_actual_artifact_path(fixture: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("test-artifacts")
        .join("render-offscreen")
        .join(format!("{fixture}-actual.png"))
}

fn default_actual_scene_snapshot_artifact_path(fixture: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("test-artifacts")
        .join("render-offscreen")
        .join(format!("{fixture}-scene-actual.txt"))
}

fn default_diff_artifact_path(fixture: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("test-artifacts")
        .join("render-offscreen")
        .join(format!("{fixture}-diff.png"))
}

fn default_diff_path_from_artifact(path: &Path) -> PathBuf {
    let parent = path.parent().map_or_else(PathBuf::new, Path::to_path_buf);
    let stem = path.file_stem().map_or_else(
        || "render-offscreen".to_owned(),
        |stem| stem.to_string_lossy().into(),
    );
    parent.join(format!("{stem}-diff.png"))
}

fn default_scene_snapshot_path_from_artifact(path: &Path) -> PathBuf {
    let parent = path.parent().map_or_else(PathBuf::new, Path::to_path_buf);
    let stem = path.file_stem().map_or_else(
        || "render-offscreen".to_owned(),
        |stem| stem.to_string_lossy().into(),
    );
    parent.join(format!("{stem}-scene.txt"))
}

fn load_expected_image(path: &Path) -> eyre::Result<RgbaImage> {
    image::open(path)
        .wrap_err_with(|| format!("failed to decode expected render image {}", path.display()))
        .map(|image| image.to_rgba8())
}

fn load_text(path: &Path) -> eyre::Result<String> {
    std::fs::read_to_string(path)
        .wrap_err_with(|| format!("failed to read render text artifact {}", path.display()))
}

fn write_png(path: &Path, image: &RgbaImage) -> eyre::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).wrap_err_with(|| {
            format!(
                "failed to create render artifact directory {}",
                parent.display()
            )
        })?;
    }
    image
        .save(path)
        .wrap_err_with(|| format!("failed to write render artifact {}", path.display()))?;
    Ok(())
}

fn write_text(path: &Path, text: &str) -> eyre::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).wrap_err_with(|| {
            format!(
                "failed to create render artifact directory {}",
                parent.display()
            )
        })?;
    }
    std::fs::write(path, text)
        .wrap_err_with(|| format!("failed to write render artifact {}", path.display()))?;
    Ok(())
}

fn images_match(expected: &RgbaImage, actual: &RgbaImage) -> bool {
    expected.dimensions() == actual.dimensions() && expected.as_raw() == actual.as_raw()
}

fn texts_match(expected: &str, actual: &str) -> bool {
    expected == actual
}

fn build_diff_image(expected: &RgbaImage, actual: &RgbaImage) -> RgbaImage {
    let width = expected.width().max(actual.width());
    let height = expected.height().max(actual.height());
    let mut diff = RgbaImage::new(width, height);

    for y in 0..height {
        for x in 0..width {
            let expected_pixel = pixel_or_transparent(expected, x, y);
            let actual_pixel = pixel_or_transparent(actual, x, y);
            diff.put_pixel(
                x,
                y,
                Rgba([
                    expected_pixel[0].abs_diff(actual_pixel[0]),
                    expected_pixel[1].abs_diff(actual_pixel[1]),
                    expected_pixel[2].abs_diff(actual_pixel[2]),
                    expected_pixel[3].abs_diff(actual_pixel[3]).max(24),
                ]),
            );
        }
    }

    diff
}

fn pixel_or_transparent(image: &RgbaImage, x: u32, y: u32) -> Rgba<u8> {
    if x < image.width() && y < image.height() {
        *image.get_pixel(x, y)
    } else {
        Rgba([0, 0, 0, 0])
    }
}

fn summarize_offscreen_image(image: &RgbaImage) -> (usize, usize) {
    let non_transparent_pixels = image.pixels().filter(|pixel| pixel[3] > 0).count();
    let bright_pixels = image
        .pixels()
        .filter(|pixel| u16::from(pixel[0]) + u16::from(pixel[1]) + u16::from(pixel[2]) > 64)
        .count();
    (non_transparent_pixels, bright_pixels)
}

fn pixel_hash(image: &RgbaImage) -> String {
    format!("{:016x}", fnv1a64(image.as_raw()))
}

fn text_hash(text: &str) -> String {
    format!("{:016x}", fnv1a64(text.as_bytes()))
}

fn snapshot_mismatch_summary(expected: &str, actual: &str) -> String {
    let mut expected_lines = expected.lines();
    let mut actual_lines = actual.lines();
    let mut line_number = 1_usize;

    loop {
        match (expected_lines.next(), actual_lines.next()) {
            (Some(expected_line), Some(actual_line)) if expected_line == actual_line => {
                line_number += 1;
            }
            (Some(expected_line), Some(actual_line)) => {
                return format!(
                    "first mismatch at line {line_number}: expected `{}` actual `{}`",
                    summarize_snapshot_line(expected_line),
                    summarize_snapshot_line(actual_line)
                );
            }
            (Some(expected_line), None) => {
                return format!(
                    "actual snapshot ended at line {}; expected `{}`",
                    line_number.saturating_sub(1),
                    summarize_snapshot_line(expected_line)
                );
            }
            (None, Some(actual_line)) => {
                return format!(
                    "actual snapshot has extra line {line_number}: `{}`",
                    summarize_snapshot_line(actual_line)
                );
            }
            (None, None) => return "snapshots matched".to_owned(),
        }
    }
}

fn summarize_snapshot_line(line: &str) -> String {
    let summary = if line.chars().count() > 120 {
        format!("{}...", line.chars().take(117).collect::<String>())
    } else {
        line.to_owned()
    };
    summary.replace('`', "'")
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

fn build_basic_terminal_frame_fixture() -> RenderFrameModel {
    let layout = windows_terminal::TerminalLayout {
        client_width: 1040,
        client_height: 680,
        cell_width: 8,
        cell_height: 16,
        diagnostic_panel_visible: true,
    };
    let terminal_display = Arc::new(windows_terminal::TerminalDisplayState {
        rows: vec![
            build_terminal_row(0, "echo offscreen", [0.92, 0.94, 0.98, 1.0], true),
            build_terminal_row(1, "headless renderer", [0.96, 0.90, 0.70, 1.0], false),
        ],
        dirty_rows: vec![0, 1],
        cursor: Some(TerminalDisplayCursor {
            cell: TerminalCellPoint::new(8, 1),
            color: [0.96, 0.45, 1.0, 1.0],
            style: TerminalDisplayCursorStyle::Block,
        }),
        scrollbar: Some(windows_terminal::TerminalDisplayScrollbar {
            total: 100,
            offset: 40,
            visible: 24,
        }),
    });

    windows_d3d12_renderer::RenderFrameModel {
        layout,
        title: Some("self-test".to_owned()),
        diagnostic_text: "offscreen render self-test".to_owned(),
        diagnostic_selection: None,
        window_chrome_buttons_state: WindowChromeButtonsState::default(),
        diagnostic_cell_width: 8,
        diagnostic_cell_height: 16,
        scene: None,
        terminal_cell_width: 8,
        terminal_cell_height: 16,
        terminal_display,
        terminal_visual_state: RendererTerminalVisualState {
            track_hovered: true,
            thumb_hovered: true,
            thumb_grabbed: false,
        },
    }
}

fn build_terminal_row(
    row: i32,
    text: &str,
    color: [f32; 4],
    include_background: bool,
) -> TerminalDisplayRow {
    TerminalDisplayRow {
        row,
        backgrounds: if include_background {
            vec![windows_terminal::TerminalDisplayBackground {
                cell: TerminalCellPoint::new(0, row),
                color: [0.18, 0.18, 0.24, 1.0],
            }]
        } else {
            Vec::new()
        },
        glyphs: text
            .chars()
            .enumerate()
            .map(
                |(column, character)| windows_terminal::TerminalDisplayGlyph {
                    cell: TerminalCellPoint::new(i32::try_from(column).unwrap_or_default(), row),
                    character,
                    color,
                },
            )
            .collect(),
    }
}

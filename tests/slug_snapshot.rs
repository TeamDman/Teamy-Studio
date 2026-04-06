#![cfg(windows)]

use std::path::PathBuf;

#[test]
#[ignore = "writes a diagnostic artifact for manual inspection"]
fn snapshot_single_glyph_slash_png() -> eyre::Result<()> {
    let output_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("test-artifacts")
        .join("slug")
        .join("slash-256.png");

    teamy_studio::app::write_slug_snapshot_png('/', 256, 512, 512, &output_path)?;

    assert!(output_path.exists());

    let image = image::open(&output_path)?.into_rgba8();
    assert!(image.pixels().any(|pixel| pixel[3] > 0));
    Ok(())
}

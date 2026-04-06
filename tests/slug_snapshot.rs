#![cfg(windows)]

use std::path::PathBuf;

#[test]
#[ignore = "writes a diagnostic artifact for manual inspection"]
fn snapshot_glyph_diagnostics_pngs() -> eyre::Result<()> {
    let output_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("test-artifacts")
        .join("slug");
    let slash_path = output_dir.join("slash-256.png");
    let r_path = output_dir.join("r-256.png");
    let sheet_path = output_dir.join("unicode-sheet.png");
    let index_path = output_dir.join("unicode-sheet-index.txt");

    teamy_studio::app::write_slug_snapshot_png('/', 256, 512, 512, &slash_path)?;
    teamy_studio::app::write_slug_snapshot_png('r', 256, 512, 512, &r_path)?;
    teamy_studio::app::write_slug_snapshot_sheet_png(48, 64, 24, &sheet_path, &index_path)?;

    assert!(slash_path.exists());
    assert!(r_path.exists());
    assert!(sheet_path.exists());
    assert!(index_path.exists());

    let image = image::open(&slash_path)?.into_rgba8();
    assert!(image.pixels().any(|pixel| pixel[3] > 0));

    let sheet = image::open(&sheet_path)?.into_rgba8();
    assert!(sheet.pixels().any(|pixel| pixel[3] > 0 && pixel[0] > 0));
    Ok(())
}

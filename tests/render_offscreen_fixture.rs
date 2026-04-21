#![cfg(windows)]

#[test]
fn built_in_render_fixture_matches_expected_output() -> eyre::Result<()> {
    let _ = teamy_studio::app::run_render_offscreen_verification_fixture(
        Some("basic-terminal-frame"),
        None,
        false,
    )?;
    Ok(())
}

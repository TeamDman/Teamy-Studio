#![deny(clippy::disallowed_methods)]
#![deny(clippy::disallowed_macros)]

#[cfg(feature = "mvp")]
mod startup;

#[cfg(feature = "mvp")]
use eyre::Result;
#[cfg(feature = "mvp")]
pub use startup::{
    AppComposition, StartupSession, build_mvp_composition, build_mvp_composition_with_native_shell,
    build_mvp_session, build_mvp_session_with_native_shell,
};
#[cfg(feature = "mvp")]
use teamy_studio_launcher_catalog as _;
#[cfg(feature = "mvp")]
use teamy_studio_timeline_core::CanonicalTimeKey;

#[cfg(feature = "mvp")]
/// Compose the MVP feature stack and publish the initial event chain.
///
/// # Errors
///
/// Returns an error if registration validation fails or if the MVP composition
/// cannot produce the initial cursor-gallery open flow.
pub fn main() -> Result<()> {
    teamy_studio_shell::initialize_dpi_awareness();

    let mut session = startup::build_mvp_session()?;
    let menu_snapshot = session.menu_snapshot().clone();
    let mut next_time_key = 0_i128;

    teamy_studio_main_menu::run_native_main_menu_window_with_click_handler(
        &menu_snapshot,
        |logical_button_id| {
            let clicked_class_id = menu_snapshot
                .buttons()
                .iter()
                .find(|button| button.logical_button_id == logical_button_id)
                .map(|button| button.class_id);
            session.publish_main_menu_click(
                logical_button_id,
                0,
                0,
                1,
                CanonicalTimeKey(next_time_key),
            )?;
            next_time_key += 16;
            let _ = session.pump_to_idle(CanonicalTimeKey(next_time_key), 8)?;
            next_time_key += 16;
            if clicked_class_id == Some(teamy_studio_cursor_gallery::CURSOR_GALLERY_BUTTON_CLASS_ID)
            {
                teamy_studio_cursor_gallery::open_native_cursor_gallery_window()?;
            }
            Ok(())
        },
    )
}

#[cfg(not(feature = "mvp"))]
/// Entrypoint for configurations where the MVP feature stack is disabled.
///
/// # Errors
///
/// Always returns an error because the thin root currently requires the MVP
/// composition feature to be enabled.
pub fn main() -> eyre::Result<()> {
    Err(eyre::eyre!(
        "teamy-studio currently requires the `mvp` feature to run"
    ))
}

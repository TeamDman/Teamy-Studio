use eyre::WrapErr;
use tracing::info;
use windows::Win32::Foundation::HWND;

use crate::{RenderScene, TextRendererHost};

pub fn create_text_renderer_host_for_scene(
    hwnd: HWND,
    scene: &RenderScene,
) -> eyre::Result<TextRendererHost> {
    let mut host = TextRendererHost::new(hwnd, scene)
        .wrap_err("failed to create reusable text renderer host for the main menu")?;
    host.present_scene_frame([0.0, 0.0, 0.0, 0.0])
        .wrap_err("failed to present initial scene frame for the main menu")?;

    info!(
        hwnd = ?hwnd,
        vertices = host.last_upload_batch.vertices.len(),
        curves = host.last_upload_batch.curve_upload_data.len(),
        bands = host.last_upload_batch.band_upload_data.len(),
        presented = 1,
        "D3D12 text renderer host ready"
    );

    Ok(host)
}

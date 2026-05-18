use std::ffi::OsStr;

use eyre::WrapErr;
use windows::Win32::Foundation::HWND;

use crate::{RenderScene, TextRendererHost};

const TEAMY_STUDIO_DISABLE_D3D12_HYDRATION_ENV: &str = "TEAMY_STUDIO_DISABLE_D3D12_HYDRATION";

pub type TextRendererSmokeBootstrap = TextRendererHost;

#[must_use]
pub fn d3d12_smoke_test_requested() -> bool {
    !env_var_truthy(std::env::var_os(TEAMY_STUDIO_DISABLE_D3D12_HYDRATION_ENV).as_deref())
}

pub fn smoke_bootstrap_text_renderer_for_scene(
    hwnd: HWND,
    scene: &RenderScene,
) -> eyre::Result<TextRendererSmokeBootstrap> {
    let mut host = TextRendererHost::new(hwnd, scene)
        .wrap_err("failed to create reusable text renderer host for D3D12 smoke bootstrap")?;
    host.present_scene_frame([0.0, 0.0, 0.0, 0.0])
        .wrap_err("failed to present initial scene frame for D3D12 smoke bootstrap")?;

    eprintln!(
        "D3D12 smoke bootstrap ready: hwnd={:?} vertices={} curves={} bands={} presented=1",
        hwnd,
        host.last_upload_batch.vertices.len(),
        host.last_upload_batch.curve_upload_data.len(),
        host.last_upload_batch.band_upload_data.len(),
    );

    Ok(host)
}

fn env_var_truthy(value: Option<&OsStr>) -> bool {
    value.is_some_and(|value| !value.is_empty() && value != OsStr::new("0"))
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use super::{d3d12_smoke_test_requested, env_var_truthy, TEAMY_STUDIO_DISABLE_D3D12_HYDRATION_ENV};

    #[test]
    fn env_var_truthy_treats_zero_as_disabled() {
        assert!(!env_var_truthy(None));
        assert!(!env_var_truthy(Some(OsStr::new(""))));
        assert!(!env_var_truthy(Some(OsStr::new("0"))));
        assert!(env_var_truthy(Some(OsStr::new("1"))));
    }

    #[test]
    fn d3d12_hydration_defaults_on_unless_explicitly_disabled() {
        unsafe {
            std::env::remove_var(TEAMY_STUDIO_DISABLE_D3D12_HYDRATION_ENV);
        }
        assert!(d3d12_smoke_test_requested());

        unsafe {
            std::env::set_var(TEAMY_STUDIO_DISABLE_D3D12_HYDRATION_ENV, "1");
        }
        assert!(!d3d12_smoke_test_requested());

        unsafe {
            std::env::set_var(TEAMY_STUDIO_DISABLE_D3D12_HYDRATION_ENV, "0");
        }
        assert!(d3d12_smoke_test_requested());

        unsafe {
            std::env::remove_var(TEAMY_STUDIO_DISABLE_D3D12_HYDRATION_ENV);
        }
    }
}
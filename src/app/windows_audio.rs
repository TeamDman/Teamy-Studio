#[expect(
    unused_imports,
    reason = "compatibility shim preserves crate::app::windows_audio while implementation lives in teamy_studio_windows_audio"
)]
pub use teamy_studio_windows_audio::*;

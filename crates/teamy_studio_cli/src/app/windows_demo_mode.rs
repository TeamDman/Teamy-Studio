#[expect(
    unused_imports,
    reason = "compatibility shim preserves crate::app::windows_demo_mode while implementation lives in teamy_studio_demo_mode"
)]
pub use teamy_studio_demo_mode::*;

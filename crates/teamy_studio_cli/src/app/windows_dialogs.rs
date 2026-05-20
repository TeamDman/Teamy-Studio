#[expect(
    unused_imports,
    reason = "compatibility shim preserves crate::app::windows_dialogs while implementation lives in teamy_studio_windows_dialogs"
)]
pub use teamy_studio_windows_dialogs::*;

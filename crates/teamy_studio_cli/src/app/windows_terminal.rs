#[expect(
    unused_imports,
    reason = "compatibility shim preserves crate::app::windows_terminal while implementation lives in teamy_studio_terminal_core"
)]
pub use teamy_studio_terminal_core::*;

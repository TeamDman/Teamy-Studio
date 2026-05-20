#[expect(
    unused_imports,
    reason = "compatibility shim preserves crate::app::windows_scene while implementation lives in teamy_studio_shell"
)]
pub use teamy_studio_shell::windows_scene::*;

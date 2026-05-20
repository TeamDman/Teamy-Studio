#[expect(
    unused_imports,
    reason = "compatibility shim preserves crate::app::cell_grid while implementation lives in teamy_studio_shell"
)]
pub use teamy_studio_shell::cell_grid::*;

#[expect(
    unused_imports,
    reason = "compatibility shim preserves crate::app::vt_types while implementation lives in teamy_studio_vt_types"
)]
pub use teamy_studio_vt_types::*;

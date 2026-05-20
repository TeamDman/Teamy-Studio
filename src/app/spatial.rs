#[expect(
    unused_imports,
    reason = "compatibility shim preserves crate::app::spatial while implementation lives in teamy_studio_spatial"
)]
pub use teamy_studio_spatial::*;

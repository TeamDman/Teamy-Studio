pub mod paths {
    pub use teamy_studio_paths::*;
}

pub mod model {
    use std::path::PathBuf;

    use teamy_studio_paths::CacheHome;

    pub const MANAGED_MODELS_DIR_NAME: &str = "models";

    #[must_use]
    pub fn managed_models_dir(cache_home: &CacheHome) -> PathBuf {
        cache_home.0.join(MANAGED_MODELS_DIR_NAME)
    }
}

mod image_model_impl;

pub use image_model_impl::*;

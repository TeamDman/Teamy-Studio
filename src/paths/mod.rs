mod app_home;
mod cache;
mod workspace;

pub use app_home::*;
pub use cache::*;
pub use workspace::*;

pub const APP_HOME_ENV_VAR: &str = "TEAMY_STUDIO_HOME_DIR";
pub const APP_HOME_DIR_NAME: &str = "Teamy-Studio";

pub const APP_CACHE_ENV_VAR: &str = "TEAMY_STUDIO_CACHE_DIR";
pub const APP_CACHE_DIR_NAME: &str = "Teamy-Studio";

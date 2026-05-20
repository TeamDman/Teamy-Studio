use crate::cli::global_args::GlobalArgs;
use teamy_studio_observability::LoggingConfig;

/// Initialize logging based on the provided configuration.
/// tool[impl logging.stderr-output]
/// tool[impl logging.file-path-option]
/// tool[impl logging.file-structured-ndjson]
/// tool[impl logging.filter.from-env]
/// tool[impl logging.filter.defaults]
/// tool[impl logging.filter.debug-conflicts-with-log-filter]
///
/// # Errors
///
/// This function will return an error if creating the log file or directories fails.
///
/// # Panics
///
/// This function may panic if locking or cloning the log file handle fails.
pub fn init_logging(global_args: &GlobalArgs) -> eyre::Result<()> {
    let config = LoggingConfig {
        debug: global_args.debug,
        log_filter: global_args.log_filter.clone(),
        log_file: global_args.log_file.clone(),
    };

    teamy_studio_observability::init_logging(&config)
}

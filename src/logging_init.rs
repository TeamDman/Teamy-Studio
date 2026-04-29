use crate::cli::global_args::GlobalArgs;
use chrono::{DateTime, Local};
use eyre::bail;
use std::fs::File;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::Mutex;
#[cfg(feature = "tracy")]
use tracing::Metadata;
use tracing::debug;
use tracing::level_filters::LevelFilter;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::Registry;
#[cfg(feature = "tracy")]
use tracing_subscriber::filter::FilterFn;
use tracing_subscriber::fmt::writer::BoxMakeWriter;
use tracing_subscriber::prelude::*;
use tracing_subscriber::util::SubscriberInitExt;

#[cfg(feature = "tracy")]
fn exclude_tracy_frame_mark(meta: &Metadata<'_>) -> bool {
    meta.fields().field("tracy.frame_mark").is_none()
}

#[derive(Debug, PartialEq, Eq)]
enum LogFilterSelection {
    Explicit(String),
    FromEnv(String),
    Default(LevelFilter),
}

fn select_log_filter(
    global_args: &GlobalArgs,
    rust_log: Option<&str>,
) -> eyre::Result<LogFilterSelection> {
    match (global_args.debug, global_args.log_filter.as_deref()) {
        (true, Some(_)) => bail!("cannot specify log filter with --debug"),
        (false, Some(filter)) => Ok(LogFilterSelection::Explicit(filter.to_owned())),
        (_, None) => match rust_log {
            Some(filter) => Ok(LogFilterSelection::FromEnv(filter.to_owned())),
            None => Ok(LogFilterSelection::Default(if global_args.debug {
                LevelFilter::DEBUG
            } else {
                LevelFilter::INFO
            })),
        },
    }
}

fn build_env_filter(global_args: &GlobalArgs, rust_log: Option<&str>) -> eyre::Result<EnvFilter> {
    let builder = EnvFilter::builder();
    match select_log_filter(global_args, rust_log)? {
        LogFilterSelection::Explicit(filter) => Ok(builder
            .with_default_directive(LevelFilter::from_str(&filter)?.into())
            .parse("")?),
        LogFilterSelection::FromEnv(filter) => Ok(builder.parse(filter)?),
        LogFilterSelection::Default(level) => {
            Ok(builder.with_default_directive(level.into()).parse("")?)
        }
    }
}

fn resolve_json_log_path(log_file: Option<&str>, now: DateTime<Local>) -> Option<PathBuf> {
    match log_file {
        None => None,
        Some(path) if PathBuf::from(path).is_dir() => {
            let timestamp = now.format("%Y-%m-%d_%H-%M-%S");
            let filename = format!("log_{timestamp}.ndjson");
            Some(PathBuf::from(path).join(filename))
        }
        Some(path) => Some(PathBuf::from(path)),
    }
}

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
    let subscriber = Registry::default();

    let rust_log = if global_args.log_filter.is_none() {
        std::env::var("RUST_LOG").ok()
    } else {
        None
    };
    let env_filter_layer = build_env_filter(global_args, rust_log.as_deref())?;
    let subscriber = subscriber.with(env_filter_layer);

    let stderr_layer = if global_args.debug {
        tracing_subscriber::fmt::layer()
            .with_file(cfg!(debug_assertions))
            .with_line_number(cfg!(debug_assertions))
            .with_target(true)
            .with_writer(std::io::stderr)
            .pretty()
            .with_timer(tracing_subscriber::fmt::time::uptime())
            .boxed()
    } else {
        tracing_subscriber::fmt::layer()
            .with_file(cfg!(debug_assertions))
            .with_line_number(cfg!(debug_assertions))
            .with_target(true)
            .with_writer(std::io::stderr)
            .pretty()
            .without_time()
            .boxed()
    };
    #[cfg(feature = "tracy")]
    let stderr_layer = stderr_layer.with_filter(FilterFn::new(exclude_tracy_frame_mark));
    let subscriber = subscriber.with(stderr_layer);

    let json_log_path = resolve_json_log_path(global_args.log_file.as_deref(), Local::now());
    let json_layer = if let Some(ref json_log_path) = json_log_path {
        if let Some(parent) = json_log_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = File::create(json_log_path)?;
        let file = Arc::new(Mutex::new(file));
        let json_writer = BoxMakeWriter::new(move || {
            file.lock()
                .expect("failed to lock json log file")
                .try_clone()
                .expect("failed to clone json log file handle")
        });

        let json_layer = tracing_subscriber::fmt::layer()
            .event_format(tracing_subscriber::fmt::format().json())
            .with_file(true)
            .with_target(false)
            .with_line_number(true)
            .with_writer(json_writer);
        #[cfg(feature = "tracy")]
        let json_layer = json_layer.with_filter(FilterFn::new(exclude_tracy_frame_mark));
        Some(json_layer)
    } else {
        None
    };
    let subscriber = subscriber.with(json_layer);

    let subscriber = subscriber.with(crate::logs::LogCollectorLayer);

    #[cfg(all(feature = "tracy", not(test)))]
    let subscriber = subscriber.with(tracing_tracy::TracyLayer::default());

    if let Err(error) = subscriber.try_init() {
        eprintln!(
            "Failed to initialize tracing subscriber - are you running `cargo test`? If so, multiple test entrypoints may be running from the same process. https://github.com/tokio-rs/console/issues/505 : {error}"
        );
        return Ok(());
    }

    #[cfg(all(feature = "tracy", not(test)))]
    tracing::info!(
        "Tracy profiling layer added, memory usage will increase until a client is connected"
    );

    debug!(
        ?json_log_path,
        debug = global_args.debug,
        "Tracing initialized"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{LogFilterSelection, resolve_json_log_path, select_log_filter};
    use crate::cli::global_args::GlobalArgs;

    fn test_global_args() -> GlobalArgs {
        GlobalArgs::default()
    }

    // tool[verify logging.filter.debug-conflicts-with-log-filter]
    #[test]
    fn debug_conflicts_with_explicit_log_filter() {
        let args = GlobalArgs {
            debug: true,
            log_filter: Some("trace".to_owned()),
            ..test_global_args()
        };

        let error = select_log_filter(&args, None).expect_err("debug plus log-filter should fail");
        assert!(
            error
                .to_string()
                .contains("cannot specify log filter with --debug")
        );
    }

    // tool[verify logging.filter.from-env]
    #[test]
    fn rust_log_is_used_when_explicit_filter_is_omitted() {
        let selection = select_log_filter(&test_global_args(), Some("warn"))
            .expect("RUST_LOG should be accepted when --log-filter is omitted");

        assert_eq!(selection, LogFilterSelection::FromEnv("warn".to_owned()));
    }

    // tool[verify logging.filter.defaults]
    #[test]
    fn debug_defaults_to_debug_filter_when_no_filter_is_provided() {
        let args = GlobalArgs {
            debug: true,
            ..test_global_args()
        };

        let selection =
            select_log_filter(&args, None).expect("debug default filter should resolve");

        assert_eq!(
            selection,
            LogFilterSelection::Default(tracing::level_filters::LevelFilter::DEBUG)
        );
    }

    // tool[verify logging.filter.defaults]
    #[test]
    fn non_debug_defaults_to_info_filter_when_no_filter_is_provided() {
        let selection = select_log_filter(&test_global_args(), None)
            .expect("non-debug default filter should resolve");

        assert_eq!(
            selection,
            LogFilterSelection::Default(tracing::level_filters::LevelFilter::INFO)
        );
    }

    // tool[verify logging.file-path-option]
    #[test]
    fn explicit_log_file_path_is_preserved() {
        let path = std::path::Path::new("logs").join("teamy.ndjson");
        let resolved = resolve_json_log_path(Some(&path.to_string_lossy()), chrono::Local::now())
            .expect("explicit log file path should resolve");

        assert_eq!(resolved, path);
    }

    // tool[verify logging.file-path-option]
    #[test]
    fn directory_log_file_path_gets_timestamped_ndjson_filename() {
        let dir = tempfile::tempdir().expect("temporary log directory should be created");

        let resolved =
            resolve_json_log_path(Some(&dir.path().to_string_lossy()), chrono::Local::now())
                .expect("directory log path should resolve to a file inside the directory");

        assert_eq!(resolved.parent(), Some(dir.path()));
        assert!(
            resolved
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("log_") && name.ends_with(".ndjson"))
        );
    }
}

use chrono::{DateTime, Local};
use eyre::{Result, eyre};
use facet::Facet;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::fs::File;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
#[cfg(feature = "tracy")]
use tracing::Metadata;
use tracing::field::{Field, Visit};
use tracing::level_filters::LevelFilter;
use tracing::{Event, Subscriber};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::Layer;
#[cfg(feature = "tracy")]
use tracing_subscriber::filter::FilterFn;
use tracing_subscriber::fmt::writer::BoxMakeWriter;
use tracing_subscriber::layer::Context;
use tracing_subscriber::prelude::*;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{Registry, fmt};

const TIMELINE_REEMIT_FIELD_NAME: &str = "teamy.timeline_reemit";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RawProcessStartupInputs {
    pub argv: Vec<String>,
    pub rust_log: Option<String>,
}

#[derive(Clone, Debug, Eq, Facet, PartialEq)]
pub struct ProcessStartupObservedEvent {
    pub argv: Vec<String>,
    pub rust_log: Option<String>,
}

impl RawProcessStartupInputs {
    #[must_use]
    pub fn capture_from_env() -> Self {
        Self {
            argv: std::env::args_os()
                .skip(1)
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect(),
            rust_log: std::env::var("RUST_LOG").ok(),
        }
    }

    #[must_use]
    pub fn argv_refs(&self) -> Vec<&str> {
        self.argv.iter().map(String::as_str).collect()
    }

    #[must_use]
    pub fn to_observed_event(&self) -> ProcessStartupObservedEvent {
        ProcessStartupObservedEvent {
            argv: self.argv.clone(),
            rust_log: self.rust_log.clone(),
        }
    }
}

#[derive(Clone, Debug, Default, Eq, Facet, PartialEq)]
pub struct GlobalArgs {
    pub debug: bool,

    pub log_filter: Option<String>,

    pub log_file: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, Facet, PartialEq)]
pub struct StartupBootstrapCli {
    pub global_args: GlobalArgs,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BootstrapCliParseError {
    MissingValue { flag: &'static str },
    UnrecognizedArgument { argument: String },
    HelpRequested { text: String },
    VersionRequested { text: String },
    CompletionsRequested { script: String },
}

impl Display for BootstrapCliParseError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingValue { flag } => write!(formatter, "{flag} requires a value"),
            Self::UnrecognizedArgument { argument } => {
                write!(formatter, "unrecognized startup argument: {argument}")
            }
            Self::HelpRequested { text } | Self::VersionRequested { text } => {
                write!(formatter, "{text}")
            }
            Self::CompletionsRequested { script } => write!(formatter, "{script}"),
        }
    }
}

impl Error for BootstrapCliParseError {}

impl BootstrapCliParseError {
    #[must_use]
    pub const fn is_builtin_request(&self) -> bool {
        matches!(
            self,
            Self::HelpRequested { .. }
                | Self::VersionRequested { .. }
                | Self::CompletionsRequested { .. }
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StartupLoggingPlanError {
    DebugConflictsWithExplicitFilter { filter: String },
}

impl Display for StartupLoggingPlanError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DebugConflictsWithExplicitFilter { .. } => {
                write!(formatter, "cannot specify log filter with --debug")
            }
        }
    }
}

impl Error for StartupLoggingPlanError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BootstrapPlanError {
    CliParse(BootstrapCliParseError),
    LoggingPlan(StartupLoggingPlanError),
}

impl Display for BootstrapPlanError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CliParse(error) => Display::fmt(error, formatter),
            Self::LoggingPlan(error) => Display::fmt(error, formatter),
        }
    }
}

impl Error for BootstrapPlanError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::CliParse(error) => Some(error),
            Self::LoggingPlan(error) => Some(error),
        }
    }
}

#[derive(Clone, Debug, Eq, Facet, PartialEq)]
pub struct StartupGlobalArgsParsedEvent {
    pub debug: bool,
    pub log_filter: Option<String>,
    pub log_file: Option<String>,
}

impl GlobalArgs {
    #[must_use]
    pub fn to_parsed_event(&self) -> StartupGlobalArgsParsedEvent {
        StartupGlobalArgsParsedEvent {
            debug: self.debug,
            log_filter: self.log_filter.clone(),
            log_file: self.log_file.clone(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LogFilterSelection {
    Explicit(String),
    FromEnv(String),
    Default(LevelFilter),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StartupLoggingPlan {
    pub debug: bool,
    pub filter_selection: LogFilterSelection,
    pub json_log_path: Option<PathBuf>,
}

#[derive(Clone, Debug, Eq, Facet, PartialEq)]
pub struct StartupLoggingConfiguredEvent {
    pub debug: bool,
    pub effective_filter_directive: String,
    pub json_log_path: Option<String>,
}

#[derive(Clone, Debug, Eq, Facet, PartialEq)]
pub struct TracingInitializedEvent {
    pub effective_filter_directive: String,
    pub json_log_path: Option<String>,
    pub subscriber_was_already_initialized: bool,
}

#[derive(Clone, Debug, Eq, Facet, PartialEq)]
pub struct TracingObservedField {
    pub name: String,
    pub value: String,
}

#[derive(Clone, Debug, Eq, Facet, PartialEq)]
pub struct TracingRecordObservedEvent {
    pub target: String,
    pub name: String,
    pub level: String,
    pub fields: Vec<TracingObservedField>,
}

#[derive(Clone, Debug, Default)]
pub struct TracingObservationLayer {
    observed_records: Arc<Mutex<Vec<TracingRecordObservedEvent>>>,
}

impl TracingObservationLayer {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn observed_records(&self) -> Vec<TracingRecordObservedEvent> {
        self.observed_records
            .lock()
            .expect("tracing observation records mutex should not be poisoned")
            .clone()
    }
}

impl<S> Layer<S> for TracingObservationLayer
where
    S: Subscriber,
{
    fn on_event(&self, event: &Event<'_>, _context: Context<'_, S>) {
        let mut visitor = TracingObservationVisitor::default();
        event.record(&mut visitor);

        if visitor.is_timeline_reemit {
            return;
        }

        let metadata = event.metadata();
        self.observed_records
            .lock()
            .expect("tracing observation records mutex should not be poisoned")
            .push(TracingRecordObservedEvent {
                target: metadata.target().to_owned(),
                name: metadata.name().to_owned(),
                level: metadata.level().to_string(),
                fields: visitor.fields,
            });
    }
}

#[derive(Debug, Default)]
struct TracingObservationVisitor {
    is_timeline_reemit: bool,
    fields: Vec<TracingObservedField>,
}

impl TracingObservationVisitor {
    fn push_field(&mut self, field: &Field, value: String) {
        if field.name() == TIMELINE_REEMIT_FIELD_NAME && value == "true" {
            self.is_timeline_reemit = true;
        }

        self.fields.push(TracingObservedField {
            name: field.name().to_owned(),
            value,
        });
    }
}

impl Visit for TracingObservationVisitor {
    fn record_bool(&mut self, field: &Field, value: bool) {
        self.push_field(field, value.to_string());
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.push_field(field, format!("{value:?}"));
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.push_field(field, value.to_string());
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.push_field(field, value.to_string());
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.push_field(field, value.to_owned());
    }
}

impl StartupLoggingPlan {
    pub fn from_global_args(
        global_args: &GlobalArgs,
        rust_log: Option<&str>,
        now: DateTime<Local>,
    ) -> std::result::Result<Self, StartupLoggingPlanError> {
        Ok(Self {
            debug: global_args.debug,
            filter_selection: select_log_filter(global_args, rust_log)?,
            json_log_path: resolve_json_log_path(global_args.log_file.as_deref(), now),
        })
    }

    #[must_use]
    pub fn effective_filter_directive(&self) -> String {
        match &self.filter_selection {
            LogFilterSelection::Explicit(filter) | LogFilterSelection::FromEnv(filter) => {
                filter.clone()
            }
            LogFilterSelection::Default(level) => match *level {
                LevelFilter::OFF => "off".to_owned(),
                LevelFilter::ERROR => "error".to_owned(),
                LevelFilter::WARN => "warn".to_owned(),
                LevelFilter::INFO => "info".to_owned(),
                LevelFilter::DEBUG => "debug".to_owned(),
                LevelFilter::TRACE => "trace".to_owned(),
            },
        }
    }

    #[must_use]
    pub fn to_configured_event(&self) -> StartupLoggingConfiguredEvent {
        StartupLoggingConfiguredEvent {
            debug: self.debug,
            effective_filter_directive: self.effective_filter_directive(),
            json_log_path: self
                .json_log_path
                .as_ref()
                .map(|path| path.to_string_lossy().into_owned()),
        }
    }

    #[must_use]
    pub fn to_tracing_initialized_event(
        &self,
        subscriber_was_already_initialized: bool,
    ) -> TracingInitializedEvent {
        TracingInitializedEvent {
            effective_filter_directive: self.effective_filter_directive(),
            json_log_path: self
                .json_log_path
                .as_ref()
                .map(|path| path.to_string_lossy().into_owned()),
            subscriber_was_already_initialized,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StartupBootstrapPlan {
    pub raw_inputs: RawProcessStartupInputs,
    pub global_args: GlobalArgs,
    pub logging: StartupLoggingPlan,
}

impl StartupBootstrapPlan {
    #[must_use]
    pub fn empty() -> Self {
        let raw_inputs = RawProcessStartupInputs {
            argv: Vec::new(),
            rust_log: None,
        };
        Self {
            logging: StartupLoggingPlan::from_global_args(
                &GlobalArgs::default(),
                None,
                Local::now(),
            )
            .expect("default startup logging plan should resolve"),
            global_args: GlobalArgs::default(),
            raw_inputs,
        }
    }
}

#[cfg(feature = "tracy")]
fn exclude_tracy_frame_mark(meta: &Metadata<'_>) -> bool {
    meta.fields().field("tracy.frame_mark").is_none()
}

fn startup_help_text() -> String {
    "Teamy Studio\n\nUsage:\n  teamy-studio [OPTIONS]\n\nOptions:\n  --debug                    Enable debug logging\n  --log-filter <FILTER>      Override the tracing filter directive\n  --log-file <PATH>          Write NDJSON logs to a file or directory\n  -h, --help                 Print help and exit\n  -V, --version              Print version and exit\n  --completions <SHELL>      Print shell completions for bash, zsh, or fish\n".to_owned()
}

fn startup_version_text() -> String {
    format!("teamy-studio {}", env!("CARGO_PKG_VERSION"))
}

fn startup_completion_script(shell: &str) -> Option<String> {
    match shell {
        "bash" => Some(
            r#"_teamy_studio()
{
    local cur
    cur="${COMP_WORDS[COMP_CWORD]}"
    COMPREPLY=( $(compgen -W '--debug --log-filter --log-file --help --version --completions' -- "$cur") )
}
complete -F _teamy_studio teamy-studio
"#
            .to_owned(),
        ),
        "zsh" => Some(
            r#"#compdef teamy-studio
_teamy_studio() {
  _arguments \
    '--debug[Enable debug logging]' \
    '--log-filter[Override the tracing filter]:filter:' \
    '--log-file[Write NDJSON logs]:path:_files' \
    '--help[Print help and exit]' \
    '--version[Print version and exit]' \
    '--completions[Print shell completions]:shell:(bash zsh fish)'
}
_teamy_studio "$@"
"#
            .to_owned(),
        ),
        "fish" => Some(
            r#"complete -c teamy-studio -l debug -d 'Enable debug logging'
complete -c teamy-studio -l log-filter -r -d 'Override the tracing filter directive'
complete -c teamy-studio -l log-file -r -d 'Write NDJSON logs to a file or directory'
complete -c teamy-studio -s h -l help -d 'Print help and exit'
complete -c teamy-studio -s V -l version -d 'Print version and exit'
complete -c teamy-studio -l completions -r -f -a 'bash zsh fish' -d 'Print shell completions'
"#
            .to_owned(),
        ),
        _ => None,
    }
}

fn detect_builtin_request(args: &[&str]) -> Option<BootstrapCliParseError> {
    if args.iter().any(|arg| matches!(*arg, "--help" | "-h")) {
        return Some(BootstrapCliParseError::HelpRequested {
            text: startup_help_text(),
        });
    }

    if args.iter().any(|arg| matches!(*arg, "--version" | "-V")) {
        return Some(BootstrapCliParseError::VersionRequested {
            text: startup_version_text(),
        });
    }

    let mut index = 0;
    while index < args.len() {
        if args[index] == "--completions" {
            let shell = match args.get(index + 1) {
                Some(shell) => *shell,
                None => {
                    return Some(BootstrapCliParseError::MissingValue {
                        flag: "--completions",
                    });
                }
            };

            return startup_completion_script(shell)
                .map(|script| BootstrapCliParseError::CompletionsRequested { script })
                .or_else(|| {
                    Some(BootstrapCliParseError::UnrecognizedArgument {
                        argument: shell.to_owned(),
                    })
                });
        }
        index += 1;
    }

    None
}

pub fn try_handle_builtin_bootstrap_cli_request(args: &[&str]) -> Result<bool> {
    match detect_builtin_request(args) {
        Some(BootstrapCliParseError::HelpRequested { text })
        | Some(BootstrapCliParseError::VersionRequested { text }) => {
            println!("{text}");
            Ok(true)
        }
        Some(BootstrapCliParseError::CompletionsRequested { script }) => {
            println!("{script}");
            Ok(true)
        }
        Some(BootstrapCliParseError::MissingValue { .. })
        | Some(BootstrapCliParseError::UnrecognizedArgument { .. })
        | None => Ok(false),
    }
}

pub fn parse_bootstrap_cli_args(
    args: &[&str],
) -> std::result::Result<StartupBootstrapCli, BootstrapCliParseError> {
    if let Some(error) = detect_builtin_request(args) {
        return Err(error);
    }

    let mut global_args = GlobalArgs::default();
    let mut index = 0;

    while index < args.len() {
        match args[index] {
            "--debug" => {
                global_args.debug = true;
                index += 1;
            }
            "--log-filter" => {
                let value = args
                    .get(index + 1)
                    .ok_or(BootstrapCliParseError::MissingValue {
                        flag: "--log-filter",
                    })?;
                global_args.log_filter = Some((*value).to_owned());
                index += 2;
            }
            "--log-file" => {
                let value = args
                    .get(index + 1)
                    .ok_or(BootstrapCliParseError::MissingValue { flag: "--log-file" })?;
                global_args.log_file = Some((*value).to_owned());
                index += 2;
            }
            unknown => {
                return Err(BootstrapCliParseError::UnrecognizedArgument {
                    argument: unknown.to_owned(),
                });
            }
        }
    }

    Ok(StartupBootstrapCli { global_args })
}

pub fn derive_bootstrap_plan(
    raw_inputs: &RawProcessStartupInputs,
    now: DateTime<Local>,
) -> std::result::Result<StartupBootstrapPlan, BootstrapPlanError> {
    let parsed_cli =
        parse_bootstrap_cli_args(&raw_inputs.argv_refs()).map_err(BootstrapPlanError::CliParse)?;
    let logging = StartupLoggingPlan::from_global_args(
        &parsed_cli.global_args,
        raw_inputs.rust_log.as_deref(),
        now,
    )
    .map_err(BootstrapPlanError::LoggingPlan)?;

    Ok(StartupBootstrapPlan {
        raw_inputs: raw_inputs.clone(),
        global_args: parsed_cli.global_args,
        logging,
    })
}

pub fn initialize_tracing_from_bootstrap_plan(
    bootstrap_plan: &StartupBootstrapPlan,
) -> Result<TracingInitializedEvent> {
    let stderr_env_filter =
        EnvFilter::builder().parse(bootstrap_plan.logging.effective_filter_directive())?;
    let stderr_layer = if bootstrap_plan.logging.debug {
        fmt::layer()
            .with_file(cfg!(debug_assertions))
            .with_line_number(cfg!(debug_assertions))
            .with_target(true)
            .with_writer(std::io::stderr)
            .pretty()
            .with_timer(tracing_subscriber::fmt::time::uptime())
            .boxed()
    } else {
        fmt::layer()
            .with_file(cfg!(debug_assertions))
            .with_line_number(cfg!(debug_assertions))
            .with_target(true)
            .with_writer(std::io::stderr)
            .pretty()
            .without_time()
            .boxed()
    };
    let stderr_layer = stderr_layer.with_filter(stderr_env_filter);
    #[cfg(feature = "tracy")]
    let stderr_layer = stderr_layer.with_filter(FilterFn::new(exclude_tracy_frame_mark));

    let subscriber = Registry::default().with(stderr_layer);
    #[cfg(all(feature = "tracy", not(test)))]
    let subscriber = subscriber.with(tracing_tracy::TracyLayer::default());

    if let Some(json_log_path) = &bootstrap_plan.logging.json_log_path {
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
        let json_layer = fmt::layer()
            .json()
            .with_file(true)
            .with_line_number(true)
            .with_target(false)
            .with_writer(json_writer)
            .with_filter(
                EnvFilter::builder().parse(bootstrap_plan.logging.effective_filter_directive())?,
            );
        #[cfg(feature = "tracy")]
        let json_layer = json_layer.with_filter(FilterFn::new(exclude_tracy_frame_mark));

        let subscriber_was_already_initialized =
            try_init_with_already_initialized(subscriber.with(json_layer).try_init())?;
        return Ok(bootstrap_plan
            .logging
            .to_tracing_initialized_event(subscriber_was_already_initialized));
    }

    let subscriber_was_already_initialized =
        try_init_with_already_initialized(subscriber.try_init())?;
    Ok(bootstrap_plan
        .logging
        .to_tracing_initialized_event(subscriber_was_already_initialized))
}

fn try_init_with_already_initialized(
    result: std::result::Result<(), tracing_subscriber::util::TryInitError>,
) -> Result<bool> {
    match result {
        Ok(()) => Ok(false),
        Err(error) => ignore_already_initialized(error),
    }
}

fn ignore_already_initialized(error: tracing_subscriber::util::TryInitError) -> Result<bool> {
    if error
        .to_string()
        .contains("a global default trace dispatcher has already been set")
    {
        return Ok(true);
    }

    Err(eyre!(error.to_string()))
}

fn select_log_filter(
    global_args: &GlobalArgs,
    rust_log: Option<&str>,
) -> std::result::Result<LogFilterSelection, StartupLoggingPlanError> {
    match (global_args.debug, global_args.log_filter.as_deref()) {
        (true, Some(filter)) => Err(StartupLoggingPlanError::DebugConflictsWithExplicitFilter {
            filter: filter.to_owned(),
        }),
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

fn resolve_json_log_path(log_file: Option<&str>, now: DateTime<Local>) -> Option<PathBuf> {
    match log_file {
        None => None,
        Some(path) if PathBuf::from(path).is_dir() => {
            let timestamp = now.format("%Y-%m-%d_%H-%M-%S");
            Some(PathBuf::from(path).join(format!("log_{timestamp}.ndjson")))
        }
        Some(path) => Some(PathBuf::from(path)),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BootstrapCliParseError, BootstrapPlanError, GlobalArgs, LogFilterSelection,
        RawProcessStartupInputs, StartupBootstrapPlan, StartupGlobalArgsParsedEvent,
        StartupLoggingPlan, StartupLoggingPlanError, TracingInitializedEvent,
        TracingObservationLayer, derive_bootstrap_plan, parse_bootstrap_cli_args,
        try_handle_builtin_bootstrap_cli_request,
    };
    use chrono::{Local, TimeZone};
    use tracing_subscriber::Registry;
    use tracing_subscriber::prelude::*;

    #[test]
    fn bootstrap_cli_parses_debug_flag() {
        let parsed = parse_bootstrap_cli_args(&["--debug"]).expect("debug flag should parse");

        assert!(parsed.global_args.debug);
        assert_eq!(parsed.global_args.log_filter, None);
        assert_eq!(parsed.global_args.log_file, None);
    }

    #[test]
    fn bootstrap_cli_parses_log_filter_and_log_file() {
        let parsed =
            parse_bootstrap_cli_args(&["--log-filter", "trace", "--log-file", "logs/teamy.ndjson"])
                .expect("log flags should parse");

        assert_eq!(parsed.global_args.log_filter.as_deref(), Some("trace"));
        assert_eq!(
            parsed.global_args.log_file.as_deref(),
            Some("logs/teamy.ndjson")
        );
    }

    #[test]
    fn bootstrap_cli_surfaces_help_requests() {
        let error = parse_bootstrap_cli_args(&["--help"]).expect_err("help should short-circuit");

        assert!(matches!(
            error,
            BootstrapCliParseError::HelpRequested { .. }
        ));
        assert!(error.to_string().contains("--help"));
    }

    #[test]
    fn bootstrap_cli_surfaces_version_requests() {
        let error =
            parse_bootstrap_cli_args(&["--version"]).expect_err("version should short-circuit");

        assert!(matches!(
            error,
            BootstrapCliParseError::VersionRequested { .. }
        ));
        assert!(error.to_string().contains(env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn builtin_handler_marks_help_as_handled() {
        let handled = try_handle_builtin_bootstrap_cli_request(&["--help"])
            .expect("help should be handled without error");

        assert!(handled);
    }

    #[test]
    fn bootstrap_cli_surfaces_completion_requests() {
        let error = parse_bootstrap_cli_args(&["--completions", "bash"])
            .expect_err("completions should short-circuit");

        assert!(matches!(
            error,
            BootstrapCliParseError::CompletionsRequested { .. }
        ));
        assert!(error.to_string().contains("teamy-studio"));
    }

    // tool[verify logging.filter.debug-conflicts-with-log-filter]
    #[test]
    fn debug_conflicts_with_explicit_log_filter() {
        let error = StartupLoggingPlan::from_global_args(
            &GlobalArgs {
                debug: true,
                log_filter: Some("trace".to_owned()),
                log_file: None,
            },
            None,
            fixed_now(),
        )
        .expect_err("debug plus log-filter should fail");

        assert_eq!(
            error,
            StartupLoggingPlanError::DebugConflictsWithExplicitFilter {
                filter: "trace".to_owned(),
            }
        );
    }

    // tool[verify logging.filter.from-env]
    #[test]
    fn rust_log_is_used_when_explicit_filter_is_omitted() {
        let logging =
            StartupLoggingPlan::from_global_args(&GlobalArgs::default(), Some("warn"), fixed_now())
                .expect("RUST_LOG should be accepted when --log-filter is omitted");

        assert_eq!(
            logging.filter_selection,
            LogFilterSelection::FromEnv("warn".to_owned())
        );
        assert_eq!(logging.effective_filter_directive(), "warn");
    }

    // tool[verify logging.filter.defaults]
    #[test]
    fn debug_defaults_to_debug_filter_when_no_filter_is_provided() {
        let logging = StartupLoggingPlan::from_global_args(
            &GlobalArgs {
                debug: true,
                ..GlobalArgs::default()
            },
            None,
            fixed_now(),
        )
        .expect("debug default filter should resolve");

        assert_eq!(
            logging.filter_selection,
            LogFilterSelection::Default(tracing::level_filters::LevelFilter::DEBUG)
        );
        assert_eq!(logging.effective_filter_directive(), "debug");
    }

    // tool[verify logging.filter.defaults]
    #[test]
    fn non_debug_defaults_to_info_filter_when_no_filter_is_provided() {
        let logging =
            StartupLoggingPlan::from_global_args(&GlobalArgs::default(), None, fixed_now())
                .expect("non-debug default filter should resolve");

        assert_eq!(
            logging.filter_selection,
            LogFilterSelection::Default(tracing::level_filters::LevelFilter::INFO)
        );
        assert_eq!(logging.effective_filter_directive(), "info");
    }

    // tool[verify logging.file-path-option]
    #[test]
    fn explicit_log_file_path_is_preserved() {
        let logging = StartupLoggingPlan::from_global_args(
            &GlobalArgs {
                log_file: Some("logs/teamy.ndjson".to_owned()),
                ..GlobalArgs::default()
            },
            None,
            fixed_now(),
        )
        .expect("explicit log file path should resolve");

        assert_eq!(
            logging
                .json_log_path
                .expect("explicit path should be present")
                .to_string_lossy(),
            "logs/teamy.ndjson"
        );
    }

    // tool[verify logging.file-path-option]
    #[test]
    fn directory_log_file_path_gets_timestamped_ndjson_filename() {
        let dir = tempfile::tempdir().expect("temporary log directory should exist");
        let logging = StartupLoggingPlan::from_global_args(
            &GlobalArgs {
                log_file: Some(dir.path().to_string_lossy().into_owned()),
                ..GlobalArgs::default()
            },
            None,
            fixed_now(),
        )
        .expect("directory log path should resolve");
        let log_path = logging.json_log_path.expect("log path should be present");

        assert_eq!(
            log_path.file_name().and_then(|name| name.to_str()),
            Some("log_2026-05-18_12-34-56.ndjson")
        );
        assert_eq!(log_path.parent(), Some(dir.path()));
    }

    #[test]
    fn bootstrap_plan_derives_logging_from_raw_inputs() {
        let plan = derive_bootstrap_plan(
            &RawProcessStartupInputs {
                argv: vec!["--log-file".to_owned(), "logs/teamy.ndjson".to_owned()],
                rust_log: Some("trace".to_owned()),
            },
            fixed_now(),
        )
        .expect("bootstrap plan should derive from raw inputs");

        assert_eq!(
            plan.global_args.log_file.as_deref(),
            Some("logs/teamy.ndjson")
        );
        assert_eq!(
            plan.logging.filter_selection,
            LogFilterSelection::FromEnv("trace".to_owned())
        );
    }

    #[test]
    fn empty_bootstrap_plan_uses_default_global_args() {
        let plan = StartupBootstrapPlan::empty();

        assert_eq!(plan.raw_inputs.argv, Vec::<String>::new());
        assert_eq!(plan.raw_inputs.rust_log, None);
        assert_eq!(plan.global_args, GlobalArgs::default());
        assert_eq!(plan.logging.effective_filter_directive(), "info");
    }

    #[test]
    fn tracing_initialized_event_reflects_logging_plan() {
        let logging = StartupLoggingPlan::from_global_args(
            &GlobalArgs {
                log_file: Some("logs/teamy.ndjson".to_owned()),
                ..GlobalArgs::default()
            },
            Some("trace"),
            fixed_now(),
        )
        .expect("logging plan should resolve");
        let event: TracingInitializedEvent = logging.to_tracing_initialized_event(true);

        assert_eq!(event.effective_filter_directive, "trace");
        assert_eq!(event.json_log_path.as_deref(), Some("logs/teamy.ndjson"));
        assert!(event.subscriber_was_already_initialized);
    }

    #[test]
    fn tracing_observation_layer_captures_unmarked_records() {
        let layer = TracingObservationLayer::new();
        let handle = layer.clone();
        let subscriber = Registry::default().with(layer);

        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(
                custom_field = "alpha",
                numeric_field = 42_u64,
                "ordinary tracing record"
            );
        });

        let records = handle.observed_records();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].level, "INFO");
        assert!(
            records[0]
                .fields
                .iter()
                .any(|field| { field.name == "custom_field" && field.value == "alpha" })
        );
        assert!(
            records[0]
                .fields
                .iter()
                .any(|field| { field.name == "numeric_field" && field.value == "42" })
        );
    }

    #[test]
    fn tracing_observation_layer_ignores_timeline_reemissions() {
        let layer = TracingObservationLayer::new();
        let handle = layer.clone();
        let subscriber = Registry::default().with(layer);

        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(
                teamy.timeline_reemit = true,
                event_schema_name = "teamy_studio.startup.succeeded",
                "timeline event re-emitted to tracing"
            );
        });

        assert!(handle.observed_records().is_empty());
    }

    #[test]
    fn global_args_parsed_event_reflects_global_args() {
        let event: StartupGlobalArgsParsedEvent = GlobalArgs {
            debug: true,
            log_filter: Some("trace".to_owned()),
            log_file: Some("logs/teamy.ndjson".to_owned()),
        }
        .to_parsed_event();

        assert!(event.debug);
        assert_eq!(event.log_filter.as_deref(), Some("trace"));
        assert_eq!(event.log_file.as_deref(), Some("logs/teamy.ndjson"));
    }

    #[test]
    fn bootstrap_cli_rejects_unknown_arguments() {
        let error = parse_bootstrap_cli_args(&["--wat"]).expect_err("unknown args should fail");

        assert_eq!(
            error,
            BootstrapCliParseError::UnrecognizedArgument {
                argument: "--wat".to_owned(),
            }
        );
    }

    #[test]
    fn bootstrap_cli_requires_values_for_value_flags() {
        let error = parse_bootstrap_cli_args(&["--log-filter"])
            .expect_err("value flags should require a following value");

        assert_eq!(
            error,
            BootstrapCliParseError::MissingValue {
                flag: "--log-filter"
            }
        );
    }

    #[test]
    fn bootstrap_plan_preserves_typed_cli_parse_failures() {
        let error = derive_bootstrap_plan(
            &RawProcessStartupInputs {
                argv: vec!["--wat".to_owned()],
                rust_log: None,
            },
            fixed_now(),
        )
        .expect_err("unknown startup args should fail plan derivation");

        assert_eq!(
            error,
            BootstrapPlanError::CliParse(BootstrapCliParseError::UnrecognizedArgument {
                argument: "--wat".to_owned(),
            })
        );
    }

    #[test]
    fn bootstrap_plan_preserves_typed_logging_plan_failures() {
        let error = derive_bootstrap_plan(
            &RawProcessStartupInputs {
                argv: vec![
                    "--debug".to_owned(),
                    "--log-filter".to_owned(),
                    "trace".to_owned(),
                ],
                rust_log: None,
            },
            fixed_now(),
        )
        .expect_err("conflicting startup logging args should fail plan derivation");

        assert_eq!(
            error,
            BootstrapPlanError::LoggingPlan(
                StartupLoggingPlanError::DebugConflictsWithExplicitFilter {
                    filter: "trace".to_owned(),
                }
            )
        );
    }

    fn fixed_now() -> chrono::DateTime<Local> {
        Local
            .with_ymd_and_hms(2026, 5, 18, 12, 34, 56)
            .single()
            .expect("fixed local time should be constructible")
    }
}

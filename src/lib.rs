#![deny(clippy::disallowed_methods)]
#![deny(clippy::disallowed_macros)]

#[cfg(feature = "mvp")]
pub use teamy_studio_startup::{
    AppComposition, BootstrapCliParseError, BootstrapPlanError, GlobalArgs,
    ObservedBootstrapPlan, ObservedBootstrapPlanFailure,
    LogFilterSelection, RawProcessStartupInputs,
    BOOTSTRAP_CLI_PARSE_FAILED_EVENT_DEFINITION,
    DEFAULT_CURSOR_GALLERY_FLOW_FAILED_EVENT_DEFINITION,
    FEATURE_ACTIVATION_GATE_RESOLVED_EVENT_DEFINITION,
    FEATURE_COMPATIBILITY_VALIDATED_EVENT_DEFINITION,
    FEATURE_COMPATIBILITY_VALIDATION_COMPLETED_EVENT_DEFINITION,
    FEATURE_COMPATIBILITY_VALIDATION_STARTED_EVENT_DEFINITION,
    PROCESS_STARTUP_OBSERVED_EVENT_DEFINITION, STARTUP_FAILED_EVENT_DEFINITION,
    REGISTRATION_VALIDATION_COMPLETED_EVENT_DEFINITION,
    REGISTRATION_VALIDATION_STARTED_EVENT_DEFINITION,
    STARTUP_GLOBAL_ARGS_PARSED_EVENT_DEFINITION,
    STARTUP_LOGGING_PLAN_FAILED_EVENT_DEFINITION,
    STARTUP_LOGGING_CONFIGURED_EVENT_DEFINITION, STARTUP_SUCCEEDED_EVENT_DEFINITION,
    TRACING_INITIALIZATION_FAILED_EVENT_DEFINITION, TRACING_INITIALIZED_EVENT_DEFINITION,
    ProcessStartupObservedEvent, BootstrapCliParseFailedEvent,
    DefaultCursorGalleryFlowFailedEvent,
    FeatureActivationGateResolvedEvent,
    FeatureCompatibilityValidatedEvent, FeatureCompatibilityValidationCompletedEvent,
    FeatureCompatibilityValidationStartedEvent,
    RegistrationValidationCompletedEvent, RegistrationValidationStartedEvent,
    StartupBootstrapCli, StartupBootstrapPlan, StartupFailedEvent,
    StartupGlobalArgsParsedEvent, StartupLoggingConfiguredEvent, StartupLoggingPlan,
    StartupLoggingPlanError, StartupLoggingPlanFailedEvent, StartupSession,
    StartupSucceededEvent, TracingInitializationFailedEvent,
    TracingInitializedEvent,
    build_mvp_composition, build_mvp_composition_from_raw_inputs,
    build_mvp_composition_with_native_shell, build_mvp_session,
    build_mvp_session_from_bootstrap_plan, build_mvp_session_from_raw_inputs,
    build_mvp_session_with_native_shell,
    build_mvp_session_with_native_shell_from_bootstrap_plan,
    build_mvp_session_with_native_shell_from_raw_inputs, derive_bootstrap_plan,
    observe_bootstrap_plan_from_raw_inputs,
    observe_bootstrap_plan_from_raw_inputs_with_native_shell,
    initialize_tracing_from_bootstrap_plan, main, main_with_raw_inputs,
    parse_bootstrap_cli_args,
};

#[cfg(not(feature = "mvp"))]
/// Entrypoint for configurations where the MVP feature stack is disabled.
///
/// # Errors
///
/// Always returns an error because the thin root currently requires the MVP
/// composition feature to be enabled.
pub fn main() -> eyre::Result<()> {
    Err(eyre::eyre!(
        "teamy-studio currently requires the `mvp` feature to run"
    ))
}

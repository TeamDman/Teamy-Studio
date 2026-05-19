#![deny(clippy::disallowed_methods)]
#![deny(clippy::disallowed_macros)]

#[cfg(feature = "mvp")]
pub use teamy_studio_startup::{
    AppComposition, BOOTSTRAP_CLI_PARSE_FAILED_EVENT_DEFINITION, BootstrapCliParseError,
    BootstrapCliParseFailedEvent, BootstrapPlanError,
    DEFAULT_CURSOR_GALLERY_FLOW_FAILED_EVENT_DEFINITION,
    DEFAULT_CURSOR_GALLERY_FLOW_REQUESTED_EVENT_DEFINITION, DefaultCursorGalleryFlowFailedEvent,
    DefaultCursorGalleryFlowRequestedEvent, FEATURE_ACTIVATION_GATE_RESOLVED_EVENT_DEFINITION,
    FEATURE_COMPATIBILITY_VALIDATED_EVENT_DEFINITION,
    FEATURE_COMPATIBILITY_VALIDATION_COMPLETED_EVENT_DEFINITION,
    FEATURE_COMPATIBILITY_VALIDATION_STARTED_EVENT_DEFINITION, FeatureActivationGateResolvedEvent,
    FeatureCompatibilityValidatedEvent, FeatureCompatibilityValidationCompletedEvent,
    FeatureCompatibilityValidationStartedEvent, GlobalArgs, LogFilterSelection,
    NATIVE_MAIN_MENU_LAUNCH_REQUESTED_EVENT_DEFINITION, NativeMainMenuLaunchRequestedEvent,
    ObservedBootstrapPlan, ObservedBootstrapPlanFailure, PROCESS_STARTUP_OBSERVED_EVENT_DEFINITION,
    ProcessStartupObservedEvent, REGISTRATION_VALIDATION_COMPLETED_EVENT_DEFINITION,
    REGISTRATION_VALIDATION_STARTED_EVENT_DEFINITION, RawProcessStartupInputs,
    RegistrationValidationCompletedEvent, RegistrationValidationStartedEvent,
    STARTUP_COMPOSITION_POLICY_DERIVED_EVENT_DEFINITION,
    STARTUP_COMPOSITION_READY_EVENT_DEFINITION, STARTUP_FAILED_EVENT_DEFINITION,
    STARTUP_GLOBAL_ARGS_PARSED_EVENT_DEFINITION, STARTUP_LOGGING_CONFIGURED_EVENT_DEFINITION,
    STARTUP_LOGGING_PLAN_FAILED_EVENT_DEFINITION, STARTUP_SUCCEEDED_EVENT_DEFINITION,
    StartupBootstrapCli, StartupBootstrapPlan, StartupCompositionPolicyDerivedEvent,
    StartupCompositionReadyEvent, StartupFailedEvent, StartupGlobalArgsParsedEvent,
    StartupLoggingConfiguredEvent, StartupLoggingPlan, StartupLoggingPlanError,
    StartupLoggingPlanFailedEvent, StartupSession, StartupSucceededEvent,
    TRACING_INITIALIZATION_FAILED_EVENT_DEFINITION, TRACING_INITIALIZED_EVENT_DEFINITION,
    TracingInitializationFailedEvent, TracingInitializedEvent, build_mvp_composition,
    build_mvp_composition_from_raw_inputs, build_mvp_composition_with_native_shell,
    build_mvp_session, build_mvp_session_from_bootstrap_plan, build_mvp_session_from_raw_inputs,
    build_mvp_session_with_native_shell, build_mvp_session_with_native_shell_from_bootstrap_plan,
    build_mvp_session_with_native_shell_from_raw_inputs, derive_bootstrap_plan,
    initialize_tracing_from_bootstrap_plan, main, main_with_raw_inputs,
    observe_bootstrap_plan_from_raw_inputs,
    observe_bootstrap_plan_from_raw_inputs_with_native_shell, parse_bootstrap_cli_args,
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

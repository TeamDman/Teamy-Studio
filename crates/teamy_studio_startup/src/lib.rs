pub mod bootstrap;

pub use bootstrap::{
    BootstrapCliParseError, BootstrapPlanError, GlobalArgs, LogFilterSelection,
    ProcessStartupObservedEvent, RawProcessStartupInputs, StartupBootstrapCli,
    StartupBootstrapPlan, StartupGlobalArgsParsedEvent, StartupLoggingConfiguredEvent,
    StartupLoggingPlan, StartupLoggingPlanError, TracingInitialization, TracingInitializedEvent,
    TracingObservationLayer, TracingObservedField, TracingRecordObservedEvent,
    derive_bootstrap_plan, initialize_tracing_from_bootstrap_plan,
    initialize_tracing_with_observation_from_bootstrap_plan, parse_bootstrap_cli_args,
    try_handle_builtin_bootstrap_cli_request,
};

use eyre::{Report, Result, eyre};
use facet::Facet;
use linkme::distributed_slice;
use std::collections::BTreeMap;
use std::error::Error;
use std::fmt::{Display, Formatter};
use teamy_studio_cursor_gallery::CursorGalleryState;
use teamy_studio_cursor_gallery::{CURSOR_GALLERY_BUTTON_CLASS_ID, CURSOR_GALLERY_FEATURE_ID};
use teamy_studio_event_core::{
    EventDefinition, EventDefinitionId, EventLogIntent, EventLogLevel, PublishedEvent,
    WritableArena,
};
use teamy_studio_launcher_catalog as _;
use teamy_studio_launcher_catalog::{
    APPLICATION_WINDOWS_BUTTON_CLASS_ID, AUDIO_DAEMON_BUTTON_CLASS_ID,
    AUDIO_DEVICES_BUTTON_CLASS_ID, AUDIO_PICKER_BUTTON_CLASS_ID, CURSOR_INFO_BUTTON_CLASS_ID,
    DEMO_MODE_BUTTON_CLASS_ID, ENVIRONMENT_VARIABLES_BUTTON_CLASS_ID, JOBS_BUTTON_CLASS_ID,
    LOGS_BUTTON_CLASS_ID, STORAGE_BUTTON_CLASS_ID, TERMINAL_BUTTON_CLASS_ID,
    TEXT_RENDERING_PLAYGROUND_BUTTON_CLASS_ID, TIMELINE_BUTTON_CLASS_ID,
    TIMELINE_PLAYGROUND_BUTTON_CLASS_ID,
};
use teamy_studio_main_menu::{
    FeatureValidationState, MainMenuButtonClassId, MainMenuLogicalButtonId, MainMenuSnapshot,
    registered_button_classes, show_main_menu_info_dialog,
};
use teamy_studio_registration_core::{
    EVENT_DEFINITION_REGISTRATIONS, EventDefinitionRegistration, FeatureId, RegistrationProvenance,
    RegistrationSnapshot, RegistrationValidationError, TriggerDefinitionId, TriggerRegistrationId,
    registration_provenance, snapshot, trigger_registrations_for, validate_registrations,
};
use teamy_studio_shell::{HostedWindowRecord, ShellRuntime, ShellState};
use teamy_studio_timeline_core::{
    CanonicalTimeKey, ConstructedTimeline, EventId, EventReference, TriggerCursor, TriggerRuntime,
};
use tracing::{Level, event, info, trace};

pub static STARTUP_SUCCEEDED_EVENT_DEFINITION: EventDefinition = EventDefinition {
    id: EventDefinitionId::from_bytes([0x01; 16]),
    schema_name: "teamy_studio.startup.succeeded",
    schema_version: 1,
    log_intent: EventLogIntent::INFO,
};

pub static STARTUP_FAILED_EVENT_DEFINITION: EventDefinition = EventDefinition {
    id: EventDefinitionId::from_bytes([0x02; 16]),
    schema_name: "teamy_studio.startup.failed",
    schema_version: 1,
    log_intent: EventLogIntent::ERROR,
};

pub static PROCESS_STARTUP_OBSERVED_EVENT_DEFINITION: EventDefinition = EventDefinition {
    id: EventDefinitionId::from_bytes([0x03; 16]),
    schema_name: "teamy_studio.startup.process_startup_observed",
    schema_version: 1,
    log_intent: EventLogIntent::TRACE,
};

pub static STARTUP_GLOBAL_ARGS_PARSED_EVENT_DEFINITION: EventDefinition = EventDefinition {
    id: EventDefinitionId::from_bytes([0x08; 16]),
    schema_name: "teamy_studio.startup.global_args_parsed",
    schema_version: 1,
    log_intent: EventLogIntent::TRACE,
};

pub static STARTUP_LOGGING_CONFIGURED_EVENT_DEFINITION: EventDefinition = EventDefinition {
    id: EventDefinitionId::from_bytes([0x04; 16]),
    schema_name: "teamy_studio.startup.logging_configured",
    schema_version: 1,
    log_intent: EventLogIntent::DEBUG,
};

pub static TRACING_INITIALIZED_EVENT_DEFINITION: EventDefinition = EventDefinition {
    id: EventDefinitionId::from_bytes([0x05; 16]),
    schema_name: "teamy_studio.startup.tracing_initialized",
    schema_version: 1,
    log_intent: EventLogIntent::INFO,
};

pub static REGISTRATION_VALIDATION_STARTED_EVENT_DEFINITION: EventDefinition = EventDefinition {
    id: EventDefinitionId::from_bytes([0x06; 16]),
    schema_name: "teamy_studio.startup.registration_validation_started",
    schema_version: 1,
    log_intent: EventLogIntent::DEBUG,
};

pub static REGISTRATION_VALIDATION_COMPLETED_EVENT_DEFINITION: EventDefinition = EventDefinition {
    id: EventDefinitionId::from_bytes([0x07; 16]),
    schema_name: "teamy_studio.startup.registration_validation_completed",
    schema_version: 1,
    log_intent: EventLogIntent::DEBUG,
};

pub static FEATURE_COMPATIBILITY_VALIDATION_STARTED_EVENT_DEFINITION: EventDefinition =
    EventDefinition {
        id: EventDefinitionId::from_bytes([0x09; 16]),
        schema_name: "teamy_studio.startup.feature_compatibility_validation_started",
        schema_version: 1,
        log_intent: EventLogIntent::DEBUG,
    };

pub static FEATURE_COMPATIBILITY_VALIDATED_EVENT_DEFINITION: EventDefinition = EventDefinition {
    id: EventDefinitionId::from_bytes([0x0A; 16]),
    schema_name: "teamy_studio.startup.feature_compatibility_validated",
    schema_version: 1,
    log_intent: EventLogIntent::DEBUG,
};

pub static FEATURE_COMPATIBILITY_VALIDATION_COMPLETED_EVENT_DEFINITION: EventDefinition =
    EventDefinition {
        id: EventDefinitionId::from_bytes([0x0B; 16]),
        schema_name: "teamy_studio.startup.feature_compatibility_validation_completed",
        schema_version: 1,
        log_intent: EventLogIntent::DEBUG,
    };

pub static FEATURE_ACTIVATION_GATE_RESOLVED_EVENT_DEFINITION: EventDefinition = EventDefinition {
    id: EventDefinitionId::from_bytes([0x0C; 16]),
    schema_name: "teamy_studio.startup.feature_activation_gate_resolved",
    schema_version: 1,
    log_intent: EventLogIntent::DEBUG,
};

pub static TRACING_INITIALIZATION_FAILED_EVENT_DEFINITION: EventDefinition = EventDefinition {
    id: EventDefinitionId::from_bytes([0x0D; 16]),
    schema_name: "teamy_studio.startup.tracing_initialization_failed",
    schema_version: 1,
    log_intent: EventLogIntent::ERROR,
};

pub static DEFAULT_CURSOR_GALLERY_FLOW_FAILED_EVENT_DEFINITION: EventDefinition = EventDefinition {
    id: EventDefinitionId::from_bytes([0x0E; 16]),
    schema_name: "teamy_studio.startup.default_cursor_gallery_flow_failed",
    schema_version: 1,
    log_intent: EventLogIntent::ERROR,
};

pub static BOOTSTRAP_CLI_PARSE_FAILED_EVENT_DEFINITION: EventDefinition = EventDefinition {
    id: EventDefinitionId::from_bytes([0x0F; 16]),
    schema_name: "teamy_studio.startup.bootstrap_cli_parse_failed",
    schema_version: 1,
    log_intent: EventLogIntent::ERROR,
};

pub static STARTUP_LOGGING_PLAN_FAILED_EVENT_DEFINITION: EventDefinition = EventDefinition {
    id: EventDefinitionId::from_bytes([0x11; 16]),
    schema_name: "teamy_studio.startup.logging_plan_failed",
    schema_version: 1,
    log_intent: EventLogIntent::ERROR,
};

pub static REGISTRATION_VALIDATION_FAILED_EVENT_DEFINITION: EventDefinition = EventDefinition {
    id: EventDefinitionId::from_bytes([0x12; 16]),
    schema_name: "teamy_studio.startup.registration_validation_failed",
    schema_version: 1,
    log_intent: EventLogIntent::ERROR,
};

pub static TRACING_RECORD_OBSERVED_EVENT_DEFINITION: EventDefinition = EventDefinition {
    id: EventDefinitionId::from_bytes([0x13; 16]),
    schema_name: "teamy_studio.startup.tracing_record_observed",
    schema_version: 1,
    log_intent: EventLogIntent::NONE,
};

const STARTUP_BOOTSTRAP_CLI_PARSE_FAILED_REASON: &str = "startup bootstrap cli parse failed";
const STARTUP_LOGGING_PLAN_DERIVATION_FAILED_REASON: &str =
    "startup logging plan derivation failed";
const STARTUP_TRACING_INITIALIZATION_FAILED_REASON: &str = "startup tracing initialization failed";
const STARTUP_REGISTRATION_VALIDATION_FAILED_REASON: &str =
    "startup registration validation failed";
const DEFAULT_CURSOR_GALLERY_FLOW_FAILURE_REASON: &str =
    "registered triggers did not produce the default cursor-gallery window flow";
const TIMELINE_TO_TRACING_REEMIT_MARKER_FIELD: &str = "teamy.timeline_reemit";

#[distributed_slice(EVENT_DEFINITION_REGISTRATIONS)]
pub static STARTUP_SUCCEEDED_EVENT_REGISTRATION: EventDefinitionRegistration =
    EventDefinitionRegistration {
        definition: &STARTUP_SUCCEEDED_EVENT_DEFINITION,
        provenance: registration_provenance!(),
    };

#[distributed_slice(EVENT_DEFINITION_REGISTRATIONS)]
pub static STARTUP_FAILED_EVENT_REGISTRATION: EventDefinitionRegistration =
    EventDefinitionRegistration {
        definition: &STARTUP_FAILED_EVENT_DEFINITION,
        provenance: registration_provenance!(),
    };

#[distributed_slice(EVENT_DEFINITION_REGISTRATIONS)]
pub static PROCESS_STARTUP_OBSERVED_EVENT_REGISTRATION: EventDefinitionRegistration =
    EventDefinitionRegistration {
        definition: &PROCESS_STARTUP_OBSERVED_EVENT_DEFINITION,
        provenance: registration_provenance!(),
    };

#[distributed_slice(EVENT_DEFINITION_REGISTRATIONS)]
pub static STARTUP_GLOBAL_ARGS_PARSED_EVENT_REGISTRATION: EventDefinitionRegistration =
    EventDefinitionRegistration {
        definition: &STARTUP_GLOBAL_ARGS_PARSED_EVENT_DEFINITION,
        provenance: registration_provenance!(),
    };

#[distributed_slice(EVENT_DEFINITION_REGISTRATIONS)]
pub static STARTUP_LOGGING_CONFIGURED_EVENT_REGISTRATION: EventDefinitionRegistration =
    EventDefinitionRegistration {
        definition: &STARTUP_LOGGING_CONFIGURED_EVENT_DEFINITION,
        provenance: registration_provenance!(),
    };

#[distributed_slice(EVENT_DEFINITION_REGISTRATIONS)]
pub static TRACING_INITIALIZED_EVENT_REGISTRATION: EventDefinitionRegistration =
    EventDefinitionRegistration {
        definition: &TRACING_INITIALIZED_EVENT_DEFINITION,
        provenance: registration_provenance!(),
    };

#[distributed_slice(EVENT_DEFINITION_REGISTRATIONS)]
pub static REGISTRATION_VALIDATION_STARTED_EVENT_REGISTRATION: EventDefinitionRegistration =
    EventDefinitionRegistration {
        definition: &REGISTRATION_VALIDATION_STARTED_EVENT_DEFINITION,
        provenance: registration_provenance!(),
    };

#[distributed_slice(EVENT_DEFINITION_REGISTRATIONS)]
pub static REGISTRATION_VALIDATION_COMPLETED_EVENT_REGISTRATION: EventDefinitionRegistration =
    EventDefinitionRegistration {
        definition: &REGISTRATION_VALIDATION_COMPLETED_EVENT_DEFINITION,
        provenance: registration_provenance!(),
    };

#[distributed_slice(EVENT_DEFINITION_REGISTRATIONS)]
pub static FEATURE_COMPATIBILITY_VALIDATION_STARTED_EVENT_REGISTRATION:
    EventDefinitionRegistration = EventDefinitionRegistration {
    definition: &FEATURE_COMPATIBILITY_VALIDATION_STARTED_EVENT_DEFINITION,
    provenance: registration_provenance!(),
};

#[distributed_slice(EVENT_DEFINITION_REGISTRATIONS)]
pub static FEATURE_COMPATIBILITY_VALIDATED_EVENT_REGISTRATION: EventDefinitionRegistration =
    EventDefinitionRegistration {
        definition: &FEATURE_COMPATIBILITY_VALIDATED_EVENT_DEFINITION,
        provenance: registration_provenance!(),
    };

#[distributed_slice(EVENT_DEFINITION_REGISTRATIONS)]
pub static FEATURE_COMPATIBILITY_VALIDATION_COMPLETED_EVENT_REGISTRATION:
    EventDefinitionRegistration = EventDefinitionRegistration {
    definition: &FEATURE_COMPATIBILITY_VALIDATION_COMPLETED_EVENT_DEFINITION,
    provenance: registration_provenance!(),
};

#[distributed_slice(EVENT_DEFINITION_REGISTRATIONS)]
pub static FEATURE_ACTIVATION_GATE_RESOLVED_EVENT_REGISTRATION: EventDefinitionRegistration =
    EventDefinitionRegistration {
        definition: &FEATURE_ACTIVATION_GATE_RESOLVED_EVENT_DEFINITION,
        provenance: registration_provenance!(),
    };

#[distributed_slice(EVENT_DEFINITION_REGISTRATIONS)]
pub static TRACING_INITIALIZATION_FAILED_EVENT_REGISTRATION: EventDefinitionRegistration =
    EventDefinitionRegistration {
        definition: &TRACING_INITIALIZATION_FAILED_EVENT_DEFINITION,
        provenance: registration_provenance!(),
    };

#[distributed_slice(EVENT_DEFINITION_REGISTRATIONS)]
pub static DEFAULT_CURSOR_GALLERY_FLOW_FAILED_EVENT_REGISTRATION: EventDefinitionRegistration =
    EventDefinitionRegistration {
        definition: &DEFAULT_CURSOR_GALLERY_FLOW_FAILED_EVENT_DEFINITION,
        provenance: registration_provenance!(),
    };

#[distributed_slice(EVENT_DEFINITION_REGISTRATIONS)]
pub static BOOTSTRAP_CLI_PARSE_FAILED_EVENT_REGISTRATION: EventDefinitionRegistration =
    EventDefinitionRegistration {
        definition: &BOOTSTRAP_CLI_PARSE_FAILED_EVENT_DEFINITION,
        provenance: registration_provenance!(),
    };

#[distributed_slice(EVENT_DEFINITION_REGISTRATIONS)]
pub static STARTUP_LOGGING_PLAN_FAILED_EVENT_REGISTRATION: EventDefinitionRegistration =
    EventDefinitionRegistration {
        definition: &STARTUP_LOGGING_PLAN_FAILED_EVENT_DEFINITION,
        provenance: registration_provenance!(),
    };

#[distributed_slice(EVENT_DEFINITION_REGISTRATIONS)]
pub static REGISTRATION_VALIDATION_FAILED_EVENT_REGISTRATION: EventDefinitionRegistration =
    EventDefinitionRegistration {
        definition: &REGISTRATION_VALIDATION_FAILED_EVENT_DEFINITION,
        provenance: registration_provenance!(),
    };

#[distributed_slice(EVENT_DEFINITION_REGISTRATIONS)]
pub static TRACING_RECORD_OBSERVED_EVENT_REGISTRATION: EventDefinitionRegistration =
    EventDefinitionRegistration {
        definition: &TRACING_RECORD_OBSERVED_EVENT_DEFINITION,
        provenance: registration_provenance!(),
    };

#[derive(Clone, Debug, Eq, Facet, PartialEq)]
pub struct StartupSucceededEvent {
    pub completed_at: CanonicalTimeKey,
    pub prior_epoch_count: u64,
}

#[derive(Clone, Debug, Eq, Facet, PartialEq)]
pub struct StartupFailedEvent {
    pub failed_at: CanonicalTimeKey,
    pub prior_epoch_count: u64,
    pub reason: &'static str,
    pub failure_references: Vec<EventReference>,
}

#[derive(Clone, Debug, Eq, Facet, PartialEq)]
pub struct RegistrationValidationStartedEvent {
    pub event_definition_count: u64,
    pub trigger_count: u64,
}

#[derive(Clone, Debug, Eq, Facet, PartialEq)]
pub struct RegistrationValidationCompletedEvent {
    pub event_definition_count: u64,
    pub trigger_count: u64,
}

#[derive(Clone, Debug, Eq, Facet, PartialEq)]
pub struct FeatureCompatibilityValidationStartedEvent {
    pub feature_count: u64,
}

#[derive(Clone, Debug, Eq, Facet, PartialEq)]
pub struct FeatureCompatibilityValidatedEvent {
    pub feature_id: FeatureId,
    pub feature_title: &'static str,
}

#[derive(Clone, Debug, Eq, Facet, PartialEq)]
pub struct FeatureCompatibilityValidationCompletedEvent {
    pub feature_count: u64,
    pub pending_count: u64,
    pub validated_count: u64,
    pub failed_count: u64,
}

#[derive(Clone, Debug, Eq, Facet, PartialEq)]
pub struct FeatureActivationGateResolvedEvent {
    pub feature_id: FeatureId,
    pub feature_title: &'static str,
    pub allows_activation: bool,
}

#[derive(Clone, Debug, Eq, Facet, PartialEq)]
pub struct TracingInitializationFailedEvent {
    pub effective_filter_directive: String,
    pub json_log_path: Option<String>,
    pub reason: String,
}

#[derive(Clone, Debug, Eq, Facet, PartialEq)]
pub struct DefaultCursorGalleryFlowFailedEvent {
    pub emitted_epoch_count: u64,
    pub cursor_gallery_window_count: u64,
    pub shell_window_count: u64,
}

#[derive(Clone, Debug, Eq, Facet, PartialEq)]
pub struct BootstrapCliParseFailedEvent {
    pub argv: Vec<String>,
    pub missing_value_flag: Option<String>,
    pub unrecognized_argument: Option<String>,
}

#[derive(Clone, Debug, Eq, Facet, PartialEq)]
pub struct StartupLoggingPlanFailedEvent {
    pub debug: bool,
    pub log_filter: Option<String>,
    pub log_file: Option<String>,
    pub rust_log: Option<String>,
    pub conflicting_log_filter: Option<String>,
}

#[derive(Clone, Debug, Eq, Facet, PartialEq)]
pub struct RegistrationValidationFailedEvent {
    pub reason: String,
    pub feature_id: Option<FeatureId>,
    pub event_definition_id: Option<EventDefinitionId>,
    pub trigger_registration_id: Option<TriggerRegistrationId>,
    pub trigger_definition_id: Option<TriggerDefinitionId>,
    pub trigger_name: Option<&'static str>,
    pub first_provenance: Option<RegistrationProvenance>,
    pub duplicate_provenance: Option<RegistrationProvenance>,
    pub trigger_provenance: Option<RegistrationProvenance>,
}

#[derive(Debug)]
pub struct AppComposition {
    pub bootstrap_plan: StartupBootstrapPlan,
    pub registration_snapshot: RegistrationSnapshot,
    pub menu_snapshot: MainMenuSnapshot,
    pub cursor_gallery_state: CursorGalleryState,
    pub shell_hosted_windows: Vec<HostedWindowRecord>,
    pub shell_state: ShellState,
    pub timeline: ConstructedTimeline<PublishedEvent>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TracingObservationSummary {
    pub total_observed_records: usize,
    pub counts_by_level: BTreeMap<String, usize>,
    pub contains_timeline_reemit_marker: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct StartupSmokeSummary {
    pub tracing_observations: TracingObservationSummary,
    pub contains_startup_succeeded_event: bool,
    pub contains_startup_failed_event: bool,
    pub latest_startup_failure_reason: Option<String>,
    pub cursor_gallery_window_count: usize,
    pub shell_hosted_window_count: usize,
    pub total_published_epoch_count: usize,
}

#[derive(Debug)]
pub struct ObservedStartupSessionFailure {
    error: Report,
    runtime: StartupRuntime,
}

#[derive(Debug)]
pub struct ObservedDefaultCursorGalleryFlowFailure {
    error: Report,
    session: StartupSession,
}

#[derive(Debug)]
pub struct ObservedStartupSmokeFailure {
    error: Report,
    summary: StartupSmokeSummary,
}

fn startup_smoke_summary_from_runtime(
    timeline: &ConstructedTimeline<PublishedEvent>,
    cursor_gallery_state: &CursorGalleryState,
    shell_hosted_window_count: usize,
) -> StartupSmokeSummary {
    let mut summary = StartupSmokeSummary {
        cursor_gallery_window_count: cursor_gallery_state.windows().len(),
        shell_hosted_window_count,
        total_published_epoch_count: timeline.published_epochs().len(),
        ..StartupSmokeSummary::default()
    };

    for (_, epoch) in timeline.published_epochs() {
        for event in epoch.events() {
            if let Some(observed_record) = event.downcast_ref::<TracingRecordObservedEvent>() {
                summary.tracing_observations.total_observed_records += 1;
                *summary
                    .tracing_observations
                    .counts_by_level
                    .entry(observed_record.level.clone())
                    .or_default() += 1;
                if observed_record
                    .fields
                    .iter()
                    .any(|field| field.name == TIMELINE_TO_TRACING_REEMIT_MARKER_FIELD)
                {
                    summary.tracing_observations.contains_timeline_reemit_marker = true;
                }
            }

            if event.definition().id == STARTUP_SUCCEEDED_EVENT_DEFINITION.id {
                summary.contains_startup_succeeded_event = true;
            }

            if let Some(startup_failed) = event.downcast_ref::<StartupFailedEvent>() {
                summary.contains_startup_failed_event = true;
                summary.latest_startup_failure_reason = Some(startup_failed.reason.to_owned());
            }
        }
    }

    summary
}

impl AppComposition {
    #[must_use]
    pub fn bootstrap_plan(&self) -> &StartupBootstrapPlan {
        &self.bootstrap_plan
    }

    #[must_use]
    pub fn published_event_records(&self) -> Vec<(TriggerCursor, EventId, &PublishedEvent)> {
        self.timeline.published_event_records()
    }

    #[must_use]
    pub fn event_references(&self) -> Vec<EventReference> {
        self.timeline.event_references()
    }

    #[must_use]
    pub fn latest_event_references(&self) -> Vec<EventReference> {
        self.timeline.latest_event_references()
    }

    #[must_use]
    pub fn tracing_observation_summary(&self) -> TracingObservationSummary {
        self.startup_smoke_summary().tracing_observations
    }

    #[must_use]
    pub fn startup_smoke_summary(&self) -> StartupSmokeSummary {
        startup_smoke_summary_from_runtime(
            &self.timeline,
            &self.cursor_gallery_state,
            self.shell_hosted_windows.len(),
        )
    }
}

impl ObservedStartupSessionFailure {
    #[must_use]
    pub fn runtime(&self) -> &StartupRuntime {
        &self.runtime
    }

    #[must_use]
    pub fn startup_smoke_summary(&self) -> StartupSmokeSummary {
        self.runtime.startup_smoke_summary()
    }
}

impl Display for ObservedStartupSessionFailure {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.error, formatter)
    }
}

impl Error for ObservedStartupSessionFailure {}

impl ObservedDefaultCursorGalleryFlowFailure {
    #[must_use]
    pub fn session(&self) -> &StartupSession {
        &self.session
    }

    #[must_use]
    pub fn startup_smoke_summary(&self) -> StartupSmokeSummary {
        self.session.startup_smoke_summary()
    }
}

impl Display for ObservedDefaultCursorGalleryFlowFailure {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.error, formatter)
    }
}

impl Error for ObservedDefaultCursorGalleryFlowFailure {}

impl ObservedStartupSmokeFailure {
    #[must_use]
    pub fn summary(&self) -> &StartupSmokeSummary {
        &self.summary
    }
}

impl Display for ObservedStartupSmokeFailure {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.error, formatter)
    }
}

impl Error for ObservedStartupSmokeFailure {}

fn bootstrap_cli_parse_failed_event(
    raw_inputs: &RawProcessStartupInputs,
    error: &BootstrapCliParseError,
) -> BootstrapCliParseFailedEvent {
    match error {
        BootstrapCliParseError::MissingValue { flag } => BootstrapCliParseFailedEvent {
            argv: raw_inputs.argv.clone(),
            missing_value_flag: Some((*flag).to_owned()),
            unrecognized_argument: None,
        },
        BootstrapCliParseError::UnrecognizedArgument { argument } => BootstrapCliParseFailedEvent {
            argv: raw_inputs.argv.clone(),
            missing_value_flag: None,
            unrecognized_argument: Some(argument.clone()),
        },
        BootstrapCliParseError::HelpRequested { .. }
        | BootstrapCliParseError::VersionRequested { .. }
        | BootstrapCliParseError::CompletionsRequested { .. } => BootstrapCliParseFailedEvent {
            argv: raw_inputs.argv.clone(),
            missing_value_flag: None,
            unrecognized_argument: None,
        },
    }
}

fn startup_logging_plan_failed_event(
    raw_inputs: &RawProcessStartupInputs,
    global_args: &GlobalArgs,
    error: &StartupLoggingPlanError,
) -> StartupLoggingPlanFailedEvent {
    match error {
        StartupLoggingPlanError::DebugConflictsWithExplicitFilter { filter } => {
            StartupLoggingPlanFailedEvent {
                debug: global_args.debug,
                log_filter: global_args.log_filter.clone(),
                log_file: global_args.log_file.clone(),
                rust_log: raw_inputs.rust_log.clone(),
                conflicting_log_filter: Some(filter.clone()),
            }
        }
    }
}

fn registration_validation_failed_event(
    error: &RegistrationValidationError,
) -> RegistrationValidationFailedEvent {
    match error {
        RegistrationValidationError::DuplicateFeatureId {
            feature_id,
            first_provenance,
            duplicate_provenance,
        } => RegistrationValidationFailedEvent {
            reason: error.to_string(),
            feature_id: Some(*feature_id),
            event_definition_id: None,
            trigger_registration_id: None,
            trigger_definition_id: None,
            trigger_name: None,
            first_provenance: Some(*first_provenance),
            duplicate_provenance: Some(*duplicate_provenance),
            trigger_provenance: None,
        },
        RegistrationValidationError::DuplicateEventDefinitionId {
            event_definition_id,
            first_provenance,
            duplicate_provenance,
        } => RegistrationValidationFailedEvent {
            reason: error.to_string(),
            feature_id: None,
            event_definition_id: Some(*event_definition_id),
            trigger_registration_id: None,
            trigger_definition_id: None,
            trigger_name: None,
            first_provenance: Some(*first_provenance),
            duplicate_provenance: Some(*duplicate_provenance),
            trigger_provenance: None,
        },
        RegistrationValidationError::DuplicateTriggerRegistrationId {
            trigger_registration_id,
            first_provenance,
            duplicate_provenance,
        } => RegistrationValidationFailedEvent {
            reason: error.to_string(),
            feature_id: None,
            event_definition_id: None,
            trigger_registration_id: Some(*trigger_registration_id),
            trigger_definition_id: None,
            trigger_name: None,
            first_provenance: Some(*first_provenance),
            duplicate_provenance: Some(*duplicate_provenance),
            trigger_provenance: None,
        },
        RegistrationValidationError::DuplicateTriggerDefinitionId {
            trigger_definition_id,
            first_provenance,
            duplicate_provenance,
        } => RegistrationValidationFailedEvent {
            reason: error.to_string(),
            feature_id: None,
            event_definition_id: None,
            trigger_registration_id: None,
            trigger_definition_id: Some(*trigger_definition_id),
            trigger_name: None,
            first_provenance: Some(*first_provenance),
            duplicate_provenance: Some(*duplicate_provenance),
            trigger_provenance: None,
        },
        RegistrationValidationError::TriggerOwnedByUnregisteredFeature {
            trigger_name,
            trigger_registration_id,
            trigger_definition_id,
            feature_id,
            trigger_provenance,
        } => RegistrationValidationFailedEvent {
            reason: error.to_string(),
            feature_id: Some(*feature_id),
            event_definition_id: None,
            trigger_registration_id: Some(*trigger_registration_id),
            trigger_definition_id: Some(*trigger_definition_id),
            trigger_name: Some(*trigger_name),
            first_provenance: None,
            duplicate_provenance: None,
            trigger_provenance: Some(*trigger_provenance),
        },
        RegistrationValidationError::TriggerTargetsUnregisteredDefinition {
            trigger_name,
            trigger_registration_id,
            trigger_definition_id,
            owner_feature_id,
            event_definition_id,
            trigger_provenance,
        } => RegistrationValidationFailedEvent {
            reason: error.to_string(),
            feature_id: Some(*owner_feature_id),
            event_definition_id: Some(*event_definition_id),
            trigger_registration_id: Some(*trigger_registration_id),
            trigger_definition_id: Some(*trigger_definition_id),
            trigger_name: Some(*trigger_name),
            first_provenance: None,
            duplicate_provenance: None,
            trigger_provenance: Some(*trigger_provenance),
        },
    }
}

#[derive(Debug)]
pub struct StartupSession {
    bootstrap_plan: StartupBootstrapPlan,
    registration_snapshot: RegistrationSnapshot,
    menu_snapshot: MainMenuSnapshot,
    runtime: StartupRuntime,
    tracing_observation_layer: Option<TracingObservationLayer>,
}

#[derive(Debug)]
pub struct ObservedBootstrapPlan {
    bootstrap_plan: StartupBootstrapPlan,
    runtime: StartupRuntime,
}

#[derive(Debug)]
pub struct ObservedBootstrapPlanFailure {
    error: BootstrapPlanError,
    runtime: StartupRuntime,
}

#[derive(Debug)]
pub struct StartupRuntime {
    timeline: ConstructedTimeline<PublishedEvent>,
    trigger_runtime: TriggerRuntime,
    cursor_gallery_state: CursorGalleryState,
    shell_runtime: ShellRuntime,
}

impl Default for StartupRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl ObservedBootstrapPlan {
    #[must_use]
    pub fn bootstrap_plan(&self) -> &StartupBootstrapPlan {
        &self.bootstrap_plan
    }

    #[must_use]
    pub fn runtime(&self) -> &StartupRuntime {
        &self.runtime
    }

    #[must_use]
    pub fn published_event_records(&self) -> Vec<(TriggerCursor, EventId, &PublishedEvent)> {
        self.runtime.published_event_records()
    }

    #[must_use]
    pub fn event_references(&self) -> Vec<EventReference> {
        self.runtime.event_references()
    }

    #[must_use]
    pub fn latest_event_references(&self) -> Vec<EventReference> {
        self.runtime.latest_event_references()
    }

    pub fn into_mvp_session(self) -> Result<StartupSession> {
        build_mvp_session_with_runtime(self.bootstrap_plan, self.runtime, true)
    }

    #[expect(
        clippy::result_large_err,
        reason = "smoke-only observed session construction preserves runtime state so failure summaries can inspect the startup timeline"
    )]
    pub fn into_mvp_session_observed(
        self,
    ) -> std::result::Result<StartupSession, ObservedStartupSessionFailure> {
        build_mvp_session_with_runtime_observed(self.bootstrap_plan, self.runtime, true)
    }

    #[must_use]
    pub fn into_parts(self) -> (StartupBootstrapPlan, StartupRuntime) {
        (self.bootstrap_plan, self.runtime)
    }
}

impl ObservedBootstrapPlanFailure {
    #[must_use]
    pub fn error(&self) -> &BootstrapPlanError {
        &self.error
    }

    #[must_use]
    pub fn runtime(&self) -> &StartupRuntime {
        &self.runtime
    }

    #[must_use]
    pub fn published_event_records(&self) -> Vec<(TriggerCursor, EventId, &PublishedEvent)> {
        self.runtime.published_event_records()
    }

    #[must_use]
    pub fn event_references(&self) -> Vec<EventReference> {
        self.runtime.event_references()
    }

    #[must_use]
    pub fn latest_event_references(&self) -> Vec<EventReference> {
        self.runtime.latest_event_references()
    }

    #[must_use]
    pub fn into_parts(self) -> (BootstrapPlanError, StartupRuntime) {
        (self.error, self.runtime)
    }
}

impl Display for ObservedBootstrapPlanFailure {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.error, formatter)
    }
}

impl Error for ObservedBootstrapPlanFailure {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.error)
    }
}

fn reemit_log_worthy_event_to_tracing(
    arena_name: &'static str,
    time_key: CanonicalTimeKey,
    event: &PublishedEvent,
) {
    let definition = event.definition();
    let Some(level) = definition.log_intent.level else {
        return;
    };

    let timeline_reemit_marker = true;
    let event_schema_name = definition.schema_name;
    let event_schema_version = definition.schema_version;
    let time_key_femtoseconds = time_key.raw_femtoseconds();

    match level {
        EventLogLevel::Trace => event!(
            Level::TRACE,
            teamy.timeline_reemit = timeline_reemit_marker,
            marker_field = TIMELINE_TO_TRACING_REEMIT_MARKER_FIELD,
            arena_name,
            event_definition_id = ?definition.id,
            event_schema_name,
            event_schema_version,
            time_key_femtoseconds,
            "timeline event re-emitted to tracing"
        ),
        EventLogLevel::Debug => event!(
            Level::DEBUG,
            teamy.timeline_reemit = timeline_reemit_marker,
            marker_field = TIMELINE_TO_TRACING_REEMIT_MARKER_FIELD,
            arena_name,
            event_definition_id = ?definition.id,
            event_schema_name,
            event_schema_version,
            time_key_femtoseconds,
            "timeline event re-emitted to tracing"
        ),
        EventLogLevel::Info => event!(
            Level::INFO,
            teamy.timeline_reemit = timeline_reemit_marker,
            marker_field = TIMELINE_TO_TRACING_REEMIT_MARKER_FIELD,
            arena_name,
            event_definition_id = ?definition.id,
            event_schema_name,
            event_schema_version,
            time_key_femtoseconds,
            "timeline event re-emitted to tracing"
        ),
        EventLogLevel::Warn => event!(
            Level::WARN,
            teamy.timeline_reemit = timeline_reemit_marker,
            marker_field = TIMELINE_TO_TRACING_REEMIT_MARKER_FIELD,
            arena_name,
            event_definition_id = ?definition.id,
            event_schema_name,
            event_schema_version,
            time_key_femtoseconds,
            "timeline event re-emitted to tracing"
        ),
        EventLogLevel::Error => event!(
            Level::ERROR,
            teamy.timeline_reemit = timeline_reemit_marker,
            marker_field = TIMELINE_TO_TRACING_REEMIT_MARKER_FIELD,
            arena_name,
            event_definition_id = ?definition.id,
            event_schema_name,
            event_schema_version,
            time_key_femtoseconds,
            "timeline event re-emitted to tracing"
        ),
    }
}

impl StartupRuntime {
    #[must_use]
    pub fn new() -> Self {
        Self::new_with_shell_runtime(ShellRuntime::new())
    }

    #[must_use]
    pub fn new_with_native_shell() -> Self {
        Self::new_with_shell_runtime(ShellRuntime::new_with_native_hosting())
    }

    #[must_use]
    pub fn new_with_shell_runtime(shell_runtime: ShellRuntime) -> Self {
        Self {
            timeline: ConstructedTimeline::new(),
            trigger_runtime: TriggerRuntime::default(),
            cursor_gallery_state: CursorGalleryState::default(),
            shell_runtime,
        }
    }

    pub fn publish(
        &mut self,
        arena_name: &'static str,
        time_key: CanonicalTimeKey,
        event: PublishedEvent,
    ) {
        reemit_log_worthy_event_to_tracing(arena_name, time_key, &event);
        let mut epoch = WritableArena::new(arena_name);
        epoch.push(event);
        self.timeline.ingest(time_key, epoch.seal());
    }

    #[must_use]
    pub fn next_publish_time_key(&self) -> CanonicalTimeKey {
        self.timeline.published_event_records().last().map_or(
            CanonicalTimeKey::ZERO,
            |(cursor, _, _)| {
                CanonicalTimeKey::from_femtoseconds(cursor.time_key.raw_femtoseconds() + 1)
            },
        )
    }

    pub fn publish_startup_succeeded(&mut self, time_key: CanonicalTimeKey) {
        let event = StartupSucceededEvent {
            completed_at: time_key,
            prior_epoch_count: self.timeline.published_epochs().len() as u64,
        };
        self.publish(
            "teamy_studio.startup",
            time_key,
            PublishedEvent::new(&STARTUP_SUCCEEDED_EVENT_DEFINITION, event),
        );
    }

    pub fn publish_startup_failed(
        &mut self,
        time_key: CanonicalTimeKey,
        reason: &'static str,
        failure_references: Vec<EventReference>,
    ) {
        let event = StartupFailedEvent {
            failed_at: time_key,
            prior_epoch_count: self.timeline.published_epochs().len() as u64,
            reason,
            failure_references,
        };
        self.publish(
            "teamy_studio.startup",
            time_key,
            PublishedEvent::new(&STARTUP_FAILED_EVENT_DEFINITION, event),
        );
    }

    pub fn publish_startup_failed_from_latest_references(
        &mut self,
        time_key: CanonicalTimeKey,
        reason: &'static str,
        max_references: usize,
    ) {
        let mut failure_references = self.event_references();
        if failure_references.len() > max_references {
            let split_index = failure_references.len() - max_references;
            failure_references.drain(0..split_index);
        }
        self.publish_startup_failed(time_key, reason, failure_references);
    }

    pub fn publish_startup_failed_from_latest_references_now(
        &mut self,
        reason: &'static str,
        max_references: usize,
    ) {
        let time_key = self.next_publish_time_key();
        self.publish_startup_failed_from_latest_references(time_key, reason, max_references);
    }

    pub fn publish_bootstrap_plan_events(&mut self, bootstrap_plan: &StartupBootstrapPlan) {
        let observed_time_key = self.next_publish_time_key();
        self.publish(
            "teamy_studio.startup.bootstrap",
            observed_time_key,
            PublishedEvent::new(
                &PROCESS_STARTUP_OBSERVED_EVENT_DEFINITION,
                bootstrap_plan.raw_inputs.to_observed_event(),
            ),
        );

        let parsed_args_time_key = self.next_publish_time_key();
        self.publish(
            "teamy_studio.startup.bootstrap",
            parsed_args_time_key,
            PublishedEvent::new(
                &STARTUP_GLOBAL_ARGS_PARSED_EVENT_DEFINITION,
                bootstrap_plan.global_args.to_parsed_event(),
            ),
        );

        let logging_time_key = self.next_publish_time_key();
        self.publish(
            "teamy_studio.startup.bootstrap",
            logging_time_key,
            PublishedEvent::new(
                &STARTUP_LOGGING_CONFIGURED_EVENT_DEFINITION,
                bootstrap_plan.logging.to_configured_event(),
            ),
        );
    }

    pub fn publish_tracing_initialized(&mut self, tracing_initialized: TracingInitializedEvent) {
        let time_key = self.next_publish_time_key();
        self.publish(
            "teamy_studio.startup.bootstrap",
            time_key,
            PublishedEvent::new(&TRACING_INITIALIZED_EVENT_DEFINITION, tracing_initialized),
        );
    }

    pub fn publish_tracing_observed_records(
        &mut self,
        observed_records: Vec<TracingRecordObservedEvent>,
    ) -> usize {
        let mut published_count = 0;
        for observed_record in observed_records {
            let time_key = self.next_publish_time_key();
            self.publish(
                "teamy_studio.startup.tracing_observation",
                time_key,
                PublishedEvent::new(&TRACING_RECORD_OBSERVED_EVENT_DEFINITION, observed_record),
            );
            published_count += 1;
        }
        published_count
    }

    pub fn publish_tracing_initialization_failed(
        &mut self,
        bootstrap_plan: &StartupBootstrapPlan,
        reason: String,
    ) {
        let time_key = self.next_publish_time_key();
        self.publish(
            "teamy_studio.startup.bootstrap",
            time_key,
            PublishedEvent::new(
                &TRACING_INITIALIZATION_FAILED_EVENT_DEFINITION,
                TracingInitializationFailedEvent {
                    effective_filter_directive: bootstrap_plan.logging.effective_filter_directive(),
                    json_log_path: bootstrap_plan
                        .logging
                        .json_log_path
                        .as_ref()
                        .map(|path| path.to_string_lossy().into_owned()),
                    reason,
                },
            ),
        );
    }

    pub fn publish_default_cursor_gallery_flow_failed(
        &mut self,
        emitted_epoch_count: usize,
        cursor_gallery_window_count: usize,
        shell_window_count: usize,
    ) {
        let time_key = self.next_publish_time_key();
        self.publish(
            "teamy_studio.startup",
            time_key,
            PublishedEvent::new(
                &DEFAULT_CURSOR_GALLERY_FLOW_FAILED_EVENT_DEFINITION,
                DefaultCursorGalleryFlowFailedEvent {
                    emitted_epoch_count: emitted_epoch_count as u64,
                    cursor_gallery_window_count: cursor_gallery_window_count as u64,
                    shell_window_count: shell_window_count as u64,
                },
            ),
        );
    }

    pub fn publish_process_startup_observed(&mut self, raw_inputs: &RawProcessStartupInputs) {
        let observed_time_key = self.next_publish_time_key();
        self.publish(
            "teamy_studio.startup.bootstrap",
            observed_time_key,
            PublishedEvent::new(
                &PROCESS_STARTUP_OBSERVED_EVENT_DEFINITION,
                raw_inputs.to_observed_event(),
            ),
        );
    }

    pub fn publish_startup_global_args_parsed(&mut self, global_args: &GlobalArgs) {
        let parsed_args_time_key = self.next_publish_time_key();
        self.publish(
            "teamy_studio.startup.bootstrap",
            parsed_args_time_key,
            PublishedEvent::new(
                &STARTUP_GLOBAL_ARGS_PARSED_EVENT_DEFINITION,
                global_args.to_parsed_event(),
            ),
        );
    }

    pub fn publish_startup_logging_configured(&mut self, logging: &StartupLoggingPlan) {
        let logging_time_key = self.next_publish_time_key();
        self.publish(
            "teamy_studio.startup.bootstrap",
            logging_time_key,
            PublishedEvent::new(
                &STARTUP_LOGGING_CONFIGURED_EVENT_DEFINITION,
                logging.to_configured_event(),
            ),
        );
    }

    pub fn publish_bootstrap_cli_parse_failed(
        &mut self,
        raw_inputs: &RawProcessStartupInputs,
        error: &BootstrapCliParseError,
    ) {
        let time_key = self.next_publish_time_key();
        self.publish(
            "teamy_studio.startup.bootstrap",
            time_key,
            PublishedEvent::new(
                &BOOTSTRAP_CLI_PARSE_FAILED_EVENT_DEFINITION,
                bootstrap_cli_parse_failed_event(raw_inputs, error),
            ),
        );
    }

    pub fn publish_bootstrap_cli_parse_failure_outcome(
        &mut self,
        raw_inputs: &RawProcessStartupInputs,
        error: &BootstrapCliParseError,
    ) {
        self.publish_bootstrap_cli_parse_failed(raw_inputs, error);
        self.publish_startup_failed_from_latest_references_now(
            STARTUP_BOOTSTRAP_CLI_PARSE_FAILED_REASON,
            1,
        );
    }

    pub fn publish_startup_logging_plan_failed(
        &mut self,
        raw_inputs: &RawProcessStartupInputs,
        global_args: &GlobalArgs,
        error: &StartupLoggingPlanError,
    ) {
        let time_key = self.next_publish_time_key();
        self.publish(
            "teamy_studio.startup.bootstrap",
            time_key,
            PublishedEvent::new(
                &STARTUP_LOGGING_PLAN_FAILED_EVENT_DEFINITION,
                startup_logging_plan_failed_event(raw_inputs, global_args, error),
            ),
        );
    }

    pub fn publish_startup_logging_plan_failure_outcome(
        &mut self,
        raw_inputs: &RawProcessStartupInputs,
        global_args: &GlobalArgs,
        error: &StartupLoggingPlanError,
    ) {
        self.publish_startup_logging_plan_failed(raw_inputs, global_args, error);
        self.publish_startup_failed_from_latest_references_now(
            STARTUP_LOGGING_PLAN_DERIVATION_FAILED_REASON,
            1,
        );
    }

    pub fn publish_registration_validation_failed(&mut self, error: &RegistrationValidationError) {
        let time_key = self.next_publish_time_key();
        self.publish(
            "teamy_studio.startup.validation",
            time_key,
            PublishedEvent::new(
                &REGISTRATION_VALIDATION_FAILED_EVENT_DEFINITION,
                registration_validation_failed_event(error),
            ),
        );
    }

    pub fn publish_registration_validation_failure_outcome(
        &mut self,
        error: &RegistrationValidationError,
    ) {
        self.publish_registration_validation_failed(error);
        self.publish_startup_failed_from_latest_references_now(
            STARTUP_REGISTRATION_VALIDATION_FAILED_REASON,
            1,
        );
    }

    pub fn publish_tracing_initialization_failure_outcome(
        &mut self,
        bootstrap_plan: &StartupBootstrapPlan,
        reason: String,
    ) {
        self.publish_tracing_initialization_failed(bootstrap_plan, reason);
        self.publish_startup_failed_from_latest_references_now(
            STARTUP_TRACING_INITIALIZATION_FAILED_REASON,
            1,
        );
    }

    pub fn publish_default_cursor_gallery_flow_failure_outcome(
        &mut self,
        emitted_epoch_count: usize,
        cursor_gallery_window_count: usize,
        shell_window_count: usize,
    ) {
        self.publish_default_cursor_gallery_flow_failed(
            emitted_epoch_count,
            cursor_gallery_window_count,
            shell_window_count,
        );
        self.publish_startup_failed_from_latest_references_now(
            DEFAULT_CURSOR_GALLERY_FLOW_FAILURE_REASON,
            1,
        );
    }

    pub fn publish_registration_validation_started(
        &mut self,
        registration_snapshot: RegistrationSnapshot,
    ) {
        let time_key = self.next_publish_time_key();
        self.publish(
            "teamy_studio.startup.validation",
            time_key,
            PublishedEvent::new(
                &REGISTRATION_VALIDATION_STARTED_EVENT_DEFINITION,
                RegistrationValidationStartedEvent {
                    event_definition_count: registration_snapshot.event_definition_count as u64,
                    trigger_count: registration_snapshot.trigger_count as u64,
                },
            ),
        );
    }

    pub fn publish_registration_validation_completed(
        &mut self,
        registration_snapshot: RegistrationSnapshot,
    ) {
        let time_key = self.next_publish_time_key();
        self.publish(
            "teamy_studio.startup.validation",
            time_key,
            PublishedEvent::new(
                &REGISTRATION_VALIDATION_COMPLETED_EVENT_DEFINITION,
                RegistrationValidationCompletedEvent {
                    event_definition_count: registration_snapshot.event_definition_count as u64,
                    trigger_count: registration_snapshot.trigger_count as u64,
                },
            ),
        );
    }

    pub fn publish_feature_compatibility_validation_started(&mut self, feature_count: u64) {
        let time_key = self.next_publish_time_key();
        self.publish(
            "teamy_studio.startup.validation",
            time_key,
            PublishedEvent::new(
                &FEATURE_COMPATIBILITY_VALIDATION_STARTED_EVENT_DEFINITION,
                FeatureCompatibilityValidationStartedEvent { feature_count },
            ),
        );
    }

    pub fn publish_feature_compatibility_validated(
        &mut self,
        feature_id: FeatureId,
        feature_title: &'static str,
    ) {
        let time_key = self.next_publish_time_key();
        self.publish(
            "teamy_studio.startup.validation",
            time_key,
            PublishedEvent::new(
                &FEATURE_COMPATIBILITY_VALIDATED_EVENT_DEFINITION,
                FeatureCompatibilityValidatedEvent {
                    feature_id,
                    feature_title,
                },
            ),
        );
    }

    pub fn publish_feature_compatibility_validation_completed(
        &mut self,
        feature_count: u64,
        pending_count: u64,
        validated_count: u64,
        failed_count: u64,
    ) {
        let time_key = self.next_publish_time_key();
        self.publish(
            "teamy_studio.startup.validation",
            time_key,
            PublishedEvent::new(
                &FEATURE_COMPATIBILITY_VALIDATION_COMPLETED_EVENT_DEFINITION,
                FeatureCompatibilityValidationCompletedEvent {
                    feature_count,
                    pending_count,
                    validated_count,
                    failed_count,
                },
            ),
        );
    }

    pub fn publish_feature_activation_gate_resolved(
        &mut self,
        feature_id: FeatureId,
        feature_title: &'static str,
        allows_activation: bool,
    ) {
        let time_key = self.next_publish_time_key();
        self.publish(
            "teamy_studio.startup.validation",
            time_key,
            PublishedEvent::new(
                &FEATURE_ACTIVATION_GATE_RESOLVED_EVENT_DEFINITION,
                FeatureActivationGateResolvedEvent {
                    feature_id,
                    feature_title,
                    allows_activation,
                },
            ),
        );
    }

    pub fn run_registered_trigger_stage(&mut self, time_key: CanonicalTimeKey) -> usize {
        let mut emitted_events = Vec::new();
        self.trigger_runtime
            .pump_unseen_records(&self.timeline, |_, _, event| {
                for registration in trigger_registrations_for(event.definition_id()) {
                    emitted_events.extend((registration.handler)(event));
                }
                Ok::<(), ()>(())
            })
            .expect("registered trigger stage should be infallible");

        if emitted_events.is_empty() {
            return 0;
        }

        for event in &emitted_events {
            teamy_studio_cursor_gallery::apply_published_event(
                &mut self.cursor_gallery_state,
                event,
            );
            self.shell_runtime.apply_published_event(event);
        }

        let mut epoch = WritableArena::new("teamy_studio.trigger_runtime");
        for event in emitted_events {
            epoch.push(event);
        }
        self.timeline.ingest(time_key, epoch.seal());
        1
    }

    pub fn run_shell_host_stage(&mut self, time_key: CanonicalTimeKey) -> Result<usize> {
        let emitted_epoch_count = self
            .shell_runtime
            .run_host_stage(&mut self.timeline, time_key)?;

        if emitted_epoch_count == 0 {
            return Ok(0);
        }

        let latest_epoch = self
            .timeline
            .published_epochs()
            .last()
            .expect("shell runtime emitted an epoch but timeline is empty");
        for event in latest_epoch.1.events() {
            teamy_studio_cursor_gallery::apply_published_event(
                &mut self.cursor_gallery_state,
                event,
            );
        }

        Ok(emitted_epoch_count)
    }

    #[must_use]
    pub fn cursor_gallery_state(&self) -> &CursorGalleryState {
        &self.cursor_gallery_state
    }

    #[must_use]
    pub fn shell_state(&self) -> &ShellState {
        self.shell_runtime.state()
    }

    #[must_use]
    pub fn shell_hosted_windows(&self) -> &[HostedWindowRecord] {
        self.shell_runtime.hosted_windows()
    }

    #[must_use]
    pub fn published_event_records(&self) -> Vec<(TriggerCursor, EventId, &PublishedEvent)> {
        self.timeline.published_event_records()
    }

    #[must_use]
    pub fn event_references(&self) -> Vec<EventReference> {
        self.timeline.event_references()
    }

    #[must_use]
    pub fn latest_event_references(&self) -> Vec<EventReference> {
        self.timeline.latest_event_references()
    }

    #[must_use]
    pub fn startup_smoke_summary(&self) -> StartupSmokeSummary {
        startup_smoke_summary_from_runtime(
            &self.timeline,
            &self.cursor_gallery_state,
            self.shell_runtime.hosted_windows().len(),
        )
    }

    #[must_use]
    pub fn into_parts(
        self,
    ) -> (
        ConstructedTimeline<PublishedEvent>,
        CursorGalleryState,
        Vec<HostedWindowRecord>,
        ShellState,
    ) {
        let (shell_host_scaffold, shell_state) = self.shell_runtime.into_parts();

        (
            self.timeline,
            self.cursor_gallery_state,
            shell_host_scaffold.hosted_windows().to_vec(),
            shell_state,
        )
    }
}

#[expect(
    clippy::result_large_err,
    reason = "smoke-only observed session construction preserves runtime state so failure summaries can inspect the startup timeline"
)]
fn build_mvp_session_with_runtime_observed(
    bootstrap_plan: StartupBootstrapPlan,
    mut runtime: StartupRuntime,
    bootstrap_events_already_published: bool,
) -> std::result::Result<StartupSession, ObservedStartupSessionFailure> {
    if !bootstrap_events_already_published {
        runtime.publish_bootstrap_plan_events(&bootstrap_plan);
    }
    let pre_validation_snapshot = snapshot();
    runtime.publish_registration_validation_started(pre_validation_snapshot);
    if let Err(error) = validate_registrations() {
        runtime.publish_registration_validation_failure_outcome(&error);
        return Err(ObservedStartupSessionFailure {
            error: error.into(),
            runtime,
        });
    }
    let registration_snapshot = snapshot();
    runtime.publish_registration_validation_completed(registration_snapshot);

    let button_classes = registered_button_classes();
    let mut menu_snapshot = MainMenuSnapshot::from_registrations(&button_classes);
    runtime.publish_feature_compatibility_validation_started(
        registration_snapshot.feature_count as u64,
    );
    menu_snapshot.set_validation_state_for_class_id(
        CURSOR_GALLERY_BUTTON_CLASS_ID,
        FeatureValidationState::Validated,
    );
    let cursor_gallery_button =
        match menu_snapshot.button_by_class_id(CURSOR_GALLERY_BUTTON_CLASS_ID) {
            Some(button) => button,
            None => {
                return Err(ObservedStartupSessionFailure {
                    error: eyre!("cursor gallery button was not registered for feature validation"),
                    runtime,
                });
            }
        };
    runtime.publish_feature_compatibility_validated(
        CURSOR_GALLERY_FEATURE_ID,
        cursor_gallery_button.title,
    );
    runtime.publish_feature_compatibility_validation_completed(
        registration_snapshot.feature_count as u64,
        0,
        1,
        0,
    );
    runtime.publish_feature_activation_gate_resolved(
        CURSOR_GALLERY_FEATURE_ID,
        cursor_gallery_button.title,
        cursor_gallery_button.validation_state == FeatureValidationState::Validated,
    );

    Ok(StartupSession {
        bootstrap_plan,
        registration_snapshot,
        menu_snapshot,
        runtime,
        tracing_observation_layer: None,
    })
}

impl StartupSession {
    #[must_use]
    pub fn bootstrap_plan(&self) -> &StartupBootstrapPlan {
        &self.bootstrap_plan
    }

    #[must_use]
    pub fn registration_snapshot(&self) -> RegistrationSnapshot {
        self.registration_snapshot
    }

    #[must_use]
    pub fn menu_snapshot(&self) -> &MainMenuSnapshot {
        &self.menu_snapshot
    }

    pub fn publish_event(
        &mut self,
        arena_name: &'static str,
        time_key: CanonicalTimeKey,
        event: PublishedEvent,
    ) {
        trace!(
            arena_name,
            time_key = time_key.raw_femtoseconds(),
            schema_name = event.definition().schema_name,
            schema_version = event.definition().schema_version,
            definition_id = ?event.definition_id(),
            "publishing timeline event"
        );
        self.runtime.publish(arena_name, time_key, event);
    }

    pub fn publish_startup_failed(
        &mut self,
        time_key: CanonicalTimeKey,
        reason: &'static str,
        failure_references: Vec<EventReference>,
    ) {
        self.runtime
            .publish_startup_failed(time_key, reason, failure_references);
    }

    pub fn publish_startup_failed_from_latest_references(
        &mut self,
        time_key: CanonicalTimeKey,
        reason: &'static str,
        max_references: usize,
    ) {
        self.runtime.publish_startup_failed_from_latest_references(
            time_key,
            reason,
            max_references,
        );
    }

    pub fn publish_main_menu_click(
        &mut self,
        logical_button_id: MainMenuLogicalButtonId,
        pointer_x: i32,
        pointer_y: i32,
        layout_revision: u64,
        time_key: CanonicalTimeKey,
    ) -> Result<()> {
        let click_event = self
            .menu_snapshot
            .publish_click(logical_button_id, pointer_x, pointer_y, layout_revision)
            .ok_or_else(|| eyre!("requested main menu button could not publish a click event"))?;
        self.publish_event("teamy_studio.main_menu", time_key, click_event);
        Ok(())
    }

    pub fn pump_to_idle(
        &mut self,
        start_time_key: CanonicalTimeKey,
        max_stages: usize,
    ) -> Result<usize> {
        let mut emitted_epoch_count = 0;
        let mut next_time_key = start_time_key.raw_femtoseconds();
        for _ in 0..max_stages {
            let registered_emitted = self
                .runtime
                .run_registered_trigger_stage(CanonicalTimeKey::from_femtoseconds(next_time_key));
            emitted_epoch_count += registered_emitted;
            next_time_key += 1;

            let shell_emitted = self
                .runtime
                .run_shell_host_stage(CanonicalTimeKey::from_femtoseconds(next_time_key))?;
            emitted_epoch_count += shell_emitted;
            next_time_key += 1;

            let tracing_observation_emitted = self.run_tracing_observation_stage();
            emitted_epoch_count += tracing_observation_emitted;
            next_time_key += 1;

            if registered_emitted == 0 && shell_emitted == 0 && tracing_observation_emitted == 0 {
                break;
            }
        }
        Ok(emitted_epoch_count)
    }

    pub fn run_tracing_observation_stage(&mut self) -> usize {
        let Some(tracing_observation_layer) = &self.tracing_observation_layer else {
            return 0;
        };
        self.runtime.publish_tracing_observed_records(
            tracing_observation_layer.drain_observed_records(TRACING_OBSERVATION_DRAIN_LIMIT),
        )
    }

    #[must_use]
    pub fn cursor_gallery_state(&self) -> &CursorGalleryState {
        self.runtime.cursor_gallery_state()
    }

    #[must_use]
    pub fn shell_state(&self) -> &ShellState {
        self.runtime.shell_state()
    }

    #[must_use]
    pub fn shell_hosted_windows(&self) -> &[HostedWindowRecord] {
        self.runtime.shell_hosted_windows()
    }

    #[must_use]
    pub fn published_event_records(&self) -> Vec<(TriggerCursor, EventId, &PublishedEvent)> {
        self.runtime.published_event_records()
    }

    #[must_use]
    pub fn event_references(&self) -> Vec<EventReference> {
        self.runtime.event_references()
    }

    #[must_use]
    pub fn latest_event_references(&self) -> Vec<EventReference> {
        self.runtime.latest_event_references()
    }

    #[must_use]
    pub fn startup_smoke_summary(&self) -> StartupSmokeSummary {
        self.runtime.startup_smoke_summary()
    }

    #[must_use]
    pub fn into_composition(self) -> AppComposition {
        let (timeline, cursor_gallery_state, shell_hosted_windows, shell_state) =
            self.runtime.into_parts();

        AppComposition {
            bootstrap_plan: self.bootstrap_plan,
            registration_snapshot: self.registration_snapshot,
            menu_snapshot: self.menu_snapshot,
            cursor_gallery_state,
            shell_hosted_windows,
            shell_state,
            timeline,
        }
    }

    pub fn destroy_native_windows(&mut self) -> Result<()> {
        self.runtime.shell_runtime.destroy_native_windows()
    }
}

pub fn build_mvp_session() -> Result<StartupSession> {
    build_mvp_session_from_bootstrap_plan(StartupBootstrapPlan::empty())
}

pub fn build_mvp_session_with_native_shell() -> Result<StartupSession> {
    build_mvp_session_with_native_shell_from_bootstrap_plan(StartupBootstrapPlan::empty())
}

#[expect(
    clippy::result_large_err,
    reason = "bootstrap derivation failures intentionally return the populated startup runtime so callers can inspect the pre-session timeline"
)]
pub fn observe_bootstrap_plan_from_raw_inputs(
    raw_inputs: RawProcessStartupInputs,
) -> std::result::Result<ObservedBootstrapPlan, ObservedBootstrapPlanFailure> {
    observe_bootstrap_plan_from_raw_inputs_with_runtime(raw_inputs, StartupRuntime::new())
}

#[expect(
    clippy::result_large_err,
    reason = "bootstrap derivation failures intentionally return the populated startup runtime so callers can inspect the pre-session timeline"
)]
pub fn observe_bootstrap_plan_from_raw_inputs_with_native_shell(
    raw_inputs: RawProcessStartupInputs,
) -> std::result::Result<ObservedBootstrapPlan, ObservedBootstrapPlanFailure> {
    observe_bootstrap_plan_from_raw_inputs_with_runtime(
        raw_inputs,
        StartupRuntime::new_with_native_shell(),
    )
}

pub fn build_mvp_session_from_raw_inputs(
    raw_inputs: RawProcessStartupInputs,
) -> Result<StartupSession> {
    observe_bootstrap_plan_from_raw_inputs(raw_inputs)
        .map_err(|error| eyre!(error.to_string()))?
        .into_mvp_session()
}

pub fn build_mvp_session_with_native_shell_from_raw_inputs(
    raw_inputs: RawProcessStartupInputs,
) -> Result<StartupSession> {
    observe_bootstrap_plan_from_raw_inputs_with_native_shell(raw_inputs)
        .map_err(|error| eyre!(error.to_string()))?
        .into_mvp_session()
}

pub fn build_mvp_session_from_bootstrap_plan(
    bootstrap_plan: StartupBootstrapPlan,
) -> Result<StartupSession> {
    build_mvp_session_with_runtime(bootstrap_plan, StartupRuntime::new(), false)
}

pub fn build_mvp_session_with_native_shell_from_bootstrap_plan(
    bootstrap_plan: StartupBootstrapPlan,
) -> Result<StartupSession> {
    build_mvp_session_with_runtime(
        bootstrap_plan,
        StartupRuntime::new_with_native_shell(),
        false,
    )
}

fn build_mvp_session_with_runtime(
    bootstrap_plan: StartupBootstrapPlan,
    runtime: StartupRuntime,
    bootstrap_events_already_published: bool,
) -> Result<StartupSession> {
    build_mvp_session_with_runtime_observed(
        bootstrap_plan,
        runtime,
        bootstrap_events_already_published,
    )
    .map_err(|error| error.error)
}

#[expect(
    clippy::result_large_err,
    reason = "bootstrap derivation failures intentionally return the populated startup runtime so callers can inspect the pre-session timeline"
)]
fn observe_bootstrap_plan_from_raw_inputs_with_runtime(
    raw_inputs: RawProcessStartupInputs,
    mut runtime: StartupRuntime,
) -> std::result::Result<ObservedBootstrapPlan, ObservedBootstrapPlanFailure> {
    let bootstrap_plan =
        match derive_bootstrap_plan_with_runtime(raw_inputs, chrono::Local::now(), &mut runtime) {
            Ok(bootstrap_plan) => bootstrap_plan,
            Err(error) => {
                return Err(ObservedBootstrapPlanFailure { error, runtime });
            }
        };
    Ok(ObservedBootstrapPlan {
        bootstrap_plan,
        runtime,
    })
}

fn derive_bootstrap_plan_with_runtime(
    raw_inputs: RawProcessStartupInputs,
    now: chrono::DateTime<chrono::Local>,
    runtime: &mut StartupRuntime,
) -> std::result::Result<StartupBootstrapPlan, BootstrapPlanError> {
    runtime.publish_process_startup_observed(&raw_inputs);

    let parsed_cli = match parse_bootstrap_cli_args(&raw_inputs.argv_refs()) {
        Ok(parsed_cli) => parsed_cli,
        Err(error) => {
            if !error.is_builtin_request() {
                runtime.publish_bootstrap_cli_parse_failure_outcome(&raw_inputs, &error);
            }
            return Err(BootstrapPlanError::CliParse(error));
        }
    };

    runtime.publish_startup_global_args_parsed(&parsed_cli.global_args);

    let logging = match StartupLoggingPlan::from_global_args(
        &parsed_cli.global_args,
        raw_inputs.rust_log.as_deref(),
        now,
    ) {
        Ok(logging) => logging,
        Err(error) => {
            runtime.publish_startup_logging_plan_failure_outcome(
                &raw_inputs,
                &parsed_cli.global_args,
                &error,
            );
            return Err(BootstrapPlanError::LoggingPlan(error));
        }
    };

    runtime.publish_startup_logging_configured(&logging);

    Ok(StartupBootstrapPlan {
        raw_inputs,
        global_args: parsed_cli.global_args,
        logging,
    })
}

pub fn build_mvp_composition() -> Result<AppComposition> {
    let session = build_mvp_session()?;
    run_default_cursor_gallery_flow(session)
}

pub fn build_mvp_composition_with_native_shell() -> Result<AppComposition> {
    let session = build_mvp_session_with_native_shell()?;
    run_default_cursor_gallery_flow(session)
}

pub fn build_mvp_composition_from_raw_inputs(
    raw_inputs: RawProcessStartupInputs,
) -> Result<AppComposition> {
    let session = build_mvp_session_from_raw_inputs(raw_inputs)?;
    run_default_cursor_gallery_flow(session)
}

#[expect(
    clippy::result_large_err,
    reason = "smoke-only observed cursor-gallery flow preserves the session so failure summaries can inspect emitted startup events"
)]
fn run_default_cursor_gallery_flow_observed(
    mut session: StartupSession,
) -> std::result::Result<AppComposition, ObservedDefaultCursorGalleryFlowFailure> {
    let clicked_button = match session
        .menu_snapshot()
        .button_by_class_id(CURSOR_GALLERY_BUTTON_CLASS_ID)
        .cloned()
    {
        Some(clicked_button) => clicked_button,
        None => {
            return Err(ObservedDefaultCursorGalleryFlowFailure {
                error: eyre!("cursor gallery button was not registered for the MVP composition"),
                session,
            });
        }
    };

    let click_time_key = session.runtime.next_publish_time_key();
    if let Err(error) =
        session.publish_main_menu_click(clicked_button.logical_button_id, 64, 32, 1, click_time_key)
    {
        return Err(ObservedDefaultCursorGalleryFlowFailure { error, session });
    }
    let pump_start_time_key = session.runtime.next_publish_time_key();
    let emitted_epoch_count = match session.pump_to_idle(pump_start_time_key, 8) {
        Ok(emitted_epoch_count) => emitted_epoch_count,
        Err(error) => {
            return Err(ObservedDefaultCursorGalleryFlowFailure { error, session });
        }
    };

    if emitted_epoch_count == 0
        || session.cursor_gallery_state().windows().is_empty()
        || session.shell_state().windows().is_empty()
    {
        session
            .runtime
            .publish_default_cursor_gallery_flow_failure_outcome(
                emitted_epoch_count,
                session.cursor_gallery_state().windows().len(),
                session.shell_state().windows().len(),
            );
        return Err(ObservedDefaultCursorGalleryFlowFailure {
            error: eyre!(DEFAULT_CURSOR_GALLERY_FLOW_FAILURE_REASON),
            session,
        });
    }

    let success_time_key = session.runtime.next_publish_time_key();
    session.runtime.publish_startup_succeeded(success_time_key);

    Ok(session.into_composition())
}

fn run_default_cursor_gallery_flow(session: StartupSession) -> Result<AppComposition> {
    run_default_cursor_gallery_flow_observed(session).map_err(|error| error.error)
}

pub fn build_startup_smoke_summary_from_raw_inputs(
    raw_inputs: RawProcessStartupInputs,
) -> std::result::Result<StartupSmokeSummary, ObservedStartupSmokeFailure> {
    let observed_bootstrap = match observe_bootstrap_plan_from_raw_inputs(raw_inputs) {
        Ok(observed_bootstrap) => observed_bootstrap,
        Err(error) => {
            return Err(ObservedStartupSmokeFailure {
                error: eyre!(error.to_string()),
                summary: error.runtime().startup_smoke_summary(),
            });
        }
    };
    let mut session = match observed_bootstrap.into_mvp_session_observed() {
        Ok(session) => session,
        Err(error) => {
            return Err(ObservedStartupSmokeFailure {
                error: eyre!(error.to_string()),
                summary: error.startup_smoke_summary(),
            });
        }
    };
    if let Err(error) = initialize_tracing_for_session(&mut session) {
        return Err(ObservedStartupSmokeFailure {
            error,
            summary: session.startup_smoke_summary(),
        });
    }
    info!("running startup smoke composition");
    match run_default_cursor_gallery_flow_observed(session) {
        Ok(composition) => Ok(composition.startup_smoke_summary()),
        Err(error) => Err(ObservedStartupSmokeFailure {
            error: eyre!(error.to_string()),
            summary: error.startup_smoke_summary(),
        }),
    }
}

fn format_startup_smoke_summary(summary: &StartupSmokeSummary) -> String {
    let mut output = String::new();
    output.push_str("Startup smoke summary\n");
    output.push_str(&format!(
        "contains_startup_succeeded_event: {}\n",
        summary.contains_startup_succeeded_event
    ));
    output.push_str(&format!(
        "contains_startup_failed_event: {}\n",
        summary.contains_startup_failed_event
    ));
    output.push_str(&format!(
        "latest_startup_failure_reason: {}\n",
        summary
            .latest_startup_failure_reason
            .as_deref()
            .unwrap_or("none")
    ));
    output.push_str(&format!(
        "cursor_gallery_window_count: {}\n",
        summary.cursor_gallery_window_count
    ));
    output.push_str(&format!(
        "shell_hosted_window_count: {}\n",
        summary.shell_hosted_window_count
    ));
    output.push_str(&format!(
        "total_published_epoch_count: {}\n",
        summary.total_published_epoch_count
    ));
    output.push_str("Tracing observation summary\n");
    output.push_str(&format!(
        "total_observed_records: {}\n",
        summary.tracing_observations.total_observed_records
    ));
    output.push_str(&format!(
        "contains_timeline_reemit_marker: {}\n",
        summary.tracing_observations.contains_timeline_reemit_marker
    ));
    for (level, count) in &summary.tracing_observations.counts_by_level {
        output.push_str(&format!("level.{level}: {count}\n"));
    }
    output
}

pub fn main() -> Result<()> {
    main_with_raw_inputs(RawProcessStartupInputs::capture_from_env())
}

const TRACING_OBSERVATION_DRAIN_LIMIT: usize = 64;

fn initialize_tracing_for_session(session: &mut StartupSession) -> Result<()> {
    match initialize_tracing_with_observation_from_bootstrap_plan(session.bootstrap_plan()) {
        Ok(tracing_initialization) => {
            session
                .runtime
                .publish_tracing_initialized(tracing_initialization.initialized_event);
            session.tracing_observation_layer = Some(tracing_initialization.observation_layer);
            session.run_tracing_observation_stage();
            Ok(())
        }
        Err(error) => {
            let bootstrap_plan = session.bootstrap_plan().clone();
            session
                .runtime
                .publish_tracing_initialization_failure_outcome(&bootstrap_plan, error.to_string());
            Err(error)
        }
    }
}

fn startup_smoke_requested(raw_inputs: &RawProcessStartupInputs) -> bool {
    raw_inputs
        .argv
        .iter()
        .any(|argument| argument == "--startup-smoke")
}

fn next_main_menu_interaction_time_seed(runtime: &StartupRuntime) -> i128 {
    runtime.next_publish_time_key().raw_femtoseconds()
}

fn unimplemented_main_menu_dialog(
    class_id: MainMenuButtonClassId,
    title: &'static str,
) -> Option<(&'static str, String)> {
    let description = match class_id {
        STORAGE_BUTTON_CLASS_ID => "Storage is not implemented yet.".to_owned(),
        ENVIRONMENT_VARIABLES_BUTTON_CLASS_ID => {
            "The environment-variable inspector is not implemented yet.".to_owned()
        }
        APPLICATION_WINDOWS_BUTTON_CLASS_ID => {
            "The application-window inspector is not implemented yet.".to_owned()
        }
        TERMINAL_BUTTON_CLASS_ID
        | CURSOR_INFO_BUTTON_CLASS_ID
        | TEXT_RENDERING_PLAYGROUND_BUTTON_CLASS_ID
        | DEMO_MODE_BUTTON_CLASS_ID
        | AUDIO_PICKER_BUTTON_CLASS_ID
        | AUDIO_DAEMON_BUTTON_CLASS_ID
        | JOBS_BUTTON_CLASS_ID
        | LOGS_BUTTON_CLASS_ID
        | AUDIO_DEVICES_BUTTON_CLASS_ID
        | TIMELINE_BUTTON_CLASS_ID
        | TIMELINE_PLAYGROUND_BUTTON_CLASS_ID => format!("{title} is not implemented yet."),
        _ => return None,
    };

    Some((title, description))
}

pub fn main_with_raw_inputs(raw_inputs: RawProcessStartupInputs) -> Result<()> {
    if try_handle_builtin_bootstrap_cli_request(&raw_inputs.argv_refs())? {
        return Ok(());
    }

    if startup_smoke_requested(&raw_inputs) {
        match build_startup_smoke_summary_from_raw_inputs(raw_inputs) {
            Ok(summary) => {
                print!("{}", format_startup_smoke_summary(&summary));
                return Ok(());
            }
            Err(error) => {
                print!("{}", format_startup_smoke_summary(error.summary()));
                return Err(error.into());
            }
        }
    }

    let mut session = observe_bootstrap_plan_from_raw_inputs_with_native_shell(raw_inputs)
        .map_err(|error| eyre!(error.to_string()))?
        .into_mvp_session()?;
    initialize_tracing_for_session(&mut session)?;
    teamy_studio_shell::initialize_dpi_awareness();
    let menu_snapshot = session.menu_snapshot().clone();
    let mut next_time_key = next_main_menu_interaction_time_seed(&session.runtime);

    teamy_studio_main_menu::run_native_main_menu_window_with_click_handler(
        &menu_snapshot,
        |logical_button_id| {
            let clicked_button = menu_snapshot
                .buttons()
                .iter()
                .find(|button| button.logical_button_id == logical_button_id)
                .map(|button| (button.class_id, button.title));
            if let Some((class_id, title)) = clicked_button {
                info!(
                    ?logical_button_id,
                    ?class_id,
                    title,
                    "startup received main menu click callback"
                );
                if let Some((title, description)) = unimplemented_main_menu_dialog(class_id, title)
                {
                    info!(title, description, "showing unimplemented main menu dialog");
                    show_main_menu_info_dialog(title, &description)?;
                    return Ok(());
                }
            }
            info!(
                ?logical_button_id,
                time_key = next_time_key,
                "publishing main menu click event"
            );
            session.publish_main_menu_click(
                logical_button_id,
                0,
                0,
                1,
                CanonicalTimeKey::from_femtoseconds(next_time_key),
            )?;
            next_time_key += 16;
            let emitted_epoch_count =
                session.pump_to_idle(CanonicalTimeKey::from_femtoseconds(next_time_key), 8)?;
            info!(
                ?logical_button_id,
                emitted_epoch_count, next_time_key, "pumped main menu click to idle"
            );
            next_time_key += 16;
            Ok(())
        },
    )
}

#[cfg(test)]
mod tests {
    use super::{
        AppComposition, BOOTSTRAP_CLI_PARSE_FAILED_EVENT_DEFINITION, BootstrapCliParseFailedEvent,
        DEFAULT_CURSOR_GALLERY_FLOW_FAILED_EVENT_DEFINITION, DefaultCursorGalleryFlowFailedEvent,
        FEATURE_ACTIVATION_GATE_RESOLVED_EVENT_DEFINITION,
        FEATURE_COMPATIBILITY_VALIDATED_EVENT_DEFINITION,
        FEATURE_COMPATIBILITY_VALIDATION_COMPLETED_EVENT_DEFINITION,
        FEATURE_COMPATIBILITY_VALIDATION_STARTED_EVENT_DEFINITION,
        FeatureActivationGateResolvedEvent, FeatureCompatibilityValidatedEvent,
        FeatureCompatibilityValidationCompletedEvent, FeatureCompatibilityValidationStartedEvent,
        PROCESS_STARTUP_OBSERVED_EVENT_DEFINITION, ProcessStartupObservedEvent,
        REGISTRATION_VALIDATION_COMPLETED_EVENT_DEFINITION,
        REGISTRATION_VALIDATION_FAILED_EVENT_DEFINITION,
        REGISTRATION_VALIDATION_STARTED_EVENT_DEFINITION, RawProcessStartupInputs,
        RegistrationValidationCompletedEvent, RegistrationValidationFailedEvent,
        RegistrationValidationStartedEvent, STARTUP_FAILED_EVENT_DEFINITION,
        STARTUP_GLOBAL_ARGS_PARSED_EVENT_DEFINITION, STARTUP_LOGGING_CONFIGURED_EVENT_DEFINITION,
        STARTUP_LOGGING_PLAN_FAILED_EVENT_DEFINITION, STARTUP_SUCCEEDED_EVENT_DEFINITION,
        StartupBootstrapPlan, StartupFailedEvent, StartupGlobalArgsParsedEvent,
        StartupLoggingConfiguredEvent, StartupLoggingPlanFailedEvent, StartupRuntime,
        StartupSession, StartupSucceededEvent, TRACING_INITIALIZATION_FAILED_EVENT_DEFINITION,
        TRACING_INITIALIZED_EVENT_DEFINITION, TRACING_OBSERVATION_DRAIN_LIMIT,
        TRACING_RECORD_OBSERVED_EVENT_DEFINITION, TracingInitializationFailedEvent,
        TracingInitializedEvent, TracingObservationLayer, TracingObservedField,
        TracingRecordObservedEvent, build_mvp_composition, build_mvp_composition_from_raw_inputs,
        build_mvp_session, build_mvp_session_from_bootstrap_plan,
        build_mvp_session_from_raw_inputs, build_mvp_session_with_native_shell,
        build_startup_smoke_summary_from_raw_inputs, derive_bootstrap_plan,
        derive_bootstrap_plan_with_runtime, format_startup_smoke_summary,
        initialize_tracing_for_session, next_main_menu_interaction_time_seed,
        observe_bootstrap_plan_from_raw_inputs, unimplemented_main_menu_dialog,
    };
    use teamy_studio_cursor_gallery::{
        CURSOR_GALLERY_BUTTON_CLASS_ID, CURSOR_GALLERY_OPEN_INTENT_DEFINITION,
    };
    use teamy_studio_event_core::{EventLogIntent, EventLogLevel};
    use teamy_studio_launcher_catalog::{
        ENVIRONMENT_VARIABLES_BUTTON_CLASS_ID, TERMINAL_BUTTON_CLASS_ID,
    };
    use teamy_studio_main_menu::{
        FeatureValidationState, MAIN_MENU_CLICKED_EVENT_DEFINITION, MainMenuSnapshot,
    };
    use teamy_studio_registration_core::{
        FeatureId, RegistrationSnapshot, RegistrationValidationError, TriggerDefinitionId,
        TriggerRegistrationId, registration_provenance,
    };
    use teamy_studio_shell::{
        InitialWindowCommand, RendererHostMode, WINDOW_CREATE_REQUEST_EVENT_DEFINITION,
        WINDOW_CREATED_EVENT_DEFINITION, WindowHostOptions,
    };
    use teamy_studio_timeline_core::CanonicalTimeKey;
    use tracing_subscriber::prelude::*;

    #[test]
    fn environment_variables_uses_legacy_placeholder_copy() {
        let dialog = unimplemented_main_menu_dialog(
            ENVIRONMENT_VARIABLES_BUTTON_CLASS_ID,
            "Environment Variables",
        )
        .expect("environment variables should show placeholder dialog");

        assert_eq!(dialog.0, "Environment Variables");
        assert_eq!(
            dialog.1,
            "The environment-variable inspector is not implemented yet."
        );
    }

    #[test]
    fn generic_unimplemented_buttons_get_title_based_placeholder_copy() {
        let dialog = unimplemented_main_menu_dialog(TERMINAL_BUTTON_CLASS_ID, "Terminal")
            .expect("terminal should show placeholder dialog until implemented");

        assert_eq!(dialog.0, "Terminal");
        assert_eq!(dialog.1, "Terminal is not implemented yet.");
    }

    #[test]
    fn startup_event_definitions_carry_log_intent_for_reemission() {
        assert_eq!(
            PROCESS_STARTUP_OBSERVED_EVENT_DEFINITION.log_intent,
            EventLogIntent::TRACE
        );
        assert_eq!(
            REGISTRATION_VALIDATION_FAILED_EVENT_DEFINITION
                .log_intent
                .level,
            Some(EventLogLevel::Error)
        );
        assert_eq!(
            STARTUP_SUCCEEDED_EVENT_DEFINITION.log_intent.level,
            Some(EventLogLevel::Info)
        );
        assert_eq!(
            CURSOR_GALLERY_OPEN_INTENT_DEFINITION.log_intent,
            EventLogIntent::NONE
        );
        assert_eq!(
            TRACING_RECORD_OBSERVED_EVENT_DEFINITION.log_intent,
            EventLogIntent::NONE
        );
    }

    #[test]
    fn cursor_gallery_does_not_use_unimplemented_placeholder_dialog() {
        assert!(
            unimplemented_main_menu_dialog(CURSOR_GALLERY_BUTTON_CLASS_ID, "Cursor Gallery")
                .is_none()
        );
    }

    #[test]
    fn mvp_composition_publishes_pure_event_chain() {
        let composition: AppComposition =
            build_mvp_composition().expect("mvp composition should build");
        let published = composition.timeline.published_epochs();
        let published_records = composition.published_event_records();
        let event_references = composition.event_references();
        let latest_event_references = composition.latest_event_references();

        assert_eq!(composition.menu_snapshot.buttons().len(), 16);
        assert_eq!(
            composition
                .menu_snapshot
                .button_by_class_id(teamy_studio_cursor_gallery::CURSOR_GALLERY_BUTTON_CLASS_ID)
                .expect("cursor gallery button should exist")
                .validation_state,
            FeatureValidationState::Validated
        );
        assert_eq!(
            composition.menu_snapshot.buttons()[0].validation_state,
            FeatureValidationState::Pending
        );
        assert!(
            composition
                .cursor_gallery_state
                .pending_window_ids()
                .is_empty()
        );
        assert_eq!(composition.cursor_gallery_state.windows().len(), 1);
        assert_eq!(composition.shell_hosted_windows.len(), 1);
        assert_eq!(
            composition.shell_hosted_windows[0].hosted_window_id.raw(),
            1
        );
        assert_eq!(
            composition.shell_hosted_windows[0].host_options,
            WindowHostOptions::standard_foreground()
        );
        assert_eq!(
            composition.shell_hosted_windows[0]
                .native_host_plan
                .initial_command,
            InitialWindowCommand::Show
        );
        assert!(
            composition.shell_hosted_windows[0]
                .native_host_plan
                .bring_to_front
        );
        assert_eq!(
            composition.shell_hosted_windows[0].renderer_host_mode,
            RendererHostMode::Composed
        );
        assert!(composition.shell_state.pending_requests().is_empty());
        assert_eq!(composition.shell_state.windows().len(), 1);
        assert!(composition.bootstrap_plan().raw_inputs.argv.is_empty());
        assert_eq!(published.len(), 14);
        assert_eq!(published_records.len(), 14);
        assert_eq!(event_references.len(), 14);
        assert_eq!(latest_event_references.len(), 1);
        assert_eq!(published[0].1.events().len(), 1);
        assert_eq!(published[1].1.events().len(), 1);
        assert_eq!(published[2].1.events().len(), 1);
        assert_eq!(published[3].1.events().len(), 1);
        assert_eq!(published[4].1.events().len(), 1);
        assert_eq!(published[5].1.events().len(), 1);
        assert_eq!(published[6].1.events().len(), 1);
        assert_eq!(published[7].1.events().len(), 1);
        assert_eq!(published[8].1.events().len(), 1);
        assert_eq!(published[9].1.events().len(), 1);
        assert_eq!(published[10].1.events().len(), 1);
        assert_eq!(published[11].1.events().len(), 1);
        assert_eq!(published[12].1.events().len(), 1);
        assert_eq!(published[13].1.events().len(), 1);
        assert_eq!(published_records[0].0.time_key.raw_femtoseconds(), 0);
        assert_eq!(published_records[1].0.time_key.raw_femtoseconds(), 1);
        assert_eq!(published_records[2].0.time_key.raw_femtoseconds(), 2);
        assert_eq!(published_records[3].0.time_key.raw_femtoseconds(), 3);
        assert_eq!(published_records[4].0.time_key.raw_femtoseconds(), 4);
        assert_eq!(published_records[5].0.time_key.raw_femtoseconds(), 5);
        assert_eq!(published_records[6].0.time_key.raw_femtoseconds(), 6);
        assert_eq!(published_records[7].0.time_key.raw_femtoseconds(), 7);
        assert_eq!(published_records[8].0.time_key.raw_femtoseconds(), 8);
        assert_eq!(published_records[9].0.time_key.raw_femtoseconds(), 9);
        assert_eq!(published_records[10].0.time_key.raw_femtoseconds(), 10);
        assert_eq!(published_records[11].0.time_key.raw_femtoseconds(), 12);
        assert_eq!(published_records[12].0.time_key.raw_femtoseconds(), 13);
        assert_eq!(published_records[13].0.time_key.raw_femtoseconds(), 14);
        assert_eq!(
            published[0].1.events()[0].definition().id,
            PROCESS_STARTUP_OBSERVED_EVENT_DEFINITION.id
        );
        assert_eq!(
            published[1].1.events()[0].definition().id,
            STARTUP_GLOBAL_ARGS_PARSED_EVENT_DEFINITION.id
        );
        assert_eq!(
            published[2].1.events()[0].definition().id,
            STARTUP_LOGGING_CONFIGURED_EVENT_DEFINITION.id
        );
        assert_eq!(
            published[3].1.events()[0].definition().id,
            REGISTRATION_VALIDATION_STARTED_EVENT_DEFINITION.id
        );
        assert_eq!(
            published[4].1.events()[0].definition().id,
            REGISTRATION_VALIDATION_COMPLETED_EVENT_DEFINITION.id
        );
        assert_eq!(
            published[5].1.events()[0].definition().id,
            FEATURE_COMPATIBILITY_VALIDATION_STARTED_EVENT_DEFINITION.id
        );
        assert_eq!(
            published[6].1.events()[0].definition().id,
            FEATURE_COMPATIBILITY_VALIDATED_EVENT_DEFINITION.id
        );
        assert_eq!(
            published[7].1.events()[0].definition().id,
            FEATURE_COMPATIBILITY_VALIDATION_COMPLETED_EVENT_DEFINITION.id
        );
        assert_eq!(
            published[8].1.events()[0].definition().id,
            FEATURE_ACTIVATION_GATE_RESOLVED_EVENT_DEFINITION.id
        );
        assert_eq!(
            published[9].1.events()[0].definition().id,
            MAIN_MENU_CLICKED_EVENT_DEFINITION.id
        );
        assert_eq!(
            published[10].1.events()[0].definition().id,
            CURSOR_GALLERY_OPEN_INTENT_DEFINITION.id
        );
        assert_eq!(
            published[11].1.events()[0].definition().id,
            WINDOW_CREATE_REQUEST_EVENT_DEFINITION.id
        );
        assert_eq!(
            published[12].1.events()[0].definition().id,
            WINDOW_CREATED_EVENT_DEFINITION.id
        );
        assert_eq!(
            published[13].1.events()[0].definition().id,
            STARTUP_SUCCEEDED_EVENT_DEFINITION.id
        );
        assert_eq!(
            event_references[0].event_definition_id,
            PROCESS_STARTUP_OBSERVED_EVENT_DEFINITION.id
        );
        assert_eq!(
            event_references[1].event_definition_id,
            STARTUP_GLOBAL_ARGS_PARSED_EVENT_DEFINITION.id
        );
        assert_eq!(
            event_references[2].event_definition_id,
            STARTUP_LOGGING_CONFIGURED_EVENT_DEFINITION.id
        );
        assert_eq!(
            event_references[3].event_definition_id,
            REGISTRATION_VALIDATION_STARTED_EVENT_DEFINITION.id
        );
        assert_eq!(
            event_references[4].event_definition_id,
            REGISTRATION_VALIDATION_COMPLETED_EVENT_DEFINITION.id
        );
        assert_eq!(
            event_references[5].event_definition_id,
            FEATURE_COMPATIBILITY_VALIDATION_STARTED_EVENT_DEFINITION.id
        );
        assert_eq!(
            event_references[6].event_definition_id,
            FEATURE_COMPATIBILITY_VALIDATED_EVENT_DEFINITION.id
        );
        assert_eq!(
            event_references[7].event_definition_id,
            FEATURE_COMPATIBILITY_VALIDATION_COMPLETED_EVENT_DEFINITION.id
        );
        assert_eq!(
            event_references[8].event_definition_id,
            FEATURE_ACTIVATION_GATE_RESOLVED_EVENT_DEFINITION.id
        );
        assert_eq!(
            event_references[9].event_definition_id,
            MAIN_MENU_CLICKED_EVENT_DEFINITION.id
        );
        assert_eq!(
            event_references[10].event_definition_id,
            CURSOR_GALLERY_OPEN_INTENT_DEFINITION.id
        );
        assert_eq!(
            event_references[11].event_definition_id,
            WINDOW_CREATE_REQUEST_EVENT_DEFINITION.id
        );
        assert_eq!(
            event_references[12].event_definition_id,
            WINDOW_CREATED_EVENT_DEFINITION.id
        );
        assert_eq!(
            event_references[13].event_definition_id,
            STARTUP_SUCCEEDED_EVENT_DEFINITION.id
        );
        assert_eq!(event_references[0].timeline_offset_hint.raw_value(), 0);
        assert_eq!(event_references[1].timeline_offset_hint.raw_value(), 1);
        assert_eq!(event_references[2].timeline_offset_hint.raw_value(), 2);
        assert_eq!(event_references[3].timeline_offset_hint.raw_value(), 3);
        assert_eq!(event_references[4].timeline_offset_hint.raw_value(), 4);
        assert_eq!(event_references[5].timeline_offset_hint.raw_value(), 5);
        assert_eq!(event_references[6].timeline_offset_hint.raw_value(), 6);
        assert_eq!(event_references[7].timeline_offset_hint.raw_value(), 7);
        assert_eq!(event_references[8].timeline_offset_hint.raw_value(), 8);
        assert_eq!(event_references[9].timeline_offset_hint.raw_value(), 9);
        assert_eq!(event_references[10].timeline_offset_hint.raw_value(), 10);
        assert_eq!(event_references[11].timeline_offset_hint.raw_value(), 12);
        assert_eq!(event_references[12].timeline_offset_hint.raw_value(), 13);
        assert_eq!(event_references[13].timeline_offset_hint.raw_value(), 14);
        assert_eq!(latest_event_references[0], event_references[13]);

        let process_startup_observed = published[0].1.events()[0]
            .downcast_ref::<ProcessStartupObservedEvent>()
            .expect("bootstrap observed payload should be present");
        assert!(process_startup_observed.argv.is_empty());
        assert_eq!(process_startup_observed.rust_log, None);

        let startup_global_args_parsed = published[1].1.events()[0]
            .downcast_ref::<StartupGlobalArgsParsedEvent>()
            .expect("bootstrap parsed-args payload should be present");
        assert!(!startup_global_args_parsed.debug);
        assert_eq!(startup_global_args_parsed.log_filter, None);
        assert_eq!(startup_global_args_parsed.log_file, None);

        let startup_logging_configured = published[2].1.events()[0]
            .downcast_ref::<StartupLoggingConfiguredEvent>()
            .expect("bootstrap logging payload should be present");
        assert_eq!(
            startup_logging_configured.effective_filter_directive,
            "info"
        );
        assert_eq!(startup_logging_configured.json_log_path, None);

        let registration_validation_started = published[3].1.events()[0]
            .downcast_ref::<RegistrationValidationStartedEvent>()
            .expect("registration validation start payload should be present");
        assert!(registration_validation_started.event_definition_count >= 7);
        assert!(registration_validation_started.trigger_count >= 1);

        let registration_validation_completed = published[4].1.events()[0]
            .downcast_ref::<RegistrationValidationCompletedEvent>()
            .expect("registration validation completion payload should be present");
        assert_eq!(
            registration_validation_completed.event_definition_count,
            registration_validation_started.event_definition_count
        );
        assert_eq!(
            registration_validation_completed.trigger_count,
            registration_validation_started.trigger_count
        );

        let feature_validation_started = published[5].1.events()[0]
            .downcast_ref::<FeatureCompatibilityValidationStartedEvent>()
            .expect("feature validation start payload should be present");
        assert_eq!(feature_validation_started.feature_count, 1);

        let feature_validated = published[6].1.events()[0]
            .downcast_ref::<FeatureCompatibilityValidatedEvent>()
            .expect("feature validated payload should be present");
        assert_eq!(
            feature_validated.feature_id,
            teamy_studio_cursor_gallery::CURSOR_GALLERY_FEATURE_ID
        );
        assert_eq!(feature_validated.feature_title, "Cursor Gallery");

        let feature_validation_completed = published[7].1.events()[0]
            .downcast_ref::<FeatureCompatibilityValidationCompletedEvent>()
            .expect("feature validation completion payload should be present");
        assert_eq!(feature_validation_completed.feature_count, 1);
        assert_eq!(feature_validation_completed.pending_count, 0);
        assert_eq!(feature_validation_completed.validated_count, 1);
        assert_eq!(feature_validation_completed.failed_count, 0);

        let feature_activation_gate = published[8].1.events()[0]
            .downcast_ref::<FeatureActivationGateResolvedEvent>()
            .expect("feature activation gate payload should be present");
        assert_eq!(
            feature_activation_gate.feature_id,
            teamy_studio_cursor_gallery::CURSOR_GALLERY_FEATURE_ID
        );
        assert_eq!(feature_activation_gate.feature_title, "Cursor Gallery");
        assert!(feature_activation_gate.allows_activation);

        let startup_succeeded = published[13].1.events()[0]
            .downcast_ref::<StartupSucceededEvent>()
            .expect("startup success payload should be present");
        assert_eq!(startup_succeeded.completed_at.raw_femtoseconds(), 14);
        assert_eq!(startup_succeeded.prior_epoch_count, 13);
    }

    #[test]
    fn mvp_session_can_accept_external_menu_clicks() {
        let mut session = build_mvp_session().expect("mvp session should build");
        let button = session
            .menu_snapshot()
            .button_by_class_id(teamy_studio_cursor_gallery::CURSOR_GALLERY_BUTTON_CLASS_ID)
            .expect("cursor gallery button should exist")
            .clone();

        session
            .publish_main_menu_click(
                button.logical_button_id,
                11,
                22,
                7,
                CanonicalTimeKey::from_femtoseconds(50),
            )
            .expect("session should publish menu clicks");

        let emitted_epoch_count = session
            .pump_to_idle(CanonicalTimeKey::from_femtoseconds(51), 8)
            .expect("session should pump to idle");
        let session_record_times = session
            .published_event_records()
            .into_iter()
            .map(|(cursor, _, _)| cursor.time_key.raw_femtoseconds())
            .collect::<Vec<_>>();
        let session_reference_definitions = session
            .event_references()
            .into_iter()
            .map(|reference| {
                (
                    reference.event_definition_id,
                    reference.timeline_offset_hint.raw_value(),
                )
            })
            .collect::<Vec<_>>();
        let latest_session_reference_definitions = session
            .latest_event_references()
            .into_iter()
            .map(|reference| {
                (
                    reference.event_definition_id,
                    reference.timeline_offset_hint.raw_value(),
                )
            })
            .collect::<Vec<_>>();

        assert_eq!(session.shell_hosted_windows().len(), 1);
        assert_eq!(
            session.shell_hosted_windows()[0].host_options,
            WindowHostOptions::standard_foreground()
        );
        assert_eq!(
            session.shell_hosted_windows()[0]
                .native_host_plan
                .initial_command,
            InitialWindowCommand::Show
        );
        assert_eq!(
            session.shell_hosted_windows()[0].renderer_host_mode,
            RendererHostMode::Composed
        );

        let composition = session.into_composition();

        assert!(emitted_epoch_count >= 1);
        assert_eq!(
            session_record_times,
            vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 50, 51, 53, 54]
        );
        assert_eq!(
            session_reference_definitions,
            vec![
                (PROCESS_STARTUP_OBSERVED_EVENT_DEFINITION.id, 0),
                (STARTUP_GLOBAL_ARGS_PARSED_EVENT_DEFINITION.id, 1),
                (STARTUP_LOGGING_CONFIGURED_EVENT_DEFINITION.id, 2),
                (REGISTRATION_VALIDATION_STARTED_EVENT_DEFINITION.id, 3),
                (REGISTRATION_VALIDATION_COMPLETED_EVENT_DEFINITION.id, 4),
                (
                    FEATURE_COMPATIBILITY_VALIDATION_STARTED_EVENT_DEFINITION.id,
                    5
                ),
                (FEATURE_COMPATIBILITY_VALIDATED_EVENT_DEFINITION.id, 6),
                (
                    FEATURE_COMPATIBILITY_VALIDATION_COMPLETED_EVENT_DEFINITION.id,
                    7
                ),
                (FEATURE_ACTIVATION_GATE_RESOLVED_EVENT_DEFINITION.id, 8),
                (MAIN_MENU_CLICKED_EVENT_DEFINITION.id, 50),
                (CURSOR_GALLERY_OPEN_INTENT_DEFINITION.id, 51),
                (WINDOW_CREATE_REQUEST_EVENT_DEFINITION.id, 53),
                (WINDOW_CREATED_EVENT_DEFINITION.id, 54),
            ]
        );
        assert_eq!(
            latest_session_reference_definitions,
            vec![(WINDOW_CREATED_EVENT_DEFINITION.id, 54)]
        );
        assert_eq!(
            composition
                .menu_snapshot
                .button_by_class_id(teamy_studio_cursor_gallery::CURSOR_GALLERY_BUTTON_CLASS_ID)
                .expect("cursor gallery button should exist")
                .logical_button_id,
            button.logical_button_id
        );
        assert_eq!(composition.cursor_gallery_state.windows().len(), 1);
        assert_eq!(composition.shell_hosted_windows.len(), 1);
        assert_eq!(composition.shell_state.windows().len(), 1);
        assert_eq!(
            composition.timeline.published_epochs()[9]
                .0
                .time_key
                .raw_femtoseconds(),
            50
        );
        assert!(composition.bootstrap_plan().raw_inputs.argv.is_empty());
    }

    #[test]
    fn mvp_session_preserves_explicit_bootstrap_inputs() {
        let session = build_mvp_session_from_raw_inputs(RawProcessStartupInputs {
            argv: vec![
                "--debug".to_owned(),
                "--log-file".to_owned(),
                "logs/teamy.ndjson".to_owned(),
            ],
            rust_log: Some("warn".to_owned()),
        })
        .expect("session should build from explicit bootstrap inputs");

        assert_eq!(
            session.bootstrap_plan().raw_inputs.argv,
            vec![
                "--debug".to_owned(),
                "--log-file".to_owned(),
                "logs/teamy.ndjson".to_owned(),
            ]
        );
        assert_eq!(session.bootstrap_plan().global_args.debug, true);
        assert_eq!(
            session
                .bootstrap_plan()
                .logging
                .effective_filter_directive(),
            "warn"
        );
    }

    #[test]
    fn observed_bootstrap_plan_failure_preserves_runtime_timeline_for_raw_input_callers() {
        let error = observe_bootstrap_plan_from_raw_inputs(RawProcessStartupInputs {
            argv: vec!["--wat".to_owned()],
            rust_log: None,
        })
        .expect_err("unknown startup args should preserve observed runtime on failure");

        let (bootstrap_error, runtime) = error.into_parts();
        let (timeline, _, _, _) = runtime.into_parts();
        let published = timeline.published_epochs();
        let detail = published[1].1.events()[0]
            .downcast_ref::<BootstrapCliParseFailedEvent>()
            .expect("cli parse failure payload should be present");
        let startup_failure = published[2].1.events()[0]
            .downcast_ref::<StartupFailedEvent>()
            .expect("startup failure payload should be present");

        assert_eq!(
            bootstrap_error.to_string(),
            "unrecognized startup argument: --wat"
        );
        assert_eq!(published.len(), 3);
        assert_eq!(
            published[0].1.events()[0].definition().id,
            PROCESS_STARTUP_OBSERVED_EVENT_DEFINITION.id
        );
        assert_eq!(
            published[1].1.events()[0].definition().id,
            BOOTSTRAP_CLI_PARSE_FAILED_EVENT_DEFINITION.id
        );
        assert_eq!(
            published[2].1.events()[0].definition().id,
            STARTUP_FAILED_EVENT_DEFINITION.id
        );
        assert_eq!(detail.unrecognized_argument.as_deref(), Some("--wat"));
        assert_eq!(startup_failure.reason, "startup bootstrap cli parse failed");
        assert_eq!(startup_failure.failure_references.len(), 1);
        assert_eq!(
            startup_failure.failure_references[0].event_definition_id,
            BOOTSTRAP_CLI_PARSE_FAILED_EVENT_DEFINITION.id
        );
    }

    #[test]
    fn observed_bootstrap_plan_exposes_bootstrap_records_before_session_construction() {
        let observed = observe_bootstrap_plan_from_raw_inputs(RawProcessStartupInputs {
            argv: vec!["--log-filter".to_owned(), "trace".to_owned()],
            rust_log: Some("warn".to_owned()),
        })
        .expect("valid startup args should produce an observed bootstrap plan");

        let bootstrap_record_definitions = observed
            .published_event_records()
            .into_iter()
            .map(|(cursor, _, event)| (event.definition().id, cursor.time_key.raw_femtoseconds()))
            .collect::<Vec<_>>();
        let latest_bootstrap_reference_definitions = observed
            .latest_event_references()
            .into_iter()
            .map(|reference| {
                (
                    reference.event_definition_id,
                    reference.timeline_offset_hint.raw_value(),
                )
            })
            .collect::<Vec<_>>();
        let session = observed
            .into_mvp_session()
            .expect("observed bootstrap plan should build an mvp session");

        assert_eq!(
            bootstrap_record_definitions,
            vec![
                (PROCESS_STARTUP_OBSERVED_EVENT_DEFINITION.id, 0),
                (STARTUP_GLOBAL_ARGS_PARSED_EVENT_DEFINITION.id, 1),
                (STARTUP_LOGGING_CONFIGURED_EVENT_DEFINITION.id, 2),
            ]
        );
        assert_eq!(
            latest_bootstrap_reference_definitions,
            vec![(STARTUP_LOGGING_CONFIGURED_EVENT_DEFINITION.id, 2)]
        );
        assert_eq!(
            session
                .bootstrap_plan()
                .logging
                .effective_filter_directive(),
            "trace"
        );
        assert_eq!(
            session.published_event_records()[0]
                .0
                .time_key
                .raw_femtoseconds(),
            0
        );
    }

    #[test]
    fn mvp_composition_preserves_explicit_bootstrap_inputs() {
        let composition = build_mvp_composition_from_raw_inputs(RawProcessStartupInputs {
            argv: vec!["--log-filter".to_owned(), "trace".to_owned()],
            rust_log: Some("warn".to_owned()),
        })
        .expect("composition should build from explicit bootstrap inputs");

        assert_eq!(
            composition.bootstrap_plan().raw_inputs.argv,
            vec!["--log-filter".to_owned(), "trace".to_owned()]
        );
        assert_eq!(
            composition
                .bootstrap_plan()
                .logging
                .effective_filter_directive(),
            "trace"
        );
    }

    #[test]
    fn mvp_session_exposes_legacy_launcher_button_catalog() {
        let session = build_mvp_session().expect("mvp session should build");
        let titles = session
            .menu_snapshot()
            .buttons()
            .iter()
            .map(|button| button.title)
            .collect::<Vec<_>>();

        assert_eq!(
            titles,
            vec![
                "Terminal",
                "Cursor Info",
                "Cursor Gallery",
                "Cursor Latency Playground",
                "Text Rendering Playground",
                "Demo Mode",
                "Storage",
                "Environment Variables",
                "Application Windows",
                "Audio",
                "Audio Daemon",
                "Jobs",
                "Logs",
                "Audio Devices",
                "Timeline",
                "Timeline Playground",
            ]
        );
    }

    #[test]
    fn native_shell_session_starts_with_native_host_mode_and_no_windows() {
        let mut session =
            build_mvp_session_with_native_shell().expect("native-shell mvp session should build");

        assert!(session.shell_hosted_windows().is_empty());

        session
            .destroy_native_windows()
            .expect("empty native-shell session should clean up");
    }

    #[test]
    fn startup_session_can_publish_thin_failure_events_with_explicit_backlinks() {
        let mut session = build_mvp_session().expect("mvp session should build");
        let button = session
            .menu_snapshot()
            .button_by_class_id(teamy_studio_cursor_gallery::CURSOR_GALLERY_BUTTON_CLASS_ID)
            .expect("cursor gallery button should exist")
            .clone();

        session
            .publish_main_menu_click(
                button.logical_button_id,
                9,
                8,
                7,
                CanonicalTimeKey::from_femtoseconds(100),
            )
            .expect("session should publish a click event");

        let failure_references = session.event_references();
        session.publish_startup_failed(
            CanonicalTimeKey::from_femtoseconds(101),
            "test startup failure",
            failure_references.clone(),
        );

        let composition = session.into_composition();
        let published = composition.timeline.published_epochs();
        let latest_event = published
            .last()
            .expect("failure publish should append an epoch")
            .1
            .events()[0]
            .downcast_ref::<StartupFailedEvent>()
            .expect("latest payload should be a startup failure event");

        assert_eq!(published.len(), 11);
        assert_eq!(
            published
                .last()
                .expect("failure epoch should exist")
                .0
                .time_key
                .raw_femtoseconds(),
            101
        );
        assert_eq!(
            published
                .last()
                .expect("failure epoch should exist")
                .1
                .events()[0]
                .definition()
                .id,
            STARTUP_FAILED_EVENT_DEFINITION.id
        );
        assert_eq!(latest_event.failed_at.raw_femtoseconds(), 101);
        assert_eq!(latest_event.prior_epoch_count, 10);
        assert_eq!(latest_event.reason, "test startup failure");
        assert_eq!(latest_event.failure_references, failure_references);
    }

    #[test]
    fn startup_session_can_publish_failure_from_latest_references() {
        let mut session = build_mvp_session().expect("mvp session should build");
        let button = session
            .menu_snapshot()
            .button_by_class_id(teamy_studio_cursor_gallery::CURSOR_GALLERY_BUTTON_CLASS_ID)
            .expect("cursor gallery button should exist")
            .clone();

        session
            .publish_main_menu_click(
                button.logical_button_id,
                21,
                22,
                23,
                CanonicalTimeKey::from_femtoseconds(200),
            )
            .expect("session should publish the first click event");
        session
            .publish_main_menu_click(
                button.logical_button_id,
                31,
                32,
                33,
                CanonicalTimeKey::from_femtoseconds(201),
            )
            .expect("session should publish the second click event");

        session.publish_startup_failed_from_latest_references(
            CanonicalTimeKey::from_femtoseconds(202),
            "latest failure test",
            1,
        );

        let composition = session.into_composition();
        let failure = composition.timeline.published_epochs()[11].1.events()[0]
            .downcast_ref::<StartupFailedEvent>()
            .expect("failure payload should be present");

        assert_eq!(composition.timeline.published_epochs().len(), 12);
        assert_eq!(failure.failed_at.raw_femtoseconds(), 202);
        assert_eq!(failure.prior_epoch_count, 11);
        assert_eq!(failure.reason, "latest failure test");
        assert_eq!(failure.failure_references.len(), 1);
        assert_eq!(
            failure.failure_references[0].event_definition_id,
            teamy_studio_main_menu::MAIN_MENU_CLICKED_EVENT_DEFINITION.id
        );
        assert_eq!(
            failure.failure_references[0]
                .timeline_offset_hint
                .raw_value(),
            201
        );
    }

    #[test]
    fn startup_runtime_can_publish_tracing_initialized_event() {
        let mut session = build_mvp_session().expect("mvp session should build");
        session
            .runtime
            .publish_tracing_initialized(TracingInitializedEvent {
                effective_filter_directive: "info".to_owned(),
                json_log_path: None,
                subscriber_was_already_initialized: false,
            });

        let composition = session.into_composition();
        let latest_event = composition
            .timeline
            .published_epochs()
            .last()
            .expect("tracing event should be published")
            .1
            .events()[0]
            .downcast_ref::<TracingInitializedEvent>()
            .expect("tracing initialized payload should be present");

        assert_eq!(
            composition
                .timeline
                .published_epochs()
                .last()
                .expect("tracing event should be published")
                .1
                .events()[0]
                .definition()
                .id,
            TRACING_INITIALIZED_EVENT_DEFINITION.id
        );
        assert_eq!(latest_event.effective_filter_directive, "info");
        assert_eq!(latest_event.json_log_path, None);
        assert!(!latest_event.subscriber_was_already_initialized);
    }

    #[test]
    fn startup_runtime_can_publish_tracing_observed_records() {
        let mut runtime = StartupRuntime::new();

        let published_count =
            runtime.publish_tracing_observed_records(vec![TracingRecordObservedEvent {
                target: "teamy_studio_startup::tests".to_owned(),
                name: "event crates/teamy_studio_startup/src/lib.rs:1".to_owned(),
                level: "INFO".to_owned(),
                fields: Vec::new(),
            }]);

        let (timeline, _, _, _) = runtime.into_parts();
        let published = timeline.published_epochs();
        let observed = published[0].1.events()[0]
            .downcast_ref::<TracingRecordObservedEvent>()
            .expect("tracing observed payload should be present");

        assert_eq!(published_count, 1);
        assert_eq!(published.len(), 1);
        assert_eq!(
            published[0].1.events()[0].definition().id,
            TRACING_RECORD_OBSERVED_EVENT_DEFINITION.id
        );
        assert_eq!(observed.level, "INFO");
    }

    #[test]
    fn app_composition_summarizes_tracing_observations() {
        let mut runtime = StartupRuntime::new();
        runtime.publish_tracing_observed_records(vec![
            TracingRecordObservedEvent {
                target: "teamy_studio_startup::tests".to_owned(),
                name: "info record".to_owned(),
                level: "INFO".to_owned(),
                fields: Vec::new(),
            },
            TracingRecordObservedEvent {
                target: "teamy_studio_startup::tests".to_owned(),
                name: "warn record".to_owned(),
                level: "WARN".to_owned(),
                fields: Vec::new(),
            },
            TracingRecordObservedEvent {
                target: "teamy_studio_startup::tests".to_owned(),
                name: "second info record".to_owned(),
                level: "INFO".to_owned(),
                fields: Vec::new(),
            },
        ]);
        let session = StartupSession {
            bootstrap_plan: StartupBootstrapPlan::empty(),
            registration_snapshot: RegistrationSnapshot::default(),
            menu_snapshot: MainMenuSnapshot::from_registrations(&[]),
            runtime,
            tracing_observation_layer: None,
        };

        let summary = session.into_composition().tracing_observation_summary();

        assert_eq!(summary.total_observed_records, 3);
        assert_eq!(summary.counts_by_level.get("INFO"), Some(&2));
        assert_eq!(summary.counts_by_level.get("WARN"), Some(&1));
        assert!(!summary.contains_timeline_reemit_marker);
    }

    #[test]
    fn tracing_observation_summary_flags_reemit_marker_if_present() {
        let mut runtime = StartupRuntime::new();
        runtime.publish_tracing_observed_records(vec![TracingRecordObservedEvent {
            target: "teamy_studio_startup::tests".to_owned(),
            name: "badly observed timeline reemit".to_owned(),
            level: "INFO".to_owned(),
            fields: vec![TracingObservedField {
                name: "teamy.timeline_reemit".to_owned(),
                value: "true".to_owned(),
            }],
        }]);
        let session = StartupSession {
            bootstrap_plan: StartupBootstrapPlan::empty(),
            registration_snapshot: RegistrationSnapshot::default(),
            menu_snapshot: MainMenuSnapshot::from_registrations(&[]),
            runtime,
            tracing_observation_layer: None,
        };

        let summary = session.into_composition().tracing_observation_summary();

        assert_eq!(summary.total_observed_records, 1);
        assert!(summary.contains_timeline_reemit_marker);
    }

    #[test]
    fn startup_smoke_summary_formats_for_startup_smoke_output() {
        let mut tracing_observations = super::TracingObservationSummary {
            total_observed_records: 3,
            contains_timeline_reemit_marker: false,
            ..super::TracingObservationSummary::default()
        };
        tracing_observations
            .counts_by_level
            .insert("INFO".to_owned(), 2);
        tracing_observations
            .counts_by_level
            .insert("WARN".to_owned(), 1);
        let summary = super::StartupSmokeSummary {
            tracing_observations,
            contains_startup_succeeded_event: true,
            contains_startup_failed_event: false,
            latest_startup_failure_reason: None,
            cursor_gallery_window_count: 1,
            shell_hosted_window_count: 1,
            total_published_epoch_count: 14,
        };

        let output = format_startup_smoke_summary(&summary);

        assert!(output.contains("Startup smoke summary"));
        assert!(output.contains("contains_startup_succeeded_event: true"));
        assert!(output.contains("contains_startup_failed_event: false"));
        assert!(output.contains("latest_startup_failure_reason: none"));
        assert!(output.contains("cursor_gallery_window_count: 1"));
        assert!(output.contains("shell_hosted_window_count: 1"));
        assert!(output.contains("total_published_epoch_count: 14"));
        assert!(output.contains("Tracing observation summary"));
        assert!(output.contains("total_observed_records: 3"));
        assert!(output.contains("contains_timeline_reemit_marker: false"));
        assert!(output.contains("level.INFO: 2"));
        assert!(output.contains("level.WARN: 1"));
    }

    #[test]
    fn startup_smoke_summary_runs_without_native_window() {
        let summary = build_startup_smoke_summary_from_raw_inputs(RawProcessStartupInputs {
            argv: vec![
                "--startup-smoke".to_owned(),
                "--log-filter".to_owned(),
                "trace".to_owned(),
            ],
            rust_log: None,
        })
        .expect("startup smoke should build simulated MVP composition");

        assert!(summary.contains_startup_succeeded_event);
        assert!(!summary.contains_startup_failed_event);
        assert_eq!(summary.latest_startup_failure_reason, None);
        assert_eq!(summary.cursor_gallery_window_count, 1);
        assert_eq!(summary.shell_hosted_window_count, 1);
        assert!(summary.total_published_epoch_count >= 1);
        assert!(!summary.tracing_observations.contains_timeline_reemit_marker);
    }

    #[test]
    fn startup_smoke_summary_preserves_bootstrap_failure_timeline() {
        let error = build_startup_smoke_summary_from_raw_inputs(RawProcessStartupInputs {
            argv: vec!["--startup-smoke".to_owned(), "--wat".to_owned()],
            rust_log: None,
        })
        .expect_err("invalid smoke args should preserve a failure summary");
        let summary = error.summary();

        assert!(!summary.contains_startup_succeeded_event);
        assert!(summary.contains_startup_failed_event);
        assert_eq!(
            summary.latest_startup_failure_reason.as_deref(),
            Some("startup bootstrap cli parse failed")
        );
        assert_eq!(summary.cursor_gallery_window_count, 0);
        assert_eq!(summary.shell_hosted_window_count, 0);
        assert_eq!(summary.total_published_epoch_count, 3);
    }

    #[test]
    fn startup_smoke_summary_preserves_tracing_initialization_failure_timeline() {
        let error = build_startup_smoke_summary_from_raw_inputs(RawProcessStartupInputs {
            argv: vec![
                "--startup-smoke".to_owned(),
                "--log-filter".to_owned(),
                "{".to_owned(),
            ],
            rust_log: None,
        })
        .expect_err("invalid tracing filter should preserve a failure summary");
        let summary = error.summary();

        assert!(!summary.contains_startup_succeeded_event);
        assert!(summary.contains_startup_failed_event);
        assert_eq!(
            summary.latest_startup_failure_reason.as_deref(),
            Some("startup tracing initialization failed")
        );
        assert_eq!(summary.cursor_gallery_window_count, 0);
        assert_eq!(summary.shell_hosted_window_count, 0);
        assert_eq!(summary.total_published_epoch_count, 11);
    }

    #[test]
    fn startup_session_tracing_observation_stage_drains_in_bounded_order() {
        let mut session = build_mvp_session().expect("mvp session should build");
        let observation_layer = TracingObservationLayer::new();
        let observation_handle = observation_layer.clone();
        session.tracing_observation_layer = Some(observation_layer.clone());
        let subscriber = tracing_subscriber::Registry::default().with(observation_layer);

        tracing::subscriber::with_default(subscriber, || {
            for sequence in 0_u64..70 {
                tracing::info!(sequence, "synthetic observed tracing record");
            }
            tracing::info!(
                teamy.timeline_reemit = true,
                event_schema_name = "teamy_studio.startup.succeeded",
                "timeline event re-emitted to tracing"
            );
        });

        let first_drain_count = session.run_tracing_observation_stage();
        let second_drain_count = session.run_tracing_observation_stage();
        let composition = session.into_composition();
        let observed_sequences = composition
            .timeline
            .published_epochs()
            .iter()
            .filter_map(|(_, epoch)| {
                epoch.events()[0]
                    .downcast_ref::<TracingRecordObservedEvent>()
                    .and_then(|event| {
                        event
                            .fields
                            .iter()
                            .find(|field| field.name == "sequence")
                            .map(|field| field.value.clone())
                    })
            })
            .collect::<Vec<_>>();

        assert_eq!(first_drain_count, TRACING_OBSERVATION_DRAIN_LIMIT);
        assert_eq!(second_drain_count, 6);
        assert!(observation_handle.observed_records().is_empty());
        assert_eq!(observed_sequences.len(), 70);
        assert_eq!(observed_sequences.first().map(String::as_str), Some("0"));
        assert_eq!(observed_sequences.last().map(String::as_str), Some("69"));
    }

    #[test]
    fn pump_to_idle_runs_tracing_observation_stage() {
        let mut session = build_mvp_session().expect("mvp session should build");
        let observation_layer = TracingObservationLayer::new();
        let observation_handle = observation_layer.clone();
        session.tracing_observation_layer = Some(observation_layer.clone());
        let subscriber = tracing_subscriber::Registry::default().with(observation_layer);

        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(source = "pump_to_idle_test", "observed from pump");
        });

        let emitted_epoch_count = session
            .pump_to_idle(CanonicalTimeKey::from_femtoseconds(500), 4)
            .expect("pump should process tracing observation stage");
        let composition = session.into_composition();
        let observed_count = composition
            .timeline
            .published_epochs()
            .iter()
            .filter(|(_, epoch)| {
                epoch.events()[0].definition().id == TRACING_RECORD_OBSERVED_EVENT_DEFINITION.id
            })
            .count();

        assert_eq!(emitted_epoch_count, 1);
        assert_eq!(observed_count, 1);
        assert!(observation_handle.observed_records().is_empty());
    }

    #[test]
    fn initialize_tracing_for_session_publishes_failure_events_for_invalid_filter() {
        let bootstrap_plan = derive_bootstrap_plan(
            &RawProcessStartupInputs {
                argv: vec!["--log-filter".to_owned(), "{".to_owned()],
                rust_log: None,
            },
            chrono::Local::now(),
        )
        .expect("bootstrap plan should preserve invalid filter text for later parsing");
        let mut session = build_mvp_session_from_bootstrap_plan(bootstrap_plan)
            .expect("session should build before tracing initialization");

        let error = initialize_tracing_for_session(&mut session)
            .expect_err("invalid tracing filter should fail initialization");
        let composition = session.into_composition();
        let published = composition.timeline.published_epochs();
        let tracing_failure = published[9].1.events()[0]
            .downcast_ref::<TracingInitializationFailedEvent>()
            .expect("tracing failure payload should be present");
        let startup_failure = published[10].1.events()[0]
            .downcast_ref::<StartupFailedEvent>()
            .expect("startup failure payload should be present");

        assert!(error.to_string().contains("directive"));
        assert_eq!(published.len(), 11);
        assert_eq!(
            published[9].1.events()[0].definition().id,
            TRACING_INITIALIZATION_FAILED_EVENT_DEFINITION.id
        );
        assert_eq!(
            published[10].1.events()[0].definition().id,
            STARTUP_FAILED_EVENT_DEFINITION.id
        );
        assert_eq!(tracing_failure.effective_filter_directive, "{");
        assert_eq!(tracing_failure.json_log_path, None);
        assert!(tracing_failure.reason.contains("directive"));
        assert_eq!(
            startup_failure.reason,
            "startup tracing initialization failed"
        );
        assert_eq!(startup_failure.failure_references.len(), 1);
        assert_eq!(
            startup_failure.failure_references[0].event_definition_id,
            TRACING_INITIALIZATION_FAILED_EVENT_DEFINITION.id
        );
        assert_eq!(startup_failure.prior_epoch_count, 10);
    }

    #[test]
    fn derive_bootstrap_plan_with_runtime_publishes_cli_parse_failure() {
        let mut runtime = StartupRuntime::new();

        let error = derive_bootstrap_plan_with_runtime(
            RawProcessStartupInputs {
                argv: vec!["--wat".to_owned()],
                rust_log: None,
            },
            chrono::Local::now(),
            &mut runtime,
        )
        .expect_err("unknown startup args should fail bootstrap derivation");
        let (timeline, _, _, _) = runtime.into_parts();
        let published = timeline.published_epochs();
        let detail = published[1].1.events()[0]
            .downcast_ref::<BootstrapCliParseFailedEvent>()
            .expect("cli parse failure payload should be present");
        let startup_failure = published[2].1.events()[0]
            .downcast_ref::<StartupFailedEvent>()
            .expect("startup failure payload should be present");

        assert_eq!(published.len(), 3);
        assert_eq!(
            published[0].1.events()[0].definition().id,
            PROCESS_STARTUP_OBSERVED_EVENT_DEFINITION.id
        );
        assert_eq!(
            published[1].1.events()[0].definition().id,
            BOOTSTRAP_CLI_PARSE_FAILED_EVENT_DEFINITION.id
        );
        assert_eq!(
            published[2].1.events()[0].definition().id,
            STARTUP_FAILED_EVENT_DEFINITION.id
        );
        assert_eq!(detail.argv, vec!["--wat".to_owned()]);
        assert_eq!(detail.missing_value_flag, None);
        assert_eq!(detail.unrecognized_argument.as_deref(), Some("--wat"));
        assert_eq!(startup_failure.reason, "startup bootstrap cli parse failed");
        assert_eq!(startup_failure.failure_references.len(), 1);
        assert_eq!(
            startup_failure.failure_references[0].event_definition_id,
            BOOTSTRAP_CLI_PARSE_FAILED_EVENT_DEFINITION.id
        );
        assert_eq!(error.to_string(), "unrecognized startup argument: --wat");
    }

    #[test]
    fn derive_bootstrap_plan_with_runtime_publishes_logging_plan_failure() {
        let mut runtime = StartupRuntime::new();

        let error = derive_bootstrap_plan_with_runtime(
            RawProcessStartupInputs {
                argv: vec![
                    "--debug".to_owned(),
                    "--log-filter".to_owned(),
                    "trace".to_owned(),
                ],
                rust_log: Some("warn".to_owned()),
            },
            chrono::Local::now(),
            &mut runtime,
        )
        .expect_err("conflicting startup logging args should fail bootstrap derivation");
        let (timeline, _, _, _) = runtime.into_parts();
        let published = timeline.published_epochs();
        let detail = published[2].1.events()[0]
            .downcast_ref::<StartupLoggingPlanFailedEvent>()
            .expect("logging-plan failure payload should be present");
        let startup_failure = published[3].1.events()[0]
            .downcast_ref::<StartupFailedEvent>()
            .expect("startup failure payload should be present");

        assert_eq!(published.len(), 4);
        assert_eq!(
            published[0].1.events()[0].definition().id,
            PROCESS_STARTUP_OBSERVED_EVENT_DEFINITION.id
        );
        assert_eq!(
            published[1].1.events()[0].definition().id,
            STARTUP_GLOBAL_ARGS_PARSED_EVENT_DEFINITION.id
        );
        assert_eq!(
            published[2].1.events()[0].definition().id,
            STARTUP_LOGGING_PLAN_FAILED_EVENT_DEFINITION.id
        );
        assert_eq!(
            published[3].1.events()[0].definition().id,
            STARTUP_FAILED_EVENT_DEFINITION.id
        );
        assert!(detail.debug);
        assert_eq!(detail.log_filter.as_deref(), Some("trace"));
        assert_eq!(detail.log_file, None);
        assert_eq!(detail.rust_log.as_deref(), Some("warn"));
        assert_eq!(detail.conflicting_log_filter.as_deref(), Some("trace"));
        assert_eq!(
            startup_failure.reason,
            "startup logging plan derivation failed"
        );
        assert_eq!(startup_failure.failure_references.len(), 1);
        assert_eq!(
            startup_failure.failure_references[0].event_definition_id,
            STARTUP_LOGGING_PLAN_FAILED_EVENT_DEFINITION.id
        );
        assert_eq!(error.to_string(), "cannot specify log filter with --debug");
    }

    #[test]
    fn registration_validation_failure_event_preserves_ids_and_provenance() {
        let trigger_provenance = registration_provenance!();
        let error = RegistrationValidationError::TriggerTargetsUnregisteredDefinition {
            trigger_name: "teamy_studio.test.missing_event_target",
            trigger_registration_id: TriggerRegistrationId::from_bytes([0x71; 16]),
            trigger_definition_id: TriggerDefinitionId::from_bytes([0x72; 16]),
            owner_feature_id: FeatureId::from_bytes([0x73; 16]),
            event_definition_id: teamy_studio_event_core::EventDefinitionId::from_bytes([0x74; 16]),
            trigger_provenance,
        };

        let event = super::registration_validation_failed_event(&error);

        assert_eq!(
            event.reason,
            "trigger teamy_studio.test.missing_event_target targets unregistered event definition EventDefinitionId(74747474-7474-7474-7474-747474747474)"
        );
        assert_eq!(event.feature_id, Some(FeatureId::from_bytes([0x73; 16])));
        assert_eq!(
            event.event_definition_id,
            Some(teamy_studio_event_core::EventDefinitionId::from_bytes(
                [0x74; 16]
            ))
        );
        assert_eq!(
            event.trigger_registration_id,
            Some(TriggerRegistrationId::from_bytes([0x71; 16]))
        );
        assert_eq!(
            event.trigger_definition_id,
            Some(TriggerDefinitionId::from_bytes([0x72; 16]))
        );
        assert_eq!(
            event.trigger_name,
            Some("teamy_studio.test.missing_event_target")
        );
        assert_eq!(event.trigger_provenance, Some(trigger_provenance));
    }

    #[test]
    fn registration_validation_failure_outcome_publishes_detail_before_startup_failure() {
        let trigger_provenance = registration_provenance!();
        let error = RegistrationValidationError::TriggerOwnedByUnregisteredFeature {
            trigger_name: "teamy_studio.test.unowned_trigger",
            trigger_registration_id: TriggerRegistrationId::from_bytes([0x81; 16]),
            trigger_definition_id: TriggerDefinitionId::from_bytes([0x82; 16]),
            feature_id: FeatureId::from_bytes([0x83; 16]),
            trigger_provenance,
        };
        let mut runtime = StartupRuntime::new();

        runtime.publish_registration_validation_failure_outcome(&error);

        let (timeline, _, _, _) = runtime.into_parts();
        let published = timeline.published_epochs();
        let detail = published[0].1.events()[0]
            .downcast_ref::<RegistrationValidationFailedEvent>()
            .expect("registration validation detail payload should be present");
        let startup_failure = published[1].1.events()[0]
            .downcast_ref::<StartupFailedEvent>()
            .expect("startup failure payload should be present");

        assert_eq!(published.len(), 2);
        assert_eq!(
            published[0].1.events()[0].definition().id,
            REGISTRATION_VALIDATION_FAILED_EVENT_DEFINITION.id
        );
        assert_eq!(
            published[1].1.events()[0].definition().id,
            STARTUP_FAILED_EVENT_DEFINITION.id
        );
        assert_eq!(
            detail.trigger_registration_id,
            Some(TriggerRegistrationId::from_bytes([0x81; 16]))
        );
        assert_eq!(detail.trigger_provenance, Some(trigger_provenance));
        assert_eq!(
            startup_failure.reason,
            "startup registration validation failed"
        );
        assert_eq!(startup_failure.failure_references.len(), 1);
        assert_eq!(
            startup_failure.failure_references[0].event_definition_id,
            REGISTRATION_VALIDATION_FAILED_EVENT_DEFINITION.id
        );
    }

    #[test]
    fn default_cursor_gallery_flow_failure_publishes_detail_event_before_startup_failure() {
        let mut session = build_mvp_session().expect("mvp session should build");

        session
            .runtime
            .publish_default_cursor_gallery_flow_failure_outcome(0, 0, 0);

        let composition = session.into_composition();
        let published = composition.timeline.published_epochs();
        let detail = published[9].1.events()[0]
            .downcast_ref::<DefaultCursorGalleryFlowFailedEvent>()
            .expect("detail payload should be present");
        let startup_failure = published[10].1.events()[0]
            .downcast_ref::<StartupFailedEvent>()
            .expect("startup failure payload should be present");

        assert_eq!(published.len(), 11);
        assert_eq!(
            published[9].1.events()[0].definition().id,
            DEFAULT_CURSOR_GALLERY_FLOW_FAILED_EVENT_DEFINITION.id
        );
        assert_eq!(
            published[10].1.events()[0].definition().id,
            STARTUP_FAILED_EVENT_DEFINITION.id
        );
        assert_eq!(detail.emitted_epoch_count, 0);
        assert_eq!(detail.cursor_gallery_window_count, 0);
        assert_eq!(detail.shell_window_count, 0);
        assert_eq!(
            startup_failure.reason,
            "registered triggers did not produce the default cursor-gallery window flow"
        );
        assert_eq!(startup_failure.failure_references.len(), 1);
        assert_eq!(
            startup_failure.failure_references[0].event_definition_id,
            DEFAULT_CURSOR_GALLERY_FLOW_FAILED_EVENT_DEFINITION.id
        );
        assert_eq!(startup_failure.prior_epoch_count, 10);
    }

    #[test]
    fn main_menu_interaction_time_seed_advances_past_bootstrap_events() {
        let mut session = build_mvp_session().expect("mvp session should build");
        session
            .runtime
            .publish_tracing_initialized(TracingInitializedEvent {
                effective_filter_directive: "info".to_owned(),
                json_log_path: None,
                subscriber_was_already_initialized: false,
            });

        assert_eq!(next_main_menu_interaction_time_seed(&session.runtime), 10);
    }
}

pub mod bootstrap;

pub use bootstrap::{
    BootstrapCliParseError, BootstrapPlanError, GlobalArgs, LogFilterSelection,
    ProcessStartupObservedEvent, RawProcessStartupInputs, StartupBootstrapCli,
    StartupBootstrapPlan, StartupGlobalArgsParsedEvent, StartupLoggingConfiguredEvent,
    StartupLoggingPlan, StartupLoggingPlanError, TracingInitializedEvent,
    derive_bootstrap_plan, initialize_tracing_from_bootstrap_plan, parse_bootstrap_cli_args,
    try_handle_builtin_bootstrap_cli_request,
};

use eyre::{Result, eyre};
use facet::Facet;
use linkme::distributed_slice;
use std::error::Error;
use std::fmt::{Display, Formatter};
use teamy_studio_cursor_gallery::CURSOR_GALLERY_BUTTON_CLASS_ID;
use teamy_studio_cursor_gallery::CursorGalleryState;
use teamy_studio_event_core::{EventDefinition, EventDefinitionId, PublishedEvent, WritableArena};
use teamy_studio_main_menu::{
    FeatureValidationState, MainMenuLogicalButtonId, MainMenuSnapshot, registered_button_classes,
};
use teamy_studio_registration_core::{
    EVENT_DEFINITION_REGISTRATIONS, EventDefinitionRegistration, RegistrationSnapshot, snapshot,
    trigger_registrations_for, validate_registrations,
};
use teamy_studio_shell::{HostedWindowRecord, ShellRuntime, ShellState};
use teamy_studio_timeline_core::{
    CanonicalTimeKey, ConstructedTimeline, EventId, EventReference, TriggerCursor,
    TriggerRuntime,
};
use teamy_studio_launcher_catalog as _;

pub static STARTUP_SUCCEEDED_EVENT_DEFINITION: EventDefinition = EventDefinition {
    id: EventDefinitionId::from_bytes([0x01; 16]),
    schema_name: "teamy_studio.startup.succeeded",
    schema_version: 1,
};

pub static STARTUP_FAILED_EVENT_DEFINITION: EventDefinition = EventDefinition {
    id: EventDefinitionId::from_bytes([0x02; 16]),
    schema_name: "teamy_studio.startup.failed",
    schema_version: 1,
};

pub static PROCESS_STARTUP_OBSERVED_EVENT_DEFINITION: EventDefinition = EventDefinition {
    id: EventDefinitionId::from_bytes([0x03; 16]),
    schema_name: "teamy_studio.startup.process_startup_observed",
    schema_version: 1,
};

pub static STARTUP_GLOBAL_ARGS_PARSED_EVENT_DEFINITION: EventDefinition = EventDefinition {
    id: EventDefinitionId::from_bytes([0x08; 16]),
    schema_name: "teamy_studio.startup.global_args_parsed",
    schema_version: 1,
};

pub static STARTUP_LOGGING_CONFIGURED_EVENT_DEFINITION: EventDefinition = EventDefinition {
    id: EventDefinitionId::from_bytes([0x04; 16]),
    schema_name: "teamy_studio.startup.logging_configured",
    schema_version: 1,
};

pub static TRACING_INITIALIZED_EVENT_DEFINITION: EventDefinition = EventDefinition {
    id: EventDefinitionId::from_bytes([0x05; 16]),
    schema_name: "teamy_studio.startup.tracing_initialized",
    schema_version: 1,
};

pub static REGISTRATION_VALIDATION_STARTED_EVENT_DEFINITION: EventDefinition = EventDefinition {
    id: EventDefinitionId::from_bytes([0x06; 16]),
    schema_name: "teamy_studio.startup.registration_validation_started",
    schema_version: 1,
};

pub static REGISTRATION_VALIDATION_COMPLETED_EVENT_DEFINITION: EventDefinition = EventDefinition {
    id: EventDefinitionId::from_bytes([0x07; 16]),
    schema_name: "teamy_studio.startup.registration_validation_completed",
    schema_version: 1,
};

pub static FEATURE_COMPATIBILITY_VALIDATION_STARTED_EVENT_DEFINITION: EventDefinition =
    EventDefinition {
        id: EventDefinitionId::from_bytes([0x09; 16]),
        schema_name: "teamy_studio.startup.feature_compatibility_validation_started",
        schema_version: 1,
    };

pub static FEATURE_COMPATIBILITY_VALIDATED_EVENT_DEFINITION: EventDefinition = EventDefinition {
    id: EventDefinitionId::from_bytes([0x0A; 16]),
    schema_name: "teamy_studio.startup.feature_compatibility_validated",
    schema_version: 1,
};

pub static FEATURE_COMPATIBILITY_VALIDATION_COMPLETED_EVENT_DEFINITION: EventDefinition =
    EventDefinition {
        id: EventDefinitionId::from_bytes([0x0B; 16]),
        schema_name: "teamy_studio.startup.feature_compatibility_validation_completed",
        schema_version: 1,
    };

pub static FEATURE_ACTIVATION_GATE_RESOLVED_EVENT_DEFINITION: EventDefinition = EventDefinition {
    id: EventDefinitionId::from_bytes([0x0C; 16]),
    schema_name: "teamy_studio.startup.feature_activation_gate_resolved",
    schema_version: 1,
};

pub static TRACING_INITIALIZATION_FAILED_EVENT_DEFINITION: EventDefinition = EventDefinition {
    id: EventDefinitionId::from_bytes([0x0D; 16]),
    schema_name: "teamy_studio.startup.tracing_initialization_failed",
    schema_version: 1,
};

pub static DEFAULT_CURSOR_GALLERY_FLOW_FAILED_EVENT_DEFINITION: EventDefinition =
    EventDefinition {
        id: EventDefinitionId::from_bytes([0x0E; 16]),
        schema_name: "teamy_studio.startup.default_cursor_gallery_flow_failed",
        schema_version: 1,
    };

pub static BOOTSTRAP_CLI_PARSE_FAILED_EVENT_DEFINITION: EventDefinition = EventDefinition {
    id: EventDefinitionId::from_bytes([0x0F; 16]),
    schema_name: "teamy_studio.startup.bootstrap_cli_parse_failed",
    schema_version: 1,
};

pub static STARTUP_LOGGING_PLAN_FAILED_EVENT_DEFINITION: EventDefinition = EventDefinition {
    id: EventDefinitionId::from_bytes([0x11; 16]),
    schema_name: "teamy_studio.startup.logging_plan_failed",
    schema_version: 1,
};

const STARTUP_BOOTSTRAP_CLI_PARSE_FAILED_REASON: &str = "startup bootstrap cli parse failed";
const STARTUP_LOGGING_PLAN_DERIVATION_FAILED_REASON: &str =
    "startup logging plan derivation failed";
const STARTUP_TRACING_INITIALIZATION_FAILED_REASON: &str =
    "startup tracing initialization failed";
const DEFAULT_CURSOR_GALLERY_FLOW_FAILURE_REASON: &str =
    "registered triggers did not produce the default cursor-gallery window flow";

#[distributed_slice(EVENT_DEFINITION_REGISTRATIONS)]
pub static STARTUP_SUCCEEDED_EVENT_REGISTRATION: EventDefinitionRegistration =
    EventDefinitionRegistration {
        definition: &STARTUP_SUCCEEDED_EVENT_DEFINITION,
    };

#[distributed_slice(EVENT_DEFINITION_REGISTRATIONS)]
pub static STARTUP_FAILED_EVENT_REGISTRATION: EventDefinitionRegistration =
    EventDefinitionRegistration {
        definition: &STARTUP_FAILED_EVENT_DEFINITION,
    };

#[distributed_slice(EVENT_DEFINITION_REGISTRATIONS)]
pub static PROCESS_STARTUP_OBSERVED_EVENT_REGISTRATION: EventDefinitionRegistration =
    EventDefinitionRegistration {
        definition: &PROCESS_STARTUP_OBSERVED_EVENT_DEFINITION,
    };

#[distributed_slice(EVENT_DEFINITION_REGISTRATIONS)]
pub static STARTUP_GLOBAL_ARGS_PARSED_EVENT_REGISTRATION: EventDefinitionRegistration =
    EventDefinitionRegistration {
        definition: &STARTUP_GLOBAL_ARGS_PARSED_EVENT_DEFINITION,
    };

#[distributed_slice(EVENT_DEFINITION_REGISTRATIONS)]
pub static STARTUP_LOGGING_CONFIGURED_EVENT_REGISTRATION: EventDefinitionRegistration =
    EventDefinitionRegistration {
        definition: &STARTUP_LOGGING_CONFIGURED_EVENT_DEFINITION,
    };

#[distributed_slice(EVENT_DEFINITION_REGISTRATIONS)]
pub static TRACING_INITIALIZED_EVENT_REGISTRATION: EventDefinitionRegistration =
    EventDefinitionRegistration {
        definition: &TRACING_INITIALIZED_EVENT_DEFINITION,
    };

#[distributed_slice(EVENT_DEFINITION_REGISTRATIONS)]
pub static REGISTRATION_VALIDATION_STARTED_EVENT_REGISTRATION: EventDefinitionRegistration =
    EventDefinitionRegistration {
        definition: &REGISTRATION_VALIDATION_STARTED_EVENT_DEFINITION,
    };

#[distributed_slice(EVENT_DEFINITION_REGISTRATIONS)]
pub static REGISTRATION_VALIDATION_COMPLETED_EVENT_REGISTRATION: EventDefinitionRegistration =
    EventDefinitionRegistration {
        definition: &REGISTRATION_VALIDATION_COMPLETED_EVENT_DEFINITION,
    };

#[distributed_slice(EVENT_DEFINITION_REGISTRATIONS)]
pub static FEATURE_COMPATIBILITY_VALIDATION_STARTED_EVENT_REGISTRATION:
    EventDefinitionRegistration = EventDefinitionRegistration {
        definition: &FEATURE_COMPATIBILITY_VALIDATION_STARTED_EVENT_DEFINITION,
    };

#[distributed_slice(EVENT_DEFINITION_REGISTRATIONS)]
pub static FEATURE_COMPATIBILITY_VALIDATED_EVENT_REGISTRATION: EventDefinitionRegistration =
    EventDefinitionRegistration {
        definition: &FEATURE_COMPATIBILITY_VALIDATED_EVENT_DEFINITION,
    };

#[distributed_slice(EVENT_DEFINITION_REGISTRATIONS)]
pub static FEATURE_COMPATIBILITY_VALIDATION_COMPLETED_EVENT_REGISTRATION:
    EventDefinitionRegistration = EventDefinitionRegistration {
        definition: &FEATURE_COMPATIBILITY_VALIDATION_COMPLETED_EVENT_DEFINITION,
    };

#[distributed_slice(EVENT_DEFINITION_REGISTRATIONS)]
pub static FEATURE_ACTIVATION_GATE_RESOLVED_EVENT_REGISTRATION: EventDefinitionRegistration =
    EventDefinitionRegistration {
        definition: &FEATURE_ACTIVATION_GATE_RESOLVED_EVENT_DEFINITION,
    };

#[distributed_slice(EVENT_DEFINITION_REGISTRATIONS)]
pub static TRACING_INITIALIZATION_FAILED_EVENT_REGISTRATION: EventDefinitionRegistration =
    EventDefinitionRegistration {
        definition: &TRACING_INITIALIZATION_FAILED_EVENT_DEFINITION,
    };

#[distributed_slice(EVENT_DEFINITION_REGISTRATIONS)]
pub static DEFAULT_CURSOR_GALLERY_FLOW_FAILED_EVENT_REGISTRATION: EventDefinitionRegistration =
    EventDefinitionRegistration {
        definition: &DEFAULT_CURSOR_GALLERY_FLOW_FAILED_EVENT_DEFINITION,
    };

#[distributed_slice(EVENT_DEFINITION_REGISTRATIONS)]
pub static BOOTSTRAP_CLI_PARSE_FAILED_EVENT_REGISTRATION: EventDefinitionRegistration =
    EventDefinitionRegistration {
        definition: &BOOTSTRAP_CLI_PARSE_FAILED_EVENT_DEFINITION,
    };

#[distributed_slice(EVENT_DEFINITION_REGISTRATIONS)]
pub static STARTUP_LOGGING_PLAN_FAILED_EVENT_REGISTRATION: EventDefinitionRegistration =
    EventDefinitionRegistration {
        definition: &STARTUP_LOGGING_PLAN_FAILED_EVENT_DEFINITION,
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
    pub feature_class_id: [u8; 16],
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
    pub feature_class_id: [u8; 16],
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
}

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
        BootstrapCliParseError::UnrecognizedArgument { argument } => {
            BootstrapCliParseFailedEvent {
                argv: raw_inputs.argv.clone(),
                missing_value_flag: None,
                unrecognized_argument: Some(argument.clone()),
            }
        }
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

fn feature_validation_state_counts(menu_snapshot: &MainMenuSnapshot) -> (u64, u64, u64) {
    menu_snapshot.buttons().iter().fold(
        (0_u64, 0_u64, 0_u64),
        |(pending_count, validated_count, failed_count), button| match button.validation_state {
            FeatureValidationState::Pending => {
                (pending_count + 1, validated_count, failed_count)
            }
            FeatureValidationState::Validated => {
                (pending_count, validated_count + 1, failed_count)
            }
            FeatureValidationState::Failed => (pending_count, validated_count, failed_count + 1),
        },
    )
}

#[derive(Debug)]
pub struct StartupSession {
    bootstrap_plan: StartupBootstrapPlan,
    registration_snapshot: RegistrationSnapshot,
    menu_snapshot: MainMenuSnapshot,
    runtime: StartupRuntime,
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
        let mut epoch = WritableArena::new(arena_name);
        epoch.push(event);
        self.timeline.ingest(time_key, epoch.seal());
    }

    #[must_use]
    pub fn next_publish_time_key(&self) -> CanonicalTimeKey {
        self.timeline
            .published_event_records()
            .last()
            .map_or(CanonicalTimeKey::ZERO, |(cursor, _, _)| {
                CanonicalTimeKey::from_femtoseconds(cursor.time_key.raw_femtoseconds() + 1)
            })
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

    pub fn publish_tracing_initialized(
        &mut self,
        tracing_initialized: TracingInitializedEvent,
    ) {
        let time_key = self.next_publish_time_key();
        self.publish(
            "teamy_studio.startup.bootstrap",
            time_key,
            PublishedEvent::new(&TRACING_INITIALIZED_EVENT_DEFINITION, tracing_initialized),
        );
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
                    effective_filter_directive: bootstrap_plan
                        .logging
                        .effective_filter_directive(),
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
        feature_class_id: [u8; 16],
        feature_title: &'static str,
    ) {
        let time_key = self.next_publish_time_key();
        self.publish(
            "teamy_studio.startup.validation",
            time_key,
            PublishedEvent::new(
                &FEATURE_COMPATIBILITY_VALIDATED_EVENT_DEFINITION,
                FeatureCompatibilityValidatedEvent {
                    feature_class_id,
                    feature_title,
                },
            ),
        );
    }

    pub fn publish_feature_compatibility_validation_completed(
        &mut self,
        menu_snapshot: &MainMenuSnapshot,
    ) {
        let feature_count = menu_snapshot.buttons().len() as u64;
        let (pending_count, validated_count, failed_count) =
            feature_validation_state_counts(menu_snapshot);
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
        feature_class_id: [u8; 16],
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
                    feature_class_id,
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
        self.runtime
            .publish_startup_failed_from_latest_references(time_key, reason, max_references);
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

            if registered_emitted == 0 && shell_emitted == 0 {
                break;
            }
        }
        Ok(emitted_epoch_count)
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

pub fn observe_bootstrap_plan_from_raw_inputs(
    raw_inputs: RawProcessStartupInputs,
) -> std::result::Result<ObservedBootstrapPlan, ObservedBootstrapPlanFailure> {
    observe_bootstrap_plan_from_raw_inputs_with_runtime(raw_inputs, StartupRuntime::new())
}

pub fn observe_bootstrap_plan_from_raw_inputs_with_native_shell(
    raw_inputs: RawProcessStartupInputs,
) -> std::result::Result<ObservedBootstrapPlan, ObservedBootstrapPlanFailure> {
    observe_bootstrap_plan_from_raw_inputs_with_runtime(
        raw_inputs,
        StartupRuntime::new_with_native_shell(),
    )
}

pub fn build_mvp_session_from_raw_inputs(raw_inputs: RawProcessStartupInputs) -> Result<StartupSession> {
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
    build_mvp_session_with_runtime(bootstrap_plan, StartupRuntime::new_with_native_shell(), false)
}

fn build_mvp_session_with_runtime(
    bootstrap_plan: StartupBootstrapPlan,
    mut runtime: StartupRuntime,
    bootstrap_events_already_published: bool,
) -> Result<StartupSession> {
    if !bootstrap_events_already_published {
        runtime.publish_bootstrap_plan_events(&bootstrap_plan);
    }
    let pre_validation_snapshot = snapshot();
    runtime.publish_registration_validation_started(pre_validation_snapshot);
    validate_registrations()?;
    let registration_snapshot = snapshot();
    runtime.publish_registration_validation_completed(registration_snapshot);

    let button_classes = registered_button_classes();
    let mut menu_snapshot = MainMenuSnapshot::from_registrations(&button_classes);
    runtime.publish_feature_compatibility_validation_started(menu_snapshot.buttons().len() as u64);
    menu_snapshot.set_validation_state_for_class_id(
        CURSOR_GALLERY_BUTTON_CLASS_ID,
        FeatureValidationState::Validated,
    );
    let cursor_gallery_button = menu_snapshot
        .button_by_class_id(CURSOR_GALLERY_BUTTON_CLASS_ID)
        .ok_or_else(|| eyre!("cursor gallery button was not registered for feature validation"))?;
    runtime.publish_feature_compatibility_validated(
        cursor_gallery_button.class_id.as_bytes(),
        cursor_gallery_button.title,
    );
    runtime.publish_feature_compatibility_validation_completed(&menu_snapshot);
    runtime.publish_feature_activation_gate_resolved(
        cursor_gallery_button.class_id.as_bytes(),
        cursor_gallery_button.title,
        cursor_gallery_button.validation_state == FeatureValidationState::Validated,
    );

    Ok(StartupSession {
        bootstrap_plan,
        registration_snapshot,
        menu_snapshot,
        runtime,
    })
}

fn observe_bootstrap_plan_from_raw_inputs_with_runtime(
    raw_inputs: RawProcessStartupInputs,
    mut runtime: StartupRuntime,
) -> std::result::Result<ObservedBootstrapPlan, ObservedBootstrapPlanFailure> {
    let bootstrap_plan = match derive_bootstrap_plan_with_runtime(
        raw_inputs,
        chrono::Local::now(),
        &mut runtime,
    ) {
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

fn run_default_cursor_gallery_flow(mut session: StartupSession) -> Result<AppComposition> {
    let clicked_button = session
        .menu_snapshot()
        .button_by_class_id(CURSOR_GALLERY_BUTTON_CLASS_ID)
        .cloned()
        .ok_or_else(|| eyre!("cursor gallery button was not registered for the MVP composition"))?;

    let click_time_key = session.runtime.next_publish_time_key();
    session.publish_main_menu_click(
        clicked_button.logical_button_id,
        64,
        32,
        1,
        click_time_key,
    )?;
    let emitted_epoch_count = session.pump_to_idle(session.runtime.next_publish_time_key(), 8)?;

    if emitted_epoch_count == 0
        || session.cursor_gallery_state().windows().is_empty()
        || session.shell_state().windows().is_empty()
    {
        session.runtime.publish_default_cursor_gallery_flow_failure_outcome(
            emitted_epoch_count,
            session.cursor_gallery_state().windows().len(),
            session.shell_state().windows().len(),
        );
        return Err(eyre!(DEFAULT_CURSOR_GALLERY_FLOW_FAILURE_REASON));
    }

    let success_time_key = session.runtime.next_publish_time_key();
    session.runtime.publish_startup_succeeded(success_time_key);

    Ok(session.into_composition())
}

pub fn main() -> Result<()> {
    main_with_raw_inputs(RawProcessStartupInputs::capture_from_env())
}

fn initialize_tracing_for_session(session: &mut StartupSession) -> Result<()> {
    match initialize_tracing_from_bootstrap_plan(session.bootstrap_plan()) {
        Ok(tracing_initialized) => {
            session.runtime.publish_tracing_initialized(tracing_initialized);
            Ok(())
        }
        Err(error) => {
            let bootstrap_plan = session.bootstrap_plan().clone();
            session.runtime.publish_tracing_initialization_failure_outcome(
                &bootstrap_plan,
                error.to_string(),
            );
            Err(error)
        }
    }
}

fn next_main_menu_interaction_time_seed(runtime: &StartupRuntime) -> i128 {
    runtime.next_publish_time_key().raw_femtoseconds()
}

pub fn main_with_raw_inputs(raw_inputs: RawProcessStartupInputs) -> Result<()> {
    if try_handle_builtin_bootstrap_cli_request(&raw_inputs.argv_refs())? {
        return Ok(());
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
            let clicked_class_id = menu_snapshot
                .buttons()
                .iter()
                .find(|button| button.logical_button_id == logical_button_id)
                .map(|button| button.class_id);
            session.publish_main_menu_click(
                logical_button_id,
                0,
                0,
                1,
                CanonicalTimeKey::from_femtoseconds(next_time_key),
            )?;
            next_time_key += 16;
            let _ = session.pump_to_idle(CanonicalTimeKey::from_femtoseconds(next_time_key), 8)?;
            next_time_key += 16;
            let _ = clicked_class_id;
            Ok(())
        },
    )
}

#[cfg(test)]
mod tests {
    use super::{
        AppComposition, BOOTSTRAP_CLI_PARSE_FAILED_EVENT_DEFINITION,
        PROCESS_STARTUP_OBSERVED_EVENT_DEFINITION,
        FEATURE_ACTIVATION_GATE_RESOLVED_EVENT_DEFINITION,
        DEFAULT_CURSOR_GALLERY_FLOW_FAILED_EVENT_DEFINITION,
        FEATURE_COMPATIBILITY_VALIDATED_EVENT_DEFINITION,
        FEATURE_COMPATIBILITY_VALIDATION_COMPLETED_EVENT_DEFINITION,
        FEATURE_COMPATIBILITY_VALIDATION_STARTED_EVENT_DEFINITION,
        REGISTRATION_VALIDATION_COMPLETED_EVENT_DEFINITION,
        REGISTRATION_VALIDATION_STARTED_EVENT_DEFINITION,
        STARTUP_FAILED_EVENT_DEFINITION, STARTUP_GLOBAL_ARGS_PARSED_EVENT_DEFINITION,
        STARTUP_LOGGING_CONFIGURED_EVENT_DEFINITION,
        STARTUP_LOGGING_PLAN_FAILED_EVENT_DEFINITION,
        STARTUP_SUCCEEDED_EVENT_DEFINITION, TRACING_INITIALIZATION_FAILED_EVENT_DEFINITION,
        TRACING_INITIALIZED_EVENT_DEFINITION,
        BootstrapCliParseFailedEvent,
        FeatureActivationGateResolvedEvent,
        DefaultCursorGalleryFlowFailedEvent,
        FeatureCompatibilityValidatedEvent, FeatureCompatibilityValidationCompletedEvent,
        FeatureCompatibilityValidationStartedEvent,
        RawProcessStartupInputs, RegistrationValidationCompletedEvent,
        RegistrationValidationStartedEvent, StartupGlobalArgsParsedEvent,
        ProcessStartupObservedEvent, StartupFailedEvent, StartupLoggingConfiguredEvent,
        StartupLoggingPlanFailedEvent,
        StartupSucceededEvent, TracingInitializationFailedEvent, TracingInitializedEvent,
        StartupRuntime,
        build_mvp_composition,
        build_mvp_composition_from_raw_inputs, build_mvp_session,
        build_mvp_session_from_bootstrap_plan,
        build_mvp_session_from_raw_inputs, build_mvp_session_with_native_shell,
        derive_bootstrap_plan,
        derive_bootstrap_plan_with_runtime,
        initialize_tracing_for_session,
        next_main_menu_interaction_time_seed,
        observe_bootstrap_plan_from_raw_inputs,
    };
    use teamy_studio_cursor_gallery::CURSOR_GALLERY_OPEN_INTENT_DEFINITION;
    use teamy_studio_main_menu::{FeatureValidationState, MAIN_MENU_CLICKED_EVENT_DEFINITION};
    use teamy_studio_shell::{
        InitialWindowCommand, RendererHostMode, WINDOW_CREATE_REQUEST_EVENT_DEFINITION,
        WINDOW_CREATED_EVENT_DEFINITION, WindowHostOptions,
    };
    use teamy_studio_timeline_core::CanonicalTimeKey;

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
        assert_eq!(startup_logging_configured.effective_filter_directive, "info");
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
        assert_eq!(feature_validation_started.feature_count, 16);

        let feature_validated = published[6].1.events()[0]
            .downcast_ref::<FeatureCompatibilityValidatedEvent>()
            .expect("feature validated payload should be present");
        assert_eq!(
            feature_validated.feature_class_id,
            teamy_studio_cursor_gallery::CURSOR_GALLERY_BUTTON_CLASS_ID.as_bytes()
        );
        assert_eq!(feature_validated.feature_title, "Cursor Gallery");

        let feature_validation_completed = published[7].1.events()[0]
            .downcast_ref::<FeatureCompatibilityValidationCompletedEvent>()
            .expect("feature validation completion payload should be present");
        assert_eq!(feature_validation_completed.feature_count, 16);
        assert_eq!(feature_validation_completed.pending_count, 15);
        assert_eq!(feature_validation_completed.validated_count, 1);
        assert_eq!(feature_validation_completed.failed_count, 0);

        let feature_activation_gate = published[8].1.events()[0]
            .downcast_ref::<FeatureActivationGateResolvedEvent>()
            .expect("feature activation gate payload should be present");
        assert_eq!(
            feature_activation_gate.feature_class_id,
            teamy_studio_cursor_gallery::CURSOR_GALLERY_BUTTON_CLASS_ID.as_bytes()
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
        assert_eq!(session_record_times, vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 50, 51, 53, 54]);
        assert_eq!(
            session_reference_definitions,
            vec![
            (PROCESS_STARTUP_OBSERVED_EVENT_DEFINITION.id, 0),
            (STARTUP_GLOBAL_ARGS_PARSED_EVENT_DEFINITION.id, 1),
            (STARTUP_LOGGING_CONFIGURED_EVENT_DEFINITION.id, 2),
            (REGISTRATION_VALIDATION_STARTED_EVENT_DEFINITION.id, 3),
            (REGISTRATION_VALIDATION_COMPLETED_EVENT_DEFINITION.id, 4),
            (FEATURE_COMPATIBILITY_VALIDATION_STARTED_EVENT_DEFINITION.id, 5),
            (FEATURE_COMPATIBILITY_VALIDATED_EVENT_DEFINITION.id, 6),
            (FEATURE_COMPATIBILITY_VALIDATION_COMPLETED_EVENT_DEFINITION.id, 7),
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
            argv: vec!["--debug".to_owned(), "--log-file".to_owned(), "logs/teamy.ndjson".to_owned()],
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
            session.bootstrap_plan().logging.effective_filter_directive(),
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

        assert_eq!(bootstrap_error.to_string(), "unrecognized startup argument: --wat");
        assert_eq!(published.len(), 3);
        assert_eq!(published[0].1.events()[0].definition().id, PROCESS_STARTUP_OBSERVED_EVENT_DEFINITION.id);
        assert_eq!(published[1].1.events()[0].definition().id, BOOTSTRAP_CLI_PARSE_FAILED_EVENT_DEFINITION.id);
        assert_eq!(published[2].1.events()[0].definition().id, STARTUP_FAILED_EVENT_DEFINITION.id);
        assert_eq!(detail.unrecognized_argument.as_deref(), Some("--wat"));
        assert_eq!(startup_failure.reason, "startup bootstrap cli parse failed");
        assert_eq!(startup_failure.failure_references.len(), 1);
        assert_eq!(startup_failure.failure_references[0].event_definition_id, BOOTSTRAP_CLI_PARSE_FAILED_EVENT_DEFINITION.id);
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
            session.bootstrap_plan().logging.effective_filter_directive(),
            "trace"
        );
        assert_eq!(session.published_event_records()[0].0.time_key.raw_femtoseconds(), 0);
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
            composition.bootstrap_plan().logging.effective_filter_directive(),
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
            published.last().expect("failure epoch should exist").0.time_key.raw_femtoseconds(),
            101
        );
        assert_eq!(
            published.last().expect("failure epoch should exist").1.events()[0]
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
        assert_eq!(failure.failure_references[0].timeline_offset_hint.raw_value(), 201);
    }

    #[test]
    fn startup_runtime_can_publish_tracing_initialized_event() {
        let mut session = build_mvp_session().expect("mvp session should build");
        session.runtime.publish_tracing_initialized(TracingInitializedEvent {
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
        assert_eq!(startup_failure.reason, "startup tracing initialization failed");
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
        assert_eq!(published[0].1.events()[0].definition().id, PROCESS_STARTUP_OBSERVED_EVENT_DEFINITION.id);
        assert_eq!(published[1].1.events()[0].definition().id, BOOTSTRAP_CLI_PARSE_FAILED_EVENT_DEFINITION.id);
        assert_eq!(published[2].1.events()[0].definition().id, STARTUP_FAILED_EVENT_DEFINITION.id);
        assert_eq!(detail.argv, vec!["--wat".to_owned()]);
        assert_eq!(detail.missing_value_flag, None);
        assert_eq!(detail.unrecognized_argument.as_deref(), Some("--wat"));
        assert_eq!(startup_failure.reason, "startup bootstrap cli parse failed");
        assert_eq!(startup_failure.failure_references.len(), 1);
        assert_eq!(startup_failure.failure_references[0].event_definition_id, BOOTSTRAP_CLI_PARSE_FAILED_EVENT_DEFINITION.id);
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
        assert_eq!(published[0].1.events()[0].definition().id, PROCESS_STARTUP_OBSERVED_EVENT_DEFINITION.id);
        assert_eq!(published[1].1.events()[0].definition().id, STARTUP_GLOBAL_ARGS_PARSED_EVENT_DEFINITION.id);
        assert_eq!(published[2].1.events()[0].definition().id, STARTUP_LOGGING_PLAN_FAILED_EVENT_DEFINITION.id);
        assert_eq!(published[3].1.events()[0].definition().id, STARTUP_FAILED_EVENT_DEFINITION.id);
        assert!(detail.debug);
        assert_eq!(detail.log_filter.as_deref(), Some("trace"));
        assert_eq!(detail.log_file, None);
        assert_eq!(detail.rust_log.as_deref(), Some("warn"));
        assert_eq!(detail.conflicting_log_filter.as_deref(), Some("trace"));
        assert_eq!(startup_failure.reason, "startup logging plan derivation failed");
        assert_eq!(startup_failure.failure_references.len(), 1);
        assert_eq!(startup_failure.failure_references[0].event_definition_id, STARTUP_LOGGING_PLAN_FAILED_EVENT_DEFINITION.id);
        assert_eq!(error.to_string(), "cannot specify log filter with --debug");
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
        session.runtime.publish_tracing_initialized(TracingInitializedEvent {
            effective_filter_directive: "info".to_owned(),
            json_log_path: None,
            subscriber_was_already_initialized: false,
        });

        assert_eq!(next_main_menu_interaction_time_seed(&session.runtime), 10);
    }
}

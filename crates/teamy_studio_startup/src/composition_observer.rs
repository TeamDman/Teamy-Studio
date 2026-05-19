use linkme::distributed_slice;
use teamy_studio_cursor_gallery::{CURSOR_GALLERY_BUTTON_CLASS_ID, CURSOR_GALLERY_FEATURE_ID};
use teamy_studio_event_core::PublishedEvent;
use teamy_studio_main_menu::{
    MAIN_MENU_CLICKED_EVENT_DEFINITION, MainMenuClickEvent, MainMenuLogicalButtonId,
};
use teamy_studio_registration_core::{
    FEATURE_DEFINITION_REGISTRATIONS, FeatureDefinitionRegistration, FeatureId,
    TRIGGER_REGISTRATIONS, TriggerDefinitionId, TriggerRegistration, TriggerRegistrationId,
    registration_provenance,
};

use crate::{
    DEFAULT_CURSOR_GALLERY_FLOW_REQUESTED_EVENT_DEFINITION, DefaultCursorGalleryFlowRequestedEvent,
    NATIVE_MAIN_MENU_LAUNCH_REQUESTED_EVENT_DEFINITION, NativeMainMenuLaunchRequestedEvent,
    STARTUP_COMPOSITION_READY_EVENT_DEFINITION, StartupBootstrapPlan,
    StartupCompositionPolicyDerivedEvent, StartupCompositionReadyEvent,
};

pub(crate) const STARTUP_FEATURE_TITLE: &str = "Startup";
const CURSOR_GALLERY_DEFAULT_POINTER_X: i32 = 64;
const CURSOR_GALLERY_DEFAULT_POINTER_Y: i32 = 32;
const CURSOR_GALLERY_DEFAULT_LAYOUT_REVISION: u64 = 1;

pub const STARTUP_FEATURE_ID: FeatureId = FeatureId::from_bytes([0x17; 16]);

pub const STARTUP_COMPOSITION_READY_TRIGGER_DEFINITION_ID: TriggerDefinitionId =
    TriggerDefinitionId::from_bytes([0x18; 16]);

pub const STARTUP_COMPOSITION_READY_TRIGGER_REGISTRATION_ID: TriggerRegistrationId =
    TriggerRegistrationId::from_bytes([0x19; 16]);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct StartupCompositionPolicy {
    default_cursor_gallery_button_id_raw: Option<u64>,
    request_native_main_menu_launch: bool,
}

impl StartupCompositionPolicy {
    pub(crate) const fn idle() -> Self {
        Self {
            default_cursor_gallery_button_id_raw: None,
            request_native_main_menu_launch: false,
        }
    }

    pub(crate) const fn default_cursor_gallery(source_button_id_raw: u64) -> Self {
        Self {
            default_cursor_gallery_button_id_raw: Some(source_button_id_raw),
            request_native_main_menu_launch: false,
        }
    }

    pub(crate) const fn native_main_menu() -> Self {
        Self {
            default_cursor_gallery_button_id_raw: None,
            request_native_main_menu_launch: true,
        }
    }

    pub(crate) const fn default_cursor_gallery_button_id_raw(self) -> Option<u64> {
        self.default_cursor_gallery_button_id_raw
    }

    pub(crate) const fn request_native_main_menu_launch(self) -> bool {
        self.request_native_main_menu_launch
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum StartupCompositionEntryKind {
    SessionOnly,
    DefaultComposition { source_button_id_raw: u64 },
    ExplicitNativeMainMenu,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct StartupCompositionPolicyDerivation {
    pub(crate) policy: StartupCompositionPolicy,
    pub(crate) event: StartupCompositionPolicyDerivedEvent,
}

pub(crate) fn derive_startup_composition_policy(
    bootstrap_plan: &StartupBootstrapPlan,
    entry_kind: StartupCompositionEntryKind,
) -> StartupCompositionPolicyDerivation {
    let startup_smoke_requested = bootstrap_plan.global_args.startup_smoke;
    let (entry_kind_name, policy) = match entry_kind {
        StartupCompositionEntryKind::SessionOnly => {
            ("session_only", StartupCompositionPolicy::idle())
        }
        StartupCompositionEntryKind::DefaultComposition {
            source_button_id_raw,
        } => (
            "default_composition",
            StartupCompositionPolicy::default_cursor_gallery(source_button_id_raw),
        ),
        StartupCompositionEntryKind::ExplicitNativeMainMenu => (
            "explicit_native_main_menu",
            StartupCompositionPolicy::native_main_menu(),
        ),
    };

    StartupCompositionPolicyDerivation {
        policy,
        event: StartupCompositionPolicyDerivedEvent {
            entry_kind: entry_kind_name,
            startup_smoke_requested,
            default_cursor_gallery_button_id_raw: policy.default_cursor_gallery_button_id_raw(),
            request_native_main_menu_launch: policy.request_native_main_menu_launch(),
        },
    }
}

#[distributed_slice(FEATURE_DEFINITION_REGISTRATIONS)]
pub static STARTUP_FEATURE_REGISTRATION: FeatureDefinitionRegistration =
    FeatureDefinitionRegistration {
        feature_id: STARTUP_FEATURE_ID,
        provenance: registration_provenance!(),
    };

fn handle_startup_composition_ready_trigger(event: &PublishedEvent) -> Vec<PublishedEvent> {
    let Some(ready) = event.downcast_ref::<StartupCompositionReadyEvent>() else {
        return Vec::new();
    };

    let mut emitted_events = Vec::new();
    if let Some(source_button_id_raw) = ready.default_cursor_gallery_button_id_raw {
        emitted_events.push(PublishedEvent::new(
            &DEFAULT_CURSOR_GALLERY_FLOW_REQUESTED_EVENT_DEFINITION,
            DefaultCursorGalleryFlowRequestedEvent {
                feature_id: CURSOR_GALLERY_FEATURE_ID,
                feature_title: "Cursor Gallery",
                source_button_id_raw,
            },
        ));
        emitted_events.push(PublishedEvent::new(
            &MAIN_MENU_CLICKED_EVENT_DEFINITION,
            MainMenuClickEvent {
                logical_button_id: MainMenuLogicalButtonId::new(source_button_id_raw),
                class_id: CURSOR_GALLERY_BUTTON_CLASS_ID,
                pointer_x: CURSOR_GALLERY_DEFAULT_POINTER_X,
                pointer_y: CURSOR_GALLERY_DEFAULT_POINTER_Y,
                layout_revision: CURSOR_GALLERY_DEFAULT_LAYOUT_REVISION,
            },
        ));
    }

    if ready.request_native_main_menu_launch {
        emitted_events.push(PublishedEvent::new(
            &NATIVE_MAIN_MENU_LAUNCH_REQUESTED_EVENT_DEFINITION,
            NativeMainMenuLaunchRequestedEvent {
                button_count: ready.button_count,
                validated_button_count: ready.validated_button_count,
                pending_button_count: ready.pending_button_count,
                failed_button_count: ready.failed_button_count,
            },
        ));
    }

    emitted_events
}

#[distributed_slice(TRIGGER_REGISTRATIONS)]
pub static STARTUP_COMPOSITION_READY_TRIGGER: TriggerRegistration = TriggerRegistration {
    registration_id: STARTUP_COMPOSITION_READY_TRIGGER_REGISTRATION_ID,
    definition_id: STARTUP_COMPOSITION_READY_TRIGGER_DEFINITION_ID,
    owner_feature_id: STARTUP_FEATURE_ID,
    name: "teamy_studio.startup.derive_launch_requests_from_composition_ready",
    event_definition: &STARTUP_COMPOSITION_READY_EVENT_DEFINITION,
    provenance: registration_provenance!(),
    handler: handle_startup_composition_ready_trigger,
};

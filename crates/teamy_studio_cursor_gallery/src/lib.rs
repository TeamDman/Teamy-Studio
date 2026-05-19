mod native_window;
mod scene;

use std::sync::atomic::{AtomicU64, Ordering};

use linkme::distributed_slice;
use teamy_studio_event_core::{EventDefinition, EventDefinitionId, PublishedEvent};
use teamy_studio_main_menu::{
    MAIN_MENU_BUTTON_CLASS_REGISTRATIONS, MAIN_MENU_CLICKED_EVENT_DEFINITION,
    MainMenuButtonClassId, MainMenuButtonClassRegistration, MainMenuClickEvent,
};
use teamy_studio_registration_core::{
    EVENT_DEFINITION_REGISTRATIONS, EventDefinitionRegistration, TRIGGER_REGISTRATIONS,
    TriggerRegistration,
};
use teamy_studio_shell::WINDOW_CREATE_REQUEST_EVENT_DEFINITION;
use teamy_studio_shell::{
    LogicalWindowId, PresentPolicy, WindowCreateRequest, WindowCreatedEvent, WindowHostOptions,
};

pub use native_window::{
    open_native_cursor_gallery_window, open_native_cursor_gallery_window_on_thread,
};

pub const CURSOR_GALLERY_BUTTON_CLASS_ID: MainMenuButtonClassId =
    MainMenuButtonClassId::from_bytes([2; 16]);

static NEXT_LOGICAL_WINDOW_ID: AtomicU64 = AtomicU64::new(1);

pub static CURSOR_GALLERY_BUTTON_CLASS_REGISTRATION: MainMenuButtonClassRegistration =
    MainMenuButtonClassRegistration {
        class_id: CURSOR_GALLERY_BUTTON_CLASS_ID,
        title: "Cursor Gallery",
        tooltip: "Inspect OS cursor sprites",
        ordinal: 30,
    };

#[distributed_slice(MAIN_MENU_BUTTON_CLASS_REGISTRATIONS)]
pub static CURSOR_GALLERY_MAIN_MENU_BUTTON: MainMenuButtonClassRegistration =
    CURSOR_GALLERY_BUTTON_CLASS_REGISTRATION;

pub static CURSOR_GALLERY_OPEN_INTENT_DEFINITION: EventDefinition = EventDefinition {
    id: EventDefinitionId::from_bytes([0x21; 16]),
    schema_name: "teamy_studio.cursor_gallery.open_intent",
    schema_version: 1,
};

#[distributed_slice(EVENT_DEFINITION_REGISTRATIONS)]
pub static CURSOR_GALLERY_OPEN_INTENT_REGISTRATION: EventDefinitionRegistration =
    EventDefinitionRegistration {
        definition: &CURSOR_GALLERY_OPEN_INTENT_DEFINITION,
    };

fn handle_main_menu_click_trigger(event: &PublishedEvent) -> Vec<PublishedEvent> {
    event
        .downcast_ref::<MainMenuClickEvent>()
        .and_then(|click| reduce_main_menu_click(click, next_logical_window_id()))
        .map(to_published_open_intent_event)
        .into_iter()
        .collect()
}

#[distributed_slice(TRIGGER_REGISTRATIONS)]
pub static CURSOR_GALLERY_OPEN_TRIGGER: TriggerRegistration = TriggerRegistration {
    name: "teamy_studio.cursor_gallery.open_from_main_menu_click",
    event_definition: &MAIN_MENU_CLICKED_EVENT_DEFINITION,
    handler: handle_main_menu_click_trigger,
};

fn handle_open_intent_trigger(event: &PublishedEvent) -> Vec<PublishedEvent> {
    event
        .downcast_ref::<CursorGalleryOpenIntent>()
        .map(reduce_open_intent)
        .map(to_published_window_create_request_event)
        .into_iter()
        .collect()
}

#[distributed_slice(TRIGGER_REGISTRATIONS)]
pub static CURSOR_GALLERY_CREATE_WINDOW_TRIGGER: TriggerRegistration = TriggerRegistration {
    name: "teamy_studio.cursor_gallery.create_window_from_open_intent",
    event_definition: &CURSOR_GALLERY_OPEN_INTENT_DEFINITION,
    handler: handle_open_intent_trigger,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CursorGalleryWindowState {
    pub logical_window_id: LogicalWindowId,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CursorGalleryState {
    pending_window_ids: Vec<LogicalWindowId>,
    windows: Vec<CursorGalleryWindowState>,
}

impl CursorGalleryState {
    pub fn apply_open_intent(&mut self, open_intent: &CursorGalleryOpenIntent) {
        self.pending_window_ids.push(open_intent.logical_window_id);
    }

    pub fn apply_window_created(&mut self, created_window: &WindowCreatedEvent) {
        self.pending_window_ids
            .retain(|window_id| *window_id != created_window.logical_window_id);
        self.windows.push(CursorGalleryWindowState {
            logical_window_id: created_window.logical_window_id,
        });
    }

    #[must_use]
    pub fn pending_window_ids(&self) -> &[LogicalWindowId] {
        &self.pending_window_ids
    }

    #[must_use]
    pub fn windows(&self) -> &[CursorGalleryWindowState] {
        &self.windows
    }
}

fn next_logical_window_id() -> LogicalWindowId {
    LogicalWindowId::new(NEXT_LOGICAL_WINDOW_ID.fetch_add(1, Ordering::Relaxed))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CursorGalleryOpenIntent {
    pub logical_window_id: LogicalWindowId,
    pub source_button_id: teamy_studio_main_menu::MainMenuLogicalButtonId,
}

#[must_use]
pub fn reduce_main_menu_click(
    click: &MainMenuClickEvent,
    logical_window_id: LogicalWindowId,
) -> Option<CursorGalleryOpenIntent> {
    (click.class_id == CURSOR_GALLERY_BUTTON_CLASS_ID).then_some(CursorGalleryOpenIntent {
        logical_window_id,
        source_button_id: click.logical_button_id,
    })
}

#[must_use]
pub fn create_window_request(open_intent: &CursorGalleryOpenIntent) -> WindowCreateRequest {
    WindowCreateRequest {
        logical_window_id: open_intent.logical_window_id,
        title: "Cursor Gallery",
        present_policy: PresentPolicy::Composed,
        host_options: WindowHostOptions::standard_foreground(),
    }
}

#[must_use]
pub fn reduce_open_intent(open_intent: &CursorGalleryOpenIntent) -> WindowCreateRequest {
    create_window_request(open_intent)
}

#[must_use]
pub fn to_published_open_intent_event(open_intent: CursorGalleryOpenIntent) -> PublishedEvent {
    PublishedEvent::new(&CURSOR_GALLERY_OPEN_INTENT_DEFINITION, open_intent)
}

#[must_use]
pub fn to_published_window_create_request_event(request: WindowCreateRequest) -> PublishedEvent {
    PublishedEvent::new(&WINDOW_CREATE_REQUEST_EVENT_DEFINITION, request)
}

pub fn apply_published_event(state: &mut CursorGalleryState, event: &PublishedEvent) {
    if let Some(open_intent) = event.downcast_ref::<CursorGalleryOpenIntent>() {
        state.apply_open_intent(open_intent);
    }
    if let Some(created_window) = event.downcast_ref::<WindowCreatedEvent>() {
        state.apply_window_created(created_window);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CURSOR_GALLERY_BUTTON_CLASS_ID, CursorGalleryState, apply_published_event,
        create_window_request, reduce_main_menu_click, reduce_open_intent,
        to_published_open_intent_event,
    };
    use teamy_studio_main_menu::{MainMenuClickEvent, MainMenuLogicalButtonId};
    use teamy_studio_shell::{
        LogicalWindowId, PresentPolicy, WindowCreatedEvent, WindowHostOptions,
    };

    #[test]
    fn cursor_gallery_click_reduces_to_window_request() {
        let click = MainMenuClickEvent {
            logical_button_id: MainMenuLogicalButtonId::new(7),
            class_id: CURSOR_GALLERY_BUTTON_CLASS_ID,
            pointer_x: 10,
            pointer_y: 20,
            layout_revision: 1,
        };

        let open_intent = reduce_main_menu_click(&click, LogicalWindowId::new(42))
            .expect("cursor gallery button should open a window");
        let request = create_window_request(&open_intent);

        assert_eq!(request.logical_window_id.raw(), 42);
        assert_eq!(request.title, "Cursor Gallery");
        assert_eq!(
            request.host_options,
            WindowHostOptions::standard_foreground()
        );
    }

    #[test]
    fn open_intent_reduces_to_same_window_request_shape() {
        let click = MainMenuClickEvent {
            logical_button_id: MainMenuLogicalButtonId::new(7),
            class_id: CURSOR_GALLERY_BUTTON_CLASS_ID,
            pointer_x: 10,
            pointer_y: 20,
            layout_revision: 1,
        };

        let open_intent = reduce_main_menu_click(&click, LogicalWindowId::new(42))
            .expect("cursor gallery button should open a window");
        let request = reduce_open_intent(&open_intent);

        assert_eq!(request.logical_window_id.raw(), 42);
        assert_eq!(request.title, "Cursor Gallery");
        assert_eq!(
            request.host_options,
            WindowHostOptions::standard_foreground()
        );
    }

    #[test]
    fn feature_state_tracks_pending_and_created_windows() {
        let click = MainMenuClickEvent {
            logical_button_id: MainMenuLogicalButtonId::new(7),
            class_id: CURSOR_GALLERY_BUTTON_CLASS_ID,
            pointer_x: 10,
            pointer_y: 20,
            layout_revision: 1,
        };
        let open_intent = reduce_main_menu_click(&click, LogicalWindowId::new(42))
            .expect("cursor gallery button should open a window");
        let mut state = CursorGalleryState::default();

        apply_published_event(&mut state, &to_published_open_intent_event(open_intent));
        assert_eq!(state.pending_window_ids(), &[LogicalWindowId::new(42)]);
        assert!(state.windows().is_empty());

        state.apply_window_created(&WindowCreatedEvent {
            logical_window_id: LogicalWindowId::new(42),
            title: "Cursor Gallery",
            present_policy: PresentPolicy::LowLatencyHwnd,
            host_options: WindowHostOptions::standard_foreground(),
        });

        assert!(state.pending_window_ids().is_empty());
        assert_eq!(state.windows().len(), 1);
        assert_eq!(state.windows()[0].logical_window_id.raw(), 42);
    }
}

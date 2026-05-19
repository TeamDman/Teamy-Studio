mod native_window;
mod scene;

use linkme::distributed_slice;
use teamy_studio_event_core::{EventDefinition, EventDefinitionId, EventLogIntent, PublishedEvent};
use teamy_studio_registration_core::{
    EVENT_DEFINITION_REGISTRATIONS, EventDefinitionRegistration, registration_provenance,
};

pub use native_window::{
    run_native_main_menu_window, run_native_main_menu_window_with_click_handler,
    show_main_menu_info_dialog,
};
pub use scene::{
    MainMenuButtonCardLayout, MainMenuSceneButtonState, build_main_menu_scene,
    layout_main_menu_button_cards,
};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct MainMenuButtonClassId([u8; 16]);

impl MainMenuButtonClassId {
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    #[must_use]
    pub const fn as_bytes(self) -> [u8; 16] {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct MainMenuLogicalButtonId(u64);

impl MainMenuLogicalButtonId {
    #[must_use]
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    #[must_use]
    pub const fn raw(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FeatureValidationState {
    Pending,
    Validated,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MainMenuButtonClassRegistration {
    pub class_id: MainMenuButtonClassId,
    pub title: &'static str,
    pub tooltip: &'static str,
    pub ordinal: u32,
}

#[distributed_slice]
pub static MAIN_MENU_BUTTON_CLASS_REGISTRATIONS: [MainMenuButtonClassRegistration];

pub static MAIN_MENU_CLICKED_EVENT_DEFINITION: EventDefinition = EventDefinition {
    id: EventDefinitionId::from_bytes([0x10; 16]),
    schema_name: "teamy_studio.main_menu.clicked",
    schema_version: 1,
    log_intent: EventLogIntent::NONE,
};

#[distributed_slice(EVENT_DEFINITION_REGISTRATIONS)]
pub static MAIN_MENU_CLICKED_EVENT_REGISTRATION: EventDefinitionRegistration =
    EventDefinitionRegistration {
        definition: &MAIN_MENU_CLICKED_EVENT_DEFINITION,
        provenance: registration_provenance!(),
    };

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MainMenuLogicalButton {
    pub logical_button_id: MainMenuLogicalButtonId,
    pub class_id: MainMenuButtonClassId,
    pub title: &'static str,
    pub tooltip: &'static str,
    pub validation_state: FeatureValidationState,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MainMenuSnapshot {
    buttons: Vec<MainMenuLogicalButton>,
}

impl MainMenuSnapshot {
    #[must_use]
    pub fn from_registrations(registrations: &[&'static MainMenuButtonClassRegistration]) -> Self {
        let buttons = registrations
            .iter()
            .enumerate()
            .map(|(index, registration)| MainMenuLogicalButton {
                logical_button_id: MainMenuLogicalButtonId::new(index as u64 + 1),
                class_id: registration.class_id,
                title: registration.title,
                tooltip: registration.tooltip,
                validation_state: FeatureValidationState::Pending,
            })
            .collect();

        Self { buttons }
    }

    #[must_use]
    pub fn buttons(&self) -> &[MainMenuLogicalButton] {
        &self.buttons
    }

    #[must_use]
    pub fn button_by_class_id(
        &self,
        class_id: MainMenuButtonClassId,
    ) -> Option<&MainMenuLogicalButton> {
        self.buttons
            .iter()
            .find(|button| button.class_id == class_id)
    }

    pub fn set_all_validation_states(&mut self, validation_state: FeatureValidationState) {
        for button in &mut self.buttons {
            button.validation_state = validation_state;
        }
    }

    pub fn set_validation_state_for_class_id(
        &mut self,
        class_id: MainMenuButtonClassId,
        validation_state: FeatureValidationState,
    ) {
        for button in &mut self.buttons {
            if button.class_id == class_id {
                button.validation_state = validation_state;
            }
        }
    }

    #[must_use]
    pub fn click_button(
        &self,
        logical_button_id: MainMenuLogicalButtonId,
        pointer_x: i32,
        pointer_y: i32,
        layout_revision: u64,
    ) -> Option<MainMenuClickEvent> {
        self.buttons
            .iter()
            .find(|button| {
                button.logical_button_id == logical_button_id
                    && button.validation_state != FeatureValidationState::Failed
            })
            .map(|button| MainMenuClickEvent {
                logical_button_id: button.logical_button_id,
                class_id: button.class_id,
                pointer_x,
                pointer_y,
                layout_revision,
            })
    }

    #[must_use]
    pub fn publish_click(
        &self,
        logical_button_id: MainMenuLogicalButtonId,
        pointer_x: i32,
        pointer_y: i32,
        layout_revision: u64,
    ) -> Option<PublishedEvent> {
        self.click_button(logical_button_id, pointer_x, pointer_y, layout_revision)
            .map(to_published_click_event)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MainMenuClickEvent {
    pub logical_button_id: MainMenuLogicalButtonId,
    pub class_id: MainMenuButtonClassId,
    pub pointer_x: i32,
    pub pointer_y: i32,
    pub layout_revision: u64,
}

#[must_use]
pub fn registered_button_classes() -> Vec<&'static MainMenuButtonClassRegistration> {
    let mut registrations: Vec<_> = MAIN_MENU_BUTTON_CLASS_REGISTRATIONS.iter().collect();
    registrations.sort_by_key(|registration| (registration.ordinal, registration.title));
    registrations
}

#[must_use]
pub fn to_published_click_event(click_event: MainMenuClickEvent) -> PublishedEvent {
    PublishedEvent::new(&MAIN_MENU_CLICKED_EVENT_DEFINITION, click_event)
}

#[cfg(test)]
mod tests {
    use super::{
        FeatureValidationState, MainMenuButtonClassId, MainMenuButtonClassRegistration,
        MainMenuSnapshot,
    };

    static VALIDATION_TEST_REGISTRATION: MainMenuButtonClassRegistration =
        MainMenuButtonClassRegistration {
            class_id: MainMenuButtonClassId::from_bytes([7; 16]),
            title: "Cursor Gallery",
            tooltip: "Inspect OS cursor sprites",
            ordinal: 100,
        };

    static CLICK_TEST_REGISTRATION: MainMenuButtonClassRegistration =
        MainMenuButtonClassRegistration {
            class_id: MainMenuButtonClassId::from_bytes([8; 16]),
            title: "Cursor Gallery",
            tooltip: "Inspect OS cursor sprites",
            ordinal: 100,
        };

    #[test]
    fn menu_snapshot_initializes_buttons_as_pending_and_can_validate_them() {
        let mut snapshot = MainMenuSnapshot::from_registrations(&[&VALIDATION_TEST_REGISTRATION]);

        assert_eq!(
            snapshot.buttons()[0].validation_state,
            FeatureValidationState::Pending
        );

        snapshot.set_all_validation_states(FeatureValidationState::Validated);

        assert_eq!(
            snapshot.buttons()[0].validation_state,
            FeatureValidationState::Validated
        );
    }

    #[test]
    fn click_button_uses_registered_button_identity() {
        let mut snapshot = MainMenuSnapshot::from_registrations(&[&CLICK_TEST_REGISTRATION]);
        snapshot.set_all_validation_states(FeatureValidationState::Validated);

        let click = snapshot
            .click_button(snapshot.buttons()[0].logical_button_id, 10, 20, 3)
            .expect("button should be clickable");

        assert_eq!(click.class_id, CLICK_TEST_REGISTRATION.class_id);
        assert_eq!(click.pointer_x, 10);
        assert_eq!(click.pointer_y, 20);
        assert_eq!(click.layout_revision, 3);
    }

    #[test]
    fn pending_button_can_publish_and_create_click_event() {
        let snapshot = MainMenuSnapshot::from_registrations(&[&CLICK_TEST_REGISTRATION]);

        assert!(
            snapshot
                .click_button(snapshot.buttons()[0].logical_button_id, 10, 20, 3)
                .is_some()
        );
        assert!(
            snapshot
                .publish_click(snapshot.buttons()[0].logical_button_id, 10, 20, 3)
                .is_some()
        );
    }

    #[test]
    fn failed_button_cannot_publish_or_create_click_event() {
        let mut snapshot = MainMenuSnapshot::from_registrations(&[&CLICK_TEST_REGISTRATION]);
        snapshot.set_validation_state_for_class_id(
            CLICK_TEST_REGISTRATION.class_id,
            FeatureValidationState::Failed,
        );

        assert!(
            snapshot
                .click_button(snapshot.buttons()[0].logical_button_id, 10, 20, 3)
                .is_none()
        );
        assert!(
            snapshot
                .publish_click(snapshot.buttons()[0].logical_button_id, 10, 20, 3)
                .is_none()
        );
    }

    #[test]
    fn snapshot_exposes_button_metadata_by_class_id() {
        let snapshot = MainMenuSnapshot::from_registrations(&[&CLICK_TEST_REGISTRATION]);

        let button = snapshot
            .button_by_class_id(CLICK_TEST_REGISTRATION.class_id)
            .expect("button should be discoverable by class id");

        assert_eq!(button.title, "Cursor Gallery");
        assert_eq!(button.tooltip, "Inspect OS cursor sprites");
    }

    #[test]
    fn publish_click_wraps_button_click_as_published_event() {
        let mut snapshot = MainMenuSnapshot::from_registrations(&[&CLICK_TEST_REGISTRATION]);
        snapshot.set_all_validation_states(FeatureValidationState::Validated);

        let published = snapshot
            .publish_click(snapshot.buttons()[0].logical_button_id, 10, 20, 3)
            .expect("button should publish a click event");
        let click = published
            .downcast_ref::<super::MainMenuClickEvent>()
            .expect("published payload should be a main menu click event");

        assert_eq!(click.class_id, CLICK_TEST_REGISTRATION.class_id);
        assert_eq!(
            published.definition().id,
            super::MAIN_MENU_CLICKED_EVENT_DEFINITION.id
        );
    }
}

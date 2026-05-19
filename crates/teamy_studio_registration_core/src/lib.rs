use std::collections::HashSet;
use std::error::Error;
use std::fmt::{Display, Formatter};

use facet::Facet;
use linkme::distributed_slice;
use teamy_studio_event_core::{EventDefinition, EventDefinitionId, PublishedEvent};
use uuid::Uuid;

pub type TriggerHandler = fn(&PublishedEvent) -> Vec<PublishedEvent>;

pub const TEAMY_STUDIO_REPOSITORY_URL: &str = "https://github.com/TeamDman/Teamy-Studio";

#[derive(Clone, Copy, Debug, Eq, Facet, PartialEq)]
pub struct RegistrationProvenance {
    pub repository_url: &'static str,
    pub repo_relative_path: &'static str,
    pub local_source_path: &'static str,
    pub module_path: &'static str,
    pub line: u32,
    pub column: u32,
    pub revision: Option<&'static str>,
}

impl RegistrationProvenance {
    #[must_use]
    pub const fn new(
        repository_url: &'static str,
        repo_relative_path: &'static str,
        local_source_path: &'static str,
        module_path: &'static str,
        line: u32,
        column: u32,
        revision: Option<&'static str>,
    ) -> Self {
        Self {
            repository_url,
            repo_relative_path,
            local_source_path,
            module_path,
            line,
            column,
            revision,
        }
    }
}

#[macro_export]
macro_rules! registration_provenance {
    () => {
        $crate::RegistrationProvenance::new(
            $crate::TEAMY_STUDIO_REPOSITORY_URL,
            file!(),
            file!(),
            module_path!(),
            line!(),
            column!(),
            option_env!("TEAMY_STUDIO_GIT_REVISION"),
        )
    };
}

#[derive(Clone, Copy, Debug, Eq, Facet, Hash, PartialEq)]
pub struct FeatureId(Uuid);

impl FeatureId {
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(Uuid::from_bytes(bytes))
    }

    #[must_use]
    pub const fn as_bytes(self) -> [u8; 16] {
        self.0.into_bytes()
    }
}

#[derive(Clone, Copy, Debug, Eq, Facet, Hash, PartialEq)]
pub struct TriggerDefinitionId(Uuid);

impl TriggerDefinitionId {
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(Uuid::from_bytes(bytes))
    }

    #[must_use]
    pub const fn as_bytes(self) -> [u8; 16] {
        self.0.into_bytes()
    }
}

#[derive(Clone, Copy, Debug, Eq, Facet, Hash, PartialEq)]
pub struct TriggerRegistrationId(Uuid);

impl TriggerRegistrationId {
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(Uuid::from_bytes(bytes))
    }

    #[must_use]
    pub const fn as_bytes(self) -> [u8; 16] {
        self.0.into_bytes()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FeatureDefinitionRegistration {
    pub feature_id: FeatureId,
    pub provenance: RegistrationProvenance,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EventDefinitionRegistration {
    pub definition: &'static EventDefinition,
    pub provenance: RegistrationProvenance,
}

#[derive(Clone, Copy, Debug)]
pub struct TriggerRegistration {
    pub registration_id: TriggerRegistrationId,
    pub definition_id: TriggerDefinitionId,
    pub owner_feature_id: FeatureId,
    pub name: &'static str,
    pub event_definition: &'static EventDefinition,
    pub provenance: RegistrationProvenance,
    pub handler: TriggerHandler,
}

#[distributed_slice]
pub static FEATURE_DEFINITION_REGISTRATIONS: [FeatureDefinitionRegistration];

#[distributed_slice]
pub static EVENT_DEFINITION_REGISTRATIONS: [EventDefinitionRegistration];

#[distributed_slice]
pub static TRIGGER_REGISTRATIONS: [TriggerRegistration];

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RegistrationSnapshot {
    pub feature_count: usize,
    pub event_definition_count: usize,
    pub trigger_count: usize,
}

#[must_use]
pub fn snapshot() -> RegistrationSnapshot {
    RegistrationSnapshot {
        feature_count: FEATURE_DEFINITION_REGISTRATIONS.len(),
        event_definition_count: EVENT_DEFINITION_REGISTRATIONS.len(),
        trigger_count: TRIGGER_REGISTRATIONS.len(),
    }
}

#[must_use]
pub fn trigger_registrations_for(
    event_definition_id: EventDefinitionId,
) -> Vec<&'static TriggerRegistration> {
    TRIGGER_REGISTRATIONS
        .iter()
        .filter(|registration| registration.event_definition.id == event_definition_id)
        .collect()
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RegistrationValidationError {
    DuplicateFeatureId {
        feature_id: FeatureId,
        first_provenance: RegistrationProvenance,
        duplicate_provenance: RegistrationProvenance,
    },
    DuplicateEventDefinitionId {
        event_definition_id: EventDefinitionId,
        first_provenance: RegistrationProvenance,
        duplicate_provenance: RegistrationProvenance,
    },
    DuplicateTriggerRegistrationId {
        trigger_registration_id: TriggerRegistrationId,
        first_provenance: RegistrationProvenance,
        duplicate_provenance: RegistrationProvenance,
    },
    DuplicateTriggerDefinitionId {
        trigger_definition_id: TriggerDefinitionId,
        first_provenance: RegistrationProvenance,
        duplicate_provenance: RegistrationProvenance,
    },
    TriggerOwnedByUnregisteredFeature {
        trigger_name: &'static str,
        trigger_registration_id: TriggerRegistrationId,
        trigger_definition_id: TriggerDefinitionId,
        feature_id: FeatureId,
        trigger_provenance: RegistrationProvenance,
    },
    TriggerTargetsUnregisteredDefinition {
        trigger_name: &'static str,
        trigger_registration_id: TriggerRegistrationId,
        trigger_definition_id: TriggerDefinitionId,
        owner_feature_id: FeatureId,
        event_definition_id: EventDefinitionId,
        trigger_provenance: RegistrationProvenance,
    },
}

impl Display for RegistrationValidationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateFeatureId { feature_id, .. } => {
                write!(
                    formatter,
                    "duplicate feature registration for {:?}",
                    feature_id
                )
            }
            Self::DuplicateEventDefinitionId {
                event_definition_id,
                ..
            } => {
                write!(
                    formatter,
                    "duplicate event definition registration for {:?}",
                    event_definition_id
                )
            }
            Self::DuplicateTriggerRegistrationId {
                trigger_registration_id,
                ..
            } => {
                write!(
                    formatter,
                    "duplicate trigger registration identity for {:?}",
                    trigger_registration_id
                )
            }
            Self::DuplicateTriggerDefinitionId {
                trigger_definition_id,
                ..
            } => {
                write!(
                    formatter,
                    "duplicate trigger definition identity for {:?}",
                    trigger_definition_id
                )
            }
            Self::TriggerOwnedByUnregisteredFeature {
                trigger_name,
                feature_id,
                ..
            } => write!(
                formatter,
                "trigger {trigger_name} is owned by unregistered feature {:?}",
                feature_id
            ),
            Self::TriggerTargetsUnregisteredDefinition {
                trigger_name,
                event_definition_id,
                ..
            } => write!(
                formatter,
                "trigger {trigger_name} targets unregistered event definition {:?}",
                event_definition_id
            ),
        }
    }
}

impl Error for RegistrationValidationError {}

pub type RegistrationValidationResult = Result<(), Box<RegistrationValidationError>>;

pub fn validate_registrations() -> RegistrationValidationResult {
    validate_registration_slices(
        &FEATURE_DEFINITION_REGISTRATIONS,
        &EVENT_DEFINITION_REGISTRATIONS,
        &TRIGGER_REGISTRATIONS,
    )
}

pub fn validate_registration_slices(
    feature_registrations: &[FeatureDefinitionRegistration],
    event_definition_registrations: &[EventDefinitionRegistration],
    trigger_registrations: &[TriggerRegistration],
) -> RegistrationValidationResult {
    let mut feature_ids = HashSet::new();
    for registration in feature_registrations {
        if !feature_ids.insert(registration.feature_id) {
            let first_provenance = feature_registrations
                .iter()
                .find(|existing| existing.feature_id == registration.feature_id)
                .expect("insert failure proves an existing feature registration")
                .provenance;
            return Err(Box::new(RegistrationValidationError::DuplicateFeatureId {
                feature_id: registration.feature_id,
                first_provenance,
                duplicate_provenance: registration.provenance,
            }));
        }
    }

    let mut event_definition_ids = HashSet::new();
    for registration in event_definition_registrations {
        if !event_definition_ids.insert(registration.definition.id) {
            let first_provenance = event_definition_registrations
                .iter()
                .find(|existing| existing.definition.id == registration.definition.id)
                .expect("insert failure proves an existing event definition registration")
                .provenance;
            return Err(Box::new(
                RegistrationValidationError::DuplicateEventDefinitionId {
                    event_definition_id: registration.definition.id,
                    first_provenance,
                    duplicate_provenance: registration.provenance,
                },
            ));
        }
    }

    let mut trigger_registration_ids = HashSet::new();
    let mut trigger_definition_ids = HashSet::new();
    for registration in trigger_registrations {
        if !trigger_registration_ids.insert(registration.registration_id) {
            let first_provenance = trigger_registrations
                .iter()
                .find(|existing| existing.registration_id == registration.registration_id)
                .expect("insert failure proves an existing trigger registration")
                .provenance;
            return Err(Box::new(
                RegistrationValidationError::DuplicateTriggerRegistrationId {
                    trigger_registration_id: registration.registration_id,
                    first_provenance,
                    duplicate_provenance: registration.provenance,
                },
            ));
        }
        if !trigger_definition_ids.insert(registration.definition_id) {
            let first_provenance = trigger_registrations
                .iter()
                .find(|existing| existing.definition_id == registration.definition_id)
                .expect("insert failure proves an existing trigger definition")
                .provenance;
            return Err(Box::new(
                RegistrationValidationError::DuplicateTriggerDefinitionId {
                    trigger_definition_id: registration.definition_id,
                    first_provenance,
                    duplicate_provenance: registration.provenance,
                },
            ));
        }
        if !feature_ids.contains(&registration.owner_feature_id) {
            return Err(Box::new(
                RegistrationValidationError::TriggerOwnedByUnregisteredFeature {
                    trigger_name: registration.name,
                    trigger_registration_id: registration.registration_id,
                    trigger_definition_id: registration.definition_id,
                    feature_id: registration.owner_feature_id,
                    trigger_provenance: registration.provenance,
                },
            ));
        }
        if !event_definition_ids.contains(&registration.event_definition.id) {
            return Err(Box::new(
                RegistrationValidationError::TriggerTargetsUnregisteredDefinition {
                    trigger_name: registration.name,
                    trigger_registration_id: registration.registration_id,
                    trigger_definition_id: registration.definition_id,
                    owner_feature_id: registration.owner_feature_id,
                    event_definition_id: registration.event_definition.id,
                    trigger_provenance: registration.provenance,
                },
            ));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        EventDefinitionRegistration, FeatureDefinitionRegistration, FeatureId,
        RegistrationValidationError, TEAMY_STUDIO_REPOSITORY_URL, TriggerDefinitionId,
        TriggerRegistration, TriggerRegistrationId, snapshot, validate_registration_slices,
        validate_registrations,
    };
    use facet::Facet;
    use teamy_studio_event_core::{
        EventDefinition, EventDefinitionId, EventLogIntent, PublishedEvent,
    };

    static TEST_EVENT_DEFINITION: EventDefinition = EventDefinition {
        id: EventDefinitionId::from_bytes([0xAA; 16]),
        schema_name: "teamy_studio.test.synthetic",
        schema_version: 1,
        log_intent: EventLogIntent::NONE,
    };

    fn no_op_trigger_handler(_: &PublishedEvent) -> Vec<PublishedEvent> {
        Vec::new()
    }

    #[test]
    fn empty_registry_reports_zero_counts() {
        let snapshot = snapshot();

        assert_eq!(snapshot.feature_count, 0);
        assert_eq!(snapshot.event_definition_count, 0);
        assert_eq!(snapshot.trigger_count, 0);
    }

    #[test]
    fn empty_registry_validates() {
        assert!(validate_registrations().is_ok());
    }

    #[test]
    fn registration_identity_types_implement_facet() {
        fn assert_facet<T>()
        where
            T: for<'facet> Facet<'facet>,
        {
        }

        assert_facet::<FeatureId>();
        assert_facet::<TriggerDefinitionId>();
        assert_facet::<TriggerRegistrationId>();
        assert_facet::<super::RegistrationProvenance>();
    }

    #[test]
    fn registration_provenance_macro_captures_source_context() {
        let provenance = registration_provenance!();

        assert_eq!(provenance.repository_url, TEAMY_STUDIO_REPOSITORY_URL);
        assert!(provenance.repo_relative_path.ends_with("src\\lib.rs"));
        assert_eq!(provenance.local_source_path, provenance.repo_relative_path);
        assert!(provenance.module_path.ends_with("tests"));
        assert!(provenance.line > 0);
        assert!(provenance.column > 0);
    }

    #[test]
    fn validation_failure_carries_trigger_identity_and_provenance() {
        let feature_registration = FeatureDefinitionRegistration {
            feature_id: FeatureId::from_bytes([1; 16]),
            provenance: registration_provenance!(),
        };
        let event_registration = EventDefinitionRegistration {
            definition: &TEST_EVENT_DEFINITION,
            provenance: registration_provenance!(),
        };
        let missing_feature_id = FeatureId::from_bytes([2; 16]);
        let trigger_provenance = registration_provenance!();
        let trigger_registration = TriggerRegistration {
            registration_id: TriggerRegistrationId::from_bytes([3; 16]),
            definition_id: TriggerDefinitionId::from_bytes([4; 16]),
            owner_feature_id: missing_feature_id,
            name: "teamy_studio.test.synthetic_trigger",
            event_definition: &TEST_EVENT_DEFINITION,
            provenance: trigger_provenance,
            handler: no_op_trigger_handler,
        };

        let error = validate_registration_slices(
            &[feature_registration],
            &[event_registration],
            &[trigger_registration],
        )
        .expect_err("unregistered trigger owner should fail validation");

        assert_eq!(
            *error,
            RegistrationValidationError::TriggerOwnedByUnregisteredFeature {
                trigger_name: "teamy_studio.test.synthetic_trigger",
                trigger_registration_id: TriggerRegistrationId::from_bytes([3; 16]),
                trigger_definition_id: TriggerDefinitionId::from_bytes([4; 16]),
                feature_id: missing_feature_id,
                trigger_provenance,
            }
        );
    }
}

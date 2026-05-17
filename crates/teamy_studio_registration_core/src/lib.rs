use std::collections::HashSet;
use std::error::Error;
use std::fmt::{Display, Formatter};

use linkme::distributed_slice;
use teamy_studio_event_core::{EventDefinition, EventDefinitionId, PublishedEvent};

pub type TriggerHandler = fn(&PublishedEvent) -> Vec<PublishedEvent>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EventDefinitionRegistration {
    pub definition: &'static EventDefinition,
}

#[derive(Clone, Copy, Debug)]
pub struct TriggerRegistration {
    pub name: &'static str,
    pub event_definition: &'static EventDefinition,
    pub handler: TriggerHandler,
}

#[distributed_slice]
pub static EVENT_DEFINITION_REGISTRATIONS: [EventDefinitionRegistration];

#[distributed_slice]
pub static TRIGGER_REGISTRATIONS: [TriggerRegistration];

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RegistrationSnapshot {
    pub event_definition_count: usize,
    pub trigger_count: usize,
}

#[must_use]
pub fn snapshot() -> RegistrationSnapshot {
    RegistrationSnapshot {
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
    DuplicateEventDefinitionId(EventDefinitionId),
    TriggerTargetsUnregisteredDefinition {
        trigger_name: &'static str,
        event_definition_id: EventDefinitionId,
    },
}

impl Display for RegistrationValidationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateEventDefinitionId(id) => {
                write!(
                    formatter,
                    "duplicate event definition registration for {:?}",
                    id
                )
            }
            Self::TriggerTargetsUnregisteredDefinition {
                trigger_name,
                event_definition_id,
            } => write!(
                formatter,
                "trigger {trigger_name} targets unregistered event definition {:?}",
                event_definition_id
            ),
        }
    }
}

impl Error for RegistrationValidationError {}

pub fn validate_registrations() -> Result<(), RegistrationValidationError> {
    let mut event_definition_ids = HashSet::new();
    for registration in EVENT_DEFINITION_REGISTRATIONS {
        if !event_definition_ids.insert(registration.definition.id) {
            return Err(RegistrationValidationError::DuplicateEventDefinitionId(
                registration.definition.id,
            ));
        }
    }

    for registration in TRIGGER_REGISTRATIONS {
        if !event_definition_ids.contains(&registration.event_definition.id) {
            return Err(
                RegistrationValidationError::TriggerTargetsUnregisteredDefinition {
                    trigger_name: registration.name,
                    event_definition_id: registration.event_definition.id,
                },
            );
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{snapshot, validate_registrations};

    #[test]
    fn empty_registry_reports_zero_counts() {
        let snapshot = snapshot();

        assert_eq!(snapshot.event_definition_count, 0);
        assert_eq!(snapshot.trigger_count, 0);
    }

    #[test]
    fn empty_registry_validates() {
        assert!(validate_registrations().is_ok());
    }
}

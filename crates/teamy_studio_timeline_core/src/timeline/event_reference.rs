use facet::Facet;
use teamy_studio_event_core::{EventDefinitionId, EventId};

use super::{ArenaOffset, Femtoseconds, TimelineArithmeticError, TimelineOffset};

#[derive(Clone, Copy, Debug, Eq, Facet, PartialEq)]
pub struct EventReference {
    pub event_id: EventId,
    pub event_definition_id: EventDefinitionId,
    pub timeline_offset_hint: TimelineOffset<i128, Femtoseconds>,
}

impl EventReference {
    #[must_use]
    pub const fn new(
        event_id: EventId,
        event_definition_id: EventDefinitionId,
        timeline_offset_hint: TimelineOffset<i128, Femtoseconds>,
    ) -> Self {
        Self {
            event_id,
            event_definition_id,
            timeline_offset_hint,
        }
    }

    pub fn from_arena_position(
        event_id: EventId,
        event_definition_id: EventDefinitionId,
        arena_base_offset: TimelineOffset<i128, Femtoseconds>,
        arena_offset: ArenaOffset<i128, Femtoseconds>,
    ) -> Result<Self, TimelineArithmeticError<i128, Femtoseconds>> {
        let timeline_offset_hint = arena_offset.compose_with(arena_base_offset)?;
        Ok(Self::new(
            event_id,
            event_definition_id,
            timeline_offset_hint,
        ))
    }
}

#[cfg(test)]
mod tests {
    use facet::Facet;

    use super::EventReference;
    use crate::timeline::{ArenaOffset, Femtoseconds, TimelineOffset};
    use teamy_studio_event_core::{EventDefinitionId, EventId};

    #[test]
    fn event_reference_can_store_exact_timeline_offset_hint_directly() {
        let event_id = EventId::from_bytes([1; 16]);
        let definition_id = EventDefinitionId::from_bytes([2; 16]);
        let reference = EventReference::new(
            event_id,
            definition_id,
            TimelineOffset::<i128, Femtoseconds>::from_raw(33),
        );

        assert_eq!(reference.event_id, event_id);
        assert_eq!(reference.event_definition_id, definition_id);
        assert_eq!(reference.timeline_offset_hint.raw_value(), 33);
    }

    #[test]
    fn event_reference_composes_arena_base_and_arena_offset() {
        let reference = EventReference::from_arena_position(
            EventId::from_bytes([3; 16]),
            EventDefinitionId::from_bytes([4; 16]),
            TimelineOffset::<i128, Femtoseconds>::from_raw(100),
            ArenaOffset::<i128, Femtoseconds>::from_raw(7),
        )
        .expect("arena composition should succeed");

        assert_eq!(reference.timeline_offset_hint.raw_value(), 107);
    }

    #[test]
    fn event_reference_implements_facet() {
        fn assert_facet<T>()
        where
            T: for<'facet> Facet<'facet>,
        {
        }

        assert_facet::<EventReference>();
    }
}

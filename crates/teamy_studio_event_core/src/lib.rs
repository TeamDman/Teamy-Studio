use std::any::Any;
use std::sync::Arc;

use facet::Facet;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Eq, Facet, Hash, Ord, PartialEq, PartialOrd)]
pub struct EventId(Uuid);

impl EventId {
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(Uuid::from_bytes(bytes))
    }

    #[must_use]
    pub const fn as_bytes(self) -> [u8; 16] {
        self.0.into_bytes()
    }

    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for EventId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug, Eq, Facet, Hash, PartialEq)]
pub struct EventDefinitionId(Uuid);

impl EventDefinitionId {
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(Uuid::from_bytes(bytes))
    }

    #[must_use]
    pub const fn as_bytes(self) -> [u8; 16] {
        self.0.into_bytes()
    }
}

#[derive(Clone, Copy, Debug, Eq, Facet, PartialEq)]
pub struct EventDefinition {
    pub id: EventDefinitionId,
    pub schema_name: &'static str,
    pub schema_version: u32,
}

pub trait PublicEventShape<'facet>: Facet<'facet> {}

impl<'facet, T> PublicEventShape<'facet> for T where T: Facet<'facet> {}

pub trait CanonicalEvent: Clone + Send + Sync + 'static {}

impl<T> CanonicalEvent for T where T: Clone + Send + Sync + 'static {}

#[derive(Clone)]
pub struct PublishedEvent {
    definition: &'static EventDefinition,
    payload: Arc<dyn Any + Send + Sync>,
}

impl PublishedEvent {
    #[must_use]
    pub fn new<T>(definition: &'static EventDefinition, payload: T) -> Self
    where
        T: Send + Sync + 'static,
    {
        Self {
            definition,
            payload: Arc::new(payload),
        }
    }

    #[must_use]
    pub fn definition(&self) -> &'static EventDefinition {
        self.definition
    }

    #[must_use]
    pub fn definition_id(&self) -> EventDefinitionId {
        self.definition.id
    }

    #[must_use]
    pub fn downcast_ref<T>(&self) -> Option<&T>
    where
        T: 'static,
    {
        self.payload.downcast_ref::<T>()
    }
}

impl std::fmt::Debug for PublishedEvent {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PublishedEvent")
            .field("definition", &self.definition)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug)]
pub struct WritableArena<T>
where
    T: CanonicalEvent,
{
    arena_name: &'static str,
    event_ids: Vec<EventId>,
    events: Vec<T>,
}

impl<T> WritableArena<T>
where
    T: CanonicalEvent,
{
    #[must_use]
    pub fn new(arena_name: &'static str) -> Self {
        Self {
            arena_name,
            event_ids: Vec::new(),
            events: Vec::new(),
        }
    }

    pub fn push(&mut self, event: T) {
        self.event_ids.push(EventId::new());
        self.events.push(event);
    }

    pub fn push_with_id(&mut self, event_id: EventId, event: T) {
        self.event_ids.push(event_id);
        self.events.push(event);
    }

    #[must_use]
    pub fn seal(self) -> SealedArenaEpoch<T> {
        SealedArenaEpoch {
            arena_name: self.arena_name,
            event_ids: self.event_ids,
            events: self.events,
        }
    }
}

#[derive(Clone, Debug)]
pub struct SealedArenaEpoch<T>
where
    T: CanonicalEvent,
{
    arena_name: &'static str,
    event_ids: Vec<EventId>,
    events: Vec<T>,
}

impl<T> SealedArenaEpoch<T>
where
    T: CanonicalEvent,
{
    #[must_use]
    pub fn arena_name(&self) -> &'static str {
        self.arena_name
    }

    #[must_use]
    pub fn event_ids(&self) -> &[EventId] {
        &self.event_ids
    }

    #[must_use]
    pub fn events(&self) -> &[T] {
        &self.events
    }

    pub fn event_records(&self) -> impl Iterator<Item = (EventId, &T)> + '_ {
        self.event_ids.iter().copied().zip(self.events.iter())
    }
}

#[cfg(test)]
mod tests {
    use super::{EventDefinition, EventDefinitionId, EventId, PublishedEvent, WritableArena};
    use facet::Facet;

    static PUBLISHED_TEST_DEFINITION: EventDefinition = EventDefinition {
        id: EventDefinitionId::from_bytes([2; 16]),
        schema_name: "test.published",
        schema_version: 1,
    };

    #[derive(Clone, Debug, Eq, Facet, PartialEq)]
    struct TestEvent {
        value: u32,
    }

    #[test]
    fn writable_arena_seals_into_epoch() {
        let definition = EventDefinition {
            id: EventDefinitionId::from_bytes([1; 16]),
            schema_name: "test.event",
            schema_version: 1,
        };
        let mut arena = WritableArena::new(definition.schema_name);
        arena.push_with_id(EventId::from_bytes([9; 16]), TestEvent { value: 7 });

        let epoch = arena.seal();

        assert_eq!(epoch.arena_name(), "test.event");
        assert_eq!(epoch.event_ids(), &[EventId::from_bytes([9; 16])]);
        assert_eq!(epoch.events(), &[TestEvent { value: 7 }]);
        assert_eq!(
            epoch.event_records().next(),
            Some((EventId::from_bytes([9; 16]), &TestEvent { value: 7 }))
        );
    }

    #[test]
    fn published_event_supports_downcasting() {
        let event = PublishedEvent::new(&PUBLISHED_TEST_DEFINITION, TestEvent { value: 99 });

        assert_eq!(event.definition_id(), PUBLISHED_TEST_DEFINITION.id);
        assert_eq!(
            event.downcast_ref::<TestEvent>().map(|value| value.value),
            Some(99)
        );
    }

    #[test]
    fn public_event_identity_types_implement_facet() {
        fn assert_facet<T>()
        where
            T: for<'facet> Facet<'facet>,
        {
        }

        assert_facet::<EventId>();
        assert_facet::<EventDefinitionId>();
        assert_facet::<EventDefinition>();
    }
}

mod canonical_time_range;
mod published_event_projection;
mod timeline_dataset;
mod timeline_query;

use facet::Facet;
use teamy_studio_event_core::{
    CanonicalEvent, EventId as CoreEventId, PublishedEvent, SealedArenaEpoch,
};

pub mod timeline;

pub use canonical_time_range::CanonicalTimeRange;
pub use published_event_projection::{
    PublishedEventProjectionSummary, project_published_event_timeline_dataset,
};
pub use timeline::{
    ArenaOffset, EventId, EventReference, Femtoseconds, Microseconds, Milliseconds, Nanoseconds,
    Seconds, TimeUnit, Timeline, TimelineArithmeticError, TimelineArithmeticFailureReason,
    TimelineArithmeticOperation, TimelineId, TimelineOffset, TimelineOrigin, TimelineRepr,
    TimelineTransform, TimelineTransformError, TimelineTransformFailureReason,
    TimelineUnitConversionError, TimelineUnitConversionFailureReason, TimelineUnitExtractionError,
    TimelineUnitExtractionFailureReason,
};
pub use timeline_dataset::{
    TimelineCompactionReport, TimelineDataset, TimelineDatasetRevision, TimelineEventItem,
    TimelineField, TimelineFieldInputValue, TimelineFieldValue, TimelineInternedStringId,
    TimelineItem, TimelineItemId, TimelineItemInput, TimelineItemKind, TimelineItemSequence,
    TimelineObjectRef, TimelineSpanItem, TimelineWriteLogEntry,
};
pub use timeline_query::{
    TimelineGroupingMode, TimelineRenderCluster, TimelineRenderEvent, TimelineRenderItem,
    TimelineRenderPlan, TimelineRenderRow, TimelineRenderRowId, TimelineRenderRowKey,
    TimelineRenderSpan, TimelineViewportQuery,
};

pub type CanonicalArenaOffset = ArenaOffset<i128, Femtoseconds>;
pub type CanonicalTimelineOffset = TimelineOffset<i128, Femtoseconds>;

#[derive(Clone, Copy, Debug, Eq, Facet, Ord, PartialEq, PartialOrd)]
#[facet(opaque, proxy = crate::timeline::TimeLikeFacetProxy)]
pub struct CanonicalTimeKey(CanonicalTimelineOffset);

impl CanonicalTimeKey {
    pub const ZERO: Self = Self(CanonicalTimelineOffset::ZERO);

    #[must_use]
    pub const fn from_offset(offset: CanonicalTimelineOffset) -> Self {
        Self(offset)
    }

    #[must_use]
    pub const fn from_femtoseconds(value: i128) -> Self {
        Self(CanonicalTimelineOffset::from_raw(value))
    }

    #[must_use]
    pub const fn as_offset(self) -> CanonicalTimelineOffset {
        self.0
    }

    #[must_use]
    pub const fn raw_femtoseconds(self) -> i128 {
        self.0.raw_value()
    }
}

impl From<i128> for CanonicalTimeKey {
    fn from(value: i128) -> Self {
        Self::from_femtoseconds(value)
    }
}

impl From<CanonicalTimelineOffset> for CanonicalTimeKey {
    fn from(value: CanonicalTimelineOffset) -> Self {
        Self::from_offset(value)
    }
}

impl From<CanonicalTimeKey> for CanonicalTimelineOffset {
    fn from(value: CanonicalTimeKey) -> Self {
        value.as_offset()
    }
}

impl Default for CanonicalTimeKey {
    fn default() -> Self {
        Self::ZERO
    }
}

impl TryFrom<&CanonicalTimeKey> for crate::timeline::TimeLikeFacetProxy {
    type Error = &'static str;

    fn try_from(value: &CanonicalTimeKey) -> Result<Self, Self::Error> {
        Ok(Self {
            category: crate::timeline::TIME_LIKE_CATEGORY_INSTANT,
            femtoseconds: value.raw_femtoseconds(),
        })
    }
}

impl TryFrom<crate::timeline::TimeLikeFacetProxy> for CanonicalTimeKey {
    type Error = &'static str;

    fn try_from(proxy: crate::timeline::TimeLikeFacetProxy) -> Result<Self, Self::Error> {
        if proxy.category != crate::timeline::TIME_LIKE_CATEGORY_INSTANT {
            return Err("time-like facet proxy category did not match an instant");
        }

        Ok(Self::from_femtoseconds(proxy.femtoseconds))
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, Facet, Ord, PartialEq, PartialOrd)]
pub struct PublicationOrdinal(pub u64);

#[derive(Clone, Copy, Debug, Default, Eq, Facet, Ord, PartialEq, PartialOrd)]
pub struct TriggerCursor {
    pub time_key: CanonicalTimeKey,
    pub publication_ordinal: PublicationOrdinal,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TriggerRuntime {
    last_seen: Option<TriggerCursor>,
}

impl TriggerRuntime {
    #[must_use]
    pub fn last_seen(&self) -> Option<TriggerCursor> {
        self.last_seen
    }

    pub fn set_last_seen(&mut self, cursor: TriggerCursor) {
        self.last_seen = Some(cursor);
    }

    #[must_use]
    pub fn unseen_epochs<T>(
        &self,
        timeline: &ConstructedTimeline<T>,
    ) -> Vec<(TriggerCursor, SealedArenaEpoch<T>)>
    where
        T: CanonicalEvent,
    {
        timeline.unseen_epochs_since(self.last_seen)
    }

    #[must_use]
    pub fn unseen_event_records<'timeline, T>(
        &self,
        timeline: &'timeline ConstructedTimeline<T>,
    ) -> Vec<(TriggerCursor, EventId, &'timeline T)>
    where
        T: CanonicalEvent,
    {
        timeline.unseen_event_records_since(self.last_seen)
    }

    pub fn pump_unseen<T, F, E>(
        &mut self,
        timeline: &ConstructedTimeline<T>,
        mut handler: F,
    ) -> Result<(), E>
    where
        T: CanonicalEvent,
        F: FnMut(TriggerCursor, &T) -> Result<(), E>,
    {
        self.pump_unseen_records(timeline, |cursor, _, event| handler(cursor, event))
    }

    pub fn pump_unseen_records<T, F, E>(
        &mut self,
        timeline: &ConstructedTimeline<T>,
        mut handler: F,
    ) -> Result<(), E>
    where
        T: CanonicalEvent,
        F: FnMut(TriggerCursor, CoreEventId, &T) -> Result<(), E>,
    {
        for (cursor, epoch) in self.unseen_epochs(timeline) {
            for (event_id, event) in epoch.event_records() {
                handler(cursor, event_id, event)?;
            }
            self.set_last_seen(cursor);
        }

        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct ConstructedTimeline<T>
where
    T: CanonicalEvent,
{
    published_epochs: Vec<(TriggerCursor, SealedArenaEpoch<T>)>,
    next_publication_ordinal: u64,
}

impl<T> Default for ConstructedTimeline<T>
where
    T: CanonicalEvent,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<T> ConstructedTimeline<T>
where
    T: CanonicalEvent,
{
    #[must_use]
    pub fn new() -> Self {
        Self {
            published_epochs: Vec::new(),
            next_publication_ordinal: 0,
        }
    }

    pub fn ingest(
        &mut self,
        time_key: CanonicalTimeKey,
        epoch: SealedArenaEpoch<T>,
    ) -> TriggerCursor {
        let cursor = TriggerCursor {
            time_key,
            publication_ordinal: PublicationOrdinal(self.next_publication_ordinal),
        };
        self.next_publication_ordinal += 1;
        self.published_epochs.push((cursor, epoch));
        cursor
    }

    #[must_use]
    pub fn published_epochs(&self) -> &[(TriggerCursor, SealedArenaEpoch<T>)] {
        &self.published_epochs
    }

    #[must_use]
    pub fn unseen_epochs_since(
        &self,
        last_seen: Option<TriggerCursor>,
    ) -> Vec<(TriggerCursor, SealedArenaEpoch<T>)> {
        self.published_epochs
            .iter()
            .filter(|(cursor, _)| last_seen.is_none_or(|seen| *cursor > seen))
            .cloned()
            .collect()
    }

    #[must_use]
    pub fn unseen_event_records_since(
        &self,
        last_seen: Option<TriggerCursor>,
    ) -> Vec<(TriggerCursor, CoreEventId, &T)> {
        self.published_epochs
            .iter()
            .filter(|(cursor, _)| last_seen.is_none_or(|seen| *cursor > seen))
            .flat_map(|(cursor, epoch)| {
                epoch
                    .event_records()
                    .map(move |(event_id, event)| (*cursor, event_id, event))
            })
            .collect()
    }
}

impl ConstructedTimeline<PublishedEvent> {
    #[must_use]
    pub fn published_event_records(&self) -> Vec<(TriggerCursor, CoreEventId, &PublishedEvent)> {
        self.unseen_event_records_since(None)
    }

    #[must_use]
    pub fn event_references(&self) -> Vec<EventReference> {
        self.event_references_since(None)
    }

    #[must_use]
    pub fn latest_event_references(&self) -> Vec<EventReference> {
        self.published_epochs
            .last()
            .map_or_else(Vec::new, |(cursor, epoch)| {
                epoch
                    .event_records()
                    .map(|(event_id, event)| {
                        EventReference::from_arena_position(
                            event_id,
                            event.definition_id(),
                            cursor.time_key.as_offset(),
                            CanonicalArenaOffset::ZERO,
                        )
                        .expect("zero arena offsets should compose with the published time key")
                    })
                    .collect()
            })
    }

    #[must_use]
    pub fn event_references_since(&self, last_seen: Option<TriggerCursor>) -> Vec<EventReference> {
        self.unseen_event_records_since(last_seen)
            .into_iter()
            .map(|(cursor, event_id, event)| {
                EventReference::from_arena_position(
                    event_id,
                    event.definition_id(),
                    cursor.time_key.as_offset(),
                    CanonicalArenaOffset::ZERO,
                )
                .expect("zero arena offsets should compose with the published time key")
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use facet::Facet;

    use super::{
        CanonicalTimeKey, ConstructedTimeline, EventReference, PublicationOrdinal, TriggerCursor,
        TriggerRuntime,
    };
    use crate::timeline::{TIME_LIKE_CATEGORY_INSTANT, TimeLikeFacetProxy};
    use teamy_studio_event_core::{
        EventDefinition, EventDefinitionId, EventId, EventLogIntent, PublishedEvent, WritableArena,
    };

    #[derive(Clone, Debug, Eq, PartialEq)]
    struct TestEvent {
        value: u32,
    }

    static TEST_PUBLISHED_DEFINITION: EventDefinition = EventDefinition {
        id: EventDefinitionId::from_bytes([0x41; 16]),
        schema_name: "timeline.test.published",
        schema_version: 1,
        log_intent: EventLogIntent::NONE,
    };

    #[test]
    fn canonical_public_timeline_types_implement_facet() {
        fn assert_facet<T>()
        where
            T: for<'facet> Facet<'facet>,
        {
        }

        assert_facet::<CanonicalTimeKey>();
        assert_facet::<PublicationOrdinal>();
        assert_facet::<TriggerCursor>();
        assert_facet::<EventReference>();
    }

    #[test]
    fn canonical_time_key_reflects_through_instant_femtosecond_proxy() {
        let value = CanonicalTimeKey::from_femtoseconds(55);
        let proxy = TimeLikeFacetProxy::try_from(&value)
            .expect("canonical time key should convert into the time-like proxy");
        let round_trip = CanonicalTimeKey::try_from(proxy)
            .expect("proxy should round-trip back into the same canonical time key");

        assert_eq!(proxy.category, TIME_LIKE_CATEGORY_INSTANT);
        assert_eq!(proxy.femtoseconds, 55);
        assert_eq!(round_trip, value);
        assert!(CanonicalTimeKey::SHAPE.proxy.is_some());
    }

    #[test]
    fn ingestion_assigns_monotonic_ordinals() {
        let mut timeline = ConstructedTimeline::new();
        let first_epoch = {
            let mut arena = WritableArena::new("timeline.test");
            arena.push(TestEvent { value: 1 });
            arena.seal()
        };
        let second_epoch = {
            let mut arena = WritableArena::new("timeline.test");
            arena.push(TestEvent { value: 2 });
            arena.seal()
        };

        let first = timeline.ingest(CanonicalTimeKey::from_femtoseconds(10), first_epoch);
        let second = timeline.ingest(CanonicalTimeKey::from_femtoseconds(20), second_epoch);

        assert_eq!(first.publication_ordinal.0, 0);
        assert_eq!(second.publication_ordinal.0, 1);
        assert_eq!(timeline.published_epochs().len(), 2);
    }

    #[test]
    fn trigger_runtime_only_processes_unseen_epochs() {
        let mut timeline = ConstructedTimeline::new();
        let mut first_arena = WritableArena::new("timeline.test");
        first_arena.push(TestEvent { value: 1 });
        timeline.ingest(CanonicalTimeKey::from_femtoseconds(10), first_arena.seal());

        let mut runtime = TriggerRuntime::default();
        let mut seen = Vec::new();
        runtime
            .pump_unseen(&timeline, |_, event| {
                seen.push(event.value);
                Ok::<(), ()>(())
            })
            .expect("first pump should succeed");
        runtime
            .pump_unseen(&timeline, |_, event| {
                seen.push(event.value);
                Ok::<(), ()>(())
            })
            .expect("second pump should succeed");

        let mut second_arena = WritableArena::new("timeline.test");
        second_arena.push(TestEvent { value: 2 });
        timeline.ingest(CanonicalTimeKey::from_femtoseconds(20), second_arena.seal());
        runtime
            .pump_unseen(&timeline, |_, event| {
                seen.push(event.value);
                Ok::<(), ()>(())
            })
            .expect("third pump should succeed");

        assert_eq!(seen, vec![1, 2]);
    }

    #[test]
    fn trigger_runtime_only_advances_cursor_after_success() {
        let mut timeline = ConstructedTimeline::new();
        let mut first_arena = WritableArena::new("timeline.test");
        first_arena.push(TestEvent { value: 1 });
        let first_cursor =
            timeline.ingest(CanonicalTimeKey::from_femtoseconds(10), first_arena.seal());

        let mut runtime = TriggerRuntime::default();
        let result = runtime.pump_unseen(&timeline, |_, _| Err::<(), _>("boom"));

        assert_eq!(result, Err("boom"));
        assert_eq!(runtime.last_seen(), None);

        runtime.set_last_seen(first_cursor);
        assert_eq!(runtime.last_seen(), Some(first_cursor));
    }

    #[test]
    fn trigger_runtime_can_expose_unseen_event_records() {
        let mut timeline = ConstructedTimeline::new();
        let mut arena = WritableArena::new("timeline.test");
        arena.push_with_id(EventId::from_bytes([7; 16]), TestEvent { value: 1 });
        timeline.ingest(CanonicalTimeKey::from_femtoseconds(10), arena.seal());

        let runtime = TriggerRuntime::default();
        let records = runtime.unseen_event_records(&timeline);

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].0.time_key.raw_femtoseconds(), 10);
        assert_eq!(records[0].1, EventId::from_bytes([7; 16]));
        assert_eq!(records[0].2.value, 1);
    }

    #[test]
    fn pump_unseen_records_passes_event_ids_to_the_handler() {
        let mut timeline = ConstructedTimeline::new();
        let mut arena = WritableArena::new("timeline.test");
        arena.push_with_id(EventId::from_bytes([8; 16]), TestEvent { value: 11 });
        let cursor = timeline.ingest(CanonicalTimeKey::from_femtoseconds(12), arena.seal());

        let mut runtime = TriggerRuntime::default();
        let mut seen = Vec::new();
        runtime
            .pump_unseen_records(&timeline, |seen_cursor, event_id, event| {
                seen.push((seen_cursor, event_id, event.value));
                Ok::<(), ()>(())
            })
            .expect("record-aware pump should succeed");

        assert_eq!(seen, vec![(cursor, EventId::from_bytes([8; 16]), 11)]);
    }

    #[test]
    fn published_event_records_preserve_epoch_event_ids() {
        let mut timeline = ConstructedTimeline::new();
        let mut arena = WritableArena::new("timeline.test.published");
        arena.push_with_id(
            EventId::from_bytes([9; 16]),
            PublishedEvent::new(&TEST_PUBLISHED_DEFINITION, TestEvent { value: 5 }),
        );
        timeline.ingest(CanonicalTimeKey::from_femtoseconds(22), arena.seal());

        let records = timeline.published_event_records();

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].0.time_key.raw_femtoseconds(), 22);
        assert_eq!(records[0].1, EventId::from_bytes([9; 16]));
        assert_eq!(records[0].2.definition_id(), TEST_PUBLISHED_DEFINITION.id);
    }

    #[test]
    fn event_references_are_constructed_from_published_event_records() {
        let mut timeline = ConstructedTimeline::new();
        let mut first_arena = WritableArena::new("timeline.test.published");
        first_arena.push_with_id(
            EventId::from_bytes([10; 16]),
            PublishedEvent::new(&TEST_PUBLISHED_DEFINITION, TestEvent { value: 1 }),
        );
        let first_cursor =
            timeline.ingest(CanonicalTimeKey::from_femtoseconds(30), first_arena.seal());

        let mut second_arena = WritableArena::new("timeline.test.published");
        second_arena.push_with_id(
            EventId::from_bytes([11; 16]),
            PublishedEvent::new(&TEST_PUBLISHED_DEFINITION, TestEvent { value: 2 }),
        );
        timeline.ingest(CanonicalTimeKey::from_femtoseconds(40), second_arena.seal());

        let all_references = timeline.event_references();
        let since_first = timeline.event_references_since(Some(first_cursor));
        let latest = timeline.latest_event_references();

        assert_eq!(all_references.len(), 2);
        assert_eq!(all_references[0].event_id, EventId::from_bytes([10; 16]));
        assert_eq!(all_references[0].timeline_offset_hint.raw_value(), 30);
        assert_eq!(all_references[1].event_id, EventId::from_bytes([11; 16]));
        assert_eq!(all_references[1].timeline_offset_hint.raw_value(), 40);

        assert_eq!(since_first.len(), 1);
        assert_eq!(since_first[0].event_id, EventId::from_bytes([11; 16]));
        assert_eq!(since_first[0].timeline_offset_hint.raw_value(), 40);

        assert_eq!(latest.len(), 1);
        assert_eq!(latest[0].event_id, EventId::from_bytes([11; 16]));
        assert_eq!(latest[0].timeline_offset_hint.raw_value(), 40);
    }
}

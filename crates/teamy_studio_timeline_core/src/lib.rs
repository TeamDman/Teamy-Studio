use teamy_studio_event_core::{CanonicalEvent, SealedArenaEpoch};

#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub struct CanonicalTimeKey(pub i128);

#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub struct PublicationOrdinal(pub u64);

#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
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

    pub fn pump_unseen<T, F, E>(
        &mut self,
        timeline: &ConstructedTimeline<T>,
        mut handler: F,
    ) -> Result<(), E>
    where
        T: CanonicalEvent,
        F: FnMut(TriggerCursor, &T) -> Result<(), E>,
    {
        for (cursor, epoch) in self.unseen_epochs(timeline) {
            for event in epoch.events() {
                handler(cursor, event)?;
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
}

#[cfg(test)]
mod tests {
    use super::{CanonicalTimeKey, ConstructedTimeline, TriggerRuntime};
    use teamy_studio_event_core::WritableArena;

    #[derive(Clone, Debug, Eq, PartialEq)]
    struct TestEvent {
        value: u32,
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

        let first = timeline.ingest(CanonicalTimeKey(10), first_epoch);
        let second = timeline.ingest(CanonicalTimeKey(20), second_epoch);

        assert_eq!(first.publication_ordinal.0, 0);
        assert_eq!(second.publication_ordinal.0, 1);
        assert_eq!(timeline.published_epochs().len(), 2);
    }

    #[test]
    fn trigger_runtime_only_processes_unseen_epochs() {
        let mut timeline = ConstructedTimeline::new();
        let mut first_arena = WritableArena::new("timeline.test");
        first_arena.push(TestEvent { value: 1 });
        timeline.ingest(CanonicalTimeKey(10), first_arena.seal());

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
        timeline.ingest(CanonicalTimeKey(20), second_arena.seal());
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
        let first_cursor = timeline.ingest(CanonicalTimeKey(10), first_arena.seal());

        let mut runtime = TriggerRuntime::default();
        let result = runtime.pump_unseen(&timeline, |_, _| Err::<(), _>("boom"));

        assert_eq!(result, Err("boom"));
        assert_eq!(runtime.last_seen(), None);

        runtime.set_last_seen(first_cursor);
        assert_eq!(runtime.last_seen(), Some(first_cursor));
    }
}

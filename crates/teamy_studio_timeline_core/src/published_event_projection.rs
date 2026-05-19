use facet::Facet;
use teamy_studio_event_core::EventLogLevel;
use uuid::Uuid;

use crate::{
    ConstructedTimeline, PublishedEvent, TimelineDataset, TimelineFieldInputValue,
    TimelineItemInput,
};

#[derive(Clone, Debug, Facet, PartialEq)]
pub struct PublishedEventProjectionSummary {
    pub event_count: usize,
    pub latest_publication_ordinal: u64,
}

#[must_use]
pub fn project_published_event_timeline_dataset(
    timeline: &ConstructedTimeline<PublishedEvent>,
) -> TimelineDataset {
    let mut dataset = TimelineDataset::new();

    for (cursor, epoch) in timeline.published_epochs() {
        for (event_id, event) in epoch.event_records() {
            let definition = event.definition();
            let log_level = definition.log_intent.level.map(event_log_level_label);
            dataset.push_event(
                TimelineItemInput::new(definition.schema_name)
                    .with_group_key(definition.schema_name)
                    .with_source_key(epoch.arena_name())
                    .with_field(
                        "log_worthy",
                        TimelineFieldInputValue::Bool(definition.log_intent.is_log_worthy()),
                    )
                    .with_field(
                        "log_level",
                        TimelineFieldInputValue::String(log_level.unwrap_or("none").to_owned()),
                    )
                    .with_field(
                        "event_id",
                        TimelineFieldInputValue::String(format_uuid_like(event_id.as_bytes())),
                    )
                    .with_field(
                        "event_definition_id",
                        TimelineFieldInputValue::String(format_uuid_like(definition.id.as_bytes())),
                    )
                    .with_field(
                        "schema_name",
                        TimelineFieldInputValue::String(definition.schema_name.to_owned()),
                    )
                    .with_field(
                        "schema_version",
                        TimelineFieldInputValue::U64(u64::from(definition.schema_version)),
                    )
                    .with_field(
                        "arena_name",
                        TimelineFieldInputValue::String(epoch.arena_name().to_owned()),
                    )
                    .with_field(
                        "publication_ordinal",
                        TimelineFieldInputValue::U64(cursor.publication_ordinal.0),
                    )
                    .with_field(
                        "time_key_femtoseconds",
                        TimelineFieldInputValue::String(
                            cursor.time_key.raw_femtoseconds().to_string(),
                        ),
                    ),
                cursor.time_key,
            );
        }
    }

    let _ = dataset.compact();
    dataset
}

impl ConstructedTimeline<PublishedEvent> {
    #[must_use]
    pub fn projected_timeline_dataset(&self) -> TimelineDataset {
        project_published_event_timeline_dataset(self)
    }

    #[must_use]
    pub fn published_event_projection_summary(&self) -> PublishedEventProjectionSummary {
        PublishedEventProjectionSummary {
            event_count: self
                .published_epochs()
                .iter()
                .map(|(_, epoch)| epoch.events().len())
                .sum(),
            latest_publication_ordinal: self
                .published_epochs()
                .last()
                .map_or(0, |(cursor, _)| cursor.publication_ordinal.0),
        }
    }
}

fn format_uuid_like(bytes: [u8; 16]) -> String {
    Uuid::from_bytes(bytes).hyphenated().to_string()
}

const fn event_log_level_label(level: EventLogLevel) -> &'static str {
    match level {
        EventLogLevel::Trace => "trace",
        EventLogLevel::Debug => "debug",
        EventLogLevel::Info => "info",
        EventLogLevel::Warn => "warn",
        EventLogLevel::Error => "error",
    }
}

#[cfg(test)]
mod tests {
    use super::project_published_event_timeline_dataset;
    use crate::{
        CanonicalTimeKey, ConstructedTimeline, TimelineGroupingMode, TimelineViewportQuery,
    };
    use teamy_studio_event_core::{
        EventDefinition, EventDefinitionId, EventLogIntent, PublishedEvent, WritableArena,
    };

    static FIRST_EVENT_DEFINITION: EventDefinition = EventDefinition {
        id: EventDefinitionId::from_bytes([0x21; 16]),
        schema_name: "teamy.timeline.first",
        schema_version: 1,
        log_intent: EventLogIntent::INFO,
    };

    static SECOND_EVENT_DEFINITION: EventDefinition = EventDefinition {
        id: EventDefinitionId::from_bytes([0x22; 16]),
        schema_name: "teamy.timeline.second",
        schema_version: 1,
        log_intent: EventLogIntent::DEBUG,
    };

    #[derive(Clone, Debug, Eq, PartialEq)]
    struct TestPayload {
        value: u32,
    }

    #[test]
    fn published_events_project_into_a_compacted_dataset() {
        let mut timeline = ConstructedTimeline::new();
        let mut first_epoch = WritableArena::new("teamy.startup.bootstrap");
        first_epoch.push(PublishedEvent::new(
            &FIRST_EVENT_DEFINITION,
            TestPayload { value: 1 },
        ));
        let mut second_epoch = WritableArena::new("teamy.startup");
        second_epoch.push(PublishedEvent::new(
            &SECOND_EVENT_DEFINITION,
            TestPayload { value: 2 },
        ));
        timeline.ingest(CanonicalTimeKey::from_femtoseconds(10), first_epoch.seal());
        timeline.ingest(CanonicalTimeKey::from_femtoseconds(20), second_epoch.seal());

        let dataset = project_published_event_timeline_dataset(&timeline);

        assert_eq!(dataset.items().len(), 2);
        assert_eq!(dataset.pending_write_count(), 0);
        let first_item = dataset.items().first().expect("first projected item");
        assert!(
            first_item
                .fields()
                .iter()
                .any(|field| matches!(field.value(), crate::TimelineFieldValue::Bool(true)))
        );
        assert!(first_item.fields().iter().any(|field| matches!(
            field.value(),
            crate::TimelineFieldValue::String(value)
                if dataset.resolve_string(*value) == Some("info")
        )));
        assert_eq!(
            dataset
                .time_bounds()
                .expect("bounds")
                .end()
                .raw_femtoseconds(),
            20
        );
    }

    #[test]
    fn projected_dataset_can_group_rows_by_arena_name() {
        let mut timeline = ConstructedTimeline::new();
        let mut bootstrap_epoch = WritableArena::new("teamy.startup.bootstrap");
        bootstrap_epoch.push(PublishedEvent::new(
            &FIRST_EVENT_DEFINITION,
            TestPayload { value: 1 },
        ));
        let mut startup_epoch = WritableArena::new("teamy.startup");
        startup_epoch.push(PublishedEvent::new(
            &SECOND_EVENT_DEFINITION,
            TestPayload { value: 2 },
        ));
        timeline.ingest(
            CanonicalTimeKey::from_femtoseconds(0),
            bootstrap_epoch.seal(),
        );
        timeline.ingest(CanonicalTimeKey::from_femtoseconds(1), startup_epoch.seal());

        let dataset = timeline.projected_timeline_dataset();
        let plan = dataset.render_plan(
            &TimelineViewportQuery::try_new(
                CanonicalTimeKey::from_femtoseconds(0),
                CanonicalTimeKey::from_femtoseconds(1),
                CanonicalTimeKey::from_femtoseconds(1),
                100,
            )
            .expect("query should build")
            .with_grouping_mode(TimelineGroupingMode::SourceKey),
        );

        assert_eq!(plan.rows().len(), 2);
    }
}

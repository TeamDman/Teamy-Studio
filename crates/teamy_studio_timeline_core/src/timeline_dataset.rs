use std::collections::HashMap;

use eyre::{Result, eyre};
use facet::Facet;

use crate::{CanonicalTimeKey, CanonicalTimeRange};

#[derive(Clone, Copy, Debug, Eq, Facet, Hash, Ord, PartialEq, PartialOrd)]
pub struct TimelineItemId(u64);

impl TimelineItemId {
    #[must_use]
    pub const fn as_u64(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, Facet, Hash, Ord, PartialEq, PartialOrd)]
pub struct TimelineItemSequence(u64);

impl TimelineItemSequence {
    #[must_use]
    pub const fn as_u64(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, Facet, Hash, Ord, PartialEq, PartialOrd)]
pub struct TimelineDatasetRevision(u64);

impl TimelineDatasetRevision {
    #[must_use]
    pub const fn as_u64(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, Facet, Hash, Ord, PartialEq, PartialOrd)]
pub struct TimelineInternedStringId(u32);

impl TimelineInternedStringId {
    #[must_use]
    pub const fn as_u32(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Debug, Facet, PartialEq)]
#[repr(C)]
pub enum TimelineFieldValue {
    Bool(bool),
    I64(i64),
    U64(u64),
    F64(f64),
    String(TimelineInternedStringId),
}

#[derive(Clone, Debug, Facet, PartialEq)]
pub struct TimelineField {
    name: TimelineInternedStringId,
    value: TimelineFieldValue,
}

impl TimelineField {
    #[must_use]
    pub const fn new(name: TimelineInternedStringId, value: TimelineFieldValue) -> Self {
        Self { name, value }
    }

    #[must_use]
    pub const fn name(&self) -> TimelineInternedStringId {
        self.name
    }

    #[must_use]
    pub const fn value(&self) -> &TimelineFieldValue {
        &self.value
    }
}

#[derive(Clone, Copy, Debug, Eq, Facet, Hash, PartialEq)]
pub struct TimelineObjectRef {
    object_id: u64,
    type_key: TimelineInternedStringId,
}

impl TimelineObjectRef {
    #[must_use]
    pub const fn new(object_id: u64, type_key: TimelineInternedStringId) -> Self {
        Self {
            object_id,
            type_key,
        }
    }

    #[must_use]
    pub const fn object_id(self) -> u64 {
        self.object_id
    }

    #[must_use]
    pub const fn type_key(self) -> TimelineInternedStringId {
        self.type_key
    }
}

#[derive(Clone, Copy, Debug, Eq, Facet, PartialEq)]
pub struct TimelineSpanItem {
    start: CanonicalTimeKey,
    end: Option<CanonicalTimeKey>,
}

impl TimelineSpanItem {
    #[must_use]
    pub const fn start(self) -> CanonicalTimeKey {
        self.start
    }

    #[must_use]
    pub const fn end(self) -> Option<CanonicalTimeKey> {
        self.end
    }

    #[must_use]
    pub const fn is_open(self) -> bool {
        self.end.is_none()
    }
}

#[derive(Clone, Copy, Debug, Eq, Facet, PartialEq)]
pub struct TimelineEventItem {
    at: CanonicalTimeKey,
}

impl TimelineEventItem {
    #[must_use]
    pub const fn at(self) -> CanonicalTimeKey {
        self.at
    }
}

#[derive(Clone, Debug, Facet, PartialEq)]
#[repr(C)]
pub enum TimelineItemKind {
    Span(TimelineSpanItem),
    Event(TimelineEventItem),
}

#[derive(Clone, Debug, Facet, PartialEq)]
pub struct TimelineItem {
    id: TimelineItemId,
    sequence: TimelineItemSequence,
    label: TimelineInternedStringId,
    source_key: TimelineInternedStringId,
    group_key: TimelineInternedStringId,
    fields: Vec<TimelineField>,
    object_refs: Vec<TimelineObjectRef>,
    kind: TimelineItemKind,
}

impl TimelineItem {
    #[must_use]
    pub const fn id(&self) -> TimelineItemId {
        self.id
    }

    #[must_use]
    pub const fn sequence(&self) -> TimelineItemSequence {
        self.sequence
    }

    #[must_use]
    pub const fn label(&self) -> TimelineInternedStringId {
        self.label
    }

    #[must_use]
    pub const fn source_key(&self) -> TimelineInternedStringId {
        self.source_key
    }

    #[must_use]
    pub const fn group_key(&self) -> TimelineInternedStringId {
        self.group_key
    }

    #[must_use]
    pub fn fields(&self) -> &[TimelineField] {
        &self.fields
    }

    #[must_use]
    pub fn object_refs(&self) -> &[TimelineObjectRef] {
        &self.object_refs
    }

    #[must_use]
    pub const fn kind(&self) -> &TimelineItemKind {
        &self.kind
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TimelineItemInput {
    label: String,
    source_key: String,
    group_key: String,
    fields: Vec<TimelineFieldInput>,
    object_refs: Vec<TimelineObjectRefInput>,
}

impl TimelineItemInput {
    #[must_use]
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            source_key: String::new(),
            group_key: String::new(),
            fields: Vec::new(),
            object_refs: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_source_key(mut self, source_key: impl Into<String>) -> Self {
        self.source_key = source_key.into();
        self
    }

    #[must_use]
    pub fn with_group_key(mut self, group_key: impl Into<String>) -> Self {
        self.group_key = group_key.into();
        self
    }

    #[must_use]
    pub fn with_field(mut self, name: impl Into<String>, value: TimelineFieldInputValue) -> Self {
        self.fields.push(TimelineFieldInput {
            name: name.into(),
            value,
        });
        self
    }

    #[must_use]
    pub fn with_object_ref(mut self, object_id: u64, type_key: impl Into<String>) -> Self {
        self.object_refs.push(TimelineObjectRefInput {
            object_id,
            type_key: type_key.into(),
        });
        self
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TimelineFieldInput {
    name: String,
    value: TimelineFieldInputValue,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TimelineFieldInputValue {
    Bool(bool),
    I64(i64),
    U64(u64),
    F64(f64),
    String(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimelineObjectRefInput {
    object_id: u64,
    type_key: String,
}

#[derive(Clone, Debug, Facet, PartialEq)]
pub struct TimelineCompactionReport {
    pending_writes_before: usize,
    item_count: usize,
    span_count: usize,
    event_count: usize,
    dataset_revision: TimelineDatasetRevision,
    index_revision: TimelineDatasetRevision,
}

impl TimelineCompactionReport {
    #[must_use]
    pub const fn pending_writes_before(&self) -> usize {
        self.pending_writes_before
    }

    #[must_use]
    pub const fn item_count(&self) -> usize {
        self.item_count
    }

    #[must_use]
    pub const fn span_count(&self) -> usize {
        self.span_count
    }

    #[must_use]
    pub const fn event_count(&self) -> usize {
        self.event_count
    }

    #[must_use]
    pub const fn dataset_revision(&self) -> TimelineDatasetRevision {
        self.dataset_revision
    }

    #[must_use]
    pub const fn index_revision(&self) -> TimelineDatasetRevision {
        self.index_revision
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TimelineWriteLogEntry {
    ItemInserted(TimelineItemId),
    SpanFinished(TimelineItemId),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct TimelineDatasetIndex {
    spans_by_start: Vec<TimelineItemId>,
    events_by_time: Vec<TimelineItemId>,
    revision: TimelineDatasetRevision,
}

#[derive(Clone, Debug, Default)]
pub struct TimelineDataset {
    next_item_id: u64,
    next_sequence: u64,
    revision: TimelineDatasetRevision,
    strings: Vec<String>,
    string_ids: HashMap<String, TimelineInternedStringId>,
    items: Vec<TimelineItem>,
    write_log: Vec<TimelineWriteLogEntry>,
    index: TimelineDatasetIndex,
}

impl TimelineDataset {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub const fn revision(&self) -> TimelineDatasetRevision {
        self.revision
    }

    #[must_use]
    pub const fn index_revision(&self) -> TimelineDatasetRevision {
        self.index.revision
    }

    #[must_use]
    pub fn items(&self) -> &[TimelineItem] {
        &self.items
    }

    #[must_use]
    pub fn write_log(&self) -> &[TimelineWriteLogEntry] {
        &self.write_log
    }

    #[must_use]
    pub const fn pending_write_count(&self) -> usize {
        self.write_log.len()
    }

    #[must_use]
    pub fn span_index(&self) -> &[TimelineItemId] {
        &self.index.spans_by_start
    }

    #[must_use]
    pub fn event_index(&self) -> &[TimelineItemId] {
        &self.index.events_by_time
    }

    #[must_use]
    pub fn resolve_string(&self, id: TimelineInternedStringId) -> Option<&str> {
        self.strings.get(id.as_u32() as usize).map(String::as_str)
    }

    #[must_use]
    pub fn time_bounds(&self) -> Option<CanonicalTimeRange> {
        let mut start: Option<CanonicalTimeKey> = None;
        let mut end: Option<CanonicalTimeKey> = None;

        for item in &self.items {
            match item.kind() {
                TimelineItemKind::Span(span) => {
                    let item_start = span.start();
                    let item_end = span.end().unwrap_or(item_start).max(item_start);
                    start = Some(start.map_or(item_start, |current| current.min(item_start)));
                    end = Some(end.map_or(item_end, |current| current.max(item_end)));
                }
                TimelineItemKind::Event(event) => {
                    let at = event.at();
                    start = Some(start.map_or(at, |current| current.min(at)));
                    end = Some(end.map_or(at, |current| current.max(at)));
                }
            }
        }

        match (start, end) {
            (Some(start), Some(end)) => Some(
                CanonicalTimeRange::try_new(start, end)
                    .expect("dataset bounds should remain ordered"),
            ),
            _ => None,
        }
    }

    #[must_use]
    pub fn intern_string(&mut self, value: impl AsRef<str>) -> TimelineInternedStringId {
        let value = value.as_ref();
        if let Some(id) = self.string_ids.get(value) {
            return *id;
        }

        let next_id = TimelineInternedStringId(
            u32::try_from(self.strings.len()).expect("timeline string table exceeded u32::MAX"),
        );
        self.strings.push(value.to_owned());
        self.string_ids.insert(value.to_owned(), next_id);
        next_id
    }

    /// # Errors
    ///
    /// Returns an error if a closed span has an end before its start.
    pub fn push_span(
        &mut self,
        input: TimelineItemInput,
        start: CanonicalTimeKey,
        end: Option<CanonicalTimeKey>,
    ) -> Result<TimelineItemId> {
        if let Some(end) = end {
            let _ = CanonicalTimeRange::try_new(start, end)?;
        }

        let id = self.allocate_item_id();
        let item = self.build_item(
            id,
            input,
            TimelineItemKind::Span(TimelineSpanItem { start, end }),
        );
        self.items.push(item);
        self.record_write(TimelineWriteLogEntry::ItemInserted(id));
        Ok(id)
    }

    pub fn push_event(&mut self, input: TimelineItemInput, at: CanonicalTimeKey) -> TimelineItemId {
        let id = self.allocate_item_id();
        let item = self.build_item(id, input, TimelineItemKind::Event(TimelineEventItem { at }));
        self.items.push(item);
        self.record_write(TimelineWriteLogEntry::ItemInserted(id));
        id
    }

    /// # Errors
    ///
    /// Returns an error when the item does not exist, is not a span, is already closed,
    /// or `end` is earlier than the span start.
    pub fn finish_span(&mut self, id: TimelineItemId, end: CanonicalTimeKey) -> Result<()> {
        let Some(item) = self.items.iter_mut().find(|item| item.id == id) else {
            return Err(eyre!("timeline item {} does not exist", id.as_u64()));
        };

        let TimelineItemKind::Span(span) = &mut item.kind else {
            return Err(eyre!("timeline item {} is not a span", id.as_u64()));
        };

        if span.end.is_some() {
            return Err(eyre!("timeline span {} is already finished", id.as_u64()));
        }

        let _ = CanonicalTimeRange::try_new(span.start, end)?;
        span.end = Some(end);
        self.record_write(TimelineWriteLogEntry::SpanFinished(id));
        Ok(())
    }

    #[must_use]
    pub fn item(&self, id: TimelineItemId) -> Option<&TimelineItem> {
        self.items.iter().find(|item| item.id == id)
    }

    pub fn compact(&mut self) -> TimelineCompactionReport {
        let pending_writes_before = self.write_log.len();
        if pending_writes_before > 0 {
            self.rebuild_index_inner();
            self.write_log.clear();
        }
        self.compaction_report(pending_writes_before)
    }

    pub fn rebuild_index(&mut self) -> TimelineCompactionReport {
        let pending_writes_before = self.write_log.len();
        self.rebuild_index_inner();
        self.write_log.clear();
        self.compaction_report(pending_writes_before)
    }

    fn allocate_item_id(&mut self) -> TimelineItemId {
        self.next_item_id = self.next_item_id.saturating_add(1);
        TimelineItemId(self.next_item_id)
    }

    fn allocate_sequence(&mut self) -> TimelineItemSequence {
        self.next_sequence = self.next_sequence.saturating_add(1);
        TimelineItemSequence(self.next_sequence)
    }

    fn build_item(
        &mut self,
        id: TimelineItemId,
        input: TimelineItemInput,
        kind: TimelineItemKind,
    ) -> TimelineItem {
        let label = self.intern_string(input.label);
        let source_key = self.intern_string(input.source_key);
        let group_key = self.intern_string(input.group_key);
        let fields = input
            .fields
            .into_iter()
            .map(|field| {
                let name = self.intern_string(field.name);
                let value = match field.value {
                    TimelineFieldInputValue::Bool(value) => TimelineFieldValue::Bool(value),
                    TimelineFieldInputValue::I64(value) => TimelineFieldValue::I64(value),
                    TimelineFieldInputValue::U64(value) => TimelineFieldValue::U64(value),
                    TimelineFieldInputValue::F64(value) => TimelineFieldValue::F64(value),
                    TimelineFieldInputValue::String(value) => {
                        TimelineFieldValue::String(self.intern_string(value))
                    }
                };
                TimelineField::new(name, value)
            })
            .collect();
        let object_refs = input
            .object_refs
            .into_iter()
            .map(|object_ref| {
                TimelineObjectRef::new(
                    object_ref.object_id,
                    self.intern_string(object_ref.type_key),
                )
            })
            .collect();

        TimelineItem {
            id,
            sequence: self.allocate_sequence(),
            label,
            source_key,
            group_key,
            fields,
            object_refs,
            kind,
        }
    }

    fn record_write(&mut self, entry: TimelineWriteLogEntry) {
        self.revision = TimelineDatasetRevision(self.revision.as_u64().saturating_add(1));
        self.write_log.push(entry);
    }

    fn rebuild_index_inner(&mut self) {
        let mut spans_by_start = Vec::new();
        let mut events_by_time = Vec::new();
        for item in &self.items {
            match item.kind {
                TimelineItemKind::Span(_) => spans_by_start.push(item.id),
                TimelineItemKind::Event(_) => events_by_time.push(item.id),
            }
        }
        spans_by_start.sort_by_key(|id| {
            let item = self.item(*id).expect("span index item must exist");
            let TimelineItemKind::Span(span) = item.kind else {
                unreachable!("span index contains only span items");
            };
            (span.start, item.sequence)
        });
        events_by_time.sort_by_key(|id| {
            let item = self.item(*id).expect("event index item must exist");
            let TimelineItemKind::Event(event) = item.kind else {
                unreachable!("event index contains only event items");
            };
            (event.at, item.sequence)
        });
        self.index = TimelineDatasetIndex {
            spans_by_start,
            events_by_time,
            revision: self.revision,
        };
    }

    fn compaction_report(&self, pending_writes_before: usize) -> TimelineCompactionReport {
        let span_count = self
            .items
            .iter()
            .filter(|item| matches!(item.kind, TimelineItemKind::Span(_)))
            .count();
        TimelineCompactionReport {
            pending_writes_before,
            item_count: self.items.len(),
            span_count,
            event_count: self.items.len().saturating_sub(span_count),
            dataset_revision: self.revision,
            index_revision: self.index.revision,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{TimelineDataset, TimelineFieldInputValue, TimelineItemInput};
    use crate::CanonicalTimeKey;

    #[test]
    fn compact_rebuilds_event_and_span_indexes() {
        let mut dataset = TimelineDataset::new();
        let span_id = dataset
            .push_span(
                TimelineItemInput::new("download")
                    .with_group_key("jobs")
                    .with_field("bytes", TimelineFieldInputValue::U64(42)),
                CanonicalTimeKey::from_femtoseconds(10),
                Some(CanonicalTimeKey::from_femtoseconds(30)),
            )
            .expect("span should insert");
        let event_id = dataset.push_event(
            TimelineItemInput::new("log").with_group_key("logs"),
            CanonicalTimeKey::from_femtoseconds(20),
        );

        let report = dataset.compact();

        assert_eq!(report.pending_writes_before(), 2);
        assert_eq!(dataset.span_index(), &[span_id]);
        assert_eq!(dataset.event_index(), &[event_id]);
        assert_eq!(dataset.pending_write_count(), 0);
    }

    #[test]
    fn time_bounds_cover_inserted_items() {
        let mut dataset = TimelineDataset::new();
        dataset.push_event(
            TimelineItemInput::new("first").with_group_key("events"),
            CanonicalTimeKey::from_femtoseconds(5),
        );
        dataset
            .push_span(
                TimelineItemInput::new("later").with_group_key("spans"),
                CanonicalTimeKey::from_femtoseconds(20),
                Some(CanonicalTimeKey::from_femtoseconds(30)),
            )
            .expect("span should insert");

        let bounds = dataset.time_bounds().expect("dataset should have bounds");

        assert_eq!(bounds.start().raw_femtoseconds(), 5);
        assert_eq!(bounds.end().raw_femtoseconds(), 30);
    }

    #[test]
    fn finish_span_validates_span_state_and_order() {
        let mut dataset = TimelineDataset::new();
        let span_id = dataset
            .push_span(
                TimelineItemInput::new("running").with_group_key("jobs"),
                CanonicalTimeKey::from_femtoseconds(10),
                None,
            )
            .expect("span should insert");

        dataset
            .finish_span(span_id, CanonicalTimeKey::from_femtoseconds(15))
            .expect("span should finish");

        let error = dataset
            .finish_span(span_id, CanonicalTimeKey::from_femtoseconds(20))
            .expect_err("closed span should reject a second finish");

        assert_eq!(error.to_string(), "timeline span 1 is already finished");
    }
}

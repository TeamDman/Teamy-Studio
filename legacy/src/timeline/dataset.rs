use std::collections::HashMap;

use arbitrary::Arbitrary;
use facet::Facet;

use super::time::{TimelineInstantNs, TimelineRangeNs};

#[derive(Arbitrary, Facet, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TimelineItemId(u64);

impl TimelineItemId {
    #[must_use]
    pub const fn as_u64(self) -> u64 {
        self.0
    }
}

#[derive(Arbitrary, Facet, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TimelineItemSequence(u64);

impl TimelineItemSequence {
    #[must_use]
    pub const fn as_u64(self) -> u64 {
        self.0
    }
}

#[derive(Arbitrary, Facet, Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TimelineDatasetRevision(u64);

impl TimelineDatasetRevision {
    #[must_use]
    pub const fn as_u64(self) -> u64 {
        self.0
    }
}

#[derive(Arbitrary, Facet, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TimelineInternedStringId(u32);

impl TimelineInternedStringId {
    #[must_use]
    pub const fn as_u32(self) -> u32 {
        self.0
    }
}

#[derive(Arbitrary, Facet, Clone, Debug, PartialEq)]
#[repr(C)]
pub enum TimelineFieldValue {
    Bool(bool),
    I64(i64),
    U64(u64),
    F64(f64),
    String(TimelineInternedStringId),
}

#[derive(Arbitrary, Facet, Clone, Debug, PartialEq)]
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

#[derive(Arbitrary, Facet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
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

#[derive(Arbitrary, Facet, Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimelineSpanItem {
    start: TimelineInstantNs,
    end: Option<TimelineInstantNs>,
}

impl TimelineSpanItem {
    #[must_use]
    pub const fn start(self) -> TimelineInstantNs {
        self.start
    }

    #[must_use]
    pub const fn end(self) -> Option<TimelineInstantNs> {
        self.end
    }

    #[must_use]
    pub const fn is_open(self) -> bool {
        self.end.is_none()
    }
}

#[derive(Arbitrary, Facet, Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimelineEventItem {
    at: TimelineInstantNs,
}

impl TimelineEventItem {
    #[must_use]
    pub const fn at(self) -> TimelineInstantNs {
        self.at
    }
}

#[derive(Arbitrary, Facet, Clone, Debug, PartialEq)]
#[repr(C)]
pub enum TimelineItemKind {
    Span(TimelineSpanItem),
    Event(TimelineEventItem),
}

#[derive(Arbitrary, Facet, Clone, Debug, PartialEq)]
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

#[derive(Facet, Clone, Debug, PartialEq)]
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
// timeline[impl display.dataset-owned-ids]
// timeline[impl display.dataset-index-compaction]
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
    /// # Panics
    ///
    /// Panics if the dataset interns more than `u32::MAX` distinct strings.
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
    // timeline[impl display.dataset-checked-mutation]
    // timeline[impl display.object-refs]
    pub fn push_span(
        &mut self,
        input: TimelineItemInput,
        start: TimelineInstantNs,
        end: Option<TimelineInstantNs>,
    ) -> eyre::Result<TimelineItemId> {
        if let Some(end) = end {
            TimelineRangeNs::try_new(start, end)?;
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

    pub fn push_event(
        &mut self,
        input: TimelineItemInput,
        at: TimelineInstantNs,
    ) -> TimelineItemId {
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
    // timeline[impl display.dataset-checked-mutation]
    pub fn finish_span(&mut self, id: TimelineItemId, end: TimelineInstantNs) -> eyre::Result<()> {
        let Some(item) = self.items.iter_mut().find(|item| item.id == id) else {
            eyre::bail!("timeline item {} does not exist", id.as_u64());
        };

        let TimelineItemKind::Span(span) = &mut item.kind else {
            eyre::bail!("timeline item {} is not a span", id.as_u64());
        };

        if span.end.is_some() {
            eyre::bail!("timeline span {} is already finished", id.as_u64());
        }

        TimelineRangeNs::try_new(span.start, end)?;
        span.end = Some(end);
        self.record_write(TimelineWriteLogEntry::SpanFinished(id));
        Ok(())
    }

    #[must_use]
    pub fn item(&self, id: TimelineItemId) -> Option<&TimelineItem> {
        self.items.iter().find(|item| item.id == id)
    }

    // timeline[impl display.dataset-index-compaction]
    pub fn compact(&mut self) -> TimelineCompactionReport {
        let pending_writes_before = self.write_log.len();
        if pending_writes_before > 0 {
            self.rebuild_index_inner();
            self.write_log.clear();
        }
        self.compaction_report(pending_writes_before)
    }

    // timeline[impl display.dataset-index-compaction]
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

impl<'a> Arbitrary<'a> for TimelineDataset {
    fn arbitrary(u: &mut arbitrary::Unstructured<'a>) -> arbitrary::Result<Self> {
        let mut dataset = Self::new();
        let item_count = u.int_in_range(0_usize..=16)?;
        for item_index in 0..item_count {
            let input = arbitrary_item_input(u, item_index)?;
            if bool::arbitrary(u)? {
                let range = TimelineRangeNs::arbitrary(u)?;
                let end = bool::arbitrary(u)?.then(|| range.end());
                dataset
                    .push_span(input, range.start(), end)
                    .expect("arbitrary range is ordered");
            } else {
                let at = TimelineInstantNs::arbitrary(u)?;
                dataset.push_event(input, at);
            }
        }
        if bool::arbitrary(u)? {
            dataset.compact();
        }
        Ok(dataset)
    }
}

fn arbitrary_item_input(
    u: &mut arbitrary::Unstructured<'_>,
    item_index: usize,
) -> arbitrary::Result<TimelineItemInput> {
    let mut input = TimelineItemInput::new(format!("item-{item_index}-{}", bounded_u8(u)?))
        .with_source_key(format!("source-{}", u.int_in_range(0_u8..=3)?))
        .with_group_key(format!("group-{}", u.int_in_range(0_u8..=5)?));
    let field_count = u.int_in_range(0_usize..=4)?;
    for field_index in 0..field_count {
        let field_value = match u.int_in_range(0_u8..=4)? {
            0 => TimelineFieldInputValue::Bool(bool::arbitrary(u)?),
            1 => TimelineFieldInputValue::I64(i64::arbitrary(u)?),
            2 => TimelineFieldInputValue::U64(u64::arbitrary(u)?),
            3 => TimelineFieldInputValue::F64(f64::arbitrary(u)?),
            _ => TimelineFieldInputValue::String(format!("value-{}", bounded_u8(u)?)),
        };
        input = input.with_field(format!("field-{field_index}"), field_value);
    }
    let object_ref_count = u.int_in_range(0_usize..=2)?;
    for _ in 0..object_ref_count {
        input = input.with_object_ref(u64::arbitrary(u)?, format!("type-{}", bounded_u8(u)?));
    }
    Ok(input)
}

fn bounded_u8(u: &mut arbitrary::Unstructured<'_>) -> arbitrary::Result<u8> {
    u.int_in_range(0_u8..=32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    // timeline[verify display.dataset-owned-ids]
    fn dataset_assigns_internal_ids_and_sequences() {
        let mut dataset = TimelineDataset::new();

        let first_id = dataset
            .push_span(
                TimelineItemInput::new("decode").with_group_key("jobs"),
                TimelineInstantNs::new(10),
                None,
            )
            .expect("span");
        let second_id = dataset.push_event(
            TimelineItemInput::new("message").with_source_key("logs"),
            TimelineInstantNs::new(11),
        );

        assert_eq!(first_id.as_u64(), 1);
        assert_eq!(second_id.as_u64(), 2);
        assert_eq!(
            dataset.item(first_id).expect("first").sequence().as_u64(),
            1
        );
        assert_eq!(
            dataset.item(second_id).expect("second").sequence().as_u64(),
            2
        );
        assert_eq!(dataset.pending_write_count(), 2);
        assert_eq!(dataset.revision().as_u64(), 2);
    }

    #[test]
    // timeline[verify display.object-refs]
    fn dataset_interns_repeated_metadata() {
        let mut dataset = TimelineDataset::new();

        let first_id = dataset.push_event(
            TimelineItemInput::new("same")
                .with_source_key("source")
                .with_group_key("group")
                .with_field(
                    "object.type_key",
                    TimelineFieldInputValue::String("clip".into()),
                )
                .with_object_ref(7, "clip"),
            TimelineInstantNs::new(1),
        );
        let second_id = dataset.push_event(
            TimelineItemInput::new("same")
                .with_source_key("source")
                .with_group_key("group")
                .with_field(
                    "object.type_key",
                    TimelineFieldInputValue::String("clip".into()),
                )
                .with_object_ref(8, "clip"),
            TimelineInstantNs::new(2),
        );

        let first = dataset.item(first_id).expect("first");
        let second = dataset.item(second_id).expect("second");
        assert_eq!(first.label(), second.label());
        assert_eq!(first.source_key(), second.source_key());
        assert_eq!(first.group_key(), second.group_key());
        assert_eq!(dataset.resolve_string(first.label()), Some("same"));
        assert_eq!(
            dataset.resolve_string(first.object_refs()[0].type_key()),
            Some("clip")
        );
    }

    #[test]
    // timeline[verify display.dataset-checked-mutation]
    fn finish_span_closes_open_span_and_rejects_invalid_finishes() {
        let mut dataset = TimelineDataset::new();
        let span_id = dataset
            .push_span(
                TimelineItemInput::new("open"),
                TimelineInstantNs::new(20),
                None,
            )
            .expect("span");
        let event_id =
            dataset.push_event(TimelineItemInput::new("event"), TimelineInstantNs::new(21));

        assert!(
            dataset
                .finish_span(span_id, TimelineInstantNs::new(19))
                .is_err()
        );
        assert!(
            dataset
                .finish_span(event_id, TimelineInstantNs::new(30))
                .is_err()
        );
        dataset
            .finish_span(span_id, TimelineInstantNs::new(30))
            .expect("finish");
        assert!(
            dataset
                .finish_span(span_id, TimelineInstantNs::new(31))
                .is_err()
        );

        let TimelineItemKind::Span(span) = dataset.item(span_id).expect("span").kind() else {
            panic!("expected span");
        };
        assert_eq!(span.end(), Some(TimelineInstantNs::new(30)));
        assert_eq!(dataset.pending_write_count(), 3);
    }

    #[test]
    // timeline[verify display.dataset-checked-mutation]
    fn push_span_rejects_reversed_closed_range() {
        let mut dataset = TimelineDataset::new();

        let error = dataset
            .push_span(
                TimelineItemInput::new("bad"),
                TimelineInstantNs::new(42),
                Some(TimelineInstantNs::new(10)),
            )
            .expect_err("reversed range");

        assert!(error.to_string().contains("earlier than start"));
        assert!(dataset.items().is_empty());
        assert_eq!(dataset.revision().as_u64(), 0);
    }

    #[test]
    // timeline[verify display.dataset-index-compaction]
    fn compaction_builds_indexes_without_discarding_raw_items() {
        let mut dataset = TimelineDataset::new();
        let late_span_id = dataset
            .push_span(
                TimelineItemInput::new("late"),
                TimelineInstantNs::new(50),
                Some(TimelineInstantNs::new(60)),
            )
            .expect("late span");
        let event_id =
            dataset.push_event(TimelineItemInput::new("event"), TimelineInstantNs::new(40));
        let early_span_id = dataset
            .push_span(
                TimelineItemInput::new("early"),
                TimelineInstantNs::new(10),
                None,
            )
            .expect("early span");

        let report = dataset.compact();

        assert_eq!(report.pending_writes_before(), 3);
        assert_eq!(report.item_count(), 3);
        assert_eq!(report.span_count(), 2);
        assert_eq!(report.event_count(), 1);
        assert_eq!(dataset.items().len(), 3);
        assert_eq!(dataset.span_index(), &[early_span_id, late_span_id]);
        assert_eq!(dataset.event_index(), &[event_id]);
        assert_eq!(dataset.pending_write_count(), 0);
        assert_eq!(dataset.index_revision(), dataset.revision());
    }

    #[test]
    // timeline[verify display.dataset-index-compaction]
    fn rebuild_index_matches_compaction_after_pending_writes() {
        let mut compacted = TimelineDataset::new();
        let mut rebuilt = TimelineDataset::new();
        for dataset in [&mut compacted, &mut rebuilt] {
            dataset
                .push_span(
                    TimelineItemInput::new("span"),
                    TimelineInstantNs::new(2),
                    None,
                )
                .expect("span");
            dataset.push_event(TimelineItemInput::new("event"), TimelineInstantNs::new(1));
        }

        compacted.compact();
        rebuilt.rebuild_index();

        assert_eq!(compacted.span_index(), rebuilt.span_index());
        assert_eq!(compacted.event_index(), rebuilt.event_index());
        assert_eq!(compacted.index_revision(), rebuilt.index_revision());
        assert_eq!(rebuilt.pending_write_count(), 0);
    }

    #[test]
    // timeline[verify display.dataset-owned-ids]
    // timeline[verify display.dataset-index-compaction]
    fn arbitrary_datasets_preserve_ids_and_index_invariants() {
        for seed in 0_u8..=u8::MAX {
            let bytes = [seed; 128];
            let mut unstructured = arbitrary::Unstructured::new(&bytes);
            let Ok(mut dataset) = TimelineDataset::arbitrary(&mut unstructured) else {
                continue;
            };

            let mut ids = dataset
                .items()
                .iter()
                .map(TimelineItem::id)
                .collect::<Vec<_>>();
            ids.sort_unstable();
            ids.dedup();
            assert_eq!(ids.len(), dataset.items().len());
            for item in dataset.items() {
                if let TimelineItemKind::Span(span) = item.kind()
                    && let Some(end) = span.end()
                {
                    assert!(span.start() <= end);
                }
            }

            dataset.rebuild_index();
            assert_eq!(dataset.pending_write_count(), 0);
            assert_eq!(
                dataset.span_index().len() + dataset.event_index().len(),
                dataset.items().len()
            );
        }
    }
}

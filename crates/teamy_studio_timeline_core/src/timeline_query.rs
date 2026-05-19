use std::collections::BTreeMap;

use facet::Facet;

use crate::{CanonicalTimeKey, CanonicalTimeRange, TimelineItemId};

use super::timeline_dataset::{
    TimelineDataset, TimelineDatasetRevision, TimelineInternedStringId, TimelineItem,
    TimelineItemKind,
};

#[derive(Clone, Copy, Debug, Default, Eq, Facet, PartialEq)]
#[repr(u8)]
pub enum TimelineGroupingMode {
    #[default]
    GroupKey,
    SourceKey,
    Label,
    All,
}

#[derive(Clone, Copy, Debug, Eq, Facet, Ord, PartialEq, PartialOrd, Hash)]
pub struct TimelineRenderRowId(u32);

impl TimelineRenderRowId {
    #[must_use]
    pub const fn as_u32(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, Facet, PartialEq, PartialOrd, Ord, Hash)]
#[repr(C)]
pub enum TimelineRenderRowKey {
    Interned(TimelineInternedStringId),
    All,
}

#[derive(Clone, Debug, Eq, Facet, PartialEq)]
pub struct TimelineRenderRow {
    id: TimelineRenderRowId,
    key: TimelineRenderRowKey,
}

impl TimelineRenderRow {
    #[must_use]
    pub const fn id(&self) -> TimelineRenderRowId {
        self.id
    }

    #[must_use]
    pub const fn key(&self) -> TimelineRenderRowKey {
        self.key
    }
}

#[derive(Clone, Debug, Eq, Facet, PartialEq)]
pub struct TimelineViewportQuery {
    visible_range: CanonicalTimeRange,
    now: CanonicalTimeKey,
    viewport_width_pixels: u32,
    grouping_mode: TimelineGroupingMode,
    minimum_visible_pixels: u32,
}

impl TimelineViewportQuery {
    /// # Errors
    ///
    /// Returns an error when `visible_range_end` is earlier than `visible_range_start`.
    pub fn try_new(
        visible_range_start: CanonicalTimeKey,
        visible_range_end: CanonicalTimeKey,
        now: CanonicalTimeKey,
        viewport_width_pixels: u32,
    ) -> eyre::Result<Self> {
        Ok(Self {
            visible_range: CanonicalTimeRange::try_new(visible_range_start, visible_range_end)?,
            now,
            viewport_width_pixels,
            grouping_mode: TimelineGroupingMode::default(),
            minimum_visible_pixels: 1,
        })
    }

    #[must_use]
    pub const fn visible_range(&self) -> CanonicalTimeRange {
        self.visible_range
    }

    #[must_use]
    pub const fn now(&self) -> CanonicalTimeKey {
        self.now
    }

    #[must_use]
    pub const fn viewport_width_pixels(&self) -> u32 {
        self.viewport_width_pixels
    }

    #[must_use]
    pub const fn grouping_mode(&self) -> TimelineGroupingMode {
        self.grouping_mode
    }

    #[must_use]
    pub const fn minimum_visible_pixels(&self) -> u32 {
        self.minimum_visible_pixels
    }

    #[must_use]
    pub const fn with_grouping_mode(mut self, grouping_mode: TimelineGroupingMode) -> Self {
        self.grouping_mode = grouping_mode;
        self
    }

    #[must_use]
    pub const fn with_minimum_visible_pixels(mut self, minimum_visible_pixels: u32) -> Self {
        self.minimum_visible_pixels = minimum_visible_pixels;
        self
    }
}

#[derive(Clone, Copy, Debug, Eq, Facet, PartialEq)]
pub struct TimelineRenderSpan {
    item_id: TimelineItemId,
    row_id: TimelineRenderRowId,
    lane_index: u32,
    range: CanonicalTimeRange,
    is_open: bool,
}

impl TimelineRenderSpan {
    #[must_use]
    pub const fn item_id(self) -> TimelineItemId {
        self.item_id
    }

    #[must_use]
    pub const fn row_id(self) -> TimelineRenderRowId {
        self.row_id
    }

    #[must_use]
    pub const fn lane_index(self) -> u32 {
        self.lane_index
    }

    #[must_use]
    pub const fn range(self) -> CanonicalTimeRange {
        self.range
    }

    #[must_use]
    pub const fn is_open(self) -> bool {
        self.is_open
    }
}

#[derive(Clone, Copy, Debug, Eq, Facet, PartialEq)]
pub struct TimelineRenderEvent {
    item_id: TimelineItemId,
    row_id: TimelineRenderRowId,
    at: CanonicalTimeKey,
}

impl TimelineRenderEvent {
    #[must_use]
    pub const fn item_id(self) -> TimelineItemId {
        self.item_id
    }

    #[must_use]
    pub const fn row_id(self) -> TimelineRenderRowId {
        self.row_id
    }

    #[must_use]
    pub const fn at(self) -> CanonicalTimeKey {
        self.at
    }
}

#[derive(Clone, Copy, Debug, Eq, Facet, PartialEq)]
pub struct TimelineRenderCluster {
    row_id: TimelineRenderRowId,
    range: CanonicalTimeRange,
    count: usize,
    representative_item_id: TimelineItemId,
}

impl TimelineRenderCluster {
    #[must_use]
    pub const fn row_id(self) -> TimelineRenderRowId {
        self.row_id
    }

    #[must_use]
    pub const fn range(self) -> CanonicalTimeRange {
        self.range
    }

    #[must_use]
    pub const fn count(self) -> usize {
        self.count
    }

    #[must_use]
    pub const fn representative_item_id(self) -> TimelineItemId {
        self.representative_item_id
    }
}

#[derive(Clone, Copy, Debug, Eq, Facet, PartialEq)]
#[repr(C)]
pub enum TimelineRenderItem {
    Span(TimelineRenderSpan),
    Event(TimelineRenderEvent),
    FoldedSpanCluster(TimelineRenderCluster),
    FoldedEventCluster(TimelineRenderCluster),
}

#[derive(Clone, Debug, Eq, Facet, PartialEq)]
pub struct TimelineRenderPlan {
    dataset_revision: TimelineDatasetRevision,
    index_revision: TimelineDatasetRevision,
    pending_write_count: usize,
    rows: Vec<TimelineRenderRow>,
    items: Vec<TimelineRenderItem>,
}

impl TimelineRenderPlan {
    #[must_use]
    pub const fn dataset_revision(&self) -> TimelineDatasetRevision {
        self.dataset_revision
    }

    #[must_use]
    pub const fn index_revision(&self) -> TimelineDatasetRevision {
        self.index_revision
    }

    #[must_use]
    pub const fn pending_write_count(&self) -> usize {
        self.pending_write_count
    }

    #[must_use]
    pub fn rows(&self) -> &[TimelineRenderRow] {
        &self.rows
    }

    #[must_use]
    pub fn items(&self) -> &[TimelineRenderItem] {
        &self.items
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TimelineCandidateKind {
    Span {
        range: CanonicalTimeRange,
        is_open: bool,
    },
    Event {
        at: CanonicalTimeKey,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TimelineRenderCandidate {
    item_id: TimelineItemId,
    row_key: TimelineRenderRowKey,
    kind: TimelineCandidateKind,
}

impl TimelineDataset {
    #[must_use]
    pub fn render_plan(&self, query: &TimelineViewportQuery) -> TimelineRenderPlan {
        let mut candidates = Vec::new();
        self.collect_visible_spans(query, &mut candidates);
        self.collect_visible_events(query, &mut candidates);

        let rows = build_rows(&candidates);
        let row_ids = rows
            .iter()
            .map(|row| (row.key, row.id))
            .collect::<BTreeMap<_, _>>();
        let items = build_render_items(query, candidates, &row_ids);

        TimelineRenderPlan {
            dataset_revision: self.revision(),
            index_revision: self.index_revision(),
            pending_write_count: self.pending_write_count(),
            rows,
            items,
        }
    }

    fn collect_visible_spans(
        &self,
        query: &TimelineViewportQuery,
        candidates: &mut Vec<TimelineRenderCandidate>,
    ) {
        for item_id in self.span_index() {
            let Some(item) = self.item(*item_id) else {
                continue;
            };
            let TimelineItemKind::Span(span) = item.kind() else {
                continue;
            };
            let end = span.end().unwrap_or(query.now()).max(span.start());
            let Ok(range) = CanonicalTimeRange::try_new(span.start(), end) else {
                continue;
            };
            if ranges_intersect(range, query.visible_range()) {
                candidates.push(TimelineRenderCandidate {
                    item_id: *item_id,
                    row_key: row_key_for_item(item, query.grouping_mode()),
                    kind: TimelineCandidateKind::Span {
                        range,
                        is_open: span.is_open(),
                    },
                });
            }
        }
    }

    fn collect_visible_events(
        &self,
        query: &TimelineViewportQuery,
        candidates: &mut Vec<TimelineRenderCandidate>,
    ) {
        for item_id in self.event_index() {
            let Some(item) = self.item(*item_id) else {
                continue;
            };
            let TimelineItemKind::Event(event) = item.kind() else {
                continue;
            };
            if instant_in_range(event.at(), query.visible_range()) {
                candidates.push(TimelineRenderCandidate {
                    item_id: *item_id,
                    row_key: row_key_for_item(item, query.grouping_mode()),
                    kind: TimelineCandidateKind::Event { at: event.at() },
                });
            }
        }
    }
}

fn build_rows(candidates: &[TimelineRenderCandidate]) -> Vec<TimelineRenderRow> {
    let mut row_keys = candidates
        .iter()
        .map(|candidate| candidate.row_key)
        .collect::<Vec<_>>();
    row_keys.sort_unstable();
    row_keys.dedup();
    row_keys
        .into_iter()
        .enumerate()
        .map(|(index, key)| TimelineRenderRow {
            id: TimelineRenderRowId(u32::try_from(index).unwrap_or(u32::MAX)),
            key,
        })
        .collect()
}

fn build_render_items(
    query: &TimelineViewportQuery,
    mut candidates: Vec<TimelineRenderCandidate>,
    row_ids: &BTreeMap<TimelineRenderRowKey, TimelineRenderRowId>,
) -> Vec<TimelineRenderItem> {
    candidates.sort_by_key(candidate_sort_key);
    let mut render_items = Vec::new();
    let mut folded_spans: Vec<(TimelineItemId, TimelineRenderRowId, CanonicalTimeRange)> =
        Vec::new();
    let mut folded_events = Vec::new();
    let mut span_lanes = BTreeMap::new();

    for candidate in candidates {
        let row_id = row_ids[&candidate.row_key];
        match candidate.kind {
            TimelineCandidateKind::Span { range, is_open } => {
                if projected_width_pixels(range, query) < f64::from(query.minimum_visible_pixels())
                {
                    if folded_spans.last().is_some_and(|(_, row, previous_range)| {
                        *row != row_id
                            || projected_instant_distance_pixels(
                                previous_range.end(),
                                range.start(),
                                query,
                            ) >= f64::from(query.minimum_visible_pixels())
                    }) {
                        flush_span_cluster(&mut folded_spans, &mut render_items);
                    }
                    folded_spans.push((candidate.item_id, row_id, range));
                } else {
                    flush_span_cluster(&mut folded_spans, &mut render_items);
                    flush_event_cluster(&mut folded_events, &mut render_items);
                    let lane_index = span_lane_index(row_id, range, &mut span_lanes);
                    render_items.push(TimelineRenderItem::Span(TimelineRenderSpan {
                        item_id: candidate.item_id,
                        row_id,
                        lane_index,
                        range,
                        is_open,
                    }));
                }
            }
            TimelineCandidateKind::Event { at } => {
                if folded_events.last().is_some_and(|(_, row, previous_at)| {
                    *row != row_id
                        || projected_instant_distance_pixels(*previous_at, at, query)
                            >= f64::from(query.minimum_visible_pixels())
                }) {
                    flush_event_cluster(&mut folded_events, &mut render_items);
                }
                folded_events.push((candidate.item_id, row_id, at));
            }
        }
    }

    flush_span_cluster(&mut folded_spans, &mut render_items);
    flush_event_cluster(&mut folded_events, &mut render_items);
    render_items
}

fn flush_span_cluster(
    folded_spans: &mut Vec<(TimelineItemId, TimelineRenderRowId, CanonicalTimeRange)>,
    render_items: &mut Vec<TimelineRenderItem>,
) {
    if folded_spans.is_empty() {
        return;
    }
    if folded_spans.len() == 1 {
        let (item_id, row_id, range) = folded_spans.remove(0);
        render_items.push(TimelineRenderItem::Span(TimelineRenderSpan {
            item_id,
            row_id,
            lane_index: 0,
            range,
            is_open: false,
        }));
        return;
    }

    let row_id = folded_spans[0].1;
    let start = folded_spans
        .iter()
        .map(|(_, _, range)| range.start())
        .min()
        .expect("folded span cluster should have a start");
    let end = folded_spans
        .iter()
        .map(|(_, _, range)| range.end())
        .max()
        .expect("folded span cluster should have an end");
    let representative_item_id = folded_spans[0].0;
    let range = CanonicalTimeRange::try_new(start, end).expect("cluster range should be ordered");
    render_items.push(TimelineRenderItem::FoldedSpanCluster(
        TimelineRenderCluster {
            row_id,
            range,
            count: folded_spans.len(),
            representative_item_id,
        },
    ));
    folded_spans.clear();
}

fn flush_event_cluster(
    folded_events: &mut Vec<(TimelineItemId, TimelineRenderRowId, CanonicalTimeKey)>,
    render_items: &mut Vec<TimelineRenderItem>,
) {
    if folded_events.is_empty() {
        return;
    }
    if folded_events.len() == 1 {
        let (item_id, row_id, at) = folded_events.remove(0);
        render_items.push(TimelineRenderItem::Event(TimelineRenderEvent {
            item_id,
            row_id,
            at,
        }));
        return;
    }

    let row_id = folded_events[0].1;
    let start = folded_events
        .iter()
        .map(|(_, _, at)| *at)
        .min()
        .expect("folded event cluster should have a start");
    let end = folded_events
        .iter()
        .map(|(_, _, at)| *at)
        .max()
        .expect("folded event cluster should have an end");
    let representative_item_id = folded_events[0].0;
    let range = CanonicalTimeRange::try_new(start, end).expect("cluster range should be ordered");
    render_items.push(TimelineRenderItem::FoldedEventCluster(
        TimelineRenderCluster {
            row_id,
            range,
            count: folded_events.len(),
            representative_item_id,
        },
    ));
    folded_events.clear();
}

fn candidate_sort_key(
    candidate: &TimelineRenderCandidate,
) -> (TimelineRenderRowKey, CanonicalTimeKey, TimelineItemId) {
    let at = match candidate.kind {
        TimelineCandidateKind::Span { range, .. } => range.start(),
        TimelineCandidateKind::Event { at } => at,
    };
    (candidate.row_key, at, candidate.item_id)
}

fn span_lane_index(
    row_id: TimelineRenderRowId,
    range: CanonicalTimeRange,
    span_lanes: &mut BTreeMap<TimelineRenderRowId, Vec<CanonicalTimeKey>>,
) -> u32 {
    let lanes = span_lanes.entry(row_id).or_default();
    for (index, lane_end) in lanes.iter_mut().enumerate() {
        if *lane_end <= range.start() {
            *lane_end = range.end();
            return u32::try_from(index).unwrap_or(u32::MAX);
        }
    }
    lanes.push(range.end());
    u32::try_from(lanes.len() - 1).unwrap_or(u32::MAX)
}

fn row_key_for_item(
    item: &TimelineItem,
    grouping_mode: TimelineGroupingMode,
) -> TimelineRenderRowKey {
    match grouping_mode {
        TimelineGroupingMode::GroupKey => TimelineRenderRowKey::Interned(item.group_key()),
        TimelineGroupingMode::SourceKey => TimelineRenderRowKey::Interned(item.source_key()),
        TimelineGroupingMode::Label => TimelineRenderRowKey::Interned(item.label()),
        TimelineGroupingMode::All => TimelineRenderRowKey::All,
    }
}

fn ranges_intersect(range: CanonicalTimeRange, visible_range: CanonicalTimeRange) -> bool {
    range.start() <= visible_range.end() && range.end() >= visible_range.start()
}

fn instant_in_range(at: CanonicalTimeKey, visible_range: CanonicalTimeRange) -> bool {
    at >= visible_range.start() && at <= visible_range.end()
}

fn visible_duration_femtoseconds(query: &TimelineViewportQuery) -> u128 {
    u128::try_from(query.visible_range().duration().raw_value()).unwrap_or(u128::MAX)
}

#[expect(
    clippy::cast_precision_loss,
    reason = "query projection converts exact femtosecond offsets into viewport pixels at the render-plan boundary"
)]
fn projected_instant_distance_pixels(
    previous: CanonicalTimeKey,
    next: CanonicalTimeKey,
    query: &TimelineViewportQuery,
) -> f64 {
    let visible_duration = visible_duration_femtoseconds(query);
    if visible_duration == 0 {
        return f64::from(query.viewport_width_pixels());
    }
    previous
        .raw_femtoseconds()
        .abs_diff(next.raw_femtoseconds()) as f64
        * f64::from(query.viewport_width_pixels())
        / visible_duration as f64
}

#[expect(
    clippy::cast_precision_loss,
    reason = "query projection converts exact femtosecond offsets into viewport pixels at the render-plan boundary"
)]
fn projected_width_pixels(range: CanonicalTimeRange, query: &TimelineViewportQuery) -> f64 {
    let visible_duration = visible_duration_femtoseconds(query);
    if visible_duration == 0 {
        return f64::from(query.viewport_width_pixels());
    }
    u128::try_from(range.duration().raw_value()).unwrap_or(u128::MAX) as f64
        * f64::from(query.viewport_width_pixels())
        / visible_duration as f64
}

#[cfg(test)]
mod tests {
    use super::{TimelineDataset, TimelineGroupingMode, TimelineRenderItem, TimelineViewportQuery};
    use crate::{CanonicalTimeKey, TimelineItemInput};

    #[test]
    fn render_plan_reports_revisions_and_pending_writes() {
        let mut dataset = TimelineDataset::new();
        dataset.push_event(
            TimelineItemInput::new("pending"),
            CanonicalTimeKey::from_femtoseconds(5),
        );
        let query = TimelineViewportQuery::try_new(
            CanonicalTimeKey::from_femtoseconds(0),
            CanonicalTimeKey::from_femtoseconds(10),
            CanonicalTimeKey::from_femtoseconds(10),
            100,
        )
        .expect("query should build");

        let plan = dataset.render_plan(&query);

        assert_eq!(plan.dataset_revision(), dataset.revision());
        assert_eq!(plan.index_revision(), dataset.index_revision());
        assert_eq!(plan.pending_write_count(), 1);
        assert!(plan.items().is_empty());
    }

    #[test]
    fn grouping_derives_compact_rows_without_sparse_source_gaps() {
        let mut dataset = TimelineDataset::new();
        dataset.push_event(
            TimelineItemInput::new("job 1").with_group_key("job-1"),
            CanonicalTimeKey::from_femtoseconds(1),
        );
        dataset.push_event(
            TimelineItemInput::new("job 207").with_group_key("job-207"),
            CanonicalTimeKey::from_femtoseconds(2),
        );
        dataset.compact();
        let query = TimelineViewportQuery::try_new(
            CanonicalTimeKey::from_femtoseconds(0),
            CanonicalTimeKey::from_femtoseconds(3),
            CanonicalTimeKey::from_femtoseconds(3),
            300,
        )
        .expect("query should build");

        let plan = dataset.render_plan(&query);

        assert_eq!(plan.rows().len(), 2);
        assert_eq!(plan.rows()[0].id().as_u32(), 0);
        assert_eq!(plan.rows()[1].id().as_u32(), 1);
    }

    #[test]
    fn open_spans_materialize_to_query_now() {
        let mut dataset = TimelineDataset::new();
        let span_id = dataset
            .push_span(
                TimelineItemInput::new("running").with_group_key("jobs"),
                CanonicalTimeKey::from_femtoseconds(10),
                None,
            )
            .expect("span should insert");
        dataset.compact();
        let query = TimelineViewportQuery::try_new(
            CanonicalTimeKey::from_femtoseconds(0),
            CanonicalTimeKey::from_femtoseconds(100),
            CanonicalTimeKey::from_femtoseconds(70),
            1_000,
        )
        .expect("query should build");

        let plan = dataset.render_plan(&query);

        let span = plan
            .items()
            .iter()
            .find_map(|item| match item {
                TimelineRenderItem::Span(span) if span.item_id() == span_id => Some(*span),
                _ => None,
            })
            .expect("open span should render");
        assert_eq!(span.range().end(), CanonicalTimeKey::from_femtoseconds(70));
        assert!(span.is_open());
    }

    #[test]
    fn tiny_spans_fold_into_cluster_without_mutating_raw_items() {
        let mut dataset = TimelineDataset::new();
        for start in [10, 12, 14] {
            dataset
                .push_span(
                    TimelineItemInput::new(format!("span-{start}")).with_group_key("dense"),
                    CanonicalTimeKey::from_femtoseconds(start),
                    Some(CanonicalTimeKey::from_femtoseconds(start + 1)),
                )
                .expect("span should insert");
        }
        dataset.compact();
        let query = TimelineViewportQuery::try_new(
            CanonicalTimeKey::from_femtoseconds(0),
            CanonicalTimeKey::from_femtoseconds(10_000),
            CanonicalTimeKey::from_femtoseconds(10_000),
            100,
        )
        .expect("query should build")
        .with_minimum_visible_pixels(2);

        let plan = dataset.render_plan(&query);

        let cluster = plan
            .items()
            .iter()
            .find_map(|item| match item {
                TimelineRenderItem::FoldedSpanCluster(cluster) => Some(*cluster),
                _ => None,
            })
            .expect("folded span cluster should exist");
        assert_eq!(cluster.count(), 3);
        assert_eq!(dataset.items().len(), 3);
    }

    #[test]
    fn dense_events_fold_into_cluster() {
        let mut dataset = TimelineDataset::new();
        for at in [10, 11, 12] {
            dataset.push_event(
                TimelineItemInput::new(format!("event-{at}")).with_group_key("dense"),
                CanonicalTimeKey::from_femtoseconds(at),
            );
        }
        dataset.compact();
        let query = TimelineViewportQuery::try_new(
            CanonicalTimeKey::from_femtoseconds(0),
            CanonicalTimeKey::from_femtoseconds(100),
            CanonicalTimeKey::from_femtoseconds(100),
            10,
        )
        .expect("query should build")
        .with_minimum_visible_pixels(2);

        let plan = dataset.render_plan(&query);

        let cluster = plan
            .items()
            .iter()
            .find_map(|item| match item {
                TimelineRenderItem::FoldedEventCluster(cluster) => Some(*cluster),
                _ => None,
            })
            .expect("folded event cluster should exist");
        assert_eq!(cluster.count(), 3);
        assert_eq!(
            cluster.range().start(),
            CanonicalTimeKey::from_femtoseconds(10)
        );
        assert_eq!(
            cluster.range().end(),
            CanonicalTimeKey::from_femtoseconds(12)
        );
    }

    #[test]
    fn grouping_mode_can_use_source_keys_for_row_derivation() {
        let mut dataset = TimelineDataset::new();
        dataset.push_event(
            TimelineItemInput::new("event-a")
                .with_group_key("same")
                .with_source_key("source-a"),
            CanonicalTimeKey::from_femtoseconds(10),
        );
        dataset.push_event(
            TimelineItemInput::new("event-b")
                .with_group_key("same")
                .with_source_key("source-b"),
            CanonicalTimeKey::from_femtoseconds(20),
        );
        dataset.compact();

        let plan = dataset.render_plan(
            &TimelineViewportQuery::try_new(
                CanonicalTimeKey::from_femtoseconds(0),
                CanonicalTimeKey::from_femtoseconds(30),
                CanonicalTimeKey::from_femtoseconds(30),
                100,
            )
            .expect("query should build")
            .with_grouping_mode(TimelineGroupingMode::SourceKey),
        );

        assert_eq!(plan.rows().len(), 2);
    }
}

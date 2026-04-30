use std::collections::BTreeMap;

use arbitrary::Arbitrary;
use facet::Facet;

use super::dataset::{
    TimelineDataset, TimelineDatasetRevision, TimelineInternedStringId, TimelineItem,
    TimelineItemId, TimelineItemKind,
};
use super::time::{TimelineInstantNs, TimelineRangeNs};

#[derive(Arbitrary, Facet, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum TimelineGroupingMode {
    #[default]
    GroupKey,
    SourceKey,
    Label,
    All,
}

#[derive(Arbitrary, Facet, Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TimelineRenderRowId(u32);

impl TimelineRenderRowId {
    #[must_use]
    pub const fn as_u32(self) -> u32 {
        self.0
    }
}

#[derive(Facet, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(C)]
pub enum TimelineRenderRowKey {
    Interned(TimelineInternedStringId),
    All,
}

#[derive(Facet, Clone, Debug, PartialEq, Eq)]
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

#[derive(Facet, Clone, Debug, PartialEq, Eq)]
// timeline[impl display.query-explicit-now]
pub struct TimelineViewportQuery {
    visible_range: TimelineRangeNs,
    now: TimelineInstantNs,
    viewport_width_pixels: u32,
    grouping_mode: TimelineGroupingMode,
    minimum_visible_pixels: u32,
}

impl TimelineViewportQuery {
    /// # Errors
    ///
    /// Returns an error when `visible_range_end` is earlier than `visible_range_start`.
    pub fn try_new(
        visible_range_start: TimelineInstantNs,
        visible_range_end: TimelineInstantNs,
        now: TimelineInstantNs,
        viewport_width_pixels: u32,
    ) -> eyre::Result<Self> {
        Ok(Self {
            visible_range: TimelineRangeNs::try_new(visible_range_start, visible_range_end)?,
            now,
            viewport_width_pixels,
            grouping_mode: TimelineGroupingMode::default(),
            minimum_visible_pixels: 1,
        })
    }

    #[must_use]
    pub const fn visible_range(&self) -> TimelineRangeNs {
        self.visible_range
    }

    #[must_use]
    pub const fn now(&self) -> TimelineInstantNs {
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
        // timeline[impl playground.minimum-span-marker]
    }
}

impl<'a> Arbitrary<'a> for TimelineViewportQuery {
    fn arbitrary(u: &mut arbitrary::Unstructured<'a>) -> arbitrary::Result<Self> {
        let visible_range = TimelineRangeNs::arbitrary(u)?;
        let now = TimelineInstantNs::arbitrary(u)?;
        let viewport_width_pixels = u.int_in_range(1_u32..=4_096)?;
        let minimum_visible_pixels = u.int_in_range(1_u32..=16)?;
        let grouping_mode = TimelineGroupingMode::arbitrary(u)?;
        Ok(Self {
            visible_range,
            now,
            viewport_width_pixels,
            grouping_mode,
            minimum_visible_pixels,
        })
    }
}

#[derive(Facet, Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimelineRenderSpan {
    item_id: TimelineItemId,
    row_id: TimelineRenderRowId,
    lane_index: u32,
    range: TimelineRangeNs,
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
    pub const fn range(self) -> TimelineRangeNs {
        self.range
    }

    #[must_use]
    pub const fn is_open(self) -> bool {
        self.is_open
    }
}

#[derive(Facet, Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimelineRenderEvent {
    item_id: TimelineItemId,
    row_id: TimelineRenderRowId,
    at: TimelineInstantNs,
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
    pub const fn at(self) -> TimelineInstantNs {
        self.at
    }
}

#[derive(Facet, Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimelineRenderCluster {
    row_id: TimelineRenderRowId,
    range: TimelineRangeNs,
    count: usize,
    representative_item_id: TimelineItemId,
}

impl TimelineRenderCluster {
    #[must_use]
    pub const fn row_id(self) -> TimelineRenderRowId {
        self.row_id
    }

    #[must_use]
    pub const fn range(self) -> TimelineRangeNs {
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

#[derive(Facet, Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
// timeline[impl display.query-render-items]
// timeline[impl display.query-folding]
pub enum TimelineRenderItem {
    Span(TimelineRenderSpan),
    Event(TimelineRenderEvent),
    FoldedSpanCluster(TimelineRenderCluster),
    FoldedEventCluster(TimelineRenderCluster),
}

#[derive(Facet, Clone, Debug, PartialEq, Eq)]
// timeline[impl display.query-derived-rows]
// timeline[impl display.query-render-items]
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
        range: TimelineRangeNs,
        is_open: bool,
    },
    Event {
        at: TimelineInstantNs,
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
    // timeline[impl display.query-explicit-now]
    // timeline[impl display.query-derived-rows]
    // timeline[impl display.query-render-items]
    // timeline[impl display.query-folding]
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
            let Ok(range) = TimelineRangeNs::try_new(span.start(), end) else {
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
    let mut folded_spans: Vec<(TimelineItemId, TimelineRenderRowId, TimelineRangeNs)> = Vec::new();
    let mut folded_events = Vec::new();
    let mut span_lanes = BTreeMap::new();

    for candidate in candidates {
        let row_id = row_ids[&candidate.row_key];
        match candidate.kind {
            TimelineCandidateKind::Span { range, is_open } => {
                if projected_width_pixels(range, query) < f64::from(query.minimum_visible_pixels())
                {
                    // timeline[impl playground.minimum-span-marker]
                    // timeline[impl playground.span-cluster-decomposition]
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
    folded_spans: &mut Vec<(TimelineItemId, TimelineRenderRowId, TimelineRangeNs)>,
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
        .expect("folded span cluster has a start");
    let end = folded_spans
        .iter()
        .map(|(_, _, range)| range.end())
        .max()
        .expect("folded span cluster has an end");
    let representative_item_id = folded_spans[0].0;
    let range = TimelineRangeNs::try_new(start, end).expect("cluster range is ordered");
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
    folded_events: &mut Vec<(TimelineItemId, TimelineRenderRowId, TimelineInstantNs)>,
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
        .expect("folded event cluster has a start");
    let end = folded_events
        .iter()
        .map(|(_, _, at)| *at)
        .max()
        .expect("folded event cluster has an end");
    let representative_item_id = folded_events[0].0;
    let range = TimelineRangeNs::try_new(start, end).expect("cluster range is ordered");
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
) -> (TimelineRenderRowKey, TimelineInstantNs, TimelineItemId) {
    let at = match candidate.kind {
        TimelineCandidateKind::Span { range, .. } => range.start(),
        TimelineCandidateKind::Event { at } => at,
    };
    (candidate.row_key, at, candidate.item_id)
}

fn span_lane_index(
    row_id: TimelineRenderRowId,
    range: TimelineRangeNs,
    span_lanes: &mut BTreeMap<TimelineRenderRowId, Vec<TimelineInstantNs>>,
) -> u32 {
    // timeline[impl playground.span-lanes]
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

fn ranges_intersect(range: TimelineRangeNs, visible_range: TimelineRangeNs) -> bool {
    range.start() <= visible_range.end() && range.end() >= visible_range.start()
}

fn instant_in_range(at: TimelineInstantNs, visible_range: TimelineRangeNs) -> bool {
    at >= visible_range.start() && at <= visible_range.end()
}

#[expect(
    clippy::cast_precision_loss,
    reason = "query projection converts integer nanoseconds to viewport pixels at the render-plan boundary"
)]
fn projected_instant_distance_pixels(
    previous: TimelineInstantNs,
    next: TimelineInstantNs,
    query: &TimelineViewportQuery,
) -> f64 {
    let visible_duration_ns = query.visible_range().duration().as_u64();
    if visible_duration_ns == 0 {
        return f64::from(query.viewport_width_pixels());
    }
    next.as_i64().abs_diff(previous.as_i64()) as f64 * f64::from(query.viewport_width_pixels())
        / visible_duration_ns as f64
}

#[expect(
    clippy::cast_precision_loss,
    reason = "query projection converts integer nanoseconds to viewport pixels at the render-plan boundary"
)]
fn projected_width_pixels(range: TimelineRangeNs, query: &TimelineViewportQuery) -> f64 {
    let visible_duration_ns = query.visible_range().duration().as_u64();
    if visible_duration_ns == 0 {
        return f64::from(query.viewport_width_pixels());
    }
    range.duration().as_u64() as f64 * f64::from(query.viewport_width_pixels())
        / visible_duration_ns as f64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeline::dataset::TimelineItemInput;

    #[test]
    // timeline[verify display.query-render-items]
    fn render_plan_reports_revisions_and_pending_writes() {
        let mut dataset = TimelineDataset::new();
        dataset.push_event(TimelineItemInput::new("pending"), TimelineInstantNs::new(5));
        let query = TimelineViewportQuery::try_new(
            TimelineInstantNs::new(0),
            TimelineInstantNs::new(10),
            TimelineInstantNs::new(10),
            100,
        )
        .expect("query");

        let plan = dataset.render_plan(&query);

        assert_eq!(plan.dataset_revision(), dataset.revision());
        assert_eq!(plan.index_revision(), dataset.index_revision());
        assert_eq!(plan.pending_write_count(), 1);
        assert!(plan.items().is_empty());
    }

    #[test]
    // timeline[verify display.query-render-items]
    fn render_plan_uses_compacted_indexes() {
        let mut dataset = TimelineDataset::new();
        let span_id = dataset
            .push_span(
                TimelineItemInput::new("download").with_group_key("jobs"),
                TimelineInstantNs::new(10),
                Some(TimelineInstantNs::new(30)),
            )
            .expect("span");
        let event_id = dataset.push_event(
            TimelineItemInput::new("log").with_group_key("logs"),
            TimelineInstantNs::new(20),
        );
        dataset.compact();
        let query = TimelineViewportQuery::try_new(
            TimelineInstantNs::new(0),
            TimelineInstantNs::new(40),
            TimelineInstantNs::new(40),
            400,
        )
        .expect("query");

        let plan = dataset.render_plan(&query);

        assert_eq!(plan.pending_write_count(), 0);
        assert_eq!(plan.rows().len(), 2);
        assert!(plan.items().iter().any(|item| matches!(
            item,
            TimelineRenderItem::Span(span) if span.item_id() == span_id
        )));
        assert!(plan.items().iter().any(|item| matches!(
            item,
            TimelineRenderItem::Event(event) if event.item_id() == event_id
        )));
    }

    #[test]
    // timeline[verify display.query-derived-rows]
    fn grouping_derives_compact_rows_without_sparse_source_gaps() {
        let mut dataset = TimelineDataset::new();
        dataset.push_event(
            TimelineItemInput::new("job 1").with_group_key("job-1"),
            TimelineInstantNs::new(1),
        );
        dataset.push_event(
            TimelineItemInput::new("job 207").with_group_key("job-207"),
            TimelineInstantNs::new(2),
        );
        dataset.compact();
        let query = TimelineViewportQuery::try_new(
            TimelineInstantNs::new(0),
            TimelineInstantNs::new(3),
            TimelineInstantNs::new(3),
            300,
        )
        .expect("query");

        let plan = dataset.render_plan(&query);

        assert_eq!(plan.rows().len(), 2);
        assert_eq!(plan.rows()[0].id().as_u32(), 0);
        assert_eq!(plan.rows()[1].id().as_u32(), 1);
    }

    #[test]
    // timeline[verify display.query-explicit-now]
    fn open_spans_materialize_to_query_now() {
        let mut dataset = TimelineDataset::new();
        let span_id = dataset
            .push_span(
                TimelineItemInput::new("running").with_group_key("jobs"),
                TimelineInstantNs::new(10),
                None,
            )
            .expect("span");
        dataset.compact();
        let query = TimelineViewportQuery::try_new(
            TimelineInstantNs::new(0),
            TimelineInstantNs::new(100),
            TimelineInstantNs::new(70),
            1_000,
        )
        .expect("query");

        let plan = dataset.render_plan(&query);

        let span = plan
            .items()
            .iter()
            .find_map(|item| match item {
                TimelineRenderItem::Span(span) if span.item_id() == span_id => Some(*span),
                _ => None,
            })
            .expect("open span item");
        assert_eq!(span.range().end(), TimelineInstantNs::new(70));
        assert!(span.is_open());
    }

    #[test]
    // timeline[verify display.query-folding]
    fn tiny_spans_fold_into_cluster_without_mutating_raw_items() {
        let mut dataset = TimelineDataset::new();
        for start in [10, 12, 14] {
            dataset
                .push_span(
                    TimelineItemInput::new(format!("span-{start}")).with_group_key("dense"),
                    TimelineInstantNs::new(start),
                    Some(TimelineInstantNs::new(start + 1)),
                )
                .expect("span");
        }
        dataset.compact();
        let query = TimelineViewportQuery::try_new(
            TimelineInstantNs::new(0),
            TimelineInstantNs::new(10_000),
            TimelineInstantNs::new(10_000),
            100,
        )
        .expect("query")
        .with_minimum_visible_pixels(2);

        let plan = dataset.render_plan(&query);

        let cluster = plan
            .items()
            .iter()
            .find_map(|item| match item {
                TimelineRenderItem::FoldedSpanCluster(cluster) => Some(*cluster),
                _ => None,
            })
            .expect("folded span cluster");
        assert_eq!(cluster.count(), 3);
        assert_eq!(dataset.items().len(), 3);
    }

    #[test]
    // timeline[verify display.query-folding]
    fn zooming_in_unfolds_tiny_spans() {
        let mut dataset = TimelineDataset::new();
        for start in [10, 30, 50] {
            dataset
                .push_span(
                    TimelineItemInput::new(format!("span-{start}")).with_group_key("dense"),
                    TimelineInstantNs::new(start),
                    Some(TimelineInstantNs::new(start + 10)),
                )
                .expect("span");
        }
        dataset.compact();
        let query = TimelineViewportQuery::try_new(
            TimelineInstantNs::new(0),
            TimelineInstantNs::new(100),
            TimelineInstantNs::new(100),
            1_000,
        )
        .expect("query")
        .with_minimum_visible_pixels(2);

        let plan = dataset.render_plan(&query);

        assert_eq!(plan.items().len(), 3);
        assert!(
            plan.items()
                .iter()
                .all(|item| matches!(item, TimelineRenderItem::Span(_)))
        );
    }

    #[test]
    // timeline[verify playground.span-lanes]
    fn overlapping_spans_get_nested_lane_indices_within_row() {
        let mut dataset = TimelineDataset::new();
        let first_id = dataset
            .push_span(
                TimelineItemInput::new("outer").with_group_key("thread-a"),
                TimelineInstantNs::new(10),
                Some(TimelineInstantNs::new(80)),
            )
            .expect("span");
        let nested_id = dataset
            .push_span(
                TimelineItemInput::new("inner").with_group_key("thread-a"),
                TimelineInstantNs::new(20),
                Some(TimelineInstantNs::new(40)),
            )
            .expect("span");
        let later_id = dataset
            .push_span(
                TimelineItemInput::new("later").with_group_key("thread-a"),
                TimelineInstantNs::new(85),
                Some(TimelineInstantNs::new(90)),
            )
            .expect("span");
        dataset.compact();
        let query = TimelineViewportQuery::try_new(
            TimelineInstantNs::new(0),
            TimelineInstantNs::new(100),
            TimelineInstantNs::new(100),
            1_000,
        )
        .expect("query")
        .with_minimum_visible_pixels(1);

        let plan = dataset.render_plan(&query);
        let span_lane = |id| {
            plan.items()
                .iter()
                .find_map(|item| match item {
                    TimelineRenderItem::Span(span) if span.item_id() == id => {
                        Some(span.lane_index())
                    }
                    _ => None,
                })
                .expect("rendered span")
        };

        assert_eq!(span_lane(first_id), 0);
        assert_eq!(span_lane(nested_id), 1);
        assert_eq!(span_lane(later_id), 0);
    }

    #[test]
    // timeline[verify playground.minimum-span-marker]
    fn tiny_spans_in_different_rows_do_not_fold_into_single_row() {
        let mut dataset = TimelineDataset::new();
        dataset
            .push_span(
                TimelineItemInput::new("row-a").with_group_key("row-a"),
                TimelineInstantNs::new(10),
                Some(TimelineInstantNs::new(11)),
            )
            .expect("span");
        dataset
            .push_span(
                TimelineItemInput::new("row-b").with_group_key("row-b"),
                TimelineInstantNs::new(12),
                Some(TimelineInstantNs::new(13)),
            )
            .expect("span");
        dataset.compact();
        let query = TimelineViewportQuery::try_new(
            TimelineInstantNs::new(0),
            TimelineInstantNs::new(1_000_000),
            TimelineInstantNs::new(1_000_000),
            100,
        )
        .expect("query")
        .with_minimum_visible_pixels(8);

        let plan = dataset.render_plan(&query);
        let row_ids = plan
            .items()
            .iter()
            .map(|item| match item {
                TimelineRenderItem::Span(span) => span.row_id(),
                TimelineRenderItem::FoldedSpanCluster(cluster)
                | TimelineRenderItem::FoldedEventCluster(cluster) => cluster.row_id(),
                TimelineRenderItem::Event(event) => event.row_id(),
            })
            .collect::<std::collections::BTreeSet<_>>();

        assert_eq!(row_ids.len(), 2);
    }

    #[test]
    // timeline[verify playground.span-cluster-decomposition]
    fn separated_tiny_spans_decompose_into_individual_markers() {
        let mut dataset = TimelineDataset::new();
        for start in [10, 50, 90] {
            dataset
                .push_span(
                    TimelineItemInput::new(format!("span-{start}")).with_group_key("dense"),
                    TimelineInstantNs::new(start),
                    Some(TimelineInstantNs::new(start + 1)),
                )
                .expect("span");
        }
        dataset.compact();
        let query = TimelineViewportQuery::try_new(
            TimelineInstantNs::new(0),
            TimelineInstantNs::new(100),
            TimelineInstantNs::new(100),
            100,
        )
        .expect("query")
        .with_minimum_visible_pixels(10);

        let plan = dataset.render_plan(&query);

        assert_eq!(plan.items().len(), 3);
        assert!(
            plan.items()
                .iter()
                .all(|item| matches!(item, TimelineRenderItem::Span(_)))
        );
    }

    #[test]
    // timeline[verify display.query-folding]
    fn dense_events_fold_into_cluster() {
        let mut dataset = TimelineDataset::new();
        for at in [10, 11, 12] {
            dataset.push_event(
                TimelineItemInput::new(format!("event-{at}")).with_group_key("dense"),
                TimelineInstantNs::new(at),
            );
        }
        dataset.compact();
        let query = TimelineViewportQuery::try_new(
            TimelineInstantNs::new(0),
            TimelineInstantNs::new(100),
            TimelineInstantNs::new(100),
            10,
        )
        .expect("query")
        .with_minimum_visible_pixels(2);

        let plan = dataset.render_plan(&query);

        let cluster = plan
            .items()
            .iter()
            .find_map(|item| match item {
                TimelineRenderItem::FoldedEventCluster(cluster) => Some(*cluster),
                _ => None,
            })
            .expect("folded event cluster");
        assert_eq!(cluster.count(), 3);
        assert_eq!(cluster.range().start(), TimelineInstantNs::new(10));
        assert_eq!(cluster.range().end(), TimelineInstantNs::new(12));
    }

    #[test]
    // timeline[verify display.query-folding]
    fn zooming_in_unfolds_dense_events() {
        let mut dataset = TimelineDataset::new();
        for at in [10, 11, 12] {
            dataset.push_event(
                TimelineItemInput::new(format!("event-{at}")).with_group_key("dense"),
                TimelineInstantNs::new(at),
            );
        }
        dataset.compact();
        let query = TimelineViewportQuery::try_new(
            TimelineInstantNs::new(0),
            TimelineInstantNs::new(100),
            TimelineInstantNs::new(100),
            1_000,
        )
        .expect("query")
        .with_minimum_visible_pixels(2);

        let plan = dataset.render_plan(&query);

        assert_eq!(plan.items().len(), 3);
        assert!(
            plan.items()
                .iter()
                .all(|item| matches!(item, TimelineRenderItem::Event(_)))
        );
    }

    #[test]
    // timeline[verify display.query-render-items]
    fn arbitrary_datasets_and_queries_render_without_panics() {
        for seed in 0_u8..=u8::MAX {
            let dataset_bytes = [seed; 128];
            let query_bytes = [u8::MAX - seed; 48];
            let mut dataset_unstructured = arbitrary::Unstructured::new(&dataset_bytes);
            let mut query_unstructured = arbitrary::Unstructured::new(&query_bytes);
            let Ok(mut dataset) = TimelineDataset::arbitrary(&mut dataset_unstructured) else {
                continue;
            };
            let Ok(query) = TimelineViewportQuery::arbitrary(&mut query_unstructured) else {
                continue;
            };

            dataset.compact();
            let plan = dataset.render_plan(&query);

            assert_eq!(plan.pending_write_count(), 0);
        }
    }
}

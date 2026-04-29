use facet::Facet;

use super::dataset::{
    TimelineDataset, TimelineFieldValue, TimelineItem, TimelineItemId, TimelineItemKind,
};
use super::query::{TimelineRenderCluster, TimelineRenderItem};
use super::time::{TimelineInstantNs, TimelineRangeNs};

#[derive(Facet, Clone, Debug, PartialEq)]
// timeline[impl playground.detail-facet-pretty]
pub struct TimelinePlaygroundDetail {
    title: String,
    render_item: TimelinePlaygroundRenderItemDetail,
    item: Option<TimelinePlaygroundItemDetail>,
    representative_item: Option<TimelinePlaygroundItemDetail>,
}

impl TimelinePlaygroundDetail {
    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    #[must_use]
    pub const fn render_item(&self) -> &TimelinePlaygroundRenderItemDetail {
        &self.render_item
    }

    #[must_use]
    pub const fn item(&self) -> Option<&TimelinePlaygroundItemDetail> {
        self.item.as_ref()
    }

    #[must_use]
    pub const fn representative_item(&self) -> Option<&TimelinePlaygroundItemDetail> {
        self.representative_item.as_ref()
    }
}

#[derive(Facet, Clone, Debug, PartialEq)]
pub struct TimelinePlaygroundRenderItemDetail {
    kind: TimelinePlaygroundRenderItemKind,
    row_id: u32,
    item_id: Option<u64>,
    range_start_ns: Option<i64>,
    range_end_ns: Option<i64>,
    at_ns: Option<i64>,
    is_open: bool,
    cluster_count: Option<usize>,
    representative_item_id: Option<u64>,
}

#[derive(Facet, Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub enum TimelinePlaygroundRenderItemKind {
    Span,
    Event,
    FoldedSpanCluster,
    FoldedEventCluster,
}

#[derive(Facet, Clone, Debug, PartialEq)]
pub struct TimelinePlaygroundItemDetail {
    item_id: u64,
    sequence: u64,
    label: String,
    source_key: String,
    group_key: String,
    kind: TimelinePlaygroundItemKind,
    start_ns: Option<i64>,
    end_ns: Option<i64>,
    at_ns: Option<i64>,
    is_open: bool,
    fields: Vec<TimelinePlaygroundFieldDetail>,
    object_refs: Vec<TimelinePlaygroundObjectRefDetail>,
}

#[derive(Facet, Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub enum TimelinePlaygroundItemKind {
    Span,
    Event,
}

#[derive(Facet, Clone, Debug, PartialEq)]
pub struct TimelinePlaygroundFieldDetail {
    name: String,
    value: TimelinePlaygroundFieldValueDetail,
}

#[derive(Facet, Clone, Debug, PartialEq)]
#[repr(C)]
pub enum TimelinePlaygroundFieldValueDetail {
    Bool(bool),
    I64(i64),
    U64(u64),
    F64(f64),
    String(String),
}

#[derive(Facet, Clone, Debug, PartialEq, Eq)]
pub struct TimelinePlaygroundObjectRefDetail {
    object_id: u64,
    type_key: String,
}

#[must_use]
// timeline[impl playground.detail-facet-pretty]
pub fn timeline_playground_detail_for_render_item(
    dataset: &TimelineDataset,
    render_item: TimelineRenderItem,
) -> Option<TimelinePlaygroundDetail> {
    let render_item_detail = render_item_detail(render_item);
    match render_item {
        TimelineRenderItem::Span(span) => {
            let item = dataset
                .item(span.item_id())
                .map(|item| item_detail(dataset, item));
            Some(TimelinePlaygroundDetail {
                title: item
                    .as_ref()
                    .map_or_else(|| "Span".to_owned(), |item| item.label.clone()),
                render_item: render_item_detail,
                item,
                representative_item: None,
            })
        }
        TimelineRenderItem::Event(event) => {
            let item = dataset
                .item(event.item_id())
                .map(|item| item_detail(dataset, item));
            Some(TimelinePlaygroundDetail {
                title: item
                    .as_ref()
                    .map_or_else(|| "Event".to_owned(), |item| item.label.clone()),
                render_item: render_item_detail,
                item,
                representative_item: None,
            })
        }
        TimelineRenderItem::FoldedSpanCluster(cluster)
        | TimelineRenderItem::FoldedEventCluster(cluster) => {
            let representative_item = dataset
                .item(cluster.representative_item_id())
                .map(|item| item_detail(dataset, item));
            Some(TimelinePlaygroundDetail {
                title: format!("Folded cluster ({} items)", cluster.count()),
                render_item: render_item_detail,
                item: None,
                representative_item,
            })
        }
    }
}

fn render_item_detail(render_item: TimelineRenderItem) -> TimelinePlaygroundRenderItemDetail {
    match render_item {
        TimelineRenderItem::Span(span) => render_item_span_detail(
            TimelinePlaygroundRenderItemKind::Span,
            span.row_id().as_u32(),
            Some(span.item_id()),
            span.range(),
            span.is_open(),
        ),
        TimelineRenderItem::Event(event) => TimelinePlaygroundRenderItemDetail {
            kind: TimelinePlaygroundRenderItemKind::Event,
            row_id: event.row_id().as_u32(),
            item_id: Some(event.item_id().as_u64()),
            range_start_ns: None,
            range_end_ns: None,
            at_ns: Some(event.at().as_i64()),
            is_open: false,
            cluster_count: None,
            representative_item_id: None,
        },
        TimelineRenderItem::FoldedSpanCluster(cluster) => {
            render_item_cluster_detail(TimelinePlaygroundRenderItemKind::FoldedSpanCluster, cluster)
        }
        TimelineRenderItem::FoldedEventCluster(cluster) => render_item_cluster_detail(
            TimelinePlaygroundRenderItemKind::FoldedEventCluster,
            cluster,
        ),
    }
}

fn render_item_span_detail(
    kind: TimelinePlaygroundRenderItemKind,
    row_id: u32,
    item_id: Option<TimelineItemId>,
    range: TimelineRangeNs,
    is_open: bool,
) -> TimelinePlaygroundRenderItemDetail {
    TimelinePlaygroundRenderItemDetail {
        kind,
        row_id,
        item_id: item_id.map(TimelineItemId::as_u64),
        range_start_ns: Some(range.start().as_i64()),
        range_end_ns: Some(range.end().as_i64()),
        at_ns: None,
        is_open,
        cluster_count: None,
        representative_item_id: None,
    }
}

fn render_item_cluster_detail(
    kind: TimelinePlaygroundRenderItemKind,
    cluster: TimelineRenderCluster,
) -> TimelinePlaygroundRenderItemDetail {
    TimelinePlaygroundRenderItemDetail {
        kind,
        row_id: cluster.row_id().as_u32(),
        item_id: None,
        range_start_ns: Some(cluster.range().start().as_i64()),
        range_end_ns: Some(cluster.range().end().as_i64()),
        at_ns: None,
        is_open: false,
        cluster_count: Some(cluster.count()),
        representative_item_id: Some(cluster.representative_item_id().as_u64()),
    }
}

fn item_detail(dataset: &TimelineDataset, item: &TimelineItem) -> TimelinePlaygroundItemDetail {
    let (kind, start_ns, end_ns, at_ns, is_open) = match item.kind() {
        TimelineItemKind::Span(span) => (
            TimelinePlaygroundItemKind::Span,
            Some(span.start().as_i64()),
            span.end().map(TimelineInstantNs::as_i64),
            None,
            span.is_open(),
        ),
        TimelineItemKind::Event(event) => (
            TimelinePlaygroundItemKind::Event,
            None,
            None,
            Some(event.at().as_i64()),
            false,
        ),
    };
    TimelinePlaygroundItemDetail {
        item_id: item.id().as_u64(),
        sequence: item.sequence().as_u64(),
        label: resolve_string(dataset, item.label()),
        source_key: resolve_string(dataset, item.source_key()),
        group_key: resolve_string(dataset, item.group_key()),
        kind,
        start_ns,
        end_ns,
        at_ns,
        is_open,
        fields: item
            .fields()
            .iter()
            .map(|field| TimelinePlaygroundFieldDetail {
                name: resolve_string(dataset, field.name()),
                value: field_value_detail(dataset, field.value()),
            })
            .collect(),
        object_refs: item
            .object_refs()
            .iter()
            .map(|object_ref| TimelinePlaygroundObjectRefDetail {
                object_id: object_ref.object_id(),
                type_key: resolve_string(dataset, object_ref.type_key()),
            })
            .collect(),
    }
}

fn field_value_detail(
    dataset: &TimelineDataset,
    value: &TimelineFieldValue,
) -> TimelinePlaygroundFieldValueDetail {
    match value {
        TimelineFieldValue::Bool(value) => TimelinePlaygroundFieldValueDetail::Bool(*value),
        TimelineFieldValue::I64(value) => TimelinePlaygroundFieldValueDetail::I64(*value),
        TimelineFieldValue::U64(value) => TimelinePlaygroundFieldValueDetail::U64(*value),
        TimelineFieldValue::F64(value) => TimelinePlaygroundFieldValueDetail::F64(*value),
        TimelineFieldValue::String(value) => {
            TimelinePlaygroundFieldValueDetail::String(resolve_string(dataset, *value))
        }
    }
}

fn resolve_string(
    dataset: &TimelineDataset,
    id: super::dataset::TimelineInternedStringId,
) -> String {
    dataset.resolve_string(id).unwrap_or("<missing>").to_owned()
}

#[cfg(test)]
mod tests {
    use facet_pretty::FacetPretty;

    use super::*;
    use crate::timeline::{TimelineItemInput, TimelineViewportQuery};

    #[test]
    // timeline[verify playground.detail-facet-pretty]
    fn playground_detail_pretty_prints_resolved_item_strings() {
        let mut dataset = TimelineDataset::new();
        let item_id = dataset
            .push_span(
                TimelineItemInput::new("Transcribe clip")
                    .with_source_key("synthetic/worker")
                    .with_group_key("job-1"),
                TimelineInstantNs::new(10),
                Some(TimelineInstantNs::new(20)),
            )
            .expect("span");
        dataset.compact();
        let query = TimelineViewportQuery::try_new(
            TimelineInstantNs::new(0),
            TimelineInstantNs::new(30),
            TimelineInstantNs::new(30),
            300,
        )
        .expect("query");
        let render_item = dataset
            .render_plan(&query)
            .items()
            .iter()
            .copied()
            .find(
                |item| matches!(item, TimelineRenderItem::Span(span) if span.item_id() == item_id),
            )
            .expect("render item");

        let detail =
            timeline_playground_detail_for_render_item(&dataset, render_item).expect("detail");
        let pretty = format!("{}", detail.pretty());

        assert!(pretty.contains("Transcribe clip"));
        assert!(pretty.contains("synthetic/worker"));
        assert!(pretty.contains("job-1"));
    }
}

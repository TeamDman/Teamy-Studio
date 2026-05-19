use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use linkme::distributed_slice;
use teamy_studio_shell::{
    ButtonVisualState, FEATURE_WINDOW_SCENE_REGISTRATIONS, FeatureWindowSceneContext,
    FeatureWindowSceneRegistration, LogicalWindowId, PanelEffect, PresentPolicy, RenderScene,
    WindowCreateRequest, WindowHostOptions, preferred_background_color, preferred_title_bar_color,
    push_panel, push_panel_with_data, push_text_block, push_title_text, push_window_chrome_buttons,
    push_window_garden_frame, scale_for_dpi,
};
use teamy_studio_timeline_core::{
    CanonicalTimeKey, CanonicalTimeRange, TimelineDataset, TimelineGroupingMode, TimelineItemKind,
    TimelineRenderItem, TimelineViewportQuery,
};
use windows::Win32::Foundation::{POINT, RECT};
use windows::Win32::UI::WindowsAndMessaging::{IDC_ARROW, IDC_HAND, IDC_SIZEALL};
use windows::core::PCWSTR;

pub(crate) const LIVE_APP_TIMELINE_WINDOW_TITLE: &str = "Timeline";
const NEXT_LIVE_APP_TIMELINE_WINDOW_ID_START: u64 = 10_000;
const TIMELINE_GROUPING_MODE: TimelineGroupingMode = TimelineGroupingMode::SourceKey;
const TIMELINE_MINIMUM_VISIBLE_PIXELS: u32 = 2;
const TIMELINE_ZOOM_IN_NUMERATOR: i128 = 3;
const TIMELINE_ZOOM_IN_DENOMINATOR: i128 = 4;
const TIMELINE_ZOOM_OUT_NUMERATOR: i128 = 4;
const TIMELINE_ZOOM_OUT_DENOMINATOR: i128 = 3;
const TIMELINE_PAN_NUMERATOR: i128 = 1;
const TIMELINE_PAN_DENOMINATOR: i128 = 4;
const TIMELINE_MIN_DURATION_FEMTOSECONDS: i128 = 8;
const TIMELINE_ZOOM_ANIMATION_STEPS: u8 = 5;

static NEXT_LIVE_APP_TIMELINE_WINDOW_ID: AtomicU64 =
    AtomicU64::new(NEXT_LIVE_APP_TIMELINE_WINDOW_ID_START);
static LIVE_APP_TIMELINE_WINDOW_MODEL: OnceLock<Mutex<LiveAppTimelineWindowModel>> =
    OnceLock::new();

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LiveAppTimelineWindowRenderItemKind {
    Span,
    Event,
    FoldedSpanCluster,
    FoldedEventCluster,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LiveAppTimelineWindowRenderItem {
    pub(crate) kind: LiveAppTimelineWindowRenderItemKind,
    pub(crate) start_femtoseconds: i128,
    pub(crate) end_femtoseconds: i128,
    pub(crate) lane_index: u32,
    pub(crate) count: usize,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct LiveAppTimelineWindowRowSnapshot {
    pub(crate) label: String,
    pub(crate) render_items: Vec<LiveAppTimelineWindowRenderItem>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct LiveAppTimelineWindowSnapshot {
    pub(crate) published_item_count: usize,
    pub(crate) visible_start_femtoseconds: i128,
    pub(crate) visible_end_femtoseconds: i128,
    pub(crate) visible_row_count: usize,
    pub(crate) visible_render_item_count: usize,
    pub(crate) folded_event_cluster_count: usize,
    pub(crate) folded_span_cluster_count: usize,
    pub(crate) rows: Vec<LiveAppTimelineWindowRowSnapshot>,
    pub(crate) recent_events: Vec<String>,
    pub(crate) empty_message: Option<String>,
}

impl LiveAppTimelineWindowSnapshot {
    #[must_use]
    pub(crate) fn with_empty_message(message: impl Into<String>) -> Self {
        Self {
            empty_message: Some(message.into()),
            ..Self::default()
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LiveAppTimelineControlAction {
    PanLeft,
    PanRight,
    ZoomIn,
    ZoomOut,
    Fit,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct LiveAppTimelineControlButton {
    action: LiveAppTimelineControlAction,
    label: &'static str,
    rect: RECT,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct LiveAppTimelineWindowLayout {
    body_rect: RECT,
    header_rect: RECT,
    summary_rect: RECT,
    controls_rect: RECT,
    lanes_rect: RECT,
    recent_rect: RECT,
    control_buttons: [LiveAppTimelineControlButton; 5],
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct LiveAppTimelinePanDrag {
    origin: POINT,
    origin_visible_range: CanonicalTimeRange,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct LiveAppTimelineZoomAnimation {
    start_visible_range: CanonicalTimeRange,
    target_visible_range: CanonicalTimeRange,
    step: u8,
}

#[derive(Clone, Debug, Default)]
struct LiveAppTimelineWindowModel {
    dataset: TimelineDataset,
    content_bounds: Option<CanonicalTimeRange>,
    visible_range: Option<CanonicalTimeRange>,
    pan_drag: Option<LiveAppTimelinePanDrag>,
    zoom_animation: Option<LiveAppTimelineZoomAnimation>,
}

#[distributed_slice(FEATURE_WINDOW_SCENE_REGISTRATIONS)]
pub static LIVE_APP_TIMELINE_WINDOW_SCENE: FeatureWindowSceneRegistration =
    FeatureWindowSceneRegistration {
        title: LIVE_APP_TIMELINE_WINDOW_TITLE,
        build_scene: build_live_app_timeline_window_scene,
        cursor_for_point: Some(live_app_timeline_cursor_for_point),
        on_left_click: Some(handle_live_app_timeline_left_click),
        on_mouse_wheel: Some(handle_live_app_timeline_mouse_wheel),
        on_right_button_down: Some(handle_live_app_timeline_right_button_down),
        on_right_drag: Some(handle_live_app_timeline_right_drag),
        on_right_button_up: Some(handle_live_app_timeline_right_button_up),
        on_frame_tick: Some(handle_live_app_timeline_frame_tick),
    };

#[must_use]
pub(crate) fn next_live_app_timeline_window_id() -> LogicalWindowId {
    LogicalWindowId::new(NEXT_LIVE_APP_TIMELINE_WINDOW_ID.fetch_add(1, Ordering::Relaxed))
}

#[must_use]
pub(crate) fn live_app_timeline_window_request(
    logical_window_id: LogicalWindowId,
) -> WindowCreateRequest {
    WindowCreateRequest {
        logical_window_id,
        title: LIVE_APP_TIMELINE_WINDOW_TITLE,
        present_policy: PresentPolicy::Composed,
        host_options: WindowHostOptions::standard_foreground(),
    }
}

pub(crate) fn store_live_app_timeline_window_dataset(dataset: TimelineDataset) {
    let content_bounds = dataset.time_bounds();
    let mut model = live_app_timeline_window_model()
        .lock()
        .expect("live app timeline model should not be poisoned");
    let visible_range = preserved_visible_range(model.visible_range, content_bounds);
    *model = LiveAppTimelineWindowModel {
        dataset,
        content_bounds,
        visible_range,
        pan_drag: None,
        zoom_animation: model.zoom_animation,
    };
}

#[cfg(test)]
pub(crate) fn current_live_app_timeline_window_snapshot_for_test(
    context: FeatureWindowSceneContext,
) -> LiveAppTimelineWindowSnapshot {
    current_live_app_timeline_window_snapshot(context)
}

fn current_live_app_timeline_window_snapshot(
    context: FeatureWindowSceneContext,
) -> LiveAppTimelineWindowSnapshot {
    let model = live_app_timeline_window_model()
        .lock()
        .expect("live app timeline model should not be poisoned")
        .clone();
    build_live_app_timeline_window_snapshot_from_model(&model, context)
}

#[cfg(test)]
fn reset_live_app_timeline_window_model_for_test() {
    *live_app_timeline_window_model()
        .lock()
        .expect("live app timeline model should not be poisoned") =
        LiveAppTimelineWindowModel::default();
}

fn live_app_timeline_window_model() -> &'static Mutex<LiveAppTimelineWindowModel> {
    LIVE_APP_TIMELINE_WINDOW_MODEL.get_or_init(|| Mutex::new(LiveAppTimelineWindowModel::default()))
}

fn build_live_app_timeline_window_scene(context: FeatureWindowSceneContext) -> RenderScene {
    let snapshot = current_live_app_timeline_window_snapshot(context);
    let shell_layout = context.layout;
    let layout = live_app_timeline_window_layout(context);
    let dpi = context.dpi;
    let mut scene = RenderScene::empty();

    push_panel(
        &mut scene,
        shell_layout.content_frame_rect,
        preferred_background_color(),
        PanelEffect::BlueBackground,
    );
    push_window_garden_frame(&mut scene, shell_layout);
    push_panel(
        &mut scene,
        shell_layout.title_bar_rect,
        preferred_title_bar_color(context.chrome.focused),
        PanelEffect::TitleBar,
    );
    push_window_chrome_buttons(&mut scene, shell_layout, context.chrome);
    push_title_text(
        &mut scene,
        shell_layout.title_text_rect,
        LIVE_APP_TIMELINE_WINDOW_TITLE,
        [0.96, 0.98, 1.00, 1.0],
    );
    push_panel(
        &mut scene,
        layout.body_rect,
        [0.06, 0.07, 0.10, 0.97],
        PanelEffect::SceneBody,
    );
    push_title_text(
        &mut scene,
        RECT {
            left: layout.header_rect.left,
            top: layout.header_rect.top,
            right: layout.header_rect.right,
            bottom: layout.header_rect.top + scale_for_dpi(28, dpi),
        },
        "Live App Timeline",
        [0.92, 0.97, 1.00, 1.0],
    );
    push_text_block(
        &mut scene,
        layout.summary_rect,
        &timeline_summary_text(&snapshot),
        scale_for_dpi(10, dpi).max(8),
        scale_for_dpi(18, dpi).max(12),
        [0.78, 0.84, 0.92, 1.0],
    );
    render_timeline_controls(&mut scene, context, layout);

    push_panel(
        &mut scene,
        layout.lanes_rect,
        [0.08, 0.10, 0.13, 0.96],
        PanelEffect::SceneBody,
    );
    if let Some(message) = &snapshot.empty_message {
        push_text_block(
            &mut scene,
            RECT {
                left: layout.lanes_rect.left + scale_for_dpi(16, dpi),
                top: layout.lanes_rect.top + scale_for_dpi(16, dpi),
                right: layout.lanes_rect.right - scale_for_dpi(16, dpi),
                bottom: layout.lanes_rect.bottom - scale_for_dpi(16, dpi),
            },
            message,
            scale_for_dpi(12, dpi).max(9),
            scale_for_dpi(20, dpi).max(14),
            [0.83, 0.87, 0.94, 1.0],
        );
    } else {
        render_timeline_rows(&mut scene, layout.lanes_rect, dpi, &snapshot);
    }

    push_panel(
        &mut scene,
        layout.recent_rect,
        [0.09, 0.11, 0.15, 0.98],
        PanelEffect::SceneBody,
    );
    push_text_block(
        &mut scene,
        RECT {
            left: layout.recent_rect.left + scale_for_dpi(14, dpi),
            top: layout.recent_rect.top + scale_for_dpi(12, dpi),
            right: layout.recent_rect.right - scale_for_dpi(14, dpi),
            bottom: layout.recent_rect.bottom - scale_for_dpi(12, dpi),
        },
        &timeline_recent_events_text(&snapshot),
        scale_for_dpi(10, dpi).max(8),
        scale_for_dpi(18, dpi).max(12),
        [0.80, 0.86, 0.94, 1.0],
    );

    scene
}

fn build_live_app_timeline_window_snapshot_from_model(
    model: &LiveAppTimelineWindowModel,
    context: FeatureWindowSceneContext,
) -> LiveAppTimelineWindowSnapshot {
    let Some(content_bounds) = model.content_bounds else {
        return LiveAppTimelineWindowSnapshot {
            published_item_count: model.dataset.items().len(),
            ..LiveAppTimelineWindowSnapshot::with_empty_message(
                "No published timeline events have been recorded yet.",
            )
        };
    };

    let layout = live_app_timeline_window_layout(context);
    let viewport_width_pixels =
        u32::try_from((layout.lanes_rect.right - layout.lanes_rect.left).max(1)).unwrap_or(1);
    let visible_range = model
        .visible_range
        .unwrap_or_else(|| fit_visible_range(content_bounds));
    let now = content_bounds.end().max(visible_range.end());
    let plan = model.dataset.render_plan(
        &TimelineViewportQuery::try_new(
            visible_range.start(),
            visible_range.end(),
            now,
            viewport_width_pixels,
        )
        .expect("timeline visible range should remain ordered")
        .with_grouping_mode(TIMELINE_GROUPING_MODE)
        .with_minimum_visible_pixels(TIMELINE_MINIMUM_VISIBLE_PIXELS),
    );

    let mut row_indices = std::collections::BTreeMap::new();
    let mut rows = Vec::new();
    for row in plan.rows() {
        let label = match row.key() {
            teamy_studio_timeline_core::TimelineRenderRowKey::Interned(id) => model
                .dataset
                .resolve_string(id)
                .unwrap_or("<unresolved row>")
                .to_owned(),
            teamy_studio_timeline_core::TimelineRenderRowKey::All => "all".to_owned(),
        };
        row_indices.insert(row.id(), rows.len());
        rows.push(LiveAppTimelineWindowRowSnapshot {
            label,
            render_items: Vec::new(),
        });
    }

    let mut snapshot = LiveAppTimelineWindowSnapshot {
        published_item_count: model.dataset.items().len(),
        visible_start_femtoseconds: visible_range.start().raw_femtoseconds(),
        visible_end_femtoseconds: visible_range.end().raw_femtoseconds(),
        visible_row_count: plan.rows().len(),
        visible_render_item_count: plan.items().len(),
        rows,
        ..LiveAppTimelineWindowSnapshot::default()
    };

    for item in plan.items() {
        let (row_id, render_item, is_folded_event, is_folded_span) = match item {
            TimelineRenderItem::Span(span) => (
                span.row_id(),
                LiveAppTimelineWindowRenderItem {
                    kind: LiveAppTimelineWindowRenderItemKind::Span,
                    start_femtoseconds: span.range().start().raw_femtoseconds(),
                    end_femtoseconds: span.range().end().raw_femtoseconds(),
                    lane_index: span.lane_index(),
                    count: 1,
                },
                false,
                false,
            ),
            TimelineRenderItem::Event(event) => (
                event.row_id(),
                LiveAppTimelineWindowRenderItem {
                    kind: LiveAppTimelineWindowRenderItemKind::Event,
                    start_femtoseconds: event.at().raw_femtoseconds(),
                    end_femtoseconds: event.at().raw_femtoseconds(),
                    lane_index: 0,
                    count: 1,
                },
                false,
                false,
            ),
            TimelineRenderItem::FoldedSpanCluster(cluster) => (
                cluster.row_id(),
                LiveAppTimelineWindowRenderItem {
                    kind: LiveAppTimelineWindowRenderItemKind::FoldedSpanCluster,
                    start_femtoseconds: cluster.range().start().raw_femtoseconds(),
                    end_femtoseconds: cluster.range().end().raw_femtoseconds(),
                    lane_index: 0,
                    count: cluster.count(),
                },
                false,
                true,
            ),
            TimelineRenderItem::FoldedEventCluster(cluster) => (
                cluster.row_id(),
                LiveAppTimelineWindowRenderItem {
                    kind: LiveAppTimelineWindowRenderItemKind::FoldedEventCluster,
                    start_femtoseconds: cluster.range().start().raw_femtoseconds(),
                    end_femtoseconds: cluster.range().end().raw_femtoseconds(),
                    lane_index: 0,
                    count: cluster.count(),
                },
                true,
                false,
            ),
        };

        if is_folded_event {
            snapshot.folded_event_cluster_count += 1;
        }
        if is_folded_span {
            snapshot.folded_span_cluster_count += 1;
        }

        if let Some(row_index) = row_indices.get(&row_id).copied() {
            snapshot.rows[row_index].render_items.push(render_item);
        }
    }

    for item in model.dataset.items().iter().rev().take(8) {
        let label = model
            .dataset
            .resolve_string(item.label())
            .unwrap_or("<unresolved label>");
        let source = model
            .dataset
            .resolve_string(item.source_key())
            .unwrap_or("<unresolved source>");
        let timestamp = match item.kind() {
            TimelineItemKind::Span(span) => span.start().raw_femtoseconds(),
            TimelineItemKind::Event(event) => event.at().raw_femtoseconds(),
        };
        snapshot
            .recent_events
            .push(format!("{timestamp}: {source} :: {label}"));
    }

    snapshot
}

fn render_timeline_controls(
    scene: &mut RenderScene,
    context: FeatureWindowSceneContext,
    layout: LiveAppTimelineWindowLayout,
) {
    let dpi = context.dpi;
    push_text_block(
        scene,
        RECT {
            left: layout.controls_rect.left,
            top: layout.controls_rect.top,
            right: layout.controls_rect.right,
            bottom: layout.controls_rect.top + scale_for_dpi(18, dpi),
        },
        "Controls",
        scale_for_dpi(10, dpi).max(8),
        scale_for_dpi(18, dpi).max(12),
        [0.84, 0.90, 0.96, 1.0],
    );

    for button in layout.control_buttons {
        let hovered = context
            .cursor_client_point
            .is_some_and(|point| rect_contains(button.rect, point));
        push_panel_with_data(
            scene,
            button.rect,
            if hovered {
                [0.24, 0.34, 0.48, 0.98]
            } else {
                [0.13, 0.17, 0.23, 0.98]
            },
            PanelEffect::SceneButtonCard,
            ButtonVisualState {
                hover_near: if hovered { 1.0 } else { 0.0 },
                hovered,
                active: hovered,
                ..Default::default()
            }
            .shader_data(),
        );
        push_title_text(scene, button.rect, button.label, [0.92, 0.96, 1.0, 1.0]);
    }
}

fn live_app_timeline_cursor_for_point(
    context: FeatureWindowSceneContext,
    point: POINT,
) -> Option<PCWSTR> {
    let layout = live_app_timeline_window_layout(context);
    if layout
        .control_buttons
        .iter()
        .any(|button| rect_contains(button.rect, point))
    {
        Some(IDC_HAND)
    } else if live_app_timeline_model_is_panning() || rect_contains(layout.lanes_rect, point) {
        Some(IDC_SIZEALL)
    } else {
        Some(IDC_ARROW)
    }
}

fn handle_live_app_timeline_left_click(context: FeatureWindowSceneContext, point: POINT) -> bool {
    let layout = live_app_timeline_window_layout(context);
    let Some(action) = layout
        .control_buttons
        .iter()
        .find(|button| rect_contains(button.rect, point))
        .map(|button| button.action)
    else {
        return false;
    };

    let mut model = live_app_timeline_window_model()
        .lock()
        .expect("live app timeline model should not be poisoned");
    match action {
        LiveAppTimelineControlAction::PanLeft => model.pan(-1),
        LiveAppTimelineControlAction::PanRight => model.pan(1),
        LiveAppTimelineControlAction::ZoomIn => {
            model.zoom(TIMELINE_ZOOM_IN_NUMERATOR, TIMELINE_ZOOM_IN_DENOMINATOR)
        }
        LiveAppTimelineControlAction::ZoomOut => {
            model.zoom(TIMELINE_ZOOM_OUT_NUMERATOR, TIMELINE_ZOOM_OUT_DENOMINATOR)
        }
        LiveAppTimelineControlAction::Fit => model.fit_to_content(),
    }
    true
}

fn handle_live_app_timeline_mouse_wheel(
    context: FeatureWindowSceneContext,
    point: POINT,
    delta: i16,
) -> bool {
    let layout = live_app_timeline_window_layout(context);
    if !rect_contains(layout.lanes_rect, point) || delta == 0 {
        return false;
    }
    let mut model = live_app_timeline_window_model()
        .lock()
        .expect("live app timeline model should not be poisoned");
    if delta > 0 {
        model.zoom_about_point(
            layout.lanes_rect,
            point,
            TIMELINE_ZOOM_IN_NUMERATOR,
            TIMELINE_ZOOM_IN_DENOMINATOR,
        );
    } else {
        model.zoom_about_point(
            layout.lanes_rect,
            point,
            TIMELINE_ZOOM_OUT_NUMERATOR,
            TIMELINE_ZOOM_OUT_DENOMINATOR,
        );
    }
    true
}

fn handle_live_app_timeline_right_button_down(
    context: FeatureWindowSceneContext,
    point: POINT,
) -> bool {
    let layout = live_app_timeline_window_layout(context);
    if !rect_contains(layout.lanes_rect, point) {
        return false;
    }
    let mut model = live_app_timeline_window_model()
        .lock()
        .expect("live app timeline model should not be poisoned");
    model.begin_pan_drag(point);
    true
}

fn handle_live_app_timeline_right_drag(context: FeatureWindowSceneContext, point: POINT) -> bool {
    let layout = live_app_timeline_window_layout(context);
    let mut model = live_app_timeline_window_model()
        .lock()
        .expect("live app timeline model should not be poisoned");
    model.apply_pan_drag(layout.lanes_rect, point)
}

fn handle_live_app_timeline_right_button_up(
    _context: FeatureWindowSceneContext,
    _point: POINT,
) -> bool {
    let mut model = live_app_timeline_window_model()
        .lock()
        .expect("live app timeline model should not be poisoned");
    model.pan_drag = None;
    true
}

fn handle_live_app_timeline_frame_tick(_context: FeatureWindowSceneContext) -> bool {
    let mut model = live_app_timeline_window_model()
        .lock()
        .expect("live app timeline model should not be poisoned");
    model.advance_zoom_animation()
}

fn render_timeline_rows(
    scene: &mut RenderScene,
    lanes_rect: RECT,
    dpi: u32,
    snapshot: &LiveAppTimelineWindowSnapshot,
) {
    let label_width = scale_for_dpi(180, dpi);
    let row_gap = scale_for_dpi(8, dpi);
    let row_height = scale_for_dpi(48, dpi).max(32);
    let available_height = (lanes_rect.bottom - lanes_rect.top).max(row_height);
    let rows_that_fit = ((available_height + row_gap) / (row_height + row_gap)).max(1);

    for (index, row) in snapshot.rows.iter().enumerate() {
        if index >= usize::try_from(rows_that_fit).unwrap_or(usize::MAX) {
            break;
        }

        let index_i32 = i32::try_from(index).unwrap_or(i32::MAX);
        let top = lanes_rect.top + index_i32 * (row_height + row_gap);
        let bottom = (top + row_height).min(lanes_rect.bottom - scale_for_dpi(8, dpi));
        let row_rect = RECT {
            left: lanes_rect.left + scale_for_dpi(8, dpi),
            top,
            right: lanes_rect.right - scale_for_dpi(8, dpi),
            bottom,
        };
        let label_rect = RECT {
            left: row_rect.left + scale_for_dpi(12, dpi),
            top: row_rect.top + scale_for_dpi(8, dpi),
            right: row_rect.left + label_width,
            bottom: row_rect.bottom - scale_for_dpi(8, dpi),
        };
        let lane_rect = RECT {
            left: label_rect.right + scale_for_dpi(8, dpi),
            top: row_rect.top + scale_for_dpi(8, dpi),
            right: row_rect.right - scale_for_dpi(12, dpi),
            bottom: row_rect.bottom - scale_for_dpi(8, dpi),
        };

        push_panel(
            scene,
            row_rect,
            [0.10, 0.12, 0.17, 0.95],
            PanelEffect::SceneBody,
        );
        push_panel(
            scene,
            lane_rect,
            [0.06, 0.08, 0.11, 0.95],
            PanelEffect::SceneBody,
        );
        let row_label = format!("{} ({})", row.label, row.render_items.len());
        push_text_block(
            scene,
            label_rect,
            &row_label,
            scale_for_dpi(10, dpi).max(8),
            scale_for_dpi(18, dpi).max(12),
            [0.87, 0.91, 0.97, 1.0],
        );

        for render_item in &row.render_items {
            if let Some(item_rect) = project_render_item_rect(snapshot, lane_rect, render_item) {
                push_panel(
                    scene,
                    item_rect,
                    render_item_color(render_item.kind),
                    PanelEffect::SceneButtonCard,
                );
                if render_item.count > 1 {
                    push_title_text(
                        scene,
                        item_rect,
                        &render_item.count.to_string(),
                        [0.04, 0.06, 0.08, 1.0],
                    );
                }
            }
        }
    }
}

fn live_app_timeline_window_layout(
    context: FeatureWindowSceneContext,
) -> LiveAppTimelineWindowLayout {
    let dpi = context.dpi;
    let body_rect = inset_rect(context.layout.body_rect, scale_for_dpi(22, dpi));
    let header_rect = RECT {
        left: body_rect.left + scale_for_dpi(18, dpi),
        top: body_rect.top + scale_for_dpi(18, dpi),
        right: body_rect.right - scale_for_dpi(18, dpi),
        bottom: body_rect.top + scale_for_dpi(120, dpi),
    };
    let controls_width = scale_for_dpi(320, dpi);
    let summary_rect = RECT {
        left: header_rect.left,
        top: header_rect.top + scale_for_dpi(28, dpi),
        right: header_rect.right - controls_width - scale_for_dpi(16, dpi),
        bottom: header_rect.bottom,
    };
    let controls_rect = RECT {
        left: header_rect.right - controls_width,
        top: header_rect.top + scale_for_dpi(8, dpi),
        right: header_rect.right,
        bottom: header_rect.bottom,
    };

    let button_gap = scale_for_dpi(8, dpi);
    let button_width = scale_for_dpi(56, dpi);
    let button_height = scale_for_dpi(28, dpi);
    let buttons_top = controls_rect.top + scale_for_dpi(24, dpi);
    let mut next_left = controls_rect.left;
    let mut build_button = |action, label| {
        let rect = RECT {
            left: next_left,
            top: buttons_top,
            right: next_left + button_width,
            bottom: buttons_top + button_height,
        };
        next_left += button_width + button_gap;
        LiveAppTimelineControlButton {
            action,
            label,
            rect,
        }
    };
    let control_buttons = [
        build_button(LiveAppTimelineControlAction::PanLeft, "<-"),
        build_button(LiveAppTimelineControlAction::PanRight, "->"),
        build_button(LiveAppTimelineControlAction::ZoomIn, "+"),
        build_button(LiveAppTimelineControlAction::ZoomOut, "-"),
        build_button(LiveAppTimelineControlAction::Fit, "Fit"),
    ];

    let recent_panel_height = scale_for_dpi(136, dpi);
    let lanes_rect = RECT {
        left: body_rect.left + scale_for_dpi(18, dpi),
        top: header_rect.bottom + scale_for_dpi(14, dpi),
        right: body_rect.right - scale_for_dpi(18, dpi),
        bottom: body_rect.bottom - recent_panel_height - scale_for_dpi(14, dpi),
    };
    let recent_rect = RECT {
        left: body_rect.left + scale_for_dpi(18, dpi),
        top: lanes_rect.bottom + scale_for_dpi(14, dpi),
        right: body_rect.right - scale_for_dpi(18, dpi),
        bottom: body_rect.bottom - scale_for_dpi(18, dpi),
    };

    LiveAppTimelineWindowLayout {
        body_rect,
        header_rect,
        summary_rect,
        controls_rect,
        lanes_rect,
        recent_rect,
        control_buttons,
    }
}

fn project_render_item_rect(
    snapshot: &LiveAppTimelineWindowSnapshot,
    lane_rect: RECT,
    render_item: &LiveAppTimelineWindowRenderItem,
) -> Option<RECT> {
    if lane_rect.right <= lane_rect.left || lane_rect.bottom <= lane_rect.top {
        return None;
    }

    let start_x = project_femtoseconds_to_x(
        render_item.start_femtoseconds,
        snapshot.visible_start_femtoseconds,
        snapshot.visible_end_femtoseconds,
        lane_rect,
    );
    let end_x = project_femtoseconds_to_x(
        render_item.end_femtoseconds,
        snapshot.visible_start_femtoseconds,
        snapshot.visible_end_femtoseconds,
        lane_rect,
    );
    let available_height = (lane_rect.bottom - lane_rect.top).max(8);
    let lane_slot_height = (available_height / 3).max(4);
    let lane_slot = i32::try_from(render_item.lane_index % 3).unwrap_or(0);
    let top = lane_rect.top + lane_slot * lane_slot_height + 2;
    let bottom = (top + lane_slot_height - 4).min(lane_rect.bottom - 2);

    let minimum_width = if matches!(
        render_item.kind,
        LiveAppTimelineWindowRenderItemKind::Event
            | LiveAppTimelineWindowRenderItemKind::FoldedEventCluster
    ) {
        3
    } else {
        6
    };
    let right = (end_x.max(start_x + minimum_width)).min(lane_rect.right - 1);
    if top >= bottom || start_x >= lane_rect.right {
        return None;
    }

    Some(RECT {
        left: start_x.clamp(lane_rect.left, lane_rect.right - 1),
        top,
        right,
        bottom,
    })
}

fn project_femtoseconds_to_x(
    femtoseconds: i128,
    visible_start_femtoseconds: i128,
    visible_end_femtoseconds: i128,
    lane_rect: RECT,
) -> i32 {
    let visible_duration = (visible_end_femtoseconds - visible_start_femtoseconds).max(1);
    let clamped = femtoseconds.clamp(visible_start_femtoseconds, visible_end_femtoseconds);
    let lane_width = i128::from((lane_rect.right - lane_rect.left).max(1));
    let offset = clamped - visible_start_femtoseconds;
    let projected = offset
        .saturating_mul(lane_width)
        .div_euclid(visible_duration);

    lane_rect
        .left
        .saturating_add(i32::try_from(projected).unwrap_or(i32::MAX))
}

fn render_item_color(kind: LiveAppTimelineWindowRenderItemKind) -> [f32; 4] {
    match kind {
        LiveAppTimelineWindowRenderItemKind::Span => [0.28, 0.78, 0.96, 0.95],
        LiveAppTimelineWindowRenderItemKind::Event => [1.00, 0.76, 0.36, 0.95],
        LiveAppTimelineWindowRenderItemKind::FoldedSpanCluster => [0.54, 0.88, 0.66, 0.95],
        LiveAppTimelineWindowRenderItemKind::FoldedEventCluster => [1.00, 0.48, 0.64, 0.95],
    }
}

fn timeline_summary_text(snapshot: &LiveAppTimelineWindowSnapshot) -> String {
    if let Some(message) = &snapshot.empty_message {
        return format!(
            "published_items: {}\n{}",
            snapshot.published_item_count, message
        );
    }

    format!(
        "published_items: {}\nvisible_range_femtoseconds: {}..{}\nvisible_rows: {}\nvisible_render_items: {}\nfolded_event_clusters: {}\nfolded_span_clusters: {}\ncontrols: <-  ->  +  -  Fit",
        snapshot.published_item_count,
        snapshot.visible_start_femtoseconds,
        snapshot.visible_end_femtoseconds,
        snapshot.visible_row_count,
        snapshot.visible_render_item_count,
        snapshot.folded_event_cluster_count,
        snapshot.folded_span_cluster_count,
    )
}

fn timeline_recent_events_text(snapshot: &LiveAppTimelineWindowSnapshot) -> String {
    if snapshot.recent_events.is_empty() {
        return String::from("Recent published events\nNo recent events recorded yet.");
    }

    let mut output = String::from("Recent published events\n");
    for event in &snapshot.recent_events {
        output.push_str("- ");
        output.push_str(event);
        output.push('\n');
    }
    output
}

fn preserved_visible_range(
    prior_visible_range: Option<CanonicalTimeRange>,
    content_bounds: Option<CanonicalTimeRange>,
) -> Option<CanonicalTimeRange> {
    match (prior_visible_range, content_bounds) {
        (_, None) => None,
        (Some(visible_range), Some(_)) => Some(visible_range),
        (None, Some(content_bounds)) => Some(fit_visible_range(content_bounds)),
    }
}

fn fit_visible_range(content_bounds: CanonicalTimeRange) -> CanonicalTimeRange {
    let duration = content_bounds
        .duration()
        .raw_value()
        .max(TIMELINE_MIN_DURATION_FEMTOSECONDS);
    let padding = (duration / 8).max(TIMELINE_MIN_DURATION_FEMTOSECONDS);
    CanonicalTimeRange::from_unordered(
        CanonicalTimeKey::from_femtoseconds(content_bounds.start().raw_femtoseconds() - padding),
        CanonicalTimeKey::from_femtoseconds(content_bounds.end().raw_femtoseconds() + padding),
    )
}

impl LiveAppTimelineWindowModel {
    fn pan(&mut self, direction: i128) {
        let Some(visible_range) = self.visible_range else {
            return;
        };
        let duration = visible_range
            .duration()
            .raw_value()
            .max(TIMELINE_MIN_DURATION_FEMTOSECONDS);
        let offset = direction
            .saturating_mul((duration * TIMELINE_PAN_NUMERATOR / TIMELINE_PAN_DENOMINATOR).max(1));
        self.visible_range = Some(CanonicalTimeRange::from_unordered(
            CanonicalTimeKey::from_femtoseconds(visible_range.start().raw_femtoseconds() + offset),
            CanonicalTimeKey::from_femtoseconds(visible_range.end().raw_femtoseconds() + offset),
        ));
    }

    fn zoom(&mut self, numerator: i128, denominator: i128) {
        let Some(visible_range) = self.visible_range else {
            return;
        };
        let duration = visible_range
            .duration()
            .raw_value()
            .max(TIMELINE_MIN_DURATION_FEMTOSECONDS);
        let center =
            (visible_range.start().raw_femtoseconds() + visible_range.end().raw_femtoseconds()) / 2;
        let new_duration =
            ((duration * numerator) / denominator).max(TIMELINE_MIN_DURATION_FEMTOSECONDS);
        let half = new_duration / 2;
        self.visible_range = Some(CanonicalTimeRange::from_unordered(
            CanonicalTimeKey::from_femtoseconds(center - half),
            CanonicalTimeKey::from_femtoseconds(center + half),
        ));
    }

    fn visible_range_or_fit(&self) -> Option<CanonicalTimeRange> {
        self.visible_range
            .or_else(|| self.content_bounds.map(fit_visible_range))
    }

    fn begin_pan_drag(&mut self, origin: POINT) {
        let Some(visible_range) = self.visible_range_or_fit() else {
            return;
        };
        self.zoom_animation = None;
        self.visible_range = Some(visible_range);
        self.pan_drag = Some(LiveAppTimelinePanDrag {
            origin,
            origin_visible_range: visible_range,
        });
    }

    fn apply_pan_drag(&mut self, lane_rect: RECT, point: POINT) -> bool {
        let Some(pan_drag) = self.pan_drag else {
            return false;
        };
        let lane_width = i128::from((lane_rect.right - lane_rect.left).max(1));
        let duration = pan_drag
            .origin_visible_range
            .duration()
            .raw_value()
            .max(TIMELINE_MIN_DURATION_FEMTOSECONDS);
        let delta_pixels = i128::from(point.x - pan_drag.origin.x);
        let time_delta = delta_pixels.saturating_mul(duration).div_euclid(lane_width);
        self.visible_range = Some(CanonicalTimeRange::from_unordered(
            CanonicalTimeKey::from_femtoseconds(
                pan_drag.origin_visible_range.start().raw_femtoseconds() - time_delta,
            ),
            CanonicalTimeKey::from_femtoseconds(
                pan_drag.origin_visible_range.end().raw_femtoseconds() - time_delta,
            ),
        ));
        true
    }

    fn zoom_about_point(
        &mut self,
        lane_rect: RECT,
        point: POINT,
        numerator: i128,
        denominator: i128,
    ) {
        let Some(base_range) = self.zoom_animation.map_or_else(
            || self.visible_range_or_fit(),
            |animation| Some(animation.target_visible_range),
        ) else {
            return;
        };
        let target = zoom_range_about_point(base_range, lane_rect, point, numerator, denominator);
        self.pan_drag = None;
        self.zoom_animation = Some(LiveAppTimelineZoomAnimation {
            start_visible_range: self.visible_range.unwrap_or(base_range),
            target_visible_range: target,
            step: 0,
        });
    }

    fn advance_zoom_animation(&mut self) -> bool {
        let Some(mut animation) = self.zoom_animation else {
            return false;
        };
        animation.step = animation.step.saturating_add(1);
        let step = i128::from(animation.step.min(TIMELINE_ZOOM_ANIMATION_STEPS));
        let total = i128::from(TIMELINE_ZOOM_ANIMATION_STEPS);
        self.visible_range = Some(lerp_time_range(
            animation.start_visible_range,
            animation.target_visible_range,
            step,
            total,
        ));
        if animation.step >= TIMELINE_ZOOM_ANIMATION_STEPS {
            self.zoom_animation = None;
        } else {
            self.zoom_animation = Some(animation);
        }
        true
    }

    fn fit_to_content(&mut self) {
        if let Some(content_bounds) = self.content_bounds {
            self.visible_range = Some(fit_visible_range(content_bounds));
        }
    }
}

fn live_app_timeline_model_is_panning() -> bool {
    live_app_timeline_window_model()
        .lock()
        .expect("live app timeline model should not be poisoned")
        .pan_drag
        .is_some()
}

fn zoom_range_about_point(
    visible_range: CanonicalTimeRange,
    lane_rect: RECT,
    point: POINT,
    numerator: i128,
    denominator: i128,
) -> CanonicalTimeRange {
    let lane_width = i128::from((lane_rect.right - lane_rect.left).max(1));
    let clamped_x = i128::from(point.x.clamp(lane_rect.left, lane_rect.right));
    let anchor_pixels = clamped_x - i128::from(lane_rect.left);
    let duration = visible_range
        .duration()
        .raw_value()
        .max(TIMELINE_MIN_DURATION_FEMTOSECONDS);
    let anchor_time = visible_range.start().raw_femtoseconds().saturating_add(
        anchor_pixels
            .saturating_mul(duration)
            .div_euclid(lane_width),
    );
    let new_duration =
        ((duration * numerator) / denominator).max(TIMELINE_MIN_DURATION_FEMTOSECONDS);
    let left_duration = new_duration
        .saturating_mul(anchor_pixels)
        .div_euclid(lane_width);
    CanonicalTimeRange::from_unordered(
        CanonicalTimeKey::from_femtoseconds(anchor_time - left_duration),
        CanonicalTimeKey::from_femtoseconds(anchor_time + new_duration - left_duration),
    )
}

fn lerp_time_range(
    start: CanonicalTimeRange,
    end: CanonicalTimeRange,
    step: i128,
    total: i128,
) -> CanonicalTimeRange {
    let lerp = |from: i128, to: i128| from + (to - from).saturating_mul(step).div_euclid(total);
    CanonicalTimeRange::from_unordered(
        CanonicalTimeKey::from_femtoseconds(lerp(
            start.start().raw_femtoseconds(),
            end.start().raw_femtoseconds(),
        )),
        CanonicalTimeKey::from_femtoseconds(lerp(
            start.end().raw_femtoseconds(),
            end.end().raw_femtoseconds(),
        )),
    )
}

fn rect_contains(rect: RECT, point: POINT) -> bool {
    point.x >= rect.left && point.x < rect.right && point.y >= rect.top && point.y < rect.bottom
}

fn inset_rect(rect: RECT, inset: i32) -> RECT {
    RECT {
        left: rect.left + inset,
        top: rect.top + inset,
        right: rect.right - inset,
        bottom: rect.bottom - inset,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::{
        LiveAppTimelineControlAction, current_live_app_timeline_window_snapshot_for_test,
        fit_visible_range, handle_live_app_timeline_frame_tick,
        handle_live_app_timeline_left_click, handle_live_app_timeline_mouse_wheel,
        handle_live_app_timeline_right_button_down, handle_live_app_timeline_right_button_up,
        handle_live_app_timeline_right_drag, live_app_timeline_window_layout,
        reset_live_app_timeline_window_model_for_test, store_live_app_timeline_window_dataset,
    };
    use teamy_studio_shell::{
        FeatureWindowSceneContext, ShellSceneLayout, WindowChromeButtonsState,
    };
    use teamy_studio_timeline_core::{
        CanonicalTimeKey, CanonicalTimeRange, TimelineDataset, TimelineItemInput,
    };
    use windows::Win32::Foundation::POINT;

    static TIMELINE_WINDOW_TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn timeline_window_pan_and_zoom_controls_recompute_visible_range() {
        let _guard = TIMELINE_WINDOW_TEST_LOCK
            .lock()
            .expect("timeline window test lock should not be poisoned");
        reset_live_app_timeline_window_model_for_test();
        let mut dataset = TimelineDataset::new();
        dataset.push_event(
            TimelineItemInput::new("bootstrap")
                .with_source_key("teamy_studio.startup.bootstrap")
                .with_group_key("teamy_studio.startup.bootstrap"),
            CanonicalTimeKey::from_femtoseconds(0),
        );
        dataset.push_event(
            TimelineItemInput::new("ready")
                .with_source_key("teamy_studio.startup")
                .with_group_key("teamy_studio.startup"),
            CanonicalTimeKey::from_femtoseconds(200),
        );
        let _ = dataset.compact();
        store_live_app_timeline_window_dataset(dataset);

        let context = FeatureWindowSceneContext {
            title: "Timeline",
            layout: ShellSceneLayout::for_main_menu_with_dpi(1280, 820, 96),
            dpi: 96,
            chrome: WindowChromeButtonsState {
                focused: true,
                ..Default::default()
            },
            cursor_client_point: None,
        };
        let initial_snapshot = current_live_app_timeline_window_snapshot_for_test(context);
        let layout = live_app_timeline_window_layout(context);
        let zoom_in = layout
            .control_buttons
            .iter()
            .find(|button| button.action == LiveAppTimelineControlAction::ZoomIn)
            .expect("zoom-in button should exist");
        let pan_right = layout
            .control_buttons
            .iter()
            .find(|button| button.action == LiveAppTimelineControlAction::PanRight)
            .expect("pan-right button should exist");
        let fit = layout
            .control_buttons
            .iter()
            .find(|button| button.action == LiveAppTimelineControlAction::Fit)
            .expect("fit button should exist");

        assert!(handle_live_app_timeline_left_click(
            context,
            POINT {
                x: (zoom_in.rect.left + zoom_in.rect.right) / 2,
                y: (zoom_in.rect.top + zoom_in.rect.bottom) / 2,
            },
        ));
        let zoomed_snapshot = current_live_app_timeline_window_snapshot_for_test(context);
        assert!(
            zoomed_snapshot.visible_end_femtoseconds - zoomed_snapshot.visible_start_femtoseconds
                < initial_snapshot.visible_end_femtoseconds
                    - initial_snapshot.visible_start_femtoseconds
        );

        assert!(handle_live_app_timeline_left_click(
            context,
            POINT {
                x: (pan_right.rect.left + pan_right.rect.right) / 2,
                y: (pan_right.rect.top + pan_right.rect.bottom) / 2,
            },
        ));
        let panned_snapshot = current_live_app_timeline_window_snapshot_for_test(context);
        assert!(
            panned_snapshot.visible_start_femtoseconds > zoomed_snapshot.visible_start_femtoseconds
        );

        assert!(handle_live_app_timeline_left_click(
            context,
            POINT {
                x: (fit.rect.left + fit.rect.right) / 2,
                y: (fit.rect.top + fit.rect.bottom) / 2,
            },
        ));
        let fit_snapshot = current_live_app_timeline_window_snapshot_for_test(context);
        let expected_fit = fit_visible_range(
            CanonicalTimeRange::try_new(
                CanonicalTimeKey::from_femtoseconds(0),
                CanonicalTimeKey::from_femtoseconds(200),
            )
            .expect("range should build"),
        );
        assert_eq!(
            fit_snapshot.visible_start_femtoseconds,
            expected_fit.start().raw_femtoseconds()
        );
        assert_eq!(
            fit_snapshot.visible_end_femtoseconds,
            expected_fit.end().raw_femtoseconds()
        );
    }

    #[test]
    fn timeline_window_mouse_wheel_animates_zoom_about_cursor() {
        let _guard = TIMELINE_WINDOW_TEST_LOCK
            .lock()
            .expect("timeline window test lock should not be poisoned");
        reset_live_app_timeline_window_model_for_test();
        let mut dataset = TimelineDataset::new();
        dataset.push_event(
            TimelineItemInput::new("bootstrap")
                .with_source_key("teamy_studio.startup.bootstrap")
                .with_group_key("teamy_studio.startup.bootstrap"),
            CanonicalTimeKey::from_femtoseconds(0),
        );
        dataset.push_event(
            TimelineItemInput::new("ready")
                .with_source_key("teamy_studio.startup")
                .with_group_key("teamy_studio.startup"),
            CanonicalTimeKey::from_femtoseconds(400),
        );
        let _ = dataset.compact();
        store_live_app_timeline_window_dataset(dataset);

        let context = FeatureWindowSceneContext {
            title: "Timeline",
            layout: ShellSceneLayout::for_main_menu_with_dpi(1280, 820, 96),
            dpi: 96,
            chrome: WindowChromeButtonsState {
                focused: true,
                ..Default::default()
            },
            cursor_client_point: None,
        };
        let layout = live_app_timeline_window_layout(context);
        let point = POINT {
            x: layout.lanes_rect.left + (layout.lanes_rect.right - layout.lanes_rect.left) / 4,
            y: layout.lanes_rect.top + 20,
        };
        let initial_snapshot = current_live_app_timeline_window_snapshot_for_test(context);

        assert!(handle_live_app_timeline_mouse_wheel(context, point, 120));
        let pre_tick_snapshot = current_live_app_timeline_window_snapshot_for_test(context);
        assert_eq!(
            pre_tick_snapshot.visible_start_femtoseconds,
            initial_snapshot.visible_start_femtoseconds
        );

        for _ in 0..5 {
            assert!(handle_live_app_timeline_frame_tick(context));
        }
        let zoomed_snapshot = current_live_app_timeline_window_snapshot_for_test(context);

        assert!(
            zoomed_snapshot.visible_end_femtoseconds - zoomed_snapshot.visible_start_femtoseconds
                < initial_snapshot.visible_end_femtoseconds
                    - initial_snapshot.visible_start_femtoseconds
        );
        assert!(
            zoomed_snapshot.visible_start_femtoseconds
                > initial_snapshot.visible_start_femtoseconds
        );
    }

    #[test]
    fn timeline_window_right_drag_pans_visible_range() {
        let _guard = TIMELINE_WINDOW_TEST_LOCK
            .lock()
            .expect("timeline window test lock should not be poisoned");
        reset_live_app_timeline_window_model_for_test();
        let mut dataset = TimelineDataset::new();
        dataset.push_event(
            TimelineItemInput::new("bootstrap")
                .with_source_key("teamy_studio.startup.bootstrap")
                .with_group_key("teamy_studio.startup.bootstrap"),
            CanonicalTimeKey::from_femtoseconds(0),
        );
        dataset.push_event(
            TimelineItemInput::new("ready")
                .with_source_key("teamy_studio.startup")
                .with_group_key("teamy_studio.startup"),
            CanonicalTimeKey::from_femtoseconds(400),
        );
        let _ = dataset.compact();
        store_live_app_timeline_window_dataset(dataset);

        let context = FeatureWindowSceneContext {
            title: "Timeline",
            layout: ShellSceneLayout::for_main_menu_with_dpi(1280, 820, 96),
            dpi: 96,
            chrome: WindowChromeButtonsState {
                focused: true,
                ..Default::default()
            },
            cursor_client_point: None,
        };
        let layout = live_app_timeline_window_layout(context);
        let origin = POINT {
            x: layout.lanes_rect.left + 100,
            y: layout.lanes_rect.top + 20,
        };
        let dragged = POINT {
            x: origin.x + 120,
            y: origin.y,
        };
        let initial_snapshot = current_live_app_timeline_window_snapshot_for_test(context);

        assert!(handle_live_app_timeline_right_button_down(context, origin));
        assert!(handle_live_app_timeline_right_drag(context, dragged));
        assert!(handle_live_app_timeline_right_button_up(context, dragged));
        let panned_snapshot = current_live_app_timeline_window_snapshot_for_test(context);

        assert!(
            panned_snapshot.visible_start_femtoseconds
                < initial_snapshot.visible_start_femtoseconds
        );
        assert!(
            panned_snapshot.visible_end_femtoseconds < initial_snapshot.visible_end_femtoseconds
        );
    }
}

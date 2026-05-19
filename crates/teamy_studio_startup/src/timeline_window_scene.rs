use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use linkme::distributed_slice;
use sguaba::Coordinate;
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
use uom::si::f64::Length;
use uom::si::length::meter;
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
const TIMELINE_WHEEL_ZOOM_FACTOR: f64 = 0.5;
const TIMELINE_ZOOM_ANIMATION_DURATION: Duration = Duration::from_millis(120);
const TIMELINE_CAMERA_ABS_FEMTOSECONDS_LIMIT: i128 = i128::MAX / 4;

sguaba::system!(struct TimelineViewportSpace using right-handed XYZ);

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

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct LiveAppTimelineWindowSnapshot {
    pub(crate) published_item_count: usize,
    pub(crate) visible_start_femtoseconds: i128,
    pub(crate) visible_end_femtoseconds: i128,
    pub(crate) visible_start_femtoseconds_f64: f64,
    pub(crate) visible_end_femtoseconds_f64: f64,
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
    summary_rect: RECT,
    controls_rect: RECT,
    ruler_rect: RECT,
    lanes_rect: RECT,
    status_rect: RECT,
    control_buttons: [LiveAppTimelineControlButton; 5],
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct LiveAppTimelinePanDrag {
    origin_visible_start_femtoseconds: f64,
    origin_visible_end_femtoseconds: f64,
    current_visible_start_femtoseconds: f64,
    current_visible_end_femtoseconds: f64,
    origin_visible_duration_femtoseconds: f64,
    anchor_time_femtoseconds: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct TimelineViewportPoint {
    coordinate: Coordinate<TimelineViewportSpace>,
}

impl TimelineViewportPoint {
    fn new_pixels(x_pixels: f64) -> Self {
        Self {
            coordinate: Coordinate::<TimelineViewportSpace>::builder()
                .x(Length::new::<meter>(x_pixels))
                .y(Length::new::<meter>(0.0))
                .z(Length::new::<meter>(0.0))
                .build(),
        }
    }

    fn pixels(self) -> f64 {
        self.coordinate.x().get::<meter>()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct LiveAppTimelineViewport {
    visible_start_femtoseconds: f64,
    visible_end_femtoseconds: f64,
    lane_rect: RECT,
}

impl LiveAppTimelineViewport {
    fn new(
        visible_start_femtoseconds: f64,
        visible_end_femtoseconds: f64,
        lane_rect: RECT,
    ) -> Option<Self> {
        (lane_rect.right > lane_rect.left && visible_end_femtoseconds > visible_start_femtoseconds)
            .then_some(Self {
                visible_start_femtoseconds,
                visible_end_femtoseconds,
                lane_rect,
            })
    }

    fn width_pixels(self) -> f64 {
        f64::from((self.lane_rect.right - self.lane_rect.left).max(1))
    }

    fn visible_duration_femtoseconds(self) -> f64 {
        (self.visible_end_femtoseconds - self.visible_start_femtoseconds)
            .max(TIMELINE_MIN_DURATION_FEMTOSECONDS as f64)
    }

    fn duration_per_pixel_femtoseconds(self) -> f64 {
        self.visible_duration_femtoseconds() / self.width_pixels()
    }

    fn point_from_client_point(self, point: POINT) -> TimelineViewportPoint {
        TimelineViewportPoint::new_pixels(f64::from(
            (point.x - self.lane_rect.left).clamp(0, self.lane_rect.right - self.lane_rect.left),
        ))
    }

    fn x_to_time_femtoseconds(self, point: TimelineViewportPoint) -> f64 {
        self.visible_start_femtoseconds + point.pixels() * self.duration_per_pixel_femtoseconds()
    }

    fn time_to_client_x(self, femtoseconds: f64) -> i32 {
        let offset = (femtoseconds - self.visible_start_femtoseconds)
            / self.duration_per_pixel_femtoseconds();
        self.lane_rect
            .left
            .saturating_add(f64_to_i32_saturating(offset))
    }

    fn scaled_about(self, anchor: TimelineViewportPoint, factor: f64) -> (f64, f64) {
        let anchor_time = self.x_to_time_femtoseconds(anchor);
        let duration_per_pixel = self.duration_per_pixel_femtoseconds() * factor.max(0.0);
        let visible_duration = (duration_per_pixel * self.width_pixels())
            .max(TIMELINE_MIN_DURATION_FEMTOSECONDS as f64);
        let target_start = anchor_time - anchor.pixels() * duration_per_pixel;
        (target_start, target_start + visible_duration)
    }

    fn rebased_to_keep_anchor_time(
        self,
        anchor: TimelineViewportPoint,
        anchor_time_femtoseconds: f64,
        visible_duration_femtoseconds: f64,
    ) -> (f64, f64) {
        let duration_per_pixel = visible_duration_femtoseconds / self.width_pixels();
        let visible_start = anchor_time_femtoseconds - anchor.pixels() * duration_per_pixel;
        (
            visible_start,
            visible_start
                + visible_duration_femtoseconds.max(TIMELINE_MIN_DURATION_FEMTOSECONDS as f64),
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct LiveAppTimelineZoomAnimation {
    start_visible_start_femtoseconds: f64,
    start_visible_end_femtoseconds: f64,
    target_visible_start_femtoseconds: f64,
    target_visible_end_femtoseconds: f64,
    started_at: Instant,
}

impl LiveAppTimelineZoomAnimation {
    fn target_range(self) -> CanonicalTimeRange {
        CanonicalTimeRange::from_unordered(
            CanonicalTimeKey::from_femtoseconds(f64_to_i128_saturating(
                self.target_visible_start_femtoseconds,
            )),
            CanonicalTimeKey::from_femtoseconds(f64_to_i128_saturating(
                self.target_visible_end_femtoseconds,
            )),
        )
    }
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
        [0.025, 0.030, 0.040, 0.98],
        PanelEffect::SceneBody,
    );
    push_text_block(
        &mut scene,
        layout.summary_rect,
        &timeline_summary_text(&snapshot),
        scale_for_dpi(10, dpi).max(8),
        scale_for_dpi(16, dpi).max(11),
        [0.70, 0.78, 0.88, 1.0],
    );
    render_timeline_controls(&mut scene, context, layout);

    push_panel(
        &mut scene,
        layout.ruler_rect,
        [0.045, 0.052, 0.065, 0.98],
        PanelEffect::SceneBody,
    );
    render_timeline_ruler(&mut scene, layout.ruler_rect, dpi, &snapshot);

    push_panel(
        &mut scene,
        layout.lanes_rect,
        [0.030, 0.035, 0.046, 0.98],
        PanelEffect::SceneBody,
    );
    render_timeline_cursor_guide(&mut scene, context, layout);
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
        render_timeline_rows(&mut scene, context, layout.lanes_rect, dpi, &snapshot);
    }

    push_panel(
        &mut scene,
        layout.status_rect,
        [0.040, 0.046, 0.058, 0.98],
        PanelEffect::SceneBody,
    );
    push_text_block(
        &mut scene,
        RECT {
            left: layout.status_rect.left + scale_for_dpi(10, dpi),
            top: layout.status_rect.top + scale_for_dpi(6, dpi),
            right: layout.status_rect.right - scale_for_dpi(10, dpi),
            bottom: layout.status_rect.bottom - scale_for_dpi(6, dpi),
        },
        &timeline_status_text(&snapshot),
        scale_for_dpi(10, dpi).max(8),
        scale_for_dpi(16, dpi).max(11),
        [0.72, 0.80, 0.90, 1.0],
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
    let (display_start, display_end) = model.display_visible_range_f64(visible_range);
    let now = content_bounds.end().max(visible_range.end());
    let query_start = CanonicalTimeKey::from_femtoseconds(f64_to_i128_saturating(display_start));
    let query_end = CanonicalTimeKey::from_femtoseconds(
        f64_to_i128_saturating(display_end).max(
            query_start
                .raw_femtoseconds()
                .saturating_add(TIMELINE_MIN_DURATION_FEMTOSECONDS),
        ),
    );
    let plan = model.dataset.render_plan(
        &TimelineViewportQuery::try_new(query_start, query_end, now, viewport_width_pixels)
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
        visible_start_femtoseconds: query_start.raw_femtoseconds(),
        visible_end_femtoseconds: query_end.raw_femtoseconds(),
        visible_start_femtoseconds_f64: display_start,
        visible_end_femtoseconds_f64: display_end,
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
            bottom: layout.controls_rect.bottom,
        },
        "view",
        scale_for_dpi(10, dpi).max(8),
        scale_for_dpi(16, dpi).max(11),
        [0.62, 0.70, 0.80, 1.0],
    );

    for button in layout.control_buttons {
        let hovered = context
            .cursor_client_point
            .is_some_and(|point| rect_contains(button.rect, point));
        push_panel_with_data(
            scene,
            button.rect,
            if hovered {
                [0.22, 0.31, 0.42, 0.98]
            } else {
                [0.075, 0.090, 0.115, 0.98]
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
    model.zoom_about_point_with_wheel_delta(layout.lanes_rect, point, delta);
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
    model.begin_pan_drag(layout.lanes_rect, point);
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
    context: FeatureWindowSceneContext,
    lanes_rect: RECT,
    dpi: u32,
    snapshot: &LiveAppTimelineWindowSnapshot,
) {
    let label_width = scale_for_dpi(220, dpi);
    let row_gap = scale_for_dpi(2, dpi);
    let row_height = scale_for_dpi(38, dpi).max(28);
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
            left: lanes_rect.left,
            top,
            right: lanes_rect.right,
            bottom: bottom.max(top),
        };
        let label_rect = RECT {
            left: row_rect.left + scale_for_dpi(10, dpi),
            top: row_rect.top + scale_for_dpi(5, dpi),
            right: row_rect.left + label_width,
            bottom: row_rect.bottom - scale_for_dpi(5, dpi),
        };
        let lane_rect = RECT {
            left: label_rect.right + scale_for_dpi(6, dpi),
            top: row_rect.top + scale_for_dpi(5, dpi),
            right: row_rect.right - scale_for_dpi(8, dpi),
            bottom: row_rect.bottom - scale_for_dpi(5, dpi),
        };

        let row_color = timeline_row_color(index);
        push_panel(
            scene,
            row_rect,
            [
                row_color[0] * 0.18,
                row_color[1] * 0.18,
                row_color[2] * 0.18,
                0.94,
            ],
            PanelEffect::SceneBody,
        );
        push_panel(
            scene,
            lane_rect,
            [0.015, 0.018, 0.024, 0.96],
            PanelEffect::SceneBody,
        );
        push_text_block(
            scene,
            label_rect,
            &compact_row_label(&row.label, row.render_items.len()),
            scale_for_dpi(10, dpi).max(8),
            scale_for_dpi(14, dpi).max(10),
            [0.74, 0.82, 0.92, 1.0],
        );

        for render_item in &row.render_items {
            if let Some(item_rect) = project_render_item_rect(snapshot, lane_rect, render_item) {
                let hovered = context
                    .cursor_client_point
                    .is_some_and(|point| rect_contains(item_rect, point));
                push_panel(
                    scene,
                    item_rect,
                    render_item_color(render_item.kind, hovered, row_color),
                    PanelEffect::SceneButtonCard,
                );
                push_timeline_item_bevel(scene, item_rect, hovered);
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

fn render_timeline_ruler(
    scene: &mut RenderScene,
    ruler_rect: RECT,
    dpi: u32,
    snapshot: &LiveAppTimelineWindowSnapshot,
) {
    if snapshot.empty_message.is_some() {
        return;
    }
    let Some(viewport) = LiveAppTimelineViewport::new(
        snapshot.visible_start_femtoseconds_f64,
        snapshot.visible_end_femtoseconds_f64,
        ruler_rect,
    ) else {
        return;
    };
    let tick_count = 6_i32;
    for tick in 0..=tick_count {
        let tick_point = TimelineViewportPoint::new_pixels(
            viewport.width_pixels() * f64::from(tick) / f64::from(tick_count),
        );
        let x = viewport.time_to_client_x(viewport.x_to_time_femtoseconds(tick_point));
        let major_rect = RECT {
            left: x,
            top: ruler_rect.top + scale_for_dpi(7, dpi),
            right: (x + 1).min(ruler_rect.right),
            bottom: ruler_rect.bottom - scale_for_dpi(7, dpi),
        };
        push_panel(
            scene,
            major_rect,
            [0.38, 0.47, 0.58, 0.85],
            PanelEffect::SceneBody,
        );
        let time = viewport.x_to_time_femtoseconds(tick_point);
        let label_rect = RECT {
            left: (x - scale_for_dpi(54, dpi)).max(ruler_rect.left + scale_for_dpi(4, dpi)),
            top: ruler_rect.top + scale_for_dpi(6, dpi),
            right: (x + scale_for_dpi(58, dpi)).min(ruler_rect.right - scale_for_dpi(4, dpi)),
            bottom: ruler_rect.bottom - scale_for_dpi(8, dpi),
        };
        let label = if tick == 0 {
            format!("base {}", format_time_label_f64(time))
        } else {
            format!(
                "+{}",
                format_time_delta_label_f64(time, snapshot.visible_start_femtoseconds_f64)
            )
        };
        push_text_block(
            scene,
            label_rect,
            &label,
            scale_for_dpi(8, dpi).max(6),
            scale_for_dpi(13, dpi).max(9),
            [0.66, 0.74, 0.84, 1.0],
        );
        if tick < tick_count {
            for subtick in 1..4 {
                let sub_x = x
                    + ((ruler_rect.right - ruler_rect.left).max(1) * subtick)
                        .div_euclid(tick_count * 4);
                push_panel(
                    scene,
                    RECT {
                        left: sub_x,
                        top: ruler_rect.bottom - scale_for_dpi(16, dpi),
                        right: (sub_x + 1).min(ruler_rect.right),
                        bottom: ruler_rect.bottom - scale_for_dpi(7, dpi),
                    },
                    [0.22, 0.28, 0.36, 0.65],
                    PanelEffect::SceneBody,
                );
            }
        }
    }
}

fn render_timeline_cursor_guide(
    scene: &mut RenderScene,
    context: FeatureWindowSceneContext,
    layout: LiveAppTimelineWindowLayout,
) {
    let Some(point) = context.cursor_client_point else {
        return;
    };
    if !rect_contains(layout.ruler_rect, point) && !rect_contains(layout.lanes_rect, point) {
        return;
    }
    push_panel(
        scene,
        RECT {
            left: point.x,
            top: layout.ruler_rect.top,
            right: point.x + 1,
            bottom: layout.lanes_rect.bottom,
        },
        [0.82, 0.90, 1.0, 0.45],
        PanelEffect::SceneBody,
    );
}

fn push_timeline_item_bevel(scene: &mut RenderScene, rect: RECT, hovered: bool) {
    let color = if hovered {
        [0.92, 0.96, 1.0, 0.82]
    } else {
        [0.74, 0.82, 0.92, 0.34]
    };
    push_panel(
        scene,
        RECT {
            left: rect.left,
            top: rect.top,
            right: rect.right,
            bottom: (rect.top + 1).min(rect.bottom),
        },
        color,
        PanelEffect::SceneBody,
    );
    push_panel(
        scene,
        RECT {
            left: rect.left,
            top: rect.top,
            right: (rect.left + 1).min(rect.right),
            bottom: rect.bottom,
        },
        color,
        PanelEffect::SceneBody,
    );
}

fn timeline_row_color(index: usize) -> [f32; 4] {
    const PALETTE: [[f32; 4]; 8] = [
        [0.36, 0.78, 0.96, 0.96],
        [0.66, 0.82, 0.42, 0.96],
        [0.92, 0.67, 0.36, 0.96],
        [0.74, 0.58, 0.96, 0.96],
        [0.42, 0.88, 0.72, 0.96],
        [0.96, 0.56, 0.62, 0.96],
        [0.72, 0.78, 0.88, 0.96],
        [0.96, 0.86, 0.42, 0.96],
    ];
    PALETTE[index % PALETTE.len()]
}

fn compact_row_label(label: &str, item_count: usize) -> String {
    let label = label.rsplit("::").next().unwrap_or(label);
    let label = label.rsplit('.').next().unwrap_or(label);
    format!("{label}  {item_count}")
}

fn format_time_label(femtoseconds: i128) -> String {
    if femtoseconds.unsigned_abs() >= 1_000_000_000_000_000 {
        format!("{}s", femtoseconds / 1_000_000_000_000_000)
    } else if femtoseconds.unsigned_abs() >= 1_000_000_000_000 {
        format!("{}ms", femtoseconds / 1_000_000_000_000)
    } else if femtoseconds.unsigned_abs() >= 1_000_000_000 {
        format!("{}us", femtoseconds / 1_000_000_000)
    } else {
        format!("{femtoseconds}fs")
    }
}

fn format_time_label_f64(femtoseconds: f64) -> String {
    format_time_label(f64_to_i128_saturating(femtoseconds.round()))
}

fn format_time_delta_label_f64(femtoseconds: f64, base_femtoseconds: f64) -> String {
    format_time_label_f64((femtoseconds - base_femtoseconds).abs())
}

fn live_app_timeline_window_layout(
    context: FeatureWindowSceneContext,
) -> LiveAppTimelineWindowLayout {
    let dpi = context.dpi;
    let body_rect = inset_rect(context.layout.body_rect, scale_for_dpi(12, dpi));
    let top_bar_height = scale_for_dpi(38, dpi);
    let ruler_height = scale_for_dpi(38, dpi);
    let status_height = scale_for_dpi(28, dpi);
    let controls_width = scale_for_dpi(328, dpi);
    let summary_rect = RECT {
        left: body_rect.left + scale_for_dpi(10, dpi),
        top: body_rect.top + scale_for_dpi(7, dpi),
        right: body_rect.right - controls_width - scale_for_dpi(12, dpi),
        bottom: body_rect.top + top_bar_height,
    };
    let controls_rect = RECT {
        left: body_rect.right - controls_width,
        top: body_rect.top + scale_for_dpi(6, dpi),
        right: body_rect.right - scale_for_dpi(10, dpi),
        bottom: body_rect.top + top_bar_height,
    };

    let button_gap = scale_for_dpi(5, dpi);
    let button_width = scale_for_dpi(46, dpi);
    let button_height = scale_for_dpi(24, dpi);
    let buttons_top = controls_rect.top;
    let mut next_left = controls_rect.left + scale_for_dpi(36, dpi);
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

    let ruler_rect = RECT {
        left: body_rect.left + scale_for_dpi(10, dpi),
        top: summary_rect.bottom + scale_for_dpi(4, dpi),
        right: body_rect.right - scale_for_dpi(10, dpi),
        bottom: summary_rect.bottom + scale_for_dpi(4, dpi) + ruler_height,
    };
    let status_rect = RECT {
        left: ruler_rect.left,
        top: body_rect.bottom - status_height - scale_for_dpi(8, dpi),
        right: ruler_rect.right,
        bottom: body_rect.bottom - scale_for_dpi(8, dpi),
    };
    let lanes_rect = RECT {
        left: ruler_rect.left,
        top: ruler_rect.bottom + scale_for_dpi(3, dpi),
        right: ruler_rect.right,
        bottom: status_rect.top - scale_for_dpi(3, dpi),
    };

    LiveAppTimelineWindowLayout {
        body_rect,
        summary_rect,
        controls_rect,
        ruler_rect,
        lanes_rect,
        status_rect,
        control_buttons,
    }
}

fn project_render_item_rect(
    snapshot: &LiveAppTimelineWindowSnapshot,
    lane_rect: RECT,
    render_item: &LiveAppTimelineWindowRenderItem,
) -> Option<RECT> {
    let viewport = LiveAppTimelineViewport::new(
        snapshot.visible_start_femtoseconds_f64,
        snapshot.visible_end_femtoseconds_f64,
        lane_rect,
    )?;

    let start_x = viewport.time_to_client_x(render_item.start_femtoseconds as f64);
    let end_x = viewport.time_to_client_x(render_item.end_femtoseconds as f64);
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

fn render_item_color(
    kind: LiveAppTimelineWindowRenderItemKind,
    hovered: bool,
    row_color: [f32; 4],
) -> [f32; 4] {
    let color = match kind {
        LiveAppTimelineWindowRenderItemKind::Span => row_color,
        LiveAppTimelineWindowRenderItemKind::Event => [1.00, 0.74, 0.35, 0.96],
        LiveAppTimelineWindowRenderItemKind::FoldedSpanCluster => [0.56, 0.92, 0.70, 0.96],
        LiveAppTimelineWindowRenderItemKind::FoldedEventCluster => [1.00, 0.48, 0.64, 0.96],
    };
    if hovered {
        [
            (color[0] + 0.16).min(1.0),
            (color[1] + 0.16).min(1.0),
            (color[2] + 0.16).min(1.0),
            1.0,
        ]
    } else {
        color
    }
}

fn timeline_summary_text(snapshot: &LiveAppTimelineWindowSnapshot) -> String {
    if let Some(message) = &snapshot.empty_message {
        return format!("items {}  {}", snapshot.published_item_count, message);
    }

    format!(
        "items {}  range {}..{}  rows {}  visible {}  folded e/s {}/{}",
        snapshot.published_item_count,
        snapshot.visible_start_femtoseconds,
        snapshot.visible_end_femtoseconds,
        snapshot.visible_row_count,
        snapshot.visible_render_item_count,
        snapshot.folded_event_cluster_count,
        snapshot.folded_span_cluster_count,
    )
}

fn timeline_status_text(snapshot: &LiveAppTimelineWindowSnapshot) -> String {
    if snapshot.recent_events.is_empty() {
        return String::from(
            "right-drag pans  |  wheel zooms under cursor  |  hover tracks cursor position",
        );
    }

    format!(
        "latest: {}  |  right-drag pans  |  wheel zooms under cursor",
        snapshot
            .recent_events
            .first()
            .map_or("none", String::as_str)
    )
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

    fn begin_pan_drag(&mut self, lane_rect: RECT, origin: POINT) {
        let Some(visible_range) = self.visible_range_or_fit() else {
            return;
        };
        let (origin_visible_start_femtoseconds, origin_visible_end_femtoseconds) =
            self.display_visible_range_f64(visible_range);
        let Some(viewport) = LiveAppTimelineViewport::new(
            origin_visible_start_femtoseconds,
            origin_visible_end_femtoseconds,
            lane_rect,
        ) else {
            return;
        };
        self.zoom_animation = None;
        self.visible_range = Some(canonical_time_range_from_f64(
            origin_visible_start_femtoseconds,
            origin_visible_end_femtoseconds,
        ));
        let origin_visible_duration_femtoseconds = viewport.visible_duration_femtoseconds();
        self.pan_drag = Some(LiveAppTimelinePanDrag {
            origin_visible_start_femtoseconds,
            origin_visible_end_femtoseconds,
            current_visible_start_femtoseconds: origin_visible_start_femtoseconds,
            current_visible_end_femtoseconds: origin_visible_end_femtoseconds,
            origin_visible_duration_femtoseconds,
            anchor_time_femtoseconds: viewport
                .x_to_time_femtoseconds(viewport.point_from_client_point(origin)),
        });
    }

    fn apply_pan_drag(&mut self, lane_rect: RECT, point: POINT) -> bool {
        let Some(pan_drag) = self.pan_drag else {
            return false;
        };
        let Some(viewport) = LiveAppTimelineViewport::new(
            pan_drag.current_visible_start_femtoseconds,
            pan_drag.current_visible_end_femtoseconds,
            lane_rect,
        ) else {
            return false;
        };
        let (visible_start, visible_end) = viewport.rebased_to_keep_anchor_time(
            viewport.point_from_client_point(point),
            pan_drag.anchor_time_femtoseconds,
            pan_drag.origin_visible_duration_femtoseconds,
        );
        if let Some(pan_drag) = self.pan_drag.as_mut() {
            pan_drag.current_visible_start_femtoseconds = visible_start;
            pan_drag.current_visible_end_femtoseconds = visible_end;
        }
        self.visible_range = Some(canonical_time_range_from_f64(visible_start, visible_end));
        true
    }

    fn zoom_about_point_with_wheel_delta(&mut self, lane_rect: RECT, point: POINT, delta: i16) {
        let factor = zoom_factor_from_wheel_delta(delta);
        self.zoom_about_point_with_factor(lane_rect, point, factor);
    }

    fn zoom_about_point_with_factor(&mut self, lane_rect: RECT, point: POINT, factor: f64) {
        let Some(base_range) = self.zoom_animation.map_or_else(
            || self.visible_range_or_fit(),
            |animation| Some(animation.target_range()),
        ) else {
            return;
        };
        let (start_visible_start_femtoseconds, start_visible_end_femtoseconds) =
            self.display_visible_range_f64(base_range);
        let Some(base_viewport) = LiveAppTimelineViewport::new(
            base_range.start().raw_femtoseconds() as f64,
            base_range.end().raw_femtoseconds() as f64,
            lane_rect,
        ) else {
            return;
        };
        let (target_start, target_end) =
            base_viewport.scaled_about(base_viewport.point_from_client_point(point), factor);
        let target = canonical_time_range_from_f64(target_start, target_end);
        if let Some(pan_drag) = self.pan_drag.as_mut() {
            self.zoom_animation = None;
            self.visible_range = Some(target);
            pan_drag.origin_visible_start_femtoseconds = target.start().raw_femtoseconds() as f64;
            pan_drag.origin_visible_end_femtoseconds = target.end().raw_femtoseconds() as f64;
            pan_drag.current_visible_start_femtoseconds =
                pan_drag.origin_visible_start_femtoseconds;
            pan_drag.current_visible_end_femtoseconds = pan_drag.origin_visible_end_femtoseconds;
            let Some(target_viewport) = LiveAppTimelineViewport::new(
                pan_drag.origin_visible_start_femtoseconds,
                pan_drag.origin_visible_end_femtoseconds,
                lane_rect,
            ) else {
                return;
            };
            pan_drag.origin_visible_duration_femtoseconds =
                target_viewport.visible_duration_femtoseconds();
            pan_drag.anchor_time_femtoseconds = target_viewport
                .x_to_time_femtoseconds(target_viewport.point_from_client_point(point));
            return;
        }
        self.visible_range = Some(canonical_time_range_from_f64(
            start_visible_start_femtoseconds,
            start_visible_end_femtoseconds,
        ));
        self.zoom_animation = Some(LiveAppTimelineZoomAnimation {
            start_visible_start_femtoseconds,
            start_visible_end_femtoseconds,
            target_visible_start_femtoseconds: target.start().raw_femtoseconds() as f64,
            target_visible_end_femtoseconds: target.end().raw_femtoseconds() as f64,
            started_at: Instant::now(),
        });
    }

    fn advance_zoom_animation(&mut self) -> bool {
        let Some(animation) = self.zoom_animation else {
            return false;
        };
        let progress = zoom_animation_progress_f64(animation.started_at);
        let start = lerp_f64(
            animation.start_visible_start_femtoseconds,
            animation.target_visible_start_femtoseconds,
            progress,
        );
        let end = lerp_f64(
            animation.start_visible_end_femtoseconds,
            animation.target_visible_end_femtoseconds,
            progress,
        )
        .max(start + TIMELINE_MIN_DURATION_FEMTOSECONDS as f64);
        self.visible_range = Some(CanonicalTimeRange::from_unordered(
            CanonicalTimeKey::from_femtoseconds(f64_to_i128_saturating(start)),
            CanonicalTimeKey::from_femtoseconds(f64_to_i128_saturating(end)),
        ));
        if progress >= 1.0 {
            self.zoom_animation = None;
        }
        true
    }

    fn fit_to_content(&mut self) {
        if let Some(content_bounds) = self.content_bounds {
            self.visible_range = Some(fit_visible_range(content_bounds));
            self.zoom_animation = None;
        }
    }

    fn display_visible_range_f64(&self, fallback: CanonicalTimeRange) -> (f64, f64) {
        if let Some(pan_drag) = self.pan_drag {
            return (
                pan_drag.current_visible_start_femtoseconds,
                pan_drag.current_visible_end_femtoseconds,
            );
        }
        if let Some(animation) = self.zoom_animation {
            let progress = zoom_animation_progress_f64(animation.started_at);
            let start = lerp_f64(
                animation.start_visible_start_femtoseconds,
                animation.target_visible_start_femtoseconds,
                progress,
            );
            let end = lerp_f64(
                animation.start_visible_end_femtoseconds,
                animation.target_visible_end_femtoseconds,
                progress,
            )
            .max(start + TIMELINE_MIN_DURATION_FEMTOSECONDS as f64);
            return (start, end);
        }
        (
            fallback.start().raw_femtoseconds() as f64,
            fallback.end().raw_femtoseconds() as f64,
        )
    }
}

#[cfg(test)]
const ZOOM_ANIMATION_PROGRESS_SCALE: u16 = 1024;

#[cfg(test)]
fn zoom_animation_progress_for_progress(progress: f64) -> u16 {
    let scaled =
        zoom_animation_progress_from_progress(progress) * f64::from(ZOOM_ANIMATION_PROGRESS_SCALE);
    f64_to_u16_saturating(scaled.round()).min(ZOOM_ANIMATION_PROGRESS_SCALE)
}

fn zoom_animation_progress_f64(started_at: Instant) -> f64 {
    let progress = (started_at.elapsed().as_secs_f64()
        / TIMELINE_ZOOM_ANIMATION_DURATION.as_secs_f64())
    .clamp(0.0, 1.0);
    zoom_animation_progress_from_progress(progress)
}

fn zoom_animation_progress_from_progress(progress: f64) -> f64 {
    if progress >= 1.0 {
        return 1.0;
    }
    let progress = progress.max(0.0);
    1.0 - (1.0 - progress).powi(3)
}

fn live_app_timeline_model_is_panning() -> bool {
    live_app_timeline_window_model()
        .lock()
        .expect("live app timeline model should not be poisoned")
        .pan_drag
        .is_some()
}

fn zoom_factor_from_wheel_delta(delta: i16) -> f64 {
    let steps = if delta.unsigned_abs() < 120 {
        i32::from(delta.signum())
    } else {
        i32::from(delta / 120)
    };
    if steps > 0 {
        TIMELINE_WHEEL_ZOOM_FACTOR.powi(steps)
    } else {
        (1.0 / TIMELINE_WHEEL_ZOOM_FACTOR).powi(-steps)
    }
}

fn canonical_time_range_from_f64(start: f64, end: f64) -> CanonicalTimeRange {
    let limit = TIMELINE_CAMERA_ABS_FEMTOSECONDS_LIMIT;
    let min_start = -limit;
    let max_end = limit;
    let max_duration = max_end - min_start;
    let unclamped_start = f64_to_i128_saturating(start);
    let unclamped_end = f64_to_i128_saturating(end);
    let unclamped_duration = unclamped_end
        .saturating_sub(unclamped_start)
        .max(TIMELINE_MIN_DURATION_FEMTOSECONDS);
    let duration = unclamped_duration.min(max_duration);
    let latest_start = max_end.saturating_sub(duration);
    let clamped_start = unclamped_start.clamp(min_start, latest_start);
    let clamped_end = clamped_start.saturating_add(duration).clamp(
        clamped_start.saturating_add(TIMELINE_MIN_DURATION_FEMTOSECONDS),
        max_end,
    );
    CanonicalTimeRange::from_unordered(
        CanonicalTimeKey::from_femtoseconds(clamped_start),
        CanonicalTimeKey::from_femtoseconds(clamped_end),
    )
}

fn lerp_f64(start: f64, end: f64, progress: f64) -> f64 {
    start + (end - start) * progress.clamp(0.0, 1.0)
}

fn f64_to_i128_saturating(value: f64) -> i128 {
    if value.is_nan() {
        return 0;
    }
    if value >= i128::MAX as f64 {
        return i128::MAX;
    }
    if value <= i128::MIN as f64 {
        return i128::MIN;
    }
    value as i128
}

fn f64_to_i32_saturating(value: f64) -> i32 {
    if value.is_nan() {
        return 0;
    }
    if value >= f64::from(i32::MAX) {
        return i32::MAX;
    }
    if value <= f64::from(i32::MIN) {
        return i32::MIN;
    }
    value.round() as i32
}

#[cfg(test)]
fn f64_to_u16_saturating(value: f64) -> u16 {
    if value.is_nan() || value <= 0.0 {
        return 0;
    }
    if value >= f64::from(u16::MAX) {
        return u16::MAX;
    }
    value as u16
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
    use std::time::Instant;

    use super::{
        LiveAppTimelineControlAction, current_live_app_timeline_window_snapshot_for_test,
        fit_visible_range, handle_live_app_timeline_frame_tick,
        handle_live_app_timeline_left_click, handle_live_app_timeline_mouse_wheel,
        handle_live_app_timeline_right_button_down, handle_live_app_timeline_right_button_up,
        handle_live_app_timeline_right_drag, live_app_timeline_window_layout,
        reset_live_app_timeline_window_model_for_test, store_live_app_timeline_window_dataset,
        timeline_status_text,
    };
    use teamy_studio_shell::{
        FeatureWindowSceneContext, ShellSceneLayout, WindowChromeButtonsState,
    };
    use teamy_studio_timeline_core::{
        CanonicalTimeKey, CanonicalTimeRange, TimelineDataset, TimelineItemInput,
    };
    use windows::Win32::Foundation::{POINT, RECT};

    static TIMELINE_WINDOW_TEST_LOCK: Mutex<()> = Mutex::new(());

    fn finish_zoom_animation_for_test() {
        let mut model = super::live_app_timeline_window_model()
            .lock()
            .expect("live app timeline model should not be poisoned");
        if let Some(animation) = model.zoom_animation.as_mut() {
            animation.started_at = Instant::now() - super::TIMELINE_ZOOM_ANIMATION_DURATION;
        }
    }

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
        assert!(
            (pre_tick_snapshot.visible_start_femtoseconds
                - initial_snapshot.visible_start_femtoseconds)
                .abs()
                <= 1
        );

        finish_zoom_animation_for_test();
        let _ = handle_live_app_timeline_frame_tick(context);
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

    #[test]
    fn timeline_window_wheel_zoom_compounds_multi_notch_input() {
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
            CanonicalTimeKey::from_femtoseconds(4_000),
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
            x: layout.lanes_rect.left + (layout.lanes_rect.right - layout.lanes_rect.left) / 2,
            y: layout.lanes_rect.top + 20,
        };

        let initial_snapshot = current_live_app_timeline_window_snapshot_for_test(context);
        assert!(handle_live_app_timeline_mouse_wheel(context, point, 360));
        finish_zoom_animation_for_test();
        let _ = handle_live_app_timeline_frame_tick(context);
        let zoomed_snapshot = current_live_app_timeline_window_snapshot_for_test(context);

        assert!(
            zoomed_snapshot.visible_end_femtoseconds - zoomed_snapshot.visible_start_femtoseconds
                <= (initial_snapshot.visible_end_femtoseconds
                    - initial_snapshot.visible_start_femtoseconds)
                    / 8
        );
    }

    #[test]
    fn timeline_window_wheel_zoom_rebases_active_pan_drag() {
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
            CanonicalTimeKey::from_femtoseconds(4_000),
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
            x: layout.lanes_rect.left + 180,
            y: layout.lanes_rect.top + 20,
        };
        let dragged = POINT {
            x: origin.x + 120,
            y: origin.y,
        };
        let initial_snapshot = current_live_app_timeline_window_snapshot_for_test(context);

        assert!(handle_live_app_timeline_right_button_down(context, origin));
        assert!(handle_live_app_timeline_mouse_wheel(context, origin, 120));
        let zoomed_snapshot = current_live_app_timeline_window_snapshot_for_test(context);
        assert!(
            zoomed_snapshot.visible_end_femtoseconds - zoomed_snapshot.visible_start_femtoseconds
                < initial_snapshot.visible_end_femtoseconds
                    - initial_snapshot.visible_start_femtoseconds
        );
        assert!(
            super::live_app_timeline_window_model()
                .lock()
                .expect("live app timeline model should not be poisoned")
                .pan_drag
                .is_some()
        );

        assert!(handle_live_app_timeline_right_drag(context, dragged));
        let panned_snapshot = current_live_app_timeline_window_snapshot_for_test(context);
        assert!(
            panned_snapshot.visible_start_femtoseconds < zoomed_snapshot.visible_start_femtoseconds
        );
    }

    #[test]
    fn timeline_window_pan_drag_keeps_grabbed_time_under_cursor() {
        let _guard = TIMELINE_WINDOW_TEST_LOCK
            .lock()
            .expect("timeline window test lock should not be poisoned");
        reset_live_app_timeline_window_model_for_test();
        let mut model = super::LiveAppTimelineWindowModel {
            visible_range: Some(
                CanonicalTimeRange::try_new(
                    CanonicalTimeKey::from_femtoseconds(100),
                    CanonicalTimeKey::from_femtoseconds(500),
                )
                .expect("test range should be valid"),
            ),
            ..Default::default()
        };
        let lane_rect = RECT {
            left: 20,
            top: 0,
            right: 220,
            bottom: 32,
        };
        let origin = POINT { x: 70, y: 10 };
        let moved = POINT { x: 150, y: 10 };
        model.begin_pan_drag(lane_rect, origin);
        let anchor_time = model
            .pan_drag
            .expect("pan drag should start")
            .anchor_time_femtoseconds;

        assert!(model.apply_pan_drag(lane_rect, moved));
        let visible_range = model
            .visible_range
            .expect("pan should keep a visible range");
        let viewport = super::LiveAppTimelineViewport::new(
            visible_range.start().raw_femtoseconds() as f64,
            visible_range.end().raw_femtoseconds() as f64,
            lane_rect,
        )
        .expect("test viewport should be valid");
        let projected_anchor_time =
            viewport.x_to_time_femtoseconds(viewport.point_from_client_point(moved));

        assert!((projected_anchor_time - anchor_time).abs() <= 1.0);
    }

    #[test]
    fn timeline_window_canonical_range_from_f64_clamps_extreme_zoom_out() {
        let range = super::canonical_time_range_from_f64(f64::NEG_INFINITY, f64::INFINITY);
        let duration = range.duration().raw_value();

        assert!(duration >= super::TIMELINE_MIN_DURATION_FEMTOSECONDS);
        assert!(
            range.start().raw_femtoseconds().abs() <= super::TIMELINE_CAMERA_ABS_FEMTOSECONDS_LIMIT
        );
        assert!(
            range.end().raw_femtoseconds().abs() <= super::TIMELINE_CAMERA_ABS_FEMTOSECONDS_LIMIT
        );
    }

    #[test]
    fn timeline_window_layout_prioritizes_ruler_and_lanes() {
        let _guard = TIMELINE_WINDOW_TEST_LOCK
            .lock()
            .expect("timeline window test lock should not be poisoned");
        reset_live_app_timeline_window_model_for_test();
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

        assert!(layout.ruler_rect.top > layout.summary_rect.top);
        assert!(layout.lanes_rect.top > layout.ruler_rect.bottom);
        assert!(layout.status_rect.top > layout.lanes_rect.top);
        assert!(
            layout.lanes_rect.bottom - layout.lanes_rect.top
                > layout.summary_rect.bottom - layout.summary_rect.top
        );
        assert!(
            timeline_status_text(&Default::default()).contains("right-drag pans"),
            "status strip should teach the tracing-style direct manipulation affordance"
        );
    }

    #[test]
    fn timeline_window_zoom_progress_uses_eased_substeps() {
        assert_eq!(super::zoom_animation_progress_for_progress(0.0), 0);
        assert_eq!(super::zoom_animation_progress_for_progress(1.0), 1024);
        assert!(super::zoom_animation_progress_for_progress(0.25) > 256);
        assert!(super::zoom_animation_progress_for_progress(0.5) > 512);
    }

    #[test]
    fn timeline_window_projection_preserves_fractional_viewports() {
        let lane_rect = RECT {
            left: 10,
            top: 0,
            right: 110,
            bottom: 24,
        };
        let viewport = super::LiveAppTimelineViewport::new(25.0, 125.0, lane_rect)
            .expect("test viewport should be valid");

        assert_eq!(viewport.time_to_client_x(50.0), 35);
    }
}

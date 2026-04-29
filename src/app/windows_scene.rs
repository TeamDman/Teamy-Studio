use std::fmt::Write as _;
use std::ops::Range;
use std::time::{Duration, Instant};

use arbitrary::Arbitrary;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Direction, Layout, Rect as RatatuiRect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols;
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Axis, Block, Borders, Chart, Dataset, GraphType, List, ListItem, Paragraph, Widget, Wrap,
};
use uom::si::f64::Time;
use uom::si::time::second;
use windows::Win32::Foundation::RECT;

use crate::model::KNOWN_WHISPER_MODELS;
use crate::timeline::{
    TimelineDocument, TimelineRulerTick, TimelineTimeNs, TimelineTimeRangeNs, TimelineTrack,
    TimelineTrackId, TimelineTrackKind, TimelineViewport, TimelineViewportPoint,
};

use super::AudioTranscriptionDaemonStatusReport;
use super::cell_grid;
use super::spatial::{ClientPoint, ClientRect, TerminalCellPoint};
use super::windows_audio_input::{
    AudioInputDeviceSummary, AudioInputDeviceWindowState, AudioInputTimelineHeadKind,
};
use super::windows_d3d12_renderer::{
    ButtonVisualState, PanelEffect, RenderScene, SpriteId, WindowChromeButtonsState,
    preferred_background_color, preferred_title_bar_color, push_centered_text, push_glyph,
    push_panel, push_panel_with_data, push_sprite, push_text_block, push_title_text,
    push_window_chrome_buttons, push_window_garden_frame,
};
use super::windows_terminal::{TerminalLayout, TerminalSelection};

pub const DEFAULT_MAX_BUTTON_SIZE: i32 = 300;
pub const AUDIO_INPUT_DEVICE_DETAIL_TEXT_CELL_WIDTH: i32 = 10;
pub const AUDIO_INPUT_DEVICE_DETAIL_TEXT_CELL_HEIGHT: i32 = 18;
const MIN_BUTTON_GAP: i32 = 12;
const MAX_BUTTON_GAP: i32 = 48;
const MIN_BUTTON_LABEL_GAP: i32 = 8;
const MAX_BUTTON_LABEL_GAP: i32 = 18;
const MIN_BUTTON_LABEL_HEIGHT: i32 = 20;
const MAX_BUTTON_LABEL_HEIGHT: i32 = 42;
const MIN_BUTTON_SPRITE_INSET: i32 = 12;
const MAX_BUTTON_SPRITE_INSET: i32 = 24;
const BUTTON_PROXIMITY_RADIUS_PX: f64 = 96.0;
const CLICK_DECAY_DURATION: Duration = Duration::from_millis(220);
const DIAGNOSTIC_TEXT_COLOR: [f32; 4] = [0.92, 0.94, 0.96, 1.0];
const DIAGNOSTIC_SELECTION_FOREGROUND: [f32; 4] = [0.04, 0.05, 0.06, 1.0];
const DIAGNOSTIC_SELECTION_BACKGROUND: [f32; 4] = [0.42, 0.67, 0.98, 1.0];
const TIMELINE_TRACK_ROW_HEIGHT: i32 = 72;
const TIMELINE_TRACK_ROW_GAP: i32 = 10;
const TIMELINE_SCROLLBAR_THICKNESS: i32 = 12;
const TIMELINE_SCROLLBAR_MIN_THUMB: i32 = 24;
const TIMELINE_SELECTION_FILL: [f32; 4] = [0.42, 0.67, 0.98, 0.18];
const TIMELINE_SELECTION_BORDER: [f32; 4] = [0.76, 0.86, 0.98, 0.92];
const TIMELINE_SELECTION_INTERSECTION: [f32; 4] = [0.95, 0.86, 0.30, 0.36];
const TIMELINE_SCROLLBAR_TRACK: [f32; 4] = [0.16, 0.18, 0.22, 0.92];
const TIMELINE_SCROLLBAR_THUMB: [f32; 4] = [0.48, 0.56, 0.66, 0.94];
const TIMELINE_TRANSCRIPTION_MAX_CHUNK_SECONDS: f64 = 30.0;
const TIMELINE_TRANSCRIPTION_INACTIVITY_SECONDS: f64 = 3.0;
const TIMELINE_TRANSCRIPTION_SETTINGS_TARGET_COLUMN_WIDTH: i32 = 92;
const TIMELINE_TRANSCRIPTION_SETTINGS_TARGET_SOCKET_SIZE: i32 = 40;
const TIMELINE_TRANSCRIPTION_SETTINGS_TARGET_PUCK_SIZE: i32 = 30;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SceneWindowKind {
    Launcher,
    AudioPicker,
    AudioDaemon,
    AudioInputDevicePicker,
    AudioInputDeviceDetails,
    CursorGallery,
    DemoMode,
    TimelineStart,
    TimelineAddTrack,
    Timeline,
    TimelineTranscriptionSettings,
}

impl SceneWindowKind {
    #[must_use]
    pub const fn title(self) -> &'static str {
        match self {
            Self::Launcher => "Teamy Studio",
            Self::AudioPicker => "Audio Sources",
            Self::AudioDaemon => "Audio Daemon",
            Self::AudioInputDevicePicker => "Audio Devices",
            Self::AudioInputDeviceDetails => "Microphone",
            Self::CursorGallery => "Cursor Gallery",
            Self::DemoMode => "Demo Mode",
            Self::TimelineStart | Self::Timeline => "Timeline",
            Self::TimelineAddTrack => "Add Track",
            Self::TimelineTranscriptionSettings => "Transcription Track",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TimelineTranscriptionSettingsColumn {
    #[default]
    Model,
    TargetTrack,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimelineTranscriptionSettingsTarget {
    Model,
    TargetTrack,
}

#[expect(
    clippy::struct_excessive_bools,
    reason = "the settings view state keeps independent hover and dock flags for the two target widgets"
)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TimelineTranscriptionSettingsViewState {
    pub track_id: TimelineTrackId,
    pub selected_column: TimelineTranscriptionSettingsColumn,
    pub selected_model_index: usize,
    pub selected_target_index: usize,
    pub model_target_docked: bool,
    pub output_target_docked: bool,
    pub dragging_target: Option<TimelineTranscriptionSettingsTarget>,
    pub drag_position: Option<ClientPoint>,
    pub hovered_model_index: Option<usize>,
    pub hovered_target_index: Option<usize>,
    pub hovered_model_socket: bool,
    pub hovered_target_socket: bool,
    pub hovered_model_target: bool,
    pub hovered_output_target: bool,
    pub hovered_add_text_track_button: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SceneAction {
    OpenTerminal,
    OpenCursorInfo,
    OpenCursorGallery,
    OpenDemoMode,
    OpenStorage,
    OpenEnvironmentVariables,
    OpenApplicationWindows,
    OpenAudioPicker,
    OpenAudioDaemon,
    OpenAudioInputDevices,
    OpenTimeline,
    OpenTimelineTrackMenu,
    CreateBlankTimeline,
    PanTimelineLeft,
    PanTimelineRight,
    ZoomTimelineIn,
    ZoomTimelineOut,
    SelectTimelineTool,
    SelectTimelineBrush,
    AppendMicrophoneTrack,
    AppendTranscriptionTrack,
    AppendTextTrack,
    CloseTimelineTrackMenu,
    ImportTimeline,
    SelectWindowsBell,
    SelectFileBell,
}

#[expect(
    clippy::struct_excessive_bools,
    reason = "timeline document visual state keeps independent hover and press flags for distinct shader-driven controls"
)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TimelineDocumentVisualState {
    pub add_track_button: ButtonVisualState,
    pub transport_play_button: ButtonVisualState,
    pub microphone_record_button: ButtonVisualState,
    pub microphone_playback_hovered: bool,
    pub microphone_playback_pressed: bool,
    pub microphone_loopback_hovered: bool,
    pub microphone_loopback_pressed: bool,
    pub transcription_toggle_hovered: bool,
    pub transcription_toggle_pressed: bool,
    pub transcription_settings_hovered: bool,
    pub transcription_settings_pressed: bool,
    pub pan_left_button: ButtonVisualState,
    pub pan_right_button: ButtonVisualState,
    pub zoom_in_button: ButtonVisualState,
    pub zoom_out_button: ButtonVisualState,
    pub select_tool_button: ButtonVisualState,
    pub brush_tool_button: ButtonVisualState,
    pub hovered_head: Option<AudioInputTimelineHeadKind>,
    pub grabbed_head: Option<AudioInputTimelineHeadKind>,
    pub vertical_scroll_offset: i32,
    pub selection: Option<TimelineRectSelection>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimelineAudioHeadGrabberLayout {
    pub kind: AudioInputTimelineHeadKind,
    pub rect: ClientRect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimelineTrackVerticalRange {
    top: i32,
    bottom: i32,
}

impl TimelineTrackVerticalRange {
    #[must_use]
    pub fn new(top: i32, bottom: i32) -> Self {
        if top <= bottom {
            Self { top, bottom }
        } else {
            Self {
                top: bottom,
                bottom: top,
            }
        }
    }

    #[must_use]
    pub const fn top(self) -> i32 {
        self.top
    }

    #[must_use]
    pub const fn bottom(self) -> i32 {
        self.bottom
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimelineRectSelection {
    time_range: TimelineTimeRangeNs,
    track_y_range: TimelineTrackVerticalRange,
}

impl TimelineRectSelection {
    #[must_use]
    pub const fn new(
        time_range: TimelineTimeRangeNs,
        track_y_range: TimelineTrackVerticalRange,
    ) -> Self {
        Self {
            time_range,
            track_y_range,
        }
    }

    #[must_use]
    pub const fn time_range(self) -> TimelineTimeRangeNs {
        self.time_range
    }

    #[must_use]
    pub const fn track_y_range(self) -> TimelineTrackVerticalRange {
        self.track_y_range
    }
}

#[expect(
    clippy::struct_field_names,
    reason = "timeline layout stores named regions used for rendering and hit testing"
)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimelineDocumentLayout {
    pub body_rect: ClientRect,
    pub track_list_rect: ClientRect,
    pub add_track_rect: ClientRect,
    pub track_text_rect: ClientRect,
    pub ruler_rect: ClientRect,
    pub transport_panel_rect: ClientRect,
    pub viewport_controls_rect: ClientRect,
    pub time_axis_rect: ClientRect,
    pub content_rect: ClientRect,
    pub scrollport_rect: ClientRect,
    pub horizontal_scrollbar_rect: ClientRect,
    pub vertical_scrollbar_rect: ClientRect,
}

#[must_use]
pub fn timeline_document_layout(body_rect: ClientRect) -> TimelineDocumentLayout {
    let track_list_width = (body_rect.width() / 4).clamp(180, 300);
    let ruler_height = 54;
    let gap = 12;
    let scrollbar_gap = 6;
    let track_list_rect = ClientRect::new(
        body_rect.left(),
        body_rect.top(),
        body_rect.left() + track_list_width,
        body_rect.bottom(),
    );
    let add_track_rect = ClientRect::new(
        track_list_rect.left() + 14,
        track_list_rect.top() + 14,
        track_list_rect.right() - 14,
        track_list_rect.top() + 58,
    );
    let track_text_rect = ClientRect::new(
        track_list_rect.left() + 14,
        add_track_rect.bottom() + 14,
        track_list_rect.right() - 14,
        track_list_rect.bottom() - 14,
    );
    let ruler_rect = ClientRect::new(
        track_list_rect.right() + gap,
        body_rect.top(),
        body_rect.right(),
        body_rect.top() + ruler_height,
    );
    let transport_panel_rect = ClientRect::new(
        ruler_rect.left() + 6,
        ruler_rect.top() + 8,
        ruler_rect.left() + 96,
        ruler_rect.bottom() - 8,
    );
    let viewport_controls_rect = ClientRect::new(
        ruler_rect.right() - 304,
        ruler_rect.top() + 8,
        ruler_rect.right() - 12,
        ruler_rect.bottom() - 8,
    );
    let time_axis_rect = ClientRect::new(
        transport_panel_rect.right() + 10,
        ruler_rect.top(),
        viewport_controls_rect.left() - 10,
        ruler_rect.bottom(),
    );
    let content_rect = ClientRect::new(
        ruler_rect.left(),
        ruler_rect.bottom() + gap,
        body_rect.right(),
        body_rect.bottom(),
    );
    let scrollport_rect = ClientRect::new(
        time_axis_rect.left(),
        content_rect.top() + 6,
        time_axis_rect.right(),
        content_rect.bottom() - TIMELINE_SCROLLBAR_THICKNESS - scrollbar_gap,
    );
    let horizontal_scrollbar_rect = ClientRect::new(
        scrollport_rect.left(),
        scrollport_rect.bottom() + scrollbar_gap,
        scrollport_rect.right(),
        content_rect.bottom() - 6,
    );
    let vertical_scrollbar_rect = ClientRect::new(
        scrollport_rect.right() + scrollbar_gap,
        scrollport_rect.top(),
        content_rect.right() - 6,
        scrollport_rect.bottom(),
    );

    TimelineDocumentLayout {
        body_rect,
        track_list_rect,
        add_track_rect,
        track_text_rect,
        ruler_rect,
        transport_panel_rect,
        viewport_controls_rect,
        time_axis_rect,
        content_rect,
        scrollport_rect,
        horizontal_scrollbar_rect,
        vertical_scrollbar_rect,
    }
}

#[must_use]
pub fn timeline_track_world_height(track_count: usize) -> i32 {
    let track_count = i32::try_from(track_count).unwrap_or(i32::MAX);
    if track_count <= 0 {
        return 0;
    }

    track_count.saturating_mul(TIMELINE_TRACK_ROW_HEIGHT)
        + track_count
            .saturating_sub(1)
            .saturating_mul(TIMELINE_TRACK_ROW_GAP)
}

#[must_use]
pub fn timeline_clamp_vertical_scroll_offset(
    layout: TimelineDocumentLayout,
    track_count: usize,
    offset: i32,
) -> i32 {
    let max_offset = timeline_track_world_height(track_count)
        .saturating_sub(layout.scrollport_rect.height().max(0));
    offset.clamp(0, max_offset.max(0))
}

#[must_use]
pub fn timeline_selection_surface_contains(
    layout: TimelineDocumentLayout,
    point: ClientPoint,
) -> bool {
    layout.time_axis_rect.contains(point) || layout.scrollport_rect.contains(point)
}

#[must_use]
pub fn timeline_scroll_interaction_contains(
    layout: TimelineDocumentLayout,
    point: ClientPoint,
) -> bool {
    timeline_selection_surface_contains(layout, point)
        || layout.horizontal_scrollbar_rect.contains(point)
        || layout.vertical_scrollbar_rect.contains(point)
}

#[must_use]
pub fn timeline_viewport_point_from_client_point(
    layout: TimelineDocumentLayout,
    point: ClientPoint,
) -> TimelineViewportPoint {
    let point_x = point
        .to_win32_point()
        .map_or(layout.scrollport_rect.left(), |point| point.x);
    TimelineViewportPoint::new_pixels(f64::from(
        (point_x - layout.scrollport_rect.left()).clamp(0, layout.scrollport_rect.width().max(0)),
    ))
}

#[must_use]
pub fn timeline_track_world_y_from_client_point(
    layout: TimelineDocumentLayout,
    vertical_scroll_offset: i32,
    point: ClientPoint,
) -> i32 {
    let point_y = point
        .to_win32_point()
        .map_or(layout.scrollport_rect.top(), |point| point.y);
    (point_y - layout.scrollport_rect.top()).saturating_add(vertical_scroll_offset)
}

#[must_use]
pub fn timeline_track_index_at_point(
    layout: TimelineDocumentLayout,
    vertical_scroll_offset: i32,
    track_count: usize,
    point: ClientPoint,
) -> Option<usize> {
    if !layout.scrollport_rect.contains(point) {
        return None;
    }

    let row_pitch = TIMELINE_TRACK_ROW_HEIGHT + TIMELINE_TRACK_ROW_GAP;
    if row_pitch <= 0 {
        return None;
    }

    let world_y = timeline_track_world_y_from_client_point(layout, vertical_scroll_offset, point);
    let row_index = usize::try_from(world_y / row_pitch).ok()?;
    let row_offset = world_y.rem_euclid(row_pitch);
    (row_index < track_count && row_offset < TIMELINE_TRACK_ROW_HEIGHT).then_some(row_index)
}

#[must_use]
pub fn timeline_track_vertical_range(row_index: usize) -> TimelineTrackVerticalRange {
    let top = timeline_track_row_world_top(row_index);
    TimelineTrackVerticalRange::new(top, top.saturating_add(TIMELINE_TRACK_ROW_HEIGHT))
}

#[must_use]
pub fn timeline_viewport_control_rect(
    layout: TimelineDocumentLayout,
    action: SceneAction,
) -> Option<ClientRect> {
    let controls_rect = layout.viewport_controls_rect;
    let gap = 8;
    let button_width = ((controls_rect.width() - (gap * 5)) / 6).max(28);
    let top = controls_rect.top();
    let bottom = controls_rect.bottom();

    [
        SceneAction::PanTimelineLeft,
        SceneAction::PanTimelineRight,
        SceneAction::ZoomTimelineIn,
        SceneAction::ZoomTimelineOut,
        SceneAction::SelectTimelineTool,
        SceneAction::SelectTimelineBrush,
    ]
    .into_iter()
    .enumerate()
    .find_map(|(index, candidate)| {
        (candidate == action).then(|| {
            let index = i32::try_from(index).unwrap_or(0);
            let left = controls_rect.left() + index * (button_width + gap);
            ClientRect::new(
                left,
                top,
                (left + button_width).min(controls_rect.right()),
                bottom,
            )
        })
    })
}

#[must_use]
pub const fn timeline_viewport_control_label(action: SceneAction) -> Option<&'static str> {
    match action {
        SceneAction::PanTimelineLeft => Some("<-"),
        SceneAction::PanTimelineRight => Some("->"),
        SceneAction::ZoomTimelineIn => Some("+"),
        SceneAction::ZoomTimelineOut => Some("-"),
        SceneAction::SelectTimelineTool => Some("Sel"),
        SceneAction::SelectTimelineBrush => Some("Box"),
        _ => None,
    }
}

#[must_use]
pub const fn timeline_viewport_control_tooltip(action: SceneAction) -> Option<&'static str> {
    match action {
        SceneAction::PanTimelineLeft => Some("Pan timeline earlier"),
        SceneAction::PanTimelineRight => Some("Pan timeline later"),
        SceneAction::ZoomTimelineIn => Some("Zoom timeline in"),
        SceneAction::ZoomTimelineOut => Some("Zoom timeline out"),
        SceneAction::SelectTimelineTool => Some("Use the selection tool"),
        SceneAction::SelectTimelineBrush => Some("Draw empty text boxes on text tracks"),
        _ => None,
    }
}

#[must_use]
pub const fn timeline_transport_play_button_rect(layout: TimelineDocumentLayout) -> ClientRect {
    layout.transport_panel_rect
}

#[expect(
    clippy::struct_field_names,
    reason = "these names distinguish the rendered row regions used for hit testing"
)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioInputDeviceRowLayout {
    pub row_rect: ClientRect,
    pub icon_rect: ClientRect,
    pub text_rect: ClientRect,
}

#[expect(
    clippy::struct_field_names,
    reason = "these rect names reflect the rendered detail regions and hit targets"
)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioInputDeviceDetailLayout {
    pub icon_rect: ClientRect,
    pub info_rect: ClientRect,
    pub arm_button_rect: ClientRect,
    pub transcription_button_rect: ClientRect,
    pub transcription_flush_button_rect: ClientRect,
    pub play_pause_button_rect: ClientRect,
    pub loopback_button_rect: ClientRect,
    pub arm_status_rect: ClientRect,
    pub legacy_recording_button_rect: ClientRect,
    pub buffer_section_rect: ClientRect,
    pub waveform_rect: ClientRect,
    pub mel_spectrogram_rect: ClientRect,
    pub transcript_terminal_rect: ClientRect,
    pub timeline_label_rect: ClientRect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioInputTimelineHeadGrabberLayout {
    pub kind: AudioInputTimelineHeadKind,
    pub rect: ClientRect,
}

#[expect(
    clippy::struct_excessive_bools,
    reason = "independent hover and press state drives separate shader controls"
)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AudioInputDeviceDetailVisualState {
    pub transcription_hovered: bool,
    pub transcription_pressed: bool,
    pub transcription_flush_hovered: bool,
    pub transcription_flush_pressed: bool,
    pub playback_hovered: bool,
    pub playback_pressed: bool,
    pub loopback_hovered: bool,
    pub loopback_pressed: bool,
    pub hovered_head: Option<AudioInputTimelineHeadKind>,
    pub grabbed_head: Option<AudioInputTimelineHeadKind>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AudioDaemonVisualState {
    pub cuda_check_hovered: bool,
    pub cuda_check_pressed: bool,
    pub hovered_model_index: Option<usize>,
    pub pressed_model_index: Option<usize>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DemoModeVisualState {
    pub demo_button: ButtonVisualState,
    pub scramble_toggle: ButtonVisualState,
}

#[expect(
    clippy::struct_field_names,
    reason = "these names distinguish the rendered demo-mode regions used for hit testing"
)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DemoModeLayout {
    pub demo_button_bounds: ClientRect,
    pub scramble_toggle_bounds: ClientRect,
    pub scramble_toggle_track_bounds: ClientRect,
    pub identifiers_bounds: ClientRect,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DemoInputDeviceIdentifier(String);

impl DemoInputDeviceIdentifier {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'a> Arbitrary<'a> for DemoInputDeviceIdentifier {
    fn arbitrary(u: &mut arbitrary::Unstructured<'a>) -> arbitrary::Result<Self> {
        let flow = u.int_in_range(0_u32..=2)?;
        let data1 = u32::arbitrary(u)?;
        let data2 = u16::arbitrary(u)?;
        let data3 = u16::arbitrary(u)?;
        let data4 = <[u8; 8]>::arbitrary(u)?;
        Ok(Self(format!(
            "{{0.0.1.{flow:08X}}}.{{{data1:08X}-{data2:04X}-{data3:04X}-{b0:02X}{b1:02X}-{b2:02X}{b3:02X}{b4:02X}{b5:02X}{b6:02X}{b7:02X}}}",
            b0 = data4[0],
            b1 = data4[1],
            b2 = data4[2],
            b3 = data4[3],
            b4 = data4[4],
            b5 = data4[5],
            b6 = data4[6],
            b7 = data4[7],
        )))
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneButtonSpec {
    pub action: SceneAction,
    pub label: &'static str,
    pub tooltip: &'static str,
    pub sprite: SpriteId,
    pub color: [f32; 4],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CursorGallerySpriteSpec {
    pub cursor: CursorGalleryCursorKind,
    pub label: &'static str,
    pub sprite: SpriteId,
    pub color: [f32; 4],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorGalleryCursorKind {
    Arrow,
    Hand,
    IBeam,
    Cross,
    Wait,
    SizeAll,
    Help,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CursorGalleryCellLayout {
    pub index: usize,
    pub spec: CursorGallerySpriteSpec,
    pub card_rect: ClientRect,
    pub sprite_rect: ClientRect,
    pub label_rect: ClientRect,
}

impl CursorGalleryCellLayout {
    #[must_use]
    pub fn hit_rect(self) -> ClientRect {
        ClientRect::new(
            self.card_rect.left().min(self.label_rect.left()),
            self.card_rect.top().min(self.label_rect.top()),
            self.card_rect.right().max(self.label_rect.right()),
            self.card_rect.bottom().max(self.label_rect.bottom()),
        )
    }
}

#[expect(
    clippy::struct_field_names,
    reason = "these rect names reflect the rendered regions and hit targets"
)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SceneButtonLayout {
    pub card_rect: ClientRect,
    pub sprite_rect: ClientRect,
    pub label_rect: ClientRect,
}

impl SceneButtonLayout {
    #[must_use]
    pub fn hit_rect(self) -> ClientRect {
        ClientRect::new(
            self.card_rect.left().min(self.label_rect.left()),
            self.card_rect.top().min(self.label_rect.top()),
            self.card_rect.right().max(self.label_rect.right()),
            self.card_rect.bottom().max(self.label_rect.bottom()),
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SceneButtonGridMetrics {
    columns: usize,
    button_size: i32,
    button_gap: i32,
    label_gap: i32,
    label_height: i32,
    sprite_inset: i32,
}

#[derive(Clone, Copy, Debug)]
pub struct ClickState<T>
where
    T: Copy,
{
    pub action: T,
    pub clicked_at: Instant,
}

#[must_use]
/// windowing[impl launcher.buttons.large-image-cards]
pub fn build_scene_render_scene(
    layout: TerminalLayout,
    scene_kind: SceneWindowKind,
    window_chrome_buttons_state: WindowChromeButtonsState,
    max_button_size: i32,
    button_states: &[(SceneAction, ButtonVisualState)],
    virtual_cursor: Option<ClientPoint>,
) -> RenderScene {
    let mut scene = build_scene_shell(layout, scene_kind, window_chrome_buttons_state);

    let specs = scene_button_specs(scene_kind);
    let button_layouts =
        layout_scene_buttons(layout.terminal_panel_rect(), specs.len(), max_button_size);
    for (index, spec) in specs.iter().enumerate() {
        let button_layout = button_layouts[index];
        let visual_state = button_states
            .iter()
            .find_map(|(action, state)| (*action == spec.action).then_some(*state))
            .unwrap_or_default();
        let card_color = if visual_state.active {
            [
                spec.color[0] + 0.08,
                spec.color[1] + 0.08,
                spec.color[2] + 0.08,
                1.0,
            ]
        } else {
            spec.color
        };

        push_panel_with_data(
            &mut scene,
            button_layout.card_rect.to_win32_rect(),
            card_color,
            PanelEffect::SceneButtonCard,
            visual_state.shader_data(),
        );
        push_sprite(
            &mut scene,
            button_layout.sprite_rect.to_win32_rect(),
            [1.0, 1.0, 1.0, 1.0],
            spec.sprite,
        );
        push_centered_text(
            &mut scene,
            button_layout.label_rect.to_win32_rect(),
            spec.label,
            [0.97, 0.97, 0.99, 1.0],
        );
    }

    if scene_kind == SceneWindowKind::Launcher {
        push_virtual_cursor_pointer(
            &mut scene,
            virtual_cursor,
            SpriteId::CursorHand,
            [0.48, 0.95, 1.0, 0.96],
        );
    }

    scene
}

#[must_use]
#[expect(
    clippy::too_many_lines,
    reason = "launcher and auxiliary scene card specifications are intentionally centralized"
)]
// windowing[impl launcher.buttons.terminal]
// windowing[impl launcher.buttons.storage-placeholder]
// windowing[impl launcher.buttons.environment-variables-placeholder]
// windowing[impl launcher.buttons.application-windows-placeholder]
// windowing[impl launcher.buttons.audio-picker]
// windowing[impl launcher.buttons.cursor-gallery]
// timeline[impl launcher.button]
// audio[impl gui.daemon-button]
// windowing[impl audio-picker.buttons.windows]
// windowing[impl audio-picker.buttons.file]
pub fn scene_button_specs(scene_kind: SceneWindowKind) -> &'static [SceneButtonSpec] {
    match scene_kind {
        SceneWindowKind::Launcher => &[
            SceneButtonSpec {
                action: SceneAction::OpenTerminal,
                label: "Terminal",
                tooltip: "Open terminal",
                sprite: SpriteId::Terminal,
                color: [0.18, 0.25, 0.35, 1.0],
            },
            SceneButtonSpec {
                action: SceneAction::OpenCursorInfo,
                label: "Cursor Info",
                tooltip: "Open cursor-info",
                sprite: SpriteId::Terminal,
                color: [0.16, 0.30, 0.24, 1.0],
            },
            SceneButtonSpec {
                action: SceneAction::OpenCursorGallery,
                label: "Cursor Gallery",
                tooltip: "Inspect OS cursor sprites",
                sprite: SpriteId::CursorArrow,
                color: [0.20, 0.18, 0.32, 1.0],
            },
            SceneButtonSpec {
                // windowing[impl launcher.buttons.demo-mode]
                action: SceneAction::OpenDemoMode,
                label: "Demo Mode",
                tooltip: "Open demo privacy controls",
                sprite: SpriteId::Terminal,
                color: [0.15, 0.25, 0.28, 1.0],
            },
            SceneButtonSpec {
                action: SceneAction::OpenStorage,
                label: "Storage",
                tooltip: "Storage is not implemented yet",
                sprite: SpriteId::Storage,
                color: [0.30, 0.21, 0.14, 1.0],
            },
            SceneButtonSpec {
                action: SceneAction::OpenEnvironmentVariables,
                label: "Environment Variables",
                tooltip: "Environment-variable inspector is not implemented yet",
                sprite: SpriteId::Storage,
                color: [0.18, 0.29, 0.22, 1.0],
            },
            SceneButtonSpec {
                action: SceneAction::OpenApplicationWindows,
                label: "Application Windows",
                tooltip: "Application-window inspector is not implemented yet",
                sprite: SpriteId::Terminal,
                color: [0.22, 0.24, 0.34, 1.0],
            },
            SceneButtonSpec {
                action: SceneAction::OpenAudioPicker,
                label: "Audio",
                tooltip: "Choose audio source",
                sprite: SpriteId::Audio,
                color: [0.25, 0.21, 0.11, 1.0],
            },
            SceneButtonSpec {
                // audio[impl gui.daemon-button]
                action: SceneAction::OpenAudioDaemon,
                label: "Audio Daemon",
                tooltip: "Inspect transcription daemon status",
                sprite: SpriteId::Terminal,
                color: [0.24, 0.18, 0.30, 1.0],
            },
            SceneButtonSpec {
                // audio[impl gui.launcher-button]
                action: SceneAction::OpenAudioInputDevices,
                label: "Audio Devices",
                tooltip: "Choose microphone input device",
                sprite: SpriteId::Audio,
                color: [0.13, 0.25, 0.32, 1.0],
            },
            SceneButtonSpec {
                action: SceneAction::OpenTimeline,
                label: "Timeline",
                tooltip: "Open timeline workspace",
                sprite: SpriteId::Terminal,
                color: [0.15, 0.23, 0.27, 1.0],
            },
        ],
        SceneWindowKind::TimelineStart => &[
            SceneButtonSpec {
                // timeline[impl start-window.create-or-import]
                // timeline[impl start-window.new-blank]
                action: SceneAction::CreateBlankTimeline,
                label: "New",
                tooltip: "Create a blank timeline",
                sprite: SpriteId::Terminal,
                color: [0.14, 0.28, 0.24, 1.0],
            },
            SceneButtonSpec {
                // timeline[impl start-window.create-or-import]
                // timeline[impl import.tracy.file-picker]
                action: SceneAction::ImportTimeline,
                label: "Import",
                tooltip: "Import a Tracy capture",
                sprite: SpriteId::FileAudio,
                color: [0.23, 0.19, 0.30, 1.0],
            },
        ],
        SceneWindowKind::TimelineAddTrack => &[
            SceneButtonSpec {
                // timeline[impl add-track.workflow]
                // timeline[impl add-track.microphone-placeholder]
                action: SceneAction::AppendMicrophoneTrack,
                label: "Microphone",
                tooltip: "Create a pending microphone track",
                sprite: SpriteId::WindowsAudio,
                color: [0.17, 0.24, 0.31, 1.0],
            },
            SceneButtonSpec {
                // timeline[impl add-track.workflow]
                action: SceneAction::AppendTranscriptionTrack,
                label: "Transcription",
                tooltip: "Add a transcription observer track",
                sprite: SpriteId::Transcription,
                color: [0.30, 0.19, 0.28, 1.0],
            },
            SceneButtonSpec {
                // timeline[impl add-track.workflow]
                action: SceneAction::AppendTextTrack,
                label: "Text",
                tooltip: "Add a manual text track",
                sprite: SpriteId::Terminal,
                color: [0.24, 0.22, 0.14, 1.0],
            },
            SceneButtonSpec {
                // timeline[impl add-track.workflow]
                // timeline[impl add-track.tracy]
                action: SceneAction::ImportTimeline,
                label: "Tracing",
                tooltip: "Append a Tracy capture track",
                sprite: SpriteId::FileAudio,
                color: [0.24, 0.19, 0.30, 1.0],
            },
            SceneButtonSpec {
                // timeline[impl add-track.workflow]
                action: SceneAction::CloseTimelineTrackMenu,
                label: "Back",
                tooltip: "Return to the timeline document",
                sprite: SpriteId::Back,
                color: [0.18, 0.18, 0.22, 1.0],
            },
        ],
        SceneWindowKind::AudioPicker => &[
            SceneButtonSpec {
                action: SceneAction::SelectWindowsBell,
                label: "Windows",
                tooltip: "Use Windows notification sound",
                sprite: SpriteId::WindowsAudio,
                color: [0.14, 0.24, 0.35, 1.0],
            },
            SceneButtonSpec {
                action: SceneAction::SelectFileBell,
                label: "Pick File",
                tooltip: "Choose custom audio file",
                sprite: SpriteId::FileAudio,
                color: [0.23, 0.19, 0.30, 1.0],
            },
        ],
        SceneWindowKind::CursorGallery
        | SceneWindowKind::Timeline
        | SceneWindowKind::DemoMode
        | SceneWindowKind::AudioDaemon
        | SceneWindowKind::AudioInputDevicePicker
        | SceneWindowKind::AudioInputDeviceDetails
        | SceneWindowKind::TimelineTranscriptionSettings => &[],
    }
}

#[must_use]
// timeline[impl blank.track-list]
// timeline[impl blank.add-track-placeholder]
#[expect(
    clippy::too_many_lines,
    reason = "timeline scene composition keeps the coupled layout and overlay drawing in one place"
)]
pub fn build_blank_timeline_render_scene(
    layout: TerminalLayout,
    window_chrome_buttons_state: WindowChromeButtonsState,
    document: Option<&TimelineDocument>,
    audio_input_state: Option<&AudioInputDeviceWindowState>,
    visual_state: TimelineDocumentVisualState,
) -> RenderScene {
    let mut scene = build_scene_shell(
        layout,
        SceneWindowKind::Timeline,
        window_chrome_buttons_state,
    );
    let timeline_layout = timeline_document_layout(layout.terminal_panel_rect().inset(24));
    push_panel(
        &mut scene,
        timeline_layout.body_rect.to_win32_rect(),
        [0.08, 0.09, 0.11, 1.0],
        PanelEffect::SceneBody,
    );

    // timeline[impl blank.tool-panels]
    push_panel(
        &mut scene,
        timeline_layout.track_list_rect.to_win32_rect(),
        [0.11, 0.12, 0.15, 1.0],
        PanelEffect::SceneButtonCard,
    );
    push_timeline_track_list_panel(
        &mut scene,
        timeline_layout,
        document,
        audio_input_state,
        visual_state,
    );
    push_panel_with_data(
        &mut scene,
        timeline_layout.add_track_rect.to_win32_rect(),
        [0.15, 0.18, 0.20, 1.0],
        PanelEffect::SceneButtonCard,
        visual_state.add_track_button.shader_data(),
    );
    push_centered_text(
        &mut scene,
        timeline_layout.add_track_rect.to_win32_rect(),
        "Add Track",
        [0.82, 0.86, 0.90, 1.0],
    );
    push_panel(
        &mut scene,
        timeline_layout.ruler_rect.to_win32_rect(),
        [0.12, 0.13, 0.16, 1.0],
        PanelEffect::TerminalPanel,
    );
    push_timeline_transport_panel(&mut scene, timeline_layout, audio_input_state, visual_state);
    push_panel(
        &mut scene,
        timeline_layout.viewport_controls_rect.to_win32_rect(),
        [0.14, 0.16, 0.20, 0.96],
        PanelEffect::SceneButtonCard,
    );
    push_timeline_viewport_controls(&mut scene, timeline_layout, visual_state);
    push_panel(
        &mut scene,
        timeline_layout.content_rect.to_win32_rect(),
        [0.07, 0.08, 0.10, 1.0],
        PanelEffect::TerminalFill,
    );
    push_panel(
        &mut scene,
        timeline_layout.scrollport_rect.to_win32_rect(),
        [0.08, 0.09, 0.11, 1.0],
        PanelEffect::TerminalFill,
    );
    let viewport = document.map_or_else(TimelineViewport::default, TimelineDocument::viewport);
    let track_count = document.map_or(0, |document| document.tracks().len());
    let vertical_scroll_offset = timeline_clamp_vertical_scroll_offset(
        timeline_layout,
        track_count,
        visual_state.vertical_scroll_offset,
    );
    let visible_selection_rect = visual_state.selection.and_then(|selection| {
        timeline_visible_selection_rect(
            timeline_layout,
            viewport,
            selection,
            vertical_scroll_offset,
            track_count,
        )
    });
    let ruler_selection_rect = visual_state.selection.and_then(|selection| {
        timeline_visible_selection_time_band_rect(timeline_layout, viewport, selection)
    });
    let ruler_ticks = viewport.ruler_ticks(timeline_layout.time_axis_rect.width(), 6);
    for tick in &ruler_ticks {
        push_timeline_ruler_tick(&mut scene, timeline_layout, tick);
    }
    if let Some(ruler_selection_rect) = ruler_selection_rect {
        push_panel(
            &mut scene,
            ruler_selection_rect.to_win32_rect(),
            TIMELINE_SELECTION_FILL,
            PanelEffect::TerminalFill,
        );
    }
    if let Some(document) = document.filter(|document| !document.tracks().is_empty()) {
        push_timeline_track_preview_rows(
            &mut scene,
            timeline_layout,
            document,
            audio_input_state,
            vertical_scroll_offset,
            visible_selection_rect,
        );
    } else {
        let content_text = timeline_content_text(document);
        push_text_block(
            &mut scene,
            timeline_layout.scrollport_rect.inset(18).to_win32_rect(),
            &content_text,
            9,
            16,
            [0.68, 0.72, 0.78, 1.0],
        );
    }
    if let Some(audio_input_state) = audio_input_state {
        push_timeline_audio_heads(
            &mut scene,
            timeline_layout,
            viewport,
            audio_input_state,
            visual_state,
        );
    }
    if let Some(selection_rect) = visible_selection_rect {
        push_timeline_selection_overlay(&mut scene, selection_rect);
    }
    push_timeline_scrollbars(
        &mut scene,
        timeline_layout,
        document,
        viewport,
        visual_state.selection,
        vertical_scroll_offset,
    );

    scene
}

#[expect(
    clippy::struct_field_names,
    reason = "the settings layout keeps explicit rect names so hit-testing helpers stay obvious"
)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TimelineTranscriptionSettingsLayout {
    body_rect: ClientRect,
    title_rect: ClientRect,
    subtitle_rect: ClientRect,
    model_target_column_rect: ClientRect,
    model_socket_rect: ClientRect,
    model_card_rect: ClientRect,
    model_list_rect: ClientRect,
    target_card_rect: ClientRect,
    target_list_rect: ClientRect,
    add_text_track_button_rect: ClientRect,
    output_target_column_rect: ClientRect,
    output_socket_rect: ClientRect,
    footer_rect: ClientRect,
}

#[must_use]
#[expect(
    clippy::too_many_lines,
    reason = "the transcription settings scene keeps both list columns and their selection rendering localized"
)]
pub fn build_timeline_transcription_settings_render_scene(
    layout: TerminalLayout,
    window_chrome_buttons_state: WindowChromeButtonsState,
    document: Option<&TimelineDocument>,
    settings_state: Option<TimelineTranscriptionSettingsViewState>,
) -> RenderScene {
    let mut scene = build_scene_shell(
        layout,
        SceneWindowKind::TimelineTranscriptionSettings,
        window_chrome_buttons_state,
    );
    let settings_layout = timeline_transcription_settings_layout(layout);
    push_panel(
        &mut scene,
        settings_layout.body_rect.to_win32_rect(),
        [0.09, 0.10, 0.13, 1.0],
        PanelEffect::SceneBody,
    );
    push_title_text(
        &mut scene,
        settings_layout.title_rect.to_win32_rect(),
        "Transcription Track Settings",
        [0.98, 0.97, 1.0, 1.0],
    );
    push_text_block(
        &mut scene,
        settings_layout.subtitle_rect.to_win32_rect(),
        "Choose a Rust Whisper model and the text track that should receive transcription output.",
        9,
        16,
        [0.72, 0.76, 0.82, 1.0],
    );
    push_panel(
        &mut scene,
        settings_layout.model_target_column_rect.to_win32_rect(),
        [0.11, 0.13, 0.17, 1.0],
        PanelEffect::SceneBody,
    );
    push_panel(
        &mut scene,
        settings_layout.model_card_rect.to_win32_rect(),
        [0.12, 0.14, 0.18, 1.0],
        PanelEffect::SceneButtonCard,
    );
    push_panel(
        &mut scene,
        settings_layout.target_card_rect.to_win32_rect(),
        [0.12, 0.14, 0.18, 1.0],
        PanelEffect::SceneButtonCard,
    );
    push_panel(
        &mut scene,
        settings_layout.output_target_column_rect.to_win32_rect(),
        [0.11, 0.13, 0.17, 1.0],
        PanelEffect::SceneBody,
    );
    push_text_block(
        &mut scene,
        ClientRect::new(
            settings_layout.model_target_column_rect.left() + 8,
            settings_layout.model_target_column_rect.top() + 10,
            settings_layout.model_target_column_rect.right() - 8,
            settings_layout.model_target_column_rect.top() + 42,
        )
        .to_win32_rect(),
        "Active Model Target",
        8,
        14,
        [0.92, 0.94, 0.98, 1.0],
    );
    push_text_block(
        &mut scene,
        ClientRect::new(
            settings_layout.model_card_rect.left() + 16,
            settings_layout.model_card_rect.top() + 12,
            settings_layout.model_card_rect.right() - 16,
            settings_layout.model_card_rect.top() + 34,
        )
        .to_win32_rect(),
        "Rust Whisper Model",
        9,
        16,
        [0.92, 0.94, 0.98, 1.0],
    );
    push_text_block(
        &mut scene,
        ClientRect::new(
            settings_layout.target_card_rect.left() + 16,
            settings_layout.target_card_rect.top() + 12,
            settings_layout.target_card_rect.right() - 16,
            settings_layout.target_card_rect.top() + 34,
        )
        .to_win32_rect(),
        "Text Track List",
        9,
        16,
        [0.92, 0.94, 0.98, 1.0],
    );
    push_text_block(
        &mut scene,
        ClientRect::new(
            settings_layout.output_target_column_rect.left() + 8,
            settings_layout.output_target_column_rect.top() + 10,
            settings_layout.output_target_column_rect.right() - 8,
            settings_layout.output_target_column_rect.top() + 42,
        )
        .to_win32_rect(),
        "Output Track Target",
        8,
        14,
        [0.92, 0.94, 0.98, 1.0],
    );

    let Some(document) = document else {
        push_text_block(
            &mut scene,
            settings_layout.body_rect.inset(36).to_win32_rect(),
            "Open a timeline document before configuring transcription settings.",
            10,
            18,
            [0.86, 0.88, 0.92, 1.0],
        );
        return scene;
    };
    let Some(settings_state) = settings_state else {
        push_text_block(
            &mut scene,
            settings_layout.body_rect.inset(36).to_win32_rect(),
            "Select a transcription track from the timeline to edit its settings.",
            10,
            18,
            [0.86, 0.88, 0.92, 1.0],
        );
        return scene;
    };

    let text_tracks = document
        .tracks()
        .iter()
        .filter(|track| track.kind() == TimelineTrackKind::Text)
        .collect::<Vec<_>>();
    let Some(track_name) = document
        .tracks()
        .iter()
        .find(|track| track.id() == settings_state.track_id)
        .map(TimelineTrack::name)
    else {
        push_text_block(
            &mut scene,
            settings_layout.body_rect.inset(36).to_win32_rect(),
            "The selected transcription track no longer exists.",
            10,
            18,
            [0.86, 0.88, 0.92, 1.0],
        );
        return scene;
    };
    push_text_block(
        &mut scene,
        ClientRect::new(
            settings_layout.title_rect.left(),
            settings_layout.title_rect.bottom() + 4,
            settings_layout.title_rect.right(),
            settings_layout.title_rect.bottom() + 26,
        )
        .to_win32_rect(),
        track_name,
        9,
        16,
        [0.90, 0.86, 0.70, 1.0],
    );

    for (index, model) in KNOWN_WHISPER_MODELS.iter().enumerate() {
        let Some(row_rect) = timeline_transcription_settings_model_row_rect(layout, index) else {
            break;
        };
        let selected = settings_state.selected_column == TimelineTranscriptionSettingsColumn::Model
            && settings_state.selected_model_index == index;
        let hovered = settings_state.hovered_model_index == Some(index);
        push_panel(
            &mut scene,
            row_rect.to_win32_rect(),
            if selected {
                [0.24, 0.35, 0.42, 1.0]
            } else if hovered {
                [0.18, 0.24, 0.30, 1.0]
            } else {
                [0.10, 0.12, 0.16, 1.0]
            },
            PanelEffect::SceneButtonCard,
        );
        push_text_block(
            &mut scene,
            ClientRect::new(
                row_rect.left() + 44,
                row_rect.top() + 6,
                row_rect.right() - 10,
                row_rect.top() + 24,
            )
            .to_win32_rect(),
            model.name,
            9,
            16,
            [0.94, 0.95, 0.98, 1.0],
        );
        push_text_block(
            &mut scene,
            ClientRect::new(
                row_rect.left() + 44,
                row_rect.top() + 22,
                row_rect.right() - 10,
                row_rect.bottom() - 6,
            )
            .to_win32_rect(),
            model.parameter_count,
            8,
            14,
            [0.68, 0.74, 0.80, 1.0],
        );
    }

    for index in 0..=text_tracks.len() {
        let Some(row_rect) = timeline_transcription_settings_target_row_rect(layout, index) else {
            break;
        };
        let selected = settings_state.selected_column
            == TimelineTranscriptionSettingsColumn::TargetTrack
            && settings_state.selected_target_index == index;
        let hovered = settings_state.hovered_target_index == Some(index);
        push_panel(
            &mut scene,
            row_rect.to_win32_rect(),
            if selected {
                [0.40, 0.30, 0.14, 1.0]
            } else if hovered {
                [0.26, 0.20, 0.10, 1.0]
            } else {
                [0.11, 0.12, 0.16, 1.0]
            },
            PanelEffect::SceneButtonCard,
        );
        let label = if index == 0 {
            "Unassigned"
        } else {
            text_tracks[index - 1].name()
        };
        push_text_block(
            &mut scene,
            ClientRect::new(
                row_rect.left() + 10,
                row_rect.top() + 8,
                row_rect.right() - 44,
                row_rect.bottom() - 8,
            )
            .to_win32_rect(),
            label,
            9,
            16,
            [0.94, 0.95, 0.98, 1.0],
        );
    }

    push_panel(
        &mut scene,
        settings_layout.add_text_track_button_rect.to_win32_rect(),
        if settings_state.hovered_add_text_track_button {
            [0.21, 0.29, 0.37, 1.0]
        } else {
            [0.14, 0.18, 0.23, 1.0]
        },
        PanelEffect::SceneButtonCard,
    );
    push_text_block(
        &mut scene,
        settings_layout
            .add_text_track_button_rect
            .inset(12)
            .to_win32_rect(),
        "Add Text Track",
        9,
        16,
        [0.96, 0.97, 1.0, 1.0],
    );

    let model_target_rect = timeline_transcription_settings_target_puck_rect(
        layout,
        settings_state,
        TimelineTranscriptionSettingsTarget::Model,
    );
    let output_target_rect = timeline_transcription_settings_target_puck_rect(
        layout,
        settings_state,
        TimelineTranscriptionSettingsTarget::TargetTrack,
    );
    push_timeline_transcription_target_widget(
        &mut scene,
        settings_layout.model_socket_rect,
        model_target_rect,
        settings_state.hovered_model_socket,
        settings_state.hovered_model_target,
        settings_state.dragging_target == Some(TimelineTranscriptionSettingsTarget::Model),
    );
    push_timeline_transcription_target_widget(
        &mut scene,
        settings_layout.output_socket_rect,
        output_target_rect,
        settings_state.hovered_target_socket,
        settings_state.hovered_output_target,
        settings_state.dragging_target == Some(TimelineTranscriptionSettingsTarget::TargetTrack),
    );

    push_text_block(
        &mut scene,
        settings_layout.footer_rect.to_win32_rect(),
        "Drag a target onto a row, use arrows to move the active list target, click an empty socket to recall its puck, and use Esc to return to the timeline.",
        8,
        14,
        [0.70, 0.74, 0.80, 1.0],
    );
    scene
}

fn push_timeline_transcription_target_widget(
    scene: &mut RenderScene,
    socket_rect: ClientRect,
    puck_rect: Option<ClientRect>,
    hovered_socket: bool,
    hovered_target: bool,
    dragging: bool,
) {
    if let Some(puck_rect) = puck_rect {
        push_timeline_transcription_target_rope(scene, socket_rect, puck_rect, dragging);
        push_panel_with_data(
            scene,
            puck_rect.to_win32_rect(),
            [1.0, 0.98, 0.96, 1.0],
            PanelEffect::TargetMarker,
            [1.0, f32::from(hovered_target), f32::from(dragging), 0.0],
        );
    }

    push_panel_with_data(
        scene,
        socket_rect.to_win32_rect(),
        [1.0, 0.98, 0.96, 1.0],
        PanelEffect::TargetMarker,
        [0.0, f32::from(hovered_socket), f32::from(dragging), 0.0],
    );
}

fn push_timeline_transcription_target_rope(
    scene: &mut RenderScene,
    socket_rect: ClientRect,
    puck_rect: ClientRect,
    dragging: bool,
) {
    let socket_center = rect_center_point(socket_rect);
    let puck_center = rect_center_point(puck_rect);
    if socket_center == puck_center {
        return;
    }

    let Some(socket_center) = socket_center.to_win32_point().ok() else {
        return;
    };
    let Some(puck_center) = puck_center.to_win32_point().ok() else {
        return;
    };

    let thickness = 3;
    let midpoint_x = i32::midpoint(socket_center.x, puck_center.x);
    let socket_y = socket_center.y;
    let puck_y = puck_center.y;
    let rope_color = if dragging {
        [0.90, 0.22, 0.18, 0.46]
    } else {
        [0.82, 0.20, 0.17, 0.34]
    };

    push_timeline_transcription_target_rope_segment(
        scene,
        socket_center.x,
        socket_y,
        midpoint_x,
        socket_y,
        thickness,
        rope_color,
    );
    push_timeline_transcription_target_rope_segment(
        scene, midpoint_x, socket_y, midpoint_x, puck_y, thickness, rope_color,
    );
    push_timeline_transcription_target_rope_segment(
        scene,
        midpoint_x,
        puck_y,
        puck_center.x,
        puck_y,
        thickness,
        rope_color,
    );
}

fn push_timeline_transcription_target_rope_segment(
    scene: &mut RenderScene,
    start_x: i32,
    start_y: i32,
    end_x: i32,
    end_y: i32,
    thickness: i32,
    color: [f32; 4],
) {
    let left = start_x.min(end_x);
    let right = start_x.max(end_x);
    let top = start_y.min(end_y);
    let bottom = start_y.max(end_y);
    let rect = if left == right {
        ClientRect::new(
            left - (thickness / 2),
            top,
            left + (thickness / 2) + 1,
            bottom + 1,
        )
    } else {
        ClientRect::new(
            left,
            top - (thickness / 2),
            right + 1,
            top + (thickness / 2) + 1,
        )
    };
    push_panel(
        scene,
        rect.to_win32_rect(),
        color,
        PanelEffect::TerminalFill,
    );
}

fn rect_center_point(rect: ClientRect) -> ClientPoint {
    ClientPoint::new(
        rect.left() + (rect.width() / 2),
        rect.top() + (rect.height() / 2),
    )
}

#[expect(
    clippy::too_many_lines,
    reason = "the transcription settings layout keeps all horizontal target/socket geometry in one place"
)]
fn timeline_transcription_settings_layout(
    layout: TerminalLayout,
) -> TimelineTranscriptionSettingsLayout {
    let body_rect = layout.terminal_panel_rect().inset(24);
    let title_rect = ClientRect::new(
        body_rect.left() + 18,
        body_rect.top() + 18,
        body_rect.right() - 18,
        body_rect.top() + 52,
    );
    let subtitle_rect = ClientRect::new(
        title_rect.left(),
        title_rect.bottom() + 6,
        title_rect.right(),
        title_rect.bottom() + 38,
    );
    let column_gap = 16;
    let card_top = subtitle_rect.bottom() + 22;
    let footer_rect = ClientRect::new(
        body_rect.left() + 18,
        body_rect.bottom() - 34,
        body_rect.right() - 18,
        body_rect.bottom() - 12,
    );
    let card_bottom = footer_rect.top() - 14;
    let inner_left = body_rect.left() + 18;
    let inner_right = body_rect.right() - 18;
    let column_width = ((inner_right
        - inner_left
        - (TIMELINE_TRANSCRIPTION_SETTINGS_TARGET_COLUMN_WIDTH * 2)
        - (column_gap * 3))
        / 2)
    .max(160);
    let model_target_column_rect = ClientRect::new(
        inner_left,
        card_top,
        inner_left + TIMELINE_TRANSCRIPTION_SETTINGS_TARGET_COLUMN_WIDTH,
        card_bottom,
    );
    let model_card_rect = ClientRect::new(
        model_target_column_rect.right() + column_gap,
        card_top,
        model_target_column_rect.right() + column_gap + column_width,
        card_bottom,
    );
    let target_card_rect = ClientRect::new(
        model_card_rect.right() + column_gap,
        card_top,
        model_card_rect.right() + column_gap + column_width,
        card_bottom,
    );
    let output_target_column_rect = ClientRect::new(
        target_card_rect.right() + column_gap,
        card_top,
        inner_right,
        card_bottom,
    );
    let model_socket_rect = centered_rect_in_rect(
        ClientRect::new(
            model_target_column_rect.left() + 8,
            model_target_column_rect.top() + 56,
            model_target_column_rect.right() - 8,
            model_target_column_rect.bottom() - 16,
        ),
        TIMELINE_TRANSCRIPTION_SETTINGS_TARGET_SOCKET_SIZE,
    );
    let output_socket_rect = centered_rect_in_rect(
        ClientRect::new(
            output_target_column_rect.left() + 8,
            output_target_column_rect.top() + 56,
            output_target_column_rect.right() - 8,
            output_target_column_rect.bottom() - 16,
        ),
        TIMELINE_TRANSCRIPTION_SETTINGS_TARGET_SOCKET_SIZE,
    );
    let model_list_rect = ClientRect::new(
        model_card_rect.left() + 12,
        model_card_rect.top() + 44,
        model_card_rect.right() - 12,
        model_card_rect.bottom() - 12,
    );
    let add_text_track_button_rect = ClientRect::new(
        target_card_rect.left() + 12,
        target_card_rect.bottom() - 46,
        target_card_rect.right() - 12,
        target_card_rect.bottom() - 12,
    );
    let target_list_rect = ClientRect::new(
        target_card_rect.left() + 12,
        target_card_rect.top() + 44,
        target_card_rect.right() - 12,
        add_text_track_button_rect.top() - 12,
    );
    TimelineTranscriptionSettingsLayout {
        body_rect,
        title_rect,
        subtitle_rect,
        model_target_column_rect,
        model_socket_rect,
        model_card_rect,
        model_list_rect,
        target_card_rect,
        target_list_rect,
        add_text_track_button_rect,
        output_target_column_rect,
        output_socket_rect,
        footer_rect,
    }
}

fn centered_rect_in_rect(rect: ClientRect, size: i32) -> ClientRect {
    let left = rect.left() + ((rect.width() - size).max(0) / 2);
    let top = rect.top() + ((rect.height() - size).max(0) / 2);
    ClientRect::new(left, top, left + size, top + size)
}

fn centered_rect_at_point(point: ClientPoint, size: i32) -> Option<ClientRect> {
    let point = point.to_win32_point().ok()?;
    let half = size / 2;
    Some(ClientRect::new(
        point.x - half,
        point.y - half,
        point.x - half + size,
        point.y - half + size,
    ))
}

fn timeline_transcription_settings_model_row_rect_from_layout(
    settings_layout: TimelineTranscriptionSettingsLayout,
    row_index: usize,
) -> Option<ClientRect> {
    timeline_transcription_settings_list_row_rect(settings_layout.model_list_rect, row_index)
}

fn timeline_transcription_settings_target_row_rect_from_layout(
    settings_layout: TimelineTranscriptionSettingsLayout,
    row_index: usize,
) -> Option<ClientRect> {
    timeline_transcription_settings_list_row_rect(settings_layout.target_list_rect, row_index)
}

#[must_use]
pub fn timeline_transcription_settings_model_row_rect(
    layout: TerminalLayout,
    row_index: usize,
) -> Option<ClientRect> {
    let settings_layout = timeline_transcription_settings_layout(layout);
    timeline_transcription_settings_model_row_rect_from_layout(settings_layout, row_index)
}

#[must_use]
pub fn timeline_transcription_settings_target_row_rect(
    layout: TerminalLayout,
    row_index: usize,
) -> Option<ClientRect> {
    let settings_layout = timeline_transcription_settings_layout(layout);
    timeline_transcription_settings_target_row_rect_from_layout(settings_layout, row_index)
}

#[must_use]
pub fn timeline_transcription_settings_model_socket_rect(layout: TerminalLayout) -> ClientRect {
    timeline_transcription_settings_layout(layout).model_socket_rect
}

#[must_use]
pub fn timeline_transcription_settings_output_socket_rect(layout: TerminalLayout) -> ClientRect {
    timeline_transcription_settings_layout(layout).output_socket_rect
}

#[must_use]
pub fn timeline_transcription_settings_add_text_track_button_rect(
    layout: TerminalLayout,
) -> ClientRect {
    timeline_transcription_settings_layout(layout).add_text_track_button_rect
}

#[must_use]
pub fn timeline_transcription_settings_target_puck_rect(
    layout: TerminalLayout,
    settings_state: TimelineTranscriptionSettingsViewState,
    target: TimelineTranscriptionSettingsTarget,
) -> Option<ClientRect> {
    let settings_layout = timeline_transcription_settings_layout(layout);
    timeline_transcription_settings_target_puck_rect_from_layout(
        settings_layout,
        settings_state,
        target,
    )
}

fn timeline_transcription_settings_target_puck_rect_from_layout(
    settings_layout: TimelineTranscriptionSettingsLayout,
    settings_state: TimelineTranscriptionSettingsViewState,
    target: TimelineTranscriptionSettingsTarget,
) -> Option<ClientRect> {
    if settings_state.dragging_target == Some(target)
        && let Some(point) = settings_state.drag_position
    {
        return centered_rect_at_point(point, TIMELINE_TRANSCRIPTION_SETTINGS_TARGET_PUCK_SIZE);
    }

    match target {
        TimelineTranscriptionSettingsTarget::Model => {
            if settings_state.model_target_docked {
                timeline_transcription_settings_model_row_rect_from_layout(
                    settings_layout,
                    settings_state.selected_model_index,
                )
                .map(|row_rect| {
                    ClientRect::new(
                        row_rect.left() + 10,
                        row_rect.top()
                            + ((row_rect.height()
                                - TIMELINE_TRANSCRIPTION_SETTINGS_TARGET_PUCK_SIZE)
                                / 2),
                        row_rect.left() + 10 + TIMELINE_TRANSCRIPTION_SETTINGS_TARGET_PUCK_SIZE,
                        row_rect.top()
                            + ((row_rect.height()
                                - TIMELINE_TRANSCRIPTION_SETTINGS_TARGET_PUCK_SIZE)
                                / 2)
                            + TIMELINE_TRANSCRIPTION_SETTINGS_TARGET_PUCK_SIZE,
                    )
                })
            } else {
                Some(centered_rect_in_rect(
                    settings_layout.model_socket_rect,
                    TIMELINE_TRANSCRIPTION_SETTINGS_TARGET_PUCK_SIZE,
                ))
            }
        }
        TimelineTranscriptionSettingsTarget::TargetTrack => {
            if settings_state.output_target_docked {
                timeline_transcription_settings_target_row_rect_from_layout(
                    settings_layout,
                    settings_state.selected_target_index,
                )
                .map(|row_rect| {
                    let left =
                        row_rect.right() - 10 - TIMELINE_TRANSCRIPTION_SETTINGS_TARGET_PUCK_SIZE;
                    let top = row_rect.top()
                        + ((row_rect.height() - TIMELINE_TRANSCRIPTION_SETTINGS_TARGET_PUCK_SIZE)
                            / 2);
                    ClientRect::new(
                        left,
                        top,
                        left + TIMELINE_TRANSCRIPTION_SETTINGS_TARGET_PUCK_SIZE,
                        top + TIMELINE_TRANSCRIPTION_SETTINGS_TARGET_PUCK_SIZE,
                    )
                })
            } else {
                Some(centered_rect_in_rect(
                    settings_layout.output_socket_rect,
                    TIMELINE_TRANSCRIPTION_SETTINGS_TARGET_PUCK_SIZE,
                ))
            }
        }
    }
}

fn timeline_transcription_settings_list_row_rect(
    list_rect: ClientRect,
    row_index: usize,
) -> Option<ClientRect> {
    let row_height = 38;
    let row_gap = 8;
    let top = list_rect.top()
        + i32::try_from(row_index)
            .unwrap_or_default()
            .saturating_mul(row_height + row_gap);
    let bottom = top + row_height;
    (bottom <= list_rect.bottom())
        .then(|| ClientRect::new(list_rect.left(), top, list_rect.right(), bottom))
}

// timeline[impl content.preview-lanes]
fn push_timeline_track_preview_rows(
    scene: &mut RenderScene,
    layout: TimelineDocumentLayout,
    document: &TimelineDocument,
    audio_input_state: Option<&AudioInputDeviceWindowState>,
    vertical_scroll_offset: i32,
    visible_selection_rect: Option<ClientRect>,
) {
    if layout.scrollport_rect.width() <= 0 || layout.scrollport_rect.height() <= 0 {
        return;
    }

    for (index, track) in document.tracks().iter().enumerate() {
        let Some(row_rect) =
            timeline_track_row_rect(layout.scrollport_rect, index, vertical_scroll_offset)
        else {
            continue;
        };
        push_panel(
            scene,
            row_rect.to_win32_rect(),
            [0.10, 0.11, 0.14, 0.92],
            PanelEffect::SceneButtonCard,
        );

        let label_rect = ClientRect::new(
            row_rect.left() + 10,
            row_rect.top() + 8,
            row_rect.right() - 10,
            (row_rect.top() + 26).min(row_rect.bottom()),
        );
        push_text_block(
            scene,
            label_rect.to_win32_rect(),
            track.name(),
            8,
            16,
            [0.80, 0.84, 0.90, 1.0],
        );

        if track.kind() == TimelineTrackKind::Text {
            push_timeline_text_blocks(scene, row_rect, layout.scrollport_rect, document, track);
            continue;
        }

        let preview_range = timeline_track_preview_time_range(track, audio_input_state);
        if let Some(preview_rect) = timeline_track_preview_clip_rect(
            row_rect,
            layout.scrollport_rect,
            document.viewport(),
            preview_range,
        ) {
            push_panel(
                scene,
                preview_rect.to_win32_rect(),
                timeline_track_preview_color(track.kind()),
                PanelEffect::TerminalFill,
            );
            if track.kind() == TimelineTrackKind::Audio
                && let Some(audio_input_state) = audio_input_state
                && !audio_input_state.runtime.samples().is_empty()
            {
                push_waveform_bars(
                    scene,
                    preview_rect.inset(4),
                    &audio_input_state.runtime.samples(),
                    timeline_waveform_sample_range(
                        layout.scrollport_rect,
                        document.viewport(),
                        preview_range,
                        audio_input_state.runtime.samples().len(),
                    ),
                );
            } else if track.kind() == TimelineTrackKind::Transcription
                && let Some(audio_input_state) = audio_input_state
            {
                push_timeline_transcription_preview(
                    scene,
                    preview_rect.inset(4),
                    layout.scrollport_rect,
                    document.viewport(),
                    audio_input_state,
                );
            } else if preview_rect.width() >= 72 {
                push_centered_text(
                    scene,
                    preview_rect.to_win32_rect(),
                    match track.kind() {
                        TimelineTrackKind::Audio => "Audio",
                        TimelineTrackKind::Transcription => "Transcription",
                        TimelineTrackKind::Text => "Text",
                        TimelineTrackKind::TracingSpans => "Tracing",
                    },
                    [0.96, 0.98, 1.0, 1.0],
                );
            }

            if let Some(selection_rect) = visible_selection_rect
                && let Some(intersection_rect) =
                    client_rect_intersection(preview_rect, selection_rect)
            {
                push_panel(
                    scene,
                    intersection_rect.to_win32_rect(),
                    TIMELINE_SELECTION_INTERSECTION,
                    PanelEffect::TerminalFill,
                );
            }
        }
    }
}

fn push_timeline_text_blocks(
    scene: &mut RenderScene,
    row_rect: ClientRect,
    content_rect: ClientRect,
    document: &TimelineDocument,
    track: &TimelineTrack,
) {
    for block in document
        .text_blocks()
        .iter()
        .filter(|block| block.track_id() == track.id())
    {
        let Some(block_rect) = timeline_time_range_clip_rect(
            row_rect,
            content_rect,
            document.viewport(),
            block.time_range(),
            28,
        ) else {
            continue;
        };

        push_panel(
            scene,
            block_rect.to_win32_rect(),
            [0.76, 0.63, 0.20, 0.94],
            PanelEffect::SceneButtonCard,
        );

        if block_rect.width() >= 36 {
            let preview_text = if block.text().is_empty() {
                "Empty text"
            } else {
                block.text().lines().next().unwrap_or("Text")
            };
            push_text_block(
                scene,
                block_rect.inset(6).to_win32_rect(),
                preview_text,
                8,
                14,
                [0.14, 0.11, 0.05, 1.0],
            );
        }
    }
}

fn push_timeline_transcription_preview(
    scene: &mut RenderScene,
    preview_rect: ClientRect,
    content_rect: ClientRect,
    viewport: TimelineViewport,
    audio_input_state: &AudioInputDeviceWindowState,
) {
    if preview_rect.width() <= 0 || preview_rect.height() <= 0 {
        return;
    }

    let samples = audio_input_state.runtime.samples();
    if !samples.is_empty() {
        push_waveform_bars(
            scene,
            preview_rect,
            &samples,
            timeline_waveform_sample_range(
                content_rect,
                viewport,
                timeline_audio_preview_range(audio_input_state),
                samples.len(),
            ),
        );
    }

    let duration_seconds = timeline_audio_preview_duration_seconds(audio_input_state);
    let transcription_head_seconds = audio_input_state.runtime.transcription_head_seconds;
    let max_chunk_end_seconds = (transcription_head_seconds
        + TIMELINE_TRANSCRIPTION_MAX_CHUNK_SECONDS)
        .min(duration_seconds);
    let inactivity_end_seconds = (transcription_head_seconds
        + TIMELINE_TRANSCRIPTION_INACTIVITY_SECONDS)
        .min(duration_seconds);

    push_head_region(
        scene,
        preview_rect,
        duration_seconds,
        transcription_head_seconds,
        max_chunk_end_seconds,
        [0.72, 0.44, 1.0, 0.18],
    );
    push_head_region(
        scene,
        preview_rect,
        duration_seconds,
        transcription_head_seconds,
        inactivity_end_seconds,
        [0.96, 0.72, 1.0, 0.30],
    );
    push_timeline_head(
        scene,
        preview_rect,
        duration_seconds,
        transcription_head_seconds,
        [0.85, 0.56, 1.0, 1.0],
    );
}

fn push_timeline_transport_panel(
    scene: &mut RenderScene,
    layout: TimelineDocumentLayout,
    audio_input_state: Option<&AudioInputDeviceWindowState>,
    visual_state: TimelineDocumentVisualState,
) {
    let rect = timeline_transport_play_button_rect(layout);
    push_panel_with_data(
        scene,
        rect.to_win32_rect(),
        if audio_input_state.is_some_and(AudioInputDeviceWindowState::is_playing) {
            [0.16, 0.44, 0.58, 1.0]
        } else {
            [0.13, 0.21, 0.28, 1.0]
        },
        PanelEffect::PlaybackButton,
        [
            if audio_input_state.is_some_and(AudioInputDeviceWindowState::is_playing) {
                1.0
            } else {
                0.0
            },
            if visual_state.transport_play_button.hovered {
                1.0
            } else {
                0.0
            },
            if visual_state.transport_play_button.pressed {
                1.0
            } else {
                0.0
            },
            audio_input_state.map_or(1.0_f32, |state| {
                playback_speed_shader_value(state.runtime.playback.speed)
            }),
        ],
    );
}

#[expect(
    clippy::too_many_lines,
    reason = "the timeline track-list panel keeps row-specific icon, metadata, and per-track controls together so the new text-track wiring stays local"
)]
fn push_timeline_track_list_panel(
    scene: &mut RenderScene,
    layout: TimelineDocumentLayout,
    document: Option<&TimelineDocument>,
    audio_input_state: Option<&AudioInputDeviceWindowState>,
    visual_state: TimelineDocumentVisualState,
) {
    // timeline[impl track.microphone-row]
    let Some(document) = document else {
        let track_text = timeline_track_list_text(None);
        push_text_block(
            scene,
            layout.track_text_rect.to_win32_rect(),
            &track_text,
            9,
            16,
            [0.84, 0.88, 0.92, 1.0],
        );
        return;
    };

    if document.tracks().is_empty() {
        let track_text = timeline_track_list_text(Some(document));
        push_text_block(
            scene,
            layout.track_text_rect.to_win32_rect(),
            &track_text,
            9,
            16,
            [0.84, 0.88, 0.92, 1.0],
        );
        return;
    }

    for (index, track) in document.tracks().iter().enumerate() {
        let Some(row_rect) = timeline_track_list_row_rect(layout, index) else {
            continue;
        };
        push_panel(
            scene,
            row_rect.to_win32_rect(),
            [0.10, 0.11, 0.14, 0.92],
            PanelEffect::SceneButtonCard,
        );

        if let Some(handle_rect) = timeline_track_reorder_handle_rect(layout, index) {
            push_track_reorder_handle(scene, handle_rect);
        }

        let icon_rect = ClientRect::new(
            row_rect.left() + 34,
            row_rect.top() + 12,
            row_rect.left() + 62,
            row_rect.top() + 40,
        );
        push_sprite(
            scene,
            icon_rect.to_win32_rect(),
            [0.92, 0.95, 0.98, 1.0],
            match track.kind() {
                TimelineTrackKind::Audio => SpriteId::WindowsAudio,
                TimelineTrackKind::Transcription => SpriteId::Transcription,
                TimelineTrackKind::Text => SpriteId::Terminal,
                TimelineTrackKind::TracingSpans => SpriteId::FileAudio,
            },
        );

        let detail_right = if track.kind() == TimelineTrackKind::Audio {
            push_timeline_audio_track_controls(
                scene,
                layout,
                index,
                audio_input_state,
                visual_state,
            )
            .map_or(row_rect.right() - 12, |rect| rect.left() - 10)
        } else if track.kind() == TimelineTrackKind::Transcription {
            push_timeline_transcription_track_controls(
                scene,
                layout,
                index,
                audio_input_state,
                visual_state,
            )
            .map_or(row_rect.right() - 12, |rect| rect.left() - 10)
        } else {
            row_rect.right() - 12
        };
        push_text_block(
            scene,
            ClientRect::new(
                icon_rect.right() + 10,
                row_rect.top() + 10,
                detail_right,
                row_rect.top() + 30,
            )
            .to_win32_rect(),
            track.name(),
            9,
            16,
            [0.88, 0.91, 0.95, 1.0],
        );
        push_text_block(
            scene,
            ClientRect::new(
                icon_rect.right() + 10,
                row_rect.top() + 32,
                detail_right,
                row_rect.bottom() - 10,
            )
            .to_win32_rect(),
            &timeline_track_row_detail_text(document, track, audio_input_state),
            8,
            14,
            [0.68, 0.74, 0.80, 1.0],
        );
    }
}

fn timeline_track_row_detail_text(
    document: &TimelineDocument,
    track: &TimelineTrack,
    audio_input_state: Option<&AudioInputDeviceWindowState>,
) -> String {
    if track.kind() == TimelineTrackKind::Text {
        let block_count = document_text_block_count(document, track.id());
        return format!(
            "{} block{} · hover to preview",
            block_count,
            if block_count == 1 { "" } else { "s" }
        );
    }

    if track.kind() != TimelineTrackKind::Audio {
        return track.detail_line();
    }

    let Some(audio_input_state) = audio_input_state else {
        return track.detail_line();
    };

    if track.kind() == TimelineTrackKind::Transcription {
        return format!(
            "chunk {:.1}s  head {:.2}s{}",
            audio_input_state.runtime.transcription.chunk_seconds,
            audio_input_state.runtime.transcription_head_seconds,
            if audio_input_state.runtime.transcription.enabled {
                "  enabled"
            } else {
                "  paused"
            }
        );
    }

    format!(
        "{}  rec {:.2}s  play {:.2}s{}{}",
        audio_input_state.device.state,
        audio_input_state.runtime.recording_head_seconds,
        audio_input_state.runtime.playback.head_seconds,
        if audio_input_state.is_recording() {
            "  recording"
        } else {
            ""
        },
        if audio_input_state.loopback_enabled {
            "  loopback"
        } else {
            ""
        }
    )
}

fn push_timeline_audio_track_controls(
    scene: &mut RenderScene,
    layout: TimelineDocumentLayout,
    row_index: usize,
    audio_input_state: Option<&AudioInputDeviceWindowState>,
    visual_state: TimelineDocumentVisualState,
) -> Option<ClientRect> {
    let playback_rect = timeline_track_playback_button_rect(layout, row_index)?;
    let loopback_rect = timeline_track_loopback_button_rect(layout, row_index)?;
    let record_rect = timeline_track_record_button_rect(layout, row_index)?;

    push_panel_with_data(
        scene,
        playback_rect.to_win32_rect(),
        if audio_input_state.is_some_and(AudioInputDeviceWindowState::is_playing) {
            [0.20, 0.56, 0.86, 1.0]
        } else {
            [0.14, 0.28, 0.38, 1.0]
        },
        PanelEffect::PlaybackButton,
        [
            if audio_input_state.is_some_and(AudioInputDeviceWindowState::is_playing) {
                1.0
            } else {
                0.0
            },
            if visual_state.microphone_playback_hovered {
                1.0
            } else {
                0.0
            },
            if visual_state.microphone_playback_pressed {
                1.0
            } else {
                0.0
            },
            audio_input_state.map_or(1.0_f32, |state| {
                playback_speed_shader_value(state.runtime.playback.speed)
            }),
        ],
    );
    push_panel_with_data(
        scene,
        loopback_rect.to_win32_rect(),
        if audio_input_state.is_some_and(|state| state.loopback_enabled) {
            [0.20, 0.54, 0.46, 1.0]
        } else {
            [0.13, 0.24, 0.22, 1.0]
        },
        PanelEffect::LoopbackButton,
        [
            if audio_input_state.is_some_and(|state| state.loopback_enabled) {
                1.0
            } else {
                0.0
            },
            if visual_state.microphone_loopback_hovered {
                1.0
            } else {
                0.0
            },
            if visual_state.microphone_loopback_pressed {
                1.0
            } else {
                0.0
            },
            0.0,
        ],
    );
    push_panel_with_data(
        scene,
        record_rect.to_win32_rect(),
        if audio_input_state.is_some_and(AudioInputDeviceWindowState::is_recording) {
            [0.92, 0.09, 0.07, 1.0]
        } else if audio_input_state.is_some_and(|state| state.armed_for_record) {
            [0.44, 0.05, 0.05, 1.0]
        } else {
            [0.18, 0.05, 0.05, 1.0]
        },
        PanelEffect::RecordArmButton,
        [
            if audio_input_state.is_some_and(AudioInputDeviceWindowState::is_recording) {
                1.0
            } else {
                0.0
            },
            if audio_input_state.is_some_and(|state| state.armed_for_record) {
                1.0
            } else {
                0.0
            },
            0.0,
            0.0,
        ],
    );

    Some(playback_rect)
}

fn push_timeline_transcription_track_controls(
    scene: &mut RenderScene,
    layout: TimelineDocumentLayout,
    row_index: usize,
    audio_input_state: Option<&AudioInputDeviceWindowState>,
    visual_state: TimelineDocumentVisualState,
) -> Option<ClientRect> {
    let toggle_rect = timeline_track_transcription_toggle_button_rect(layout, row_index)?;
    let settings_rect = timeline_track_transcription_settings_button_rect(layout, row_index)?;

    push_panel_with_data(
        scene,
        settings_rect.to_win32_rect(),
        [0.18, 0.22, 0.28, 1.0],
        PanelEffect::GearButton,
        [
            if visual_state.transcription_settings_hovered {
                1.0
            } else {
                0.0
            },
            if visual_state.transcription_settings_pressed {
                1.0
            } else {
                0.0
            },
            0.0,
            0.0,
        ],
    );
    push_panel_with_data(
        scene,
        toggle_rect.to_win32_rect(),
        if audio_input_state.is_some_and(|state| state.runtime.transcription.enabled) {
            [0.70, 0.45, 0.18, 1.0]
        } else {
            [0.26, 0.18, 0.14, 1.0]
        },
        PanelEffect::TranscriptionToggle,
        [
            if audio_input_state.is_some_and(|state| state.runtime.transcription.enabled) {
                1.0
            } else {
                0.0
            },
            if visual_state.transcription_toggle_hovered {
                1.0
            } else {
                0.0
            },
            if visual_state.transcription_toggle_pressed {
                1.0
            } else {
                0.0
            },
            0.0,
        ],
    );

    Some(toggle_rect)
}

fn push_timeline_viewport_controls(
    scene: &mut RenderScene,
    layout: TimelineDocumentLayout,
    visual_state: TimelineDocumentVisualState,
) {
    let controls = [
        (
            SceneAction::PanTimelineLeft,
            visual_state.pan_left_button,
            [0.20, 0.26, 0.34, 1.0],
        ),
        (
            SceneAction::PanTimelineRight,
            visual_state.pan_right_button,
            [0.20, 0.26, 0.34, 1.0],
        ),
        (
            SceneAction::ZoomTimelineIn,
            visual_state.zoom_in_button,
            [0.24, 0.34, 0.24, 1.0],
        ),
        (
            SceneAction::ZoomTimelineOut,
            visual_state.zoom_out_button,
            [0.34, 0.22, 0.22, 1.0],
        ),
        (
            SceneAction::SelectTimelineTool,
            visual_state.select_tool_button,
            [0.38, 0.30, 0.16, 1.0],
        ),
        (
            SceneAction::SelectTimelineBrush,
            visual_state.brush_tool_button,
            [0.44, 0.30, 0.10, 1.0],
        ),
    ];

    for (action, button_state, color) in controls {
        let Some(rect) = timeline_viewport_control_rect(layout, action) else {
            continue;
        };
        let Some(label) = timeline_viewport_control_label(action) else {
            continue;
        };
        push_panel_with_data(
            scene,
            rect.to_win32_rect(),
            color,
            PanelEffect::SceneButtonCard,
            button_state.shader_data(),
        );
        push_centered_text(scene, rect.to_win32_rect(), label, [0.92, 0.95, 0.98, 1.0]);
    }
}

pub fn timeline_track_row_rect(
    scrollport_rect: ClientRect,
    row_index: usize,
    vertical_scroll_offset: i32,
) -> Option<ClientRect> {
    let world_top = timeline_track_row_world_top(row_index);
    let world_bottom = world_top.saturating_add(TIMELINE_TRACK_ROW_HEIGHT);
    let top = scrollport_rect.top() + world_top - vertical_scroll_offset;
    let bottom = scrollport_rect.top() + world_bottom - vertical_scroll_offset;
    let visible_top = top.max(scrollport_rect.top());
    let visible_bottom = bottom.min(scrollport_rect.bottom());
    (visible_bottom > visible_top).then(|| {
        ClientRect::new(
            scrollport_rect.left(),
            visible_top,
            scrollport_rect.right(),
            visible_bottom,
        )
    })
}

#[must_use]
pub fn timeline_track_list_row_rect(
    layout: TimelineDocumentLayout,
    row_index: usize,
) -> Option<ClientRect> {
    let world_top = timeline_track_row_world_top(row_index);
    let world_bottom = world_top.saturating_add(TIMELINE_TRACK_ROW_HEIGHT);
    let top = layout.track_text_rect.top() + world_top;
    let bottom = layout.track_text_rect.top() + world_bottom;
    let visible_top = top.max(layout.track_text_rect.top());
    let visible_bottom = bottom.min(layout.track_text_rect.bottom());
    (visible_bottom > visible_top).then(|| {
        ClientRect::new(
            layout.track_text_rect.left(),
            visible_top,
            layout.track_text_rect.right(),
            visible_bottom,
        )
    })
}

#[must_use]
pub fn timeline_track_record_button_rect(
    layout: TimelineDocumentLayout,
    row_index: usize,
) -> Option<ClientRect> {
    timeline_track_control_button_rect(layout, row_index, 0)
}

#[must_use]
pub fn timeline_track_loopback_button_rect(
    layout: TimelineDocumentLayout,
    row_index: usize,
) -> Option<ClientRect> {
    timeline_track_control_button_rect(layout, row_index, 1)
}

#[must_use]
pub fn timeline_track_playback_button_rect(
    layout: TimelineDocumentLayout,
    row_index: usize,
) -> Option<ClientRect> {
    timeline_track_control_button_rect(layout, row_index, 2)
}

#[must_use]
pub fn timeline_track_transcription_toggle_button_rect(
    layout: TimelineDocumentLayout,
    row_index: usize,
) -> Option<ClientRect> {
    timeline_track_control_button_rect(layout, row_index, 1)
}

#[must_use]
pub fn timeline_track_transcription_settings_button_rect(
    layout: TimelineDocumentLayout,
    row_index: usize,
) -> Option<ClientRect> {
    timeline_track_control_button_rect(layout, row_index, 0)
}

#[must_use]
pub fn timeline_track_reorder_handle_rect(
    layout: TimelineDocumentLayout,
    row_index: usize,
) -> Option<ClientRect> {
    timeline_track_list_row_rect(layout, row_index).map(|row_rect| {
        let handle_width = 16;
        let handle_height = (row_rect.height() - 22).clamp(28, 38);
        let left = row_rect.left() + 12;
        let top = row_rect.top() + ((row_rect.height() - handle_height) / 2);
        ClientRect::new(left, top, left + handle_width, top + handle_height)
    })
}

fn timeline_track_control_button_rect(
    layout: TimelineDocumentLayout,
    row_index: usize,
    slot_from_right: i32,
) -> Option<ClientRect> {
    timeline_track_list_row_rect(layout, row_index).map(|row_rect| {
        let button_size = (row_rect.height() - 24).clamp(28, 40);
        let gap = 8;
        let right = row_rect.right() - 12 - (slot_from_right * (button_size + gap));
        let top = row_rect.top() + ((row_rect.height() - button_size) / 2);
        ClientRect::new(right - button_size, top, right, top + button_size)
    })
}

fn push_track_reorder_handle(scene: &mut RenderScene, handle_rect: ClientRect) {
    for offset in [0, 1, 2] {
        let top = handle_rect.top() + 6 + (offset * 8);
        push_panel(
            scene,
            ClientRect::new(
                handle_rect.left() + 4,
                top,
                handle_rect.right() - 4,
                top + 3,
            )
            .to_win32_rect(),
            [0.62, 0.68, 0.74, 0.95],
            PanelEffect::SceneButtonCard,
        );
    }
}

fn timeline_track_preview_clip_rect(
    row_rect: ClientRect,
    content_rect: ClientRect,
    viewport: TimelineViewport,
    preview_range: TimelineTimeRangeNs,
) -> Option<ClientRect> {
    timeline_time_range_clip_rect(row_rect, content_rect, viewport, preview_range, 8)
}

fn timeline_track_preview_time_range(
    track: &TimelineTrack,
    audio_input_state: Option<&AudioInputDeviceWindowState>,
) -> TimelineTimeRangeNs {
    match track.kind() {
        TimelineTrackKind::Audio | TimelineTrackKind::Transcription | TimelineTrackKind::Text => {
            audio_input_state.map_or(track.preview_range(), timeline_audio_preview_range)
        }
        TimelineTrackKind::TracingSpans => track.preview_range(),
    }
}

pub fn timeline_time_range_clip_rect(
    row_rect: ClientRect,
    content_rect: ClientRect,
    viewport: TimelineViewport,
    time_range: TimelineTimeRangeNs,
    min_visible_width: i32,
) -> Option<ClientRect> {
    let projected = viewport.project_range(time_range);
    let content_width_pixels = content_rect.width().max(0);
    let start_pixels = projected.start().pixels();
    let end_pixels = projected.end().pixels();
    if end_pixels <= 0.0 || start_pixels >= f64::from(content_width_pixels) {
        return None;
    }

    let clip_top = row_rect.top() + 28;
    let clip_bottom = row_rect.bottom() - 10;
    if clip_bottom <= clip_top {
        return None;
    }

    let unclipped_left = content_rect
        .left()
        .saturating_add(f64_to_i32_saturating(start_pixels));
    let unclipped_right = content_rect
        .left()
        .saturating_add(f64_to_i32_saturating(end_pixels));
    let clip_left = unclipped_left.clamp(content_rect.left(), content_rect.right());
    let mut clip_right = unclipped_right.clamp(content_rect.left(), content_rect.right());
    let clipped_by_viewport =
        unclipped_left < content_rect.left() || unclipped_right > content_rect.right();
    if !clipped_by_viewport && clip_right < clip_left + min_visible_width {
        clip_right = (clip_left + min_visible_width).min(content_rect.right());
    }
    if clip_right <= clip_left {
        return None;
    }

    Some(ClientRect::new(
        clip_left,
        clip_top,
        clip_right,
        clip_bottom,
    ))
}

fn document_text_block_count(
    document: &TimelineDocument,
    track_id: crate::timeline::TimelineTrackId,
) -> usize {
    document
        .text_blocks()
        .iter()
        .filter(|block| block.track_id() == track_id)
        .count()
}

fn timeline_audio_preview_range(
    audio_input_state: &AudioInputDeviceWindowState,
) -> TimelineTimeRangeNs {
    let duration_seconds = timeline_audio_preview_duration_seconds(audio_input_state);
    TimelineTimeRangeNs::new(
        TimelineTimeNs::ZERO,
        TimelineTimeNs::from_duration(Time::new::<second>(duration_seconds)),
    )
}

fn timeline_viewport_visible_time_range(
    content_rect: ClientRect,
    viewport: TimelineViewport,
) -> TimelineTimeRangeNs {
    TimelineTimeRangeNs::new(
        viewport.origin(),
        viewport.x_to_time(TimelineViewportPoint::new_pixels(f64::from(
            content_rect.width().max(0),
        ))),
    )
}

fn timeline_time_range_intersection(
    left: TimelineTimeRangeNs,
    right: TimelineTimeRangeNs,
) -> Option<TimelineTimeRangeNs> {
    let start = left.start().max(right.start());
    let end = left.end().min(right.end());
    (start < end).then_some(TimelineTimeRangeNs::new(start, end))
}

fn timeline_waveform_sample_range(
    content_rect: ClientRect,
    viewport: TimelineViewport,
    preview_range: TimelineTimeRangeNs,
    sample_count: usize,
) -> Range<usize> {
    let visible_range = timeline_time_range_intersection(
        preview_range,
        timeline_viewport_visible_time_range(content_rect, viewport),
    )
    .unwrap_or(preview_range);
    waveform_visible_sample_range(sample_count, preview_range, visible_range)
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    reason = "waveform sample windows are proportional projections from typed time ranges into sample indices"
)]
fn waveform_visible_sample_range(
    sample_count: usize,
    full_range: TimelineTimeRangeNs,
    visible_range: TimelineTimeRangeNs,
) -> Range<usize> {
    if sample_count == 0 {
        return 0..0;
    }

    let full_duration_i64 = full_range
        .end()
        .as_i64()
        .saturating_sub(full_range.start().as_i64());
    let full_duration = full_duration_i64.max(1) as f64;
    let start_offset = (visible_range.start().as_i64() - full_range.start().as_i64())
        .clamp(0, full_duration_i64) as f64;
    let end_offset = (visible_range.end().as_i64() - full_range.start().as_i64())
        .clamp(0, full_duration_i64) as f64;
    let start = ((start_offset / full_duration) * sample_count as f64).floor() as usize;
    let end = (((end_offset / full_duration) * sample_count as f64).ceil() as usize)
        .max(start.saturating_add(1))
        .min(sample_count);
    start.min(end)..end
}

fn timeline_audio_preview_duration_seconds(audio_input_state: &AudioInputDeviceWindowState) -> f64 {
    audio_input_state
        .runtime
        .duration_seconds()
        .max(audio_input_state.runtime.recording_head_seconds)
        .max(audio_input_state.runtime.playback.head_seconds)
        .max(audio_input_state.runtime.transcription_head_seconds)
        .max(0.0)
}

fn timeline_track_preview_color(kind: TimelineTrackKind) -> [f32; 4] {
    match kind {
        TimelineTrackKind::Audio => [0.19, 0.55, 0.44, 0.95],
        TimelineTrackKind::Transcription => [0.45, 0.28, 0.66, 0.90],
        TimelineTrackKind::Text => [0.76, 0.63, 0.20, 0.94],
        TimelineTrackKind::TracingSpans => [0.70, 0.47, 0.26, 0.95],
    }
}

fn timeline_track_row_world_top(row_index: usize) -> i32 {
    let row_index = i32::try_from(row_index).unwrap_or(i32::MAX);
    row_index.saturating_mul(TIMELINE_TRACK_ROW_HEIGHT + TIMELINE_TRACK_ROW_GAP)
}

fn timeline_audio_head_x(
    layout: TimelineDocumentLayout,
    viewport: TimelineViewport,
    seconds: f64,
) -> i32 {
    let time = TimelineTimeNs::from_duration(Time::new::<second>(seconds.max(0.0)));
    let pixels = viewport.time_to_x(time).pixels().round();
    layout.time_axis_rect.left()
        + f64_to_i32_clamped(pixels, 0, layout.time_axis_rect.width().max(0))
}

#[must_use]
pub fn timeline_audio_head_grabbers(
    layout: TimelineDocumentLayout,
    viewport: TimelineViewport,
    audio_input_state: &AudioInputDeviceWindowState,
) -> Vec<TimelineAudioHeadGrabberLayout> {
    let mut heads = vec![
        (
            AudioInputTimelineHeadKind::Recording,
            timeline_audio_head_x(
                layout,
                viewport,
                audio_input_state.runtime.recording_head_seconds,
            ),
        ),
        (
            AudioInputTimelineHeadKind::Playback,
            timeline_audio_head_x(
                layout,
                viewport,
                audio_input_state.runtime.playback.head_seconds,
            ),
        ),
        (
            AudioInputTimelineHeadKind::Transcription,
            timeline_audio_head_x(
                layout,
                viewport,
                audio_input_state.runtime.transcription_head_seconds,
            ),
        ),
    ];
    heads.sort_by_key(|(kind, x)| (*x, head_kind_sort_key(*kind)));
    let grabber_size: i32 = 14;
    let vertical_gap: i32 = 4;
    let threshold = (grabber_size + 2).unsigned_abs();
    let mut previous_x = i32::MIN;
    let mut stack_index = 0;
    heads
        .into_iter()
        .map(|(kind, x)| {
            if previous_x != i32::MIN && x.abs_diff(previous_x) <= threshold {
                stack_index += 1;
            } else {
                stack_index = 0;
                previous_x = x;
            }
            let top = layout.time_axis_rect.top() + 8 + stack_index * (grabber_size + vertical_gap);
            TimelineAudioHeadGrabberLayout {
                kind,
                rect: ClientRect::new(x - 7, top, x + 7, top + grabber_size),
            }
        })
        .collect()
}

fn push_timeline_audio_heads(
    scene: &mut RenderScene,
    layout: TimelineDocumentLayout,
    viewport: TimelineViewport,
    audio_input_state: &AudioInputDeviceWindowState,
    visual_state: TimelineDocumentVisualState,
) {
    // timeline[impl heads.recording-playback]
    for (_kind, seconds, color) in [
        (
            AudioInputTimelineHeadKind::Recording,
            audio_input_state.runtime.recording_head_seconds,
            timeline_head_color(AudioInputTimelineHeadKind::Recording),
        ),
        (
            AudioInputTimelineHeadKind::Playback,
            audio_input_state.runtime.playback.head_seconds,
            timeline_head_color(AudioInputTimelineHeadKind::Playback),
        ),
        (
            AudioInputTimelineHeadKind::Transcription,
            audio_input_state.runtime.transcription_head_seconds,
            timeline_head_color(AudioInputTimelineHeadKind::Transcription),
        ),
    ] {
        let x = timeline_audio_head_x(layout, viewport, seconds);
        push_panel(
            scene,
            ClientRect::new(
                x - 1,
                layout.time_axis_rect.top(),
                x + 2,
                layout.scrollport_rect.bottom(),
            )
            .to_win32_rect(),
            color,
            PanelEffect::TerminalFill,
        );
    }

    for grabber in timeline_audio_head_grabbers(layout, viewport, audio_input_state) {
        let active = match grabber.kind {
            AudioInputTimelineHeadKind::Recording => audio_input_state.is_recording(),
            AudioInputTimelineHeadKind::Playback => audio_input_state.is_playing(),
            AudioInputTimelineHeadKind::Transcription => false,
        };
        push_panel_with_data(
            scene,
            grabber.rect.to_win32_rect(),
            timeline_head_color(grabber.kind),
            PanelEffect::TimelineHeadGrabber,
            [
                if active { 1.0 } else { 0.0 },
                if visual_state.hovered_head == Some(grabber.kind) {
                    1.0
                } else {
                    0.0
                },
                if visual_state.grabbed_head == Some(grabber.kind) {
                    1.0
                } else {
                    0.0
                },
                head_kind_shader_index(grabber.kind),
            ],
        );
    }
}

fn timeline_visible_selection_rect(
    layout: TimelineDocumentLayout,
    viewport: TimelineViewport,
    selection: TimelineRectSelection,
    vertical_scroll_offset: i32,
    track_count: usize,
) -> Option<ClientRect> {
    let total_track_height = timeline_track_world_height(track_count);
    if total_track_height <= 0 {
        return None;
    }

    let (left, right) =
        timeline_visible_selection_x_bounds(layout, viewport, selection.time_range())?;
    let top_world = selection.track_y_range().top().clamp(0, total_track_height);
    let bottom_world = selection
        .track_y_range()
        .bottom()
        .clamp(0, total_track_height);
    let top = layout.scrollport_rect.top() + top_world - vertical_scroll_offset;
    let bottom = layout.scrollport_rect.top() + bottom_world - vertical_scroll_offset;
    let visible_top = top.max(layout.scrollport_rect.top());
    let visible_bottom = bottom.min(layout.scrollport_rect.bottom());
    (right > left && visible_bottom > visible_top)
        .then(|| ClientRect::new(left, visible_top, right, visible_bottom))
}

fn timeline_visible_selection_time_band_rect(
    layout: TimelineDocumentLayout,
    viewport: TimelineViewport,
    selection: TimelineRectSelection,
) -> Option<ClientRect> {
    let (left, right) =
        timeline_visible_selection_x_bounds(layout, viewport, selection.time_range())?;
    (right > left).then(|| {
        ClientRect::new(
            left,
            layout.time_axis_rect.top() + 8,
            right,
            layout.time_axis_rect.bottom() - 8,
        )
    })
}

fn timeline_visible_selection_x_bounds(
    layout: TimelineDocumentLayout,
    viewport: TimelineViewport,
    time_range: TimelineTimeRangeNs,
) -> Option<(i32, i32)> {
    let projected = viewport.project_range(time_range);
    let axis_width = layout.time_axis_rect.width().max(0);
    let start = projected.start().pixels();
    let end = projected.end().pixels();
    if end <= 0.0 || start >= f64::from(axis_width) {
        return None;
    }

    let left = layout.time_axis_rect.left() + f64_to_i32_clamped(start.round(), 0, axis_width);
    let right = layout.time_axis_rect.left() + f64_to_i32_clamped(end.round(), 0, axis_width);
    Some((left.min(right), right.max(left)))
}

// timeline[impl selection.rectangle]
// timeline[impl selection.ruler-all-tracks]
fn push_timeline_selection_overlay(scene: &mut RenderScene, rect: ClientRect) {
    push_panel(
        scene,
        rect.to_win32_rect(),
        TIMELINE_SELECTION_FILL,
        PanelEffect::TerminalFill,
    );
    for border_rect in outline_rects(rect) {
        push_panel(
            scene,
            border_rect.to_win32_rect(),
            TIMELINE_SELECTION_BORDER,
            PanelEffect::SceneButtonCard,
        );
    }
}

// timeline[impl viewport.scrollbars]
fn push_timeline_scrollbars(
    scene: &mut RenderScene,
    layout: TimelineDocumentLayout,
    document: Option<&TimelineDocument>,
    viewport: TimelineViewport,
    selection: Option<TimelineRectSelection>,
    vertical_scroll_offset: i32,
) {
    push_panel(
        scene,
        layout.horizontal_scrollbar_rect.to_win32_rect(),
        TIMELINE_SCROLLBAR_TRACK,
        PanelEffect::SceneButtonCard,
    );
    if let Some(thumb_rect) =
        timeline_horizontal_scrollbar_thumb_rect(layout, document, viewport, selection)
    {
        push_panel(
            scene,
            thumb_rect.to_win32_rect(),
            TIMELINE_SCROLLBAR_THUMB,
            PanelEffect::TerminalFill,
        );
    }

    push_panel(
        scene,
        layout.vertical_scrollbar_rect.to_win32_rect(),
        TIMELINE_SCROLLBAR_TRACK,
        PanelEffect::SceneButtonCard,
    );
    if let Some(thumb_rect) = timeline_vertical_scrollbar_thumb_rect(
        layout,
        document.map_or(0, |document| document.tracks().len()),
        vertical_scroll_offset,
    ) {
        push_panel(
            scene,
            thumb_rect.to_win32_rect(),
            TIMELINE_SCROLLBAR_THUMB,
            PanelEffect::TerminalFill,
        );
    }
}

fn timeline_horizontal_scrollbar_thumb_rect(
    layout: TimelineDocumentLayout,
    document: Option<&TimelineDocument>,
    viewport: TimelineViewport,
    selection: Option<TimelineRectSelection>,
) -> Option<ClientRect> {
    let scrollbar_rect = layout.horizontal_scrollbar_rect;
    if scrollbar_rect.width() <= 0 || scrollbar_rect.height() <= 0 {
        return None;
    }

    let visible_width = layout.time_axis_rect.width().max(1);
    let visible_end =
        viewport.x_to_time(TimelineViewportPoint::new_pixels(f64::from(visible_width)));
    let visible_range = TimelineTimeRangeNs::new(viewport.origin(), visible_end);
    let total_range = timeline_document_time_bounds(document, visible_range, selection);
    let total_duration = (total_range.end().as_i64() - total_range.start().as_i64()).max(1);
    let visible_duration = (visible_range.end().as_i64() - visible_range.start().as_i64()).max(1);
    let track_width = scrollbar_rect.width().max(1);
    let min_thumb_width = TIMELINE_SCROLLBAR_MIN_THUMB.min(track_width);
    let proportional_thumb = ((i128::from(track_width) * i128::from(visible_duration))
        / i128::from(total_duration))
    .clamp(i128::from(min_thumb_width), i128::from(track_width));
    let thumb_width = i32::try_from(proportional_thumb).unwrap_or(track_width);
    let travel = (track_width - thumb_width).max(0);
    let available_duration = (total_duration - visible_duration).max(0);
    let offset_duration = (visible_range.start().as_i64() - total_range.start().as_i64()).max(0);
    let thumb_offset = if available_duration == 0 || travel == 0 {
        0
    } else {
        i32::try_from(
            (i128::from(travel) * i128::from(offset_duration)) / i128::from(available_duration),
        )
        .unwrap_or(0)
    };
    let thumb_left = scrollbar_rect.left() + thumb_offset;
    Some(ClientRect::new(
        thumb_left,
        scrollbar_rect.top(),
        (thumb_left + thumb_width).min(scrollbar_rect.right()),
        scrollbar_rect.bottom(),
    ))
}

fn timeline_vertical_scrollbar_thumb_rect(
    layout: TimelineDocumentLayout,
    track_count: usize,
    vertical_scroll_offset: i32,
) -> Option<ClientRect> {
    let scrollbar_rect = layout.vertical_scrollbar_rect;
    if scrollbar_rect.width() <= 0 || scrollbar_rect.height() <= 0 {
        return None;
    }

    let visible_height = layout.scrollport_rect.height().max(1);
    let total_height = timeline_track_world_height(track_count).max(visible_height);
    let track_height = scrollbar_rect.height().max(1);
    let min_thumb_height = TIMELINE_SCROLLBAR_MIN_THUMB.min(track_height);
    let proportional_thumb = ((i128::from(track_height) * i128::from(visible_height))
        / i128::from(total_height.max(1)))
    .clamp(i128::from(min_thumb_height), i128::from(track_height));
    let thumb_height = i32::try_from(proportional_thumb).unwrap_or(track_height);
    let travel = (track_height - thumb_height).max(0);
    let available_height = (total_height - visible_height).max(0);
    let thumb_offset = if available_height == 0 || travel == 0 {
        0
    } else {
        i32::try_from(
            (i128::from(travel) * i128::from(vertical_scroll_offset.clamp(0, available_height)))
                / i128::from(available_height),
        )
        .unwrap_or(0)
    };
    let thumb_top = scrollbar_rect.top() + thumb_offset;
    Some(ClientRect::new(
        scrollbar_rect.left(),
        thumb_top,
        scrollbar_rect.right(),
        (thumb_top + thumb_height).min(scrollbar_rect.bottom()),
    ))
}

fn timeline_document_time_bounds(
    document: Option<&TimelineDocument>,
    visible_range: TimelineTimeRangeNs,
    selection: Option<TimelineRectSelection>,
) -> TimelineTimeRangeNs {
    let mut bounds = visible_range;
    if let Some(document) = document {
        for track in document.tracks() {
            bounds = union_time_ranges(bounds, track.preview_range());
        }
    }
    if let Some(selection) = selection {
        bounds = union_time_ranges(bounds, selection.time_range());
    }
    bounds
}

fn union_time_ranges(left: TimelineTimeRangeNs, right: TimelineTimeRangeNs) -> TimelineTimeRangeNs {
    TimelineTimeRangeNs::new(
        TimelineTimeNs::new(left.start().as_i64().min(right.start().as_i64())),
        TimelineTimeNs::new(left.end().as_i64().max(right.end().as_i64())),
    )
}

fn outline_rects(rect: ClientRect) -> [ClientRect; 4] {
    [
        ClientRect::new(rect.left(), rect.top(), rect.right(), rect.top() + 1),
        ClientRect::new(rect.left(), rect.bottom() - 1, rect.right(), rect.bottom()),
        ClientRect::new(rect.left(), rect.top(), rect.left() + 1, rect.bottom()),
        ClientRect::new(rect.right() - 1, rect.top(), rect.right(), rect.bottom()),
    ]
}

fn client_rect_intersection(left: ClientRect, right: ClientRect) -> Option<ClientRect> {
    let intersection = ClientRect::new(
        left.left().max(right.left()),
        left.top().max(right.top()),
        left.right().min(right.right()),
        left.bottom().min(right.bottom()),
    );
    (intersection.right() > intersection.left() && intersection.bottom() > intersection.top())
        .then_some(intersection)
}

fn timeline_track_list_text(document: Option<&TimelineDocument>) -> String {
    let mut track_text = String::from("Tracks\n\n");
    if let Some(document) = document {
        if document.tracks().is_empty() {
            track_text.push_str("No tracks yet.\n\nImport a .tracy capture or add audio later.");
        } else {
            for track in document.tracks() {
                let _ = writeln!(track_text, "{}\n  {}\n", track.name(), track.detail_line());
            }
        }
    } else {
        track_text.push_str("No timeline document loaded.");
    }
    track_text
}

fn timeline_content_text(document: Option<&TimelineDocument>) -> String {
    document.map_or_else(
        || "Blank timeline\n\nNo document loaded yet.".to_owned(),
        |document| {
            format!(
                "{}\n{}\n\nTracks: {}\nViewport origin: {} ns\nEdit list entries: {}",
                document.title(),
                document.subtitle(),
                document.tracks().len(),
                document.viewport().origin().as_i64(),
                document.edits().len(),
            )
        },
    )
}

fn push_timeline_ruler_tick(
    scene: &mut RenderScene,
    layout: TimelineDocumentLayout,
    tick: &TimelineRulerTick,
) {
    let tick_x = layout.time_axis_rect.left()
        + timeline_tick_pixel_offset(tick, layout.time_axis_rect.width());
    let tick_rect = ClientRect::new(
        tick_x,
        layout.time_axis_rect.top() + 6,
        (tick_x + 1).min(layout.time_axis_rect.right()),
        layout.time_axis_rect.bottom() - 6,
    );
    push_panel(
        scene,
        tick_rect.to_win32_rect(),
        [0.58, 0.63, 0.70, 0.9],
        PanelEffect::SceneButtonCard,
    );

    let grid_rect = ClientRect::new(
        tick_x,
        layout.scrollport_rect.top(),
        (tick_x + 1).min(layout.scrollport_rect.right()),
        layout.scrollport_rect.bottom(),
    );
    push_panel(
        scene,
        grid_rect.to_win32_rect(),
        [0.20, 0.23, 0.28, 0.65],
        PanelEffect::SceneButtonCard,
    );

    let label_rect = ClientRect::new(
        (tick_x - 40).max(layout.time_axis_rect.left() + 6),
        layout.time_axis_rect.top() + 8,
        (tick_x + 44).min(layout.time_axis_rect.right() - 6),
        layout.time_axis_rect.bottom() - 8,
    );
    push_text_block(
        scene,
        label_rect.to_win32_rect(),
        tick.label(),
        8,
        16,
        [0.80, 0.84, 0.90, 1.0],
    );
}

fn timeline_tick_pixel_offset(tick: &TimelineRulerTick, ruler_width_pixels: i32) -> i32 {
    let pixels = tick.x().pixels().round();
    if !pixels.is_finite() {
        return 0;
    }

    f64_to_i32_clamped(pixels, 0, ruler_width_pixels.max(0))
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "timeline ruler positions are rounded and clamped to the visible pixel range before conversion to i32"
)]
fn f64_to_i32_clamped(value: f64, min: i32, max: i32) -> i32 {
    value.clamp(f64::from(min), f64::from(max)) as i32
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "timeline preview projection rounds block edges into client pixels before clipping"
)]
fn f64_to_i32_saturating(value: f64) -> i32 {
    if value.is_nan() {
        0
    } else if value >= f64::from(i32::MAX) {
        i32::MAX
    } else if value <= f64::from(i32::MIN) {
        i32::MIN
    } else {
        value.round() as i32
    }
}

#[must_use]
// audio[impl gui.daemon-window]
#[expect(
    clippy::too_many_lines,
    reason = "the daemon window composes status panels, model controls, and CUDA actions"
)]
pub fn build_audio_daemon_render_scene(
    layout: TerminalLayout,
    window_chrome_buttons_state: WindowChromeButtonsState,
    status: &AudioTranscriptionDaemonStatusReport,
    visual_state: AudioDaemonVisualState,
) -> RenderScene {
    let mut scene = build_scene_shell(
        layout,
        SceneWindowKind::AudioDaemon,
        window_chrome_buttons_state,
    );
    let body_rect = layout.terminal_panel_rect().inset(24);
    push_panel(
        &mut scene,
        body_rect.to_win32_rect(),
        [0.10, 0.11, 0.15, 1.0],
        PanelEffect::SceneBody,
    );

    let title_rect = ClientRect::new(
        body_rect.left() + 18,
        body_rect.top() + 18,
        body_rect.right() - 18,
        body_rect.top() + 58,
    );
    push_title_text(
        &mut scene,
        title_rect.to_win32_rect(),
        "Audio Daemon",
        [0.98, 0.96, 1.0, 1.0],
    );

    let model_rect = ClientRect::new(
        body_rect.left() + 18,
        title_rect.bottom() + 18,
        body_rect.right() - 18,
        title_rect.bottom() + 112,
    );
    push_panel(
        &mut scene,
        model_rect.to_win32_rect(),
        [0.12, 0.14, 0.18, 1.0],
        PanelEffect::SceneButtonCard,
    );
    push_text_block(
        &mut scene,
        ClientRect::new(
            model_rect.left() + 16,
            model_rect.top() + 12,
            model_rect.right() - 16,
            model_rect.top() + 34,
        )
        .to_win32_rect(),
        &format!("Model: {}", status.selected_model),
        9,
        16,
        [0.92, 0.94, 0.98, 1.0],
    );
    for (index, model) in status.available_models.iter().enumerate() {
        let button_rect =
            audio_daemon_model_button_rect(body_rect, index, status.available_models.len());
        let active = model == &status.selected_model;
        push_panel_with_data(
            &mut scene,
            button_rect.to_win32_rect(),
            if active {
                [0.24, 0.38, 0.30, 1.0]
            } else {
                [0.17, 0.18, 0.23, 1.0]
            },
            PanelEffect::SceneButtonCard,
            ButtonVisualState {
                hovered: visual_state.hovered_model_index == Some(index),
                pressed: visual_state.pressed_model_index == Some(index),
                active,
                ..Default::default()
            }
            .shader_data(),
        );
        push_centered_text(
            &mut scene,
            button_rect.to_win32_rect(),
            model,
            if active {
                [0.90, 1.0, 0.92, 1.0]
            } else {
                [0.86, 0.88, 0.94, 1.0]
            },
        );
    }

    let contract_rect = ClientRect::new(
        body_rect.left() + 18,
        model_rect.bottom() + 16,
        body_rect.right() - 18,
        model_rect.bottom() + 162,
    );
    push_panel(
        &mut scene,
        contract_rect.to_win32_rect(),
        [0.13, 0.15, 0.19, 1.0],
        PanelEffect::SceneButtonCard,
    );
    push_text_block(
        &mut scene,
        contract_rect.inset(16).to_win32_rect(),
        &audio_daemon_contract_text(status),
        9,
        16,
        [0.92, 0.94, 0.98, 1.0],
    );

    let paths_rect = ClientRect::new(
        body_rect.left() + 18,
        contract_rect.bottom() + 16,
        body_rect.right() - 18,
        contract_rect.bottom() + 164,
    );
    push_panel(
        &mut scene,
        paths_rect.to_win32_rect(),
        [0.11, 0.14, 0.17, 1.0],
        PanelEffect::SceneButtonCard,
    );
    push_text_block(
        &mut scene,
        paths_rect.inset(16).to_win32_rect(),
        &audio_daemon_paths_text(status),
        9,
        16,
        [0.84, 0.90, 0.88, 1.0],
    );

    let controls_rect = ClientRect::new(
        body_rect.left() + 18,
        paths_rect.bottom() + 16,
        body_rect.right() - 18,
        body_rect.bottom() - 18,
    );
    push_panel(
        &mut scene,
        controls_rect.to_win32_rect(),
        [0.15, 0.12, 0.18, 1.0],
        PanelEffect::TerminalPanel,
    );
    push_text_block(
        &mut scene,
        controls_rect.inset(16).to_win32_rect(),
        &audio_daemon_control_text(status),
        9,
        16,
        [0.93, 0.88, 0.98, 1.0],
    );
    let cuda_rect = audio_daemon_cuda_check_button_rect(body_rect);
    push_panel_with_data(
        &mut scene,
        cuda_rect.to_win32_rect(),
        [0.16, 0.25, 0.35, 1.0],
        PanelEffect::SceneButtonCard,
        ButtonVisualState {
            hovered: visual_state.cuda_check_hovered,
            pressed: visual_state.cuda_check_pressed,
            ..Default::default()
        }
        .shader_data(),
    );
    push_centered_text(
        &mut scene,
        cuda_rect.to_win32_rect(),
        "CUDA Check",
        [0.90, 0.96, 1.0, 1.0],
    );

    scene
}

#[must_use]
// audio[impl transcription.model-selection]
pub fn audio_daemon_model_button_rect(
    body_rect: ClientRect,
    index: usize,
    model_count: usize,
) -> ClientRect {
    let model_count = i32::try_from(model_count.max(1)).unwrap_or(1);
    let gap = 8;
    let row_left = body_rect.left() + 34;
    let row_right = body_rect.right() - 34;
    let row_width = (row_right - row_left).max(1);
    let button_width = ((row_width - gap * (model_count - 1)) / model_count).max(48);
    let left = row_left + i32::try_from(index).unwrap_or_default() * (button_width + gap);
    let top = body_rect.top() + 88;
    ClientRect::new(left, top, (left + button_width).min(row_right), top + 36)
}

#[must_use]
// audio[impl transcription.cuda-check]
pub fn audio_daemon_cuda_check_button_rect(body_rect: ClientRect) -> ClientRect {
    ClientRect::new(
        body_rect.right() - 186,
        body_rect.bottom() - 70,
        body_rect.right() - 34,
        body_rect.bottom() - 34,
    )
}

fn audio_daemon_contract_text(status: &AudioTranscriptionDaemonStatusReport) -> String {
    format!(
        "Tensor contract\nshape: {} x {}\ndtype: {}\nvalues: {}\nbytes: {}\nslot bytes: {}\nslot pool: {} slots / {} bytes",
        status.tensor_mel_bins,
        status.tensor_frames,
        status.tensor_dtype,
        status.tensor_values,
        status.tensor_bytes,
        status.shared_memory_slot_bytes,
        status.shared_memory_minimum_slots,
        status.shared_memory_total_bytes,
    )
}

fn audio_daemon_paths_text(status: &AudioTranscriptionDaemonStatusReport) -> String {
    format!(
        "Paths\ndaemon source: {}\nuv venv: {}\nmodel cache: {}\npython entrypoint: {}",
        status.daemon_source_dir,
        status.uv_venv_dir,
        status.model_cache_dir,
        status.python_entrypoint,
    )
}

fn audio_daemon_control_text(status: &AudioTranscriptionDaemonStatusReport) -> String {
    format!(
        "Transports\ncontrol: {}\npayload: {}\nselected model: {}\nqueued requests: {}\noldest queued: {} ms\npython lag: {} ms\n\nAlt+X opens the full diagnostics TUI.",
        status.control_transport,
        status.payload_transport,
        status.selected_model,
        status.queued_request_count,
        status.oldest_queued_age_ms,
        status.python_lag_ms,
    )
}

#[must_use]
// windowing[impl demo-mode.input-device-identifier-scramble]
pub fn demo_mode_input_device_identifiers(scramble: bool) -> Vec<String> {
    if !scramble {
        return vec![
            "{0.0.1.00000000}.{6B24B7C8-8F5A-4B91-A784-2A27C1E8E001}".to_owned(),
            "{0.0.1.00000001}.{41F27D33-5A64-44D0-BE6C-2C1D9AE84002}".to_owned(),
            "{0.0.1.00000002}.{93CB0F4A-EB15-4E40-8F7C-9B8AB4E2D003}".to_owned(),
        ];
    }

    [
        b"teamy demo input device 0".as_slice(),
        b"teamy demo input device 1".as_slice(),
        b"teamy demo input device 2".as_slice(),
    ]
    .into_iter()
    .map(demo_mode_scrambled_input_device_identifier)
    .collect()
}

#[must_use]
pub fn demo_mode_scrambled_input_device_identifier(seed: &[u8]) -> String {
    let mut unstructured = arbitrary::Unstructured::new(seed);
    DemoInputDeviceIdentifier::arbitrary(&mut unstructured).map_or_else(
        |_| "{0.0.1.00000000}.{00000000-0000-0000-0000-000000000000}".to_owned(),
        |identifier| identifier.as_str().to_owned(),
    )
}

#[must_use]
pub fn input_device_identifier_display_text(identifier: &str, scramble: bool) -> String {
    // windowing[impl demo-mode.live-audio-device-scramble]
    if scramble {
        demo_mode_scrambled_input_device_identifier(identifier.as_bytes())
    } else {
        identifier.to_owned()
    }
}

#[must_use]
pub fn demo_mode_layout(body_rect: ClientRect) -> DemoModeLayout {
    let button_width = body_rect.width().clamp(180, 360);
    let button_left = body_rect.left() + ((body_rect.width() - button_width).max(0) / 2);
    let demo_button_bounds = ClientRect::new(
        button_left,
        body_rect.top() + 38,
        button_left + button_width,
        body_rect.top() + 102,
    );
    let scramble_toggle_bounds = ClientRect::new(
        body_rect.left() + 24,
        demo_button_bounds.bottom() + 34,
        body_rect.right() - 24,
        demo_button_bounds.bottom() + 84,
    );
    let scramble_toggle_track_bounds = ClientRect::new(
        scramble_toggle_bounds.left(),
        scramble_toggle_bounds.top() + 8,
        scramble_toggle_bounds.left() + 86,
        scramble_toggle_bounds.top() + 42,
    );
    let identifiers_bounds = ClientRect::new(
        body_rect.left() + 24,
        scramble_toggle_bounds.bottom() + 24,
        body_rect.right() - 24,
        body_rect.bottom() - 24,
    );

    DemoModeLayout {
        demo_button_bounds,
        scramble_toggle_bounds,
        scramble_toggle_track_bounds,
        identifiers_bounds,
    }
}

#[must_use]
pub fn demo_mode_toggle_thumb_bounds(
    layout: DemoModeLayout,
    scramble_input_device_identifiers: bool,
) -> ClientRect {
    let track = layout.scramble_toggle_track_bounds;
    let thumb_size = 28;
    let left = if scramble_input_device_identifiers {
        track.right() - thumb_size - 3
    } else {
        track.left() + 3
    };
    ClientRect::new(
        left,
        track.top() + 3,
        left + thumb_size,
        track.top() + 3 + thumb_size,
    )
}

#[must_use]
// windowing[impl demo-mode.window]
pub fn build_demo_mode_render_scene(
    layout: TerminalLayout,
    window_chrome_buttons_state: WindowChromeButtonsState,
    scramble_input_device_identifiers: bool,
    visual_state: DemoModeVisualState,
) -> RenderScene {
    let mut scene = build_scene_shell(
        layout,
        SceneWindowKind::DemoMode,
        window_chrome_buttons_state,
    );
    let body_rect = layout.terminal_panel_rect().inset(30);
    let demo_layout = demo_mode_layout(body_rect);

    push_panel_with_data(
        &mut scene,
        demo_layout.demo_button_bounds.to_win32_rect(),
        [0.13, 0.28, 0.30, 0.96],
        PanelEffect::SceneButtonCard,
        visual_state.demo_button.shader_data(),
    );
    push_centered_text(
        &mut scene,
        demo_layout.demo_button_bounds.to_win32_rect(),
        "Demo Mode",
        [0.96, 0.98, 1.0, 1.0],
    );

    push_panel_with_data(
        &mut scene,
        demo_layout.scramble_toggle_bounds.to_win32_rect(),
        [0.08, 0.09, 0.11, 0.92],
        PanelEffect::SceneButtonCard,
        visual_state.scramble_toggle.shader_data(),
    );
    // windowing[impl demo-mode.input-device-identifier-scramble]
    push_panel_with_data(
        &mut scene,
        demo_layout.scramble_toggle_track_bounds.to_win32_rect(),
        [0.16, 0.18, 0.20, 1.0],
        PanelEffect::DemoToggle,
        demo_mode_toggle_shader_data(
            visual_state.scramble_toggle,
            scramble_input_device_identifiers,
        ),
    );
    let thumb_bounds =
        demo_mode_toggle_thumb_bounds(demo_layout, scramble_input_device_identifiers);
    push_panel(
        &mut scene,
        thumb_bounds.to_win32_rect(),
        if scramble_input_device_identifiers {
            [0.82, 1.00, 0.94, 1.0]
        } else {
            [1.00, 0.68, 0.66, 1.0]
        },
        PanelEffect::TerminalFill,
    );
    push_centered_text(
        &mut scene,
        ClientRect::new(
            demo_layout.scramble_toggle_track_bounds.left(),
            demo_layout.scramble_toggle_track_bounds.top(),
            demo_layout.scramble_toggle_track_bounds.right(),
            demo_layout.scramble_toggle_track_bounds.bottom(),
        )
        .to_win32_rect(),
        if scramble_input_device_identifiers {
            "ON"
        } else {
            "OFF"
        },
        [0.96, 0.98, 1.0, 1.0],
    );
    push_text_block(
        &mut scene,
        ClientRect::new(
            demo_layout.scramble_toggle_track_bounds.right() + 16,
            demo_layout.scramble_toggle_bounds.top() + 12,
            demo_layout.scramble_toggle_bounds.right() - 12,
            demo_layout.scramble_toggle_bounds.bottom(),
        )
        .to_win32_rect(),
        "scramble input device identifiers",
        10,
        20,
        [0.92, 0.95, 0.98, 1.0],
    );

    push_panel(
        &mut scene,
        demo_layout.identifiers_bounds.to_win32_rect(),
        [0.05, 0.06, 0.08, 0.94],
        PanelEffect::TerminalFill,
    );
    let identifier_text = demo_mode_identifier_text(scramble_input_device_identifiers);
    push_text_block(
        &mut scene,
        demo_layout.identifiers_bounds.inset(14).to_win32_rect(),
        &identifier_text,
        8,
        16,
        [0.80, 0.90, 0.88, 1.0],
    );

    scene
}

fn demo_mode_toggle_shader_data(
    visual_state: ButtonVisualState,
    scramble_input_device_identifiers: bool,
) -> [f32; 4] {
    [
        if scramble_input_device_identifiers {
            1.0
        } else {
            0.0
        },
        if visual_state.hovered { 1.0 } else { 0.0 },
        if visual_state.pressed { 1.0 } else { 0.0 },
        visual_state.click_decay,
    ]
}

fn demo_mode_identifier_text(scramble: bool) -> String {
    let mode = if scramble {
        "scrambled"
    } else {
        "representative"
    };
    let mut text = format!("input device identifiers ({mode})\n");
    for (index, identifier) in demo_mode_input_device_identifiers(scramble)
        .iter()
        .enumerate()
    {
        let _ = writeln!(text, "{index}: {identifier}");
    }
    text
}

#[must_use]
pub const fn cursor_gallery_sprite_specs() -> &'static [CursorGallerySpriteSpec] {
    &[
        CursorGallerySpriteSpec {
            cursor: CursorGalleryCursorKind::Arrow,
            label: "Arrow",
            sprite: SpriteId::CursorArrow,
            color: [0.48, 0.95, 1.00, 1.0],
        },
        CursorGallerySpriteSpec {
            cursor: CursorGalleryCursorKind::Hand,
            label: "Hand",
            sprite: SpriteId::CursorHand,
            color: [1.00, 0.56, 0.88, 1.0],
        },
        CursorGallerySpriteSpec {
            cursor: CursorGalleryCursorKind::IBeam,
            label: "I-Beam",
            sprite: SpriteId::CursorIBeam,
            color: [0.78, 1.00, 0.58, 1.0],
        },
        CursorGallerySpriteSpec {
            cursor: CursorGalleryCursorKind::Cross,
            label: "Cross",
            sprite: SpriteId::CursorCross,
            color: [1.00, 0.78, 0.36, 1.0],
        },
        CursorGallerySpriteSpec {
            cursor: CursorGalleryCursorKind::Wait,
            label: "Wait",
            sprite: SpriteId::CursorWait,
            color: [0.72, 0.64, 1.00, 1.0],
        },
        CursorGallerySpriteSpec {
            cursor: CursorGalleryCursorKind::SizeAll,
            label: "Move",
            sprite: SpriteId::CursorSizeAll,
            color: [0.44, 0.88, 0.70, 1.0],
        },
        CursorGallerySpriteSpec {
            cursor: CursorGalleryCursorKind::Help,
            label: "Help",
            sprite: SpriteId::CursorHelp,
            color: [1.00, 0.86, 0.48, 1.0],
        },
    ]
}

#[must_use]
pub fn cursor_gallery_cell_layouts(layout: TerminalLayout) -> Vec<CursorGalleryCellLayout> {
    let body_rect = layout.terminal_panel_rect().inset(30);
    let title_rect = cursor_gallery_title_rect(body_rect);
    let specs = cursor_gallery_sprite_specs();
    let card_size = 132;
    let gap = 24;
    let top = title_rect.bottom() + 20;
    let columns = usize::try_from(((body_rect.width() + gap) / (card_size + gap)).max(1))
        .unwrap_or(1)
        .max(1);

    specs
        .iter()
        .copied()
        .enumerate()
        .filter_map(|(index, spec)| {
            let column = i32::try_from(index % columns).unwrap_or_default();
            let row = i32::try_from(index / columns).unwrap_or_default();
            let left = body_rect.left() + column * (card_size + gap);
            let card_top = top + row * (card_size + 52);
            let card_rect = ClientRect::new(left, card_top, left + card_size, card_top + card_size);
            (card_rect.top() < body_rect.bottom()).then_some(CursorGalleryCellLayout {
                index,
                spec,
                card_rect,
                sprite_rect: card_rect.inset(20),
                label_rect: ClientRect::new(
                    card_rect.left(),
                    card_rect.bottom() + 6,
                    card_rect.right(),
                    card_rect.bottom() + 42,
                ),
            })
        })
        .collect()
}

#[must_use]
// windowing[impl cursor-gallery.stock-os-cursors]
// windowing[impl cursor-gallery.hover-cursor-shape]
pub fn build_cursor_gallery_render_scene(
    layout: TerminalLayout,
    window_chrome_buttons_state: WindowChromeButtonsState,
    selected_index: usize,
    virtual_cursor: Option<ClientPoint>,
    pointer_position: Option<ClientPoint>,
) -> RenderScene {
    let mut scene = build_scene_shell(
        layout,
        SceneWindowKind::CursorGallery,
        window_chrome_buttons_state,
    );
    let body_rect = layout.terminal_panel_rect().inset(30);
    let title_rect = cursor_gallery_title_rect(body_rect);
    push_title_text(
        &mut scene,
        title_rect.to_win32_rect(),
        "Cursor Gallery",
        [0.96, 0.98, 1.00, 1.0],
    );

    let cells = cursor_gallery_cell_layouts(layout);
    let selected_index = selected_index.min(cells.len().saturating_sub(1));
    let pointer_hovered_index = pointer_position.and_then(|point| {
        cells
            .iter()
            .find(|cell| cell.hit_rect().contains(point))
            .map(|cell| cell.index)
    });

    for cell in &cells {
        let active = cell.index == selected_index;
        let hovered = pointer_hovered_index == Some(cell.index);
        if active || hovered {
            push_cursor_gallery_glow(&mut scene, cell.card_rect, cell.spec.color, hovered);
        }

        push_panel_with_data(
            &mut scene,
            cell.card_rect.to_win32_rect(),
            if active || hovered {
                [
                    cell.spec.color[0] * 0.28,
                    cell.spec.color[1] * 0.28,
                    cell.spec.color[2] * 0.28,
                    0.96,
                ]
            } else {
                [0.11, 0.12, 0.16, 0.92]
            },
            PanelEffect::SceneButtonCard,
            ButtonVisualState {
                hover_near: if active || hovered { 1.0 } else { 0.0 },
                hovered,
                pressed: false,
                click_decay: 0.0,
                active,
            }
            .shader_data(),
        );
        push_sprite(
            &mut scene,
            cell.sprite_rect.to_win32_rect(),
            cell.spec.color,
            cell.spec.sprite,
        );
        push_centered_text(
            &mut scene,
            cell.label_rect.to_win32_rect(),
            cell.spec.label,
            [0.90, 0.92, 0.96, 1.0],
        );
    }

    let pointer_spec = pointer_hovered_index
        .and_then(|index| cells.iter().find(|cell| cell.index == index))
        .or_else(|| cells.get(selected_index))
        .map(|cell| cell.spec);
    if let Some(pointer_spec) = pointer_spec {
        let virtual_cursor = virtual_cursor.or_else(|| {
            cells
                .get(selected_index)
                .map(|cell| client_rect_center(cell.hit_rect()))
        });
        push_virtual_cursor_pointer(
            &mut scene,
            virtual_cursor,
            pointer_spec.sprite,
            pointer_spec.color,
        );
    }

    scene
}

fn client_rect_center(rect: ClientRect) -> ClientPoint {
    ClientPoint::new(
        rect.left() + (rect.width() / 2),
        rect.top() + (rect.height() / 2),
    )
}

fn cursor_gallery_title_rect(body_rect: ClientRect) -> ClientRect {
    ClientRect::new(
        body_rect.left(),
        body_rect.top(),
        body_rect.right(),
        (body_rect.top() + 52).min(body_rect.bottom()),
    )
}

fn push_cursor_gallery_glow(
    scene: &mut RenderScene,
    card_rect: ClientRect,
    color: [f32; 4],
    hovered: bool,
) {
    // windowing[impl cursor-gallery.hover-glow-color]
    let glow_alpha = if hovered { 0.34 } else { 0.22 };
    for (inflate, alpha) in [
        (18, glow_alpha * 0.32),
        (10, glow_alpha * 0.55),
        (4, glow_alpha),
    ] {
        push_panel_with_data(
            scene,
            ClientRect::new(
                card_rect.left() - inflate,
                card_rect.top() - inflate,
                card_rect.right() + inflate,
                card_rect.bottom() + inflate,
            )
            .to_win32_rect(),
            [color[0], color[1], color[2], alpha],
            PanelEffect::SceneButtonCard,
            ButtonVisualState {
                hover_near: 1.0,
                hovered,
                pressed: false,
                click_decay: 0.0,
                active: true,
            }
            .shader_data(),
        );
    }
}

#[must_use]
// audio[impl gui.picker-window]
// audio[impl gui.pretty-device-list]
pub fn build_audio_input_device_picker_render_scene(
    layout: TerminalLayout,
    window_chrome_buttons_state: WindowChromeButtonsState,
    devices: &[AudioInputDeviceSummary],
    selected_index: usize,
    scramble_input_device_identifiers: bool,
) -> RenderScene {
    let mut scene = build_scene_shell(
        layout,
        SceneWindowKind::AudioInputDevicePicker,
        window_chrome_buttons_state,
    );
    let body_rect = layout.terminal_panel_rect().inset(22);
    let title_rect = ClientRect::new(
        body_rect.left(),
        body_rect.top(),
        body_rect.right(),
        body_rect.top() + 44,
    );
    push_title_text(
        &mut scene,
        title_rect.to_win32_rect(),
        "Microphones",
        [0.98, 0.98, 1.0, 1.0],
    );
    let legacy_dialog_button_rect = audio_input_legacy_recording_dialog_button_rect(body_rect);
    // audio[impl gui.legacy-recording-dialog]
    push_legacy_recording_dialog_button(&mut scene, legacy_dialog_button_rect);

    if devices.is_empty() {
        let empty_rect = ClientRect::new(
            body_rect.left(),
            title_rect.bottom() + 24,
            body_rect.right(),
            title_rect.bottom() + 96,
        );
        push_panel(
            &mut scene,
            empty_rect.to_win32_rect(),
            [0.13, 0.15, 0.17, 1.0],
            PanelEffect::SceneButtonCard,
        );
        push_text_block(
            &mut scene,
            empty_rect.inset(16).to_win32_rect(),
            "No active Windows recording devices were found.",
            10,
            18,
            [0.92, 0.93, 0.95, 1.0],
        );
        return scene;
    }

    for (index, device) in devices.iter().enumerate() {
        let Some(row_layout) = audio_input_device_row_layout(body_rect, index, devices.len())
        else {
            break;
        };
        let selected = index == selected_index;
        push_panel_with_data(
            &mut scene,
            row_layout.row_rect.to_win32_rect(),
            if selected {
                [0.20, 0.31, 0.38, 1.0]
            } else {
                [0.13, 0.16, 0.19, 1.0]
            },
            PanelEffect::SceneButtonCard,
            [0.0, if selected { 1.0 } else { 0.0 }, 0.0, 0.0],
        );
        push_sprite(
            &mut scene,
            row_layout.icon_rect.to_win32_rect(),
            [1.0, 1.0, 1.0, 1.0],
            SpriteId::Audio,
        );
        let default_marker = if device.is_default { " [default]" } else { "" };
        let sample_rate = device.sample_rate_hz.map_or_else(
            || "sample rate: unknown".to_owned(),
            |rate| format!("sample rate: {rate} Hz"),
        );
        let text = format!(
            "{}{}\n{}\n{}",
            device.name,
            default_marker,
            sample_rate,
            input_device_identifier_display_text(&device.id, scramble_input_device_identifiers)
        );
        push_text_block(
            &mut scene,
            row_layout.text_rect.to_win32_rect(),
            &text,
            9,
            16,
            [0.95, 0.96, 0.98, 1.0],
        );
    }

    scene
}

#[must_use]
pub fn audio_input_device_row_layout(
    body_rect: ClientRect,
    index: usize,
    device_count: usize,
) -> Option<AudioInputDeviceRowLayout> {
    if device_count == 0 {
        return None;
    }
    let index_i32 = i32::try_from(index).ok()?;
    let list_top = body_rect.top() + 66;
    let row_height = 96;
    let row_gap = 12;
    let row_top = list_top + index_i32 * (row_height + row_gap);
    if row_top >= body_rect.bottom() {
        return None;
    }
    let row_rect = ClientRect::new(
        body_rect.left(),
        row_top,
        body_rect.right(),
        (row_top + row_height).min(body_rect.bottom()),
    );
    let icon_size = (row_rect.height() - 28).clamp(36, 68);
    let icon_top = row_rect.top() + ((row_rect.height() - icon_size) / 2);
    let icon_rect = ClientRect::new(
        row_rect.left() + 18,
        icon_top,
        row_rect.left() + 18 + icon_size,
        icon_top + icon_size,
    );
    let text_rect = ClientRect::new(
        icon_rect.right() + 18,
        row_rect.top() + 14,
        row_rect.right() - 18,
        row_rect.bottom() - 12,
    );
    Some(AudioInputDeviceRowLayout {
        row_rect,
        icon_rect,
        text_rect,
    })
}

#[must_use]
pub fn audio_input_legacy_recording_dialog_button_rect(body_rect: ClientRect) -> ClientRect {
    ClientRect::new(
        body_rect.right() - 42,
        body_rect.top() + 2,
        body_rect.right(),
        body_rect.top() + 44,
    )
}

#[must_use]
// audio[impl gui.selected-device-window]
// audio[impl gui.arm-for-record]
#[expect(
    clippy::too_many_lines,
    reason = "the selected-device window composes one compact visual surface with metadata and controls"
)]
pub fn build_audio_input_device_detail_render_scene(
    layout: TerminalLayout,
    window_chrome_buttons_state: WindowChromeButtonsState,
    device_state: Option<&AudioInputDeviceWindowState>,
    visual_state: AudioInputDeviceDetailVisualState,
    scramble_input_device_identifiers: bool,
    text_selection: Option<TerminalSelection>,
) -> RenderScene {
    let mut scene = build_scene_shell(
        layout,
        SceneWindowKind::AudioInputDeviceDetails,
        window_chrome_buttons_state,
    );
    let body_rect = layout.terminal_panel_rect().inset(24);

    let Some(device_state) = device_state else {
        let empty_rect = ClientRect::new(
            body_rect.left(),
            body_rect.top() + 48,
            body_rect.right(),
            body_rect.top() + 132,
        );
        push_panel(
            &mut scene,
            empty_rect.to_win32_rect(),
            [0.13, 0.15, 0.17, 1.0],
            PanelEffect::SceneButtonCard,
        );
        let text = audio_input_device_detail_info_text(None, scramble_input_device_identifiers);
        push_selectable_text_block(
            &mut scene,
            empty_rect.inset(18),
            &text,
            AUDIO_INPUT_DEVICE_DETAIL_TEXT_CELL_WIDTH,
            AUDIO_INPUT_DEVICE_DETAIL_TEXT_CELL_HEIGHT,
            [0.92, 0.93, 0.95, 1.0],
            text_selection,
        );
        return scene;
    };

    let detail_layout = audio_input_device_detail_layout(body_rect);
    push_panel(
        &mut scene,
        body_rect.to_win32_rect(),
        [0.10, 0.13, 0.15, 1.0],
        PanelEffect::SceneBody,
    );
    push_sprite(
        &mut scene,
        detail_layout.icon_rect.to_win32_rect(),
        [1.0, 1.0, 1.0, 1.0],
        SpriteId::Audio,
    );

    let details =
        audio_input_device_detail_info_text(Some(device_state), scramble_input_device_identifiers);
    push_selectable_text_block(
        &mut scene,
        detail_layout.info_rect,
        &details,
        AUDIO_INPUT_DEVICE_DETAIL_TEXT_CELL_WIDTH,
        AUDIO_INPUT_DEVICE_DETAIL_TEXT_CELL_HEIGHT,
        [0.95, 0.96, 0.98, 1.0],
        text_selection,
    );
    push_legacy_recording_dialog_button(&mut scene, detail_layout.legacy_recording_button_rect);

    push_panel_with_data(
        &mut scene,
        detail_layout.arm_button_rect.to_win32_rect(),
        if device_state.is_recording() {
            [0.92, 0.09, 0.07, 1.0]
        } else if device_state.armed_for_record {
            [0.44, 0.05, 0.05, 1.0]
        } else {
            [0.18, 0.05, 0.05, 1.0]
        },
        // audio[impl gui.record-arm-shader]
        PanelEffect::RecordArmButton,
        [
            if device_state.is_recording() {
                1.0
            } else {
                0.0
            },
            if device_state.armed_for_record {
                1.0
            } else {
                0.0
            },
            0.0,
            0.0,
        ],
    );
    push_panel_with_data(
        &mut scene,
        detail_layout.loopback_button_rect.to_win32_rect(),
        if device_state.loopback_enabled {
            [0.20, 0.54, 0.46, 1.0]
        } else {
            [0.13, 0.24, 0.22, 1.0]
        },
        PanelEffect::LoopbackButton,
        [
            if device_state.loopback_enabled {
                1.0
            } else {
                0.0
            },
            if visual_state.loopback_hovered {
                1.0
            } else {
                0.0
            },
            if visual_state.loopback_pressed {
                1.0
            } else {
                0.0
            },
            0.0,
        ],
    );
    push_panel_with_data(
        &mut scene,
        detail_layout.transcription_button_rect.to_win32_rect(),
        if device_state.runtime.transcription.enabled {
            [0.70, 0.45, 0.18, 1.0]
        } else {
            [0.26, 0.18, 0.14, 1.0]
        },
        // audio[impl gui.transcription-toggle]
        PanelEffect::TranscriptionToggle,
        [
            if device_state.runtime.transcription.enabled {
                1.0
            } else {
                0.0
            },
            if visual_state.transcription_hovered {
                1.0
            } else {
                0.0
            },
            if visual_state.transcription_pressed {
                1.0
            } else {
                0.0
            },
            0.0,
        ],
    );
    push_panel(
        &mut scene,
        detail_layout
            .transcription_flush_button_rect
            .to_win32_rect(),
        if visual_state.transcription_flush_pressed {
            [0.58, 0.36, 0.18, 1.0]
        } else if visual_state.transcription_flush_hovered {
            [0.36, 0.25, 0.17, 1.0]
        } else {
            [0.18, 0.15, 0.12, 1.0]
        },
        PanelEffect::TerminalFill,
    );
    push_text_block(
        &mut scene,
        detail_layout
            .transcription_flush_button_rect
            .inset(8)
            .to_win32_rect(),
        "Flush chunk",
        8,
        14,
        if device_state.runtime.transcription.enabled {
            [0.95, 0.78, 0.50, 1.0]
        } else {
            [0.62, 0.58, 0.52, 1.0]
        },
    );
    push_panel_with_data(
        &mut scene,
        detail_layout.play_pause_button_rect.to_win32_rect(),
        if device_state.is_playing() {
            [0.20, 0.56, 0.86, 1.0]
        } else {
            [0.14, 0.28, 0.38, 1.0]
        },
        // audio[impl gui.playback-transport]
        PanelEffect::PlaybackButton,
        [
            if device_state.is_playing() { 1.0 } else { 0.0 },
            if visual_state.playback_hovered {
                1.0
            } else {
                0.0
            },
            if visual_state.playback_pressed {
                1.0
            } else {
                0.0
            },
            playback_speed_shader_value(device_state.runtime.playback.speed),
        ],
    );
    push_text_block(
        &mut scene,
        detail_layout.arm_status_rect.to_win32_rect(),
        if device_state.is_recording() {
            "Recording"
        } else {
            "Not recording"
        },
        10,
        18,
        if device_state.is_recording() {
            [1.0, 0.64, 0.58, 1.0]
        } else if device_state.armed_for_record {
            [0.94, 0.74, 0.70, 1.0]
        } else {
            [0.82, 0.84, 0.86, 1.0]
        },
    );
    push_text_block(
        &mut scene,
        ClientRect::new(
            detail_layout.loopback_button_rect.right() + 16,
            detail_layout.loopback_button_rect.top() + 10,
            detail_layout.arm_status_rect.right(),
            detail_layout.loopback_button_rect.bottom() - 8,
        )
        .to_win32_rect(),
        if device_state.loopback_enabled {
            "Loopback enabled"
        } else {
            "Loopback disabled"
        },
        10,
        18,
        if device_state.loopback_enabled {
            [0.73, 0.93, 0.88, 1.0]
        } else {
            [0.76, 0.82, 0.84, 1.0]
        },
    );
    // audio[impl gui.audio-buffer-waveform]
    push_audio_input_buffer_section(&mut scene, detail_layout, device_state, visual_state);

    scene
}

fn playback_speed_shader_value(speed: f64) -> f32 {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "playback shuttle speed is bounded to a small range before reaching shader data"
    )]
    {
        speed as f32
    }
}

#[must_use]
// windowing[impl scene.pretty-text.selection]
pub fn audio_input_device_detail_info_text(
    device_state: Option<&AudioInputDeviceWindowState>,
    scramble_input_device_identifiers: bool,
) -> String {
    let Some(device_state) = device_state else {
        return "No microphone is selected.".to_owned();
    };

    let device = &device_state.device;
    let default_marker = if device.is_default { " [default]" } else { "" };
    let sample_rate = device.sample_rate_hz.map_or_else(
        || "sample rate: unknown".to_owned(),
        |rate| format!("sample rate: {rate} Hz"),
    );
    format!(
        "{}{}\n{}\nstate: {}\n{}",
        device.name,
        default_marker,
        sample_rate,
        device.state,
        input_device_identifier_display_text(&device.id, scramble_input_device_identifiers)
    )
}

#[must_use]
pub fn audio_input_device_detail_selectable_text_rect(
    body_rect: ClientRect,
    has_device: bool,
) -> ClientRect {
    if has_device {
        audio_input_device_detail_layout(body_rect).info_rect
    } else {
        ClientRect::new(
            body_rect.left(),
            body_rect.top() + 48,
            body_rect.right(),
            body_rect.top() + 132,
        )
        .inset(18)
    }
}

#[must_use]
#[expect(
    clippy::too_many_lines,
    reason = "audio detail geometry is kept in one function so hit testing and rendering share it"
)]
pub fn audio_input_device_detail_layout(body_rect: ClientRect) -> AudioInputDeviceDetailLayout {
    let icon_size = body_rect.width().min(body_rect.height()).clamp(96, 164);
    let icon_rect = ClientRect::new(
        body_rect.left() + 28,
        body_rect.top() + 42,
        body_rect.left() + 28 + icon_size,
        body_rect.top() + 42 + icon_size,
    );
    let arm_button_size = 74;
    let arm_button_rect = ClientRect::new(
        icon_rect.left(),
        icon_rect.bottom() + 36,
        icon_rect.left() + arm_button_size,
        icon_rect.bottom() + 36 + arm_button_size,
    );
    let loopback_left = (body_rect.right() - 260).max(arm_button_rect.right() + 180);
    let loopback_button_rect = ClientRect::new(
        loopback_left,
        arm_button_rect.top() + 8,
        loopback_left + 58,
        arm_button_rect.top() + 8 + 58,
    );
    let play_pause_button_size = 64;
    let play_pause_button_rect = ClientRect::new(
        loopback_button_rect.left() - 24 - play_pause_button_size,
        arm_button_rect.top() + 5,
        loopback_button_rect.left() - 24,
        arm_button_rect.top() + 5 + play_pause_button_size,
    );
    let transcription_button_size = 64;
    let transcription_button_rect = ClientRect::new(
        play_pause_button_rect.left() - 24 - transcription_button_size,
        arm_button_rect.top() + 5,
        play_pause_button_rect.left() - 24,
        arm_button_rect.top() + 5 + transcription_button_size,
    );
    let transcription_flush_button_rect = ClientRect::new(
        transcription_button_rect.left(),
        arm_button_rect.bottom() + 8,
        loopback_button_rect.right(),
        arm_button_rect.bottom() + 32,
    );
    let arm_status_rect = ClientRect::new(
        arm_button_rect.right() + 18,
        arm_button_rect.top() + 14,
        transcription_button_rect.left() - 18,
        arm_button_rect.bottom() - 10,
    );
    let info_rect = ClientRect::new(
        icon_rect.right() + 32,
        icon_rect.top() + 6,
        body_rect.right() - 84,
        icon_rect.bottom() + 24,
    );
    let legacy_recording_button_rect = ClientRect::new(
        body_rect.right() - 58,
        body_rect.top() + 28,
        body_rect.right() - 16,
        body_rect.top() + 70,
    );
    let buffer_section_rect = ClientRect::new(
        body_rect.left() + 28,
        arm_button_rect.bottom() + 38,
        body_rect.right() - 28,
        body_rect.bottom() - 28,
    );
    let timeline_label_rect = ClientRect::new(
        buffer_section_rect.left() + 16,
        buffer_section_rect.top() + 12,
        buffer_section_rect.right() - 16,
        buffer_section_rect.top() + 48,
    );
    let waveform_rect = ClientRect::new(
        buffer_section_rect.left() + 16,
        timeline_label_rect.bottom() + 10,
        buffer_section_rect.right() - 16,
        timeline_label_rect.bottom() + 132,
    );
    let mel_spectrogram_rect = ClientRect::new(
        buffer_section_rect.left() + 16,
        waveform_rect.bottom() + 10,
        buffer_section_rect.right() - 16,
        waveform_rect.bottom() + 98,
    );
    let transcript_terminal_rect = ClientRect::new(
        buffer_section_rect.left() + 16,
        mel_spectrogram_rect.bottom() + 10,
        buffer_section_rect.right() - 16,
        buffer_section_rect.bottom() - 16,
    );

    AudioInputDeviceDetailLayout {
        icon_rect,
        info_rect,
        arm_button_rect,
        transcription_button_rect,
        transcription_flush_button_rect,
        play_pause_button_rect,
        loopback_button_rect,
        arm_status_rect,
        legacy_recording_button_rect,
        buffer_section_rect,
        waveform_rect,
        mel_spectrogram_rect,
        transcript_terminal_rect,
        timeline_label_rect,
    }
}

fn push_audio_input_buffer_section(
    scene: &mut RenderScene,
    detail_layout: AudioInputDeviceDetailLayout,
    device_state: &AudioInputDeviceWindowState,
    visual_state: AudioInputDeviceDetailVisualState,
) {
    push_panel(
        scene,
        detail_layout.buffer_section_rect.to_win32_rect(),
        [0.08, 0.10, 0.12, 1.0],
        PanelEffect::SceneBody,
    );
    let duration_seconds = device_state.runtime.duration_seconds();
    let selection_text = device_state.runtime.selection.map_or_else(
        || "selection: none".to_owned(),
        |selection| {
            format!(
                "selection: {:.2}s - {:.2}s ({:.2}s)",
                selection.begin_seconds,
                selection.end_seconds,
                selection.duration_seconds()
            )
        },
    );
    let labels = format!(
        "Audio Buffer   duration {:.2}s   rec {:.2}s   play {:.2}s   transcript {:.2}s\n{}   Space play/pause   Enter record   J/K/L shuttle",
        duration_seconds,
        device_state.runtime.recording_head_seconds,
        device_state.runtime.playback.head_seconds,
        device_state.runtime.transcription_head_seconds,
        selection_text
    );
    push_text_block(
        scene,
        detail_layout.timeline_label_rect.to_win32_rect(),
        &labels,
        9,
        16,
        [0.88, 0.91, 0.94, 1.0],
    );
    push_waveform(
        scene,
        detail_layout.waveform_rect,
        device_state,
        visual_state,
    );
    // audio[impl gui.mel-spectrogram-preview]
    push_mel_spectrogram(scene, detail_layout.mel_spectrogram_rect, device_state);
    // audio[impl gui.transcription-terminal-island]
    push_transcript_terminal_island(scene, detail_layout.transcript_terminal_rect, device_state);
}

fn push_waveform(
    scene: &mut RenderScene,
    waveform_rect: ClientRect,
    device_state: &AudioInputDeviceWindowState,
    visual_state: AudioInputDeviceDetailVisualState,
) {
    push_panel(
        scene,
        waveform_rect.to_win32_rect(),
        [0.04, 0.05, 0.06, 1.0],
        PanelEffect::TerminalFill,
    );
    let samples = device_state.runtime.samples();
    let duration_seconds = device_state.runtime.duration_seconds().max(1.0);
    if let Some(selection) = device_state.runtime.selection {
        push_head_region(
            scene,
            waveform_rect,
            duration_seconds,
            selection.begin_seconds,
            selection.end_seconds,
            [0.28, 0.48, 0.72, 0.34],
        );
    }
    if samples.is_empty() {
        push_text_block(
            scene,
            waveform_rect.inset(16).to_win32_rect(),
            "No recorded audio yet.",
            10,
            18,
            [0.52, 0.56, 0.60, 1.0],
        );
    } else {
        push_waveform_bars(scene, waveform_rect, &samples, 0..samples.len());
    }
    push_timeline_head(
        scene,
        waveform_rect,
        duration_seconds,
        device_state.runtime.recording_head_seconds,
        [1.0, 0.20, 0.16, 1.0],
    );
    push_timeline_head(
        scene,
        waveform_rect,
        duration_seconds,
        device_state.runtime.playback.head_seconds,
        [0.28, 0.72, 1.0, 1.0],
    );
    push_timeline_head(
        scene,
        waveform_rect,
        duration_seconds,
        device_state.runtime.transcription_head_seconds,
        [0.85, 0.56, 1.0, 1.0],
    );
    push_timeline_head_grabbers(scene, waveform_rect, device_state, visual_state);
    if let Some(selection) = device_state.runtime.selection {
        push_timeline_head(
            scene,
            waveform_rect,
            duration_seconds,
            selection.begin_seconds,
            [0.95, 0.86, 0.30, 1.0],
        );
        push_timeline_head(
            scene,
            waveform_rect,
            duration_seconds,
            selection.end_seconds,
            [0.95, 0.86, 0.30, 1.0],
        );
    }
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    reason = "waveform amplitudes are deliberately converted to integer pixel bar heights"
)]
fn push_waveform_bars(
    scene: &mut RenderScene,
    waveform_rect: ClientRect,
    samples: &[f32],
    sample_range: Range<usize>,
) {
    if waveform_rect.width() <= 0
        || waveform_rect.height() <= 0
        || sample_range.start >= sample_range.end
        || sample_range.end > samples.len()
    {
        return;
    }

    let width = waveform_rect.width().max(1);
    let mid_y = waveform_rect.top() + waveform_rect.height() / 2;
    let half_height = (waveform_rect.height() / 2 - 6).max(2);
    let bars = width;
    let peak = samples
        .iter()
        .map(|sample| sample.abs())
        .fold(0.0_f32, f32::max)
        .max(0.015);
    let visible_sample_count = sample_range.end - sample_range.start;
    for bar_index in 0..bars {
        let start = sample_range.start
            + (usize::try_from(bar_index).unwrap_or_default() * visible_sample_count)
                / usize::try_from(bars).unwrap_or(1);
        let end = (sample_range.start
            + (usize::try_from(bar_index + 1).unwrap_or_default() * visible_sample_count)
                / usize::try_from(bars).unwrap_or(1))
        .max(start + 1)
        .min(sample_range.end);
        let amplitude = samples[start..end]
            .iter()
            .map(|sample| sample.abs())
            .fold(0.0_f32, f32::max)
            / peak;
        let amplitude = amplitude.clamp(0.0, 1.0);
        let bar_height = (amplitude * half_height as f32).round() as i32;
        let x = waveform_rect.left() + (bar_index * waveform_rect.width()) / bars;
        let next_x = waveform_rect.left() + ((bar_index + 1) * waveform_rect.width()) / bars;
        let rect = ClientRect::new(
            x,
            mid_y - bar_height.max(1),
            next_x.max(x + 1).min(waveform_rect.right()),
            mid_y + bar_height.max(1),
        );
        push_panel(
            scene,
            rect.to_win32_rect(),
            [0.40, 0.78, 0.86, 1.0],
            PanelEffect::TerminalFill,
        );
    }
}

fn push_mel_spectrogram(
    scene: &mut RenderScene,
    spectrogram_rect: ClientRect,
    device_state: &AudioInputDeviceWindowState,
) {
    push_panel(
        scene,
        spectrogram_rect.to_win32_rect(),
        [0.035, 0.045, 0.052, 1.0],
        PanelEffect::TerminalFill,
    );
    let inner_rect = spectrogram_rect.inset(8);
    if !device_state.runtime.transcription.enabled {
        push_text_block(
            scene,
            inner_rect.to_win32_rect(),
            "transcription disabled",
            9,
            16,
            [0.46, 0.50, 0.54, 1.0],
        );
        return;
    }

    let preview = &device_state.runtime.transcription.preview;
    if preview.intensities.is_empty()
        || preview
            .intensities
            .iter()
            .all(|intensity| *intensity == 0.0)
    {
        push_text_block(
            scene,
            inner_rect.to_win32_rect(),
            "waiting for recorded audio",
            9,
            16,
            [0.72, 0.66, 0.58, 1.0],
        );
        return;
    }

    push_mel_spectrogram_tiles(scene, inner_rect, device_state);
    let status = transcription_preview_status_text(device_state);
    push_text_block(
        scene,
        ClientRect::new(
            inner_rect.left() + 8,
            inner_rect.top() + 6,
            inner_rect.right() - 8,
            inner_rect.top() + 38,
        )
        .to_win32_rect(),
        &status,
        8,
        14,
        [0.88, 0.86, 0.74, 1.0],
    );
}

fn push_mel_spectrogram_tiles(
    scene: &mut RenderScene,
    rect: ClientRect,
    device_state: &AudioInputDeviceWindowState,
) {
    let preview = &device_state.runtime.transcription.preview;
    if preview.columns == 0 || preview.bins == 0 {
        return;
    }
    let columns = i32::try_from(preview.columns).unwrap_or(1).max(1);
    let bins = i32::try_from(preview.bins).unwrap_or(1).max(1);

    for column in 0..preview.columns {
        for bin in 0..preview.bins {
            let intensity = preview.intensities[(bin * preview.columns) + column];
            let color = mel_spectrogram_color(intensity);
            let left =
                rect.left() + (i32::try_from(column).unwrap_or_default() * rect.width()) / columns;
            let right = rect.left()
                + (i32::try_from(column + 1).unwrap_or_default() * rect.width()) / columns;
            let bottom =
                rect.bottom() - (i32::try_from(bin).unwrap_or_default() * rect.height()) / bins;
            let top =
                rect.bottom() - (i32::try_from(bin + 1).unwrap_or_default() * rect.height()) / bins;
            push_panel(
                scene,
                ClientRect::new(left, top, right.max(left + 1), bottom.max(top + 1))
                    .to_win32_rect(),
                color,
                PanelEffect::TerminalFill,
            );
        }
    }
}

fn transcription_preview_status_text(device_state: &AudioInputDeviceWindowState) -> String {
    let transcription = &device_state.runtime.transcription;
    let request_text = transcription
        .last_sent_reason
        .as_deref()
        .unwrap_or("no chunk sent yet");
    let completed = transcription
        .last_completed_request_id
        .map_or_else(|| "none".to_owned(), |request_id| request_id.to_string());
    format!(
        "chunk {:.2}s  energy {:.3}  sent: {}  done: {}",
        transcription.chunk_seconds, transcription.energy_rms, request_text, completed
    )
}

fn mel_spectrogram_color(intensity: f32) -> [f32; 4] {
    let low = [0.05, 0.08, 0.11];
    let mid = [0.10, 0.58, 0.54];
    let high = [1.00, 0.64, 0.24];
    let (from, to, local) = if intensity < 0.55 {
        (low, mid, intensity / 0.55)
    } else {
        (mid, high, (intensity - 0.55) / 0.45)
    };
    [
        from[0] + ((to[0] - from[0]) * local),
        from[1] + ((to[1] - from[1]) * local),
        from[2] + ((to[2] - from[2]) * local),
        1.0,
    ]
}

fn push_transcript_terminal_island(
    scene: &mut RenderScene,
    terminal_rect: ClientRect,
    device_state: &AudioInputDeviceWindowState,
) {
    push_panel(
        scene,
        terminal_rect.to_win32_rect(),
        [0.035, 0.04, 0.048, 1.0],
        PanelEffect::TerminalPanel,
    );
    let text = if device_state.runtime.transcription.staged_text.is_empty() {
        transcription_terminal_status_text(device_state)
    } else {
        device_state.runtime.transcription.staged_text.clone()
    };
    push_text_block(
        scene,
        terminal_rect.inset(12).to_win32_rect(),
        &text,
        9,
        16,
        [0.84, 0.90, 0.88, 1.0],
    );
}

fn transcription_terminal_status_text(device_state: &AudioInputDeviceWindowState) -> String {
    if !device_state.runtime.transcription.enabled {
        return "transcript island idle".to_owned();
    }
    if device_state.runtime.samples().is_empty() {
        return "transcription enabled; waiting for audio ahead of the head".to_owned();
    }
    "transcription enabled; mel preview ready for Python handoff".to_owned()
}

fn push_timeline_head(
    scene: &mut RenderScene,
    waveform_rect: ClientRect,
    duration_seconds: f64,
    seconds: f64,
    color: [f32; 4],
) {
    let x = timeline_seconds_to_x(waveform_rect, duration_seconds, seconds);
    let rect = ClientRect::new(x - 1, waveform_rect.top(), x + 2, waveform_rect.bottom());
    push_panel(
        scene,
        rect.to_win32_rect(),
        color,
        PanelEffect::TerminalFill,
    );
}

fn push_timeline_head_grabbers(
    scene: &mut RenderScene,
    waveform_rect: ClientRect,
    device_state: &AudioInputDeviceWindowState,
    visual_state: AudioInputDeviceDetailVisualState,
) {
    for grabber in audio_input_timeline_head_grabbers(waveform_rect, device_state) {
        let active = match grabber.kind {
            AudioInputTimelineHeadKind::Recording => device_state.is_recording(),
            AudioInputTimelineHeadKind::Playback => device_state.is_playing(),
            AudioInputTimelineHeadKind::Transcription => device_state.runtime.transcription.enabled,
        };
        push_panel_with_data(
            scene,
            grabber.rect.to_win32_rect(),
            timeline_head_color(grabber.kind),
            PanelEffect::TimelineHeadGrabber,
            [
                if active { 1.0 } else { 0.0 },
                if visual_state.hovered_head == Some(grabber.kind) {
                    1.0
                } else {
                    0.0
                },
                if visual_state.grabbed_head == Some(grabber.kind) {
                    1.0
                } else {
                    0.0
                },
                head_kind_shader_index(grabber.kind),
            ],
        );
    }
}

#[must_use]
pub fn audio_input_timeline_head_grabbers(
    waveform_rect: ClientRect,
    device_state: &AudioInputDeviceWindowState,
) -> Vec<AudioInputTimelineHeadGrabberLayout> {
    let duration_seconds = device_state.runtime.duration_seconds().max(1.0);
    let mut heads = vec![
        (
            AudioInputTimelineHeadKind::Recording,
            timeline_seconds_to_x(
                waveform_rect,
                duration_seconds,
                device_state.runtime.recording_head_seconds,
            ),
        ),
        (
            AudioInputTimelineHeadKind::Playback,
            timeline_seconds_to_x(
                waveform_rect,
                duration_seconds,
                device_state.runtime.playback.head_seconds,
            ),
        ),
        (
            AudioInputTimelineHeadKind::Transcription,
            timeline_seconds_to_x(
                waveform_rect,
                duration_seconds,
                device_state.runtime.transcription_head_seconds,
            ),
        ),
    ];
    heads.sort_by_key(|(kind, x)| (*x, head_kind_sort_key(*kind)));
    let grabber_size: i32 = 14;
    let vertical_gap: i32 = 4;
    let threshold = (grabber_size + 2).cast_unsigned();
    let mut previous_x = i32::MIN;
    let mut stack_index = 0;
    heads
        .into_iter()
        .map(|(kind, x)| {
            if previous_x != i32::MIN && x.abs_diff(previous_x) <= threshold {
                stack_index += 1;
            } else {
                stack_index = 0;
                previous_x = x;
            }
            let top = waveform_rect.top() + 8 + stack_index * (grabber_size + vertical_gap);
            AudioInputTimelineHeadGrabberLayout {
                kind,
                rect: ClientRect::new(x - 7, top, x + 7, top + grabber_size),
            }
        })
        .collect()
}

fn timeline_head_color(head: AudioInputTimelineHeadKind) -> [f32; 4] {
    match head {
        AudioInputTimelineHeadKind::Recording => [1.0, 0.20, 0.16, 1.0],
        AudioInputTimelineHeadKind::Playback => [0.28, 0.72, 1.0, 1.0],
        AudioInputTimelineHeadKind::Transcription => [0.85, 0.56, 1.0, 1.0],
    }
}

fn head_kind_shader_index(head: AudioInputTimelineHeadKind) -> f32 {
    match head {
        AudioInputTimelineHeadKind::Recording => 0.0,
        AudioInputTimelineHeadKind::Playback => 1.0,
        AudioInputTimelineHeadKind::Transcription => 2.0,
    }
}

fn head_kind_sort_key(head: AudioInputTimelineHeadKind) -> i32 {
    match head {
        AudioInputTimelineHeadKind::Recording => 0,
        AudioInputTimelineHeadKind::Playback => 1,
        AudioInputTimelineHeadKind::Transcription => 2,
    }
}

fn push_head_region(
    scene: &mut RenderScene,
    waveform_rect: ClientRect,
    duration_seconds: f64,
    begin_seconds: f64,
    end_seconds: f64,
    color: [f32; 4],
) {
    let begin_x = timeline_seconds_to_x(waveform_rect, duration_seconds, begin_seconds);
    let end_x = timeline_seconds_to_x(waveform_rect, duration_seconds, end_seconds);
    let rect = ClientRect::new(
        begin_x.min(end_x),
        waveform_rect.top(),
        begin_x.max(end_x),
        waveform_rect.bottom(),
    );
    push_panel(
        scene,
        rect.to_win32_rect(),
        color,
        PanelEffect::TerminalFill,
    );
}

#[must_use]
pub fn audio_input_timeline_seconds_from_point(
    waveform_rect: ClientRect,
    duration_seconds: f64,
    point: ClientPoint,
) -> f64 {
    let width = waveform_rect.width().max(1);
    let point_x = point
        .to_win32_point()
        .map_or(waveform_rect.left(), |point| point.x);
    let offset = (point_x - waveform_rect.left()).clamp(0, width);
    (f64::from(offset) / f64::from(width)) * duration_seconds.max(1.0)
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "timeline seconds are clamped and converted to pixel x coordinates"
)]
fn timeline_seconds_to_x(waveform_rect: ClientRect, duration_seconds: f64, seconds: f64) -> i32 {
    let width = waveform_rect.width().max(1);
    waveform_rect.left()
        + ((seconds.clamp(0.0, duration_seconds.max(1.0)) / duration_seconds.max(1.0))
            * f64::from(width)) as i32
}

// audio[impl gui.legacy-recording-dialog]
fn push_legacy_recording_dialog_button(scene: &mut RenderScene, rect: ClientRect) {
    push_panel_with_data(
        scene,
        rect.to_win32_rect(),
        [0.16, 0.22, 0.29, 1.0],
        PanelEffect::GearButton,
        [0.0, 0.0, 0.0, 0.0],
    );
}

fn push_selectable_text_block(
    scene: &mut RenderScene,
    rect: ClientRect,
    text: &str,
    cell_width: i32,
    cell_height: i32,
    color: [f32; 4],
    selection: Option<TerminalSelection>,
) {
    let text_scene = cell_grid::build_text_grid_scene_with_palette(
        rect,
        text,
        cell_width,
        cell_height,
        selection,
        color,
        [0.06, 0.07, 0.09, 1.0],
        [0.42, 0.67, 0.98, 1.0],
    );
    scene.panels.extend(text_scene.panels);
    scene.glyphs.extend(text_scene.glyphs);
    scene.sprites.extend(text_scene.sprites);
    scene.overlay_panels.extend(text_scene.overlay_panels);
}

#[must_use]
/// windowing[impl diagnostics.scene-window.replaces-body]
pub fn build_scene_diagnostic_render_scene(
    layout: TerminalLayout,
    scene_kind: SceneWindowKind,
    window_chrome_buttons_state: WindowChromeButtonsState,
    diagnostic_text: &str,
    selection: Option<super::windows_terminal::TerminalSelection>,
    cell_width: i32,
    cell_height: i32,
) -> RenderScene {
    let mut scene = build_scene_shell(layout, scene_kind, window_chrome_buttons_state);
    let body_rect = layout.terminal_panel_rect().inset(20);
    let diagnostic_scene = cell_grid::build_text_grid_scene(
        body_rect,
        diagnostic_text,
        cell_width,
        cell_height,
        selection,
    );
    scene.panels.extend(diagnostic_scene.panels);
    scene.glyphs.extend(diagnostic_scene.glyphs);
    scene.sprites.extend(diagnostic_scene.sprites);
    scene.overlay_panels.extend(diagnostic_scene.overlay_panels);
    scene
}

#[must_use]
// windowing[impl diagnostics.launcher-tui]
pub fn build_launcher_diagnostic_render_scene(
    layout: TerminalLayout,
    window_chrome_buttons_state: WindowChromeButtonsState,
    selected_index: usize,
    virtual_cursor: Option<ClientPoint>,
    selection: Option<TerminalSelection>,
    cell_width: i32,
    cell_height: i32,
) -> RenderScene {
    let mut scene = build_scene_shell(
        layout,
        SceneWindowKind::Launcher,
        window_chrome_buttons_state,
    );
    let body_rect = layout.terminal_panel_rect().inset(20);
    let diagnostic_scene = build_launcher_diagnostic_body_scene(
        body_rect,
        selected_index,
        virtual_cursor,
        selection,
        cell_width,
        cell_height,
    );
    scene.panels.extend(diagnostic_scene.panels);
    scene.glyphs.extend(diagnostic_scene.glyphs);
    scene.sprites.extend(diagnostic_scene.sprites);
    scene.overlay_panels.extend(diagnostic_scene.overlay_panels);
    push_virtual_cursor_pointer(
        &mut scene,
        virtual_cursor,
        SpriteId::CursorHand,
        [0.48, 0.95, 1.0, 0.96],
    );
    scene
}

#[must_use]
pub fn launcher_diagnostic_action_hit_rects(
    layout: TerminalLayout,
    cell_width: i32,
    cell_height: i32,
) -> Vec<ClientRect> {
    let body_rect = layout.terminal_panel_rect().inset(20);
    let columns = u16::try_from((body_rect.width() / cell_width.max(1)).max(0)).unwrap_or_default();
    let rows = u16::try_from((body_rect.height() / cell_height.max(1)).max(0)).unwrap_or_default();
    if columns == 0 || rows == 0 {
        return Vec::new();
    }

    let area = RatatuiRect::new(0, 0, columns, rows);
    let chunks = launcher_diagnostic_chunks(area);
    let action_inner = ratatui_block_inner(chunks[1]);
    let action_count = scene_button_specs(SceneWindowKind::Launcher).len();
    (0..action_count)
        .filter_map(|index| {
            let row = action_inner
                .y
                .saturating_add(u16::try_from(index.saturating_mul(2)).ok()?);
            (row < action_inner.y.saturating_add(action_inner.height)).then(|| {
                ratatui_rect_to_client_rect(
                    body_rect,
                    RatatuiRect::new(action_inner.x, row, action_inner.width, 2),
                    cell_width,
                    cell_height,
                )
            })
        })
        .collect()
}

#[must_use]
// audio[impl gui.diagnostics-tui]
#[expect(
    clippy::too_many_arguments,
    reason = "audio diagnostics rendering takes layout, selection, and demo-mode display policy together"
)]
pub fn build_audio_input_device_diagnostic_render_scene(
    layout: TerminalLayout,
    window_chrome_buttons_state: WindowChromeButtonsState,
    devices: &[AudioInputDeviceSummary],
    selected_index: usize,
    selection: Option<TerminalSelection>,
    cell_width: i32,
    cell_height: i32,
    scramble_input_device_identifiers: bool,
) -> RenderScene {
    let mut scene = build_scene_shell(
        layout,
        SceneWindowKind::AudioInputDevicePicker,
        window_chrome_buttons_state,
    );
    let body_rect = layout.terminal_panel_rect().inset(20);
    let diagnostic_scene = build_audio_input_device_diagnostic_body_scene(
        body_rect,
        devices,
        selected_index,
        selection,
        cell_width,
        cell_height,
        scramble_input_device_identifiers,
    );
    scene.panels.extend(diagnostic_scene.panels);
    scene.glyphs.extend(diagnostic_scene.glyphs);
    scene.sprites.extend(diagnostic_scene.sprites);
    scene.overlay_panels.extend(diagnostic_scene.overlay_panels);
    scene
}

#[must_use]
// audio[impl gui.daemon-diagnostics-tui]
pub fn build_audio_daemon_diagnostic_render_scene(
    layout: TerminalLayout,
    window_chrome_buttons_state: WindowChromeButtonsState,
    status: &AudioTranscriptionDaemonStatusReport,
    selection: Option<TerminalSelection>,
    cell_width: i32,
    cell_height: i32,
) -> RenderScene {
    let mut scene = build_scene_shell(
        layout,
        SceneWindowKind::AudioDaemon,
        window_chrome_buttons_state,
    );
    let body_rect = layout.terminal_panel_rect().inset(20);
    let diagnostic_scene = build_audio_daemon_diagnostic_body_scene(
        body_rect,
        status,
        selection,
        cell_width,
        cell_height,
    );
    scene.panels.extend(diagnostic_scene.panels);
    scene.glyphs.extend(diagnostic_scene.glyphs);
    scene.sprites.extend(diagnostic_scene.sprites);
    scene.overlay_panels.extend(diagnostic_scene.overlay_panels);
    scene
}

#[must_use]
// audio[impl gui.selected-device-diagnostics-tui]
pub fn build_audio_input_device_detail_diagnostic_render_scene(
    layout: TerminalLayout,
    window_chrome_buttons_state: WindowChromeButtonsState,
    device_state: Option<&AudioInputDeviceWindowState>,
    selection: Option<TerminalSelection>,
    cell_width: i32,
    cell_height: i32,
    scramble_input_device_identifiers: bool,
) -> RenderScene {
    let mut scene = build_scene_shell(
        layout,
        SceneWindowKind::AudioInputDeviceDetails,
        window_chrome_buttons_state,
    );
    let body_rect = layout.terminal_panel_rect().inset(20);
    let diagnostic_scene = build_audio_input_device_detail_diagnostic_body_scene(
        body_rect,
        device_state,
        selection,
        cell_width,
        cell_height,
        scramble_input_device_identifiers,
    );
    scene.panels.extend(diagnostic_scene.panels);
    scene.glyphs.extend(diagnostic_scene.glyphs);
    scene.sprites.extend(diagnostic_scene.sprites);
    scene.overlay_panels.extend(diagnostic_scene.overlay_panels);
    scene
}

fn build_audio_daemon_diagnostic_body_scene(
    body_rect: ClientRect,
    status: &AudioTranscriptionDaemonStatusReport,
    selection: Option<TerminalSelection>,
    cell_width: i32,
    cell_height: i32,
) -> RenderScene {
    let columns = u16::try_from((body_rect.width() / cell_width.max(1)).max(0)).unwrap_or_default();
    let rows = u16::try_from((body_rect.height() / cell_height.max(1)).max(0)).unwrap_or_default();
    if columns == 0 || rows == 0 {
        return empty_render_scene();
    }

    let area = RatatuiRect::new(0, 0, columns, rows);
    let mut buffer = Buffer::empty(area);
    render_audio_daemon_diagnostic_buffer(&mut buffer, area, status);
    ratatui_buffer_to_scene(body_rect, &buffer, selection, cell_width, cell_height)
}

#[expect(
    clippy::too_many_lines,
    reason = "the daemon diagnostics TUI composes transport, payload, paths, and queue state"
)]
fn render_audio_daemon_diagnostic_buffer(
    buffer: &mut Buffer,
    area: RatatuiRect,
    status: &AudioTranscriptionDaemonStatusReport,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(7),
            Constraint::Length(9),
            Constraint::Min(8),
            Constraint::Length(7),
        ])
        .split(area);

    Paragraph::new(vec![
        Line::from(vec![
            Span::styled("Mode ", Style::new().fg(Color::DarkGray)),
            Span::styled(
                "daemon diagnostics",
                Style::new()
                    .fg(Color::LightCyan)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("Control ", Style::new().fg(Color::DarkGray)),
            Span::styled(
                status.control_transport.as_str(),
                Style::new().fg(Color::LightGreen),
            ),
        ]),
        Line::from(vec![
            Span::styled("Payload ", Style::new().fg(Color::DarkGray)),
            Span::styled(
                status.payload_transport.as_str(),
                Style::new().fg(Color::LightYellow),
            ),
        ]),
        Line::from(vec![
            Span::styled("Entrypoint ", Style::new().fg(Color::DarkGray)),
            Span::styled(
                status.python_entrypoint.as_str(),
                Style::new().fg(Color::White),
            ),
        ]),
        Line::from(vec![
            Span::styled("Model ", Style::new().fg(Color::DarkGray)),
            Span::styled(
                status.selected_model.as_str(),
                Style::new().fg(Color::LightGreen),
            ),
        ]),
    ])
    .block(
        Block::default()
            .title(" Audio Daemon ")
            .borders(Borders::ALL)
            .border_style(Style::new().fg(Color::Cyan)),
    )
    .render(chunks[0], buffer);

    let tensor_items = vec![
        ListItem::new(format!("dtype: {}", status.tensor_dtype)),
        ListItem::new(format!(
            "shape: {} x {}",
            status.tensor_mel_bins, status.tensor_frames
        )),
        ListItem::new(format!("values: {}", status.tensor_values)),
        ListItem::new(format!("bytes: {}", status.tensor_bytes)),
        ListItem::new(format!("slot bytes: {}", status.shared_memory_slot_bytes)),
        ListItem::new(format!(
            "pool floor: {} slots / {} bytes",
            status.shared_memory_minimum_slots, status.shared_memory_total_bytes
        )),
    ];
    List::new(tensor_items)
        .block(
            Block::default()
                .title(" Log-Mel Payload Contract ")
                .borders(Borders::ALL)
                .border_style(Style::new().fg(Color::Blue)),
        )
        .style(Style::new().fg(Color::White))
        .render(chunks[1], buffer);

    Paragraph::new(vec![
        Line::from(vec![
            Span::styled("daemon source ", Style::new().fg(Color::DarkGray)),
            Span::raw(status.daemon_source_dir.as_str()),
        ]),
        Line::from(vec![
            Span::styled("uv venv ", Style::new().fg(Color::DarkGray)),
            Span::raw(status.uv_venv_dir.as_str()),
        ]),
        Line::from(vec![
            Span::styled("model cache ", Style::new().fg(Color::DarkGray)),
            Span::raw(status.model_cache_dir.as_str()),
        ]),
    ])
    .block(
        Block::default()
            .title(" Filesystem ")
            .borders(Borders::ALL)
            .border_style(Style::new().fg(Color::Green)),
    )
    .wrap(Wrap { trim: false })
    .render(chunks[2], buffer);

    Paragraph::new(vec![
        Line::from(vec![
            Span::styled("queue ", Style::new().fg(Color::DarkGray)),
            Span::styled(
                status.queued_request_count.to_string(),
                Style::new().fg(Color::LightMagenta),
            ),
            Span::raw(" requests  "),
            Span::styled("oldest ", Style::new().fg(Color::DarkGray)),
            Span::styled(
                format!("{} ms", status.oldest_queued_age_ms),
                Style::new().fg(Color::LightMagenta),
            ),
        ]),
        Line::from(vec![
            Span::styled("python lag ", Style::new().fg(Color::DarkGray)),
            Span::styled(
                format!("{} ms", status.python_lag_ms),
                Style::new().fg(Color::LightMagenta),
            ),
        ]),
        Line::from("Alt+X pretty view   Ctrl+C copy selection   Ctrl+D close window"),
    ])
    .block(
        Block::default()
            .title(" Live Flow ")
            .borders(Borders::ALL)
            .border_style(Style::new().fg(Color::Magenta)),
    )
    .render(chunks[3], buffer);
}

fn build_audio_input_device_detail_diagnostic_body_scene(
    body_rect: ClientRect,
    device_state: Option<&AudioInputDeviceWindowState>,
    selection: Option<TerminalSelection>,
    cell_width: i32,
    cell_height: i32,
    scramble_input_device_identifiers: bool,
) -> RenderScene {
    let columns = u16::try_from((body_rect.width() / cell_width.max(1)).max(0)).unwrap_or_default();
    let rows = u16::try_from((body_rect.height() / cell_height.max(1)).max(0)).unwrap_or_default();
    if columns == 0 || rows == 0 {
        return empty_render_scene();
    }

    let area = RatatuiRect::new(0, 0, columns, rows);
    let mut buffer = Buffer::empty(area);
    render_audio_input_device_detail_diagnostic_buffer(
        &mut buffer,
        area,
        device_state,
        scramble_input_device_identifiers,
    );
    ratatui_buffer_to_scene(body_rect, &buffer, selection, cell_width, cell_height)
}

#[expect(
    clippy::too_many_lines,
    reason = "the microphone diagnostics TUI composes status, chart, and controls together"
)]
fn render_audio_input_device_detail_diagnostic_buffer(
    buffer: &mut Buffer,
    area: RatatuiRect,
    device_state: Option<&AudioInputDeviceWindowState>,
    scramble_input_device_identifiers: bool,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(7),
            Constraint::Min(7),
            Constraint::Length(5),
        ])
        .split(area);
    let Some(device_state) = device_state else {
        Paragraph::new("No microphone selected")
            .block(Block::default().title(" Microphone ").borders(Borders::ALL))
            .render(area, buffer);
        return;
    };

    let duration_seconds = device_state.runtime.duration_seconds();
    let sample_rate = device_state.runtime.sample_rate_hz();
    let status = if device_state.is_recording() {
        "recording"
    } else if device_state.is_playing() {
        "playing"
    } else if device_state.armed_for_record {
        "armed"
    } else {
        "idle"
    };
    let header = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("Device ", Style::new().fg(Color::DarkGray)),
            Span::styled(
                device_state.device.name.clone(),
                Style::new().fg(Color::White).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("Status ", Style::new().fg(Color::DarkGray)),
            Span::styled(
                status,
                Style::new()
                    .fg(Color::LightRed)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("   "),
            Span::styled("Duration ", Style::new().fg(Color::DarkGray)),
            Span::styled(
                format!("{duration_seconds:.2}s"),
                Style::new().fg(Color::LightCyan),
            ),
            Span::raw("   "),
            Span::styled("Sample rate ", Style::new().fg(Color::DarkGray)),
            Span::styled(
                format!("{sample_rate} Hz"),
                Style::new().fg(Color::LightGreen),
            ),
        ]),
        Line::from(input_device_identifier_display_text(
            &device_state.device.id,
            scramble_input_device_identifiers,
        )),
    ])
    .block(
        Block::default()
            .title(" Microphone ")
            .borders(Borders::ALL)
            .border_style(Style::new().fg(Color::Cyan)),
    )
    .wrap(Wrap { trim: true });
    header.render(chunks[0], buffer);

    let points = waveform_chart_points(&device_state.runtime.samples(), duration_seconds.max(1.0));
    let datasets = vec![
        Dataset::default()
            .name("waveform")
            .marker(symbols::Marker::Braille)
            .graph_type(GraphType::Line)
            .style(Style::new().fg(Color::LightCyan))
            .data(&points),
    ];
    let chart = Chart::new(datasets)
        .block(
            Block::default()
                .title(" Audio Buffer ")
                .borders(Borders::ALL)
                .border_style(Style::new().fg(Color::Blue)),
        )
        .x_axis(Axis::default().bounds([0.0, duration_seconds.max(1.0)]))
        .y_axis(Axis::default().bounds([-1.0, 1.0]));
    chart.render(chunks[1], buffer);

    let selection_line = device_state.runtime.selection.map_or_else(
        || "Selection none".to_owned(),
        |selection| {
            format!(
                "Selection {:.2}s - {:.2}s ({:.2}s)",
                selection.begin_seconds,
                selection.end_seconds,
                selection.duration_seconds()
            )
        },
    );
    let footer = Paragraph::new(vec![
        Line::from(vec![
            Span::styled(
                "Enter",
                Style::new()
                    .fg(Color::LightCyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" record  "),
            Span::styled(
                "Space",
                Style::new()
                    .fg(Color::LightCyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" play/pause  "),
            Span::styled(
                "J/K/L",
                Style::new()
                    .fg(Color::LightCyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" shuttle  "),
            Span::styled(
                "Alt+X",
                Style::new()
                    .fg(Color::LightCyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" pretty"),
        ]),
        Line::from(selection_line),
    ])
    .block(
        Block::default()
            .title(" Controls ")
            .borders(Borders::ALL)
            .border_style(Style::new().fg(Color::DarkGray)),
    );
    footer.render(chunks[2], buffer);
}

#[expect(
    clippy::cast_precision_loss,
    reason = "ratatui chart coordinates are display-scale f64 values"
)]
fn waveform_chart_points(samples: &[f32], duration_seconds: f64) -> Vec<(f64, f64)> {
    if samples.is_empty() {
        return vec![(0.0, 0.0), (duration_seconds, 0.0)];
    }
    let max_points = 240usize;
    let points = samples.len().min(max_points);
    let peak = samples
        .iter()
        .map(|sample| sample.abs())
        .fold(0.0_f32, f32::max)
        .max(0.015);
    (0..points)
        .map(|index| {
            let sample_index = (index * samples.len()) / points;
            let x = (index as f64 / points.saturating_sub(1).max(1) as f64) * duration_seconds;
            (
                x,
                f64::from((samples[sample_index] / peak).clamp(-1.0, 1.0)),
            )
        })
        .collect()
}

fn build_audio_input_device_diagnostic_body_scene(
    body_rect: ClientRect,
    devices: &[AudioInputDeviceSummary],
    selected_index: usize,
    selection: Option<TerminalSelection>,
    cell_width: i32,
    cell_height: i32,
    scramble_input_device_identifiers: bool,
) -> RenderScene {
    let columns = u16::try_from((body_rect.width() / cell_width.max(1)).max(0)).unwrap_or_default();
    let rows = u16::try_from((body_rect.height() / cell_height.max(1)).max(0)).unwrap_or_default();
    if columns == 0 || rows == 0 {
        return empty_render_scene();
    }

    let area = RatatuiRect::new(0, 0, columns, rows);
    let mut buffer = Buffer::empty(area);
    render_audio_input_device_diagnostic_buffer(
        &mut buffer,
        area,
        devices,
        selected_index,
        scramble_input_device_identifiers,
    );
    ratatui_buffer_to_scene(body_rect, &buffer, selection, cell_width, cell_height)
}

fn build_launcher_diagnostic_body_scene(
    body_rect: ClientRect,
    selected_index: usize,
    virtual_cursor: Option<ClientPoint>,
    selection: Option<TerminalSelection>,
    cell_width: i32,
    cell_height: i32,
) -> RenderScene {
    let columns = u16::try_from((body_rect.width() / cell_width.max(1)).max(0)).unwrap_or_default();
    let rows = u16::try_from((body_rect.height() / cell_height.max(1)).max(0)).unwrap_or_default();
    if columns == 0 || rows == 0 {
        return empty_render_scene();
    }

    let area = RatatuiRect::new(0, 0, columns, rows);
    let mut buffer = Buffer::empty(area);
    render_launcher_diagnostic_buffer(&mut buffer, area, selected_index, virtual_cursor);
    ratatui_buffer_to_scene(body_rect, &buffer, selection, cell_width, cell_height)
}

fn render_launcher_diagnostic_buffer(
    buffer: &mut Buffer,
    area: RatatuiRect,
    selected_index: usize,
    virtual_cursor: Option<ClientPoint>,
) {
    let specs = scene_button_specs(SceneWindowKind::Launcher);
    let selected = specs.get(selected_index).unwrap_or(&specs[0]);
    let cursor_label = virtual_cursor
        .and_then(|point| point.to_win32_point().ok())
        .map_or_else(
            || "unset".to_owned(),
            |point| format!("{}, {}", point.x, point.y),
        );
    let chunks = launcher_diagnostic_chunks(area);

    let header = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("Mode ", Style::new().fg(Color::DarkGray)),
            Span::styled(
                "diagnostics",
                Style::new()
                    .fg(Color::LightCyan)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("Selected ", Style::new().fg(Color::DarkGray)),
            Span::styled(
                selected.label,
                Style::new().fg(Color::White).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("Cards ", Style::new().fg(Color::DarkGray)),
            Span::styled(specs.len().to_string(), Style::new().fg(Color::LightGreen)),
        ]),
        Line::from(vec![
            Span::styled("Intent ", Style::new().fg(Color::DarkGray)),
            Span::styled("main menu", Style::new().fg(Color::LightBlue)),
        ]),
        Line::from(vec![
            Span::styled("Virtual cursor ", Style::new().fg(Color::DarkGray)),
            Span::styled(cursor_label, Style::new().fg(Color::LightYellow)),
        ]),
    ])
    .block(
        Block::default()
            .title(" Launcher ")
            .borders(Borders::ALL)
            .border_style(Style::new().fg(Color::Cyan)),
    )
    .wrap(Wrap { trim: true });
    header.render(chunks[0], buffer);

    let items: Vec<ListItem<'_>> = specs
        .iter()
        .enumerate()
        .map(|(index, spec)| launcher_diagnostic_item(index, selected_index, spec))
        .collect();
    let list = List::new(items).block(
        Block::default()
            .title(" Actions ")
            .borders(Borders::ALL)
            .border_style(Style::new().fg(Color::Blue)),
    );
    list.render(chunks[1], buffer);

    let footer = Paragraph::new(vec![
        Line::from(vec![
            Span::styled(
                "Arrow keys / Tab",
                Style::new()
                    .fg(Color::LightCyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" navigate  "),
            Span::styled(
                "Enter / Space",
                Style::new()
                    .fg(Color::LightCyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" invoke"),
        ]),
        Line::from(vec![
            Span::styled(
                "Alt+X",
                Style::new()
                    .fg(Color::LightCyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" pretty view"),
        ]),
    ])
    .block(
        Block::default()
            .title(" Controls ")
            .borders(Borders::ALL)
            .border_style(Style::new().fg(Color::DarkGray)),
    );
    footer.render(chunks[2], buffer);
}

fn launcher_diagnostic_item<'a>(
    index: usize,
    selected_index: usize,
    spec: &SceneButtonSpec,
) -> ListItem<'a> {
    let selected = index == selected_index;
    let base_style = if selected {
        Style::new()
            .fg(Color::White)
            .bg(Color::Rgb(50, 66, 98))
            .add_modifier(Modifier::BOLD)
    } else {
        Style::new().fg(Color::Gray)
    };

    ListItem::new(vec![
        Line::from(vec![
            Span::styled(if selected { "> " } else { "  " }, base_style),
            Span::styled(spec.label, base_style),
        ]),
        Line::from(vec![
            Span::styled("    ", base_style),
            Span::styled(spec.tooltip, base_style.fg(Color::LightBlue)),
        ]),
    ])
    .style(base_style)
}

fn launcher_diagnostic_chunks(area: RatatuiRect) -> std::rc::Rc<[RatatuiRect]> {
    Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(7),
            Constraint::Min(5),
            Constraint::Length(4),
        ])
        .split(area)
}

fn ratatui_block_inner(rect: RatatuiRect) -> RatatuiRect {
    let x = rect.x.saturating_add(1);
    let y = rect.y.saturating_add(1);
    let width = rect.width.saturating_sub(2);
    let height = rect.height.saturating_sub(2);
    RatatuiRect::new(x, y, width, height)
}

fn ratatui_rect_to_client_rect(
    body_rect: ClientRect,
    rect: RatatuiRect,
    cell_width: i32,
    cell_height: i32,
) -> ClientRect {
    let left = body_rect.left() + i32::from(rect.x) * cell_width;
    let top = body_rect.top() + i32::from(rect.y) * cell_height;
    ClientRect::new(
        left,
        top,
        left + i32::from(rect.width) * cell_width,
        top + i32::from(rect.height) * cell_height,
    )
}

#[expect(
    clippy::too_many_lines,
    reason = "the ratatui diagnostic layout is clearer when the three blocks are composed together"
)]
fn render_audio_input_device_diagnostic_buffer(
    buffer: &mut Buffer,
    area: RatatuiRect,
    devices: &[AudioInputDeviceSummary],
    selected_index: usize,
    scramble_input_device_identifiers: bool,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),
            Constraint::Min(4),
            Constraint::Length(4),
        ])
        .split(area);
    let selected_name = devices
        .get(selected_index)
        .map_or("None", |device| device.name.as_str());
    let header = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("Mode ", Style::new().fg(Color::DarkGray)),
            Span::styled(
                "diagnostics",
                Style::new()
                    .fg(Color::LightCyan)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("Selected ", Style::new().fg(Color::DarkGray)),
            Span::styled(
                selected_name.to_owned(),
                Style::new().fg(Color::White).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("Devices ", Style::new().fg(Color::DarkGray)),
            Span::styled(
                devices.len().to_string(),
                Style::new().fg(Color::LightGreen),
            ),
        ]),
    ])
    .block(
        Block::default()
            .title(" Audio Devices ")
            .borders(Borders::ALL)
            .border_style(Style::new().fg(Color::Cyan)),
    )
    .wrap(Wrap { trim: true });
    header.render(chunks[0], buffer);

    let items = if devices.is_empty() {
        vec![ListItem::new(Line::styled(
            "No active Windows recording devices found",
            Style::new().fg(Color::LightYellow),
        ))]
    } else {
        devices
            .iter()
            .enumerate()
            .map(|(index, device)| {
                audio_input_device_diagnostic_item(
                    index,
                    selected_index,
                    device,
                    scramble_input_device_identifiers,
                )
            })
            .collect()
    };
    let list = List::new(items).block(
        Block::default()
            .title(" Active Microphones ")
            .borders(Borders::ALL)
            .border_style(Style::new().fg(Color::Blue)),
    );
    list.render(chunks[1], buffer);

    let footer = Paragraph::new(vec![
        Line::from(vec![
            Span::styled(
                "Up/Down",
                Style::new()
                    .fg(Color::LightCyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" navigate  "),
            Span::styled(
                "Enter",
                Style::new()
                    .fg(Color::LightCyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" choose  "),
            Span::styled(
                "Esc",
                Style::new()
                    .fg(Color::LightCyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" close"),
        ]),
        Line::from(vec![
            Span::styled(
                "Alt+X",
                Style::new()
                    .fg(Color::LightCyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" pretty view  "),
            Span::styled(
                "Alt+R",
                Style::new()
                    .fg(Color::LightCyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" Windows recording devices"),
        ]),
    ])
    .block(
        Block::default()
            .title(" Controls ")
            .borders(Borders::ALL)
            .border_style(Style::new().fg(Color::DarkGray)),
    );
    footer.render(chunks[2], buffer);
}

fn audio_input_device_diagnostic_item<'a>(
    index: usize,
    selected_index: usize,
    device: &AudioInputDeviceSummary,
    scramble_input_device_identifiers: bool,
) -> ListItem<'a> {
    let selected = index == selected_index;
    let sample_rate = device.sample_rate_hz.map_or_else(
        || "sample rate unknown".to_owned(),
        |rate| format!("{rate} Hz"),
    );
    let default_marker = if device.is_default { " default" } else { "" };
    let base_style = if selected {
        Style::new()
            .fg(Color::White)
            .bg(Color::Rgb(28, 76, 88))
            .add_modifier(Modifier::BOLD)
    } else {
        Style::new().fg(Color::Gray)
    };

    ListItem::new(vec![
        Line::from(vec![
            Span::styled(if selected { "> " } else { "  " }, base_style),
            Span::styled(device.name.clone(), base_style),
            Span::styled(default_marker, base_style.fg(Color::LightGreen)),
        ]),
        Line::from(vec![
            Span::styled("    ", base_style),
            Span::styled(sample_rate, base_style.fg(Color::LightBlue)),
            Span::styled(" | ", base_style),
            Span::styled(device.state.clone(), base_style.fg(Color::LightGreen)),
        ]),
        Line::from(vec![
            Span::styled("    ", base_style),
            Span::styled(
                input_device_identifier_display_text(&device.id, scramble_input_device_identifiers),
                base_style.fg(Color::DarkGray),
            ),
        ]),
    ])
    .style(base_style)
}

fn ratatui_buffer_to_scene(
    body_rect: ClientRect,
    buffer: &Buffer,
    selection: Option<TerminalSelection>,
    cell_width: i32,
    cell_height: i32,
) -> RenderScene {
    let mut scene = empty_render_scene();
    for row in 0..buffer.area.height {
        for column in 0..buffer.area.width {
            let Some(cell) = buffer.cell((column, row)) else {
                continue;
            };
            let column_i32 = i32::from(column);
            let row_i32 = i32::from(row);
            let terminal_cell = TerminalCellPoint::new(column_i32, row_i32);
            let selected = selection.is_some_and(|selection| selection.contains(terminal_cell));
            let rect = RECT {
                left: body_rect.left() + column_i32 * cell_width,
                top: body_rect.top() + row_i32 * cell_height,
                right: body_rect.left() + (column_i32 + 1) * cell_width,
                bottom: body_rect.top() + (row_i32 + 1) * cell_height,
            };

            if selected {
                push_panel(
                    &mut scene,
                    rect,
                    DIAGNOSTIC_SELECTION_BACKGROUND,
                    PanelEffect::TerminalFill,
                );
            } else if let Some(background) = ratatui_color_to_rgba(cell.bg) {
                push_panel(&mut scene, rect, background, PanelEffect::TerminalFill);
            }

            let symbol = cell.symbol();
            let Some(character) = symbol
                .chars()
                .next()
                .filter(|character| !character.is_whitespace())
            else {
                continue;
            };
            push_glyph(
                &mut scene,
                rect,
                character,
                if selected {
                    DIAGNOSTIC_SELECTION_FOREGROUND
                } else {
                    ratatui_color_to_rgba(cell.fg).unwrap_or(DIAGNOSTIC_TEXT_COLOR)
                },
            );
        }
    }
    scene
}

fn empty_render_scene() -> RenderScene {
    RenderScene {
        panels: Vec::new(),
        glyphs: Vec::new(),
        sprites: Vec::new(),
        overlay_panels: Vec::new(),
    }
}

fn ratatui_color_to_rgba(color: Color) -> Option<[f32; 4]> {
    let [red, green, blue] = match color {
        Color::Reset => return None,
        Color::Black => [10, 12, 14],
        Color::Red => [210, 76, 76],
        Color::Green => [88, 188, 116],
        Color::Yellow => [212, 176, 74],
        Color::Blue => [76, 126, 220],
        Color::Magenta => [190, 110, 210],
        Color::Cyan => [80, 190, 210],
        Color::Gray => [190, 196, 204],
        Color::DarkGray => [102, 112, 124],
        Color::LightRed => [238, 112, 112],
        Color::LightGreen => [124, 224, 148],
        Color::LightYellow => [240, 210, 112],
        Color::LightBlue => [122, 172, 255],
        Color::LightMagenta => [220, 150, 238],
        Color::LightCyan => [126, 230, 238],
        Color::White => [242, 246, 250],
        Color::Rgb(red, green, blue) => [red, green, blue],
        Color::Indexed(index) => [index, index, index],
    };
    Some([
        f32::from(red) / 255.0,
        f32::from(green) / 255.0,
        f32::from(blue) / 255.0,
        1.0,
    ])
}

#[must_use]
pub fn layout_scene_buttons(
    body_rect: ClientRect,
    count: usize,
    max_button_size: i32,
) -> Vec<SceneButtonLayout> {
    if count == 0 {
        return Vec::new();
    }

    let metrics = scene_button_grid_metrics(body_rect, count, max_button_size.max(1));
    let columns = metrics.columns;
    let rows = count.div_ceil(columns);
    let columns_i32 = i32::try_from(columns).unwrap_or(1).max(1);
    let rows_i32 = i32::try_from(rows).unwrap_or(1).max(1);
    let total_width =
        columns_i32 * metrics.button_size + (columns_i32 - 1).max(0) * metrics.button_gap;
    let row_height = metrics.button_size + metrics.label_gap + metrics.label_height;
    let total_height = rows_i32 * row_height + (rows_i32 - 1).max(0) * metrics.button_gap;
    let start_x = body_rect.left() + ((body_rect.width() - total_width).max(0) / 2);
    let start_y = body_rect.top() + ((body_rect.height() - total_height).max(0) / 2);

    let mut layouts = Vec::with_capacity(count);
    for index in 0..count {
        let column = i32::try_from(index % columns).unwrap_or_default();
        let row = i32::try_from(index / columns).unwrap_or_default();
        let left = start_x + column * (metrics.button_size + metrics.button_gap);
        let top = start_y + row * (row_height + metrics.button_gap);
        let card_rect = ClientRect::new(
            left,
            top,
            left + metrics.button_size,
            top + metrics.button_size,
        );
        layouts.push(SceneButtonLayout {
            sprite_rect: card_rect.inset(metrics.sprite_inset),
            label_rect: ClientRect::new(
                card_rect.left(),
                card_rect.bottom() + metrics.label_gap,
                card_rect.right(),
                card_rect.bottom() + metrics.label_gap + metrics.label_height,
            ),
            card_rect,
        });
    }

    layouts
}

fn scene_button_grid_metrics(
    body_rect: ClientRect,
    count: usize,
    max_button_size: i32,
) -> SceneButtonGridMetrics {
    let mut best_metrics = scene_button_grid_candidate(body_rect, count, 1, max_button_size);
    for columns in 2..=count {
        let candidate = scene_button_grid_candidate(body_rect, count, columns, max_button_size);
        if candidate.button_size > best_metrics.button_size
            || (candidate.button_size == best_metrics.button_size
                && candidate.columns > best_metrics.columns)
        {
            best_metrics = candidate;
        }
    }

    best_metrics
}

fn scene_button_grid_candidate(
    body_rect: ClientRect,
    count: usize,
    columns: usize,
    max_button_size: i32,
) -> SceneButtonGridMetrics {
    let rows = count.div_ceil(columns);
    let columns_i32 = i32::try_from(columns).unwrap_or(1).max(1);
    let rows_i32 = i32::try_from(rows).unwrap_or(1).max(1);
    let button_gap =
        (body_rect.width().min(body_rect.height()) / 20).clamp(MIN_BUTTON_GAP, MAX_BUTTON_GAP);
    let width_budget = body_rect.width() - ((columns_i32 - 1).max(0) * button_gap);
    let height_budget = body_rect.height() - ((rows_i32 - 1).max(0) * button_gap);
    let provisional_button_size = (width_budget / columns_i32)
        .min(height_budget / rows_i32)
        .clamp(1, max_button_size);
    let label_gap =
        (provisional_button_size / 18).clamp(MIN_BUTTON_LABEL_GAP, MAX_BUTTON_LABEL_GAP);
    let label_height =
        (provisional_button_size / 7).clamp(MIN_BUTTON_LABEL_HEIGHT, MAX_BUTTON_LABEL_HEIGHT);
    let height_budget = body_rect.height()
        - ((rows_i32 - 1).max(0) * button_gap)
        - (rows_i32 * (label_gap + label_height));
    let button_size = (width_budget / columns_i32)
        .min(height_budget / rows_i32)
        .clamp(1, max_button_size);

    SceneButtonGridMetrics {
        columns,
        button_size,
        button_gap,
        label_gap,
        label_height,
        sprite_inset: (button_size / 12).clamp(MIN_BUTTON_SPRITE_INSET, MAX_BUTTON_SPRITE_INSET),
    }
}

#[must_use]
pub fn compute_button_visual_state(
    rect: ClientRect,
    pointer: Option<ClientPoint>,
    pressed: bool,
    last_clicked: Option<Instant>,
    active: bool,
    now: Instant,
) -> ButtonVisualState {
    let hover_near = pointer.map_or(0.0, |pointer| proximity_to_rect(rect, pointer));
    let hovered = pointer.is_some_and(|pointer| rect.contains(pointer));
    let click_decay = last_clicked.map_or(0.0, |clicked_at| {
        let elapsed = now.saturating_duration_since(clicked_at);
        if elapsed >= CLICK_DECAY_DURATION {
            0.0
        } else {
            1.0 - (elapsed.as_secs_f32() / CLICK_DECAY_DURATION.as_secs_f32())
        }
    });

    ButtonVisualState {
        hover_near,
        hovered,
        pressed,
        click_decay,
        active,
    }
}

fn build_scene_shell(
    layout: TerminalLayout,
    scene_kind: SceneWindowKind,
    window_chrome_buttons_state: WindowChromeButtonsState,
) -> RenderScene {
    let mut scene = RenderScene {
        panels: Vec::new(),
        glyphs: Vec::new(),
        sprites: Vec::new(),
        overlay_panels: Vec::new(),
    };

    push_panel(
        &mut scene,
        layout.content_frame_rect().to_win32_rect(),
        preferred_background_color(),
        PanelEffect::BlueBackground,
    );
    push_window_garden_frame(&mut scene, layout);
    push_panel(
        &mut scene,
        layout.title_bar_rect().to_win32_rect(),
        preferred_title_bar_color(window_chrome_buttons_state.focused),
        PanelEffect::TitleBar,
    );
    push_panel(
        &mut scene,
        layout.terminal_panel_rect().to_win32_rect(),
        [0.09, 0.10, 0.12, 0.96],
        PanelEffect::SceneBody,
    );
    push_window_chrome_buttons(&mut scene, layout, window_chrome_buttons_state);
    push_title_text(
        &mut scene,
        layout.title_text_rect().to_win32_rect(),
        scene_kind.title(),
        [0.95, 0.95, 0.98, 1.0],
    );

    scene
}

fn push_virtual_cursor_pointer(
    scene: &mut RenderScene,
    virtual_cursor: Option<ClientPoint>,
    sprite: SpriteId,
    color: [f32; 4],
) {
    // windowing[impl virtual-cursor.os-cursor-sprite]
    let Some(point) = virtual_cursor.and_then(|point| point.to_win32_point().ok()) else {
        return;
    };
    let size = 66;
    let left = point.x - 6;
    let top = point.y - 4;
    let pointer_rect = ClientRect::new(left, top, left + size, top + size);
    let shadow_rect = ClientRect::new(
        pointer_rect.left() + 4,
        pointer_rect.top() + 5,
        pointer_rect.right() + 4,
        pointer_rect.bottom() + 5,
    );

    push_sprite(
        scene,
        shadow_rect.to_win32_rect(),
        [0.0, 0.0, 0.0, 0.38],
        sprite,
    );
    push_sprite(scene, pointer_rect.to_win32_rect(), color, sprite);
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "the hover proximity is normalized into the 0..=1 range before conversion"
)]
fn proximity_to_rect(rect: ClientRect, pointer: ClientPoint) -> f32 {
    let Ok(point) = pointer.to_win32_point() else {
        return 0.0;
    };
    let dx = if point.x < rect.left() {
        f64::from(rect.left() - point.x)
    } else if point.x > rect.right() {
        f64::from(point.x - rect.right())
    } else {
        0.0
    };
    let dy = if point.y < rect.top() {
        f64::from(rect.top() - point.y)
    } else if point.y > rect.bottom() {
        f64::from(point.y - rect.bottom())
    } else {
        0.0
    };
    let distance = (dx * dx + dy * dy).sqrt();
    (1.0 - (distance / BUTTON_PROXIMITY_RADIUS_PX)).clamp(0.0, 1.0) as f32
}

#[cfg(test)]
mod tests {
    use crate::app::windows_d3d12_renderer::window_garden_shader_data;

    use super::*;

    fn sample_layout() -> TerminalLayout {
        TerminalLayout {
            client_width: 1040,
            client_height: 680,
            cell_width: 8,
            cell_height: 16,
            diagnostic_panel_visible: true,
        }
    }

    fn sample_audio_input_device(id: &str, name: &str) -> AudioInputDeviceSummary {
        AudioInputDeviceSummary {
            id: id.to_owned(),
            name: name.to_owned(),
            is_default: false,
            state: "active".to_owned(),
            icon: "microphone".to_owned(),
            sample_rate_hz: None,
        }
    }

    fn sample_audio_input_device_window() -> AudioInputDeviceWindowState {
        let mut device = sample_audio_input_device("endpoint-a", "Studio Mic");
        device.sample_rate_hz = Some(48_000);
        AudioInputDeviceWindowState::new(device)
    }

    fn sample_audio_daemon_status() -> AudioTranscriptionDaemonStatusReport {
        AudioTranscriptionDaemonStatusReport {
            daemon_source_dir: "python/whisperx-daemon".to_owned(),
            uv_venv_dir: "cache/python/whisperx-daemon/.venv".to_owned(),
            model_cache_dir: "cache/models/whisperx".to_owned(),
            tensor_dtype: "f32le".to_owned(),
            tensor_mel_bins: 80,
            tensor_frames: 3_000,
            tensor_values: 240_000,
            tensor_bytes: 960_000,
            shared_memory_slot_bytes: 960_000,
            shared_memory_minimum_slots: 3,
            shared_memory_total_bytes: 2_880_000,
            queued_request_count: 0,
            oldest_queued_age_ms: 0,
            python_lag_ms: 0,
            control_transport: "windows named pipe".to_owned(),
            payload_transport: "rust-owned shared-memory slot".to_owned(),
            python_entrypoint: "teamy_whisperx_daemon".to_owned(),
            selected_model: "large-v3".to_owned(),
            available_models: vec![
                "large-v3".to_owned(),
                "large-v2".to_owned(),
                "medium.en".to_owned(),
                "small.en".to_owned(),
                "base.en".to_owned(),
                "tiny.en".to_owned(),
            ],
        }
    }

    #[test]
    fn scene_button_layouts_center_buttons_in_the_body() {
        let layouts =
            layout_scene_buttons(ClientRect::new(0, 0, 1100, 720), 3, DEFAULT_MAX_BUTTON_SIZE);

        assert_eq!(layouts.len(), 3);
        assert!(layouts[0].card_rect.left() < layouts[1].card_rect.left());
        assert_eq!(layouts[0].card_rect.top(), layouts[1].card_rect.top());
    }

    #[test]
    fn scene_button_layouts_shrink_to_fit_small_windows() {
        let body_rect = ClientRect::new(0, 0, 560, 340);
        let layouts = layout_scene_buttons(body_rect, 3, DEFAULT_MAX_BUTTON_SIZE);

        assert_eq!(layouts.len(), 3);
        assert!(layouts[0].card_rect.width() < DEFAULT_MAX_BUTTON_SIZE);
        for layout in layouts {
            assert!(layout.card_rect.left() >= body_rect.left());
            assert!(layout.card_rect.right() <= body_rect.right());
            assert!(layout.label_rect.bottom() <= body_rect.bottom());
        }
    }

    #[test]
    fn scene_button_layouts_respect_larger_scaled_maximum_sizes() {
        let body_rect = ClientRect::new(0, 0, 2200, 1400);
        let layouts = layout_scene_buttons(body_rect, 3, DEFAULT_MAX_BUTTON_SIZE * 2);

        assert_eq!(layouts.len(), 3);
        assert!(layouts[0].card_rect.width() > DEFAULT_MAX_BUTTON_SIZE);
        assert!(layouts[0].card_rect.width() <= DEFAULT_MAX_BUTTON_SIZE * 2);
    }

    #[test]
    fn scene_button_hit_rect_covers_card_and_label() {
        let layout = SceneButtonLayout {
            card_rect: ClientRect::new(10, 20, 110, 120),
            sprite_rect: ClientRect::new(20, 30, 100, 110),
            label_rect: ClientRect::new(10, 130, 110, 160),
        };

        assert_eq!(layout.hit_rect(), ClientRect::new(10, 20, 110, 160));
    }

    #[test]
    fn click_decay_fades_out_after_the_configured_window() {
        let now = Instant::now();
        let state = compute_button_visual_state(
            ClientRect::new(0, 0, 100, 100),
            None,
            false,
            Some(now - Duration::from_millis(110)),
            false,
            now,
        );

        assert!(state.click_decay > 0.0);

        let expired = compute_button_visual_state(
            ClientRect::new(0, 0, 100, 100),
            None,
            false,
            Some(now - Duration::from_millis(500)),
            false,
            now,
        );

        assert_eq!(expired.click_decay, 0.0);
    }

    // windowing[verify launcher.buttons.terminal]
    // windowing[verify launcher.buttons.storage-placeholder]
    // windowing[verify launcher.buttons.environment-variables-placeholder]
    // windowing[verify launcher.buttons.application-windows-placeholder]
    // windowing[verify launcher.buttons.cursor-gallery]
    // windowing[verify launcher.buttons.demo-mode]
    // windowing[verify launcher.buttons.audio-picker]
    // timeline[verify launcher.button]
    // audio[verify gui.daemon-button]
    #[test]
    fn launcher_scene_specs_expose_primary_actions() {
        let specs = scene_button_specs(SceneWindowKind::Launcher);

        assert!(
            specs
                .iter()
                .any(|spec| spec.action == SceneAction::OpenTerminal)
        );
        assert!(
            specs
                .iter()
                .any(|spec| spec.action == SceneAction::OpenCursorInfo)
        );
        assert!(
            specs
                .iter()
                .any(|spec| spec.action == SceneAction::OpenCursorGallery)
        );
        assert!(
            specs
                .iter()
                .any(|spec| spec.action == SceneAction::OpenDemoMode)
        );
        assert!(
            specs
                .iter()
                .any(|spec| spec.action == SceneAction::OpenStorage)
        );
        assert!(
            specs
                .iter()
                .any(|spec| spec.action == SceneAction::OpenEnvironmentVariables)
        );
        assert!(
            specs
                .iter()
                .any(|spec| spec.action == SceneAction::OpenApplicationWindows)
        );
        assert!(
            specs
                .iter()
                .any(|spec| spec.action == SceneAction::OpenAudioPicker)
        );
        assert!(
            specs
                .iter()
                .any(|spec| spec.action == SceneAction::OpenAudioDaemon)
        );
        assert!(
            specs
                .iter()
                .any(|spec| spec.action == SceneAction::OpenTimeline)
        );
    }

    // timeline[verify start-window.create-or-import]
    // timeline[verify start-window.new-blank]
    // timeline[verify import.tracy.file-picker]
    #[test]
    fn timeline_start_scene_specs_expose_new_and_import() {
        let specs = scene_button_specs(SceneWindowKind::TimelineStart);

        assert_eq!(specs.len(), 2);
        assert_eq!(specs[0].action, SceneAction::CreateBlankTimeline);
        assert_eq!(specs[0].label, "New");
        assert_eq!(specs[1].action, SceneAction::ImportTimeline);
        assert_eq!(specs[1].label, "Import");
    }

    // timeline[verify blank.track-list]
    // timeline[verify blank.add-track-placeholder]
    #[test]
    fn blank_timeline_render_scene_has_track_ruler_and_content_regions() {
        let scene = build_blank_timeline_render_scene(
            sample_layout(),
            WindowChromeButtonsState::default(),
            None,
            None,
            TimelineDocumentVisualState::default(),
        );

        assert!(scene.panels.len() >= 7);
        assert!(scene.glyphs.len() >= "Add TrackBlank timeline".chars().count());
        assert!(
            scene
                .panels
                .iter()
                .any(|panel| matches!(panel.effect, PanelEffect::TerminalPanel))
        );
        assert!(
            scene
                .panels
                .iter()
                .any(|panel| matches!(panel.effect, PanelEffect::TerminalFill))
        );
    }

    // timeline[verify document.window-state]
    #[test]
    fn blank_timeline_render_scene_accepts_document_state() {
        let document = TimelineDocument::blank();
        let scene = build_blank_timeline_render_scene(
            sample_layout(),
            WindowChromeButtonsState::default(),
            Some(&document),
            None,
            TimelineDocumentVisualState::default(),
        );

        assert!(scene.glyphs.len() >= "Blank timeline".chars().count());
    }

    #[test]
    // timeline[verify import.tracy.document]
    // timeline[verify track.kinds]
    fn imported_timeline_render_scene_includes_track_details() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let capture_path = temp_dir.path().join("capture.tracy");
        std::fs::write(&capture_path, [b't', b'r', 253, b'P', 0, 2, 0, 0]).expect("capture");
        let document = TimelineDocument::import_tracy_capture(&capture_path).expect("import");

        let scene = build_blank_timeline_render_scene(
            sample_layout(),
            WindowChromeButtonsState::default(),
            Some(&document),
            None,
            TimelineDocumentVisualState::default(),
        );

        assert!(scene.glyphs.len() >= "Tracing spanscapture.tracyTracy capture".chars().count());
    }

    #[test]
    // timeline[verify content.preview-lanes]
    fn timeline_render_scene_projects_track_preview_block_into_content_area() {
        let mut document = TimelineDocument::blank();
        let _ = document.append_microphone_track();
        let layout = timeline_document_layout(sample_layout().terminal_panel_rect().inset(24));
        let row_rect = timeline_track_row_rect(layout.scrollport_rect, 0, 0).expect("row rect");
        let expected_preview_rect = timeline_track_preview_clip_rect(
            row_rect,
            layout.scrollport_rect,
            document.viewport(),
            timeline_track_preview_time_range(&document.tracks()[0], None),
        )
        .expect("preview rect");

        let scene = build_blank_timeline_render_scene(
            sample_layout(),
            WindowChromeButtonsState::default(),
            Some(&document),
            None,
            TimelineDocumentVisualState::default(),
        );

        assert!(scene.panels.iter().any(|panel| {
            panel.rect == expected_preview_rect.to_win32_rect()
                && panel.effect == PanelEffect::TerminalFill
                && panel.color == timeline_track_preview_color(TimelineTrackKind::Audio)
        }));
    }

    #[test]
    fn timeline_track_preview_clip_rect_clips_partially_visible_blocks_without_compressing_them() {
        let mut document = TimelineDocument::blank();
        let _ = document.append_microphone_track();
        document.set_viewport(TimelineViewport::new(
            TimelineTimeNs::from_duration(Time::new::<second>(0.2)),
            Time::new::<uom::si::time::millisecond>(1.0),
        ));
        let layout = timeline_document_layout(sample_layout().terminal_panel_rect().inset(24));
        let row_rect = timeline_track_row_rect(layout.scrollport_rect, 0, 0).expect("row rect");

        let preview_rect = timeline_track_preview_clip_rect(
            row_rect,
            layout.scrollport_rect,
            document.viewport(),
            timeline_track_preview_time_range(&document.tracks()[0], None),
        )
        .expect("preview rect");

        assert_eq!(preview_rect.left(), layout.scrollport_rect.left());
        assert_eq!(preview_rect.right(), layout.scrollport_rect.left() + 120);
    }

    #[test]
    fn waveform_visible_sample_range_tracks_zoomed_visible_slice() {
        let full_range = TimelineTimeRangeNs::new(
            TimelineTimeNs::ZERO,
            TimelineTimeNs::from_duration(Time::new::<second>(1.0)),
        );
        let visible_range = TimelineTimeRangeNs::new(
            TimelineTimeNs::from_duration(Time::new::<second>(0.25)),
            TimelineTimeNs::from_duration(Time::new::<second>(0.5)),
        );

        let sample_range = waveform_visible_sample_range(10, full_range, visible_range);

        assert_eq!(sample_range, 2..5);
    }

    #[test]
    fn waveform_visible_sample_range_clamps_when_preview_is_partially_offscreen() {
        let full_range = TimelineTimeRangeNs::new(
            TimelineTimeNs::ZERO,
            TimelineTimeNs::from_duration(Time::new::<second>(1.0)),
        );
        let visible_range = TimelineTimeRangeNs::new(
            TimelineTimeNs::from_duration(Time::new::<second>(-0.2)),
            TimelineTimeNs::from_duration(Time::new::<second>(0.3)),
        );

        let sample_range = waveform_visible_sample_range(10, full_range, visible_range);

        assert_eq!(sample_range, 0..3);
    }

    #[test]
    // timeline[verify viewport.scrollbars]
    fn timeline_render_scene_draws_scrollbar_thumbs() {
        let mut document = TimelineDocument::blank();
        let _ = document.append_microphone_track();
        let layout = timeline_document_layout(sample_layout().terminal_panel_rect().inset(24));
        let horizontal_thumb = timeline_horizontal_scrollbar_thumb_rect(
            layout,
            Some(&document),
            document.viewport(),
            None,
        )
        .expect("horizontal thumb rect");
        let vertical_thumb =
            timeline_vertical_scrollbar_thumb_rect(layout, document.tracks().len(), 0)
                .expect("vertical thumb rect");

        let scene = build_blank_timeline_render_scene(
            sample_layout(),
            WindowChromeButtonsState::default(),
            Some(&document),
            None,
            TimelineDocumentVisualState::default(),
        );

        assert!(scene.panels.iter().any(|panel| {
            panel.rect == horizontal_thumb.to_win32_rect()
                && panel.effect == PanelEffect::TerminalFill
                && panel.color == TIMELINE_SCROLLBAR_THUMB
        }));
        assert!(scene.panels.iter().any(|panel| {
            panel.rect == vertical_thumb.to_win32_rect()
                && panel.effect == PanelEffect::TerminalFill
                && panel.color == TIMELINE_SCROLLBAR_THUMB
        }));
    }

    #[test]
    // timeline[verify selection.rectangle]
    fn timeline_visible_selection_rect_is_limited_to_selected_rows() {
        let document = TimelineDocument::blank();
        let layout = timeline_document_layout(sample_layout().terminal_panel_rect().inset(24));
        let selection = TimelineRectSelection::new(
            TimelineTimeRangeNs::new(
                TimelineTimeNs::new(250_000_000),
                TimelineTimeNs::new(750_000_000),
            ),
            TimelineTrackVerticalRange::new(
                10,
                TIMELINE_TRACK_ROW_HEIGHT + TIMELINE_TRACK_ROW_GAP + 8,
            ),
        );

        let rect = timeline_visible_selection_rect(layout, document.viewport(), selection, 0, 2)
            .expect("selection rect");

        assert!(rect.top() > layout.scrollport_rect.top());
        assert!(rect.bottom() < layout.scrollport_rect.bottom());
        assert!(rect.height() < layout.scrollport_rect.height());
    }

    #[test]
    // timeline[verify selection.ruler-all-tracks]
    fn timeline_ruler_selection_band_spans_the_full_time_axis_for_all_track_selection() {
        let document = TimelineDocument::blank();
        let layout = timeline_document_layout(sample_layout().terminal_panel_rect().inset(24));
        let selection = TimelineRectSelection::new(
            TimelineTimeRangeNs::new(
                TimelineTimeNs::new(125_000_000),
                TimelineTimeNs::new(625_000_000),
            ),
            TimelineTrackVerticalRange::new(0, timeline_track_world_height(6)),
        );

        let band_rect =
            timeline_visible_selection_time_band_rect(layout, document.viewport(), selection)
                .expect("ruler band rect");

        assert_eq!(band_rect.top(), layout.time_axis_rect.top() + 8);
        assert_eq!(band_rect.bottom(), layout.time_axis_rect.bottom() - 8);
        assert!(band_rect.left() >= layout.time_axis_rect.left());
        assert!(band_rect.right() <= layout.time_axis_rect.right());
    }

    // timeline[verify add-track.workflow]
    #[test]
    fn timeline_add_track_scene_specs_expose_microphone_text_tracing_and_back() {
        let specs = scene_button_specs(SceneWindowKind::TimelineAddTrack);

        assert_eq!(specs.len(), 5);
        assert_eq!(specs[0].action, SceneAction::AppendMicrophoneTrack);
        assert_eq!(specs[1].action, SceneAction::AppendTranscriptionTrack);
        assert_eq!(specs[1].sprite, SpriteId::Transcription);
        assert_eq!(specs[2].action, SceneAction::AppendTextTrack);
        assert_eq!(specs[2].sprite, SpriteId::Terminal);
        assert_eq!(specs[3].action, SceneAction::ImportTimeline);
        assert_eq!(specs[4].action, SceneAction::CloseTimelineTrackMenu);
        assert_eq!(specs[4].sprite, SpriteId::Back);
    }

    #[test]
    fn timeline_audio_track_panel_uses_record_playback_and_loopback_effects() {
        let mut document = TimelineDocument::blank();
        let _ = document.append_microphone_track();
        let state = sample_audio_input_device_window();
        let layout = timeline_document_layout(sample_layout().terminal_panel_rect().inset(24));
        let playback_rect = timeline_track_playback_button_rect(layout, 0).expect("playback rect");
        let loopback_rect = timeline_track_loopback_button_rect(layout, 0).expect("loopback rect");
        let record_rect = timeline_track_record_button_rect(layout, 0).expect("record rect");

        let scene = build_blank_timeline_render_scene(
            sample_layout(),
            WindowChromeButtonsState::default(),
            Some(&document),
            Some(&state),
            TimelineDocumentVisualState::default(),
        );

        assert!(scene.panels.iter().any(|panel| {
            panel.rect == playback_rect.to_win32_rect()
                && matches!(panel.effect, PanelEffect::PlaybackButton)
        }));
        assert!(scene.panels.iter().any(|panel| {
            panel.rect == loopback_rect.to_win32_rect()
                && matches!(panel.effect, PanelEffect::LoopbackButton)
        }));
        assert!(scene.panels.iter().any(|panel| {
            panel.rect == record_rect.to_win32_rect()
                && matches!(panel.effect, PanelEffect::RecordArmButton)
        }));
    }

    #[test]
    fn timeline_document_layout_reserves_a_clickable_add_track_region() {
        let layout = timeline_document_layout(sample_layout().terminal_panel_rect().inset(24));

        assert!(layout.add_track_rect.left() >= layout.track_list_rect.left());
        assert!(layout.add_track_rect.right() <= layout.track_list_rect.right());
        assert!(layout.track_text_rect.top() >= layout.add_track_rect.bottom());
        assert!(layout.viewport_controls_rect.left() >= layout.ruler_rect.left());
        assert!(layout.viewport_controls_rect.right() <= layout.ruler_rect.right());
        assert!(layout.scrollport_rect.right() <= layout.time_axis_rect.right());
        assert!(layout.horizontal_scrollbar_rect.top() >= layout.scrollport_rect.bottom());
        assert!(layout.vertical_scrollbar_rect.left() >= layout.scrollport_rect.right());
    }

    #[test]
    // timeline[verify viewport.pan-controls]
    // timeline[verify viewport.zoom-controls]
    fn blank_timeline_render_scene_draws_viewport_controls() {
        let document = TimelineDocument::blank();
        let layout = timeline_document_layout(sample_layout().terminal_panel_rect().inset(24));
        let scene = build_blank_timeline_render_scene(
            sample_layout(),
            WindowChromeButtonsState::default(),
            Some(&document),
            None,
            TimelineDocumentVisualState::default(),
        );

        for action in [
            SceneAction::PanTimelineLeft,
            SceneAction::PanTimelineRight,
            SceneAction::ZoomTimelineIn,
            SceneAction::ZoomTimelineOut,
        ] {
            let rect = timeline_viewport_control_rect(layout, action).expect("control rect");
            assert!(
                scene
                    .panels
                    .iter()
                    .any(|panel| panel.rect == rect.to_win32_rect())
            );
        }
    }

    // windowing[verify audio-picker.buttons.windows]
    // windowing[verify audio-picker.buttons.file]
    #[test]
    fn audio_picker_scene_specs_expose_audio_sources() {
        let specs = scene_button_specs(SceneWindowKind::AudioPicker);

        assert!(
            specs
                .iter()
                .any(|spec| spec.action == SceneAction::SelectWindowsBell)
        );
        assert!(
            specs
                .iter()
                .any(|spec| spec.action == SceneAction::SelectFileBell)
        );
    }

    // windowing[verify launcher.buttons.large-image-cards]
    // windowing[verify launcher.keyboard-navigation]
    #[test]
    // audio[verify gui.launcher-button]
    // timeline[verify launcher.button]
    fn launcher_scene_uses_card_panels_for_primary_actions() {
        let scene = build_scene_render_scene(
            sample_layout(),
            SceneWindowKind::Launcher,
            WindowChromeButtonsState::default(),
            DEFAULT_MAX_BUTTON_SIZE,
            &[],
            None,
        );
        let card_count = scene
            .panels
            .iter()
            .filter(|panel| matches!(panel.effect, PanelEffect::SceneButtonCard))
            .count();
        let expected_count = scene_button_specs(SceneWindowKind::Launcher).len();

        assert_eq!(card_count, expected_count);
        assert_eq!(scene.sprites.len(), expected_count);
    }

    #[test]
    // audio[verify gui.daemon-window]
    fn audio_daemon_scene_draws_status_dashboard() {
        let scene = build_audio_daemon_render_scene(
            sample_layout(),
            WindowChromeButtonsState::default(),
            &sample_audio_daemon_status(),
            AudioDaemonVisualState::default(),
        );

        assert!(scene.panels.len() >= 11);
        assert!(scene.glyphs.len() > 40);
    }

    #[test]
    // audio[verify transcription.model-selection]
    // audio[verify transcription.cuda-check]
    fn audio_daemon_controls_have_stable_hit_rects() {
        let body_rect = sample_layout().terminal_panel_rect().inset(24);
        let first = audio_daemon_model_button_rect(body_rect, 0, 6);
        let second_button = audio_daemon_model_button_rect(body_rect, 1, 6);
        let cuda = audio_daemon_cuda_check_button_rect(body_rect);

        assert!(first.width() > 0);
        assert!(second_button.left() > first.left());
        assert!(cuda.width() > 0);
        assert!(cuda.bottom() <= body_rect.bottom());
    }

    #[test]
    // audio[verify gui.daemon-diagnostics-tui]
    fn audio_daemon_diagnostics_scene_draws_full_tui() {
        let scene = build_audio_daemon_diagnostic_render_scene(
            sample_layout(),
            WindowChromeButtonsState::default(),
            &sample_audio_daemon_status(),
            None,
            8,
            16,
        );

        assert!(scene.panels.len() >= 4);
        assert!(scene.glyphs.len() > 80);
    }

    // windowing[verify demo-mode.window]
    // windowing[verify demo-mode.input-device-identifier-scramble]
    #[test]
    fn demo_mode_scene_draws_button_toggle_and_identifiers() {
        let scene = build_demo_mode_render_scene(
            sample_layout(),
            WindowChromeButtonsState::default(),
            true,
            DemoModeVisualState::default(),
        );

        assert!(scene.panels.len() >= 4);
        assert!(
            demo_mode_input_device_identifiers(true)
                .iter()
                .all(|identifier| identifier.starts_with("{0.0.1."))
        );
        assert_ne!(
            demo_mode_input_device_identifiers(false),
            demo_mode_input_device_identifiers(true)
        );
    }

    // windowing[verify virtual-cursor.os-cursor-sprite]
    #[test]
    fn launcher_scene_draws_virtual_cursor_pointer_when_present() {
        let scene = build_scene_render_scene(
            sample_layout(),
            SceneWindowKind::Launcher,
            WindowChromeButtonsState::default(),
            DEFAULT_MAX_BUTTON_SIZE,
            &[],
            Some(ClientPoint::new(120, 140)),
        );

        assert!(
            scene
                .sprites
                .iter()
                .any(|sprite| sprite.sprite == SpriteId::CursorHand)
        );
    }

    // windowing[verify cursor-gallery.stock-os-cursors]
    // windowing[verify cursor-gallery.hover-glow-color]
    #[test]
    fn cursor_gallery_scene_draws_stock_cursor_sprites() {
        let scene = build_cursor_gallery_render_scene(
            sample_layout(),
            WindowChromeButtonsState::default(),
            0,
            None,
            None,
        );

        assert_eq!(cursor_gallery_sprite_specs().len(), 7);
        assert!(scene.sprites.len() >= cursor_gallery_sprite_specs().len());
        assert!(scene.panels.len() > cursor_gallery_sprite_specs().len());
    }

    // windowing[verify cursor-gallery.virtual-navigation]
    #[test]
    fn cursor_gallery_cell_layouts_expose_hit_rects() {
        let cells = cursor_gallery_cell_layouts(sample_layout());

        assert_eq!(cells.len(), cursor_gallery_sprite_specs().len());
        assert!(cells.iter().all(|cell| cell.hit_rect().width() > 0));
        assert!(cells.iter().all(|cell| cell.hit_rect().height() > 0));
    }

    #[test]
    // windowing[verify diagnostics.launcher-tui]
    fn launcher_diagnostics_render_blocks_and_selected_color() {
        let area = RatatuiRect::new(0, 0, 82, 24);
        let mut buffer = Buffer::empty(area);

        render_launcher_diagnostic_buffer(&mut buffer, area, 1, Some(ClientPoint::new(42, 99)));

        assert_ne!(buffer.cell((0, 0)).map(|cell| cell.symbol()), Some(" "));
        assert!(
            buffer
                .content()
                .iter()
                .any(|cell| { cell.symbol().contains("C") && cell.bg == Color::Rgb(50, 66, 98) })
        );
    }

    #[test]
    // audio[verify gui.diagnostics-tui]
    fn audio_input_diagnostics_render_blocks_and_selected_color() {
        let devices = vec![
            sample_audio_input_device("endpoint-a", "Studio Mic"),
            sample_audio_input_device("endpoint-b", "Desk Mic"),
        ];
        let area = RatatuiRect::new(0, 0, 80, 24);
        let mut buffer = Buffer::empty(area);

        render_audio_input_device_diagnostic_buffer(&mut buffer, area, &devices, 1, false);

        assert_ne!(buffer.cell((0, 0)).map(|cell| cell.symbol()), Some(" "));
        assert!(
            buffer
                .content()
                .iter()
                .any(|cell| { cell.symbol().contains("D") && cell.bg == Color::Rgb(28, 76, 88) })
        );
    }

    #[test]
    // audio[verify gui.legacy-recording-dialog]
    fn audio_input_picker_render_shows_legacy_windows_gear_button() {
        let devices = vec![sample_audio_input_device("endpoint-a", "Studio Mic")];
        let scene = build_audio_input_device_picker_render_scene(
            sample_layout(),
            WindowChromeButtonsState::default(),
            &devices,
            0,
            false,
        );
        let body_rect = sample_layout().terminal_panel_rect().inset(22);
        let legacy_dialog_rect = audio_input_legacy_recording_dialog_button_rect(body_rect);

        assert!(
            scene
                .panels
                .iter()
                .any(|panel| panel.rect == legacy_dialog_rect.to_win32_rect()
                    && matches!(panel.effect, PanelEffect::GearButton))
        );
    }

    #[test]
    // audio[verify gui.selected-device-window]
    // audio[verify gui.arm-for-record]
    // audio[verify gui.playback-transport]
    // audio[verify gui.transcription-toggle]
    // audio[verify gui.mel-spectrogram-preview]
    // audio[verify gui.transcription-terminal-island]
    // audio[verify gui.legacy-recording-dialog]
    fn audio_input_device_detail_render_shows_device_and_arm_button() {
        let state = sample_audio_input_device_window();
        let scene = build_audio_input_device_detail_render_scene(
            sample_layout(),
            WindowChromeButtonsState::default(),
            Some(&state),
            AudioInputDeviceDetailVisualState::default(),
            false,
            None,
        );
        let body_rect = sample_layout().terminal_panel_rect().inset(24);
        let detail_layout = audio_input_device_detail_layout(body_rect);

        assert!(
            scene
                .sprites
                .iter()
                .any(|sprite| sprite.rect == detail_layout.icon_rect.to_win32_rect())
        );
        assert!(
            scene
                .panels
                .iter()
                .any(|panel| panel.rect == detail_layout.arm_button_rect.to_win32_rect())
        );
        assert!(scene.panels.iter().any(|panel| panel.rect
            == detail_layout.legacy_recording_button_rect.to_win32_rect()
            && matches!(panel.effect, PanelEffect::GearButton)));
        assert!(scene.panels.iter().any(|panel| panel.rect
            == detail_layout.loopback_button_rect.to_win32_rect()
            && matches!(panel.effect, PanelEffect::LoopbackButton)));
        assert!(scene.panels.iter().any(|panel| panel.rect
            == detail_layout.transcription_button_rect.to_win32_rect()
            && matches!(panel.effect, PanelEffect::TranscriptionToggle)));
        assert!(scene.panels.iter().any(|panel| panel.rect
            == detail_layout.play_pause_button_rect.to_win32_rect()
            && matches!(panel.effect, PanelEffect::PlaybackButton)));
        assert!(scene.panels.iter().any(|panel| panel.rect
            == detail_layout.mel_spectrogram_rect.to_win32_rect()
            && matches!(panel.effect, PanelEffect::TerminalFill)));
        assert!(scene.panels.iter().any(|panel| panel.rect
            == detail_layout.transcript_terminal_rect.to_win32_rect()
            && matches!(panel.effect, PanelEffect::TerminalPanel)));
        assert!(!scene.glyphs.is_empty());
    }

    #[test]
    // windowing[verify scene.pretty-text.selection]
    fn audio_input_device_detail_pretty_metadata_renders_selection() {
        let state = sample_audio_input_device_window();
        let selection = TerminalSelection::new(
            TerminalCellPoint::new(0, 1),
            TerminalCellPoint::new(10, 1),
            crate::app::windows_terminal::TerminalSelectionMode::Linear,
        );
        let scene = build_audio_input_device_detail_render_scene(
            sample_layout(),
            WindowChromeButtonsState::default(),
            Some(&state),
            AudioInputDeviceDetailVisualState::default(),
            false,
            Some(selection),
        );

        assert!(scene.panels.iter().any(|panel| {
            panel.effect == PanelEffect::TerminalFill && panel.color == [0.42, 0.67, 0.98, 1.0]
        }));
    }

    #[test]
    // windowing[verify demo-mode.input-device-identifier-scramble]
    // windowing[verify demo-mode.live-audio-device-scramble]
    fn input_device_identifier_display_text_scrambles_without_mutating_shape() {
        let scrambled = input_device_identifier_display_text("endpoint-a", true);

        assert_ne!(scrambled, "endpoint-a");
        assert!(scrambled.starts_with("{0.0.1."));
        assert!(!scrambled.starts_with("SWD\\MMDEVAPI\\"));
        assert_eq!(
            input_device_identifier_display_text("endpoint-a", false),
            "endpoint-a"
        );
    }

    // windowing[verify garden-band.shared]
    #[test]
    fn scene_shell_uses_shared_garden_frame_surface() {
        let layout = sample_layout();
        let scene = build_scene_shell(
            layout,
            SceneWindowKind::Launcher,
            WindowChromeButtonsState::default(),
        );
        let garden_panel = scene
            .panels
            .iter()
            .find(|panel| matches!(panel.effect, PanelEffect::GardenFrame))
            .expect("scene shell should include a garden frame panel");

        assert_eq!(garden_panel.rect, layout.full_client_rect().to_win32_rect());
        assert_eq!(garden_panel.data, window_garden_shader_data(layout));
    }

    #[test]
    fn scene_shell_limits_blue_background_to_content_frame() {
        let layout = sample_layout();
        let scene = build_scene_shell(
            layout,
            SceneWindowKind::Launcher,
            WindowChromeButtonsState::default(),
        );
        let blue_panel = scene
            .panels
            .iter()
            .find(|panel| matches!(panel.effect, PanelEffect::BlueBackground))
            .expect("scene shell should include a blue background panel");

        assert_eq!(blue_panel.rect, layout.content_frame_rect().to_win32_rect());
    }

    // windowing[verify diagnostics.scene-window.replaces-body]
    #[test]
    fn scene_diagnostic_render_scene_replaces_cards_with_text_grid_body() {
        let scene = build_scene_diagnostic_render_scene(
            sample_layout(),
            SceneWindowKind::Launcher,
            WindowChromeButtonsState::default(),
            "launcher diagnostic text",
            None,
            8,
            16,
        );
        let card_count = scene
            .panels
            .iter()
            .filter(|panel| matches!(panel.effect, PanelEffect::SceneButtonCard))
            .count();

        assert_eq!(card_count, 0);
        assert!(!scene.glyphs.is_empty());
    }
}

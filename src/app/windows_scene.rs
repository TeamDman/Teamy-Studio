use std::time::{Duration, Instant};

use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Direction, Layout, Rect as RatatuiRect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols;
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Axis, Block, Borders, Chart, Dataset, GraphType, List, ListItem, Paragraph, Widget, Wrap,
};
use windows::Win32::Foundation::RECT;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SceneWindowKind {
    Launcher,
    AudioPicker,
    AudioInputDevicePicker,
    AudioInputDeviceDetails,
}

impl SceneWindowKind {
    #[must_use]
    pub const fn title(self) -> &'static str {
        match self {
            Self::Launcher => "Teamy Studio",
            Self::AudioPicker => "Audio Sources",
            Self::AudioInputDevicePicker => "Audio Devices",
            Self::AudioInputDeviceDetails => "Microphone",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SceneAction {
    OpenTerminal,
    OpenCursorInfo,
    OpenStorage,
    OpenAudioPicker,
    OpenAudioInputDevices,
    SelectWindowsBell,
    SelectFileBell,
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
    pub loopback_button_rect: ClientRect,
    pub arm_status_rect: ClientRect,
    pub legacy_recording_button_rect: ClientRect,
    pub buffer_section_rect: ClientRect,
    pub waveform_rect: ClientRect,
    pub timeline_label_rect: ClientRect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioInputTimelineHeadGrabberLayout {
    pub kind: AudioInputTimelineHeadKind,
    pub rect: ClientRect,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AudioInputDeviceDetailVisualState {
    pub loopback_hovered: bool,
    pub loopback_pressed: bool,
    pub hovered_head: Option<AudioInputTimelineHeadKind>,
    pub grabbed_head: Option<AudioInputTimelineHeadKind>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneButtonSpec {
    pub action: SceneAction,
    pub label: &'static str,
    pub tooltip: &'static str,
    pub sprite: SpriteId,
    pub color: [f32; 4],
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

    scene
}

#[must_use]
// windowing[impl launcher.buttons.terminal]
// windowing[impl launcher.buttons.storage-placeholder]
// windowing[impl launcher.buttons.audio-picker]
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
                action: SceneAction::OpenStorage,
                label: "Storage",
                tooltip: "Storage is not implemented yet",
                sprite: SpriteId::Storage,
                color: [0.30, 0.21, 0.14, 1.0],
            },
            SceneButtonSpec {
                action: SceneAction::OpenAudioPicker,
                label: "Audio",
                tooltip: "Choose audio source",
                sprite: SpriteId::Audio,
                color: [0.25, 0.21, 0.11, 1.0],
            },
            SceneButtonSpec {
                // audio[impl gui.launcher-button]
                action: SceneAction::OpenAudioInputDevices,
                label: "Audio Devices",
                tooltip: "Choose microphone input device",
                sprite: SpriteId::Audio,
                color: [0.13, 0.25, 0.32, 1.0],
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
        SceneWindowKind::AudioInputDevicePicker | SceneWindowKind::AudioInputDeviceDetails => &[],
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
            device.name, default_marker, sample_rate, device.id
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
        push_text_block(
            &mut scene,
            empty_rect.inset(18).to_win32_rect(),
            "No microphone is selected.",
            10,
            18,
            [0.92, 0.93, 0.95, 1.0],
        );
        return scene;
    };

    let detail_layout = audio_input_device_detail_layout(body_rect);
    let device = &device_state.device;
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

    let default_marker = if device.is_default { " [default]" } else { "" };
    let sample_rate = device.sample_rate_hz.map_or_else(
        || "sample rate: unknown".to_owned(),
        |rate| format!("sample rate: {rate} Hz"),
    );
    let details = format!(
        "{}{}\n{}\nstate: {}\n{}",
        device.name, default_marker, sample_rate, device.state, device.id
    );
    push_text_block(
        &mut scene,
        detail_layout.info_rect.to_win32_rect(),
        &details,
        10,
        18,
        [0.95, 0.96, 0.98, 1.0],
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

#[must_use]
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
    let arm_status_rect = ClientRect::new(
        arm_button_rect.right() + 18,
        arm_button_rect.top() + 14,
        loopback_button_rect.left() - 24,
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
        buffer_section_rect.bottom() - 16,
    );

    AudioInputDeviceDetailLayout {
        icon_rect,
        info_rect,
        arm_button_rect,
        loopback_button_rect,
        arm_status_rect,
        legacy_recording_button_rect,
        buffer_section_rect,
        waveform_rect,
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
        push_waveform_bars(scene, waveform_rect, &samples);
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
fn push_waveform_bars(scene: &mut RenderScene, waveform_rect: ClientRect, samples: &[f32]) {
    let width = waveform_rect.width().max(1);
    let mid_y = waveform_rect.top() + waveform_rect.height() / 2;
    let half_height = (waveform_rect.height() / 2 - 6).max(2);
    let bars = width;
    let peak = samples
        .iter()
        .map(|sample| sample.abs())
        .fold(0.0_f32, f32::max)
        .max(0.015);
    for bar_index in 0..bars {
        let start = (usize::try_from(bar_index).unwrap_or_default() * samples.len())
            / usize::try_from(bars).unwrap_or(1);
        let end = ((usize::try_from(bar_index + 1).unwrap_or_default() * samples.len())
            / usize::try_from(bars).unwrap_or(1))
        .max(start + 1)
        .min(samples.len());
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
// audio[impl gui.diagnostics-tui]
pub fn build_audio_input_device_diagnostic_render_scene(
    layout: TerminalLayout,
    window_chrome_buttons_state: WindowChromeButtonsState,
    devices: &[AudioInputDeviceSummary],
    selected_index: usize,
    selection: Option<TerminalSelection>,
    cell_width: i32,
    cell_height: i32,
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
    );
    scene.panels.extend(diagnostic_scene.panels);
    scene.glyphs.extend(diagnostic_scene.glyphs);
    scene.sprites.extend(diagnostic_scene.sprites);
    scene.overlay_panels.extend(diagnostic_scene.overlay_panels);
    scene
}

fn build_audio_input_device_detail_diagnostic_body_scene(
    body_rect: ClientRect,
    device_state: Option<&AudioInputDeviceWindowState>,
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
    render_audio_input_device_detail_diagnostic_buffer(&mut buffer, area, device_state);
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
        Line::from(device_state.device.id.clone()),
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
) -> RenderScene {
    let columns = u16::try_from((body_rect.width() / cell_width.max(1)).max(0)).unwrap_or_default();
    let rows = u16::try_from((body_rect.height() / cell_height.max(1)).max(0)).unwrap_or_default();
    if columns == 0 || rows == 0 {
        return empty_render_scene();
    }

    let area = RatatuiRect::new(0, 0, columns, rows);
    let mut buffer = Buffer::empty(area);
    render_audio_input_device_diagnostic_buffer(&mut buffer, area, devices, selected_index);
    ratatui_buffer_to_scene(body_rect, &buffer, selection, cell_width, cell_height)
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
                audio_input_device_diagnostic_item(index, selected_index, device)
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
            Span::styled(device.id.clone(), base_style.fg(Color::DarkGray)),
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
    // windowing[verify launcher.buttons.audio-picker]
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
                .any(|spec| spec.action == SceneAction::OpenStorage)
        );
        assert!(
            specs
                .iter()
                .any(|spec| spec.action == SceneAction::OpenAudioPicker)
        );
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
    #[test]
    // audio[verify gui.launcher-button]
    fn launcher_scene_uses_card_panels_for_primary_actions() {
        let scene = build_scene_render_scene(
            sample_layout(),
            SceneWindowKind::Launcher,
            WindowChromeButtonsState::default(),
            DEFAULT_MAX_BUTTON_SIZE,
            &[],
        );
        let card_count = scene
            .panels
            .iter()
            .filter(|panel| matches!(panel.effect, PanelEffect::SceneButtonCard))
            .count();

        assert_eq!(card_count, 5);
        assert_eq!(scene.sprites.len(), 5);
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

        render_audio_input_device_diagnostic_buffer(&mut buffer, area, &devices, 1);

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
    // audio[verify gui.legacy-recording-dialog]
    fn audio_input_device_detail_render_shows_device_and_arm_button() {
        let state = sample_audio_input_device_window();
        let scene = build_audio_input_device_detail_render_scene(
            sample_layout(),
            WindowChromeButtonsState::default(),
            Some(&state),
            AudioInputDeviceDetailVisualState::default(),
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
        assert!(!scene.glyphs.is_empty());
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

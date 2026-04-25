use std::time::{Duration, Instant};

use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Direction, Layout, Rect as RatatuiRect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Widget, Wrap};
use windows::Win32::Foundation::RECT;

use super::cell_grid;
use super::spatial::{ClientPoint, ClientRect, TerminalCellPoint};
use super::windows_audio_input::{AudioInputDeviceSummary, AudioInputDeviceWindowState};
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
    pub arm_status_rect: ClientRect,
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

    push_panel_with_data(
        &mut scene,
        detail_layout.arm_button_rect.to_win32_rect(),
        if device_state.armed_for_record {
            [0.16, 0.36, 0.25, 1.0]
        } else {
            [0.18, 0.20, 0.22, 1.0]
        },
        PanelEffect::SceneButtonCard,
        [
            0.0,
            if device_state.armed_for_record {
                1.0
            } else {
                0.0
            },
            0.0,
            0.0,
        ],
    );
    push_sprite(
        &mut scene,
        detail_layout.arm_button_rect.inset(14).to_win32_rect(),
        if device_state.armed_for_record {
            [0.72, 1.0, 0.80, 1.0]
        } else {
            [0.68, 0.70, 0.72, 1.0]
        },
        SpriteId::Audio,
    );
    push_text_block(
        &mut scene,
        detail_layout.arm_status_rect.to_win32_rect(),
        if device_state.armed_for_record {
            "Armed for future recording"
        } else {
            "Recording arm disabled"
        },
        10,
        18,
        if device_state.armed_for_record {
            [0.76, 0.96, 0.82, 1.0]
        } else {
            [0.82, 0.84, 0.86, 1.0]
        },
    );

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
    let arm_status_rect = ClientRect::new(
        arm_button_rect.right() + 18,
        arm_button_rect.top() + 14,
        body_rect.right() - 28,
        arm_button_rect.bottom() - 10,
    );
    let info_rect = ClientRect::new(
        icon_rect.right() + 32,
        icon_rect.top() + 6,
        body_rect.right() - 28,
        icon_rect.bottom() + 24,
    );

    AudioInputDeviceDetailLayout {
        icon_rect,
        info_rect,
        arm_button_rect,
        arm_status_rect,
    }
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
            Constraint::Length(3),
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

    let footer = Paragraph::new(Line::from(vec![
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
            "Alt+X",
            Style::new()
                .fg(Color::LightCyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" pretty view  "),
        Span::styled(
            "Esc",
            Style::new()
                .fg(Color::LightCyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" close"),
    ]))
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
    // audio[verify gui.selected-device-window]
    // audio[verify gui.arm-for-record]
    fn audio_input_device_detail_render_shows_device_and_arm_button() {
        let state = sample_audio_input_device_window();
        let scene = build_audio_input_device_detail_render_scene(
            sample_layout(),
            WindowChromeButtonsState::default(),
            Some(&state),
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

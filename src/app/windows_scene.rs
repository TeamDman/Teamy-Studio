use std::time::{Duration, Instant};

use super::cell_grid;
use super::spatial::{ClientPoint, ClientRect};
use super::windows_d3d12_renderer::{
    ButtonVisualState, PanelEffect, RenderScene, SpriteId, push_centered_text, push_panel,
    push_panel_with_data, push_sprite,
};
use super::windows_terminal::TerminalLayout;

const BUTTON_SIZE: i32 = 300;
const BUTTON_GAP: i32 = 48;
const BUTTON_LABEL_GAP: i32 = 18;
const BUTTON_LABEL_HEIGHT: i32 = 42;
const BUTTON_PROXIMITY_RADIUS_PX: f32 = 96.0;
const CLICK_DECAY_DURATION: Duration = Duration::from_millis(220);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SceneWindowKind {
    Launcher,
    AudioPicker,
}

impl SceneWindowKind {
    #[must_use]
    pub const fn title(self) -> &'static str {
        match self {
            Self::Launcher => "Teamy Studio",
            Self::AudioPicker => "Audio Sources",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SceneAction {
    OpenTerminal,
    OpenStorage,
    OpenAudioPicker,
    SelectWindowsBell,
    SelectFileBell,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneButtonSpec {
    pub action: SceneAction,
    pub label: &'static str,
    pub sprite: SpriteId,
    pub color: [f32; 4],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SceneButtonLayout {
    pub card_rect: ClientRect,
    pub sprite_rect: ClientRect,
    pub label_rect: ClientRect,
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
pub fn build_scene_render_scene(
    layout: TerminalLayout,
    scene_kind: SceneWindowKind,
    diagnostics_button_state: ButtonVisualState,
    button_states: &[(SceneAction, ButtonVisualState)],
) -> RenderScene {
    let mut scene = build_scene_shell(layout, scene_kind, diagnostics_button_state);

    let specs = scene_button_specs(scene_kind);
    let button_layouts = layout_scene_buttons(layout.terminal_panel_rect(), specs.len());
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
pub fn scene_button_specs(scene_kind: SceneWindowKind) -> &'static [SceneButtonSpec] {
    match scene_kind {
        SceneWindowKind::Launcher => &[
            SceneButtonSpec {
                action: SceneAction::OpenTerminal,
                label: "Terminal",
                sprite: SpriteId::Terminal,
                color: [0.18, 0.25, 0.35, 1.0],
            },
            SceneButtonSpec {
                action: SceneAction::OpenStorage,
                label: "Storage",
                sprite: SpriteId::Storage,
                color: [0.30, 0.21, 0.14, 1.0],
            },
            SceneButtonSpec {
                action: SceneAction::OpenAudioPicker,
                label: "Audio",
                sprite: SpriteId::Audio,
                color: [0.25, 0.21, 0.11, 1.0],
            },
        ],
        SceneWindowKind::AudioPicker => &[
            SceneButtonSpec {
                action: SceneAction::SelectWindowsBell,
                label: "Windows",
                sprite: SpriteId::WindowsAudio,
                color: [0.14, 0.24, 0.35, 1.0],
            },
            SceneButtonSpec {
                action: SceneAction::SelectFileBell,
                label: "Pick File",
                sprite: SpriteId::FileAudio,
                color: [0.23, 0.19, 0.30, 1.0],
            },
        ],
    }
}

#[must_use]
pub fn build_scene_diagnostic_render_scene(
    layout: TerminalLayout,
    scene_kind: SceneWindowKind,
    diagnostics_button_state: ButtonVisualState,
    diagnostic_text: &str,
    selection: Option<super::windows_terminal::TerminalSelection>,
    cell_width: i32,
    cell_height: i32,
) -> RenderScene {
    let mut scene = build_scene_shell(layout, scene_kind, diagnostics_button_state);
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
pub fn layout_scene_buttons(body_rect: ClientRect, count: usize) -> Vec<SceneButtonLayout> {
    if count == 0 {
        return Vec::new();
    }

    let max_columns = (body_rect.width() + BUTTON_GAP) / (BUTTON_SIZE + BUTTON_GAP);
    let columns = usize::try_from(max_columns.max(1))
        .unwrap_or(1)
        .min(count)
        .max(1);
    let rows = count.div_ceil(columns);
    let total_width = i32::try_from(columns).unwrap_or(1) * BUTTON_SIZE
        + (i32::try_from(columns).unwrap_or(1) - 1).max(0) * BUTTON_GAP;
    let row_height = BUTTON_SIZE + BUTTON_LABEL_GAP + BUTTON_LABEL_HEIGHT;
    let total_height = i32::try_from(rows).unwrap_or(1) * row_height
        + (i32::try_from(rows).unwrap_or(1) - 1).max(0) * BUTTON_GAP;
    let start_x = body_rect.left() + ((body_rect.width() - total_width).max(0) / 2);
    let start_y = body_rect.top() + ((body_rect.height() - total_height).max(0) / 2);

    let mut layouts = Vec::with_capacity(count);
    for index in 0..count {
        let column = i32::try_from(index % columns).unwrap_or_default();
        let row = i32::try_from(index / columns).unwrap_or_default();
        let left = start_x + column * (BUTTON_SIZE + BUTTON_GAP);
        let top = start_y + row * (row_height + BUTTON_GAP);
        let card_rect = ClientRect::new(left, top, left + BUTTON_SIZE, top + BUTTON_SIZE);
        layouts.push(SceneButtonLayout {
            sprite_rect: card_rect.inset(20),
            label_rect: ClientRect::new(
                card_rect.left(),
                card_rect.bottom() + BUTTON_LABEL_GAP,
                card_rect.right(),
                card_rect.bottom() + BUTTON_LABEL_GAP + BUTTON_LABEL_HEIGHT,
            ),
            card_rect,
        });
    }

    layouts
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
    diagnostics_button_state: ButtonVisualState,
) -> RenderScene {
    let mut scene = RenderScene {
        panels: Vec::new(),
        glyphs: Vec::new(),
        sprites: Vec::new(),
        overlay_panels: Vec::new(),
    };

    push_panel(
        &mut scene,
        ClientRect::new(0, 0, layout.client_width, layout.client_height).to_win32_rect(),
        [0.11, 0.44, 0.94, 0.5],
        PanelEffect::BlueBackground,
    );
    push_panel(
        &mut scene,
        layout.title_bar_rect().to_win32_rect(),
        [0.42, 0.18, 0.60, 1.0],
        PanelEffect::TitleBar,
    );
    push_panel(
        &mut scene,
        layout.terminal_panel_rect().to_win32_rect(),
        [0.09, 0.10, 0.12, 0.96],
        PanelEffect::SceneBody,
    );
    push_panel_with_data(
        &mut scene,
        layout.diagnostics_button_rect().to_win32_rect(),
        if diagnostics_button_state.active {
            [0.23, 0.48, 0.69, 1.0]
        } else {
            [0.12, 0.13, 0.17, 1.0]
        },
        PanelEffect::DiagnosticsButton,
        diagnostics_button_state.shader_data(),
    );
    push_centered_text(
        &mut scene,
        layout.title_text_rect().to_win32_rect(),
        scene_kind.title(),
        [0.95, 0.95, 0.98, 1.0],
    );

    scene
}

fn proximity_to_rect(rect: ClientRect, pointer: ClientPoint) -> f32 {
    let point = match pointer.to_win32_point() {
        Ok(point) => point,
        Err(_) => return 0.0,
    };
    let dx = if point.x < rect.left() {
        (rect.left() - point.x) as f32
    } else if point.x > rect.right() {
        (point.x - rect.right()) as f32
    } else {
        0.0
    };
    let dy = if point.y < rect.top() {
        (rect.top() - point.y) as f32
    } else if point.y > rect.bottom() {
        (point.y - rect.bottom()) as f32
    } else {
        0.0
    };
    let distance = (dx * dx + dy * dy).sqrt();
    (1.0 - (distance / BUTTON_PROXIMITY_RADIUS_PX)).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scene_button_layouts_center_buttons_in_the_body() {
        let layouts = layout_scene_buttons(ClientRect::new(0, 0, 1100, 720), 3);

        assert_eq!(layouts.len(), 3);
        assert!(layouts[0].card_rect.left() < layouts[1].card_rect.left());
        assert_eq!(layouts[0].card_rect.top(), layouts[1].card_rect.top());
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
}

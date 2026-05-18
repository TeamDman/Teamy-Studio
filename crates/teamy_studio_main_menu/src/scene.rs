use windows::Win32::Foundation::RECT;

use crate::{FeatureValidationState, MainMenuLogicalButton, MainMenuSnapshot};
use teamy_studio_shell::{
    ButtonVisualState, PanelEffect, RenderScene, ShellSceneLayout, SpriteId,
    WindowChromeButtonsState, preferred_background_color, preferred_title_bar_color,
    push_centered_text, push_panel, push_panel_with_data, push_sprite, push_title_text,
    push_window_chrome_buttons, push_window_garden_frame,
};

const DEFAULT_MAX_BUTTON_SIZE: i32 = 168;
const MIN_BUTTON_GAP: i32 = 10;
const MAX_BUTTON_GAP: i32 = 26;
const MIN_BUTTON_LABEL_GAP: i32 = 10;
const MAX_BUTTON_LABEL_GAP: i32 = 16;
const MIN_BUTTON_LABEL_HEIGHT: i32 = 24;
const MAX_BUTTON_LABEL_HEIGHT: i32 = 34;
const MIN_BUTTON_SPRITE_INSET: i32 = 18;
const MAX_BUTTON_SPRITE_INSET: i32 = 26;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MainMenuButtonCardLayout {
    pub card_rect: RECT,
    pub sprite_rect: RECT,
    pub label_rect: RECT,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MainMenuSceneButtonState {
    pub visual_state: ButtonVisualState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ButtonGridMetrics {
    columns: usize,
    button_size: i32,
    button_gap: i32,
    label_gap: i32,
    label_height: i32,
    sprite_inset: i32,
}

#[must_use]
pub fn build_main_menu_scene(
    snapshot: &MainMenuSnapshot,
    shell_layout: ShellSceneLayout,
    window_chrome_buttons_state: WindowChromeButtonsState,
    button_states: &[(crate::MainMenuLogicalButtonId, MainMenuSceneButtonState)],
) -> RenderScene {
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
        preferred_title_bar_color(window_chrome_buttons_state.focused),
        PanelEffect::TitleBar,
    );
    push_panel(
        &mut scene,
        shell_layout.body_rect,
        [0.08, 0.09, 0.11, 0.985],
        PanelEffect::SceneBody,
    );
    push_window_chrome_buttons(&mut scene, shell_layout, window_chrome_buttons_state);
    push_title_text(
        &mut scene,
        shell_layout.title_text_rect,
        "Teamy Studio",
        [0.95, 0.95, 0.98, 1.0],
    );

    let layouts = layout_main_menu_button_cards(shell_layout.body_rect, snapshot.buttons().len());
    for (button, layout) in snapshot.buttons().iter().zip(layouts.iter()) {
        let visual_state = button_states
            .iter()
            .find_map(|(button_id, state)| {
                (*button_id == button.logical_button_id).then_some(*state)
            })
            .unwrap_or_default()
            .visual_state;
        let card_color = button_card_color(button, visual_state);

        push_panel_with_data(
            &mut scene,
            layout.card_rect,
            card_color,
            PanelEffect::SceneButtonCard,
            visual_state.shader_data(),
        );
        push_sprite(
            &mut scene,
            layout.sprite_rect,
            [1.0, 1.0, 1.0, 1.0],
            button_sprite(button),
        );
        push_centered_text(
            &mut scene,
            layout.label_rect,
            button.title,
            if button.validation_state == FeatureValidationState::Validated {
                [0.97, 0.97, 0.99, 1.0]
            } else {
                [0.85, 0.87, 0.91, 1.0]
            },
        );
    }

    scene
}

fn button_card_color(button: &MainMenuLogicalButton, visual_state: ButtonVisualState) -> [f32; 4] {
    let mut color = button_base_color(button);
    if button.validation_state != FeatureValidationState::Validated {
        color = [color[0] * 0.82, color[1] * 0.82, color[2] * 0.82, 0.96];
    }
    if visual_state.active {
        color = [
            (color[0] + 0.08).min(1.0),
            (color[1] + 0.08).min(1.0),
            (color[2] + 0.08).min(1.0),
            1.0,
        ];
    }
    color
}

fn button_base_color(button: &MainMenuLogicalButton) -> [f32; 4] {
    match button.title {
        "Terminal" => [0.18, 0.25, 0.35, 1.0],
        "Cursor Info" => [0.16, 0.30, 0.24, 1.0],
        "Cursor Gallery" => [0.20, 0.18, 0.32, 1.0],
        "Cursor Latency Playground" => [0.28, 0.17, 0.18, 1.0],
        "Text Rendering Playground" => [0.17, 0.21, 0.34, 1.0],
        "Demo Mode" => [0.15, 0.25, 0.28, 1.0],
        "Storage" => [0.30, 0.21, 0.14, 1.0],
        "Environment Variables" => [0.18, 0.29, 0.22, 1.0],
        "Application Windows" => [0.22, 0.24, 0.34, 1.0],
        "Audio" => [0.25, 0.21, 0.11, 1.0],
        "Audio Daemon" => [0.24, 0.18, 0.30, 1.0],
        "Jobs" => [0.18, 0.27, 0.28, 1.0],
        "Logs" => [0.20, 0.22, 0.30, 1.0],
        "Audio Devices" => [0.13, 0.25, 0.32, 1.0],
        "Timeline" => [0.15, 0.23, 0.27, 1.0],
        "Timeline Playground" => [0.18, 0.24, 0.18, 1.0],
        _ => [0.20, 0.22, 0.30, 1.0],
    }
}

fn button_sprite(button: &MainMenuLogicalButton) -> SpriteId {
    match button.title {
        "Storage" | "Environment Variables" => SpriteId::Storage,
        "Audio" | "Audio Devices" | "Audio Daemon" => SpriteId::Audio,
        "Cursor Gallery" | "Cursor Latency Playground" | "Cursor Info" => SpriteId::CursorArrow,
        _ => SpriteId::Terminal,
    }
}

#[must_use]
pub fn layout_main_menu_button_cards(
    body_rect: RECT,
    count: usize,
) -> Vec<MainMenuButtonCardLayout> {
    if count == 0 {
        return Vec::new();
    }

    let metrics = button_grid_metrics(body_rect, count, DEFAULT_MAX_BUTTON_SIZE);
    let columns = metrics.columns;
    let rows = count.div_ceil(columns);
    let columns_i32 = i32::try_from(columns).unwrap_or(1).max(1);
    let rows_i32 = i32::try_from(rows).unwrap_or(1).max(1);
    let total_width =
        columns_i32 * metrics.button_size + (columns_i32 - 1).max(0) * metrics.button_gap;
    let row_height = metrics.button_size + metrics.label_gap + metrics.label_height;
    let total_height = rows_i32 * row_height + (rows_i32 - 1).max(0) * metrics.button_gap;
    let start_x = body_rect.left + (((body_rect.right - body_rect.left) - total_width).max(0) / 2);
    let start_y = body_rect.top + (((body_rect.bottom - body_rect.top) - total_height).max(0) / 2);

    let mut layouts = Vec::with_capacity(count);
    for index in 0..count {
        let column = i32::try_from(index % columns).unwrap_or_default();
        let row = i32::try_from(index / columns).unwrap_or_default();
        let left = start_x + column * (metrics.button_size + metrics.button_gap);
        let top = start_y + row * (row_height + metrics.button_gap);
        let card_rect = RECT {
            left,
            top,
            right: left + metrics.button_size,
            bottom: top + metrics.button_size,
        };
        layouts.push(MainMenuButtonCardLayout {
            sprite_rect: inset_rect(card_rect, metrics.sprite_inset),
            label_rect: RECT {
                left: card_rect.left,
                top: card_rect.bottom + metrics.label_gap,
                right: card_rect.right,
                bottom: card_rect.bottom + metrics.label_gap + metrics.label_height,
            },
            card_rect,
        });
    }
    layouts
}

fn button_grid_metrics(body_rect: RECT, count: usize, max_button_size: i32) -> ButtonGridMetrics {
    let mut best = button_grid_candidate(body_rect, count, 1, max_button_size);
    for columns in 2..=count {
        let candidate = button_grid_candidate(body_rect, count, columns, max_button_size);
        if candidate.button_size > best.button_size
            || (candidate.button_size == best.button_size && candidate.columns > best.columns)
        {
            best = candidate;
        }
    }
    best
}

fn button_grid_candidate(
    body_rect: RECT,
    count: usize,
    columns: usize,
    max_button_size: i32,
) -> ButtonGridMetrics {
    let width = body_rect.right - body_rect.left;
    let height = body_rect.bottom - body_rect.top;
    let rows = count.div_ceil(columns);
    let columns_i32 = i32::try_from(columns).unwrap_or(1).max(1);
    let rows_i32 = i32::try_from(rows).unwrap_or(1).max(1);
    let button_gap = (width.min(height) / 28).clamp(MIN_BUTTON_GAP, MAX_BUTTON_GAP);
    let width_budget = width - ((columns_i32 - 1).max(0) * button_gap);
    let provisional_button_size = (width_budget / columns_i32).clamp(1, max_button_size);
    let label_gap =
        (provisional_button_size / 18).clamp(MIN_BUTTON_LABEL_GAP, MAX_BUTTON_LABEL_GAP);
    let label_height =
        (provisional_button_size / 7).clamp(MIN_BUTTON_LABEL_HEIGHT, MAX_BUTTON_LABEL_HEIGHT);
    let height_budget =
        height - ((rows_i32 - 1).max(0) * button_gap) - (rows_i32 * (label_gap + label_height));
    let button_size = (width_budget / columns_i32)
        .min(height_budget / rows_i32)
        .clamp(1, max_button_size);
    ButtonGridMetrics {
        columns,
        button_size,
        button_gap,
        label_gap,
        label_height,
        sprite_inset: (button_size / 12).clamp(MIN_BUTTON_SPRITE_INSET, MAX_BUTTON_SPRITE_INSET),
    }
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
    use crate::{
        FeatureValidationState, MainMenuButtonClassId, MainMenuButtonClassRegistration,
        MainMenuSnapshot,
    };
    use teamy_studio_shell::{PanelEffect, ShellSceneLayout, WindowChromeButtonsState};

    use super::{build_main_menu_scene, layout_main_menu_button_cards};

    static TERMINAL: MainMenuButtonClassRegistration = MainMenuButtonClassRegistration {
        class_id: MainMenuButtonClassId::from_bytes([1; 16]),
        title: "Terminal",
        tooltip: "Open terminal",
        ordinal: 10,
    };

    static CURSOR_GALLERY: MainMenuButtonClassRegistration = MainMenuButtonClassRegistration {
        class_id: MainMenuButtonClassId::from_bytes([2; 16]),
        title: "Cursor Gallery",
        tooltip: "Inspect OS cursor sprites",
        ordinal: 30,
    };

    #[test]
    fn scene_builder_emits_shell_chrome_and_button_cards() {
        let mut snapshot = MainMenuSnapshot::from_registrations(&[&TERMINAL, &CURSOR_GALLERY]);
        snapshot.set_validation_state_for_class_id(
            CURSOR_GALLERY.class_id,
            FeatureValidationState::Validated,
        );
        let scene = build_main_menu_scene(
            &snapshot,
            ShellSceneLayout::for_main_menu(1280, 820),
            WindowChromeButtonsState {
                focused: true,
                ..Default::default()
            },
            &[],
        );

        assert!(
            scene
                .panels
                .iter()
                .any(|panel| panel.effect == PanelEffect::GardenFrame)
        );
        assert!(
            scene
                .panels
                .iter()
                .any(|panel| panel.effect == PanelEffect::TitleBar)
        );
        assert_eq!(
            scene
                .panels
                .iter()
                .filter(|panel| panel.effect == PanelEffect::SceneButtonCard)
                .count(),
            2
        );
        assert!(scene.glyphs.iter().any(|glyph| glyph.character == 'T'));
    }

    #[test]
    fn button_layouts_expand_into_grid() {
        let layouts =
            layout_main_menu_button_cards(ShellSceneLayout::for_main_menu(1280, 820).body_rect, 16);
        assert_eq!(layouts.len(), 16);
        assert_eq!(layouts[0].card_rect.top, layouts[1].card_rect.top);
        assert!(layouts[0].card_rect.left < layouts[1].card_rect.left);
    }
}

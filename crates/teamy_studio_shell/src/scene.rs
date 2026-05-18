use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Dwm::DwmGetColorizationColor;
use windows::core::BOOL;

const MAX_PANEL_COUNT: usize = 8_192;
const MAX_GLYPH_COUNT: usize = 8_192;
pub const USER_DEFAULT_SCREEN_DPI: u32 = 96;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ButtonVisualState {
    pub hover_near: f32,
    pub hovered: bool,
    pub pressed: bool,
    pub click_decay: f32,
    pub active: bool,
}

impl ButtonVisualState {
    #[must_use]
    pub fn shader_data(self) -> [f32; 4] {
        [
            self.hover_near.clamp(0.0, 1.0),
            if self.hovered { 1.0 } else { 0.0 },
            if self.pressed { 1.0 } else { 0.0 },
            self.click_decay.clamp(0.0, 1.0),
        ]
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WindowChromeButtonsState {
    pub focused: bool,
    pub pinned: bool,
    pub maximized: bool,
    pub latency_enabled: bool,
    pub latency_visible: bool,
    pub pin: ButtonVisualState,
    pub latency: ButtonVisualState,
    pub diagnostics: ButtonVisualState,
    pub minimize: ButtonVisualState,
    pub maximize_restore: ButtonVisualState,
    pub close: ButtonVisualState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelEffect {
    BlueBackground = 0,
    GardenFrame = 1,
    TitleBar = 2,
    Text = 12,
    SpriteImage = 13,
    SceneButtonCard = 14,
    SceneBody = 15,
    WindowChromePin = 16,
    WindowChromeLatency = 17,
    WindowChromeDiagnostics = 18,
    WindowChromeMinimize = 19,
    WindowChromeMaximize = 20,
    WindowChromeRestore = 21,
    WindowChromeClose = 22,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PanelRect {
    pub rect: RECT,
    pub color: [f32; 4],
    pub effect: PanelEffect,
    pub data: [f32; 4],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlyphQuad {
    pub rect: RECT,
    pub color: [f32; 4],
    pub character: char,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpriteId {
    Terminal,
    Storage,
    Audio,
    CursorArrow,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpriteQuad {
    pub rect: RECT,
    pub color: [f32; 4],
    pub sprite: SpriteId,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RenderScene {
    pub panels: Vec<PanelRect>,
    pub glyphs: Vec<GlyphQuad>,
    pub sprites: Vec<SpriteQuad>,
}

impl RenderScene {
    #[must_use]
    pub fn empty() -> Self {
        Self {
            panels: Vec::new(),
            glyphs: Vec::new(),
            sprites: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShellSceneLayout {
    pub content_frame_rect: RECT,
    pub garden_rect: RECT,
    pub title_bar_rect: RECT,
    pub title_text_rect: RECT,
    pub body_rect: RECT,
    pub pin_button_rect: RECT,
    pub diagnostics_button_rect: RECT,
    pub latency_button_rect: RECT,
    pub minimize_button_rect: RECT,
    pub maximize_restore_button_rect: RECT,
    pub close_button_rect: RECT,
}

impl ShellSceneLayout {
    #[must_use]
    pub fn for_main_menu(client_width: i32, client_height: i32) -> Self {
        Self::for_main_menu_with_dpi(client_width, client_height, USER_DEFAULT_SCREEN_DPI)
    }

    #[must_use]
    pub fn for_main_menu_with_dpi(client_width: i32, client_height: i32, dpi: u32) -> Self {
        let content_inset = scale_for_dpi(18, dpi);
        let title_bar_height = scale_for_dpi(52, dpi);
        let title_text_left = scale_for_dpi(72, dpi);
        let title_text_right = scale_for_dpi(144, dpi);
        let body_top_gap = scale_for_dpi(14, dpi);
        let button_top_gap = scale_for_dpi(4, dpi);
        let button_size = scale_for_dpi(22, dpi);
        let right_inset = scale_for_dpi(4, dpi);
        let button_gap = scale_for_dpi(2, dpi);
        let left_button_inset = scale_for_dpi(6, dpi);
        let left_button_gap = scale_for_dpi(3, dpi);
        let content_frame_rect = RECT {
            left: content_inset,
            top: content_inset,
            right: client_width - content_inset,
            bottom: client_height - content_inset,
        };
        let garden_rect = RECT {
            left: 0,
            top: 0,
            right: client_width,
            bottom: client_height,
        };
        let title_bar_rect = RECT {
            left: content_frame_rect.left,
            top: content_frame_rect.top,
            right: content_frame_rect.right,
            bottom: content_frame_rect.top + title_bar_height,
        };
        let title_text_rect = RECT {
            left: title_bar_rect.left + title_text_left,
            top: title_bar_rect.top + scale_for_dpi(1, dpi),
            right: title_bar_rect.right - title_text_right,
            bottom: title_bar_rect.bottom - scale_for_dpi(3, dpi),
        };
        let body_rect = RECT {
            left: content_frame_rect.left,
            top: title_bar_rect.bottom + body_top_gap,
            right: content_frame_rect.right,
            bottom: content_frame_rect.bottom,
        };
        let button_top = title_bar_rect.top + button_top_gap;
        let right = title_bar_rect.right - right_inset;
        let close_button_rect = RECT {
            left: right - button_size,
            top: button_top,
            right,
            bottom: button_top + button_size,
        };
        let maximize_restore_button_rect = RECT {
            left: close_button_rect.left - button_size - button_gap,
            top: button_top,
            right: close_button_rect.left - button_gap,
            bottom: button_top + button_size,
        };
        let minimize_button_rect = RECT {
            left: maximize_restore_button_rect.left - button_size - button_gap,
            top: button_top,
            right: maximize_restore_button_rect.left - button_gap,
            bottom: button_top + button_size,
        };
        let pin_button_rect = RECT {
            left: title_bar_rect.left + left_button_inset,
            top: button_top,
            right: title_bar_rect.left + left_button_inset + button_size,
            bottom: button_top + button_size,
        };
        let diagnostics_button_rect = RECT {
            left: pin_button_rect.right + left_button_gap,
            top: button_top,
            right: pin_button_rect.right + left_button_gap + button_size,
            bottom: button_top + button_size,
        };
        let latency_button_rect = RECT {
            left: diagnostics_button_rect.right + left_button_gap,
            top: button_top,
            right: diagnostics_button_rect.right + left_button_gap + button_size,
            bottom: button_top + button_size,
        };

        Self {
            content_frame_rect,
            garden_rect,
            title_bar_rect,
            title_text_rect,
            body_rect,
            pin_button_rect,
            diagnostics_button_rect,
            latency_button_rect,
            minimize_button_rect,
            maximize_restore_button_rect,
            close_button_rect,
        }
    }
}

#[must_use]
pub fn scale_for_dpi(value: i32, dpi: u32) -> i32 {
    if dpi == USER_DEFAULT_SCREEN_DPI {
        return value;
    }

    let sign = value.signum();
    let magnitude = i64::from(value.abs());
    let scaled = ((magnitude * i64::from(dpi)) + i64::from(USER_DEFAULT_SCREEN_DPI / 2))
        / i64::from(USER_DEFAULT_SCREEN_DPI);
    let scaled = i32::try_from(scaled).unwrap_or(i32::MAX);
    sign.saturating_mul(scaled)
}

#[must_use]
pub fn preferred_background_color() -> [f32; 4] {
    preferred_background_color_from_dwm().unwrap_or([0.11, 0.44, 0.94, 0.5])
}

#[must_use]
pub fn preferred_title_bar_color(focused: bool) -> [f32; 4] {
    if focused {
        preferred_background_color_with_alpha(1.0).unwrap_or([0.11, 0.44, 0.94, 1.0])
    } else {
        [43.0 / 255.0, 43.0 / 255.0, 43.0 / 255.0, 1.0]
    }
}

fn preferred_background_color_from_dwm() -> Option<[f32; 4]> {
    preferred_background_color_with_alpha(0.5)
}

fn preferred_background_color_with_alpha(alpha: f32) -> Option<[f32; 4]> {
    let mut colorization = 0_u32;
    let mut opaque_blend = BOOL(0);
    unsafe { DwmGetColorizationColor(&mut colorization, &mut opaque_blend) }.ok()?;
    Some(colorization_color_to_rgba(colorization, alpha))
}

fn colorization_color_to_rgba(colorization: u32, alpha: f32) -> [f32; 4] {
    let red = ((colorization >> 16) & 0xFF) as u8;
    let green = ((colorization >> 8) & 0xFF) as u8;
    let blue = (colorization & 0xFF) as u8;

    [
        f32::from(red) / 255.0,
        f32::from(green) / 255.0,
        f32::from(blue) / 255.0,
        alpha,
    ]
}

#[must_use]
pub fn window_garden_shader_data(layout: ShellSceneLayout) -> [f32; 4] {
    let parent = layout.garden_rect;
    let child = layout.content_frame_rect;
    let width = (parent.right - parent.left).max(1) as f32;
    let height = (parent.bottom - parent.top).max(1) as f32;
    [
        ((child.left - parent.left) as f32 / width).clamp(0.0, 1.0),
        ((child.top - parent.top) as f32 / height).clamp(0.0, 1.0),
        ((child.right - parent.left) as f32 / width).clamp(0.0, 1.0),
        ((child.bottom - parent.top) as f32 / height).clamp(0.0, 1.0),
    ]
}

pub fn push_window_garden_frame(scene: &mut RenderScene, layout: ShellSceneLayout) {
    push_panel_with_data(
        scene,
        layout.garden_rect,
        preferred_background_color_with_alpha(1.0).unwrap_or([0.11, 0.44, 0.94, 1.0]),
        PanelEffect::GardenFrame,
        window_garden_shader_data(layout),
    );
}

pub fn push_window_chrome_buttons(
    scene: &mut RenderScene,
    layout: ShellSceneLayout,
    window_chrome_buttons_state: WindowChromeButtonsState,
) {
    let mut buttons = vec![
        (
            layout.pin_button_rect,
            window_chrome_button_color(window_chrome_buttons_state.pinned, false),
            window_chrome_buttons_state.pin,
            PanelEffect::WindowChromePin,
        ),
        (
            layout.diagnostics_button_rect,
            window_chrome_button_color(window_chrome_buttons_state.diagnostics.active, false),
            window_chrome_buttons_state.diagnostics,
            PanelEffect::WindowChromeDiagnostics,
        ),
        (
            layout.minimize_button_rect,
            window_chrome_button_color(false, false),
            window_chrome_buttons_state.minimize,
            PanelEffect::WindowChromeMinimize,
        ),
        (
            layout.maximize_restore_button_rect,
            window_chrome_button_color(window_chrome_buttons_state.maximized, false),
            window_chrome_buttons_state.maximize_restore,
            if window_chrome_buttons_state.maximized {
                PanelEffect::WindowChromeRestore
            } else {
                PanelEffect::WindowChromeMaximize
            },
        ),
        (
            layout.close_button_rect,
            [0.34, 0.12, 0.15, 1.0],
            window_chrome_buttons_state.close,
            PanelEffect::WindowChromeClose,
        ),
    ];

    if window_chrome_buttons_state.latency_enabled {
        buttons.insert(
            1,
            (
                layout.latency_button_rect,
                window_chrome_button_color(window_chrome_buttons_state.latency_visible, false),
                window_chrome_buttons_state.latency,
                PanelEffect::WindowChromeLatency,
            ),
        );
    }

    for (rect, color, state, effect) in buttons {
        push_panel_with_data(scene, rect, color, effect, state.shader_data());
    }
}

fn window_chrome_button_color(active: bool, destructive: bool) -> [f32; 4] {
    if destructive {
        [0.34, 0.12, 0.15, 1.0]
    } else if active {
        [0.23, 0.48, 0.69, 1.0]
    } else {
        [0.12, 0.13, 0.17, 1.0]
    }
}

pub fn push_panel(scene: &mut RenderScene, rect: RECT, color: [f32; 4], effect: PanelEffect) {
    push_panel_with_data(scene, rect, color, effect, [0.0; 4]);
}

pub fn push_panel_with_data(
    scene: &mut RenderScene,
    rect: RECT,
    color: [f32; 4],
    effect: PanelEffect,
    data: [f32; 4],
) {
    if scene.panels.len() >= MAX_PANEL_COUNT {
        return;
    }
    scene.panels.push(PanelRect {
        rect,
        color,
        effect,
        data,
    });
}

pub fn push_sprite(scene: &mut RenderScene, rect: RECT, color: [f32; 4], sprite: SpriteId) {
    scene.sprites.push(SpriteQuad {
        rect,
        color,
        sprite,
    });
}

pub fn push_text_block(
    scene: &mut RenderScene,
    rect: RECT,
    text: &str,
    glyph_width: i32,
    glyph_height: i32,
    color: [f32; 4],
) {
    let mut cursor_x = rect.left;
    let mut cursor_y = rect.top;
    for character in text.chars() {
        if character == '\n' {
            cursor_x = rect.left;
            cursor_y += glyph_height;
            if cursor_y + glyph_height > rect.bottom {
                break;
            }
            continue;
        }
        if cursor_x + glyph_width > rect.right {
            cursor_x = rect.left;
            cursor_y += glyph_height;
        }
        if cursor_y + glyph_height > rect.bottom {
            break;
        }
        if character != ' ' && scene.glyphs.len() < MAX_GLYPH_COUNT {
            scene.glyphs.push(GlyphQuad {
                rect: RECT {
                    left: cursor_x,
                    top: cursor_y,
                    right: cursor_x + glyph_width,
                    bottom: cursor_y + glyph_height,
                },
                color,
                character,
            });
        }
        cursor_x += glyph_width;
    }
}

pub fn push_centered_text(scene: &mut RenderScene, rect: RECT, text: &str, color: [f32; 4]) {
    let glyph_count = i32::try_from(text.chars().count())
        .unwrap_or_default()
        .max(1);
    let available_width = (rect.right - rect.left - 16).max(8);
    let available_height = (rect.bottom - rect.top - 16).max(8);
    let glyph_height = available_height.clamp(12, 28);
    let glyph_width = ((available_width / glyph_count).min((glyph_height * 3) / 2)).max(8);
    let total_width = glyph_width * glyph_count;
    let text_rect = RECT {
        left: rect.left + (((rect.right - rect.left) - total_width).max(0) / 2),
        top: rect.top + (((rect.bottom - rect.top) - glyph_height).max(0) / 2),
        right: rect.right,
        bottom: rect.bottom,
    };
    push_text_block(scene, text_rect, text, glyph_width, glyph_height, color);
}

pub fn push_title_text(scene: &mut RenderScene, rect: RECT, text: &str, color: [f32; 4]) {
    let glyph_count = i32::try_from(text.chars().count())
        .unwrap_or_default()
        .max(1);
    let available_width = (rect.right - rect.left - 20).max(8);
    let available_height = (rect.bottom - rect.top - 12).max(8);
    let glyph_height = available_height.clamp(16, 36);
    let glyph_width = ((available_width / glyph_count).min((glyph_height * 3) / 2)).max(8);
    let total_width = glyph_width * glyph_count;
    let text_rect = RECT {
        left: rect.left + (((rect.right - rect.left) - total_width).max(0) / 2),
        top: rect.top + (((rect.bottom - rect.top) - glyph_height).max(0) / 2),
        right: rect.right,
        bottom: rect.bottom,
    };
    push_text_block(scene, text_rect, text, glyph_width, glyph_height, color);
}

#[cfg(test)]
mod tests {
    use super::{
        ButtonVisualState, PanelEffect, RenderScene, ShellSceneLayout, WindowChromeButtonsState,
        preferred_title_bar_color, push_window_chrome_buttons, push_window_garden_frame,
    };

    #[test]
    fn focused_title_bar_uses_opaque_accent_color() {
        let color = preferred_title_bar_color(true);
        assert_eq!(color[3], 1.0);
    }

    #[test]
    fn shell_helpers_emit_garden_frame_and_chrome_panels() {
        let layout = ShellSceneLayout::for_main_menu(1280, 820);
        let mut scene = RenderScene::empty();
        push_window_garden_frame(&mut scene, layout);
        push_window_chrome_buttons(
            &mut scene,
            layout,
            WindowChromeButtonsState {
                focused: true,
                pin: ButtonVisualState::default(),
                diagnostics: ButtonVisualState::default(),
                minimize: ButtonVisualState::default(),
                maximize_restore: ButtonVisualState::default(),
                close: ButtonVisualState::default(),
                ..Default::default()
            },
        );

        assert_eq!(scene.panels[0].effect, PanelEffect::GardenFrame);
        assert!(
            scene
                .panels
                .iter()
                .any(|panel| panel.effect == PanelEffect::WindowChromePin)
        );
        assert!(
            scene
                .panels
                .iter()
                .any(|panel| panel.effect == PanelEffect::WindowChromeClose)
        );
    }

    #[test]
    fn panel_effect_values_match_legacy_shader_contract() {
        assert_eq!(PanelEffect::Text as u32, 12);
        assert_eq!(PanelEffect::SpriteImage as u32, 13);
        assert_eq!(PanelEffect::SceneButtonCard as u32, 14);
        assert_eq!(PanelEffect::SceneBody as u32, 15);
    }

    #[test]
    fn main_menu_layout_matches_legacy_garden_and_body_spacing() {
        let layout = ShellSceneLayout::for_main_menu(1280, 820);

        assert_eq!(layout.content_frame_rect.left, 18);
        assert_eq!(layout.content_frame_rect.top, 18);
        assert_eq!(layout.garden_rect.left, 0);
        assert_eq!(layout.garden_rect.top, 0);
        assert_eq!(layout.title_bar_rect.bottom - layout.title_bar_rect.top, 52);
        assert_eq!(layout.body_rect.left, layout.content_frame_rect.left);
        assert_eq!(layout.body_rect.right, layout.content_frame_rect.right);
        assert_eq!(layout.body_rect.bottom, layout.content_frame_rect.bottom);
        assert_eq!(layout.body_rect.top - layout.title_bar_rect.bottom, 14);
    }
}

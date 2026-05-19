use linkme::distributed_slice;
use teamy_studio_shell::{
    ButtonVisualState, FEATURE_WINDOW_SCENE_REGISTRATIONS, FeatureWindowSceneContext,
    FeatureWindowSceneRegistration, PanelEffect, RenderScene, SpriteId, push_centered_text,
    push_panel, push_panel_with_data, push_sprite, push_text_block, push_title_text,
    push_window_chrome_buttons, push_window_garden_frame, scale_for_dpi,
};
use windows::Win32::Foundation::{POINT, RECT};
use windows::Win32::UI::WindowsAndMessaging::{
    IDC_ARROW, IDC_CROSS, IDC_HAND, IDC_HELP, IDC_IBEAM, IDC_SIZEALL, IDC_WAIT,
};
use windows::core::PCWSTR;

const MAX_COLUMNS: usize = 4;

#[derive(Clone, Copy)]
enum CursorGalleryCursorKind {
    Arrow,
    Hand,
    IBeam,
    Cross,
    Wait,
    SizeAll,
    Help,
}

#[derive(Clone, Copy)]
struct CursorGallerySpriteSpec {
    label: &'static str,
    sprite: SpriteId,
    cursor: CursorGalleryCursorKind,
    color: [f32; 4],
}

#[derive(Clone, Copy)]
struct CursorGalleryCellLayout {
    index: usize,
    spec: CursorGallerySpriteSpec,
    card_rect: RECT,
    sprite_rect: RECT,
    label_rect: RECT,
}

const CURSOR_GALLERY_SPRITES: [CursorGallerySpriteSpec; 7] = [
    CursorGallerySpriteSpec {
        label: "Arrow",
        sprite: SpriteId::CursorArrow,
        cursor: CursorGalleryCursorKind::Arrow,
        color: [0.48, 0.95, 1.00, 1.0],
    },
    CursorGallerySpriteSpec {
        label: "Hand",
        sprite: SpriteId::CursorHand,
        cursor: CursorGalleryCursorKind::Hand,
        color: [1.00, 0.56, 0.88, 1.0],
    },
    CursorGallerySpriteSpec {
        label: "I-Beam",
        sprite: SpriteId::CursorIBeam,
        cursor: CursorGalleryCursorKind::IBeam,
        color: [0.78, 1.00, 0.58, 1.0],
    },
    CursorGallerySpriteSpec {
        label: "Cross",
        sprite: SpriteId::CursorCross,
        cursor: CursorGalleryCursorKind::Cross,
        color: [1.00, 0.78, 0.36, 1.0],
    },
    CursorGallerySpriteSpec {
        label: "Wait",
        sprite: SpriteId::CursorWait,
        cursor: CursorGalleryCursorKind::Wait,
        color: [0.72, 0.64, 1.00, 1.0],
    },
    CursorGallerySpriteSpec {
        label: "Move",
        sprite: SpriteId::CursorSizeAll,
        cursor: CursorGalleryCursorKind::SizeAll,
        color: [0.44, 0.88, 0.70, 1.0],
    },
    CursorGallerySpriteSpec {
        label: "Help",
        sprite: SpriteId::CursorHelp,
        cursor: CursorGalleryCursorKind::Help,
        color: [1.00, 0.86, 0.48, 1.0],
    },
];

#[distributed_slice(FEATURE_WINDOW_SCENE_REGISTRATIONS)]
pub static CURSOR_GALLERY_WINDOW_SCENE: FeatureWindowSceneRegistration =
    FeatureWindowSceneRegistration {
        title: "Cursor Gallery",
        build_scene: build_cursor_gallery_window_scene,
        cursor_for_point: Some(cursor_gallery_cursor_for_point),
    };

fn build_cursor_gallery_window_scene(context: FeatureWindowSceneContext) -> RenderScene {
    let mut scene = RenderScene::empty();
    let layout = context.layout;
    let dpi = context.dpi;

    push_panel(
        &mut scene,
        layout.content_frame_rect,
        teamy_studio_shell::preferred_background_color(),
        PanelEffect::BlueBackground,
    );
    push_window_garden_frame(&mut scene, layout);
    push_panel(
        &mut scene,
        layout.title_bar_rect,
        teamy_studio_shell::preferred_title_bar_color(context.chrome.focused),
        PanelEffect::TitleBar,
    );
    push_window_chrome_buttons(&mut scene, layout, context.chrome);
    push_title_text(
        &mut scene,
        layout.title_text_rect,
        "Cursor Gallery",
        [0.96, 0.98, 1.00, 1.0],
    );

    let body_rect = inset_rect(layout.body_rect, scale_for_dpi(24, dpi));
    push_panel(
        &mut scene,
        body_rect,
        [0.07, 0.08, 0.10, 0.96],
        PanelEffect::SceneBody,
    );

    let headline_rect = cursor_gallery_headline_rect(body_rect, dpi);
    let summary_rect = cursor_gallery_summary_rect(headline_rect, dpi);
    push_title_text(
        &mut scene,
        headline_rect,
        "Stock OS Cursor Sprites",
        [0.94, 0.97, 1.00, 1.0],
    );
    push_text_block(
        &mut scene,
        summary_rect,
        "This composed shell window now renders the cursor gallery through the event-driven host path, including stock cursor hover behavior and adaptive card fitting.",
        scale_for_dpi(10, dpi).max(8),
        scale_for_dpi(18, dpi).max(12),
        [0.80, 0.84, 0.90, 1.0],
    );

    let cells = cursor_gallery_cell_layouts(body_rect, dpi);
    let hovered_index = context.cursor_client_point.and_then(|point| {
        cells
            .iter()
            .find(|cell| rect_contains(cell.card_rect, point))
            .map(|cell| cell.index)
    });

    for cell in &cells {
        let hovered = hovered_index == Some(cell.index);

        if hovered {
            push_cursor_gallery_glow(&mut scene, cell.card_rect, cell.spec.color, true);
        }
        push_panel_with_data(
            &mut scene,
            cell.card_rect,
            if hovered {
                [
                    cell.spec.color[0] * 0.30,
                    cell.spec.color[1] * 0.30,
                    cell.spec.color[2] * 0.30,
                    0.98,
                ]
            } else {
                [0.11, 0.12, 0.16, 0.92]
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
        push_sprite(
            &mut scene,
            cell.sprite_rect,
            cell.spec.color,
            cell.spec.sprite,
        );
        push_centered_text(
            &mut scene,
            cell.label_rect,
            cell.spec.label,
            [0.90, 0.92, 0.96, 1.0],
        );
    }

    scene
}

fn cursor_gallery_cell_layouts(body_rect: RECT, dpi: u32) -> Vec<CursorGalleryCellLayout> {
    let left_inset = scale_for_dpi(20, dpi);
    let right_inset = scale_for_dpi(20, dpi);
    let cards_top = cursor_gallery_summary_rect(cursor_gallery_headline_rect(body_rect, dpi), dpi)
        .bottom
        + scale_for_dpi(18, dpi);
    let bottom_inset = scale_for_dpi(18, dpi);
    let preferred_card_size = scale_for_dpi(132, dpi);
    let min_card_size = scale_for_dpi(84, dpi);
    let gap = scale_for_dpi(20, dpi);
    let row_gap = scale_for_dpi(18, dpi);
    let label_gap = scale_for_dpi(6, dpi);
    let label_height = scale_for_dpi(36, dpi);
    let available_width = (body_rect.right - body_rect.left) - left_inset - right_inset;
    let available_height = (body_rect.bottom - cards_top) - bottom_inset;

    let mut best_columns = 1usize;
    let mut best_card_size = min_card_size;
    for columns in 1..=CURSOR_GALLERY_SPRITES.len().min(MAX_COLUMNS) {
        let rows = CURSOR_GALLERY_SPRITES.len().div_ceil(columns);
        let width_limited = (available_width - gap * (columns as i32 - 1)) / columns as i32;
        let height_limited = (available_height
            - row_gap * (rows as i32 - 1)
            - (label_gap + label_height) * rows as i32)
            / rows as i32;
        let candidate = width_limited.min(height_limited).min(preferred_card_size);
        if candidate >= min_card_size && candidate >= best_card_size {
            best_columns = columns;
            best_card_size = candidate;
        }
    }

    let columns = best_columns;
    let card_size = best_card_size.max(min_card_size);

    CURSOR_GALLERY_SPRITES
        .iter()
        .copied()
        .enumerate()
        .map(|(index, spec)| {
            let column = (index % columns) as i32;
            let row = (index / columns) as i32;
            let left = body_rect.left + left_inset + column * (card_size + gap);
            let top = cards_top + row * (card_size + label_gap + label_height + row_gap);
            let card_rect = RECT {
                left,
                top,
                right: left + card_size,
                bottom: top + card_size,
            };
            let sprite_rect = inset_rect(card_rect, (card_size / 5).max(scale_for_dpi(12, dpi)));
            let label_rect = RECT {
                left: card_rect.left,
                top: card_rect.bottom + label_gap,
                right: card_rect.right,
                bottom: card_rect.bottom + label_gap + label_height,
            };

            CursorGalleryCellLayout {
                index,
                spec,
                card_rect,
                sprite_rect,
                label_rect,
            }
        })
        .filter(|cell| cell.label_rect.bottom <= body_rect.bottom - bottom_inset + label_height)
        .collect()
}

fn cursor_gallery_cursor_for_point(
    context: FeatureWindowSceneContext,
    point: POINT,
) -> Option<PCWSTR> {
    let body_rect = inset_rect(context.layout.body_rect, scale_for_dpi(24, context.dpi));
    cursor_gallery_cell_layouts(body_rect, context.dpi)
        .into_iter()
        .find(|cell| rect_contains(cell.card_rect, point))
        .map(|cell| cursor_gallery_system_cursor(cell.spec.cursor))
}

fn cursor_gallery_headline_rect(body_rect: RECT, dpi: u32) -> RECT {
    RECT {
        left: body_rect.left + scale_for_dpi(20, dpi),
        top: body_rect.top + scale_for_dpi(14, dpi),
        right: body_rect.right - scale_for_dpi(20, dpi),
        bottom: body_rect.top + scale_for_dpi(62, dpi),
    }
}

fn cursor_gallery_summary_rect(headline_rect: RECT, dpi: u32) -> RECT {
    RECT {
        left: headline_rect.left,
        top: headline_rect.bottom + scale_for_dpi(6, dpi),
        right: headline_rect.right,
        bottom: headline_rect.bottom + scale_for_dpi(42, dpi),
    }
}

fn cursor_gallery_system_cursor(cursor: CursorGalleryCursorKind) -> PCWSTR {
    match cursor {
        CursorGalleryCursorKind::Arrow => IDC_ARROW,
        CursorGalleryCursorKind::Hand => IDC_HAND,
        CursorGalleryCursorKind::IBeam => IDC_IBEAM,
        CursorGalleryCursorKind::Cross => IDC_CROSS,
        CursorGalleryCursorKind::Wait => IDC_WAIT,
        CursorGalleryCursorKind::SizeAll => IDC_SIZEALL,
        CursorGalleryCursorKind::Help => IDC_HELP,
    }
}

fn push_cursor_gallery_glow(
    scene: &mut RenderScene,
    card_rect: RECT,
    color: [f32; 4],
    hovered: bool,
) {
    for (inflate, alpha) in if hovered {
        [(18, 0.10), (10, 0.18), (4, 0.28)]
    } else {
        [(10, 0.04), (6, 0.08), (2, 0.12)]
    } {
        push_panel_with_data(
            scene,
            RECT {
                left: card_rect.left - inflate,
                top: card_rect.top - inflate,
                right: card_rect.right + inflate,
                bottom: card_rect.bottom + inflate,
            },
            [color[0], color[1], color[2], alpha],
            PanelEffect::SceneButtonCard,
            ButtonVisualState {
                hover_near: 1.0,
                active: true,
                ..Default::default()
            }
            .shader_data(),
        );
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

fn rect_contains(rect: RECT, point: POINT) -> bool {
    point.x >= rect.left && point.x < rect.right && point.y >= rect.top && point.y < rect.bottom
}

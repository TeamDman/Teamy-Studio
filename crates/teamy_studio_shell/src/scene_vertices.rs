use crate::{PanelEffect, RenderScene, SceneTextAtlas, SpriteId, build_scene_text_atlas};
use teamy_studio_fonts::{LoadedTerminalFont, SlugGlyph};
use windows::Win32::Foundation::RECT;

const MAX_PANEL_COUNT: usize = 8_192;
const MAX_GLYPH_COUNT: usize = 8_192;
const MAX_SPRITE_COUNT: usize = 256;
const MAX_VERTEX_COUNT: usize = (MAX_PANEL_COUNT + MAX_GLYPH_COUNT + MAX_SPRITE_COUNT) * 6;
const FALLBACK_GLYPH: char = '?';

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneVertex {
    pub position: [f32; 3],
    pub color: [f32; 4],
    pub uv: [f32; 2],
    pub effect: f32,
    pub glyph: f32,
    pub glyph_data: [f32; 4],
    pub banding: [f32; 4],
    pub normal: [f32; 2],
    pub jacobian: [f32; 4],
    pub local_bounds: [f32; 4],
    pub padding: [f32; 2],
}

#[must_use]
pub fn build_scene_vertices(scene: &RenderScene) -> eyre::Result<Vec<SceneVertex>> {
    let text_atlas = build_scene_text_atlas(scene)?;
    Ok(build_scene_vertices_with_text_atlas(scene, &text_atlas))
}

#[must_use]
pub fn build_scene_vertices_with_text_atlas(
    scene: &RenderScene,
    text_atlas: &SceneTextAtlas,
) -> Vec<SceneVertex> {
    let mut vertices = Vec::with_capacity((scene.panels.len() + scene.sprites.len() + scene.glyphs.len()) * 6);

    for panel in &scene.panels {
        append_rect_with_data(
            &mut vertices,
            panel.rect,
            panel.color,
            panel.effect as u32,
            0,
            [0.0, 0.0, 1.0, 1.0],
            panel.data,
        );
    }

    for sprite in &scene.sprites {
        append_rect_with_data(
            &mut vertices,
            sprite.rect,
            sprite.color,
            PanelEffect::SpriteImage as u32,
            0,
            sprite_uv_rect(sprite.sprite),
            [0.0; 4],
        );
    }

    for glyph in &scene.glyphs {
        let slug_glyph = text_atlas
            .glyphs
            .get(&glyph.character)
            .or_else(|| text_atlas.glyphs.get(&FALLBACK_GLYPH))
            .copied()
            .unwrap_or_else(|| empty_slug_glyph(&text_atlas.font));
        append_text_rect(
            &mut vertices,
            glyph.rect,
            glyph.color,
            slug_glyph,
            &text_atlas.font,
        );
    }

    vertices
}

fn empty_slug_glyph(font: &LoadedTerminalFont) -> SlugGlyph {
    SlugGlyph {
        curve_start: 0,
        curve_count: 0,
        band_start: 0,
        band_count_x: 1,
        band_count_y: 1,
        band_transform: [0.0; 4],
        x_min: 0.0,
        y_min: 0.0,
        x_max: font.cell_advance,
        y_max: font.ascender,
        advance: font.cell_advance,
    }
}

fn sprite_uv_rect(sprite: SpriteId) -> [f32; 4] {
    match sprite {
        SpriteId::Terminal => [0.0, 0.0, 0.25, 0.25],
        SpriteId::Storage => [0.25, 0.0, 0.5, 0.25],
        SpriteId::Audio => [0.5, 0.0, 0.75, 0.25],
        SpriteId::CursorArrow => [0.75, 0.0, 1.0, 0.25],
    }
}

fn append_text_rect(
    vertices: &mut Vec<SceneVertex>,
    rect: RECT,
    color: [f32; 4],
    glyph: SlugGlyph,
    font: &LoadedTerminalFont,
) {
    if vertices.len() + 6 > MAX_VERTEX_COUNT {
        return;
    }

    let left = rect.left as f32;
    let top = rect.top as f32;
    let glyph_data = [
        glyph.curve_start as f32,
        glyph.curve_count as f32,
        glyph.band_count_x.saturating_sub(1) as f32,
        glyph.band_count_y.saturating_sub(1) as f32,
    ];
    let banding = glyph.band_transform;
    let screen_width = (rect.right - rect.left) as f32;
    let screen_height = (rect.bottom - rect.top) as f32;
    let advance = glyph.advance.max(1.0);
    let font_height = font.units_per_em.max(1.0);
    let glyph_left = left + (glyph.x_min / advance) * screen_width;
    let glyph_right = left + (glyph.x_max / advance) * screen_width;
    let glyph_top = top + ((font.ascender - glyph.y_max) / font_height) * screen_height;
    let glyph_bottom = top + ((font.ascender - glyph.y_min) / font_height) * screen_height;
    let jacobian = [
        advance / screen_width.max(1.0),
        0.0,
        0.0,
        -font_height / screen_height.max(1.0),
    ];
    let effect = PanelEffect::Text as u32 as f32;
    let glyph_index = glyph.band_start as f32;

    let top_left = SceneVertex {
        position: [glyph_left, glyph_top, 0.0],
        color,
        uv: [glyph.x_min, glyph.y_max],
        effect,
        glyph: glyph_index,
        glyph_data,
        banding,
        normal: [-1.0, 1.0],
        jacobian,
        local_bounds: [0.0; 4],
        padding: [0.0; 2],
    };
    let top_right = SceneVertex {
        position: [glyph_right, glyph_top, 0.0],
        color,
        uv: [glyph.x_max, glyph.y_max],
        effect,
        glyph: glyph_index,
        glyph_data,
        banding,
        normal: [1.0, 1.0],
        jacobian,
        local_bounds: [0.0; 4],
        padding: [0.0; 2],
    };
    let bottom_right = SceneVertex {
        position: [glyph_right, glyph_bottom, 0.0],
        color,
        uv: [glyph.x_max, glyph.y_min],
        effect,
        glyph: glyph_index,
        glyph_data,
        banding,
        normal: [1.0, -1.0],
        jacobian,
        local_bounds: [0.0; 4],
        padding: [0.0; 2],
    };
    let bottom_left = SceneVertex {
        position: [glyph_left, glyph_bottom, 0.0],
        color,
        uv: [glyph.x_min, glyph.y_min],
        effect,
        glyph: glyph_index,
        glyph_data,
        banding,
        normal: [-1.0, -1.0],
        jacobian,
        local_bounds: [0.0; 4],
        padding: [0.0; 2],
    };

    vertices.extend_from_slice(&[
        top_left,
        top_right,
        bottom_right,
        top_left,
        bottom_right,
        bottom_left,
    ]);
}

fn append_rect_with_data(
    vertices: &mut Vec<SceneVertex>,
    rect: RECT,
    color: [f32; 4],
    effect: u32,
    glyph_index: u32,
    uv_rect: [f32; 4],
    data: [f32; 4],
) {
    if vertices.len() + 6 > MAX_VERTEX_COUNT {
        return;
    }

    let left = rect.left as f32;
    let top = rect.top as f32;
    let right = rect.right as f32;
    let bottom = rect.bottom as f32;
    let effect = effect as f32;
    let glyph = glyph_index as f32;
    let [uv_left, uv_top, uv_right, uv_bottom] = uv_rect;

    let top_left = SceneVertex {
        position: [left, top, 0.0],
        color,
        uv: [uv_left, uv_top],
        effect,
        glyph,
        glyph_data: data,
        banding: [0.0; 4],
        normal: [0.0; 2],
        jacobian: [0.0; 4],
        local_bounds: [0.0; 4],
        padding: [0.0; 2],
    };
    let top_right = SceneVertex {
        position: [right, top, 0.0],
        color,
        uv: [uv_right, uv_top],
        effect,
        glyph,
        glyph_data: data,
        banding: [0.0; 4],
        normal: [0.0; 2],
        jacobian: [0.0; 4],
        local_bounds: [0.0; 4],
        padding: [0.0; 2],
    };
    let bottom_right = SceneVertex {
        position: [right, bottom, 0.0],
        color,
        uv: [uv_right, uv_bottom],
        effect,
        glyph,
        glyph_data: data,
        banding: [0.0; 4],
        normal: [0.0; 2],
        jacobian: [0.0; 4],
        local_bounds: [0.0; 4],
        padding: [0.0; 2],
    };
    let bottom_left = SceneVertex {
        position: [left, bottom, 0.0],
        color,
        uv: [uv_left, uv_bottom],
        effect,
        glyph,
        glyph_data: data,
        banding: [0.0; 4],
        normal: [0.0; 2],
        jacobian: [0.0; 4],
        local_bounds: [0.0; 4],
        padding: [0.0; 2],
    };

    vertices.extend_from_slice(&[
        top_left,
        top_right,
        bottom_right,
        top_left,
        bottom_right,
        bottom_left,
    ]);
}

#[cfg(test)]
mod tests {
    use super::{SceneVertex, build_scene_vertices, build_scene_vertices_with_text_atlas};
    use crate::{
        GlyphQuad, PanelEffect, PanelRect, RenderScene, build_scene_text_atlas_from_fragments,
    };
    use windows::Win32::Foundation::RECT;

    fn rect(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
        RECT {
            left,
            top,
            right,
            bottom,
        }
    }

    fn scene_with_panel_and_text() -> RenderScene {
        RenderScene {
            panels: vec![PanelRect {
                rect: rect(10, 20, 110, 220),
                color: [0.1, 0.2, 0.3, 1.0],
                effect: PanelEffect::SceneBody,
                data: [0.0; 4],
            }],
            glyphs: vec![GlyphQuad {
                rect: rect(20, 40, 52, 72),
                color: [1.0, 1.0, 1.0, 1.0],
                character: 'A',
            }],
            sprites: Vec::new(),
        }
    }

    #[test]
    fn build_scene_vertices_emits_panel_and_text_quads() -> eyre::Result<()> {
        let scene = scene_with_panel_and_text();

        let vertices = build_scene_vertices(&scene)?;

        assert_eq!(vertices.len(), 12);
        assert_eq!(vertices[0].effect, PanelEffect::SceneBody as u32 as f32);
        assert_eq!(vertices[6].effect, PanelEffect::Text as u32 as f32);
        Ok(())
    }

    #[test]
    fn build_scene_vertices_with_text_atlas_preserves_text_triangle_order() -> eyre::Result<()> {
        let scene = scene_with_panel_and_text();
        let text_atlas = build_scene_text_atlas_from_fragments(&[&scene])?;

        let vertices = build_scene_vertices_with_text_atlas(&scene, &text_atlas);
        let text_vertices: &[SceneVertex] = &vertices[6..12];

        assert_eq!(text_vertices.len(), 6);
        assert_eq!(text_vertices[0].normal, [-1.0, 1.0]);
        assert_eq!(text_vertices[1].normal, [1.0, 1.0]);
        assert_eq!(text_vertices[2].normal, [1.0, -1.0]);
        assert_eq!(text_vertices[3], text_vertices[0]);
        assert_eq!(text_vertices[4], text_vertices[2]);
        assert_eq!(text_vertices[5].normal, [-1.0, -1.0]);
        Ok(())
    }
}
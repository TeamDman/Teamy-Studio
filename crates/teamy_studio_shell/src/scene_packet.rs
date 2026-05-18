use std::sync::Arc;

use teamy_studio_fonts::LoadedTerminalFont;

use crate::{
    RenderScene, SceneVertex, build_scene_text_atlas, build_scene_vertices_with_text_atlas,
};

#[derive(Clone, Debug)]
pub struct PreparedRenderScene {
    pub font: Arc<LoadedTerminalFont>,
    pub vertices: Vec<SceneVertex>,
    pub curve_data: Vec<[f32; 4]>,
    pub band_data: Vec<u32>,
}

pub fn prepare_render_scene(scene: &RenderScene) -> eyre::Result<PreparedRenderScene> {
    let text_atlas = build_scene_text_atlas(scene)?;
    let vertices = build_scene_vertices_with_text_atlas(scene, &text_atlas);
    Ok(PreparedRenderScene {
        font: Arc::clone(&text_atlas.font),
        vertices,
        curve_data: text_atlas.curve_data,
        band_data: text_atlas.band_data,
    })
}

#[cfg(test)]
mod tests {
    use crate::{GlyphQuad, RenderScene};
    use windows::Win32::Foundation::RECT;

    use super::prepare_render_scene;

    #[test]
    fn prepare_render_scene_packages_vertices_and_slug_buffers() -> eyre::Result<()> {
        let scene = RenderScene {
            panels: Vec::new(),
            glyphs: vec![GlyphQuad {
                rect: RECT {
                    left: 0,
                    top: 0,
                    right: 24,
                    bottom: 24,
                },
                color: [1.0, 1.0, 1.0, 1.0],
                character: 'A',
            }],
            sprites: Vec::new(),
        };

        let prepared = prepare_render_scene(&scene)?;

        assert_eq!(prepared.vertices.len(), 6);
        assert!(prepared.font.cell_advance > 0.0);
        assert!(!prepared.curve_data.is_empty());
        assert!(!prepared.band_data.is_empty());
        Ok(())
    }
}

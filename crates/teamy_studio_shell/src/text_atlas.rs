use std::collections::BTreeSet;
use std::sync::Arc;

use teamy_studio_fonts::{LoadedTerminalFont, SlugAtlasData, SlugGlyph, build_terminal_slug_atlas};

use crate::RenderScene;

#[derive(Clone, Debug)]
pub struct SceneTextAtlas {
    pub font: Arc<LoadedTerminalFont>,
    pub curve_data: Vec<[f32; 4]>,
    pub band_data: Vec<u32>,
    pub glyphs: std::collections::HashMap<char, SlugGlyph>,
}

pub fn build_scene_text_atlas(scene: &RenderScene) -> eyre::Result<SceneTextAtlas> {
    build_scene_text_atlas_from_fragments(&[scene])
}

pub fn build_scene_text_atlas_from_fragments(
    scenes: &[&RenderScene],
) -> eyre::Result<SceneTextAtlas> {
    let characters = collect_scene_characters(scenes);
    let SlugAtlasData {
        font,
        curve_data,
        band_data,
        glyphs,
    } = build_terminal_slug_atlas(&characters)?;

    Ok(SceneTextAtlas {
        font,
        curve_data,
        band_data,
        glyphs,
    })
}

#[must_use]
pub fn collect_scene_characters(scenes: &[&RenderScene]) -> Vec<char> {
    let mut characters = BTreeSet::new();
    for scene in scenes {
        for glyph in &scene.glyphs {
            characters.insert(glyph.character);
        }
    }
    characters.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use windows::Win32::Foundation::RECT;

    use crate::{GlyphQuad, RenderScene};

    use super::{build_scene_text_atlas_from_fragments, collect_scene_characters};

    fn glyph(character: char, left: i32) -> GlyphQuad {
        GlyphQuad {
            rect: RECT {
                left,
                top: 0,
                right: left + 12,
                bottom: 16,
            },
            color: [1.0, 1.0, 1.0, 1.0],
            character,
        }
    }

    #[test]
    fn collect_scene_characters_deduplicates_and_sorts() {
        let first = RenderScene {
            panels: Vec::new(),
            glyphs: vec![glyph('B', 0), glyph('A', 12), glyph('B', 24)],
            sprites: Vec::new(),
        };
        let second = RenderScene {
            panels: Vec::new(),
            glyphs: vec![glyph('?', 0), glyph('A', 12)],
            sprites: Vec::new(),
        };

        let characters = collect_scene_characters(&[&first, &second]);
        assert_eq!(characters, vec!['?', 'A', 'B']);
    }

    #[test]
    fn scene_text_atlas_builds_slug_assets_from_shell_fragments() -> eyre::Result<()> {
        let first = RenderScene {
            panels: Vec::new(),
            glyphs: vec![glyph('A', 0), glyph('b', 12)],
            sprites: Vec::new(),
        };
        let second = RenderScene {
            panels: Vec::new(),
            glyphs: vec![glyph('?', 0)],
            sprites: Vec::new(),
        };

        let atlas = build_scene_text_atlas_from_fragments(&[&first, &second])?;
        assert!(atlas.font.cell_advance > 0.0);
        assert!(atlas.glyphs.contains_key(&'A'));
        assert!(atlas.glyphs.contains_key(&'b'));
        assert!(atlas.glyphs.contains_key(&'?'));
        assert!(!atlas.curve_data.is_empty());
        assert!(!atlas.band_data.is_empty());
        Ok(())
    }
}

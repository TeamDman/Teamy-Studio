use std::ops::Range;

use crate::{
    PreparedRenderScene, SceneVertex, ShaderResourceCapacities, ensure_shader_resource_capacities,
    padded_band_upload_data, padded_curve_upload_data,
};

#[derive(Clone, Debug)]
pub struct PreparedSceneUploadBatch {
    pub vertices: Vec<SceneVertex>,
    pub vertex_ranges: Vec<Range<usize>>,
    pub capacities: ShaderResourceCapacities,
    pub curve_upload_data: Vec<[f32; 4]>,
    pub band_upload_data: Vec<u32>,
}

#[must_use]
pub fn build_prepared_scene_upload_batch(
    prepared_scene: &PreparedRenderScene,
    current_capacities: ShaderResourceCapacities,
) -> PreparedSceneUploadBatch {
    let capacities = ensure_shader_resource_capacities(current_capacities, prepared_scene);
    let vertex_count = prepared_scene.vertices.len();
    PreparedSceneUploadBatch {
        vertices: prepared_scene.vertices.clone(),
        vertex_ranges: if vertex_count == 0 {
            Vec::new()
        } else {
            vec![0..vertex_count]
        },
        capacities,
        curve_upload_data: padded_curve_upload_data(prepared_scene, capacities),
        band_upload_data: padded_band_upload_data(prepared_scene, capacities),
    }
}

#[cfg(test)]
mod tests {
    use crate::{GlyphQuad, RenderScene, ShaderResourceCapacities, prepare_render_scene};
    use windows::Win32::Foundation::RECT;

    use super::build_prepared_scene_upload_batch;

    #[test]
    fn build_prepared_scene_upload_batch_packages_full_upload_inputs() -> eyre::Result<()> {
        let prepared = prepare_render_scene(&RenderScene {
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
        })?;

        let batch =
            build_prepared_scene_upload_batch(&prepared, ShaderResourceCapacities::default());

        assert_eq!(batch.vertices.len(), prepared.vertices.len());
        assert_eq!(batch.vertex_ranges, vec![0..prepared.vertices.len()]);
        assert!(batch.curve_upload_data.len() >= prepared.curve_data.len());
        assert!(batch.band_upload_data.len() >= prepared.band_data.len());
        Ok(())
    }
}

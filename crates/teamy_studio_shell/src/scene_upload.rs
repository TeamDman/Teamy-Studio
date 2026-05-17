use std::mem::size_of;
use std::ops::Range;

use crate::{PreparedRenderScene, SceneVertex};

pub const DEFAULT_CURVE_FLOAT4_CAPACITY: usize = 262_144;
pub const DEFAULT_BAND_UINT_CAPACITY: usize = 1_048_576;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShaderResourceCapacities {
    pub curve_capacity: usize,
    pub band_capacity: usize,
}

impl Default for ShaderResourceCapacities {
    fn default() -> Self {
        Self {
            curve_capacity: DEFAULT_CURVE_FLOAT4_CAPACITY,
            band_capacity: DEFAULT_BAND_UINT_CAPACITY,
        }
    }
}

#[must_use]
pub fn ensure_shader_resource_capacities(
    current: ShaderResourceCapacities,
    prepared_scene: &PreparedRenderScene,
) -> ShaderResourceCapacities {
    let required_curve_capacity = prepared_scene.curve_data.len();
    let required_band_capacity = prepared_scene.band_data.len();

    if required_curve_capacity <= current.curve_capacity
        && required_band_capacity <= current.band_capacity
    {
        return current;
    }

    ShaderResourceCapacities {
        curve_capacity: required_curve_capacity
            .max(current.curve_capacity.saturating_mul(2))
            .max(DEFAULT_CURVE_FLOAT4_CAPACITY),
        band_capacity: required_band_capacity
            .max(current.band_capacity.saturating_mul(2))
            .max(DEFAULT_BAND_UINT_CAPACITY),
    }
}

#[must_use]
pub fn padded_curve_upload_data(
    prepared_scene: &PreparedRenderScene,
    capacities: ShaderResourceCapacities,
) -> Vec<[f32; 4]> {
    let mut padded = vec![[0.0; 4]; capacities.curve_capacity.max(prepared_scene.curve_data.len())];
    padded[..prepared_scene.curve_data.len()].copy_from_slice(&prepared_scene.curve_data);
    padded
}

#[must_use]
pub fn padded_band_upload_data(
    prepared_scene: &PreparedRenderScene,
    capacities: ShaderResourceCapacities,
) -> Vec<u32> {
    let mut padded = vec![0; capacities.band_capacity.max(prepared_scene.band_data.len())];
    padded[..prepared_scene.band_data.len()].copy_from_slice(&prepared_scene.band_data);
    padded
}

#[must_use]
pub fn vertex_byte_range(range: Range<usize>) -> Range<usize> {
    let stride = size_of::<SceneVertex>();
    range.start.saturating_mul(stride)..range.end.saturating_mul(stride)
}

#[cfg(test)]
mod tests {
    use crate::{GlyphQuad, RenderScene, prepare_render_scene};
    use windows::Win32::Foundation::RECT;

    use super::{
        DEFAULT_BAND_UINT_CAPACITY, DEFAULT_CURVE_FLOAT4_CAPACITY, ShaderResourceCapacities,
        ensure_shader_resource_capacities, padded_band_upload_data, padded_curve_upload_data,
        vertex_byte_range,
    };

    fn prepared_scene() -> eyre::Result<crate::PreparedRenderScene> {
        prepare_render_scene(&RenderScene {
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
        })
    }

    #[test]
    fn ensure_shader_resource_capacities_keeps_existing_when_sufficient() -> eyre::Result<()> {
        let prepared = prepared_scene()?;
        let current = ShaderResourceCapacities::default();

        let next = ensure_shader_resource_capacities(current, &prepared);

        assert_eq!(next, current);
        Ok(())
    }

    #[test]
    fn ensure_shader_resource_capacities_grows_by_required_or_doubled_capacity() {
        let prepared = crate::PreparedRenderScene {
            font: std::sync::Arc::new(teamy_studio_fonts::LoadedTerminalFont {
                font_bytes: Vec::new(),
                face_index: 0,
                units_per_em: 1.0,
                ascender: 1.0,
                descender: 0.0,
                cell_advance: 1.0,
            }),
            vertices: Vec::new(),
            curve_data: vec![[0.0; 4]; DEFAULT_CURVE_FLOAT4_CAPACITY + 1],
            band_data: vec![0; DEFAULT_BAND_UINT_CAPACITY + 1],
        };

        let next = ensure_shader_resource_capacities(ShaderResourceCapacities::default(), &prepared);

        assert_eq!(next.curve_capacity, DEFAULT_CURVE_FLOAT4_CAPACITY * 2);
        assert_eq!(next.band_capacity, DEFAULT_BAND_UINT_CAPACITY * 2);
    }

    #[test]
    fn padded_upload_data_zero_fills_remaining_capacity() -> eyre::Result<()> {
        let prepared = prepared_scene()?;
        let capacities = ShaderResourceCapacities {
            curve_capacity: prepared.curve_data.len() + 4,
            band_capacity: prepared.band_data.len() + 4,
        };

        let curves = padded_curve_upload_data(&prepared, capacities);
        let bands = padded_band_upload_data(&prepared, capacities);

        assert_eq!(curves.len(), prepared.curve_data.len() + 4);
        assert_eq!(bands.len(), prepared.band_data.len() + 4);
        assert_eq!(curves[prepared.curve_data.len()], [0.0; 4]);
        assert_eq!(bands[prepared.band_data.len()], 0);
        Ok(())
    }

    #[test]
    fn vertex_byte_range_scales_by_vertex_stride() {
        let byte_range = vertex_byte_range(2..5);
        let stride = size_of::<crate::SceneVertex>();

        assert_eq!(byte_range, 2 * stride..5 * stride);
    }
}
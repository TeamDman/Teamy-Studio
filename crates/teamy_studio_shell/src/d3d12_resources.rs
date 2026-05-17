use std::mem::size_of;

use windows::Win32::Graphics::Direct3D12::{
    D3D12_HEAP_FLAG_NONE, D3D12_HEAP_PROPERTIES, D3D12_HEAP_TYPE_UPLOAD,
    D3D12_RESOURCE_DESC, D3D12_RESOURCE_DIMENSION_BUFFER, D3D12_RESOURCE_STATE_GENERIC_READ,
    D3D12_TEXTURE_LAYOUT_ROW_MAJOR, D3D12_VERTEX_BUFFER_VIEW, ID3D12Device, ID3D12Resource,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_SAMPLE_DESC;

use crate::d3d12_sprite_atlas::{sprite_atlas_pixels, sprite_buffer_size_bytes};
use crate::{SceneVertex, ShaderResourceCapacities};

const MAX_PANEL_COUNT: usize = 8_192;
const MAX_GLYPH_COUNT: usize = 8_192;
const MAX_SPRITE_COUNT: usize = 256;
const MAX_VERTEX_COUNT: usize = (MAX_PANEL_COUNT + MAX_GLYPH_COUNT + MAX_SPRITE_COUNT) * 6;
const MAX_TRANSFORMED_GLYPH_INVERSE_FLOAT4_COUNT: usize = MAX_GLYPH_COUNT * 2;
const SHADER_PARAM_BUFFER_SIZE_BYTES: u64 = 256;
pub struct SceneUploadResources {
    pub curve_buffer: ID3D12Resource,
    pub band_buffer: ID3D12Resource,
    pub sprite_buffer: ID3D12Resource,
    pub transformed_glyph_inverse_buffer: ID3D12Resource,
}

#[must_use]
pub fn scene_vertex_buffer_size_bytes() -> u64 {
    (size_of::<SceneVertex>() * MAX_VERTEX_COUNT) as u64
}

#[must_use]
pub fn curve_buffer_size_bytes(curve_capacity: usize) -> u64 {
    (curve_capacity * size_of::<[f32; 4]>()) as u64
}

#[must_use]
pub fn band_buffer_size_bytes(band_capacity: usize) -> u64 {
    (band_capacity * size_of::<u32>()) as u64
}

#[must_use]
pub fn transformed_glyph_inverse_buffer_size_bytes() -> u64 {
    (MAX_TRANSFORMED_GLYPH_INVERSE_FLOAT4_COUNT * size_of::<[f32; 4]>()) as u64
}

pub fn create_scene_vertex_buffer(
    device: &ID3D12Device,
) -> windows::core::Result<(ID3D12Resource, D3D12_VERTEX_BUFFER_VIEW)> {
    let buffer_size = scene_vertex_buffer_size_bytes();
    let vertex_buffer = create_upload_buffer(device, buffer_size)?;

    Ok((
        vertex_buffer.clone(),
        D3D12_VERTEX_BUFFER_VIEW {
            BufferLocation: unsafe { vertex_buffer.GetGPUVirtualAddress() },
            SizeInBytes: u32::try_from(buffer_size).unwrap_or(u32::MAX),
            StrideInBytes: u32::try_from(size_of::<SceneVertex>()).unwrap_or(u32::MAX),
        },
    ))
}

pub fn create_shader_param_buffer(device: &ID3D12Device) -> windows::core::Result<ID3D12Resource> {
    create_upload_buffer(device, SHADER_PARAM_BUFFER_SIZE_BYTES)
}

pub fn create_scene_upload_buffers(
    device: &ID3D12Device,
    capacities: ShaderResourceCapacities,
) -> windows::core::Result<SceneUploadResources> {
    let sprite_buffer = create_upload_buffer(device, sprite_buffer_size_bytes())?;
    let sprite_pixels = sprite_atlas_pixels();
    unsafe {
        let mut mapped = std::ptr::null_mut();
        sprite_buffer.Map(0, None, Some(&mut mapped))?;
        std::ptr::copy_nonoverlapping(sprite_pixels.as_ptr(), mapped as *mut u32, sprite_pixels.len());
        sprite_buffer.Unmap(0, None);
    }

    Ok(SceneUploadResources {
        curve_buffer: create_upload_buffer(device, curve_buffer_size_bytes(capacities.curve_capacity))?,
        band_buffer: create_upload_buffer(device, band_buffer_size_bytes(capacities.band_capacity))?,
        sprite_buffer,
        transformed_glyph_inverse_buffer: create_upload_buffer(
            device,
            transformed_glyph_inverse_buffer_size_bytes(),
        )?,
    })
}

fn create_upload_buffer(device: &ID3D12Device, byte_len: u64) -> windows::core::Result<ID3D12Resource> {
    let mut resource = None;
    unsafe {
        device.CreateCommittedResource(
            &D3D12_HEAP_PROPERTIES {
                Type: D3D12_HEAP_TYPE_UPLOAD,
                ..Default::default()
            },
            D3D12_HEAP_FLAG_NONE,
            &D3D12_RESOURCE_DESC {
                Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
                Width: byte_len,
                Height: 1,
                DepthOrArraySize: 1,
                MipLevels: 1,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
                ..Default::default()
            },
            D3D12_RESOURCE_STATE_GENERIC_READ,
            None,
            &mut resource,
        )?;
    }

    Ok(resource.expect("upload buffer should be initialized"))
}

#[cfg(test)]
mod tests {
    use std::mem::size_of;

    use super::{
        band_buffer_size_bytes, curve_buffer_size_bytes, scene_vertex_buffer_size_bytes,
        sprite_buffer_size_bytes,
        transformed_glyph_inverse_buffer_size_bytes,
    };
    use crate::SceneVertex;

    #[test]
    fn scene_vertex_buffer_size_matches_max_vertex_capacity() {
        let expected = ((8_192 + 8_192 + 256) * 6 * size_of::<SceneVertex>()) as u64;
        assert_eq!(scene_vertex_buffer_size_bytes(), expected);
    }

    #[test]
    fn shader_upload_buffer_sizes_scale_with_element_counts() {
        assert_eq!(curve_buffer_size_bytes(7), (7 * size_of::<[f32; 4]>()) as u64);
        assert_eq!(band_buffer_size_bytes(9), (9 * size_of::<u32>()) as u64);
    }

    #[test]
    fn transformed_glyph_inverse_buffer_size_tracks_double_glyph_capacity() {
        let expected = (8_192 * 2 * size_of::<[f32; 4]>()) as u64;
        assert_eq!(transformed_glyph_inverse_buffer_size_bytes(), expected);
    }

    #[test]
    fn sprite_buffer_size_tracks_real_sprite_atlas() {
        assert!(sprite_buffer_size_bytes() > size_of::<u32>() as u64);
    }
}
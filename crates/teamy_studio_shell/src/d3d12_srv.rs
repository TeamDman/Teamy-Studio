use windows::Win32::Graphics::Direct3D12::{
    D3D12_BUFFER_SRV, D3D12_CPU_DESCRIPTOR_HANDLE, D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
    D3D12_DESCRIPTOR_HEAP_DESC, D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,
    D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV, D3D12_SHADER_RESOURCE_VIEW_DESC,
    D3D12_SHADER_RESOURCE_VIEW_DESC_0, D3D12_SRV_DIMENSION_BUFFER, ID3D12DescriptorHeap,
    ID3D12Device,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_R32G32B32A32_FLOAT, DXGI_FORMAT_R32_UINT};

use crate::{
    SceneUploadResources, ShaderResourceCapacities, create_scene_upload_buffers,
    transformed_glyph_inverse_buffer_size_bytes,
};
use crate::d3d12_sprite_atlas::sprite_buffer_size_bytes;

const TEXT_SHADER_SRV_DESCRIPTOR_COUNT: u32 = 4;

pub struct TextShaderResourceSet {
    pub srv_heap: ID3D12DescriptorHeap,
    pub descriptor_size: u32,
    pub resources: SceneUploadResources,
    pub capacities: ShaderResourceCapacities,
}

pub fn create_text_shader_resource_set(
    device: &ID3D12Device,
    capacities: ShaderResourceCapacities,
) -> windows::core::Result<TextShaderResourceSet> {
    let resources = create_scene_upload_buffers(device, capacities)?;
    let srv_heap: ID3D12DescriptorHeap = unsafe {
        device.CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC {
            Type: D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,
            NumDescriptors: TEXT_SHADER_SRV_DESCRIPTOR_COUNT,
            Flags: D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,
            ..Default::default()
        })?
    };
    let descriptor_size = unsafe {
        device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV)
    };

    let curve_desc = D3D12_SHADER_RESOURCE_VIEW_DESC {
        Format: DXGI_FORMAT_R32G32B32A32_FLOAT,
        ViewDimension: D3D12_SRV_DIMENSION_BUFFER,
        Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
        Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
            Buffer: D3D12_BUFFER_SRV {
                FirstElement: 0,
                NumElements: u32::try_from(capacities.curve_capacity).unwrap_or(u32::MAX),
                StructureByteStride: 0,
                Flags: Default::default(),
            },
        },
    };

    let band_desc = D3D12_SHADER_RESOURCE_VIEW_DESC {
        Format: DXGI_FORMAT_R32_UINT,
        ViewDimension: D3D12_SRV_DIMENSION_BUFFER,
        Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
        Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
            Buffer: D3D12_BUFFER_SRV {
                FirstElement: 0,
                NumElements: u32::try_from(capacities.band_capacity).unwrap_or(u32::MAX),
                StructureByteStride: 0,
                Flags: Default::default(),
            },
        },
    };

    let transformed_inverse_desc = D3D12_SHADER_RESOURCE_VIEW_DESC {
        Format: DXGI_FORMAT_R32G32B32A32_FLOAT,
        ViewDimension: D3D12_SRV_DIMENSION_BUFFER,
        Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
        Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
            Buffer: D3D12_BUFFER_SRV {
                FirstElement: 0,
                NumElements: u32::try_from(transformed_glyph_inverse_buffer_size_bytes() / 16)
                    .unwrap_or(u32::MAX),
                StructureByteStride: 0,
                Flags: Default::default(),
            },
        },
    };

    let sprite_desc = D3D12_SHADER_RESOURCE_VIEW_DESC {
        Format: DXGI_FORMAT_R32_UINT,
        ViewDimension: D3D12_SRV_DIMENSION_BUFFER,
        Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
        Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
            Buffer: D3D12_BUFFER_SRV {
                FirstElement: 0,
                NumElements: u32::try_from(sprite_buffer_size_bytes() / 4).unwrap_or(u32::MAX),
                StructureByteStride: 0,
                Flags: Default::default(),
            },
        },
    };

    unsafe {
        let heap_start = srv_heap.GetCPUDescriptorHandleForHeapStart();
        device.CreateShaderResourceView(&resources.curve_buffer, Some(&curve_desc), heap_start);
        device.CreateShaderResourceView(
            &resources.band_buffer,
            Some(&band_desc),
            D3D12_CPU_DESCRIPTOR_HANDLE {
                ptr: heap_start.ptr + descriptor_size as usize,
            },
        );
        device.CreateShaderResourceView(
            &resources.sprite_buffer,
            Some(&sprite_desc),
            D3D12_CPU_DESCRIPTOR_HANDLE {
                ptr: heap_start.ptr + (descriptor_size as usize * 2),
            },
        );
        device.CreateShaderResourceView(
            &resources.transformed_glyph_inverse_buffer,
            Some(&transformed_inverse_desc),
            D3D12_CPU_DESCRIPTOR_HANDLE {
                ptr: heap_start.ptr + (descriptor_size as usize * 3),
            },
        );
    }

    Ok(TextShaderResourceSet {
        srv_heap,
        descriptor_size,
        resources,
        capacities,
    })
}

#[cfg(test)]
mod tests {
    use super::TEXT_SHADER_SRV_DESCRIPTOR_COUNT;

    #[test]
    fn text_shader_resource_heap_tracks_text_only_descriptor_count() {
        assert_eq!(TEXT_SHADER_SRV_DESCRIPTOR_COUNT, 4);
    }
}
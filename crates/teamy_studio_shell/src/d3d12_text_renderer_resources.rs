use windows::Win32::Graphics::Direct3D12::{D3D12_VERTEX_BUFFER_VIEW, ID3D12Device, ID3D12Resource};

use crate::{
    ShaderResourceCapacities, TextShaderResourceSet, create_scene_vertex_buffer,
    create_shader_param_buffer, create_text_shader_resource_set,
};

pub struct TextRendererResources {
    pub vertex_buffer: ID3D12Resource,
    pub vertex_buffer_view: D3D12_VERTEX_BUFFER_VIEW,
    pub shader_param_buffer: ID3D12Resource,
    pub text_shader_resources: TextShaderResourceSet,
}

pub fn create_text_renderer_resources(
    device: &ID3D12Device,
    capacities: ShaderResourceCapacities,
) -> windows::core::Result<TextRendererResources> {
    let (vertex_buffer, vertex_buffer_view) = create_scene_vertex_buffer(device)?;
    let shader_param_buffer = create_shader_param_buffer(device)?;
    let text_shader_resources = create_text_shader_resource_set(device, capacities)?;

    Ok(TextRendererResources {
        vertex_buffer,
        vertex_buffer_view,
        shader_param_buffer,
        text_shader_resources,
    })
}

#[cfg(test)]
mod tests {
    use std::mem::size_of;

    use super::TextRendererResources;

    #[test]
    fn text_renderer_resources_layout_stays_bootstrap_friendly() {
        assert!(size_of::<TextRendererResources>() > 0);
    }
}
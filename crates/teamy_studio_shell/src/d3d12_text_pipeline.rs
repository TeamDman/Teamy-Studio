use std::ffi::c_void;

use windows::Win32::Foundation::TRUE;
use windows::Win32::Graphics::Direct3D::{D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST, ID3DBlob};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_R32_FLOAT, DXGI_FORMAT_R32G32_FLOAT,
    DXGI_FORMAT_R32G32B32_FLOAT, DXGI_FORMAT_R32G32B32A32_FLOAT, DXGI_FORMAT_UNKNOWN,
    DXGI_SAMPLE_DESC,
};
use windows::core::s;

use crate::d3d12_sprite_atlas::sprite_atlas_dimensions;

const COMPILED_VERTEX_SHADER: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/windows_panel_vs.cso"));
const COMPILED_PIXEL_SHADER: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/windows_panel_ps.cso"));

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct TextShaderParams {
    pub slug_matrix: [[f32; 4]; 4],
    pub slug_viewport: [f32; 4],
    pub scene_time: [f32; 4],
    pub transformed_text_clip_rect: [f32; 4],
    pub transformed_text_debug_hover: [f32; 4],
    pub transformed_text_projection: [[f32; 4]; 2],
    pub transformed_text_inverse_homography: [[f32; 4]; 2],
    pub sprite_atlas: [f32; 4],
}

pub fn build_text_shader_params(width: f32, height: f32, elapsed_seconds: f32) -> TextShaderParams {
    let safe_width = width.max(1.0);
    let safe_height = height.max(1.0);
    let (sprite_atlas_width, sprite_atlas_height) = sprite_atlas_dimensions();
    TextShaderParams {
        slug_matrix: [
            [2.0 / safe_width, 0.0, 0.0, -1.0],
            [0.0, -2.0 / safe_height, 0.0, 1.0],
            [0.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
        slug_viewport: [safe_width, safe_height, 0.0, 0.0],
        scene_time: [elapsed_seconds, 0.0, 0.0, 0.0],
        transformed_text_clip_rect: [-1.0, -1.0, -1.0, -1.0],
        transformed_text_debug_hover: [0.0; 4],
        transformed_text_projection: [[0.0; 4]; 2],
        transformed_text_inverse_homography: [[0.0; 4]; 2],
        sprite_atlas: [
            sprite_atlas_width as f32,
            sprite_atlas_height as f32,
            0.0,
            0.0,
        ],
    }
}

pub fn update_text_shader_params(
    shader_param_buffer: &ID3D12Resource,
    width: u32,
    height: u32,
    elapsed_seconds: f32,
) -> eyre::Result<()> {
    let params = build_text_shader_params(width as f32, height as f32, elapsed_seconds);
    unsafe {
        let mut mapped = std::ptr::null_mut();
        shader_param_buffer.Map(0, None, Some(&mut mapped))?;
        std::ptr::copy_nonoverlapping(&params, mapped as *mut TextShaderParams, 1);
        shader_param_buffer.Unmap(0, None);
    }
    Ok(())
}

pub fn create_text_root_signature(device: &ID3D12Device) -> eyre::Result<ID3D12RootSignature> {
    let descriptor_ranges = [D3D12_DESCRIPTOR_RANGE {
        RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_SRV,
        NumDescriptors: 4,
        BaseShaderRegister: 0,
        RegisterSpace: 0,
        OffsetInDescriptorsFromTableStart: D3D12_DESCRIPTOR_RANGE_OFFSET_APPEND,
    }];
    let root_parameters = [
        D3D12_ROOT_PARAMETER {
            ParameterType: D3D12_ROOT_PARAMETER_TYPE_CBV,
            Anonymous: D3D12_ROOT_PARAMETER_0 {
                Descriptor: D3D12_ROOT_DESCRIPTOR {
                    ShaderRegister: 0,
                    RegisterSpace: 0,
                },
            },
            ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
        },
        D3D12_ROOT_PARAMETER {
            ParameterType: D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE,
            Anonymous: D3D12_ROOT_PARAMETER_0 {
                DescriptorTable: D3D12_ROOT_DESCRIPTOR_TABLE {
                    NumDescriptorRanges: 1,
                    pDescriptorRanges: descriptor_ranges.as_ptr(),
                },
            },
            ShaderVisibility: D3D12_SHADER_VISIBILITY_PIXEL,
        },
    ];
    let description = D3D12_ROOT_SIGNATURE_DESC {
        NumParameters: root_parameters.len() as u32,
        pParameters: root_parameters.as_ptr(),
        Flags: D3D12_ROOT_SIGNATURE_FLAG_ALLOW_INPUT_ASSEMBLER_INPUT_LAYOUT,
        ..Default::default()
    };

    let mut signature = None;
    let mut error = None;
    unsafe {
        D3D12SerializeRootSignature(
            &description,
            D3D_ROOT_SIGNATURE_VERSION_1,
            &mut signature,
            Some(&mut error),
        )
    }
    .map_err(|err| shader_error(err, error).wrap_err("failed to serialize root signature"))?;

    let signature = signature.expect("root signature blob should be initialized");
    Ok(unsafe {
        device.CreateRootSignature(
            0,
            std::slice::from_raw_parts(
                signature.GetBufferPointer() as *const u8,
                signature.GetBufferSize(),
            ),
        )?
    })
}

pub fn create_text_pipeline_state(
    device: &ID3D12Device,
    root_signature: &ID3D12RootSignature,
) -> eyre::Result<ID3D12PipelineState> {
    let input_layout = [
        D3D12_INPUT_ELEMENT_DESC {
            SemanticName: s!("POSITION"),
            Format: DXGI_FORMAT_R32G32B32_FLOAT,
            ..Default::default()
        },
        D3D12_INPUT_ELEMENT_DESC {
            SemanticName: s!("COLOR"),
            Format: DXGI_FORMAT_R32G32B32A32_FLOAT,
            AlignedByteOffset: 12,
            ..Default::default()
        },
        D3D12_INPUT_ELEMENT_DESC {
            SemanticName: s!("TEXCOORD"),
            Format: DXGI_FORMAT_R32G32_FLOAT,
            AlignedByteOffset: 28,
            ..Default::default()
        },
        D3D12_INPUT_ELEMENT_DESC {
            SemanticName: s!("EFFECT"),
            Format: DXGI_FORMAT_R32_FLOAT,
            AlignedByteOffset: 36,
            ..Default::default()
        },
        D3D12_INPUT_ELEMENT_DESC {
            SemanticName: s!("GLYPH"),
            Format: DXGI_FORMAT_R32_FLOAT,
            AlignedByteOffset: 40,
            ..Default::default()
        },
        D3D12_INPUT_ELEMENT_DESC {
            SemanticName: s!("GLYPHDATA"),
            Format: DXGI_FORMAT_R32G32B32A32_FLOAT,
            AlignedByteOffset: 44,
            ..Default::default()
        },
        D3D12_INPUT_ELEMENT_DESC {
            SemanticName: s!("BANDING"),
            Format: DXGI_FORMAT_R32G32B32A32_FLOAT,
            AlignedByteOffset: 60,
            ..Default::default()
        },
        D3D12_INPUT_ELEMENT_DESC {
            SemanticName: s!("NORMAL"),
            Format: DXGI_FORMAT_R32G32_FLOAT,
            AlignedByteOffset: 76,
            ..Default::default()
        },
        D3D12_INPUT_ELEMENT_DESC {
            SemanticName: s!("JACOBIAN"),
            Format: DXGI_FORMAT_R32G32B32A32_FLOAT,
            AlignedByteOffset: 84,
            ..Default::default()
        },
        D3D12_INPUT_ELEMENT_DESC {
            SemanticName: s!("LOCALBOUNDS"),
            Format: DXGI_FORMAT_R32G32B32A32_FLOAT,
            AlignedByteOffset: 100,
            ..Default::default()
        },
        D3D12_INPUT_ELEMENT_DESC {
            SemanticName: s!("VIEWPORT"),
            Format: DXGI_FORMAT_R32G32_FLOAT,
            AlignedByteOffset: 116,
            ..Default::default()
        },
    ];

    let blend_target = D3D12_RENDER_TARGET_BLEND_DESC {
        BlendEnable: TRUE,
        LogicOpEnable: false.into(),
        SrcBlend: D3D12_BLEND_ONE,
        DestBlend: D3D12_BLEND_INV_SRC_ALPHA,
        BlendOp: D3D12_BLEND_OP_ADD,
        SrcBlendAlpha: D3D12_BLEND_ONE,
        DestBlendAlpha: D3D12_BLEND_INV_SRC_ALPHA,
        BlendOpAlpha: D3D12_BLEND_OP_ADD,
        LogicOp: D3D12_LOGIC_OP_NOOP,
        RenderTargetWriteMask: D3D12_COLOR_WRITE_ENABLE_ALL.0 as u8,
    };

    let description = D3D12_GRAPHICS_PIPELINE_STATE_DESC {
        pRootSignature: std::mem::ManuallyDrop::new(Some(root_signature.clone())),
        VS: shader_bytecode_slice(COMPILED_VERTEX_SHADER),
        PS: shader_bytecode_slice(COMPILED_PIXEL_SHADER),
        BlendState: D3D12_BLEND_DESC {
            AlphaToCoverageEnable: false.into(),
            IndependentBlendEnable: false.into(),
            RenderTarget: [blend_target; 8],
        },
        SampleMask: u32::MAX,
        RasterizerState: D3D12_RASTERIZER_DESC {
            FillMode: D3D12_FILL_MODE_SOLID,
            CullMode: D3D12_CULL_MODE_NONE,
            FrontCounterClockwise: false.into(),
            DepthBias: D3D12_DEFAULT_DEPTH_BIAS,
            DepthBiasClamp: D3D12_DEFAULT_DEPTH_BIAS_CLAMP,
            SlopeScaledDepthBias: D3D12_DEFAULT_SLOPE_SCALED_DEPTH_BIAS,
            DepthClipEnable: TRUE,
            MultisampleEnable: false.into(),
            AntialiasedLineEnable: false.into(),
            ForcedSampleCount: 0,
            ConservativeRaster: D3D12_CONSERVATIVE_RASTERIZATION_MODE_OFF,
        },
        DepthStencilState: D3D12_DEPTH_STENCIL_DESC {
            DepthEnable: false.into(),
            StencilEnable: false.into(),
            ..Default::default()
        },
        InputLayout: D3D12_INPUT_LAYOUT_DESC {
            pInputElementDescs: input_layout.as_ptr(),
            NumElements: input_layout.len() as u32,
        },
        PrimitiveTopologyType: D3D12_PRIMITIVE_TOPOLOGY_TYPE_TRIANGLE,
        NumRenderTargets: 1,
        RTVFormats: [
            DXGI_FORMAT_B8G8R8A8_UNORM,
            DXGI_FORMAT_UNKNOWN,
            DXGI_FORMAT_UNKNOWN,
            DXGI_FORMAT_UNKNOWN,
            DXGI_FORMAT_UNKNOWN,
            DXGI_FORMAT_UNKNOWN,
            DXGI_FORMAT_UNKNOWN,
            DXGI_FORMAT_UNKNOWN,
        ],
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        ..Default::default()
    };

    Ok(unsafe { device.CreateGraphicsPipelineState(&description) }?)
}

#[must_use]
pub const fn primitive_topology_triangle_list()
-> windows::Win32::Graphics::Direct3D::D3D_PRIMITIVE_TOPOLOGY {
    D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST
}

fn shader_error(error: windows::core::Error, blob: Option<ID3DBlob>) -> eyre::Error {
    if let Some(blob) = blob {
        let bytes = unsafe {
            std::slice::from_raw_parts(blob.GetBufferPointer() as *const u8, blob.GetBufferSize())
        };
        eyre::eyre!("{error}: {}", String::from_utf8_lossy(bytes).trim())
    } else {
        error.into()
    }
}

fn shader_bytecode_slice(shader: &[u8]) -> D3D12_SHADER_BYTECODE {
    D3D12_SHADER_BYTECODE {
        pShaderBytecode: shader.as_ptr() as *const c_void,
        BytecodeLength: shader.len(),
    }
}

#[cfg(test)]
mod tests {
    use crate::d3d12_sprite_atlas::sprite_atlas_dimensions;

    use super::{build_text_shader_params, primitive_topology_triangle_list};

    #[test]
    fn build_text_shader_params_sets_pixel_to_ndc_matrix() {
        let params = build_text_shader_params(200.0, 100.0, 0.0);
        let (sprite_atlas_width, sprite_atlas_height) = sprite_atlas_dimensions();
        assert_eq!(params.slug_matrix[0][0], 0.01);
        assert_eq!(params.slug_matrix[1][1], -0.02);
        assert_eq!(params.sprite_atlas[0], sprite_atlas_width as f32);
        assert_eq!(params.sprite_atlas[1], sprite_atlas_height as f32);
    }

    #[test]
    fn primitive_topology_is_triangle_list() {
        assert_eq!(primitive_topology_triangle_list().0, 4);
    }
}

use std::time::Instant;

use eyre::WrapErr;
use windows::Win32::Foundation::{HANDLE, HWND, RECT};
use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_0;
use windows::Win32::Graphics::Direct3D12::{
    D3D12_COMMAND_LIST_TYPE_DIRECT, D3D12_COMMAND_QUEUE_DESC, D3D12_FENCE_FLAG_NONE,
    D3D12_RESOURCE_BARRIER, D3D12_RESOURCE_BARRIER_0,
    D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES, D3D12_RESOURCE_BARRIER_FLAG_NONE,
    D3D12_RESOURCE_BARRIER_TYPE_TRANSITION, D3D12_RESOURCE_STATE_PRESENT,
    D3D12_RESOURCE_STATE_RENDER_TARGET, D3D12_RESOURCE_STATES,
    D3D12_RESOURCE_TRANSITION_BARRIER, D3D12CreateDevice, D3D12_CPU_DESCRIPTOR_HANDLE,
    D3D12_DESCRIPTOR_HEAP_DESC, D3D12_DESCRIPTOR_HEAP_TYPE_RTV, ID3D12CommandAllocator,
    ID3D12CommandList, ID3D12CommandQueue, ID3D12DescriptorHeap, ID3D12Device, ID3D12Fence,
    ID3D12GraphicsCommandList, ID3D12PipelineState, ID3D12Resource, ID3D12RootSignature,
    D3D12_VIEWPORT,
};
use windows::Win32::Graphics::DirectComposition::{
    DCompositionCreateDevice, IDCompositionDevice, IDCompositionTarget, IDCompositionVisual,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory2, DXGI_ADAPTER_FLAG, DXGI_ADAPTER_FLAG_NONE, DXGI_ADAPTER_FLAG_SOFTWARE,
    DXGI_CREATE_FACTORY_FLAGS, DXGI_ERROR_NOT_FOUND, DXGI_MWA_NO_ALT_ENTER, DXGI_PRESENT,
    DXGI_SCALING_STRETCH, DXGI_SWAP_CHAIN_DESC1,
    DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT, DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
    DXGI_USAGE_RENDER_TARGET_OUTPUT, IDXGIAdapter, IDXGIAdapter1, IDXGIDevice, IDXGIFactory2,
    IDXGIFactory4, IDXGISwapChain1, IDXGISwapChain3,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC,
};
use windows::Win32::System::Threading::{CreateEventW, INFINITE, WaitForSingleObjectEx};
use windows::Win32::UI::WindowsAndMessaging::GetClientRect;
use windows::core::Interface;
use windows::core::Owned;

use crate::{
    PreparedSceneUploadBatch, RenderScene, ShaderResourceCapacities, TextRendererResources,
    build_prepared_scene_upload_batch, create_text_renderer_resources, prepare_render_scene,
    upload_band_data, upload_curve_data, upload_vertex_ranges,
};
use crate::d3d12_text_pipeline::{
    create_text_pipeline_state, create_text_root_signature, primitive_topology_triangle_list,
    update_text_shader_params,
};

const FRAME_COUNT: usize = 2;

pub struct TextRendererHost {
    pub hwnd: HWND,
    pub dxgi_factory: IDXGIFactory4,
    pub device: ID3D12Device,
    pub dcomp_device: IDCompositionDevice,
    pub dcomp_target: IDCompositionTarget,
    pub dcomp_visual: IDCompositionVisual,
    pub command_queue: ID3D12CommandQueue,
    pub command_allocator: ID3D12CommandAllocator,
    pub command_list: ID3D12GraphicsCommandList,
    pub swap_chain: IDXGISwapChain3,
    pub rtv_heap: ID3D12DescriptorHeap,
    pub rtv_descriptor_size: u32,
    pub render_targets: [Option<ID3D12Resource>; FRAME_COUNT],
    pub fence: ID3D12Fence,
    pub fence_event: Owned<HANDLE>,
    pub next_fence_value: u64,
    pub root_signature: ID3D12RootSignature,
    pub pipeline_state: ID3D12PipelineState,
    pub start_time: Instant,
    pub viewport: D3D12_VIEWPORT,
    pub scissor_rect: RECT,
    pub width: u32,
    pub height: u32,
    pub resources: TextRendererResources,
    pub capacities: ShaderResourceCapacities,
    pub last_upload_batch: PreparedSceneUploadBatch,
}

pub fn create_text_renderer_device() -> eyre::Result<(IDXGIFactory4, ID3D12Device)> {
    let dxgi_factory: IDXGIFactory4 = unsafe { CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)) }
        .wrap_err("failed to create DXGI factory")?;
    let adapter = get_hardware_or_warp_adapter(&dxgi_factory)?;
    let mut device = None;
    unsafe { D3D12CreateDevice(&adapter, D3D_FEATURE_LEVEL_11_0, &mut device) }
        .wrap_err("failed to create D3D12 device from DXGI adapter")?;
    Ok((
        dxgi_factory,
        device.expect("device should be initialized after D3D12CreateDevice succeeds"),
    ))
}

impl TextRendererHost {
    pub fn new(hwnd: HWND, scene: &RenderScene) -> eyre::Result<Self> {
        let (dxgi_factory, device) = create_text_renderer_device()?;
        let command_queue = create_command_queue(&device)
            .wrap_err("failed to create D3D12 command queue")?;
        let command_allocator: ID3D12CommandAllocator = unsafe {
            device.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT)
        }
        .wrap_err("failed to create D3D12 command allocator")?;
        let command_list: ID3D12GraphicsCommandList = unsafe {
            device.CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &command_allocator, None)
        }
        .wrap_err("failed to create D3D12 command list")?;
        unsafe { command_list.Close() }.wrap_err("failed to close D3D12 command list")?;
        let (width, height) = client_size(hwnd)?;
        let swap_chain =
            create_composition_swap_chain(&dxgi_factory, &command_queue, width, height)
                .wrap_err("failed to create composition swap chain for text renderer host")?;
        unsafe { dxgi_factory.MakeWindowAssociation(hwnd, DXGI_MWA_NO_ALT_ENTER) }
            .wrap_err("failed to configure DXGI window association for text renderer host")?;
        let (dcomp_device, dcomp_target, dcomp_visual) =
            attach_swap_chain_to_window(hwnd, &swap_chain)
                .wrap_err("failed to attach composition swap chain to text renderer host window")?;
        unsafe { swap_chain.SetMaximumFrameLatency(1) }
            .wrap_err("failed to set text renderer host maximum frame latency")?;
        let (rtv_heap, rtv_descriptor_size, render_targets) =
            create_render_targets(&device, &swap_chain)
                .wrap_err("failed to create render targets for text renderer host")?;
        let fence: ID3D12Fence = unsafe { device.CreateFence(0, D3D12_FENCE_FLAG_NONE) }
            .wrap_err("failed to create fence for text renderer host")?;
        let fence_event = unsafe { Owned::new(CreateEventW(None, false, false, None)?) };
        let root_signature = create_text_root_signature(&device)
            .wrap_err("failed to create text renderer root signature")?;
        let pipeline_state = create_text_pipeline_state(&device, &root_signature)
            .wrap_err("failed to create text renderer pipeline state")?;

        let prepared_scene = prepare_render_scene(scene)
            .wrap_err("failed to prepare initial render scene for text renderer host")?;
        let upload_batch = build_prepared_scene_upload_batch(
            &prepared_scene,
            ShaderResourceCapacities::default(),
        );
        let resources = create_text_renderer_resources(&device, upload_batch.capacities)
            .wrap_err("failed to create text renderer resources")?;

        let mut host = Self {
            hwnd,
            dxgi_factory,
            device,
            dcomp_device,
            dcomp_target,
            dcomp_visual,
            command_queue,
            command_allocator,
            command_list,
            swap_chain,
            rtv_heap,
            rtv_descriptor_size,
            render_targets,
            fence,
            fence_event,
            next_fence_value: 1,
            root_signature,
            pipeline_state,
            start_time: Instant::now(),
            viewport: D3D12_VIEWPORT {
                TopLeftX: 0.0,
                TopLeftY: 0.0,
                Width: width as f32,
                Height: height as f32,
                MinDepth: 0.0,
                MaxDepth: 1.0,
            },
            scissor_rect: RECT {
                left: 0,
                top: 0,
                right: i32::try_from(width).unwrap_or(i32::MAX),
                bottom: i32::try_from(height).unwrap_or(i32::MAX),
            },
            width,
            height,
            resources,
            capacities: upload_batch.capacities,
            last_upload_batch: upload_batch,
        };
        host.upload_last_batch()?;
        Ok(host)
    }

    pub fn upload_scene(&mut self, scene: &RenderScene) -> eyre::Result<()> {
        let prepared_scene = prepare_render_scene(scene)
            .wrap_err("failed to prepare render scene for text renderer host upload")?;
        let upload_batch = build_prepared_scene_upload_batch(&prepared_scene, self.capacities);
        if upload_batch.capacities != self.capacities {
            self.resources = create_text_renderer_resources(&self.device, upload_batch.capacities)
                .wrap_err("failed to grow text renderer resources")?;
            self.capacities = upload_batch.capacities;
        }
        self.last_upload_batch = upload_batch;
        self.upload_last_batch()
    }

    pub fn resize_swap_chain(&mut self, width: u32, height: u32) -> eyre::Result<()> {
        if self.width == width && self.height == height {
            return Ok(());
        }

        self.wait_for_gpu()?;
        self.render_targets.fill(None);
        unsafe {
            self.swap_chain.ResizeBuffers(
                FRAME_COUNT as u32,
                width,
                height,
                DXGI_FORMAT_B8G8R8A8_UNORM,
                DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT,
            )
        }
        .wrap_err("failed to resize text renderer host swap chain")?;

        let (rtv_heap, rtv_descriptor_size, render_targets) =
            create_render_targets(&self.device, &self.swap_chain)
                .wrap_err("failed to recreate render targets after swap-chain resize")?;
        self.rtv_heap = rtv_heap;
        self.rtv_descriptor_size = rtv_descriptor_size;
        self.render_targets = render_targets;
        self.width = width;
        self.height = height;
        self.viewport.Width = width as f32;
        self.viewport.Height = height as f32;
        self.scissor_rect.right = i32::try_from(width).unwrap_or(i32::MAX);
        self.scissor_rect.bottom = i32::try_from(height).unwrap_or(i32::MAX);
        Ok(())
    }

    pub fn present_scene_frame(&mut self, clear_color: [f32; 4]) -> eyre::Result<()> {
        let frame_index = unsafe { self.swap_chain.GetCurrentBackBufferIndex() as usize };
        let current_target = self.render_targets[frame_index]
            .as_ref()
            .expect("current swap-chain back buffer should exist before present");
        let vertex_count = self.last_upload_batch.vertices.len();

        unsafe {
            self.command_allocator.Reset()?;
            self.command_list.Reset(&self.command_allocator, &self.pipeline_state)?;
            update_text_shader_params(
                &self.resources.shader_param_buffer,
                self.width,
                self.height,
                self.start_time.elapsed().as_secs_f32(),
            )?;

            self.command_list
                .SetDescriptorHeaps(&[Some(self.resources.text_shader_resources.srv_heap.clone())]);
            self.command_list
                .SetGraphicsRootSignature(&self.root_signature);
            self.command_list.SetGraphicsRootConstantBufferView(
                0,
                self.resources.shader_param_buffer.GetGPUVirtualAddress(),
            );
            self.command_list.SetGraphicsRootDescriptorTable(
                1,
                self.resources.text_shader_resources.srv_heap.GetGPUDescriptorHandleForHeapStart(),
            );
            self.command_list.RSSetViewports(&[self.viewport]);
            self.command_list.RSSetScissorRects(&[self.scissor_rect]);

            issue_transition_barrier(
                &self.command_list,
                current_target,
                D3D12_RESOURCE_STATE_PRESENT,
                D3D12_RESOURCE_STATE_RENDER_TARGET,
            );

            let rtv_handle = D3D12_CPU_DESCRIPTOR_HANDLE {
                ptr: self.rtv_heap.GetCPUDescriptorHandleForHeapStart().ptr
                    + frame_index * self.rtv_descriptor_size as usize,
            };
            self.command_list
                .OMSetRenderTargets(1, Some(&rtv_handle), false, None);
            self.command_list
                .ClearRenderTargetView(rtv_handle, &clear_color, None);
            self.command_list
                .IASetPrimitiveTopology(primitive_topology_triangle_list());
            self.command_list
                .IASetVertexBuffers(0, Some(&[self.resources.vertex_buffer_view]));
            self.command_list.DrawInstanced(
                u32::try_from(vertex_count).unwrap_or(u32::MAX),
                1,
                0,
                0,
            );

            issue_transition_barrier(
                &self.command_list,
                current_target,
                D3D12_RESOURCE_STATE_RENDER_TARGET,
                D3D12_RESOURCE_STATE_PRESENT,
            );

            self.command_list.Close()?;
        }

        let command_lists = [Some(self.command_list.cast::<ID3D12CommandList>()?)];
        unsafe {
            self.command_queue.ExecuteCommandLists(&command_lists);
        }
        unsafe { self.swap_chain.Present(0, DXGI_PRESENT(0)).ok() }
            .wrap_err("failed to present text renderer host clear frame")?;
        self.wait_for_gpu()
    }

    fn wait_for_gpu(&mut self) -> eyre::Result<()> {
        let fence_value = self.next_fence_value;
        unsafe {
            self.command_queue.Signal(&self.fence, fence_value)?;
            if self.fence.GetCompletedValue() < fence_value {
                self.fence.SetEventOnCompletion(fence_value, *self.fence_event)?;
                WaitForSingleObjectEx(*self.fence_event, INFINITE, false);
            }
        }
        self.next_fence_value += 1;
        Ok(())
    }

    fn upload_last_batch(&mut self) -> eyre::Result<()> {
        upload_vertex_ranges(
            &self.resources.vertex_buffer,
            &self.last_upload_batch.vertices,
            &self.last_upload_batch.vertex_ranges,
        )
        .wrap_err("failed to upload text renderer vertex data")?;
        upload_curve_data(
            &self.resources.text_shader_resources.resources.curve_buffer,
            self.capacities.curve_capacity,
            &self.last_upload_batch.curve_upload_data,
        )
        .wrap_err("failed to upload text renderer curve data")?;
        upload_band_data(
            &self.resources.text_shader_resources.resources.band_buffer,
            self.capacities.band_capacity,
            &self.last_upload_batch.band_upload_data,
        )
        .wrap_err("failed to upload text renderer band data")?;
        Ok(())
    }
}

fn get_hardware_or_warp_adapter(factory: &IDXGIFactory4) -> eyre::Result<IDXGIAdapter1> {
    for index in 0.. {
        let adapter = match unsafe { factory.EnumAdapters1(index) } {
            Ok(adapter) => adapter,
            Err(error) if error.code() == DXGI_ERROR_NOT_FOUND => break,
            Err(error) => return Err(error).wrap_err("failed to enumerate DXGI adapter"),
        };

        let description = unsafe { adapter.GetDesc1() }
            .wrap_err("failed to read DXGI adapter description")?;
        let is_software =
            (DXGI_ADAPTER_FLAG(description.Flags as i32) & DXGI_ADAPTER_FLAG_SOFTWARE)
                != DXGI_ADAPTER_FLAG_NONE;
        if is_software {
            continue;
        }

        let mut test_device: Option<ID3D12Device> = None;
        if unsafe { D3D12CreateDevice(&adapter, D3D_FEATURE_LEVEL_11_0, &mut test_device) }
            .is_ok()
        {
            return Ok(adapter);
        }
    }

    let warp: IDXGIAdapter = unsafe { factory.EnumWarpAdapter() }
        .wrap_err("failed to fall back to WARP adapter for D3D12 text renderer host")?;
    warp.cast()
        .wrap_err("failed to cast WARP adapter to IDXGIAdapter1")
}

fn create_command_queue(device: &ID3D12Device) -> eyre::Result<ID3D12CommandQueue> {
    unsafe {
        device.CreateCommandQueue(&D3D12_COMMAND_QUEUE_DESC {
            Type: D3D12_COMMAND_LIST_TYPE_DIRECT,
            ..Default::default()
        })
    }
    .wrap_err("failed to create command queue")
}

#[cfg(test)]
mod tests {
    use windows::Win32::Foundation::RECT;

    use crate::{GlyphQuad, RenderScene};

    use super::TextRendererHost;

    #[test]
    fn text_renderer_host_type_exists_for_worker_ownership() {
        assert!(std::mem::size_of::<TextRendererHost>() > 0);
    }

    #[test]
    fn small_scene_preparation_still_yields_uploadable_data() -> eyre::Result<()> {
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
        let prepared = crate::prepare_render_scene(&scene)?;
        let batch = crate::build_prepared_scene_upload_batch(
            &prepared,
            crate::ShaderResourceCapacities::default(),
        );
        assert_eq!(batch.vertex_ranges, vec![0..6]);
        Ok(())
    }
}

fn client_size(hwnd: HWND) -> eyre::Result<(u32, u32)> {
    let mut rect = RECT::default();
    unsafe { GetClientRect(hwnd, &mut rect) }.wrap_err("failed to query client size")?;
    let width = u32::try_from((rect.right - rect.left).max(1)).unwrap_or(1);
    let height = u32::try_from((rect.bottom - rect.top).max(1)).unwrap_or(1);
    Ok((width, height))
}

fn create_composition_swap_chain(
    factory: &IDXGIFactory4,
    command_queue: &ID3D12CommandQueue,
    width: u32,
    height: u32,
) -> eyre::Result<IDXGISwapChain3> {
    let factory: IDXGIFactory2 = factory.cast()?;
    let description = DXGI_SWAP_CHAIN_DESC1 {
        Width: width,
        Height: height,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        Stereo: false.into(),
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
        BufferCount: FRAME_COUNT as u32,
        Scaling: DXGI_SCALING_STRETCH,
        SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
        AlphaMode: DXGI_ALPHA_MODE_PREMULTIPLIED,
        Flags: DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT.0 as u32,
    };
    let swap_chain: IDXGISwapChain1 =
        unsafe { factory.CreateSwapChainForComposition(command_queue, &description, None) }
            .wrap_err("failed to create DXGI composition swap chain")?;
    swap_chain.cast().wrap_err("failed to cast swap chain to IDXGISwapChain3")
}

fn attach_swap_chain_to_window(
    hwnd: HWND,
    swap_chain: &IDXGISwapChain3,
) -> eyre::Result<(IDCompositionDevice, IDCompositionTarget, IDCompositionVisual)> {
    let dcomp_device: IDCompositionDevice =
        unsafe { DCompositionCreateDevice::<_, IDCompositionDevice>(None::<&IDXGIDevice>) }
            .wrap_err("failed to create DirectComposition device")?;
    let dcomp_target = unsafe { dcomp_device.CreateTargetForHwnd(hwnd, true) }
        .wrap_err("failed to create DirectComposition target")?;
    let dcomp_visual = unsafe { dcomp_device.CreateVisual() }
        .wrap_err("failed to create DirectComposition visual")?;

    unsafe {
        dcomp_visual.SetContent(swap_chain)?;
        dcomp_target.SetRoot(&dcomp_visual)?;
        dcomp_device.Commit()?;
    }

    Ok((dcomp_device, dcomp_target, dcomp_visual))
}

fn create_render_targets(
    device: &ID3D12Device,
    swap_chain: &IDXGISwapChain3,
) -> eyre::Result<(ID3D12DescriptorHeap, u32, [Option<ID3D12Resource>; FRAME_COUNT])> {
    let rtv_heap: ID3D12DescriptorHeap = unsafe {
        device.CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC {
            Type: D3D12_DESCRIPTOR_HEAP_TYPE_RTV,
            NumDescriptors: FRAME_COUNT as u32,
            ..Default::default()
        })?
    };
    let rtv_descriptor_size =
        unsafe { device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_RTV) };
    let heap_start = unsafe { rtv_heap.GetCPUDescriptorHandleForHeapStart() };

    let mut render_targets = std::array::from_fn(|_| None::<ID3D12Resource>);
    for (index, slot) in render_targets.iter_mut().enumerate() {
        let resource: ID3D12Resource = unsafe { swap_chain.GetBuffer(index as u32) }?;
        let descriptor = D3D12_CPU_DESCRIPTOR_HANDLE {
            ptr: heap_start.ptr + index * rtv_descriptor_size as usize,
        };
        unsafe { device.CreateRenderTargetView(&resource, None, descriptor) };
        *slot = Some(resource);
    }

    Ok((rtv_heap, rtv_descriptor_size, render_targets))
}

fn transition_barrier(
    resource: &ID3D12Resource,
    before: D3D12_RESOURCE_STATES,
    after: D3D12_RESOURCE_STATES,
) -> D3D12_RESOURCE_BARRIER {
    D3D12_RESOURCE_BARRIER {
        Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
        Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
        Anonymous: D3D12_RESOURCE_BARRIER_0 {
            Transition: std::mem::ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                pResource: std::mem::ManuallyDrop::new(Some(resource.clone())),
                StateBefore: before,
                StateAfter: after,
                Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
            }),
        },
    }
}

fn issue_transition_barrier(
    command_list: &ID3D12GraphicsCommandList,
    resource: &ID3D12Resource,
    before: D3D12_RESOURCE_STATES,
    after: D3D12_RESOURCE_STATES,
) {
    let mut barriers = [transition_barrier(resource, before, after)];
    unsafe {
        command_list.ResourceBarrier(&barriers);

        let transition = &mut barriers[0].Anonymous.Transition;
        let resource = std::mem::ManuallyDrop::take(&mut transition.pResource);
        drop(resource);
    }
}
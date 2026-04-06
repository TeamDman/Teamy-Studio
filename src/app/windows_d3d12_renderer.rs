use std::path::PathBuf;

use eyre::Context;
use windows::Win32::Foundation::{COLORREF, E_FAIL, HANDLE, HWND, RECT, SIZE, TRUE};
use windows::Win32::Graphics::Direct3D::Fxc::{
    D3DCOMPILE_DEBUG, D3DCOMPILE_SKIP_OPTIMIZATION, D3DCompileFromFile,
};
use windows::Win32::Graphics::Direct3D::{
    D3D_FEATURE_LEVEL_11_0, D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST, ID3DBlob,
};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CLEARTYPE_QUALITY, CreateCompatibleDC,
    CreateDIBSection, CreateFontIndirectW, DIB_RGB_COLORS, DeleteDC, DeleteObject,
    GetTextExtentPoint32W, HFONT, LOGFONTW, SelectObject, SetBkMode, SetTextColor,
    TRANSPARENT, TextOutW,
};
use windows::Win32::System::Threading::{CreateEventW, INFINITE, WaitForSingleObjectEx};
use windows::Win32::UI::WindowsAndMessaging::GetClientRect;
use windows::core::{Error, HSTRING, Interface, Owned, PCSTR, s};

use super::windows_terminal::TerminalLayout;

const FRAME_COUNT: usize = 2;
const MAX_PANEL_COUNT: usize = 32;
const MAX_GLYPH_COUNT: usize = 8_192;
const MAX_VERTEX_COUNT: usize = (MAX_PANEL_COUNT + MAX_GLYPH_COUNT) * 6;
const FONT_GLYPH_COUNT: usize = 128;
const FONT_ATLAS_COLUMNS: usize = 16;
const FONT_ATLAS_ROWS: usize = FONT_GLYPH_COUNT / FONT_ATLAS_COLUMNS;
const FONT_ATLAS_CELL_WIDTH: usize = 32;
const FONT_ATLAS_CELL_HEIGHT: usize = 64;
const FONT_ATLAS_WIDTH: usize = FONT_ATLAS_COLUMNS * FONT_ATLAS_CELL_WIDTH;
const FONT_ATLAS_HEIGHT: usize = FONT_ATLAS_ROWS * FONT_ATLAS_CELL_HEIGHT;
const TERMINAL_FONT_FAMILY: &str = "CaskaydiaCove Nerd Font Mono";
const TERMINAL_FONT_HEIGHT: i32 = -42;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct Vertex {
    position: [f32; 3],
    color: [f32; 4],
    uv: [f32; 2],
    effect: f32,
    glyph: f32,
}

#[derive(Clone, Copy, Debug)]
pub enum PanelEffect {
    BlueBackground = 0,
    Sidecar = 1,
    DragHandle = 2,
    CodePanel = 3,
    ResultPanel = 4,
    PlayButton = 5,
    StopButton = 6,
    PlusButton = 7,
    Text = 8,
}

#[derive(Clone, Copy, Debug)]
pub struct PanelRect {
    pub rect: RECT,
    pub color: [f32; 4],
    pub effect: PanelEffect,
}

#[derive(Clone, Copy, Debug)]
pub struct GlyphQuad {
    pub rect: RECT,
    pub color: [f32; 4],
    pub glyph_index: u32,
}

#[derive(Debug)]
pub struct RenderScene {
    pub panels: Vec<PanelRect>,
    pub glyphs: Vec<GlyphQuad>,
}

#[derive(Debug)]
pub struct D3d12PanelRenderer {
    _dxgi_factory: IDXGIFactory4,
    _device: ID3D12Device,
    command_queue: ID3D12CommandQueue,
    swap_chain: IDXGISwapChain3,
    render_targets: [Option<ID3D12Resource>; FRAME_COUNT],
    rtv_heap: ID3D12DescriptorHeap,
    rtv_descriptor_size: u32,
    command_allocators: [ID3D12CommandAllocator; FRAME_COUNT],
    command_list: ID3D12GraphicsCommandList,
    fence: ID3D12Fence,
    next_fence_value: u64,
    frame_fence_values: [u64; FRAME_COUNT],
    fence_event: Owned<HANDLE>,
    frame_latency_waitable_object: Owned<HANDLE>,
    root_signature: ID3D12RootSignature,
    pipeline_state: ID3D12PipelineState,
    vertex_buffer: ID3D12Resource,
    vertex_buffer_view: D3D12_VERTEX_BUFFER_VIEW,
    srv_heap: ID3D12DescriptorHeap,
    _font_buffer: ID3D12Resource,
    viewport: D3D12_VIEWPORT,
    scissor_rect: RECT,
    width: u32,
    height: u32,
}

impl D3d12PanelRenderer {
    pub fn new(hwnd: HWND) -> eyre::Result<Self> {
        let (dxgi_factory, device) = create_device()?;
        let command_queue = create_command_queue(&device)?;
        let (width, height) = client_size(hwnd)?;
        let swap_chain = create_swap_chain(&dxgi_factory, &command_queue, hwnd, width, height)?;
        unsafe { dxgi_factory.MakeWindowAssociation(hwnd, DXGI_MWA_NO_ALT_ENTER)? };
        unsafe { swap_chain.SetMaximumFrameLatency(1)? };
        let frame_latency_waitable_object =
            unsafe { Owned::new(swap_chain.GetFrameLatencyWaitableObject()) };

        let (rtv_heap, rtv_descriptor_size, render_targets) =
            create_render_targets(&device, &swap_chain)?;
        let command_allocators = create_command_allocators(&device)?;
        let (srv_heap, font_buffer) = create_font_buffer_and_srv(&device)?;
        let root_signature = create_root_signature(&device)?;
        let pipeline_state = create_pipeline_state(&device, &root_signature)?;
        let command_list: ID3D12GraphicsCommandList = unsafe {
            device.CreateCommandList(
                0,
                D3D12_COMMAND_LIST_TYPE_DIRECT,
                &command_allocators[0],
                &pipeline_state,
            )
        }?;
        unsafe { command_list.Close()? };

        let (vertex_buffer, vertex_buffer_view) = create_vertex_buffer(&device)?;
        let fence: ID3D12Fence = unsafe { device.CreateFence(0, D3D12_FENCE_FLAG_NONE) }?;
        let fence_event = unsafe { Owned::new(CreateEventW(None, false, false, None)?) };

        let viewport = D3D12_VIEWPORT {
            TopLeftX: 0.0,
            TopLeftY: 0.0,
            Width: width as f32,
            Height: height as f32,
            MinDepth: D3D12_MIN_DEPTH,
            MaxDepth: D3D12_MAX_DEPTH,
        };
        let scissor_rect = RECT {
            left: 0,
            top: 0,
            right: width as i32,
            bottom: height as i32,
        };

        Ok(Self {
            _dxgi_factory: dxgi_factory,
            _device: device,
            command_queue,
            swap_chain,
            render_targets: render_targets.map(Some),
            rtv_heap,
            rtv_descriptor_size,
            command_allocators,
            command_list,
            fence,
            next_fence_value: 1,
            frame_fence_values: [0; FRAME_COUNT],
            fence_event,
            frame_latency_waitable_object,
            root_signature,
            pipeline_state,
            vertex_buffer,
            vertex_buffer_view,
            srv_heap,
            _font_buffer: font_buffer,
            viewport,
            scissor_rect,
            width,
            height,
        })
    }

    pub fn resize(&mut self, width: u32, height: u32) -> eyre::Result<()> {
        if width == 0 || height == 0 {
            return Ok(());
        }
        if width == self.width && height == self.height {
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
            )?
        };

        let (rtv_heap, rtv_descriptor_size, render_targets) =
            create_render_targets(&self._device, &self.swap_chain)?;
        self.rtv_heap = rtv_heap;
        self.rtv_descriptor_size = rtv_descriptor_size;
        self.render_targets = render_targets.map(Some);
        self.width = width;
        self.height = height;
        self.viewport.Width = width as f32;
        self.viewport.Height = height as f32;
        self.scissor_rect = RECT {
            left: 0,
            top: 0,
            right: width as i32,
            bottom: height as i32,
        };
        Ok(())
    }

    pub fn render(&mut self, scene: &RenderScene) -> eyre::Result<()> {
        self.wait_for_frame_latency()?;
        let frame_index = unsafe { self.swap_chain.GetCurrentBackBufferIndex() as usize };
        self.wait_for_frame(frame_index)?;

        let vertex_count = self.update_scene_vertices(scene)?;
        let current_target = self.render_targets[frame_index]
            .as_ref()
            .ok_or_else(|| eyre::eyre!("render target was missing for current frame"))?;
        let command_allocator = &self.command_allocators[frame_index];

        unsafe {
            command_allocator.Reset()?;
            self.command_list
                .Reset(command_allocator, &self.pipeline_state)?;

            self.command_list
                .SetDescriptorHeaps(&[Some(self.srv_heap.clone())]);
            self.command_list
                .SetGraphicsRootSignature(&self.root_signature);
            self.command_list.SetGraphicsRootDescriptorTable(
                0,
                self.srv_heap.GetGPUDescriptorHandleForHeapStart(),
            );
            self.command_list.RSSetViewports(&[self.viewport]);
            self.command_list.RSSetScissorRects(&[self.scissor_rect]);

            self.command_list.ResourceBarrier(&[transition_barrier(
                current_target,
                D3D12_RESOURCE_STATE_PRESENT,
                D3D12_RESOURCE_STATE_RENDER_TARGET,
            )]);

            let rtv_handle = D3D12_CPU_DESCRIPTOR_HANDLE {
                ptr: self.rtv_heap.GetCPUDescriptorHandleForHeapStart().ptr
                    + frame_index * self.rtv_descriptor_size as usize,
            };
            self.command_list
                .OMSetRenderTargets(1, Some(&rtv_handle), false, None);

            let clear_color = [0.0_f32, 0.0_f32, 0.0_f32, 0.0_f32];
            self.command_list
                .ClearRenderTargetView(rtv_handle, &clear_color, None);
            self.command_list
                .IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            self.command_list
                .IASetVertexBuffers(0, Some(&[self.vertex_buffer_view]));
            self.command_list
                .DrawInstanced(vertex_count as u32, 1, 0, 0);

            self.command_list.ResourceBarrier(&[transition_barrier(
                current_target,
                D3D12_RESOURCE_STATE_RENDER_TARGET,
                D3D12_RESOURCE_STATE_PRESENT,
            )]);
            self.command_list.Close()?;
        }

        let command_lists = [Some(self.command_list.cast::<ID3D12CommandList>()?)];
        unsafe {
            self.command_queue.ExecuteCommandLists(&command_lists);
            self.swap_chain.Present(0, DXGI_PRESENT(0)).ok()?;
        }

        self.signal_frame(frame_index)
    }

    fn update_scene_vertices(&self, scene: &RenderScene) -> eyre::Result<usize> {
        let mut vertices = Vec::with_capacity(scene.panels.len() * 6);
        for panel in &scene.panels {
            append_rect(
                &mut vertices,
                self.width as f32,
                self.height as f32,
                panel.rect,
                panel.color,
                panel.effect as u32,
                0,
            );
        }
        for glyph in &scene.glyphs {
            append_rect(
                &mut vertices,
                self.width as f32,
                self.height as f32,
                glyph.rect,
                glyph.color,
                PanelEffect::Text as u32,
                glyph.glyph_index,
            );
        }

        unsafe {
            let mut mapped = std::ptr::null_mut();
            self.vertex_buffer.Map(0, None, Some(&mut mapped))?;
            std::ptr::copy_nonoverlapping(vertices.as_ptr(), mapped as *mut Vertex, vertices.len());
            self.vertex_buffer.Unmap(0, None);
        }

        Ok(vertices.len())
    }

    fn wait_for_frame_latency(&self) -> eyre::Result<()> {
        if self.frame_latency_waitable_object.0.is_null() {
            return Err(eyre::eyre!(
                "swap chain did not provide a frame latency waitable object"
            ));
        }

        unsafe {
            WaitForSingleObjectEx(*self.frame_latency_waitable_object, INFINITE, false);
        }

        Ok(())
    }

    fn wait_for_frame(&self, frame_index: usize) -> eyre::Result<()> {
        let fence_value = self.frame_fence_values[frame_index];
        if fence_value == 0 {
            return Ok(());
        }

        unsafe {
            if self.fence.GetCompletedValue() < fence_value {
                self.fence
                    .SetEventOnCompletion(fence_value, *self.fence_event)?;
                WaitForSingleObjectEx(*self.fence_event, INFINITE, false);
            }
        }

        Ok(())
    }

    fn signal_frame(&mut self, frame_index: usize) -> eyre::Result<()> {
        let fence_value = self.next_fence_value;
        unsafe {
            self.command_queue.Signal(&self.fence, fence_value)?;
        }
        self.frame_fence_values[frame_index] = fence_value;
        self.next_fence_value += 1;
        Ok(())
    }

    fn wait_for_gpu(&mut self) -> eyre::Result<()> {
        let fence_value = self.next_fence_value;
        unsafe {
            self.command_queue.Signal(&self.fence, fence_value)?;
            if self.fence.GetCompletedValue() < fence_value {
                self.fence
                    .SetEventOnCompletion(fence_value, *self.fence_event)?;
                WaitForSingleObjectEx(*self.fence_event, INFINITE, false);
            }
        }
        self.next_fence_value += 1;
        Ok(())
    }
}

impl Drop for D3d12PanelRenderer {
    fn drop(&mut self) {
        let _ = self.wait_for_gpu();
    }
}

pub fn build_panel_scene(layout: TerminalLayout) -> RenderScene {
    let blue = [0.11, 0.44, 0.94, 0.5];
    let sidecar = [0.55, 0.14, 0.14, 1.0];
    let drag = [0.42, 0.18, 0.60, 1.0];
    let code = [0.05, 0.06, 0.08, 1.0];
    let result = [0.84, 0.44, 0.13, 1.0];
    let button = [0.12, 0.13, 0.17, 1.0];
    let mut panels = Vec::with_capacity(9);
    panels.push(PanelRect {
        rect: RECT {
            left: 0,
            top: 0,
            right: layout.client_width,
            bottom: layout.client_height,
        },
        color: blue,
        effect: PanelEffect::BlueBackground,
    });
    panels.push(PanelRect {
        rect: layout.sidecar_rect(),
        color: sidecar,
        effect: PanelEffect::Sidecar,
    });
    panels.push(PanelRect {
        rect: layout.drag_handle_rect(),
        color: drag,
        effect: PanelEffect::DragHandle,
    });
    panels.push(PanelRect {
        rect: layout.code_panel_rect(),
        color: code,
        effect: PanelEffect::CodePanel,
    });
    panels.push(PanelRect {
        rect: layout.terminal_rect(),
        color: [0.02, 0.02, 0.03, 1.0],
        effect: PanelEffect::CodePanel,
    });
    panels.push(PanelRect {
        rect: layout.result_panel_rect(),
        color: result,
        effect: PanelEffect::ResultPanel,
    });
    panels.push(PanelRect {
        rect: layout.plus_button_rect(),
        color: button,
        effect: PanelEffect::PlusButton,
    });
    panels.push(PanelRect {
        rect: layout.sidecar_button_rect(0),
        color: button,
        effect: PanelEffect::PlayButton,
    });
    panels.push(PanelRect {
        rect: layout.sidecar_button_rect(1),
        color: button,
        effect: PanelEffect::StopButton,
    });
    RenderScene {
        panels,
        glyphs: Vec::with_capacity(2_048),
    }
}

pub fn push_text_block(
    scene: &mut RenderScene,
    rect: RECT,
    text: &str,
    glyph_width: i32,
    glyph_height: i32,
    color: [f32; 4],
) {
    let mut cursor_x = rect.left;
    let mut cursor_y = rect.top;

    for character in text.chars() {
        if character == '\n' {
            cursor_x = rect.left;
            cursor_y += glyph_height;
            if cursor_y + glyph_height > rect.bottom {
                break;
            }
            continue;
        }

        if cursor_x + glyph_width > rect.right {
            cursor_x = rect.left;
            cursor_y += glyph_height;
        }
        if cursor_y + glyph_height > rect.bottom {
            break;
        }

        if character != ' ' && scene.glyphs.len() < MAX_GLYPH_COUNT {
            scene.glyphs.push(GlyphQuad {
                rect: RECT {
                    left: cursor_x,
                    top: cursor_y,
                    right: cursor_x + glyph_width,
                    bottom: cursor_y + glyph_height,
                },
                color,
                glyph_index: glyph_index_for_char(character),
            });
        }

        cursor_x += glyph_width;
    }
}

pub fn push_centered_text(scene: &mut RenderScene, rect: RECT, text: &str, color: [f32; 4]) {
    let glyph_count = i32::try_from(text.chars().count())
        .unwrap_or_default()
        .max(1);
    let available_width = (rect.right - rect.left - 16).max(8);
    let available_height = (rect.bottom - rect.top - 16).max(8);
    let glyph_height = available_height.clamp(12, 28);
    let glyph_width = ((available_width / glyph_count).min((glyph_height * 3) / 2)).max(8);
    let total_width = glyph_width * glyph_count;
    let text_rect = RECT {
        left: rect.left + (((rect.right - rect.left) - total_width).max(0) / 2),
        top: rect.top + (((rect.bottom - rect.top) - glyph_height).max(0) / 2),
        right: rect.right,
        bottom: rect.bottom,
    };
    push_text_block(scene, text_rect, text, glyph_width, glyph_height, color);
}

fn glyph_index_for_char(character: char) -> u32 {
    if character.is_ascii() {
        u32::from(character as u8)
    } else {
        u32::from(b'?')
    }
}

fn build_font_rows() -> Vec<u32> {
    let mut bitmap_info = BITMAPINFO::default();
    bitmap_info.bmiHeader = BITMAPINFOHEADER {
        biSize: u32::try_from(std::mem::size_of::<BITMAPINFOHEADER>()).unwrap_or_default(),
        biWidth: i32::try_from(FONT_ATLAS_WIDTH).unwrap_or_default(),
        biHeight: -i32::try_from(FONT_ATLAS_HEIGHT).unwrap_or_default(),
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB.0,
        ..Default::default()
    };

    let memory_dc = unsafe { CreateCompatibleDC(None) };
    if memory_dc.0.is_null() {
        return vec![0_u32; FONT_ATLAS_WIDTH * FONT_ATLAS_HEIGHT];
    }

    let mut bits = std::ptr::null_mut();
    let bitmap = unsafe {
        CreateDIBSection(
            Some(memory_dc),
            &bitmap_info,
            DIB_RGB_COLORS,
            &mut bits,
            None,
            0,
        )
    };
    let Ok(bitmap) = bitmap else {
        unsafe {
            let _ = DeleteDC(memory_dc);
        }
        return vec![0_u32; FONT_ATLAS_WIDTH * FONT_ATLAS_HEIGHT];
    };
    if bitmap.0.is_null() || bits.is_null() {
        unsafe {
            let _ = DeleteDC(memory_dc);
        }
        return vec![0_u32; FONT_ATLAS_WIDTH * FONT_ATLAS_HEIGHT];
    }

    let font = unsafe { CreateFontIndirectW(&terminal_font_definition()) };
    if font.0.is_null() {
        unsafe {
            let _ = DeleteObject(bitmap.into());
            let _ = DeleteDC(memory_dc);
        }
        return vec![0_u32; FONT_ATLAS_WIDTH * FONT_ATLAS_HEIGHT];
    }

    let previous_bitmap = unsafe { SelectObject(memory_dc, bitmap.into()) };
    let previous_font = unsafe { SelectObject(memory_dc, HFONT(font.0).into()) };
    let _ = unsafe { SetBkMode(memory_dc, TRANSPARENT) };
    let _ = unsafe { SetTextColor(memory_dc, COLORREF(0x00FF_FFFF)) };

    let atlas_pixels = unsafe {
        std::slice::from_raw_parts_mut(bits.cast::<u8>(), FONT_ATLAS_WIDTH * FONT_ATLAS_HEIGHT * 4)
    };
    atlas_pixels.fill(0);

    for code in 0_u8..u8::try_from(FONT_GLYPH_COUNT).unwrap_or(u8::MAX) {
        let glyph = char::from(code);
        let glyph_utf16 = [u16::from(code)];
        let column = usize::from(code) % FONT_ATLAS_COLUMNS;
        let row = usize::from(code) / FONT_ATLAS_COLUMNS;
        let origin_x = i32::try_from(column * FONT_ATLAS_CELL_WIDTH).unwrap_or_default();
        let origin_y = i32::try_from(row * FONT_ATLAS_CELL_HEIGHT).unwrap_or_default();

        let mut size = SIZE::default();
        let _ = unsafe { GetTextExtentPoint32W(memory_dc, &glyph_utf16, &mut size) };
        let x = origin_x
            + ((i32::try_from(FONT_ATLAS_CELL_WIDTH).unwrap_or_default() - size.cx).max(0) / 2);
        let y = origin_y
            + ((i32::try_from(FONT_ATLAS_CELL_HEIGHT).unwrap_or_default() - size.cy).max(0) / 2);
        let _ = unsafe { TextOutW(memory_dc, x, y, &glyph_utf16) };

        if !glyph.is_ascii_graphic() && glyph != ' ' {
            continue;
        }
    }

    let mut rows = vec![0_u32; FONT_ATLAS_WIDTH * FONT_ATLAS_HEIGHT];
    for y in 0..FONT_ATLAS_HEIGHT {
        for x in 0..FONT_ATLAS_WIDTH {
            let base = ((y * FONT_ATLAS_WIDTH) + x) * 4;
            let blue = u32::from(atlas_pixels[base]);
            let green = u32::from(atlas_pixels[base + 1]);
            let red = u32::from(atlas_pixels[base + 2]);
            let alpha = red.max(green).max(blue);
            rows[(y * FONT_ATLAS_WIDTH) + x] = alpha;
        }
    }

    unsafe {
        let _ = SelectObject(memory_dc, previous_font);
        let _ = SelectObject(memory_dc, previous_bitmap);
        let _ = DeleteObject(font.into());
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(memory_dc);
    }

    rows
}

fn terminal_font_definition() -> LOGFONTW {
    let mut font = LOGFONTW {
        lfHeight: TERMINAL_FONT_HEIGHT,
        lfQuality: CLEARTYPE_QUALITY,
        ..Default::default()
    };
    for (slot, value) in font
        .lfFaceName
        .iter_mut()
        .zip(TERMINAL_FONT_FAMILY.encode_utf16())
    {
        *slot = value;
    }
    font
}

fn create_device() -> eyre::Result<(IDXGIFactory4, ID3D12Device)> {
    let mut dxgi_flags = DXGI_CREATE_FACTORY_FLAGS(0);
    if cfg!(debug_assertions) {
        unsafe {
            let mut debug = None;
            if D3D12GetDebugInterface::<ID3D12Debug>(&mut debug).is_ok() {
                if let Some(debug) = debug {
                    debug.EnableDebugLayer();
                    dxgi_flags |= DXGI_CREATE_FACTORY_DEBUG;
                }
            }
        }
    }

    let dxgi_factory: IDXGIFactory4 = unsafe { CreateDXGIFactory2(dxgi_flags) }?;
    let adapter = get_hardware_adapter(&dxgi_factory)?;

    let mut device = None;
    unsafe { D3D12CreateDevice(&adapter, D3D_FEATURE_LEVEL_11_0, &mut device) }?;
    let device = device.expect("device should be initialized after D3D12CreateDevice succeeds");
    Ok((dxgi_factory, device))
}

fn get_hardware_adapter(factory: &IDXGIFactory4) -> eyre::Result<IDXGIAdapter1> {
    for index in 0.. {
        let adapter = match unsafe { factory.EnumAdapters1(index) } {
            Ok(adapter) => adapter,
            Err(error) if error.code() == DXGI_ERROR_NOT_FOUND => break,
            Err(error) => return Err(error.into()),
        };

        let description = unsafe { adapter.GetDesc1() }?;
        let is_software = (DXGI_ADAPTER_FLAG(description.Flags as i32)
            & DXGI_ADAPTER_FLAG_SOFTWARE)
            != DXGI_ADAPTER_FLAG_NONE;
        if is_software {
            continue;
        }

        let mut test_device: Option<ID3D12Device> = None;
        if unsafe { D3D12CreateDevice(&adapter, D3D_FEATURE_LEVEL_11_0, &mut test_device) }.is_ok()
        {
            return Ok(adapter);
        }
    }

    Err(Error::new(E_FAIL, "no suitable D3D12 adapter found").into())
}

fn create_command_queue(device: &ID3D12Device) -> eyre::Result<ID3D12CommandQueue> {
    Ok(unsafe {
        device.CreateCommandQueue(&D3D12_COMMAND_QUEUE_DESC {
            Type: D3D12_COMMAND_LIST_TYPE_DIRECT,
            ..Default::default()
        })?
    })
}

fn create_command_allocators(
    device: &ID3D12Device,
) -> eyre::Result<[ID3D12CommandAllocator; FRAME_COUNT]> {
    let mut allocators = std::array::from_fn(|_| None::<ID3D12CommandAllocator>);
    for slot in &mut allocators {
        *slot = Some(unsafe { device.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT) }?);
    }

    Ok(allocators.map(Option::unwrap))
}

fn create_swap_chain(
    factory: &IDXGIFactory4,
    command_queue: &ID3D12CommandQueue,
    hwnd: HWND,
    width: u32,
    height: u32,
) -> eyre::Result<IDXGISwapChain3> {
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
        SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
        AlphaMode: DXGI_ALPHA_MODE_IGNORE,
        Flags: DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT.0 as u32,
    };

    let swap_chain: IDXGISwapChain1 =
        unsafe { factory.CreateSwapChainForHwnd(command_queue, hwnd, &description, None, None)? };
    Ok(swap_chain.cast()?)
}

fn client_size(hwnd: HWND) -> eyre::Result<(u32, u32)> {
    let mut rect = RECT::default();
    unsafe { GetClientRect(hwnd, &mut rect) }.wrap_err("failed to query client size")?;
    let width = (rect.right - rect.left).max(0) as u32;
    let height = (rect.bottom - rect.top).max(0) as u32;
    Ok((width, height))
}

fn create_render_targets(
    device: &ID3D12Device,
    swap_chain: &IDXGISwapChain3,
) -> eyre::Result<(ID3D12DescriptorHeap, u32, [ID3D12Resource; FRAME_COUNT])> {
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

    Ok((
        rtv_heap,
        rtv_descriptor_size,
        render_targets.map(Option::unwrap),
    ))
}

fn create_root_signature(device: &ID3D12Device) -> eyre::Result<ID3D12RootSignature> {
    let descriptor_ranges = [D3D12_DESCRIPTOR_RANGE {
        RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_SRV,
        NumDescriptors: 1,
        BaseShaderRegister: 0,
        RegisterSpace: 0,
        OffsetInDescriptorsFromTableStart: D3D12_DESCRIPTOR_RANGE_OFFSET_APPEND,
    }];
    let root_parameters = [D3D12_ROOT_PARAMETER {
        ParameterType: D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE,
        Anonymous: D3D12_ROOT_PARAMETER_0 {
            DescriptorTable: D3D12_ROOT_DESCRIPTOR_TABLE {
                NumDescriptorRanges: descriptor_ranges.len() as u32,
                pDescriptorRanges: descriptor_ranges.as_ptr(),
            },
        },
        ShaderVisibility: D3D12_SHADER_VISIBILITY_PIXEL,
    }];
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

fn create_pipeline_state(
    device: &ID3D12Device,
    root_signature: &ID3D12RootSignature,
) -> eyre::Result<ID3D12PipelineState> {
    let compile_flags = if cfg!(debug_assertions) {
        D3DCOMPILE_DEBUG | D3DCOMPILE_SKIP_OPTIMIZATION
    } else {
        0
    };

    let shader_path = shader_path();
    let shader_path_hstring: HSTRING = shader_path.to_string_lossy().as_ref().into();
    let vertex_shader = compile_shader(
        &shader_path_hstring,
        s!("VSMain"),
        s!("vs_5_0"),
        compile_flags,
    )?;
    let pixel_shader = compile_shader(
        &shader_path_hstring,
        s!("PSMain"),
        s!("ps_5_0"),
        compile_flags,
    )?;

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
    ];

    let blend_target = D3D12_RENDER_TARGET_BLEND_DESC {
        BlendEnable: TRUE,
        LogicOpEnable: false.into(),
        SrcBlend: D3D12_BLEND_SRC_ALPHA,
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
        VS: shader_bytecode(&vertex_shader),
        PS: shader_bytecode(&pixel_shader),
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

fn compile_shader(
    path: &HSTRING,
    entry_point: PCSTR,
    target: PCSTR,
    flags: u32,
) -> eyre::Result<ID3DBlob> {
    let mut shader = None;
    let mut error = None;
    unsafe {
        D3DCompileFromFile(
            path,
            None,
            None,
            entry_point,
            target,
            flags,
            0,
            &mut shader,
            Some(&mut error),
        )
    }
    .map_err(|err| shader_error(err, error))?;

    Ok(shader.expect("shader blob should be initialized"))
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

fn shader_bytecode(shader: &ID3DBlob) -> D3D12_SHADER_BYTECODE {
    D3D12_SHADER_BYTECODE {
        pShaderBytecode: unsafe { shader.GetBufferPointer() },
        BytecodeLength: unsafe { shader.GetBufferSize() },
    }
}

fn shader_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("app")
        .join("windows_panel_shaders.hlsl")
}

fn create_vertex_buffer(
    device: &ID3D12Device,
) -> eyre::Result<(ID3D12Resource, D3D12_VERTEX_BUFFER_VIEW)> {
    let buffer_size = (std::mem::size_of::<Vertex>() * MAX_VERTEX_COUNT) as u64;

    let mut vertex_buffer = None;
    unsafe {
        device.CreateCommittedResource(
            &D3D12_HEAP_PROPERTIES {
                Type: D3D12_HEAP_TYPE_UPLOAD,
                ..Default::default()
            },
            D3D12_HEAP_FLAG_NONE,
            &D3D12_RESOURCE_DESC {
                Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
                Width: buffer_size,
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
            &mut vertex_buffer,
        )?
    };
    let vertex_buffer: ID3D12Resource = vertex_buffer.expect("vertex buffer should be initialized");

    Ok((
        vertex_buffer.clone(),
        D3D12_VERTEX_BUFFER_VIEW {
            BufferLocation: unsafe { vertex_buffer.GetGPUVirtualAddress() },
            SizeInBytes: buffer_size as u32,
            StrideInBytes: std::mem::size_of::<Vertex>() as u32,
        },
    ))
}

fn create_font_buffer_and_srv(
    device: &ID3D12Device,
) -> eyre::Result<(ID3D12DescriptorHeap, ID3D12Resource)> {
    let font_rows = build_font_rows();
    let byte_len = (font_rows.len() * std::mem::size_of::<u32>()) as u64;

    let mut font_buffer = None;
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
            &mut font_buffer,
        )?
    };
    let font_buffer: ID3D12Resource = font_buffer.expect("font buffer should be initialized");

    unsafe {
        let mut mapped = std::ptr::null_mut();
        font_buffer.Map(0, None, Some(&mut mapped))?;
        std::ptr::copy_nonoverlapping(font_rows.as_ptr(), mapped as *mut u32, font_rows.len());
        font_buffer.Unmap(0, None);
    }

    let srv_heap: ID3D12DescriptorHeap = unsafe {
        device.CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC {
            Type: D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,
            NumDescriptors: 1,
            Flags: D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,
            ..Default::default()
        })?
    };

    let desc = D3D12_SHADER_RESOURCE_VIEW_DESC {
        Format: DXGI_FORMAT_R32_UINT,
        ViewDimension: D3D12_SRV_DIMENSION_BUFFER,
        Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
        Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
            Buffer: D3D12_BUFFER_SRV {
                FirstElement: 0,
                NumElements: font_rows.len() as u32,
                StructureByteStride: 0,
                Flags: D3D12_BUFFER_SRV_FLAG_NONE,
            },
        },
    };

    unsafe {
        device.CreateShaderResourceView(
            &font_buffer,
            Some(&desc),
            srv_heap.GetCPUDescriptorHandleForHeapStart(),
        );
    }

    Ok((srv_heap, font_buffer))
}

fn append_rect(
    vertices: &mut Vec<Vertex>,
    width: f32,
    height: f32,
    rect: RECT,
    color: [f32; 4],
    effect: u32,
    glyph_index: u32,
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

    let top_left = Vertex {
        position: to_ndc(width, height, left, top),
        color,
        uv: [0.0, 0.0],
        effect,
        glyph,
    };
    let top_right = Vertex {
        position: to_ndc(width, height, right, top),
        color,
        uv: [1.0, 0.0],
        effect,
        glyph,
    };
    let bottom_right = Vertex {
        position: to_ndc(width, height, right, bottom),
        color,
        uv: [1.0, 1.0],
        effect,
        glyph,
    };
    let bottom_left = Vertex {
        position: to_ndc(width, height, left, bottom),
        color,
        uv: [0.0, 1.0],
        effect,
        glyph,
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

fn to_ndc(width: f32, height: f32, x: f32, y: f32) -> [f32; 3] {
    [(x / width) * 2.0 - 1.0, 1.0 - (y / height) * 2.0, 0.0]
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

#[cfg(test)]
mod tests {
    use super::{RenderScene, push_centered_text, push_text_block};
    use windows::Win32::Foundation::RECT;

    #[test]
    fn push_text_block_emits_visible_glyphs() {
        let mut scene = RenderScene {
            panels: Vec::new(),
            glyphs: Vec::new(),
        };
        push_text_block(
            &mut scene,
            RECT {
                left: 0,
                top: 0,
                right: 100,
                bottom: 40,
            },
            "A B",
            8,
            16,
            [1.0, 1.0, 1.0, 1.0],
        );

        assert_eq!(scene.glyphs.len(), 2);
    }

    #[test]
    fn push_centered_text_places_a_glyph() {
        let mut scene = RenderScene {
            panels: Vec::new(),
            glyphs: Vec::new(),
        };
        push_centered_text(
            &mut scene,
            RECT {
                left: 0,
                top: 0,
                right: 80,
                bottom: 60,
            },
            "7",
            [1.0, 1.0, 1.0, 1.0],
        );

        assert_eq!(scene.glyphs.len(), 1);
    }
}

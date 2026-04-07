# DirectX Migration Plan

Goal: move Teamy Studio from a whole-window GDI layered-alpha presentation to a D3D12 renderer that can control transparency per region, sample pointer state with minimal latency, and animate the blue frame background independently.

Why this migration exists:
- The current Win32 host uses `SetLayeredWindowAttributes`, which applies one alpha value to the whole window.
- The product direction now needs the blue frame background to be translucent while terminal and panel surfaces remain visually solid.
- Animated clouds in only the blue regions require a GPU-rendered background pass rather than a flat GDI fill.

Reference inspiration:
- `g:/Programming/Repos/DirectX-Learning/crates/windows-rs-sample-direct3d12-improved-v6/src/graphics/mod.rs`

## Constraints

- Keep PTY startup, keyboard input, and workspace lifecycle behavior stable while migrating rendering.
- Keep each slice runnable and testable on its own.
- Prefer the sample's low-latency frame pacing model over a paint-on-demand WM_PAINT loop.
- Accept higher setup complexity if it buys explicit presentation control and future in-app drag responsiveness.
- Preserve the product's frameless presentation: Teamy Studio owns the full visible surface, with no OS title bar, caption buttons, or preference-colored borders.
- Preserve native resize affordances despite the frameless shell: edge and corner hit-testing must still produce the expected OS resize cursors and resize behaviors.
- Treat interactive resize as a low-latency path: the presented UI should keep reacting during the drag itself rather than freezing and snapping when the drag completes.

## Plan

### Phase 0: Isolate Current Background Responsibilities

Goal: separate the blue frame background paint path from terminal text and panel paint logic so it can be replaced later.

Tasks:
- move the background fill logic into a dedicated Windows background module
- keep the existing GDI implementation as the default backend
- leave panel and terminal painting behavior unchanged

Status:
- complete: `src/app/windows_background.rs` now owns the current background alpha and fill path

### Phase 1: Replace The Host Presentation Path

Goal: stop treating DirectX as an optional background path and make D3D12 the only window renderer.

Tasks:
- remove the buffered GDI WM_PAINT presentation path
- install a continuous D3D12 render loop with frame-latency waitable-object pacing
- keep workspace interactions and PTY lifecycle on the Win32 host side

Status:
- complete: the window host now targets a dedicated D3D12 panel renderer instead of the previous GDI backbuffer blit path
- complete: the Win32 host now claims the full window as client area and supplies its own edge/corner hit-testing so the app stays frameless while retaining native resize cursors

### Phase 2: Move Notebook Chrome Into Shader Panels

Goal: make every colored notebook region a shader-driven D3D12 surface rather than a flat GDI fill.

Tasks:
- render the blue frame, sidecar, drag region, code panel, result panel, and buttons as D3D12 quads
- give each panel family its own subtle shader treatment
- keep the blue region visually strongest because it is the global backdrop

Status:
- complete: the D3D12 panel renderer now owns those surfaces and shades them in HLSL
- complete: OS-managed chrome is intentionally absent; the visible accent strip and notebook frame are entirely app-rendered surfaces

### Phase 3: Restrict Transparency to the Blue Background

Goal: make only the blue frame regions translucent while terminal, sidecar, drag zone, and result panel surfaces remain visually opaque.

Tasks:
- remove dependence on whole-window alpha for the final presentation path
- composite the opaque panel surfaces above the translucent background layer
- validate that gaps and panel edges remain crisp

### Phase 4: Add Animated Clouds Shader

Goal: animate cloud motion inside only the blue background areas.

Tasks:
- add a lightweight shader pass for cloud noise and motion
- scope the shader output to the blue frame regions only
- keep frame pacing reasonable without affecting terminal input latency

### Phase 5: Port Terminal Glyph Rendering To The GPU Path

Goal: remove the last GDI-era rendering assumptions by drawing terminal glyphs and notebook labels in the same presented frame.

Tasks:
- extract terminal render data from `libghostty-vt` without relying on GDI drawing calls
- draw glyphs and cursor through the GPU presentation path
- preserve current keyboard and PTY behavior while the text renderer changes

## Immediate Next Slice

1. switch the D3D12 swap chain from the initial HWND presentation path to the composition-aware path needed for real per-pixel blue-only transparency
2. move terminal glyph rendering into the same presented frame
3. add time-driven cloud motion once the composition path is in place

## Recent Window-Host Findings

- The resize crash was caused by leaked swap-chain back-buffer references in the resource-barrier helper, not by the resize architecture itself.
- The post-resize transparent gap came from leaving a native non-client resize frame attached to a visually frameless popup window; the fix was to make the whole window client-owned with `WM_NCCALCSIZE` and explicit edge/corner `WM_NCHITTEST` handling.
- The expected Teamy Studio shell is now explicit: no OS chrome, no OS-colored borders, but native edge resize cursors and behaviors must still work.
- Live resize should be treated as an always-hot path. Deferring all resize work until after the drag ends causes visible freeze-frame behavior, so the host should resize and present during the drag itself.
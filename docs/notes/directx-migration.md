# DirectX Migration Plan

Goal: move Teamy Studio from a whole-window GDI layered-alpha presentation to a renderer that can control transparency per region and animate the blue frame background independently.

Why this migration exists:
- The current Win32 host uses `SetLayeredWindowAttributes`, which applies one alpha value to the whole window.
- The product direction now needs the blue frame background to be translucent while terminal and panel surfaces remain visually solid.
- Animated clouds in only the blue regions require a GPU-rendered background pass rather than a flat GDI fill.

Reference inspiration:
- `g:/Programming/Repos/DirectX-Learning/crates/windows-rs-sample-direct3d12-improved-v6/src/graphics/mod.rs`

## Constraints

- Keep PTY startup, keyboard input, and workspace lifecycle behavior stable while migrating rendering.
- Keep each slice runnable and testable on its own.
- Avoid a large all-at-once renderer rewrite.

## Plan

### Phase 0: Isolate Current Background Responsibilities

Goal: separate the blue frame background paint path from terminal text and panel paint logic so it can be replaced later.

Tasks:
- move the background fill logic into a dedicated Windows background module
- keep the existing GDI implementation as the default backend
- leave panel and terminal painting behavior unchanged

Status:
- complete: `src/app/windows_background.rs` now owns the current background alpha and fill path

### Phase 1: Define a Background Renderer Seam

Goal: make the background layer replaceable without touching terminal input or PTY state.

Tasks:
- introduce a small background renderer interface used by the window host
- pass client size and timing information through that seam
- keep a GDI fallback implementation alongside a future DirectX implementation

### Phase 2: Introduce a DirectX Background Renderer

Goal: draw the blue frame background through a DirectX swap chain or compositor-backed surface.

Tasks:
- create a dedicated DirectX renderer module for the background layer
- render only the background regions, not terminal glyphs
- preserve the existing window interaction model and plus-button behavior

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

## Immediate Next Slice

1. add a background renderer seam that the window host can call each paint
2. keep the current GDI background implementation behind that seam
3. start a DirectX background module using the DirectX-Learning sample as structure guidance
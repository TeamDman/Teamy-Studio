# Window Border Garden Plan

## Goal

Replace Teamy Studio's current inward white panel-border fade with a
more expressive animated border language that reads as an exterior frame
instead of an overlay on top of content.

The first implementation target is not true rendering outside the HWND.
It is a practical, resumable redesign of the client-area layout so the
window owns an explicit decorative garden band around the content frame,
can render a feathered outer edge with shader-driven motion, and can do
so without stealing pixels from the terminal, diagnostics panel, or
scene body.

This plan is a focused follow-on to the broader window-language work in
docs/notes/window-language-and-launcher-plan.md. It narrows in on the
border/garden architecture, shader organization, and Tracey coverage
needed to land the effect cleanly.

## Current Status

- Done so far:
  - Read the current custom-window host in src/app/windows_app.rs, the
    shared layout model in src/app/windows_terminal.rs, the D3D12
    renderer in src/app/windows_d3d12_renderer.rs, the current shader in
    src/app/windows_panel_shaders.hlsl, the windowing spec in
    docs/spec/product/windowing.md, the Tracey config in
    .config/tracey/config.styx, and the earlier window-language plan in
    docs/notes/window-language-and-launcher-plan.md.
  - Confirmed that Teamy Studio already owns the full visible window
    surface with WS_POPUP and WM_NCCALCSIZE returning 0, so there is no
    OS caption or border to piggyback on.
  - Confirmed that all panel geometry is currently defined in client-area
    coordinates via TerminalLayout, with frame_rect() using WINDOW_PADDING
    as an implicit interior margin rather than a first-class decorative
    band.
  - Confirmed that the current border effect is a blanket post-process in
    PSMain that applies border_mask() to essentially every shaded panel,
    which is why the glow encroaches inward on content.
  - Confirmed that the renderer currently compiles a single shader source
    file, src/app/windows_panel_shaders.hlsl, using D3DCompileFromFile
    with no include handler and a single VSMain/PSMain pipeline.
  - Confirmed that teamy-studio-windowing currently reports 0 of 13
    requirements covered in Tracey, so this area already has coverage
    debt that should be addressed alongside new work.
- Current focus:
  - Record a concrete, resumable implementation strategy for the garden
    band and outward border effect before changing layout or shader
    plumbing.
- Remaining work:
  - Define the garden/content coordinate model.
  - Refactor the renderer so borders are dedicated surfaces instead of a
    blanket final fade.
  - Decide how to separate the new border/chrome shader concern from the
    current monolithic shader file.
  - Land a first non-overlapping exterior frame effect.
  - Adapt the supplied ring/noise language into a rectangular border.
  - Restore Tracey usefulness for the windowing surface.
- Next step:
  - Introduce an explicit surface-layout abstraction with a reserved
    garden band and prove it with a static outward-only frame before
    attempting animated noise or true translucent exterior edges.

## Constraints And Assumptions

- The app currently renders into a swap chain sized to the HWND client
  rect. The swap-chain alpha mode is ignore, so true pixels outside the
  HWND or per-pixel transparent window edges are not part of the current
  rendering model.
- The current window host already owns drag, resize, caption buttons, and
  diagnostics affordances. Any garden-band change must preserve native
  resize semantics from the real window edges, not from the inset content
  frame.
- TerminalLayout currently mixes three concepts together:
  - the full client renderable area
  - the interior frame margin created by WINDOW_PADDING
  - the content frame that title, terminal, and diagnostics panels are
    laid out inside
- The current shader architecture assumes one HLSL source file and one
  graphics pipeline. A separate shader source for border/chrome concerns
  is possible, but it is not a zero-cost asset move.
- The current border effect is global. Replacing it safely requires
  decoupling border rendering from fill shading so code-panel and scene
  surfaces stop receiving accidental edge treatment.
- The existing windowing spec already describes launcher, picker, and
  diagnostics behaviors, but its implementation coverage is effectively
  absent. New work should improve that situation instead of widening the
  gap.
- Validation should continue to use .\check-all.ps1 rather than ad hoc
  cargo commands.

## Product Requirements

- Teamy Studio windows must stop using an inward white border fade that
  overlaps terminal and scene content.
- The visible border treatment must read as exterior decoration around a
  content frame rather than a highlight applied inside content panels.
- The border effect should support a richer visual language inspired by
  the supplied ring/noise shader: animated motion, chromatic glow, and a
  softer outer falloff.
- The outer edge should feather away instead of appearing as motion on a
  hard black rectangle.
- Terminal windows and non-terminal scene windows should share the same
  garden/frame language where practical.
- Title-bar controls, diagnostics toggles, and scene hit targets must
  remain readable and predictable while the effect animates.
- The content frame must remain stable for terminal viewport math,
  diagnostics layout, scene-button layout, and selection logic.
- The solution should leave room for the future window language described
  in docs/notes/window-language-and-launcher-plan.md and the broader
  cell-grid/window-state direction captured in docs/notes/cellgrid.md.
- The shader concern for border/chrome work should be separated from the
  existing monolith as an intentional design step, even if the first cut
  uses shared compilation plumbing rather than a wholly separate runtime
  pipeline.

## Architectural Direction

### Recommended first slice

Treat the garden as a first-class band inside the client rect, not as a
true outside-the-HWND effect.

This is the best first step because it fits the current architecture:

- the window already owns a frameless WS_POPUP surface
- D3D12 rendering already fills the client area
- hit testing already operates on the real client edges
- the current content layout already expects an inset frame

The garden-band version can preserve the current host architecture while
achieving the core user-visible requirement: the border grows outward
from the content frame instead of stealing space from the content itself.

### Surface model recommendation

Introduce an explicit surface-layout abstraction, either by expanding
TerminalLayout or by adding a new higher-level WindowSurfaceLayout that
feeds it.

The abstraction should separate:

- full_client_rect: the full renderable swap-chain area
- garden_rect: the decorative band that surrounds the content frame
- content_frame_rect: the inset frame containing title bar and body
- title_bar_rect: derived from content_frame_rect, not from the full
  client rect
- body/content rects: terminal panel, diagnostics panel, scene body,
  future menu surfaces

The current frame_rect() logic in TerminalLayout is the natural starting
point for this refactor, but it should stop being “just padding” and
become an explicit semantic boundary.

### Rendering direction

Move away from a blanket final border mask and toward dedicated garden
surfaces.

Recommended shape:

- interior panels keep their own fill shaders
- a dedicated garden/frame surface renders the outer glow/rim
- optional overlay panels handle feather or highlight passes when needed
- border modulation is driven by dedicated PanelEffect values, not by a
  final border blend applied to all non-text panels

This lets the terminal panel, diagnostics panel, scene body, and title
bar remain visually stable while the garden band evolves independently.

### Shader separation direction

The current renderer compiles a single shader file:

- src/app/windows_panel_shaders.hlsl

The requested separation should be planned as one of these, in order of
practicality:

1. Enable file-include support in the current compile path and split the
   border/chrome helpers into a new HLSL include or sibling source such
   as src/app/windows_chrome_shaders.hlsl while preserving the current
   VSMain/PSMain entrypoints.
2. If include-based organization proves awkward, introduce a second
   pipeline path for chrome/garden surfaces later.

Recommendation: do not start with a second pipeline. Start by making the
compile path capable of shared HLSL composition, then factor the border
and chrome code into a dedicated module.

### Out-of-scope for the first slice

- true pixels outside the HWND
- layered-window or per-pixel transparent host changes
- DWM shadow replacement experiments
- full menu-bar implementation
- the larger Facet-backed window-state/cell-grid unification

Those remain valid future directions, but they should not block the
first garden-band implementation.

## Tracey Specification Strategy

This is a narrow extension of the existing windowing surface, not a new
product subsystem. The right place for the behavior spec remains:

- docs/spec/product/windowing.md

Recommended spec additions or refinements:

- a requirement that shared custom-chrome windows reserve a decorative
  exterior band outside the content frame
- a requirement that border effects do not overlap the terminal/scene
  content frame
- a requirement that the exterior edge feathers rather than ending on a
  hard opaque band, if that behavior is considered committed and not just
  stylistic tuning

Tracey baseline at planning time:

```powershell
tracey query status
tracey query uncovered
tracey query unmapped
tracey query unmapped --path src/app/windows_app.rs
tracey query unmapped --path src/app/windows_d3d12_renderer.rs
tracey query validate --deny warnings
```

Current known baseline:

- tracey query validate is clean.
- teamy-studio-windowing currently reports 0 of 13 requirements covered
  and 0 verified.
- The current config does not include HLSL sources in the windowing impl,
  so unmapped queries against src/app/windows_panel_shaders.hlsl are not
  yet meaningful.

Recommended Tracey strategy for this work:

- Phase 1 should restore meaningful Rust-side windowing coverage in
  src/app/windows_app.rs, src/app/windows_terminal.rs, and
  src/app/windows_d3d12_renderer.rs.
- The plan should explicitly decide whether shader sources deserve their
  own Tracey impl, for example:
  - teamy-studio-windowing/rust for host/layout/renderer Rust
  - teamy-studio-windowing/shaders for HLSL behavior

Recommendation: if the garden effect meaningfully lives in HLSL rather
than just parameter wiring, add a dedicated shaders impl to
.config/tracey/config.styx instead of pretending all observable behavior
is captured solely by Rust orchestration.

Follow-up once mappings are in place:

```powershell
tracey query untested
```

## Phased Task Breakdown

### Phase 1: Windowing Spec And Coverage Baseline

Objective:

- Make the current window/chrome surface measurable before changing its
  layout or shader behavior.

Tasks:

- Extend docs/spec/product/windowing.md with the committed border/garden
  requirements that this plan intends to preserve.
- Add or refine Tracey implementation references in:
  - src/app/windows_app.rs
  - src/app/windows_terminal.rs
  - src/app/windows_d3d12_renderer.rs
- Decide whether to add shader coverage in
  .config/tracey/config.styx for HLSL sources.
- Capture the new baseline with tracey query status and validate.

Definition of done:

- Windowing requirements for the garden/content boundary are written.
- teamy-studio-windowing no longer reports zero coverage.
- Tracey validation stays clean after the spec/config updates.

### Phase 2: Surface Layout And Garden Band Introduction

Objective:

- Introduce an explicit decorative band around the content frame without
  changing the visible style yet.

Tasks:

- Refactor TerminalLayout or introduce a new WindowSurfaceLayout to model:
  - full client bounds
  - garden band
  - content frame
  - title/body regions derived from the content frame
- Rebase title bar, terminal panel, diagnostics panel, and scene body
  rects on the content frame.
- Keep resize hit testing tied to the real client edges in
  src/app/windows_app.rs.
- Add layout tests covering:
  - content does not overlap the garden band
  - caption buttons remain inside the title bar
  - terminal and diagnostics regions still fit at small sizes

Definition of done:

- The renderer can lay out windows with a reserved garden band.
- Existing content surfaces render within the new content frame without
  overlap regressions.
- Resize and drag behavior still work from the real window edges.

### Phase 3: Border Effect Isolation

Objective:

- Stop applying border treatment as a blanket post-process on every panel.

Tasks:

- Remove or gate the global border_mask() blend at the end of PSMain.
- Introduce dedicated PanelEffect variants for garden/frame surfaces.
- Update build_panel_scene() and any shared scene builders to emit
  garden/frame panels explicitly.
- Add renderer tests verifying that border/garden surfaces are emitted as
  distinct panel effects rather than being implied by a final global
  shader blend.

Definition of done:

- Interior fills no longer receive accidental inward border modulation.
- Border rendering is driven by explicit surfaces/effects.
- Tests cover the presence of the new dedicated frame effects.

### Phase 4: Shader Organization And Static Exterior Frame

Objective:

- Separate the border/chrome shader concern and land a first non-animated
  outward-only frame.

Tasks:

- Update the shader compile path in
  src/app/windows_d3d12_renderer.rs so border/chrome logic can live in a
  separate HLSL source or include.
- Extract border/chrome helpers out of
  src/app/windows_panel_shaders.hlsl into a dedicated module.
- Implement a static garden-frame shader with:
  - an outer glow
  - a controlled rim highlight
  - no encroachment into the content frame
  - a softer outer alpha falloff than the current hard edge
- Verify title text and caption buttons still sit on stable surfaces.

Definition of done:

- Border/chrome shader logic is no longer only an inline section of the
  monolithic panel shader.
- The window renders with an outward-only static garden frame.
- Terminal and scene content remain unchanged inside the content frame.

### Phase 5: Animated Ring And Noise Treatment

Objective:

- Adapt the supplied ring/noise visual language into a rectangular window
  border.

Tasks:

- Port or recreate the needed noise helpers in the chrome/garden shader
  module.
- Replace circular “distance from center” ring logic with a rectangular
  or frame-distance metric so the motion follows the border geometry.
- Use the existing scene_time uniform for animation.
- Tune alpha extraction and falloff so the exterior edge feels feathered
  rather than like motion on black fill.
- Keep the effect bounded to the garden band so content remains stable.
- Add at least one regression hook or diagnostic artifact path that makes
  garden-frame rendering inspectable during future refactors.

Definition of done:

- The garden frame animates with noise/ring-like motion.
- The effect remains readable and does not compromise content legibility.
- The exterior edge falls off softly instead of ending as a hard band.

### Phase 6: Interaction And Performance Hardening

Objective:

- Ensure the new garden/frame work does not regress usability or redraw
  behavior.

Tasks:

- Verify hit testing for:
  - drag handle
  - resize edges
  - diagnostics button
  - minimize/maximize/close buttons
  - terminal selection and scene interactions
- Audit cached-scene invalidation so garden animation does not force
  unnecessary content rebuilds.
- Check panel counts and renderer budget after garden panels are added.
- Run .\check-all.ps1 and resolve any regressions tied to the new layout
  or shader plumbing.

Definition of done:

- Input behavior matches current expectations.
- Validation is green.
- Garden animation does not imply avoidable content-scene churn.

### Phase 7: Long-Horizon Follow-Through

Objective:

- Leave the next architectural branch explicit instead of implicit.

Tasks:

- Record whether future menu bars, picker windows, and diagnostics/x-ray
  views should reuse the same garden-band layout primitives.
- Decide whether true outside-the-HWND translucency is still desirable
  after the garden-band version lands.
- If yes, spin that into a dedicated follow-up plan rather than letting
  it remain an unbounded “maybe” inside this one.
- Note how the garden-band work relates to the broader cell-grid/window
  state direction in docs/notes/cellgrid.md.

Definition of done:

- The next branch of work is explicit.
- A future agent can tell whether the garden-band version is the end
  state or just the first milestone.

## Recommended Implementation Order

1. Restore meaningful Tracey coverage for the existing windowing surface.
2. Introduce explicit garden/content layout boundaries.
3. Remove the global inward border fade and replace it with dedicated
   garden/frame effects.
4. Separate border/chrome shader organization from the monolithic shader
   file.
5. Land a static outward-only frame.
6. Layer in the animated ring/noise treatment.
7. Harden interaction, caching, and validation.

## Open Decisions

- Should the first implementation stop at a garden band inside the client
  rect, or should it pursue true rendering outside the HWND immediately?
  Recommendation: stop at the garden band first.
- Should HLSL sources gain their own Tracey impl, or is Rust-side mapping
  sufficient for now?
  Recommendation: add a shaders impl if the effect logic meaningfully
  lives in HLSL.
- Should shader separation use include-based composition or a second
  pipeline?
  Recommendation: start with include-based composition and keep one
  pipeline until there is a measurable need for more.
- Should the title bar remain wholly inside the content frame, or should
  some title-bar styling migrate into the garden band?
- Should garden thickness stay fixed in pixels or scale with DPI/window
  size?
- Should the feathered exterior edge be simulated entirely inside the
  client rect, or is there still product value in later pursuing a truly
  translucent outside-the-window silhouette?

## First Concrete Slice

- Add the missing windowing spec mappings and record the current baseline.
- Refactor the layout model so the content frame is distinct from the
  garden band.
- Remove the global PSMain border fade from interior panels.
- Add one dedicated garden-frame panel effect with a static outward glow
  and soft outer falloff.
- Validate with .\check-all.ps1 before introducing animation or shader
  noise.
# Cursor Info Plan

## Goal

Add a new cursor-info diagnostic surface to Teamy Studio that helps debug
desktop-space versus client-space geometry problems, especially tooltip
placement and DPI-sensitive cursor overlap.

The user-facing target is:

- a new `teamy-studio cursor-info` subcommand
- a new `Cursor Info` launcher button in the Teamy Studio landing window
- a live-updating TUI that visualizes a ground-truth segmentation mask of
  desktop-space observations around the cursor
- support for panning, zooming, resize, and coordinate inspection so the
  tool can explain why a tooltip or button intersects the cursor or the
  monitor edge

This plan intentionally treats the cursor-info tool as a focused diagnostic
product slice, not as proof that every Teamy Studio window must
immediately become a universal Facet-backed cell-grid runtime. The broader
window-language direction remains relevant, but this tool should deliver
debugging value before that larger architecture is complete.

## Current Status

- Done so far:
  - Read the current request, the attached `docs/notes/cellgrid.md`
    context, the existing launcher/window-language plan, the repo
    instructions in `AGENTS.md`, and the resumable-plans skill.
  - Confirmed that Teamy Studio already opens a launcher scene by default
    through `src/app/mod.rs` and `src/app/windows_app.rs` via
    `run_launcher()`.
  - Confirmed that launcher and picker windows already exist in
    `src/app/windows_scene.rs`, with large image-card actions for
    Terminal, Storage, and Audio.
  - Confirmed that scene diagnostics already reuse the shared text-grid
    renderer in `src/app/cell_grid.rs`, including terminal-style linear and
    block selection behavior.
  - Confirmed that the current CLI surface only exposes `terminal` and
    `self-test`; there is no `cursor-info` command yet.
  - Confirmed that Teamy Studio already has useful Win32 geometry anchors:
    `GetCursorPos`, monitor bounds queries, top-level `EnumWindows`,
    window-rect helpers, native tooltip controllers, and typed
    screen/client geometry in `src/app/spatial.rs`.
  - Confirmed that `crossterm` is already a dependency.
  - Confirmed that a local ratatui workspace is available at
    `G:\Programming\Repos\ratatui`, so Teamy Studio can consume ratatui
    directly as a path dependency if needed instead of treating it as a
    hypothetical future option.
  - Confirmed from the provided `picker_tui.rs` example that a ratatui TUI
    can be initialized manually on `stderr` using
    `CrosstermBackend<BufWriter<Stderr>>`, explicit raw-mode / alternate-
    screen entry and exit, and a panic hook that restores the terminal.
  - Confirmed from ratatui documentation that `Terminal::draw` performs a
    buffer diff and only writes changed cells after each full-frame render,
    which makes it a good fit for the live segmentation-mask viewport.
  - Confirmed that `G:\Programming\Repos\tui-widgets\tui-big-text\src\pixel_size.rs`
    already contains a practical pixel-density helper that maps higher-
    resolution logical pixels onto terminal cells using half, quadrant,
    sextant, quarter-height, and octant Unicode block symbols.
  - Confirmed that `D:\Repos\rust\winc\src\lib.rs` exports a small set of
    screen-capture-oriented modules through its prelude, including monitor
    and monitor-region capture helpers, so Teamy Studio can vendor only the
    screenshot pieces it needs for desktop-view rendering.
  - Captured the current Tracey baseline with `tracey query status`:
    - behavior: 39 of 56 covered, 31 of 56 verified
    - cli: 40 of 43 covered, 26 of 43 verified
    - convention: 4 of 4 covered, 0 of 4 verified
    - os: 10 of 10 covered, 6 of 10 verified
    - publishing-standards: 8 of 8 covered, 0 of 8 verified
    - tool-standards: 22 of 35 covered, 14 of 35 verified
    - windowing: 11 of 16 covered, 11 of 16 verified
- Current focus:
  - Record a concrete implementation plan for the cursor-info diagnostic
    tool and its launcher integration.
- Remaining work:
  - Add a new top-level CLI subcommand for cursor-info.
  - Add a new launcher button and action for opening the tool.
  - Build the desktop-observation backend needed to describe cursor,
    monitor, window, button, and tooltip geometry in a single snapshot.
  - Build the standalone TUI with pan, zoom, live-update, and overlap
    visualization.
  - Decide the first hosted-window strategy for opening the same tool from
    the launcher.
  - Add dedicated Tracey coverage for this subsystem.
- Next step:
  - Land the new Tracey spec plus a minimal `teamy-studio cursor-info`
    command skeleton and shared snapshot model before implementing the
    live TUI.

## Constraints And Assumptions

- Teamy Studio already has a working launcher scene and custom scene-window
  chrome. The cursor-info feature should extend that surface rather than
  replace it.
- CLI subcommands in this repo must follow the repo rule from `AGENTS.md`:
  each subcommand gets its own directory module and its own `*_cli.rs`
  entry file that the module re-exports.
- The repository standard validation command is `./check-all.ps1`, not
  `cargo check`.
- `src/app/spatial.rs` already models desktop-relative geometry as
  `ScreenPoint` and `ScreenRect`. For this plan, “desktop coordinates” and
  the existing “screen coordinates” mean the same OS-space coordinate
  system. Do not introduce a third naming scheme without a clear payoff.
- The current tooltip placement work already uses desktop-space geometry,
  but the current model still approximates the cursor as a rectangle. The
  new tool exists to expose the real cursor shape, hotspot, and overlap.
- `src/app/cell_grid.rs` and existing diagnostics selection behavior are
  reusable for text-mode inspection views. That is a better starting point
  than inventing a second selection model.
- `crossterm` is already available, and ratatui is available locally. The
  default TUI direction for this plan is a ratatui app using the crossterm
  backend, not hand-written VT diffing.
- The cursor-info TUI should operate on `stderr`, following the same manual
  initialization and restoration shape shown in the provided
  `picker_tui.rs` example, because that pattern is already known to work in
  this environment.
- Teamy Studio already has standalone diagnostic binaries such as
  `src/bin/windows_key_probe.rs`, but the committed user-facing interface
  for this feature is a top-level `teamy-studio cursor-info` subcommand,
  not a separate ad hoc probe executable.
- The launcher currently exposes Terminal, Storage, and Audio. Adding
  Cursor Info is a small extension to an existing scene abstraction, not a
  brand-new launcher system.
- Enumerating “other app windows” is feasible with `EnumWindows`, but
  exact visual bounds may need DWM frame bounds instead of raw
  `GetWindowRect` for good fidelity.
- Enumerating Teamy-owned tooltip bounds is feasible because Teamy creates
  and positions those tooltip windows itself. Enumerating foreign app
  tooltip windows may not be reliable enough for phase one.

## Product Requirements

### Committed Requirements

- Teamy Studio must expose a `cursor-info` top-level CLI subcommand.
- Teamy Studio’s launcher window must expose a `Cursor Info` action.
- Activating the launcher action must open a new cursor-info window or
  hosted surface rather than replacing the launcher.
- The cursor-info tool must be usable as a standalone true TUI from
  `teamy-studio cursor-info`.
- The TUI must live-update as the cursor moves.
- The initial viewport must center on the cursor’s active point.
- The TUI must distinguish at least these classes visually:
  - cursor-covered pixels
  - cursor hotspot pixel
  - monitor out-of-bounds area
  - Teamy Studio window bounds
  - foreign top-level window bounds
  - Teamy button bounds relevant to tooltip diagnosis
  - Teamy tooltip bounds
- When a pixel belongs to multiple classes, the TUI must make the overlap
  visible rather than silently hiding one class behind another.
- The TUI must support right-drag panning.
- The TUI must support mouse-wheel zooming of the viewport.
- The TUI window/surface must remain resizable.
- The tool must display the cursor active point in desktop coordinates and
  in the active Teamy client coordinate system when applicable.
- The tool must display monitor bounds relative to the cursor.
- The tool must help diagnose tooltip-placement bugs specifically, not
  just act as a generic cursor toy.
- The TUI must support rendering the segmentation mask at a higher logical
  resolution than one terminal cell per pixel by using a vendored pixel-
  size helper.
- The TUI must support cycling among three render modes with `x`:
  - `mask`
  - `desktop`
  - `overlay`
- `mask` mode must show the colourful segmentation mask only.
- `desktop` mode must show an actual desktop screenshot of the inspected
  area.
- `overlay` mode must show the desktop screenshot with segmentation-derived
  cursor, hotspot, tooltip, button, monitor, and window overlays on top.

### Captured But Not Yet Committed As Phase-One Requirements

- Rendering foreign application tooltips as a supported, trustworthy class.
- Reusing the cursor-info TUI runtime as the canonical renderer for all
  future Teamy Studio windows.
- A generalized renderer-switching model where every Teamy window can flip
  among raw cell-grid, TUI, and richer D3D12 presentations.
- Full Facet-backed persistence and replay for every live window state.

These longer-horizon ideas should remain visible, but they must not block
the first diagnostic tool that answers the immediate tooltip/cursor/DPI
question.

## Architectural Direction

Build the feature in two layers:

1. a reusable desktop-observation backend
2. a presentation layer that renders those observations into a TUI grid

The backend should produce a snapshot model with only value types and
plain geometry, for example:

- `CursorInfoSnapshot`
- `CursorGeometry`
- `MonitorGeometry`
- `ObservedWindowGeometry`
- `ObservedTooltipGeometry`
- `ObservedButtonGeometry`
- `CursorInfoViewport`
- `CursorInfoRenderMode`
- `DesktopCaptureFrame`

Recommended modeling rules:

- All geometry in the snapshot is expressed in desktop/screen
  coordinates.
- Client-relative coordinates are derived views attached only when a
  cursor point lies inside a Teamy window that can provide a client
  transform.
- The observation backend should expose enough metadata for textual
  diagnostics and future serialization, but it should not carry raw Win32
  handles deeper into the TUI model than necessary.
- Snapshot structs that are stable and value-like should derive `Facet`
  where practical. Raw OS handles and live thread-owned objects should not
  be forced into `Facet` just to satisfy a philosophy goal.

Recommended backend responsibilities:

- query the live cursor active point
- query the real cursor shape and hotspot, not just an axis-aligned guess
- enumerate monitor work/bounds and per-monitor DPI context when needed
- enumerate relevant Teamy windows and foreign top-level windows
- expose Teamy-owned button and tooltip rectangles from live Teamy state
- produce a unified desktop-space snapshot at a fixed polling cadence
- capture a desktop image for the currently inspected region so the TUI can
  render screenshot-backed modes

Recommended presentation responsibilities:

- maintain viewport center and zoom
- rasterize snapshot classes into a cell-grid mask
- support multiple logical pixel densities so a single terminal cell can
  encode more than one mask pixel when the selected pixel size allows it
- encode overlap explicitly, likely by time-rotating the winning class for
  each conflicted cell instead of trying to add colors together
- render either mask pixels, captured desktop pixels, or both, depending on
  the active render mode
- render a textual legend and coordinate readout alongside the mask
- support right-drag panning and wheel zoom
- support `x` as a render-mode cycle key: `mask -> desktop -> overlay -> mask`
- preserve resize behavior without breaking the underlying desktop-space
  interpretation

Recommended TUI implementation direction:

- use ratatui for frame composition, layout, and terminal buffer diffing
- use the crossterm backend because Teamy Studio already depends on
  `crossterm`
- vendor the pixel-size helper logic from
  `G:\Programming\Repos\tui-widgets\tui-big-text\src\pixel_size.rs`
  into Teamy Studio so the cursor-info mask can render at higher logical
  density using half-block, quadrant, sextant, quarter-height, or octant
  cell encodings without taking a broad dependency on `tui-big-text`
- vendor the narrow screenshot-capture pieces needed from
  `D:\Repos\rust\winc\src\lib.rs` so the cursor-info tool can render a
  desktop screenshot without taking a broad dependency on the full `winc`
  crate surface
- manually construct the ratatui terminal on `stderr`, following the
  `picker_tui.rs` pattern rather than the default `stdout` convenience
  helpers
- keep the terminal lifecycle explicit:
  - enable raw mode
  - enter alternate screen on `stderr`
  - install a restore-on-panic hook
  - draw full frames through ratatui
  - restore raw mode and alternate-screen state on all exits
- treat ratatui as the layer that handles dirty-cell output efficiently;
  Teamy Studio should focus on producing the per-frame mask and metadata,
  not on re-implementing terminal diff logic
- keep the pixel-density abstraction local to Teamy Studio so the mask
  renderer can choose an initial default such as quadrant or octant and
  later expose density switching without entangling the rest of the app
- keep the render-mode model local as well:
  - `mask` for pure semantic debugging
  - `desktop` for visual ground truth
  - `overlay` for correlating both at once

Recommended hosting direction:

- Phase one should prioritize the standalone `teamy-studio cursor-info`
  subcommand because it gives immediate diagnostic value.
- Launcher integration should come next by opening a dedicated Teamy
  window that hosts the same underlying model, ideally without requiring a
  PTY when the app owns both the backend and the input routing.
- If an in-process hosted TUI would delay the first useful slice, allow a
  temporary split where the standalone TUI lands first and the Teamy-owned
  host catches up in the next phase.

## Tracey Specification Strategy

This is a new user-facing subsystem with its own interaction model and its
own diagnostic purpose. It should get a dedicated spec by default rather
than being squeezed entirely into the existing windowing or behavior spec.

Recommended spec work:

- add a new spec file: `docs/spec/product/cursor-info.md`
- add a new Tracey entry in `.config/tracey/config.styx`, for example
  `teamy-studio-cursor-info`
- extend `docs/spec/product/cli.md` narrowly for the new top-level
  subcommand surface
- extend `docs/spec/product/windowing.md` narrowly for the launcher button
  and hosted-window entry point

Recommended `cursor-info.md` coverage areas:

- standalone subcommand availability
- launcher entry point
- live cursor-centered viewport behavior
- render-mode cycling behavior
- overlap visualization semantics
- pan and zoom behavior
- monitor/window/tooltip/button visibility classes
- desktop/client coordinate reporting
- Teamy-owned tooltip and button introspection rules

Baseline Tracey workflow for this roadmap:

```powershell
tracey query status
tracey query uncovered
tracey query unmapped
tracey query unmapped --path src/cli/cursor_info
tracey query unmapped --path src/app/windows_cursor_info.rs
tracey query unmapped --path src/app/windows_app.rs
tracey query unmapped --path src/app/spatial.rs
tracey query validate --deny warnings
```

Follow-up once the new implementation references are mostly in place:

```powershell
tracey query untested
```

Current baseline at plan creation time:

- `tracey query status` shows partial coverage debt repo-wide.
- `teamy-studio-windowing` currently has 11 of 16 requirements covered.
- `teamy-studio-cli` currently has 40 of 43 requirements covered.
- The new cursor-info work should add focused mappings as it lands instead
  of pretending the surrounding specs are already complete.

## Phased Task Breakdown

### Phase 1: Spec And Command Skeleton

Objective:

- Define the cursor-info subsystem in Tracey and add the new command
  surface before implementing the live renderer.

Tasks:

- Add `docs/spec/product/cursor-info.md` with the initial observable
  requirements for the new tool.
- Update `.config/tracey/config.styx` to include the new spec.
- Extend `docs/spec/product/cli.md` for a new top-level `cursor-info`
  command.
- Extend `docs/spec/product/windowing.md` for the launcher entry point.
- Add `src/cli/cursor_info/mod.rs` and
  `src/cli/cursor_info/cursor_info_cli.rs`.
- Update `src/cli/mod.rs` to register the new top-level command.
- Add the app-facing entry point in `src/app/mod.rs` for launching the
  cursor-info tool.
- Introduce the first version of the pure snapshot/view-model types in a
  dedicated app module, recommended name: `src/app/windows_cursor_info.rs`.

Definition of done:

- `teamy-studio cursor-info --help` is wired into the CLI.
- Tracey validates cleanly with the new spec present.
- A minimal command path exists that can launch a placeholder cursor-info
  experience or print a structured placeholder report without inventing the
  full backend yet.

### Phase 2: Desktop Observation Backend

Objective:

- Build the OS-backed snapshot model that makes cursor and tooltip bugs
  observable in one coordinate system.

Tasks:

- Implement cursor active-point collection using desktop coordinates.
- Implement real cursor geometry collection, including hotspot and mask
  extraction, instead of relying only on a rectangle heuristic.
- Enumerate monitor bounds and record which monitor currently contains the
  cursor active point.
- Enumerate Teamy windows and foreign top-level windows with stable
  snapshot records.
- Prefer DWM frame bounds over raw `GetWindowRect` when that materially
  improves visible-bound fidelity.
- Expose Teamy-owned tooltip rectangles from the tooltip controller logic
  rather than recomputing them indirectly.
- Expose Teamy button rectangles needed for tooltip diagnosis.
- Vendor the smallest useful screenshot-capture slice from `winc`, likely
  around monitor and monitor-region capture helpers, and adapt it into a
  Teamy-owned desktop capture interface.
- Add a capture path that can produce an image for the currently inspected
  desktop region or current monitor at a cadence appropriate for a live TUI.
- Add pure geometry tests for coordinate transforms, hotspot placement,
  and clipping logic where possible.

Definition of done:

- A single snapshot call can describe cursor, monitor, Teamy window,
  foreign window, button, and Teamy tooltip geometry in desktop space.
- The backend can also provide a desktop capture frame suitable for the
  `desktop` and `overlay` render modes.
- The snapshot model is usable without the TUI renderer.
- The backend is covered by focused unit tests where deterministic logic
  exists.

### Phase 3: Standalone Cursor-Info TUI

Objective:

- Ship the first truly useful diagnostic tool via `teamy-studio
  cursor-info`.

Tasks:

- Implement the standalone TUI event loop on top of the snapshot backend.
- Add ratatui as a Teamy Studio dependency, preferably as an explicit path
  dependency to the local workspace during initial development.
- Vendor a focused `PixelSize` helper module derived from
  `tui-big-text` so the viewport can map multiple logical pixels into a
  single terminal cell.
- Implement a manual `stderr` terminal harness modeled after the provided
  `picker_tui.rs` setup so the app controls raw mode, alternate screen,
  and panic restoration directly.
- Initialize the viewport so the cursor hotspot starts at the center of
  the grid.
- Render a segmentation-mask-like view with explicit classes for cursor,
  hotspot, out-of-bounds, Teamy windows, foreign windows, Teamy buttons,
  and Teamy tooltips.
- Render desktop-backed cells when the active mode is `desktop` or
  `overlay`.
- Add overlap cycling so multi-class cells are visibly conflicted instead
  of flattened.
- Choose and implement the first default pixel density for the mask view,
  with a bias toward quadrant or octant if terminal/font rendering stays
  legible enough.
- Add `x` key handling to cycle render modes in the order `mask`,
  `desktop`, `overlay`.
- Add a legend and textual readout for desktop coordinates, active client
  coordinates, zoom level, and current viewport origin.
- Add right-drag panning.
- Add wheel zoom for viewport scaling.
- Handle terminal resize cleanly.
- Keep the update loop live while the cursor moves.

Definition of done:

- `teamy-studio cursor-info` launches a live, resizable TUI.
- The cursor hotspot remains visually identifiable.
- Pan and zoom work without corrupting coordinate reporting.
- The viewport renders at a higher logical resolution than plain full-cell
  blocks by using the vendored pixel-size helper.
- Pressing `x` cycles cleanly among `mask`, `desktop`, and `overlay`
  modes.
- `desktop` mode shows an actual captured desktop view for the inspected
  region, and `overlay` mode combines that capture with the semantic
  geometry overlay.
- The tool can visibly demonstrate whether a Teamy tooltip intersects the
  cursor or monitor edge.

### Phase 4: Launcher And Hosted Window Integration

Objective:

- Make the diagnostic tool reachable from the Teamy launcher and usable as
  a Teamy-managed window.

Tasks:

- Add `Cursor Info` to the launcher scene button list in
  `src/app/windows_scene.rs`.
- Extend scene actions and action handling in `src/app/windows_app.rs` to
  open the tool.
- Choose and implement the first hosted-window strategy:
  - preferred direction: host the same core model in-process inside a
    Teamy-owned window
  - acceptable fallback: temporarily spawn the standalone cursor-info path
    if that is the fastest way to unblock the diagnostic workflow
- Ensure the hosted window remains resizable.
- Add a diagnostics view for the hosted cursor-info surface so it can show
  its own textual representation when Teamy diagnostics mode is toggled.

Definition of done:

- The launcher exposes a working `Cursor Info` button.
- Activating it opens a Teamy-managed cursor-info experience in a separate
  window.
- The hosted surface and standalone command share the same conceptual
  model, even if their rendering plumbing is not yet fully unified.

### Phase 5: Hardening, Coverage, And Architecture Follow-Through

Objective:

- Lock in the new subsystem and decide which broader architectural ideas
  actually earned adoption.

Tasks:

- Add focused tests for TUI interaction math, overlap cycling, viewport
  centering, and launcher plumbing where feasible.
- Add focused tests for render-mode cycling and any deterministic desktop-
  to-cell sampling logic.
- Add Tracey references and verification tags throughout the new cursor-info
  implementation.
- Run `tracey query untested` and close the most relevant new gaps.
- Run `./check-all.ps1` after each meaningful implementation slice.
- Revisit whether the hosted cursor-info path should stay TUI-based,
  migrate toward a direct D3D12 scene, or become the first proving ground
  for a shared cell-grid window runtime.
- Decide whether foreign tooltip discovery is worth pursuing or should stay
  out of scope.

Definition of done:

- The new command and launcher feature are covered by Tracey and repo
  validation.
- The team has an explicit recorded decision on the post-phase-one hosting
  direction.
- The plan can be updated with either completed work or a narrowed follow-on
  roadmap instead of drifting into speculation.

## Recommended Implementation Order

1. Add the dedicated spec and top-level command skeleton first.
2. Build the pure desktop observation backend before touching TUI
   rendering.
3. Ship the standalone `teamy-studio cursor-info` TUI as the first useful
  diagnostic tool, using ratatui on `stderr` rather than hand-managed VT
  output.
4. Add launcher integration and hosted Teamy window support.
5. Harden tests, Tracey coverage, and the long-term hosting decision.

This order is deliberate. The standalone command gives immediate
diagnostic leverage for the tooltip-placement problem without forcing the
entire Teamy window architecture to be solved first.

## Open Decisions

- What exact Win32 API combination should be used for trustworthy cursor
  mask extraction on all relevant DPI configurations?
- Which pixel density should be the default for the first release:
  quadrant, sextant, quarter-height, or octant?
- What screenshot refresh policy should the desktop-backed modes use:
  every frame, throttled cadence, or change-driven where feasible?
- Should foreign window bounds default to DWM extended frame bounds, or is
  raw `GetWindowRect` sufficient for the first slice?
- Should the first hosted launcher path spawn the standalone subcommand or
  host the model in-process immediately?
- Is foreign-tooltip visualization a real product requirement, or should
  phase one remain limited to Teamy-owned tooltips?
- Which snapshot structs should derive `Facet` now, and which should stay
  plain Rust types until persistence requirements are concrete?

## First Concrete Slice

Implement Phase 1 as the next work session.

Recommended first edits:

- add `docs/spec/product/cursor-info.md`
- update `.config/tracey/config.styx`
- add `src/cli/cursor_info/mod.rs`
- add `src/cli/cursor_info/cursor_info_cli.rs`
- update `src/cli/mod.rs`
- update `src/app/mod.rs`
- add the first `CursorInfoSnapshot` and `CursorInfoViewport` types in a
  new app module
- include a `CursorInfoRenderMode` in the first view-model sketch so
  `mask`, `desktop`, and `overlay` are designed into the model early
- plan a small vendored `pixel_size` module derived from `tui-big-text`
  for the first ratatui rendering slice
- plan a small vendored desktop-capture adapter derived from `winc` for the
  first screenshot-backed rendering slice
- prepare `Cargo.toml` for ratatui integration once Phase 3 begins,
  keeping the TUI on `stderr` via manual backend construction rather than
  the convenience `stdout` helpers

Recommended first validation loop:

```powershell
tracey query validate --deny warnings
./check-all.ps1
```

If Phase 1 lands cleanly, the next slice should be the pure backend for
cursor/mask/monitor/window snapshot collection, not launcher polish.
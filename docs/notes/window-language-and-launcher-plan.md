# Window Language And Launcher Plan

## Goal

Evolve Teamy Studio from a single terminal-first window into a small
family of native windows with a shared UI language:

- a landing page window with large image buttons for Terminal, Storage,
  and Audio
- a reusable pick-window abstraction for icon-driven choices such as the
  planned audio-source picker
- a consistent diagnostics affordance that can swap any window into a
  text-oriented scene description
- an audible terminal bell when the shell emits a standalone BEL
- a path from the current orange diagnostics panel toward a real
  cell-grid-backed auxiliary view that can support selection and richer
  inspection

The immediate product target is not the full generalized window system.
It is a practical first step that lets Teamy Studio open into a launcher,
keep the current terminal path working, and establish the abstractions
that later picker, paint, explorer, and diagnostics windows can share.

## Current Status

- Done so far:
  - Read the current Teamy Studio window/runtime code, Tracey config,
    existing notes, and the current notebook roadmap.
  - Confirmed that the app currently launches a single terminal window
    through `src/app/windows_app.rs`, with a fixed D3D12 render path, a
    purple drag strip, an orange diagnostics panel, and a top-right plus
    button.
  - Confirmed that `resources/main.png` and `resources/storage.png`
    already exist in the repo and can support compile-time embedded
    launcher buttons later.
  - Confirmed that bell handling did not exist in either VT backend and
    that raw-byte scanning would be wrong because BEL is also used as an
    OSC terminator.
  - Landed a first bell slice by adding a shared Windows bell helper,
    wiring real bell callbacks through both the Teamy and Ghostty VT
    engines, extending the product behavior spec, and validating the new
    path with targeted bell tests, OSC regression tests, and `tracey
    query validate --deny warnings`.
- Current focus:
  - Record the launcher/window-language architecture in a resumable plan
    and sequence the first non-bell implementation slices.
- Remaining work:
  - Build a real landing page window surface.
  - Build a reusable image-button scene abstraction and separate shader
    source for its hover/press effects.
  - Replace the current plus-button behavior with a diagnostics toggle.
  - Turn the orange panel into a toggleable auxiliary cell-grid view.
  - Add the audio picker window and decide the first supported audio
    sources.
  - Decide how far to push the long-horizon “every window has a canonical
    cell-grid / Facet-backed state” direction in this roadmap.
- Next step:
  - Introduce a small shared window-scene abstraction so Teamy Studio can
    render at least two content modes: the existing terminal scene and a
    new launcher scene.

## Constraints And Assumptions

- The current app is terminal-first. `crate::app::run()` opens a terminal
  window directly, and `src/app/windows_app.rs` is built around that
  assumption.
- The current rendering pipeline is centralized in
  `src/app/windows_d3d12_renderer.rs` and compiles a single shader file,
  `src/app/windows_panel_shaders.hlsl`. Separate shader sources are
  possible, but they need explicit pipeline/plumbing work rather than a
  trivial asset drop.
- The orange diagnostics panel is currently a text block rendered from a
  string plus diagnostic cell dimensions. It is not yet a cell-grid scene
  with selection behavior.
- Terminal selection behavior already exists and supports both linear and
  rectangular selection modes. That logic should be reused when the
  diagnostics/output surface becomes a real selectable cell grid.
- The current product spec contains a mismatch that must be resolved in
  the new window-language work: `behavior[workspace.plus-button.appends-
  cell]` describes a notebook-style plus button, the current
  implementation clones a new terminal window, and the new target is for
  that top-right affordance to become a diagnostics toggle.
- Bell handling must remain terminal-semantic. Standalone BEL should ring
  the bell; BEL bytes used to terminate OSC sequences must not.
- The current `PickerTui` attachment is useful as interaction inspiration,
  but not as an implementation substrate for the native D3D12 windows.
- The existing notebook roadmap in `docs/notes/notebook-roadmap.md`
  remains relevant for long-term multi-window cell workflows. This plan
  focuses on the launcher/window-language work needed before that broader
  notebook system can become the primary surface.

## Product Requirements

### Near-term committed requirements

- Teamy Studio must support an audible bell when the shell emits a
  standalone BEL control character.
- Teamy Studio must gain a landing page window with large image buttons.
- The first landing page must include buttons for Terminal, Storage, and
  Audio.
- The Terminal launcher button must open the existing terminal window
  flow.
- The Storage launcher button may initially open a placeholder dialog
  that says the feature is not implemented yet.
- The Audio launcher button must open a second picker-style window for
  selecting an audio source.
- The first audio-source picker must present Windows and Pick File as
  large image buttons with text below the icons.
- The image-button windows must share a common interaction language,
  including neutral, hovered, hover-near anticipation, pressed, and
  clicked-decay states.
- The image-button effects must be shader-driven and must not depend on a
  one-off button implementation for each new window.
- Each non-terminal pick window must still have Teamy Studio’s custom
  title-bar language and a top-right diagnostics affordance.
- Toggling diagnostics in a pick window must replace the main scene with a
  text representation of that scene rather than opening a separate native
  dialog.
- The current top-right plus button in terminal windows must become the
  diagnostics button and must toggle the visibility of the orange panel.
- The orange panel must stop being a dead text block and must move toward
  a cell-grid-backed auxiliary surface that can eventually support linear
  and rectangular selection.

### Captured but not yet committed as phase-one requirements

- A generalized model where every window has a canonical cell-grid-backed
  representation and can switch between multiple renderers.
- Facet-backed window state, renderer switching, provenance-rich config,
  event sourcing, and exploratory tools such as paint, brush, and tree
  inspector windows.
- Full menu bars, decoration-button families, and a generalized focus and
  event-routing language across many independent Teamy Studio windows.

These longer-horizon directions are important and should remain visible,
but they should not be allowed to block the first launcher/picker slices.

## Architectural Direction

Treat the next phase as a shared window-scene refactor, not as a
collection of one-off dialogs.

The recommended direction is:

- keep the existing Win32 host and D3D12 renderer
- introduce an explicit scene/content mode layer above the renderer
- use that scene layer to represent terminal, launcher, audio picker, and
  diagnostics-text scenes
- move common button hit-testing and visual state into reusable scene
  primitives rather than encoding them inside terminal-specific layout
  code

Short-term abstraction targets:

- `WindowSceneKind` or equivalent:
  - terminal
  - launcher
  - picker
  - diagnostics-text
- shared top chrome state:
  - title
  - diagnostics toggle button state
  - hover / pressed / click-decay animation state
- shared image-button scene model:
  - compile-time embedded image bytes via `include_bytes!`
  - label text below icon
  - layout rect
  - visual interaction state
  - action enum
- diagnostics view contract:
  - every scene can provide a lossy text representation of itself
  - diagnostics toggle swaps the rendered body to that text scene

Recommended boundaries for the first refactor:

- keep the existing terminal PTY/session architecture intact
- do not try to make the launcher or picker windows terminal-backed
- do not force the full generalized cell-grid/Facet architecture into the
  first launcher slice
- do introduce enough scene separation that the terminal window is no
  longer the only content model the app can render

For the orange panel and future auxiliary surfaces, prefer a dedicated
cell-grid scene model that can reuse terminal-style selection semantics,
rather than continuing to special-case a single diagnostic string.

## Tracey Specification Strategy

This work now spans two kinds of spec changes:

- small terminal behavior extensions that belong in existing specs
- a genuinely new behavior area for launcher/picker windows and shared
  window-scene language

The bell work that already landed is a narrow terminal behavior extension,
so it belongs in `docs/spec/product/behavior.md`.

The launcher/picker/diagnostics window work is a new behavior area and
should get a dedicated spec file rather than being forced into the
existing behavior spec. The recommended new file is:

- `docs/spec/product/windowing.md`

That phase must also update `.config/tracey/config.styx` so the new spec
is included alongside the existing product specs.

The new windowing spec should cover:

- launcher startup behavior
- picker-window behavior
- diagnostics-toggle behavior across window types
- image-button interaction semantics
- the repurposed diagnostics button behavior in terminal windows

The existing `behavior[workspace.plus-button.appends-cell]` requirement
must be revisited during that phase. It no longer matches either the
current implementation or the new target behavior.

Baseline workflow for this roadmap:

```powershell
tracey query status
tracey query uncovered
tracey query unmapped
tracey query unmapped --path src/app/windows_app.rs
tracey query unmapped --path src/app/windows_d3d12_renderer.rs
tracey query unmapped --path src/app/windows_audio.rs
tracey query validate --deny warnings
```

Follow-up once the new scene/windowing layers are in place:

```powershell
tracey query untested
```

Current baseline after the bell slice:

- `tracey query validate --deny warnings` is clean.
- `tracey query status` reports:
  - behavior: 33 of 51 covered, 29 of 51 verified
  - cli: 37 of 40 covered, 26 of 40 verified
  - convention: 4 of 4 covered, 0 of 4 verified
  - os: 10 of 10 covered, 6 of 10 verified
  - tool standards: 22 of 35 covered, 14 of 35 verified
- The repo still has broader coverage debt, so the launcher/windowing work
  should add focused mappings as it lands rather than pretending the repo
  is otherwise fully mapped.

## Phased Task Breakdown

### Phase 1: Bell And Terminal Event Hygiene

Objective:

- Make BEL behave correctly before broadening the window surface.

Tasks:

- Add a shared bell helper in the Windows app layer.
- Surface real bell callbacks from both VT engines.
- Ensure standalone BEL rings the bell and OSC BEL terminators do not.
- Add terminal bell behavior to the product spec.
- Validate with targeted tests and Tracey validation.

Definition of done:

- BEL produces an audible bell through both Teamy and Ghostty backends.
- OSC title/prompt parsing does not regress.
- Tracey validation is clean.

Status:

- Completed in this work session.

### Phase 2: Shared Window Scene Scaffolding

Objective:

- Decouple window chrome from terminal-only content so the app can host
  non-terminal scenes.

Tasks:

- Introduce a scene/content abstraction for Teamy Studio windows.
- Keep the current custom title bar, drag strip, and render loop.
- Make the rendered body switchable between terminal and non-terminal
  scenes.
- Define a shared diagnostics button state and content toggle contract.
- Add a dedicated `windowing.md` spec and include it in Tracey config.

Definition of done:

- One native Teamy Studio window host can render at least terminal and one
  non-terminal scene type.
- Diagnostics toggling is modeled as shared window behavior rather than a
  terminal-only exception.
- The new windowing spec validates cleanly.

### Phase 3: Landing Page Window

Objective:

- Make the default user entry surface a launcher rather than immediately
  dropping into a shell.

Tasks:

- Create a launcher scene with 300x300 image buttons.
- Use compile-time embedded image assets for Terminal and Storage, and add
  the Audio button to the same framework.
- Keep the existing terminal open path as the Terminal button action.
- Add a placeholder dialog for Storage.
- Decide whether bare `teamy-studio` opens the launcher by default while
  keeping a direct terminal-open CLI path intact.

Definition of done:

- Teamy Studio can open into a launcher scene.
- Terminal launches the current shell window flow.
- Storage shows a placeholder response.
- The launcher is not hard-coded for only one future button family.

### Phase 4: Shared Image-Button UX Language

Objective:

- Create one reusable button visual/interaction system for launcher and
  picker windows.

Tasks:

- Define reusable image-button scene data with label, icon, rect, and
  action.
- Introduce button interaction state for neutral, hovered,
  hover-near-anticipation, pressed, and clicked-decay.
- Move picker/button shader logic into a dedicated shader source file or
  clearly separated shader stage plumbing.
- Render icon, label, glow, edge darkening, and time-driven animation from
  shared shader inputs.
- Add tests for layout and hit-testing where practical.

Definition of done:

- Launcher and picker windows use the same button abstraction.
- Button visual state is shader-driven, time-based, and not copied per
  scene.
- Shader concerns are separated from the existing panel shader logic.

### Phase 5: Audio Picker Window

Objective:

- Establish the first non-terminal picker workflow after the launcher.

Tasks:

- Add an audio-source picker window or scene opened from the launcher.
- Present Windows and Pick File as large image buttons with text below.
- Define the meaning of Windows for the first release.
  - likely: OS / packaged sounds or a Windows audio-source flow
- Define the Pick File behavior.
- Reuse the shared diagnostics toggle and image-button framework.

Definition of done:

- Audio opens a picker-style Teamy Studio window.
- The picker uses the shared window language and diagnostics mode.
- The action results are explicit, even if one option remains partially
  stubbed at first.

### Phase 6: Diagnostics Toggle And Orange Panel Rework

Objective:

- Turn diagnostics from a fixed text block into a shared concept across
  windows.

Tasks:

- Repurpose the current top-right plus button into the diagnostics button.
- Make the terminal diagnostics button toggle the auxiliary panel instead
  of spawning a cloned terminal window.
- Define a diagnostics-text representation for every scene.
- Make the pick-window diagnostics view swap the scene contents for that
  text representation.
- Remove or update the stale plus-button product requirement.

Definition of done:

- The top-right affordance has one consistent meaning across window types.
- Terminal windows toggle diagnostics visibility.
- Pick windows can flip into a text scene view.

### Phase 7: Auxiliary Cell-Grid Surface

Objective:

- Replace the current orange string panel with a selectable auxiliary
  cell-grid scene.

Tasks:

- Define a cell-grid-backed auxiliary scene model.
- Reuse or adapt terminal selection semantics for linear and rectangular
  selection.
- Add copy behavior for auxiliary scene selections.
- Decide how zoom and cell density behave for diagnostics / output views.
- Move the current diagnostic text rendering to that new model.

Definition of done:

- The orange panel is no longer just a string render.
- The auxiliary view supports meaningful selection behavior.
- The implementation is reusable for future output/explorer/picker scenes.

### Phase 8: Long-Horizon Window Language

Objective:

- Decide how far Teamy Studio should push a canonical window memory model
  without blocking near-term product wins.

Tasks:

- Evaluate a canonical cell-grid representation per window.
- Evaluate Facet-backed window state and multi-renderer scene views.
- Evaluate event-sourced settings/state history, provenance-aware config,
  and replayable window logs.
- Decide whether paint, brush, tree explorer, and watch-window concepts
  belong in this roadmap or a separate tool-suite roadmap.

Definition of done:

- The long-term direction is written down with explicit yes/no decisions.
- Immediate launcher/picker work is no longer blocked on abstract system
  design.

## Recommended Implementation Order

1. Finish the bell slice and keep it green.
2. Introduce a shared window-scene abstraction without changing the
   terminal PTY core.
3. Add the dedicated windowing spec and update Tracey config.
4. Build the launcher scene and make it capable of opening the current
   terminal window.
5. Build the shared image-button system and separate shader source.
6. Add the audio picker and storage placeholder flow.
7. Repurpose the top-right terminal affordance into diagnostics toggle
   semantics.
8. Replace the orange panel with a real auxiliary cell-grid view.
9. Revisit the broader Facet/cell-grid/event-sourcing architecture after
   the concrete launcher/picker flows exist.

## Open Decisions

- Should bare `teamy-studio` open the launcher by default, or should the
  launcher initially live behind a new CLI entry while the current default
  terminal path remains unchanged?
- Should the launcher and picker scenes live inside the current window host
  process only, or should some of them become separate Teamy Studio window
  classes with their own message-loop plumbing?
- Does the first Audio picker’s Windows option mean packaged system sounds,
  Windows sound packs on disk, or live Windows audio-session selection?
- Should the first diagnostics text view be purely descriptive text, or
  should it already be backed by a reduced cell-grid/x-ray scene model?
- How soon should the current plus-button behavior/spec mismatch be
  corrected relative to the launcher work?
- Is the paint/canvas/brush direction in scope for this roadmap, or should
  it move to a later tool-window plan once the launcher and picker system
  is stable?

## Captured Design Threads

These ideas came up in the design discussion and should stay visible even
if they are not phase-one commitments:

- Every window may eventually have a canonical state object and a lossy
  text representation.
- The state object may eventually be backed by a strongly typed Rust struct
  that implements `Facet`.
- Teamy Studio may eventually support multiple renderers or view modes for
  the same window state, such as raw cell-grid, ratatui-like, and richer
  D3D scenes.
- Event sourcing, provenance-aware config resolution, and replayable log
  history are promising directions for settings and window inspection.
- The same interaction language should eventually scale to menu bars,
  decoration buttons, pick windows, explorer windows, audio pickers, paint
  tools, and richer notebook/workspace windows.

## First Concrete Slice

The next practical slice after the bell work should be:

1. Add a shared window-scene abstraction that can host both terminal and
   launcher content.
2. Add a dedicated `windowing.md` spec and Tracey config entry for the new
   launcher/picker behavior area.
3. Render a static launcher scene with 300x300 Terminal and Storage image
   buttons using compile-time embedded assets and simple hit-testing.
4. Wire the Terminal button to the existing terminal open path and make the
   Storage button show a placeholder dialog.

That slice creates a real non-terminal scene, establishes the spec surface
for the larger refactor, and keeps the audio picker and richer shader work
as follow-up slices instead of blocking the launcher foundation.
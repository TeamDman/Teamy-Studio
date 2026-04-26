# Computer Metaphor And Paint Overlay Plan

## Goal

Grow Teamy Studio from a launcher plus terminal/audio surfaces into a playful, inspectable model of the computer: windows, input devices, files, timelines, shapes, layouts, environment variables, and observed application state should become visible objects that can be inspected, annotated, replayed, and gradually manipulated.

The guiding idea from the source note is that Teamy Studio should not merely mimic Windows, Excalidraw, Graphite, ShareX, Cursor Hero, SFM Draw, or browser developer tools. It should learn from them and rebuild the useful behaviors in Teamy's own mental model: immediate-mode native windows, shader-backed controls, typed event flows, inspectable geometry, visible timelines, and small stubs that preserve future intent inside the application instead of burying it only in notes.

## Current Status

- Done so far:
  - Read the full source note at `C:\Users\TeamD\OneDrive\Documents\Ideas\2026-04-25_18-54-30_mission accomplished.md` without modifying it.
  - Read the referenced subtitle text for the Theo/t3.gg fake GitHub stars video.
  - Read Teamy Studio repo instructions in `AGENTS.md`.
  - Read the existing launcher/window plan in `docs/notes/window-language-and-launcher-plan.md`.
  - Read the current windowing spec in `docs/spec/product/windowing.md`.
  - Inspected the current launcher button structure in `src/app/windows_scene.rs`.
  - Read the SFM Draw roadmap at `D:\Repos\Minecraft\SFM\worktrees\1.19.2-draw\docs\notes\draw_notes.md`.
  - Read Cursor Hero's README, integrated element environment note, and research notes for UI Automation, screen texture, element-tree, game-world, and tool-mode context.
  - Confirmed the current Teamy launcher already has `Terminal`, `Cursor Info`, `Storage`, `Audio`, and `Audio Devices` actions.
  - Confirmed the current windowing spec covers launcher cards, shared diagnostics, and the garden frame, but not paint mode, app-window inspection, input-device inventory, environment variables, shape/SDF tools, timeline replay, or file-extension rules.
  - Fixed the observed microphone-page loopback label overlap by moving the loopback control to its own right-side lane.
  - Changed microphone loopback so it can start a monitor-only capture session even when recording is not active.
  - Validated the microphone-page corrections with `./check-all.ps1`: format, clippy, build, tests, and Tracey status passed.
  - Added Tracey-backed launcher memory stubs for `Environment Variables` and `Application Windows`; each card opens an explicit placeholder dialog instead of silently doing nothing.
  - Validated the launcher-stub slice with `./check-all.ps1`: format, clippy, build, tests, and Tracey status passed. Tracey reported `teamy-studio-windowing/rust: 18 of 18 requirements are covered. 13 of 18 have a verification reference.`
  - Added main-menu keyboard navigation: Left/Up select the previous card, Right/Down/Tab select the next card, and Enter/Space invokes the selected card.
  - Refined main-menu arrow navigation to use a virtual 2D cursor and rendered card hit rectangles, so vertical movement follows the actual wrapped layout instead of assuming fixed grid rows.
  - Refined diagnostics-mode launcher navigation to extract action-row rectangles from the ratatui layout, so arrow keys navigate the visible TUI rows while diagnostics are active instead of continuing to use hidden pretty-card geometry.
  - Replaced the launcher's generic `Alt+X` diagnostics body with a ratatui-style main-menu diagnostics application showing the selected card, action list, and controls.
  - Validated the main-menu keyboard/diagnostics slice with `./check-all.ps1`: format, clippy, build, tests, and Tracey status passed. Tracey reported `teamy-studio-windowing/rust: 20 of 20 requirements are covered. 15 of 20 have a verification reference.`
  - Added a visible virtual cursor pointer using enlarged, tinted OS cursor sprites in the renderer atlas.
  - Added a `Cursor Gallery` launcher item for inspecting the stock OS cursor sprites used by the virtual cursor path.
  - Extended the Cursor Gallery direction: gallery cells should be navigable shapes, hover should change both the native cursor and virtual cursor shape, and hover glow should use the gallery cell color.
  - Documented the future SDF/shader cursor path: stock cursor bitmaps can later become edge, distance-field, or curve data for stylized shader rendering.
  - Validated the cursor sprite/gallery slice with `./check-all.ps1`: format, clippy, build, tests, and Tracey status passed. Tracey reported `teamy-studio-windowing/rust: 24 of 24 requirements are covered. 18 of 24 have a verification reference.`
  - Implemented the cursor-gallery interaction slice: arrow keys and Tab move the virtual cursor between cursor cells, physical mouse hover changes the native cursor shape, the virtual pointer uses the selected/hovered cursor sprite, and selected/hovered cells glow with the gallery color.
  - Validated the cursor-gallery interaction slice with `./check-all.ps1`: format, clippy, build, tests, and Tracey status passed. Tracey reported `teamy-studio-windowing/rust: 27 of 27 requirements are covered. 20 of 27 have a verification reference.`
  - Added the Demo Mode launcher/window slice with realistic Arbitrary-backed fake input-device identifiers for privacy-preserving demos.
  - Replaced the Demo Mode scramble checkbox direction with a shader-animated toggle direction, including native hover text and keyboard virtual-cursor tooltip behavior.
  - Made the Demo Mode scramble toggle persist to `demo-mode.txt` under the application home directory and broadcast live updates to open audio-device windows so endpoint IDs redraw as scrambled/unscrambled without reopening those windows.
  - Validated the Demo Mode persistence/live-update slice with `./check-all.ps1`: format, clippy, build, tests, and Tracey status passed. Tracey reported `teamy-studio-windowing/rust: 33 of 33 requirements are covered. 26 of 33 have a verification reference.`
  - Added a left-edge chrome pin affordance direction: Teamy windows can be pinned above other windows, and the shared chrome renderer now has a dedicated pin state/icon surface.
  - Corrected Demo Mode fake input-device IDs so they preserve realistic endpoint shape without adding the `SWD\MMDEVAPI\` prefix when the obscured values do not have it.
  - Began extending selectable text beyond diagnostics by making the selected microphone details render through the same text-grid selection model used for copyable diagnostics text.
  - Finished and validated the microphone transport polish slice: the selected microphone window now has a shader-rendered play/pause button, scene-window Ctrl+C copies active selectable text, and scene-window Ctrl+D closes the window.
  - Validated the microphone playback/scene-shortcut slice with `./check-all.ps1`: format, clippy, build, tests, and Tracey status passed. Tracey reported `teamy-studio-windowing/rust: 37 of 37 requirements are covered. 30 of 37 have a verification reference.`
  - Redirected the immediate next work from paint chrome to transcription: the selected microphone window now hosts a transcription toggle, mel-preview region, and terminal-styled transcript island below the audio buffer.
  - Started Python transcription integration with a Rust fixed-shape log-mel tensor contract, `audio daemon status`, and a Teamy-owned Python daemon scaffold.
  - Added a visible `Audio Daemon` main-menu entry and daemon scene window, including a full ratatui diagnostics view for paths, transports, payload shape, shared-memory pool sizing, and queue counters.
  - Added the first real Rust-owned shared-memory slot pool for transcription payloads, including Windows file mappings, ready-request queueing, elastic growth, status counters, and slot release.
  - Added the versioned Rust/Python JSONL control-message contract that will run over the named pipe, including queued slot requests, daemon result lines, and slot-release instructions.
  - Added the first tested Rust-side live named-pipe transport for one transcription request/result roundtrip.
  - Connected the Python daemon scaffold to the named-pipe debug path, including validation of a real Rust-created shared-memory slot and return of a slot-release result.
  - Added the Rust-side result-staging hook so returned daemon text can appear in the microphone transcript island while released slots return to the pool.
  - Added the first app-side debug transcription tick, letting the mic window launch the Python pipe path off the UI thread and stage the returned debug transcript text.
- Current focus:
  - Use the microphone timeline, shared-memory slot pool, named-pipe Python handoff, and daemon dashboard as the first real proving ground for captured-audio feature preparation.
- Remaining work:
  - Add Tracey requirements for each new behavior area before implementation lands.
  - Add shallow launcher stubs for the new surfaces.
  - Implement function-key teleport bindings for F1 through F12, including hold-to-bind, tap-to-focus, sound feedback, and undoable jump-history entries.
  - Build the first paint overlay mode for all Teamy windows.
  - Add application-window inspection using the Cursor Hero lessons.
  - Add environment variable, input-device, file-extension, shape, SDF, and timeline explorer surfaces as progressively richer windows.
- Next step:
  - Continue in `docs/notes/audio-input-inbox-plan.md` with Rust log-mel preparation from recorded samples before returning to paint chrome.

## Source Ideas Extracted

### Trust, Stars, And Vouching

The xkcd 810 thread and the fake GitHub stars video point at the same core shape: when a metric becomes valuable, agents learn to look valuable. Stars are a weak vouch because the UI collapses account quality, history, intent, and adoption evidence into one unannotated number.

Teamy-relevant extraction:

- Build tools that preserve reasons, provenance, and confidence instead of flattening them too early.
- Treat `who said this`, `why do I believe it`, and `what evidence supports it` as first-class inspectable data.
- Prefer richer local analysis over single-number trust signals.
- Keep room for reputation overlays and user-published opinions later, but do not start by building a social network.

### Screenshots As Requirements

The source note says that when the user describes what is wrong, the agent should insist on an annotated screenshot. The attached microphone screenshot proves the value: it made the loopback layout bug immediately concrete.

Teamy-relevant extraction:

- Screenshot capture, annotation, and replay should become a native workflow.
- A screenshot should be able to point back to rendered panels, glyphs, sprites, hit boxes, and scene data when Teamy owns the window.
- External-window screenshots should be decomposed best-effort through UI Automation, window bounds, OCR/vision, and image analysis.

### Paint Overlay And Mesh Reveal

The note asks for a paint button in the chrome, to the left of `Show diagnostics`, using the Paint shortcut icon, dimmed when inactive and normal when active. Paint mode should draw on top of the current app, intercept keyboard input first, and reveal meshes/elements for the current window.

Teamy-relevant extraction:

- Paint overlay is the next unifying affordance after diagnostics.
- Diagnostics explains state as text/TUI; paint mode explains state spatially.
- Paint mode should be available on every Teamy window.
- The first version can be simple: chrome button, mode toggle, overlay layer, visible panel bounds, and a small brush/selection tool.

### Spatial Partitioning And Layout Strategies

The note connects Excalidraw, Graphite, SFM Draw, Hyprland, Clay, ratatui, CSS-like layout, and Teamy's current scene layout code. The user's phrase `position things as if by play` is the product direction: layout should become inspectable and adjustable, not just hardcoded rectangles.

Teamy-relevant extraction:

- Add a launcher entry for `Spatial Partitioning Strategies`.
- Start as a picker/stub window with cards for Excalidraw, SFM Draw, Hyprland, Graphite, Clay, ratatui, and Teamy native layout.
- Gradually turn hardcoded layout regions into inspectable layout nodes.
- Let paint mode reveal these layout nodes over the rendered scene.

### File Mapping And Operating System Relationship

The note identifies file extension rules, browser save barriers, and program associations as part of rebuilding the user's relationship with the operating system.

Teamy-relevant extraction:

- Add a launcher entry for `File Extension Rules`.
- Start with a read-only view of known extension associations where feasible.
- Later, model rules such as `when I Ctrl+S this website diagram, what local file shape should it become?`

### Environment Variables

The screenshots include Windows System Properties and Environment Variables dialogs. The note asks for a Teamy window that impersonates the structure, not styling, of the Windows dialogs to build understanding of the codebase/computer relationship.

Teamy-relevant extraction:

- Add a launcher entry for `Environment Variables`.
- First window should show user/system variable tables and selected details.
- Do not edit variables in the first slice; inspect first, mutate later.
- This is a good early target because it is concrete, useful, and OS-shaped without requiring a giant architecture.

### Input Devices

The note asks for an `Input Devices` button that lists mice, keyboards, USB sticks, and other plugged-in devices with names and icons, preferably from the OS.

Teamy-relevant extraction:

- Generalize lessons from audio input device enumeration.
- Add an `Input Devices` launcher entry.
- First slice can enumerate simple Windows device classes read-only.
- Later slices can attach per-device input interpretation rules: `how is this signal interpreted?`

### Application Windows

The note points at Cursor Hero and asks for an `Application Windows` surface with a tree view of open windows, image preview, and a computed-style panel for positioning rules.

Cursor Hero already explored:

- Windows UI Automation tree extraction.
- Window/screen texture capture into a world model.
- UI element bounds and drill ids.
- A tree view with future properties panel.
- Process/icon extraction through the Windows crate.
- Tool modes and input-mode switching.

Teamy-relevant extraction:

- Add an `Application Windows` launcher entry.
- First slice: list top-level windows with process name, title, rect, and icon if available.
- Second slice: preview selected window image.
- Third slice: UI Automation tree and computed geometry/properties panel.
- Long-term: use this as the external-window analog to Teamy's own paint-mode mesh reveal.

### Shapes, SDFs, And Cellular Editors

The note asks for `Shapes`, `SDF Explorer`, and a level/cellular editor for distance fields, including Chebyshev distance fields.

Teamy-relevant extraction:

- Add a `Shapes` launcher entry for basic triangle/circle/square composition.
- Add an `SDF Explorer` launcher entry.
- Keep the first slices small and shader-backed: show a generated field, inspect sample values, and expose a couple of editable parameters.
- Acronyms on screen must have hover definitions; if a tooltip contains an acronym, the tooltip itself should be hoverable.

### Timeline, Replay, And Temporal Capture

The note links screenshots, screen recording buffers, ShadowPlay/action replay, audio waveforms, event logs, Braid-like time manipulation, and scrubbers.

Teamy-relevant extraction:

- Add a long-term `Timeline` concept shared by microphone buffers, event logs, screenshot history, and replay buffers.
- The microphone waveform is already a 1D timeline; use it as the first real proving ground.
- Later, build a screenshot/event replay buffer where the user can capture the screen from a few seconds ago.
- Model streams as event producers and timelines as indexed views over those event streams.

### Function-Key Teleport And Event History

The latest interaction direction treats the virtual cursor as a place the user can bind, revisit, and undo like an edit in a paint application.

Teamy-relevant extraction:

- F1 through F12 should become window/cursor teleport slots.
- Holding a function key for one second should bind that key to the current focused Teamy window and virtual cursor position, then play an audio confirmation.
- Tapping a bound function key should focus the bound window and move that window's virtual cursor to the saved point.
- The main menu opening flow should support binding F1 to the launcher, opening Cursor Gallery, binding F2 to that newly focused window, then tapping F1 or F2 to jump between them.
- Event listener priority should define teleportable areas: scene controls, diagnostics/panels, paint overlay, terminal content, and finally window chrome. Window chrome should be the last keyboard listener and keep owning OS-level behavior such as Alt+F4.
- If function keys also deliver character-like messages on some paths, Teamy should model that explicitly instead of treating those messages as accidental noise.
- Every teleport should append an undoable history entry such as `jumped to cursor X from position Y`.
- The history should become an edit timeline similar to Paint.NET: visible, replayable, and eventually undo/redo capable.

### Trink: Units, Dimensions, And Typed Transformations

The note references Frink, `uom`, `sguaba`, unit conversion, dimensions, shapes, JSON/schema guessing, Facet, jq, and transformation languages. The proposed name is `Trink`.

Teamy-relevant extraction:

- Treat `Trink` as a future expression language for units, dimensions, shapes, and transformations.
- Do not start by inventing a full language.
- Start by using Rust types/newtypes and existing crates (`uom`, `sguaba`, `facet`) to make internal representations explicit.
- Add a design spike later for whether `Trink` is a CLI, a mini expression evaluator, or just a naming umbrella for typed transforms.

## Constraints And Assumptions

- Do not edit the raw source note. It is captured source material.
- Teamy Studio is currently a Rust 2024 Windows app with a custom Win32/D3D12 renderer and scene-window model.
- Current launcher button definitions live in `src/app/windows_scene.rs` as `SceneWindowKind`, `SceneAction`, and `scene_button_specs`.
- Current scene event dispatch lives primarily in `src/app/windows_app.rs`.
- Current panel effects and sprites live in `src/app/windows_d3d12_renderer.rs` and HLSL shader files.
- Tracey is the observable behavior contract. New user-facing surfaces should get requirements before or alongside implementation.
- Use `./check-all.ps1` for validation, not direct `cargo check`.
- New CLI subcommands, when added, must follow the repo rule: each subcommand has its own directory module and `*_cli.rs` file re-exported by `mod.rs`.
- Prefer shallow visible stubs over invisible backlog items, but each stub should say what is unsupported rather than silently doing nothing.
- Use OS-provided icons where practical, but do not block initial stubs on perfect icon extraction.
- External-window inspection must be read-only at first. Mutating other apps is a later and more sensitive phase.

## Product Requirements

### Committed Near-Term Requirements

- Teamy Studio must add a paint-mode chrome button to every Teamy window, positioned to the left of the diagnostics button.
- The paint button should use the Windows Paint shortcut icon when available, with a fallback icon if resolution fails.
- Paint mode must visibly toggle between inactive and active states.
- Paint mode must draw an overlay above the current Teamy window content.
- Paint mode must reveal Teamy-owned scene geometry: panels, sprites, glyph bounds, button hit regions, and layout partitions.
- Paint mode keyboard input must be routed to the overlay before default window behavior.
- Teamy Studio must add shallow launcher entries for future surfaces that the note identifies as worth preserving.
- Unsupported launcher entries must open a real placeholder scene or message, not do nothing.
- Acronyms shown in new UI text must provide hover definitions.
- Tooltips introduced for these surfaces should be traversable or otherwise not disappear immediately when the cursor moves toward them.

### Planned Launcher Entries

- `Paint Overlay` or chrome-only paint mode, depending on final UX.
- `Spatial Partitioning Strategies`.
- `File Extension Rules`.
- `Environment Variables`.
- `Input Devices`.
- `Application Windows`.
- `Shapes`.
- `SDF Explorer`.
- `Timeline` or `Replay Buffer`.
- `Trust Signals` or `Vouching`, for the GitHub-stars/reputation-analysis thread.

### Deferred Requirements

- Full Excalidraw/Graphite-level drawing functionality.
- Full ShareX replacement.
- Screen recording and rewind capture.
- Mutating external application windows.
- A generalized typed event bus for all input devices.
- A complete Trink language.
- A complete Bevy-like ECS inside Teamy Studio.
- Social publication of likes, trust labels, or scammer labels.
- Privacy-preserving anonymous publishing protocols.

## Architectural Direction

### Layer 1: Launcher Stubs

Keep the first move humble: add the missing entry points so the application remembers the ideas in its own interface. Each entry can initially open a placeholder scene with a short, concrete statement of intended behavior.

This converts notes into visible product affordances without committing to a large backend too early.

### Layer 2: Shared Chrome Modes

Add paint mode beside diagnostics mode.

Diagnostics mode is text/TUI-oriented. Paint mode is spatial/visual. Both should be shared window capabilities, not one-off scene features.

Likely state additions:

- `paint_overlay_visible` or `paint_mode_active` in terminal and scene app state.
- `WindowChromeButton::Paint` beside `Diagnostics`.
- `WindowChromeButtonsState.paint` beside diagnostics/minimize/maximize/close.
- `PanelEffect::WindowChromePaint` and shader icon routing.
- A paint overlay render pass or overlay panels in the existing scene model.

### Layer 3: Teamy-Owned Geometry Introspection

Before inspecting other apps, inspect Teamy itself.

Expose current render-scene objects as overlay geometry:

- panel rects
- glyph rects
- sprite rects
- scene button hit rects
- waveform head grabbers
- window chrome button rects
- layout partition rects

The first paint overlay should be able to show these as selectable outlines.

### Layer 4: External Window Inspection

Use Cursor Hero as the prior-art bridge:

- top-level window enumeration
- process path and icon extraction
- window rects and monitor placement
- screenshot/preview capture
- UI Automation tree
- selected element properties panel

This becomes Teamy's `Application Windows` surface.

### Layer 5: Timeline And Replay

Unify event streams, audio buffers, screenshots, and UI observations under a timeline model. The current microphone waveform is the first practical timeline. Do not generalize too far until the audio path proves the interaction model.

### Layer 6: Typed Shapes, Units, And Transformations

Use Rust newtypes, Facet, `uom`, and `sguaba` as the first language. Introduce `Trink` only when repeated unit/shape transformations need a textual expression layer.

## Tracey Specification Strategy

This plan introduces multiple behavior areas. It should not all be stuffed into the existing `windowing.md` spec.

Recommended spec split:

- Extend `docs/spec/product/windowing.md` for shared chrome paint button behavior and launcher stubs.
- Add `docs/spec/product/paint-overlay.md` when paint mode grows beyond the first chrome toggle and overlay outlines.
- Add `docs/spec/product/application-windows.md` for external window enumeration and inspection.
- Add `docs/spec/product/input-devices.md` for general device inventory.
- Add `docs/spec/product/environment.md` for environment variable inspection and eventual editing.
- Add `docs/spec/product/timeline.md` when replay buffers and cross-stream timelines become real.
- Add `docs/spec/product/shapes.md` or `docs/spec/product/spatial.md` for shape/SDF/spatial partitioning tools.

Baseline workflow:

```powershell
tracey query status
tracey query uncovered
tracey query unmapped
tracey query unmapped --path src/app/windows_scene.rs
tracey query unmapped --path src/app/windows_app.rs
tracey query unmapped --path src/app/windows_d3d12_renderer.rs
tracey query validate --deny warnings
```

After implementation coverage is under control:

```powershell
tracey query untested
```

Tracey rule for this roadmap:

- Add requirements before adding each visible launcher stub.
- Map implementation references as each stub or behavior lands.
- Do not solve all repo-wide unmapped debt as part of this work unless it directly blocks the touched behavior.

## Phased Task Breakdown

### Phase 0: Microphone Page Corrections

Objective:

- Resolve defects discovered while using the current microphone page.

Tasks:

- Move the loopback control far enough right that its status label does not overlap `Recording` / `Not recording`.
- Make loopback monitoring work when recording is inactive.
- Update audio-input spec text if loopback semantics change.
- Validate with `./check-all.ps1`.

Definition of done:

- The microphone page has separated record and loopback labels.
- Enabling loopback without recording starts monitor playback from the selected microphone.
- Recording still appends to the buffer when recording is active.
- `./check-all.ps1` passes.

### Phase 1: Launcher Memory Stubs

Objective:

- Preserve the major source-note ideas in Teamy's interface as shallow but real entry points.

Tasks:

- Extend `SceneAction` with new stub actions.
- Extend launcher `scene_button_specs` with a conservative first set:
  - `Environment Variables`
  - `Input Devices`
  - `Application Windows`
  - `Shapes`
  - `SDF Explorer`
  - `File Extension Rules`
  - `Spatial Layouts`
- Add placeholder action handling in `perform_scene_action`.
- Add or reuse a generic placeholder scene/dialog function.
- Add windowing spec requirements for visible stubs.
- Add focused tests that the launcher exposes the new specs.

Definition of done:

- New launcher cards render.
- Clicking each unsupported card produces an explicit placeholder response.
- Tracey validates with no warnings.
- `./check-all.ps1` passes.

Status:

- Completed for the first two cards: `Environment Variables` and `Application Windows`.
- Added to the launcher imagery todo list: replace the PNG-backed imagery for `Terminal`, `Cursor Info`, `Storage`, `Environment Variables`, and `Application Windows` with code-generated imagery; intentionally leave `Audio` and `Audio Devices` on OS imagery during that change.
- Remaining launcher memory cards are deferred until the first paint-mode slice lands.

### Phase 1.5: Main Menu Keyboard And Diagnostics Polish

Objective:

- Make the launcher usable without the mouse and make its diagnostics mode feel like the richer ratatui diagnostics used by the audio surfaces.

Tasks:

- Add keyboard selection state for launcher cards.
- Let arrow keys move a virtual 2D cursor through rendered card hit rectangles, choosing the next card by geometry rather than assuming a perfect fixed grid.
- Keep Tab as sequential traversal and let Enter/Space invoke the selected card.
- Render the selected launcher card with a visible active treatment.
- Replace the launcher's plain diagnostics text body with a ratatui-style main-menu diagnostics application.
- Keep `Alt+X` as the diagnostics toggle.

Definition of done:

- The launcher can be navigated and invoked from the keyboard.
- `Alt+X` on the launcher shows a structured diagnostics TUI with selected action, action list, and controls.
- `./check-all.ps1` passes.

Status:

- Completed. The selected card is highlighted in the pretty launcher, arrow keys navigate by rendered geometry through a virtual 2D cursor, `Alt+X` opens the ratatui-style diagnostics body, and diagnostics-mode navigation uses ratatui action-row shapes instead of hidden pretty-card geometry.

### Phase 1.6: Inspectable Virtual Cursor And Shape Navigation

Objective:

- Generalize the launcher keyboard navigation model into an inspectable virtual cursor that can later power keyboard whiteboarding, paint overlay navigation, and cursor-info shape inspection.

Tasks:

- Promote the launcher's private action-rectangle navigation into a reusable shape-navigation module.
- Represent navigable targets as simple shapes first: rectangles with centroids, hit regions, and nearest-perimeter queries.
- Expose the virtual cursor point and current hovered target in diagnostics.
- Draw the virtual cursor as a visible, enlarged, tinted pointer using OS cursor sprites in the renderer atlas.
- Add a Cursor Gallery launcher item that shows the stock OS cursor sprite sheet for debugging.
- In the Cursor Gallery, make the OS cursor cells navigable by the virtual cursor, make the native cursor take the hovered cell's cursor shape, and glow selected/hovered cells using the cell's gallery color.
- Let a keyboard command click the target under the virtual cursor.
- Add cursor-info or paint-overlay diagnostics that can show the navigable shape set for the current window.
- Add later motion primitives such as `jump to the beginning of the line below the bottom of the ratatui block containing the cursor` once block, line, and text-run shapes are exposed.
- Keep the longer-term cursor rendering route open: bake ground-truth SDFs or curve data for basic OS cursors, and eventually let a cursor shader render stylized silhouettes from edge/distance/shape data rather than relying only on tinted cursor bitmaps.
- Add a Teamy-style custom color picker modeled after the Windows text-cursor custom color picker: a hue/saturation field, a value/brightness rail, draggable pucks controlled by the virtual cursor, compact RGB/HSV/hex editor islands, and a design-language-compatible `Done`/`Cancel` footer.
- Treat editable text islands as embedded terminal sessions that can run a future `edit -` command; Enter focuses the island, then three Escape presses return to the virtual-pointer keyboard layer.
- Add an Event Bus diagnostic surface that shows the known input-routing order for keyboard and mouse as a spatial TUI map with legend and text area, using the `ratatui-key-debug` triple-Escape behavior as a reference for layer escape semantics.
- Explore a stethoscope observer window: a draggable probe can attach to a desktop-coordinate point, reports z-ordered windows occupying that point, and uses `sguaba` coordinate types for desktop/window/client-space clarity.

Definition of done:

- Keyboard navigation can operate over a list of drawn shapes without knowing whether they came from a grid, wrapped flow, whiteboard, or app inspector.
- The virtual cursor point and chosen target are visible in diagnostics.
- The launcher still uses the shared model.
- `./check-all.ps1` passes.

### Phase 2: Paint Chrome Button

Objective:

- Add the shared paint-mode affordance to every Teamy window.

Tasks:

- Resolve the Paint shortcut icon from `C:\ProgramData\Microsoft\Windows\Start Menu\Programs\Accessories\Paint.lnk` or provide a fallback.
- Add `WindowChromeButton::Paint` and associated hit testing, tooltip, cursor, visual state, and click handling.
- Place the paint button to the left of diagnostics.
- Add a paint-active state to terminal and scene windows.
- Render paint button dimmed when inactive and normal/colorful when active.
- Add Tracey requirements for shared paint chrome.

Definition of done:

- Every Teamy window shows a paint button beside diagnostics.
- The paint button toggles state and updates its visual treatment.
- The button has a tooltip and keyboard-accessible behavior plan recorded.
- `./check-all.ps1` passes.

### Phase 3: Teamy Mesh Reveal Overlay

Objective:

- Make paint mode useful by revealing Teamy-owned geometry.

Tasks:

- Add a paint overlay scene layer above existing content.
- Draw outlines for panels, sprites, glyphs, chrome buttons, scene cards, and hit rects.
- Add hover/selection of overlay items.
- Display a small inspector label for the selected element: kind, rect, effect, action if known.
- Route keyboard input to paint mode first while active.
- Add a safe escape path to leave paint mode.

Definition of done:

- Paint mode overlays geometry on launcher, audio picker, microphone detail, and terminal windows.
- Hovering or selecting an overlay item identifies it.
- Normal window behavior resumes when paint mode is disabled.
- `./check-all.ps1` passes.

### Phase 4: Environment Variables Window

Objective:

- Build the first OS-shaped inspection window requested by the note.

Tasks:

- Add an `Environment Variables` scene kind.
- Read user and process environment variables first; system variables can be added after the safe read-only path is clear.
- Render two table-like regions inspired by the Windows dialog structure but using Teamy's visual language.
- Add selected-variable details.
- Keep editing disabled in the first slice.

Definition of done:

- Launcher opens an environment variables window.
- The window shows variables and values read-only.
- Diagnostics mode explains counts and selection.
- `./check-all.ps1` passes.

### Phase 5: Application Windows Inspector

Objective:

- Recreate the useful Cursor Hero window-inspection path inside Teamy Studio.

Tasks:

- Add top-level window enumeration.
- Show a left tree/list of windows with title, process, rect, and icon if available.
- Show selected window geometry details.
- Add screenshot preview after basic enumeration is stable.
- Add UI Automation tree after preview is stable.
- Add Tracey spec for application-window inspection.

Definition of done:

- Launcher opens an Application Windows window.
- It lists top-level windows and selected geometry.
- It is read-only and safe.
- `./check-all.ps1` passes.

### Phase 6: Input Devices Inventory

Objective:

- Generalize the audio-device inventory idea to the broader computer.

Tasks:

- Add a read-only input-device window.
- Start with Windows device enumeration for keyboards, mice, and removable drives if practical.
- Render device name, class, state, and icon/fallback icon.
- Record future input-interpretation rules as deferred.

Definition of done:

- Launcher opens Input Devices.
- Devices are visible read-only with clear unavailable states.
- `./check-all.ps1` passes.

### Phase 7: Spatial Layouts, Shapes, And SDF Explorer

Objective:

- Begin the visual math/tooling branch without blocking core OS inspection work.

Tasks:

- Add a Spatial Layouts picker with cards for Teamy native, ratatui, Clay, Hyprland, Excalidraw, Graphite, and SFM Draw references.
- Add a Shapes window that renders simple shader-backed triangle/circle/square examples.
- Add an SDF Explorer stub that defines SDF on hover and displays at least one generated field.
- Add acronym tooltip behavior for SDF.

Definition of done:

- The surfaces exist as visible stubs or first demos.
- Acronyms have hover definitions.
- `./check-all.ps1` passes.

### Phase 8: Timeline And Replay Research Slice

Objective:

- Connect audio timelines, event logs, screenshots, and replay buffers into one implementation path.

Tasks:

- Document current timeline models in microphone recording.
- Add a timeline spec covering event streams, scrubbers, and replay buffers.
- Prototype an in-memory event timeline for Teamy-owned UI events.
- Defer screen replay until event timeline storage is understood.

Definition of done:

- A timeline spec exists.
- A small Teamy-owned event stream can be inspected or rendered.
- The microphone timeline remains compatible with the direction.

### Phase 9: Trink Design Spike

Objective:

- Decide whether `Trink` is a real language, a CLI, a library concept, or a name for typed transforms.

Tasks:

- Compare Frink's unit model with current Teamy use of `uom` and `sguaba`.
- Identify three concrete transformations Teamy needs.
- Decide whether Rust APIs are enough for now.
- If not, sketch the smallest text expression grammar.

Definition of done:

- A short design note exists with a recommendation.
- No broad language implementation starts without concrete use cases.

## Recommended Implementation Order

1. Finish Phase 0 microphone corrections and validation.
2. Add launcher memory stubs for the new surfaces.
3. Add paint chrome button.
4. Add Teamy mesh reveal overlay.
5. Build Environment Variables as the first OS-shaped inspector.
6. Build Application Windows as the Cursor-Hero-derived inspector.
7. Add Input Devices.
8. Add Shapes/SDF/Spatial Layout demos.
9. Return to Timeline/Replay and Trink once enough real surfaces exist.

## Open Decisions

- Should paint mode be purely a chrome mode, or should there also be a launcher card for it?
- Should the paint button use the actual Paint shortcut icon at runtime, or should Teamy cache/extract a Paint-like icon into resources?
- How much of paint mode should be persisted as annotations versus ephemeral overlay state?
- Should Teamy use UI Automation directly for Application Windows, or first re-port a subset of Cursor Hero's existing window model?
- What is the minimum safe screenshot/preview path for external windows?
- Should Environment Variables read system variables through registry APIs, PowerShell-compatible APIs, or process environment first only?
- Should `Input Devices` include storage devices in the first slice, or should storage/removable media live under a separate Storage surface?
- Should `Trink` be its own future repo/crate or remain a Teamy-internal concept until it earns extraction?

## First Concrete Slice

The first implementation slice after this plan was completed:

1. Finished and validated the microphone loopback fixes from Phase 0.
2. Added Tracey requirements for launcher memory stubs in `docs/spec/product/windowing.md`.
3. Added two launcher stubs:
   - `Environment Variables`
   - `Application Windows`
4. Made each stub open an explicit placeholder dialog.
5. Ran `./check-all.ps1` successfully.

Why these two first:

- `Environment Variables` is concrete and maps directly to the attached Windows dialogs.
- `Application Windows` connects directly to prior Cursor Hero work and the user's desire to inspect other windows.
- Both reinforce the central metaphor: Teamy Studio is becoming a tool for seeing the computer as structured, inspectable, manipulable state.

## Next Concrete Slice

Add the first shared paint-mode chrome button:

1. Add windowing or paint-overlay Tracey requirements for a shared paint button beside diagnostics.
2. Add a `WindowChromeButton::Paint` path, visual state, hit testing, tooltip, and click handling.
3. Add state for whether paint mode is active in scene windows.
4. Render the button inactive/active, using a fallback shader/icon if Paint shortcut extraction is not ready yet.
5. Keep overlay drawing deferred unless the button/state path is already stable.
6. Run `./check-all.ps1`.

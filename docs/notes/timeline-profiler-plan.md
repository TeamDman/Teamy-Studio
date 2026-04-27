# Timeline And Tracy Capture Viewer Plan

## Goal

Build a Teamy-owned timeline framework that can start as a Tracy capture viewer and grow into the audio recording/editing surface for microphone tracks, transcription regions, selections, and real-time processing state.

The first proof should let the launcher open a Timeline flow, create an empty timeline, or load a `.tracy` capture. Loading a Tracy capture should prove that Teamy can parse nanosecond-precision spans/messages and render them in an interactive, zoomable, pannable timeline window. Later slices should reuse the same timeline model for microphone tracks, live sample buffers, audio clips, transcription work regions, and DAW-style editing interactions.

## Current Status

- Done so far:
  - Added the dedicated timeline product spec and kept Tracey mappings current; `tracey query status` now reports `teamy-studio-timeline` at 34 of 34 requirements covered.
  - Landed the Timeline launcher/start workflow, blank document creation, and `.tracy` import flow with append-to-existing-track behavior for imported tracing captures.
  - Reworked `src/timeline/mod.rs` into a typed timeline model with integer nanosecond time, typed viewport projection, ruler ticks, track projections, non-destructive edit placeholders, and append APIs for both tracing and audio tracks.
  - Implemented document-backed viewport controls, keyboard panning/zooming, cursor-anchored mouse-wheel zoom, right-drag panning, visible scrollbars, rectangular marquee selection, and ruler-driven all-track selection.
  - Integrated the timeline scene with the existing live audio runtime from `src/app/windows_audio_input.rs` instead of creating a second transport implementation.
  - Added a microphone-backed timeline row that shows the live device name, device icon, and record button in the left tool panel.
  - Added a transport tool panel with a play/pause button, spacebar transport toggle, and visible draggable recording/playback heads aligned to the timeline ruler.
  - Added animated elastic mouse-wheel zoom so the timeline eases between the current and target viewport instead of snapping.
  - Fixed timeline clip projection so zooming and panning clip partially visible blocks at the viewport edge instead of visually compressing them into the available width.
  - Added a first text-track milestone: text tracks can be added from the add-track picker, the top toolbar now includes Select and Box tools, drag-brushing on a text track creates empty document-owned text boxes, and hovering a text box shows its current contents in a tooltip.
  - Updated tests and validation so `./check-all.ps1` passes with the timeline microphone/transport/text-box slice.
- Current focus:
  - Turn the new text-track box workflow into a real editing surface while preparing the shared timeline-session architecture needed for detached tool windows.
- Remaining work:
  - Add pinned text editing windows with cursor movement, primitive cell editing, and the requested word-navigation/delete shortcuts instead of the current hover-only tooltip preview.
  - Persist recorded timeline audio as document-owned clip/source state instead of reflecting only the live `AudioInputDeviceWindowState` runtime.
  - Introduce a shared timeline-session object so detached toolbar, track-list, and editor windows can bind to one document instead of each scene window owning isolated state.
  - Implement the Rust Tracy capture reader and zone/message rendering path beyond the current file-header/track append workflow.
  - Extend the timeline model to carry richer text documents, source media references, transcription regions, and later editing primitives.
- Next step:
  - Add a minimal pinned text editor window backed by `TimelineDocument` text blocks, then refactor the timeline surfaces toward a shared session model before splitting the toolbar and track list into detached windows.

## Constraints And Assumptions

- Teamy is MPL-2.0. Tracy is BSD-licensed. The plan is to reimplement the needed behavior in Rust and use Tracy as a reference for concepts and file layout. If any Tracy source text or substantial code is copied instead of reimplemented, preserve the required BSD notice and review the licensing boundary deliberately.
- The timeline core must store time in integer nanoseconds, not `f64`, so Tracy captures and later audio/transcription ranges do not lose precision. UI projection can convert to `f64` only at the viewport/rendering boundary.
- The first viewer should target `.tracy` files produced by the Tracy version currently in the workspace. Backward-compatible loading can come after the first local capture works.
- The first UI should use Teamy scene windows and renderer primitives rather than introducing ImGui or Tracy UI code.
- The new timeline behavior is a new product surface, not a narrow extension of audio input or windowing. It should get a dedicated Tracey spec.
- The existing microphone detail timeline is useful prior art, but it is a single-device surface. The new timeline should become a multi-track document/view model that audio input can feed into later.
- Separate detached tool windows are not a cheap follow-up in the current scene architecture. Each scene window owns thread-local state today, so detached timeline tools require an explicit shared timeline-session layer instead of more ad hoc window state plumbing.
- The viewer should be able to load large captures incrementally or with indexed summaries eventually. The first slice may parse a bounded subset, but the architecture should not require materializing every visual primitive for every frame.
- `.tracy` captures may contain source paths, messages, symbols, and other sensitive data. The UI should treat loaded captures as local files and avoid accidental export or logging of payload text.

## Product Requirements

- The launcher must expose a Timeline action card.
- The Timeline action must open a dedicated Timeline window.
- The Timeline window must let the user choose between creating a new empty Teamy timeline and loading a Tracy capture.
- An empty Teamy timeline must show a track list area, an add-track affordance, a time ruler, and an empty content area.
- Loading a `.tracy` capture must create a read-only timeline document with tracks for CPU threads and at least one message/metadata lane.
- Tracy zones must preserve nanosecond start/end times internally and render with stable horizontal placement at different zoom levels.
- Tracy messages must render as point events or annotations at their capture timestamp.
- The timeline viewport must support pan and zoom without changing the underlying timeline data.
- The track list must support adding a microphone-derived track in a later slice by reusing the existing microphone picker.
- The current timeline UI should use in-window tool panels for track list, transport, and viewport controls until a shared detached-window session architecture exists.
- Live microphone tracks must expose device identity, recording control, and draggable transport heads in the timeline itself.
- Text tracks must support document-owned text boxes with explicit time ranges, hover previews, and later pinned editing.
- The timeline must support a box-authoring tool that creates empty text boxes by dragging within a text track lane.
- Recording from the timeline should become durable document state rather than staying a purely live runtime projection.
- A microphone track must eventually map one source medium, such as a live sample buffer or recorded clip, into one or more track lanes depending on mono/stereo channel count.
- Audio clips, Tracy zones, transcription work ranges, and user selections should share one interval/marker vocabulary so the timeline can visualize what is being recorded, processed, transcribed, or selected.

## Architectural Direction

Use three layers and keep them separate:

1. Timeline document model:
   - `TimelineDocument` owns tracks, sources, clips/events, selections, and metadata.
   - `TimelineTime` stores integer nanoseconds.
  - Tracks are typed: Tracy thread, Tracy messages, audio channel, transcription work, text documents, derived analysis, or placeholder.
   - Source media is separate from track clips. A clip is a view into source data, so later audio editing can move/slice clips without duplicating the original recording buffer.

2. Import/adapters:
   - `timeline::tracy` reads `.tracy` captures into a normalized timeline document.
   - `timeline::audio` adapts microphone capture buffers and audio clips into timeline sources/tracks.
   - `timeline::transcription` can add processing ranges, completed transcript annotations, and error markers.

3. View/render state:
   - `TimelineViewport` maps nanoseconds to pixels and supports zoom/pan.
   - `TimelineSelection` represents time ranges and track-scoped regions.
   - The renderer builds visible primitives only for events intersecting the viewport.
   - Hit-testing should use the same projected rectangles used for rendering.

Initial implementation should live near the existing scene-window modules, then split into dedicated modules once the data model stabilizes. A likely structure is:

- `src/timeline/mod.rs` for pure data types and viewport math.
- `src/timeline/tracy_capture.rs` for `.tracy` parsing and import.
- `src/app/windows_timeline.rs` for timeline window state and interaction orchestration.
- `src/app/windows_scene.rs` additions for timeline landing/empty document render scenes.

## Tracy Capture Reader Direction

The first parser should intentionally be smaller than the Tracy profiler:

- Validate the `tracy` file header and version using the same header semantics observed in `TracyWorker.cpp`.
- Read capture metadata: resolution, timer multiplier, capture name, capture program, capture time, host info, CPU metadata.
- Read string and source-location data needed to name zones and messages.
- Read message records into timestamped timeline markers.
- Read CPU thread timelines into nested zone intervals.
- Defer GPU zones, plots, memory, call stacks, frame images, context switches, source contents, statistics, and search indexes until the basic timeline is useful.

The parser should have tests against a tiny captured fixture or a generated/minimized fixture as soon as the first section can be read. If `.tracy` compression details make a native Rust reader too large for the first slice, add a temporary import command that shells out to `tracy-csvexport.exe` for summaries, but keep that as a stepping stone, not the target architecture.

## Timeline UI Direction

The first Teamy timeline UI should feel like an operational editor, not a marketing screen:

- A narrow left track list with track names, type badges, and add-track control.
- A top ruler showing time labels based on the current zoom.
- A central scrollable/zoomable lane area with stable track heights.
- A right or bottom details area can come later; first slice can use hover/selection text.
- Zoom should center around cursor position or viewport center.
- Pan should move the viewport in integer nanoseconds and clamp to document bounds.
- The first Tracy view can render nested zones as horizontal bars stacked by depth within each thread track.
- Message markers can render as ticks/diamonds in a message lane.

For microphone workflows, reuse the existing `AudioInputDevicePicker` and `AudioInputDeviceWindowState` machinery at first. The current timeline already follows this direction by binding microphone rows to the live audio runtime and rendering the transport directly in the timeline scene. The next step is to push captured data into document-owned clip/source structures so the runtime becomes the recorder/player for the document instead of the document surrogate.

For tool windows, keep the current in-window panel treatment until the timeline has a proper shared session object. Detached native tool windows should come only after there is one owner for timeline document state, transport state, hover/selection state, and tool-window synchronization.

## Tracey Specification Strategy

Create a dedicated spec at `docs/spec/product/timeline.md` and add a `teamy-studio-timeline` entry to `.config/tracey/config.styx`.

Initial requirement groups should be:

- `timeline[launcher.button]`: launcher exposes Timeline.
- `timeline[gui.window]`: Timeline opens a dedicated window.
- `timeline[gui.new-empty]`: Create New shows empty timeline with track list, ruler, and add-track affordance.
- `timeline[gui.load-tracy]`: Load Tracy accepts a `.tracy` file and reports parse status.
- `timeline[tracy.header]`: Rust reader validates Tracy header/version.
- `timeline[tracy.metadata]`: Rust reader extracts capture metadata.
- `timeline[tracy.cpu-zones]`: Rust reader maps CPU zones to thread tracks with nanosecond intervals.
- `timeline[tracy.messages]`: Rust reader maps Tracy messages to timestamped markers.
- `timeline[viewport.nanoseconds]`: viewport stores timeline positions in integer nanoseconds.
- `timeline[viewport.zoom-pan]`: user can zoom and pan without changing document data.
- `timeline[audio.add-microphone-track]`: add-track flow can create microphone-backed timeline tracks.
- `timeline[audio.source-vs-clip]`: audio track clips are views into source media rather than duplicated buffers.

Baseline and validation loop:

```powershell
tracey query status
tracey query uncovered
tracey query unmapped
tracey query unmapped --path src/app/windows_timeline.rs
tracey query unmapped --path src/timeline
tracey query validate --deny warnings
tracey query untested
```

Continue to use `./check-all.ps1` as the final validation command.

## Phased Task Breakdown

### Phase 1: Timeline Launcher And Empty Window

Objective: Create the visible entry point and an empty timeline shell.

Tasks:

- Add `docs/spec/product/timeline.md` and register it in `.config/tracey/config.styx`.
- Add `SceneAction::OpenTimeline` and `SceneWindowKind::Timeline`.
- Add a Timeline launcher card to `scene_button_specs`.
- Add `perform_scene_action` handling that opens the Timeline window on a new thread.
- Add a Timeline scene renderer that shows Create New and Load Tracy actions.
- Add tests that the launcher exposes the Timeline action and the Timeline scene renders its two starter actions.

Definition of done:

- Teamy launches and the main menu contains Timeline.
- Clicking Timeline opens a dedicated Timeline window.
- The Timeline window shows Create New and Load Tracy choices.
- Tracey coverage includes the new launcher/window requirements.
- `./check-all.ps1` passes.

### Phase 2: Empty Timeline Document And Track List

Objective: Turn Create New into a real document view with no tracks yet.

Tasks:

- Add pure timeline data types: `TimelineDocument`, `TimelineTrack`, `TimelineTrackId`, `TimelineTime`, `TimelineViewport`, and `TimelineSelection`.
- Add a `TimelineWindowState` that owns either the landing screen or an open document.
- Render track list, time ruler, empty content area, and add-track button.
- Implement viewport math tests for nanosecond-to-pixel and pixel-to-nanosecond conversion.
- Add keyboard/mouse affordances for basic pan and zoom, even if no tracks exist.

Definition of done:

- Create New switches the Timeline window into an empty document view.
- The empty document view has stable track-list/ruler/content geometry.
- Pan/zoom state updates are visible in the ruler and diagnostics.
- Timeline positions remain integer nanoseconds in state/tests.

### Phase 3: Tracy Header And Metadata Import

Objective: Load a `.tracy` file far enough to prove local parsing and show capture identity.

Tasks:

- Add `src/timeline/tracy_capture.rs` with a small reader for the Tracy header/version and initialization metadata.
- Support the compression/container format needed by current local captures, or clearly report unsupported compression while preserving the UI flow.
- Add Load Tracy action using `rfd::FileDialog` with `.tracy` filter.
- Show capture name, program, host, time range placeholders, and parse status in the Timeline window.
- Add parser tests against a small fixture or generated minimal file.

Definition of done:

- Load Tracy can select a local `.tracy` file.
- Teamy validates the header/version and displays metadata or a useful error.
- The parser keeps all raw times in integer nanoseconds or source tick units until conversion is explicitly required.

### Phase 4: Tracy CPU Zones And Messages

Objective: Render real Tracy spans and messages on the timeline.

Tasks:

- Parse string/source-location tables needed for zone labels.
- Parse CPU thread timelines into `TimelineTrack` instances.
- Parse messages into a message lane.
- Preserve nesting depth and parent/child relationships enough to stack zones visually.
- Render visible zone bars and message markers in the current viewport.
- Add hit testing for selecting a zone or message and showing details.

Definition of done:

- A Teamy-generated `.tracy` capture opens as a read-only timeline document.
- CPU thread tracks show labeled spans with nanosecond start/end values.
- Messages render at the right relative positions.
- Selecting a span/message shows its name, file/line when available, start, end, and duration.

### Phase 5: Timeline Navigation Performance

Objective: Make zooming and panning reliable on large captures.

Tasks:

- Add per-track interval indexes for querying visible events.
- Build only visible render primitives for the current viewport.
- Add coarse aggregation for events that are narrower than a pixel.
- Add Tracy spans around timeline parse, indexing, visible-query, and render-build phases.
- Profile with the Clay transcription capture and at least one larger capture.

Definition of done:

- Zoom/pan remains responsive on realistic `.tracy` captures.
- Render-build time scales with visible events rather than total capture events.
- Tracy profiling of Teamy's timeline viewer has useful spans.

### Phase 6: Add-Track Flow And Microphone Tracks

Objective: Connect the timeline shell to the audio input workflow.

Tasks:

- Add add-track UI that offers Microphone as a track source.
- Reuse or open the existing `AudioInputDevicePicker` to choose a microphone.
- Bind the chosen/default microphone device to a timeline audio track and reuse the live audio runtime for timeline transport.
- Render microphone device identity, recording control, transport play/pause control, and draggable recording/playback heads inside the timeline scene.
- Keep the current tool surfaces in-window rather than detached until the timeline has shared session state.

Definition of done:

- A new empty timeline can add microphone-backed tracks.
- The timeline shows the live device name/icon/record control in the left panel.
- The timeline exposes play/pause and draggable recording/playback heads in the transport/ruler area.
- Wheel zoom animates elastically while preserving cursor-anchored zoom semantics.

Status:

- Completed.

### Phase 7: Text Tracks And Box Authoring

Objective: Make text documents a first-class timeline surface that can later receive transcription output and manual authoring.

Tasks:

- Add a dedicated text track kind and document-owned text-box interval type.
- Render text boxes as viewport-clipped blocks rather than stretching or compressing them to fit the visible lane.
- Add a toolbar mode that can brush out empty text boxes by dragging in a text track lane.
- Show text-box contents via native hover tooltips so the user can inspect off-screen or truncated content.
- Add focused tests for text-track creation, box projection, and box-authoring interactions where practical.

Definition of done:

- A new empty timeline can add a text track.
- Dragging in a text track with the box tool enabled creates a document-owned text box.
- Text boxes stay stable under pan and zoom and clip cleanly at the viewport edge.
- Hovering a text box exposes its current contents.

Status:

- In progress: add-track, document model, hover tooltip, and drag-box creation are implemented; pinned editing and detached editor windows remain to be built.

### Phase 8: Persist Recorded Audio Into The Timeline Document

Objective: Replace the current runtime-only recording projection with durable document-owned audio source and clip state.

Tasks:

- Add timeline audio source/clip structures that can reference recorded sample buffers independently of the live device window state.
- Decide whether the first persisted clip path stores in-memory buffers, temp files, or a simple project-local asset path.
- On timeline recording start/stop, create or extend document-owned audio clips and keep them synchronized with the live transport.
- Render recorded clips from document state so they remain visible after transport/runtime state changes.
- Add tests for clip creation, appended duration updates, and source-vs-clip invariants.

Definition of done:

- Recording from the timeline creates durable audio clip/source state in `TimelineDocument`.
- Recorded material remains visible and replayable without depending on a transient live-only projection.
- The document model still preserves non-destructive clip semantics.

### Phase 9: Shared Timeline Session For Detached Tool Windows

Objective: Enable true detached timeline tool windows only after the timeline has a shared session layer.

Tasks:

- Introduce a shared timeline-session object that owns document, transport, selection, viewport, and tool-window synchronization state.
- Refactor scene-window startup so multiple timeline-related windows can bind to the same session instead of each creating isolated thread-local state.
- Split the current in-window track list, transport, and viewport controls into optional detached scene windows backed by the shared session.
- Keep hit-testing, tooltips, cursor state, and render invalidation synchronized across all attached windows.
- Add tests or harness coverage for session reuse and synchronized tool-window updates where practical.

Definition of done:

- Detached timeline tool windows stay synchronized with the main timeline document and transport.
- Opening/closing a tool window does not fork timeline state.
- The main timeline can still fall back to in-window panels when detached windows are disabled or unavailable.

### Phase 10: Transcription Regions And Editing Primitives

Objective: Use the timeline as the control surface for transcription and audio editing.

Tasks:

- Add timeline regions for queued, in-progress, completed, and failed transcription work.
- Add selection-to-transcribe flow that sends selected audio ranges to the transcription backend.
- Add basic clip move/split primitives for audio clips.
- Keep source media immutable unless explicitly destructively edited later.
- Add tests for source-vs-clip invariants.

Definition of done:

- Users can select an audio region on the timeline and see transcription processing state on that same region.
- Completed transcripts attach to the relevant timeline interval.
- Clips can be repositioned or split without losing the original source buffer.

## Recommended Implementation Order

1. Add the dedicated Tracey spec and Timeline launcher/window shell.
2. Add pure timeline data types and empty document rendering.
3. Implement Tracy metadata import before full zone parsing.
4. Render CPU zones/messages from a local `.tracy` capture.
5. Profile and index the viewer for pan/zoom performance.
6. Add microphone-backed tracks using existing audio picker/capture code.
7. Add text tracks and document-owned text-box authoring.
8. Persist recorded audio as document-owned sources/clips.
9. Add a shared session layer before detached timeline tool windows.
10. Add transcription regions and audio clip editing primitives.

## Open Decisions

- Whether the first `.tracy` parser should support all compression modes immediately or start with whichever mode local `tracy-capture.exe` emits.
- Whether timeline windows should stay in the scene-window system or eventually become a dedicated renderer path with more specialized GPU buffers.
- Whether persisted recorded audio should first live in memory, temp files, or an explicit project-asset path.
- When detached timeline tool windows become important enough to justify the shared session refactor.
- How much of the existing audio-input detail timeline should be folded into the new timeline framework versus left as a focused single-device control surface once the timeline owns durable clip state.
- Whether Teamy's own timeline document should use a custom binary format, JSON sidecar plus media files, or a database-style store once audio editing lands.

## First Concrete Slice

Implement Phase 7 next:

- Add a pinned text editor surface that binds to a hovered or clicked text box.
- Implement primitive cursor movement and text entry over the text-box document model.
- Support the requested control-word navigation and delete variants before widening into a richer editor.
- Keep the current in-window toolbar working while designing the shared session boundary for later detached windows.
- Add focused tests for text-box selection/edit binding and keyboard movement semantics where practical.
- Run `./check-all.ps1` and `tracey query status`.

This slice keeps momentum on the newly landed text-track milestone by making the boxes editable before the larger shared-session refactor that detached toolbar, track-list, and editor windows will require.

# Timeline And Tracy Capture Viewer Plan

## Goal

Build a Teamy-owned timeline framework that can start as a Tracy capture viewer and grow into the audio recording/editing surface for microphone tracks, transcription regions, selections, and real-time processing state.

The first proof should let the launcher open a Timeline flow, create an empty timeline, or load a `.tracy` capture. Loading a Tracy capture should prove that Teamy can parse nanosecond-precision spans/messages and render them in an interactive, zoomable, pannable timeline window. Later slices should reuse the same timeline model for microphone tracks, live sample buffers, audio clips, transcription work regions, and DAW-style editing interactions.

## Current Status

- Done so far:
  - Added the Tracy repository to the VS Code workspace at `G:\Programming\Repos\tracy`, including `profiler\src` and the server-side capture loading code under `server\`.
  - Added `<activeCodePage>UTF-8</activeCodePage>` to `resources/app.manifest`. This asks Windows to use UTF-8 as the process active ANSI code page for non-Unicode Win32 APIs; it is useful defensive hygiene for libraries or legacy calls that still pass narrow strings, but Teamy should continue preferring UTF-16 Windows APIs where practical.
  - Confirmed Teamy already has a launcher action system in `src/app/windows_scene.rs` and `src/app/windows_app.rs` using `SceneAction`, `SceneWindowKind`, `scene_button_specs`, `run_scene_window`, and centralized scene rendering/dispatch.
  - Confirmed Teamy already has microphone selection and detail windows with capture state, shared sample buffers, recording/playback/transcription heads, waveform selection, mel preview caching, and transcript staging in `src/app/windows_audio_input.rs`.
  - Confirmed the renderer already has timeline-oriented shader concepts such as `TimelineHeadGrabber` and audio detail layout fields for waveform, mel spectrogram, transcript terminal, and timeline labels.
  - Inspected Tracy capture loading enough to identify the first format boundary: `server/TracyWorker.cpp` reads a `tracy` version header, initialization metadata, string/source-location tables, messages, CPU zones, GPU zones, plots, memory events, call stacks, frame images, and context-switch sections through `server/TracyFileRead.hpp`.
  - Added `docs/spec/product/timeline.md` and registered `teamy-studio-timeline` in `.config/tracey/config.styx`.
  - Added a Timeline launcher button that opens a Timeline start window.
  - Added New and Import actions to the Timeline start window; New switches the same window into a blank timeline document and Import reports a placeholder until the Tracy reader lands.
  - Added blank timeline rendering with an empty track list area, add-track placeholder, time ruler, and content area.
  - Added tests and Tracey references for the Timeline launcher button, start window actions, blank timeline view, and import placeholder.
  - Added `src/timeline/mod.rs` with pure timeline document, track id, track, integer-nanosecond time, and viewport projection types.
  - Added tests for blank document defaults and integer-nanosecond viewport projection.
- Current focus:
  - Thread the pure timeline document model into `SceneAppState` so the blank timeline window renders from state rather than fixed placeholder geometry.
- Remaining work:
  - Implement a Rust Tracy capture reader that starts with capture metadata, strings/source locations, threads, messages, and CPU zones.
  - Render Tracy zones/messages on a zoomable timeline with nanosecond precision.
  - Generalize the timeline model so microphone tracks and Tracy capture tracks share the same viewport, track layout, selection, and clip/zone rendering concepts.
  - Add live microphone track creation from the existing audio-device picker and shared capture buffer.
- Next step:
  - Add `TimelineWindowState` to the app window layer and pass document/viewport state into the timeline renderer.

## Constraints And Assumptions

- Teamy is MPL-2.0. Tracy is BSD-licensed. The plan is to reimplement the needed behavior in Rust and use Tracy as a reference for concepts and file layout. If any Tracy source text or substantial code is copied instead of reimplemented, preserve the required BSD notice and review the licensing boundary deliberately.
- The timeline core must store time in integer nanoseconds, not `f64`, so Tracy captures and later audio/transcription ranges do not lose precision. UI projection can convert to `f64` only at the viewport/rendering boundary.
- The first viewer should target `.tracy` files produced by the Tracy version currently in the workspace. Backward-compatible loading can come after the first local capture works.
- The first UI should use Teamy scene windows and renderer primitives rather than introducing ImGui or Tracy UI code.
- The new timeline behavior is a new product surface, not a narrow extension of audio input or windowing. It should get a dedicated Tracey spec.
- The existing microphone detail timeline is useful prior art, but it is a single-device surface. The new timeline should become a multi-track document/view model that audio input can feed into later.
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
- A microphone track must eventually map one source medium, such as a live sample buffer or recorded clip, into one or more track lanes depending on mono/stereo channel count.
- Audio clips, Tracy zones, transcription work ranges, and user selections should share one interval/marker vocabulary so the timeline can visualize what is being recorded, processed, transcribed, or selected.

## Architectural Direction

Use three layers and keep them separate:

1. Timeline document model:
   - `TimelineDocument` owns tracks, sources, clips/events, selections, and metadata.
   - `TimelineTime` stores integer nanoseconds.
   - Tracks are typed: Tracy thread, Tracy messages, audio channel, transcription work, derived analysis, or placeholder.
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

For microphone workflows, reuse the existing `AudioInputDevicePicker` and `AudioInputDeviceWindowState` machinery at first. The add-track flow should open a small Teamy scene/modal asking what kind of track to add; choosing Microphone should open or reuse the microphone picker and then add mono/stereo audio channel tracks to the timeline document.

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
- Determine mono/stereo channel count from the selected endpoint/capture format.
- Add one track for mono or two linked channel tracks for stereo.
- Start/stop recording from the timeline and append samples to the source medium.
- Render a live waveform view backed by the captured sample buffer.

Definition of done:

- A new empty timeline can add microphone-backed tracks.
- Recording appends live audio to the timeline source.
- The waveform updates while recording.
- Track clips remain views into the recorded source medium.

### Phase 7: Transcription Regions And Editing Primitives

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
7. Add transcription regions and audio clip editing primitives.

## Open Decisions

- Whether the first `.tracy` parser should support all compression modes immediately or start with whichever mode local `tracy-capture.exe` emits.
- Whether timeline windows should stay in the scene-window system or eventually become a dedicated renderer path with more specialized GPU buffers.
- Whether the add-track chooser should be a scene window, modal-like overlay, or dedicated popup window.
- How much of the existing audio-input detail timeline should be folded into the new timeline framework versus left as a focused single-device control surface.
- Whether Teamy's own timeline document should use a custom binary format, JSON sidecar plus media files, or a database-style store once audio editing lands.

## First Concrete Slice

Implement Phase 1 only:

- Create `docs/spec/product/timeline.md`.
- Register `teamy-studio-timeline` in `.config/tracey/config.styx`.
- Add Timeline to the launcher card list.
- Add a Timeline scene window with Create New and Load Tracy buttons.
- Make both buttons report placeholder behavior if their backing logic is not implemented yet.
- Add tests for launcher exposure and Timeline window render contents.
- Run `./check-all.ps1`.

This slice creates the user-visible entry point without committing to the full parser or audio editing architecture too early.

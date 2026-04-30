# Timeline

This specification covers the Teamy timeline surface used for Tracy capture viewing, audio track recording, transcription regions, and future clip editing.

## Launcher And Start Window

timeline[launcher.button]
The launcher window must expose a Timeline action that opens the timeline workflow.

timeline[start-window.create-or-import]
Opening Timeline must show a dedicated start window with New and Import actions.

timeline[start-window.new-blank]
Choosing New in the Timeline start window must create a blank timeline document in the same window.

timeline[import.tracy.file-picker]
Choosing Import in the Timeline start window must open a `.tracy` file picker and either import the selected Tracy capture or report an explicit error.

timeline[import.tracy.document]
Successfully importing a Tracy capture must create a timeline document with at least one tracing-spans track backed by the imported capture metadata.

## Blank Timeline

timeline[blank.track-list]
A blank timeline document must render an empty track list area, a time ruler area, and an empty timeline content area.

timeline[blank.tool-panels]
The timeline window must reserve distinct in-window tool panels for the track list, transport play or pause control, and viewport scrub or zoom controls.

timeline[blank.add-track-placeholder]
A blank timeline document must reserve an add-track affordance area so microphone and media tracks can be added in a later slice.

## Track Addition

timeline[add-track.workflow]
Clicking Add Track from a timeline document must open an in-window track-source workflow with explicit options for timeline track creation.

timeline[add-track.tracy]
Importing a Tracy capture from the Add Track workflow must append a tracing-spans track to the current timeline document instead of replacing existing tracks.

timeline[add-track.microphone-live-device]
Choosing Microphone from the Add Track workflow must create an audio track backed by the selected or default live audio input device when one is available, and otherwise fall back to a generic microphone track.

timeline[add-track.microphone-placeholder]
Timeline documents must be able to create a generic microphone placeholder track when no concrete live input device has been selected yet.

timeline[track.microphone-row]
Live microphone tracks must render their device icon, device name, and a record control in the left track-list panel.

timeline[recording.append-live]
Starting recording from a microphone track row must append captured audio to that timeline track and advance a visible recording head as samples are written.

timeline[heads.recording-playback]
Timeline audio recording must expose visible draggable recording and playback heads aligned to the shared time ruler.

timeline[transport.spacebar-playback]
The timeline transport panel must expose a play or pause control for the recorded buffer, and pressing Space in the timeline must toggle the same playback state.

## Document Model

timeline[document.blank-model]
A blank timeline document must have a stable identity, no tracks, and a viewport initialized at zero nanoseconds.

timeline[document.window-state]
Creating a blank timeline from the start window must store the timeline document in the window state used for rendering.

timeline[track.kinds]
Timeline documents must support distinct track kinds so audio and tracing spans can coexist in the same document model.

timeline[track.projection-model]
Timeline tracks must store projected content metadata separately from their user-visible name so imported traces and future recorded audio can share the same track container.

timeline[track.preview-ranges]
Timeline tracks must expose a deterministic preview time range so the timeline content area can render positioned placeholders before full media or Tracy span import is wired.

timeline[edit-list.model]
Timeline documents must store non-destructive edit operations separately from track source data so later ripple deletes and clip edits do not mutate imported source media directly.

timeline[viewport.nanoseconds]
Timeline document and viewport positions must store time as integer nanoseconds instead of floating-point seconds.

timeline[viewport.projection]
The timeline viewport must convert between integer nanosecond positions and horizontal pixels without mutating document data.

timeline[viewport.typed-projection]
Timeline viewport projection should prefer typed time and coordinate values so timeline-space time and viewport-space positions cannot be mixed accidentally in touched code.

timeline[viewport.pan-controls]
Timeline documents should expose viewport pan controls that move the visible origin without mutating track source data.

timeline[viewport.zoom-controls]
Timeline documents should expose viewport zoom controls that change time-per-pixel scaling without mutating track source data.

timeline[viewport.mouse-pan]
The timeline should support right-mouse drag panning across the full timeline scroll surface, including horizontal time movement and vertical track scrolling.

timeline[viewport.mouse-zoom-anchor]
Mouse-wheel zoom over the timeline should keep the hovered time pinned to the same screen-space x position while changing the viewport scale.

timeline[viewport.mouse-zoom-animation]
Mouse-wheel zoom over the timeline should animate non-linearly between the current and target viewport states instead of snapping immediately.

timeline[ruler.ticks]
The timeline ruler must render multiple tick marks and labels derived from the current viewport instead of showing only a single origin label.

timeline[content.preview-lanes]
Timeline documents with tracks should render visible lane blocks in the content area by projecting each track preview range through the current viewport.

timeline[selection.rectangle]
Left-drag selection in the timeline should produce a rectangular marquee bounded in both time and track height instead of selecting every track that overlaps the chosen time range.

timeline[selection.ruler-all-tracks]
Dragging from the timeline ruler should produce a rectangular selection whose vertical extent covers all timeline tracks, including tracks currently scrolled off-screen.

timeline[viewport.scrollbars]
Timeline documents should render horizontal and vertical scrollbar affordances that reflect the current visible time span and track scroll position.

## Reusable Display Model

timeline[display.time-strict]
Reusable timeline display-model ranges must reject reversed start and end times instead of silently normalizing them.

timeline[display.dataset-owned-ids]
Reusable timeline datasets must assign internal item IDs and insertion sequences independently from source-specific job, tracing, Tracy, or calendar identifiers.

timeline[display.dataset-checked-mutation]
Reusable timeline datasets must mutate raw span and event items only through checked APIs that preserve time-range and item-kind invariants.

timeline[display.dataset-index-compaction]
Reusable timeline dataset compaction must update query indexes and clear pending writes without discarding raw timeline items.

timeline[display.object-refs]
Reusable timeline items must support lightweight object references and primitive object fields so future typed object inspection can be connected without storing large payloads in timeline items.

timeline[display.query-explicit-now]
Reusable timeline viewport queries must carry an explicit `now` value so open spans can be rendered deterministically without reading ambient wall-clock time.

timeline[display.query-derived-rows]
Reusable timeline render plans must derive compact rows from grouping settings rather than treating sparse source IDs as fixed row numbers.

timeline[display.query-render-items]
Reusable timeline render plans must expose explicit render item variants for spans, instant events, folded span clusters, and folded event clusters.

timeline[display.query-folding]
Reusable timeline render projection must fold dense spans and events into viewport-dependent clusters without mutating raw timeline items.

timeline[display.synthetic-data]
Reusable timeline synthetic data generation must be available outside test-only code and must create valid renderable datasets with dense spans, event bursts, open spans, sparse group keys, repeated metadata, and object-reference-bearing items.

timeline[playground.launcher-button]
The launcher window must expose a Timeline Playground action that opens a visible synthetic playground for the reusable timeline display model.

timeline[playground.synthetic-render-plan]
The Timeline Playground must generate synthetic timeline data and render it through `TimelineViewportQuery` and `TimelineRenderPlan` rather than through the older timeline document editor model.

timeline[playground.query-controls]
The Timeline Playground must expose controls for changing the synthetic seed, grouping mode, and folding threshold so row derivation and dense-item clustering can be exercised interactively.

timeline[playground.viewport-controls]
The Timeline Playground must support panning and zooming over synthetic timeline data and recompute the render plan from the updated viewport.

timeline[playground.pan-negative-time]
The Timeline Playground must allow the visible range to pan before zero so the origin can appear in the middle of the screen instead of being pinned to the left edge.

timeline[playground.fit-content]
The Timeline Playground must expose a fit-to-content action that sets the visible range to the dataset's content bounds with padding, including near-zero content that benefits from negative-time overscan.

timeline[playground.pan-button-snap-item]
Timeline Playground pan buttons must bring the nearest offscreen item into view when the current visible range is empty and the user pans back toward existing content.

timeline[playground.data-bounds-dimming]
The Timeline Playground must dim regions before time zero or the first closed data point, whichever is earlier, and after the last closed data point, ignoring open spans that have not ended when computing those bounds.

timeline[playground.vertical-pan]
The Timeline Playground must allow right-drag panning to move vertically across grouped rows as well as horizontally across time.

timeline[playground.vertical-pan-clamp]
The Timeline Playground must reclamp vertical row panning after viewport or grouping changes shrink the rendered row set.

timeline[playground.vertical-render-clip]
The Timeline Playground must skip fully offscreen rows and items during vertical row panning and clip partially visible row and item geometry to the timeline content surface so it cannot render over the ruler or controls.

timeline[playground.row-transition-animation]
When zooming or filtering changes the visible row set, Timeline Playground rows that remain visible must animate from their previous row-key position to their new position instead of snapping immediately.

timeline[playground.row-stable-colors]
Timeline Playground row and span colors must remain tied to stable row identity rather than transient visible row index while rows are added, removed, or repositioned.

timeline[playground.mouse-zoom-anchor]
Mouse-wheel zooming in the Timeline Playground must keep the time under the mouse cursor stable, matching the main timeline's anchor-aware zoom behavior.

timeline[playground.zoom-compounds]
Rapid repeated mouse-wheel zoom events in the Timeline Playground must compound against the pending target range instead of restarting each animation from the partially animated range.

timeline[playground.viewport-transition]
Timeline Playground zoom changes must animate between visible ranges with a non-bouncy ease-in-out transition.

timeline[playground.ruler-ticks]
The Timeline Playground must render top ruler tick marks, grid lines, and time labels for the currently visible range.

timeline[playground.ruler-subticks]
The Timeline Playground ruler must render intermediate subticks between labeled ticks, with a larger midpoint subtick and the labeled major ticks remaining the strongest interval markers.

timeline[playground.cursor-guide]
The Timeline Playground must render a light vertical guide line at the cursor x-position over the ruler and timeline content surface.

timeline[playground.event-arrows]
Timeline Playground instant events must render as compact downward markers at their timestamp instead of duration-like clips, while preserving hover and pin hit testing for event details.

timeline[playground.live-tracing-events]
The Timeline Playground must be able to switch from synthetic data to live tracing events captured by Teamy's tracing subscriber, mapping captured log events into timeline instant-event items as they arrive while preserving user-controlled pan and zoom after the user navigates away from the live tail.

timeline[playground.live-tracing-pan]
Right-drag panning the Timeline Playground in live tracing mode must count as user navigation and stop live-tail resets immediately.

timeline[playground.live-tracing-unfiltered]
The Timeline Playground live tracing collector must receive trace-level events independently of console or file log filtering so the timeline can inspect low-level observability events that are intentionally hidden from normal logs.

timeline[playground.live-tracing-spans]
The Timeline Playground live tracing mode must capture closed tracing span lifecycles as duration spans in addition to instant log events.

timeline[playground.span-lanes]
The Timeline Playground must render overlapping spans in nested lanes within their grouped row so thread timelines can show span nesting and overlap without stacking clips on top of each other.

timeline[playground.minimum-span-marker]
The Timeline Playground must preserve an interactive minimum-width marker for visible duration spans when zoomed out, including tiny folded spans in different rows.

timeline[playground.span-cluster-decomposition]
Folded Timeline Playground span clusters must split as zoom makes adjacent tiny spans visually separable, matching event cluster decomposition instead of shrinking aggregate counts without revealing the separated spans.

timeline[playground.projected-span-width]
The Timeline Playground must render duration spans at their projected time width whenever that width is greater than the minimum marker width.

timeline[playground.span-clip-readability]
Timeline Playground span clips must be tall enough for readable in-span labels when the clip is wide enough to show text.

timeline[playground.span-labels]
The Timeline Playground must render a span's title inside its duration clip when the title fits, center the title around the full projected span when possible, clamp it to the visible span edge when the projected center is offscreen, and leave the clip intact without text when it does not fit.

timeline[playground.span-bevel]
Timeline Playground span clips must render a subtle bevel or edge treatment so adjacent spans remain visually distinguishable.

timeline[playground.hover-detail]
Hovering a rendered span, event, folded span cluster, or folded event cluster in the Timeline Playground must open or update a sidecar detail window.

timeline[playground.hover-title-tooltip]
Hovering a rendered span, event, folded span cluster, or folded event cluster in the Timeline Playground must also show a native tooltip containing the resolved detail title.

timeline[playground.hover-title-tooltip-cursor]
Timeline Playground item title tooltips must position from the cursor point rather than the hovered item's centroid.

timeline[playground.hover-title-tooltip-stable]
Timeline Playground item title tooltips must avoid redundant native tooltip updates when the text and position have not changed.

timeline[playground.hover-detail-no-activate]
Timeline Playground hover detail windows must be created and shown without taking focus from the playground window.

timeline[playground.pin-detail]
Left-clicking a rendered span, event, folded span cluster, or folded event cluster in the Timeline Playground must promote the current hover detail into a pinned detail window.

timeline[playground.detail-window-clamped]
Timeline Playground detail windows must clamp their initial sidecar placement to the available virtual desktop bounds.

timeline[playground.detail-facet-pretty]
Timeline Playground detail windows must render a resolved Facet-derived detail view model with `facet-pretty` so interned labels, source keys, group keys, primitive fields, object references, and cluster metadata are readable.

timeline[playground.detail-vt-text]
Timeline Playground detail windows must parse VT/ANSI styling in reflected detail text before rendering, so styled `facet-pretty` output becomes colored text instead of visible escape sequences.

timeline[playground.detail-title-prefix]
Timeline Playground detail windows must put the selected timeline item's title in the native window title as a prefix before `Timeline Detail`, rather than duplicating that title inside the detail body.

timeline[playground.detail-selectable-text]
Timeline Playground detail windows must expose reflected detail text through the shared terminal-cell selection and copy path.

timeline[playground.detail-diagnostics-tui]
Timeline Playground detail windows must provide a diagnostics view rendered as a Ratatui-style terminal UI, matching the existing scene diagnostics pattern.

## Transcription Tracks

timeline[transcription.targets]
Transcription tracks must model three independent targets: the Rust Whisper model to run, the audio track to observe as input, and the text track to receive output blocks.

timeline[transcription.settings]
Transcription track settings must expose input-audio and output-text target selection, inactivity detection period, activity threshold, a manual flush action, automatic chunk-boundary advancement, and automatic chunk submission.

timeline[transcription.defaults]
New transcription tracks must default automatic chunk-boundary advancement and automatic chunk submission to enabled.

timeline[transcription.chunk-heads]
Transcription tracks should render the watched chunk begin, chunk end, and transcription progress heads in the transcription lane so the observed audio range is visible while recording grows the source track.

timeline[transcription.completion-refresh]
When a transcription worker finishes while another scene window has focus, the owning timeline window must be notified so completed transcript text is committed without waiting for pointer hover or focus changes.

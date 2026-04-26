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

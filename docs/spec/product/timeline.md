# Timeline

This specification covers the Teamy timeline surface used for Tracy capture viewing, audio track recording, transcription regions, and future clip editing.

## Launcher And Start Window

timeline[launcher.button]
The launcher window must expose a Timeline action that opens the timeline workflow.

timeline[start-window.create-or-import]
Opening Timeline must show a dedicated start window with New and Import actions.

timeline[start-window.new-blank]
Choosing New in the Timeline start window must create a blank timeline document in the same window.

timeline[start-window.import-placeholder]
Choosing Import in the Timeline start window may report that Tracy import is not implemented yet until the Tracy capture reader lands, but it must not silently do nothing.

## Blank Timeline

timeline[blank.track-list]
A blank timeline document must render an empty track list area, a time ruler area, and an empty timeline content area.

timeline[blank.add-track-placeholder]
A blank timeline document must reserve an add-track affordance area so microphone and media tracks can be added in a later slice.

## Document Model

timeline[document.blank-model]
A blank timeline document must have a stable identity, no tracks, and a viewport initialized at zero nanoseconds.

timeline[document.window-state]
Creating a blank timeline from the start window must store the timeline document in the window state used for rendering.

timeline[viewport.nanoseconds]
Timeline document and viewport positions must store time as integer nanoseconds instead of floating-point seconds.

timeline[viewport.projection]
The timeline viewport must convert between integer nanosecond positions and horizontal pixels without mutating document data.

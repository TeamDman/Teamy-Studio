cursorlatency[launcher-button]
The Teamy Studio launcher must expose a Cursor Latency Playground action that opens a dedicated scene window for comparing app-drawn cursor presentation against the visible OS cursor.

cursorlatency[playground.window]
The Cursor Latency Playground must render continuously while focused so it can sample the current OS cursor each frame and update the comparison without requiring mouse clicks or keyboard input.

cursorlatency[playground.behavior-controls]
The Cursor Latency Playground must expose actions for `Fastest` and `Match OS`, and the active mode must stay visibly selectable in the window controls.

cursorlatency[playground.fastest]
`Fastest` mode must use recent OS cursor motion to project the app-drawn cursor polygon slightly ahead of the latest sampled position so the playground can dramatize an app-first presentation policy.

cursorlatency[playground.match-os]
`Match OS` mode must place the app-drawn cursor polygon directly on the latest sampled OS cursor position so the playground can demonstrate a welded comparison mode.

cursorlatency[playground.polygon]
The Cursor Latency Playground must construct a filled cursor polygon from recent OS cursor samples instead of rendering only a single point marker, making the motion and lead difference visually obvious.

cursorlatency[playground.split-halves]
The Cursor Latency Playground must show `Fastest` on the left and `Match OS` on the right at the same time so both presentation rules can be compared side by side in one window.

cursorlatency[playground.ripple-sdf]
Each half of the Cursor Latency Playground must fill its play area with sinusoidal ripples derived from the signed distance field of a centered triangle at that mode's latest cursor position, instead of drawing a discrete cursor symbol.

cursorlatency[overlay.f3-toggle]
Pressing `F3` on a scene page must toggle the latency overlay on and off without losing the last chosen placement.

cursorlatency[overlay.position-cycle]
Repeated `F3` presses within a short burst must rotate the latency overlay between corner, edge-center, and center placements.

cursorlatency[overlay.hide-after-pause]
After a pause, the next `F3` press must switch from rotating the overlay to hiding it, and the following press must restore it at the prior placement.

cursorlatency[overlay.frame-graph]
The latency overlay must render a frame-time graph where slower frames occupy more visual width so hitches stand out immediately.
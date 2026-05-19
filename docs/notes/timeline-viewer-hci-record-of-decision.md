# Timeline Viewer HCI Record of Decision

This document captures the May 19, 2026 decisions about the active timeline viewer's interaction model and visual feel.

## Decisions

### D1. The active event viewer is a Teamy timeline viewer.

The viewer should render Teamy-owned `Timeline` data. Tracing logs, spans, and events may appear in the viewer only after they have been ingested into the Teamy timeline model.

The viewer should not grow a separate tracing-only rendering path.

### D2. The interaction target is the legacy tracing timeline and Tracy, not the audio timeline.

The active viewer should feel closer to the legacy tracing timeline that showed spans/events, informed by Tracy profiler behavior.

The audio-oriented timeline remains relevant to long-term transcription goals, but it is not the primary reference for the current event viewer HCI.

### D3. Pointer interaction must feel immediate.

The viewer should prioritize:

- zero-latency cursor positional awareness as much as the swapchain/present architecture permits
- immediate hover feedback
- cursor guide/ruler feedback that tracks the mouse without waiting on semantic selection
- right-button drag panning over the timeline lane surface
- mouse-wheel zooming anchored under the cursor

During right-button panning, the grabbed timeline point should stay aligned under the cursor instead of drifting with visible parallax.
Dragging should preserve a continuous fractional display camera while the gesture is live instead of snapping every visible update to integer timeline endpoints.

### D4. Timeline camera motion should use continuous math.

Zooming and panning should avoid visible snapping to integer grid positions.

The implementation may use floating-point or another fractional camera representation for the displayed viewport, even when authoritative timeline timestamps remain integer/femtosecond based.
Pointer-to-timeline transforms should be expressed through an explicit typed viewport-point model in the style of the legacy `sguaba`-backed timeline code, rather than re-deriving ad hoc anchor ratios in unrelated call sites.

Wheel zoom should compose smoothly when input arrives during an existing zoom animation.
Extreme zoom-out should clamp to a valid representable camera range instead of crashing or producing invalid canonical ranges.
Cursor-guide rendering should prefer the freshest available pointer sample during frame construction so the guide does not visibly trail the hardware cursor.

### D5. Ruler labeling should support global base plus relative increments.

The legacy viewer showed global time directly in the ruler. Tracy-style labeling with an absolute/global base at the left and relative increments across the ruler is also acceptable and preferred for dense views.

The important requirement is that users can orient to global timeline position while reading nearby tick increments without visual clutter.

## Consequences

- Rendering code should keep fractional display range separate from canonical timestamp storage when needed.
- Tests for timeline HCI should cover cursor-anchored zoom, right-button panning, fractional projection, and eased zoom progress.
- Future timeline visual changes should be evaluated against pointer feel and inspection usefulness before dashboard-style information density.

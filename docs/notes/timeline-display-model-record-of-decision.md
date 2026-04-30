# Timeline Display Model Record Of Decision

Date: 2026-04-29

Related plan: `docs/notes/timeline-display-model-plan.md`

## Purpose

This note preserves the conversational decision trail that led to the timeline display model plan. The implementation plan captures the forward path; this record captures the questions, answers, file references, and clarifications that may otherwise be lost when chat context is compacted.

## Local References Consulted

- `docs/notes/spatial.md`
  - User-authored design note that motivated replacing the hard-coded Jobs window with structured observability/progress views, decomposing large windows into smaller spatial islands, adding playgrounds, and improving spatial reasoning across windows, tooltips, terminal-like grids, and timeline views.
- `docs/notes/timeline-profiler-plan.md`
  - Existing timeline and Tracy capture viewer plan. The new display-model plan complements it instead of replacing it. The profiler plan focuses on the Timeline product surface, document/editor behavior, and Tracy capture loading; the new plan focuses on reusable raw-data/query/render projection primitives and the intermediary playground.
- `docs/spec/product/timeline.md`
  - Existing Tracey product spec for the Timeline surface. It should be extended with display-model/query/render-projection requirements rather than immediately creating a third spec namespace.
- `docs/spec/product/observability.md`
  - Existing Tracey product spec for logs and toasts. It should be extended later with object-reference tracing conventions and span/event timeline ingestion requirements.
- `.config/tracey/config.styx`
  - Confirms current Tracey spec entries include `teamy-studio-timeline` and `teamy-studio-observability`.
- `src/timeline/mod.rs`
  - Existing timeline model with integer nanosecond time, viewport projection, track/document concepts, and current range normalization behavior. The strict display model cannot directly reuse the current normalizing range constructor without changing semantics.
- `src/logs.rs`
  - Existing bounded tracing event collector. It captures events and span context but not span lifetimes. Future observability ingestion needs explicit span lifecycle data.
- `src/app/jobs.rs`
  - Existing hard-coded Jobs system. It is a likely migration target once Progress Hub can be backed by structured timeline observability.
- `src/app/windows_scene.rs`
  - Existing scene actions, window kinds, and rendering helpers for logs, jobs, timeline, scrollbars, and timeline projections.
- `src/app/windows_app.rs`
  - Existing window runtime/input/render orchestration, toast host behavior, timeline pan/zoom/hit-testing, and scene action dispatch.
- `src/app/spatial.rs`
  - Typed geometry and coordinate transform primitives. Useful context for future spatial-reflection work, though not the first timeline display-model slice.
- `Cargo.toml` and `Cargo.lock`
  - Confirmed Facet, Facet serialization helpers, and Arbitrary are already dependencies.
- Tracy profiler source under `g:\Programming\Repos\tracy\profiler\src\profiler`
  - Files inspected included `TracyTimelineItemThread.cpp`, `TracyTimelineDraw.hpp`, `TracyTimelineController.cpp`, `TracyView_ZoneTimeline.cpp`, and `TracyTimelineContext.hpp`.
  - Tracy keeps raw capture data intact and builds per-viewport draw lists. It folds visually tiny zones/messages using a pixel-to-time threshold, grounding the Teamy decision that aggregation belongs in query/render projection rather than raw data mutation.
- `tracing-core` source from the local Cargo registry
  - Confirmed `tracing_core::field::Value` is sealed, so Teamy cannot implement custom tracing values for object references.

## Decision Trail

### Playground Detail And Live Event Polish

Question: After the first visible playground was usable, what should happen to the awkward detail and event presentation discovered during manual play?

Response: Keep the native chrome as the place for window identity, make reflected text behave like other terminal-grid text, and use event-specific markers instead of span-like rectangles.

Clarification:

- The playground window already has a chrome title, so an additional in-body `Timeline Playground` title wastes space and can collide with controls.
- Detail windows should be named by the selected item in native chrome, for example `Import Capture - Timeline Detail`, instead of repeating that title inside the body.
- Detail text should preserve explicit line breaks, parse VT styling, and participate in the existing terminal-cell selection/copy path.
- Diagnostics should look and behave like the existing Ratatui diagnostics views rather than falling back to an unstructured text dump.
- Instant events are not durations. They should render as small downward markers at their timestamp, with hover/pin details still using the same hit-testing path.

Decision:

- Remove the duplicate in-body playground title and stack the playground summary fields vertically.
- Render playground ruler subticks between labeled major ticks.
- Render instant events as compact downward markers.
- Update detail window titles dynamically to `<detail title> - Timeline Detail` and leave the body focused on state plus detail text.
- Reuse the scene terminal selection machinery for timeline detail text and diagnostics.
- Add a live tracing-event mode by projecting events captured by `LogCollectorLayer` into the reusable timeline dataset as instant events. This is an event-only first slice; span lifecycle capture remains future work.

### Playground Navigation And Event Burst Behavior

Question: Why did fast wheel input feel weak, live events become hard to navigate, and synthetic event bursts disappear rather than decompose while zooming in?

Response: The viewport should treat user navigation as an accumulated target, live mode should stop tail-following once the user navigates, and event folding should be driven by projected event spacing instead of a single all-events cluster per visible row.

Decision:

- Repeated playground wheel zoom events compound against the pending target range while the visible range animates from the current interpolated range.
- Live tracing-event mode follows the tail only on entry and while untouched. Panning or zooming disables tail follow so the chosen view remains navigable.
- Event folding flushes clusters when adjacent visible events are separated by at least the minimum visible pixel threshold, so zooming in decomposes dense bursts into individual markers.
- A light vertical cursor guide is rendered over the playground ruler/content surface to make the inspected timestamp easier to read.

### Playground Overscan, Recovery, And Tooltip Titles

Question: What should happen when the user wants the origin centered, pans far away from content, or wants quick identity without shifting attention to the detail sidecar?

Response: Treat the playground viewport as an inspectable camera over time, not a scroll range clamped to zero. Recovery controls should be forgiving, and item identity should be available both in the sidecar detail and in the native tooltip channel.

Decision:

- Playground pan and zoom ranges may move before zero, allowing zero to sit in the center or right side of the viewport.
- A Fit control sets the visible range to dataset content bounds with padding, including negative padding around near-zero content.
- Pan buttons keep their normal quarter-viewport movement when content is visible, but when the viewport is empty they snap the nearest content in the requested direction into view.
- The live tracing-event collector receives trace-level events before console/file output filters are applied, so the timeline can inspect low-level events even when normal logs stay quieter.
- Hovering a rendered playground span, event, or folded cluster shows a native tooltip containing the same resolved title used by the sidecar detail window.

### Closed Data Bounds And Thread Span Lanes

Question: How should the playground distinguish real captured time from overscan, and how should it move toward Tracy-style thread rows?

Response: Bounds should come only from data that has a complete timestamp range. Closed spans and instant events count. Open spans do not count until they close, because their start alone does not prove the visible future contains data. Thread rows should allow nested span lanes rather than forcing overlapping duration clips into the same vertical slot.

Decision:

- Dim the playground ruler/content before the first closed data point and after the last closed data point.
- Ignore open spans when computing those dimming bounds, while still rendering them as open spans in the query when they are visible.
- Capture tracing span lifecycles in the live adapter by recording span creation and close times, then projecting closed spans into the reusable timeline dataset.
- Group live tracing spans by thread name, matching the live tracing event grouping.
- Assign overlapping spans to nested per-row lanes in the render plan so the scene can draw multiple span lines inside a thread row.

### Playground Manual Interaction Fixes

Question: What should happen when manual play exposes native tooltip flicker, missing vertical panning, offscreen sidecars, or tiny spans disappearing while zoomed out?

Response: Treat the playground as a native inspection surface. The cursor should drive hover tooltip placement, right-drag should move the camera in both axes, and small visible spans should retain a row-local marker even when folded. Sidecar placement should stay inside the virtual desktop so monitor layout cannot hide the detail window.

Decision:

- Anchor playground item title tooltips to a one-pixel cursor rect instead of the hovered item's centroid.
- Cache the active native tooltip text and position so unchanged tooltip updates do not resend `TTM_UPDATETIPTEXTW`, `TTM_TRACKPOSITION`, and `TTM_TRACKACTIVATE` every frame.
- Add playground vertical scroll state and apply right-drag y deltas through the same pan interaction as horizontal time panning.
- Reclamp playground vertical scroll after render-plan row changes so zooming or grouping cannot leave the remaining rows offscreen.
- Clip playground rows/items to the content surface and skip fully offscreen rows/items so vertical panning cannot draw timeline content over the ruler or controls.
- Use the raw scrolled row geometry for span lane placement and clip only the final render/hit rects, preventing lanes from sticking to the content edge while their row scrolls away.
- Keep row colors tied to row keys and retain row-key world positions across query changes so visible rows can animate into compacted positions without changing color.
- Treat right-drag panning in live tracing mode as user navigation, stopping live-tail resets just like wheel zoom and pan buttons.
- Split folded tiny span clusters by projected separation so zooming reveals separated spans instead of only shrinking the aggregate count.
- Anchor span labels to the full projected span center and clamp the text box into the visible span slice, then add highlight/shadow bevel edges to keep adjacent spans visually separated.
- Start closed-data bright bounds at time zero when all closed data starts after zero, while still ignoring open spans for right-side bounds.
- Flush folded tiny-span clusters when their row changes, preserving at least one minimum-width marker per visible row.
- Preserve projected duration width whenever a span is wider than the minimum marker, and draw the span title inside the clip only if it fits.
- Clamp timeline detail sidecar windows to the virtual desktop bounds before opening them.

### Initial Direction: Progress Hub Versus Playground

Question: Should the next implementation target a Progress Hub that replaces Jobs first?

Response: No. An intermediary playground is needed first.

Clarification:

- Progress Hub depends on reusable UX and model primitives that are currently scattered across logs, jobs, timeline, toasts, terminal scrolling, and timeline viewport behavior.
- The playground should let Teamy exercise pan/zoom, scrolling, scrollbars, hover details, pinned detail windows, filtering, grouping, synthetic data, and folded dense events before wiring live observability.
- The playground is not just a demo. It is the proving ground for the model and interaction language that Progress Hub will later consume.

Decision:

- Build a reusable display/query model first.
- Build the intermediary playground after model/query tests exist.
- Build Progress Hub after the playground validates the shared primitives.

### Relationship To Existing Timeline Code

Question: Should the new work live completely beside the existing timeline model or replace it immediately?

Response: Temporary side-by-side implementation is acceptable, but the long-term goal is a smaller codebase through migration.

Clarification:

- The existing timeline editor already contains useful concepts: integer nanosecond time, viewport projection, ruler/scrollbar behavior, document/track concepts, and scene integration.
- The existing range constructor normalizes reversed input. That is useful for drag gestures in the current editor but too permissive for a strict reusable data model.
- The new model should not permanently create a second competing timeline architecture.

Decision:

- Add strict submodules under `src/timeline/` first.
- Keep compatibility while the first model/query slices land.
- Later migrate current timeline editor call sites to strict range semantics.
- Reversed drag points should be handled at interaction boundaries, not silently inside the core range type.

### Raw Data Versus Aggregation

Question: When spans or events are too dense to draw individually, should the dataset compact or summarize raw data?

Response: No. Aggregation is a render/query projection, not raw data mutation.

Clarification:

- Tracy was used as a reference model. It keeps capture data intact and derives draw lists for the current viewport.
- Folding tiny items is based on current zoom and pixel thresholds, so it is inherently view-dependent.
- Losing raw spans/events during compaction would block zooming in, inspection, replay, and alternate grouping modes.

Decision:

- Raw items stay intact.
- `compact()` means index compaction only.
- Render plans contain explicit folded cluster items when the query/viewport requires them.
- Folded clusters summarize count/range/group/severity/representative metadata and can later support details-on-demand.

### Dataset IDs And External Lifecycle IDs

Question: Should timeline item IDs reuse external IDs such as tracing span IDs, job IDs, or Tracy zone indexes?

Response: No. The core dataset owns internal IDs.

Clarification:

- Different adapters have different lifecycle identity rules.
- A tracing span ID, Tracy zone index, job ID, or calendar UID is source-specific and may not be stable across import/replay/session boundaries.
- The display model should be reusable across many sources.

Decision:

- `TimelineDataset` assigns `TimelineItemId`s.
- Adapters keep external maps such as `tracing::Id -> TimelineItemId`.
- Dataset insertion sequence is separate from item identity and is used for provenance and deterministic tie-breaking.

### Open Spans And Query Time

Question: How should running jobs/spans be represented?

Response: Raw spans have optional ends; queries must provide `now`.

Clarification:

- A running job should visually grow over time.
- The raw item should not be repeatedly mutated just to make it look longer.
- Querying must be deterministic and testable, so model code should not read wall-clock time directly.

Decision:

- Raw span items use `end: Option<TimelineInstantNs>`.
- Render projection materializes open spans to `TimelineViewportQuery::now`.
- `TimelineViewportQuery::now` is required for every query.

### Row Semantics

Question: Are rows stored raw on items, or derived at display time?

Response: Rows are derived projection.

Clarification:

- A job timeline may have sparse source IDs such as job 1 and job 207. Those IDs must not produce hundreds of empty visible rows.
- Different views may group the same data differently: by job, span name, target, object type, level, source, or calendar bucket.

Decision:

- Raw items carry semantic grouping keys and metadata, not fixed display rows.
- Query/grouping settings derive compact render rows.
- Row order must be deterministic.

### Facet, Arbitrary, And Tests

Question: How strongly should the first model lean on Facet and Arbitrary?

Response: Strongly, but not naively.

Clarification:

- Facet is already present and should support later inspection/reflection, serialization, and display discovery.
- Arbitrary should generate valid model values, not random invalid junk that every test must filter.
- Derived `Arbitrary` is only safe for unconstrained structures.

Decision:

- Derive Facet for value-like model structs where appropriate.
- Implement `Arbitrary` for useful model/query types.
- Manually implement `Arbitrary` when invariants require coordination, such as valid ranges, unique IDs, open spans, parent relationships, grouped datasets, and query ranges.
- Add synthetic production data generation outside `cfg(test)` so the playground and tests share the same data source.

### Object References In Timeline Items

Question: Should timeline items carry rich typed payloads directly?

Response: Not in the first slice. They should carry lightweight object references and primitive fields.

Clarification:

- Timeline items should remain lightweight and log/export friendly.
- Future payloads such as audio buffers, transcript chunks, images, Tracy capture slices, or render snapshots belong in a typed object store.
- The display/query model should be able to show basic summaries without loading large payloads.

Decision:

- Add a minimal `TimelineObjectRef` concept: object ID plus type key.
- Keep primitive fields on timeline items for filtering/search/display.
- Defer typed object pool implementation.
- Future inspection can use Facet shape/value information from referenced objects.

### Object References In Tracing Fields

Question: Can Teamy define a custom `tracing::Value` implementation for object references?

Response: No. `tracing_core::field::Value` is sealed.

Clarification:

- The source confirmed `Value` cannot be implemented for arbitrary Teamy types.
- Tracing object references must therefore use primitive fields.

Decision:

- Standardize primitive tracing fields for one primary object reference:
  - `object.id` as `u64`
  - `object.type_key` as `str`
- The observability adapter reconstructs `TimelineObjectRef` from those fields.

### Tracey Specification Strategy

Question: Should this work create a new Tracey spec file?

Response: Not initially.

Clarification:

- The existing timeline spec already covers the Timeline surface.
- The existing observability spec already covers logs/toasts.
- The new model supports both areas rather than defining a wholly separate product surface yet.

Decision:

- Extend `docs/spec/product/timeline.md` for display-model/query/render-projection behavior.
- Extend `docs/spec/product/observability.md` for object-ref tracing conventions and future span timeline ingestion.
- Create a new spec later only if the object store or playground becomes a distinct user-facing subsystem.

### First Visible Playground Slice

Question: Should the first UI consumer of the new timeline model be synthetic, live logs/jobs, or a hybrid?

Response: Synthetic playground first.

Clarification:

- The new timeline model is currently represented in code and tests, not in a visible UI.
- A synthetic playground lets Teamy play with the new render-plan behavior without first solving live tracing span lifecycle capture or replacing the current Jobs window.
- The codebase already has launcher-style scene windows and a production synthetic dataset generator, so a launcher-accessible playground is the smallest visible path.

Decision:

- Add a separate Timeline Playground launcher action/window before live logs/jobs ingestion.
- Keep names source-agnostic so live observability and Progress Hub can reuse the same surface later.
- Stay synthetic-only in the first implementation, with live logs/jobs deferred until the playground proves the interaction model.

### Playground Controls And Interactions

Question: What makes the first playground playable?

Response: It must include pan, zoom, folding visibility, hover details, pinned details, seed regeneration, grouping controls, and folding-threshold controls.

Clarification:

- The first model already supports grouping modes and minimum visible pixel thresholds, so the UI should expose those knobs instead of hiding them in tests.
- Synthetic data should be deterministic by default, with a Regenerate button that advances the seed. Full config sliders can come later.
- Hover details are required in the first slice because visual rendering alone does not let the user inspect whether render items preserve the intended data.

Decision:

- Add a Regenerate control that changes the synthetic seed.
- Add visible grouping-mode controls for group key, source key, label, and all-items grouping.
- Add a visible folding-threshold control.
- Add pan and zoom controls tied to render-plan recomputation.
- Treat folded clusters as first-class inspectable render items instead of immediately drilling into representative items.

### Hover And Pinned Detail Windows

Question: Should hover details appear in the playground window, near the cursor, or in a separate window?

Response: Use a lazy sidecar detail-window object pool.

Clarification:

- The desired behavior is closer to a separate object/detail window than an in-window inspector.
- Hover should not spawn unbounded windows. A lazy pool should create or reuse a transient inspector window.
- Left-clicking a rendered item or cluster should promote the current hover detail into a pinned detail window, then allow a new transient hover inspector to be used.
- Stable sidecar placement adjacent to the playground window is preferred over cursor-chasing behavior for the first slice.

Decision:

- Implement a lazy detail-window pool, initially with one transient hover inspector plus click-to-pin promotion.
- Position detail windows adjacent to the playground window, preferring the right side and falling back later when needed.
- Use left-click on spans, events, folded span clusters, and folded event clusters to pin the current detail.
- Keep right-drag and mouse wheel available for timeline panning and zooming.

### Detail Window Content

Question: Should detail windows show handcrafted summaries, raw timeline structs, or reflected dumps?

Response: Show reflected dumps using `facet-pretty`, but dump a resolved hover detail view model rather than raw interned timeline structs.

Clarification:

- Raw `TimelineItem` values contain interned string IDs, which are useful internally but not enough for readable inspection.
- A purpose-built detail model can include both the render item context and resolved label/source/group strings.
- The first slice should not build the future typed object store, but it should show primitive fields and lightweight object refs.

Decision:

- Add a Facet-derived hover detail view model for playground inspection.
- Include render item kind, item ID, sequence, resolved label/source/group keys, timing, open state, primitive fields, object refs, and cluster count/range/representative metadata.
- Render detail content with `facet_pretty::FacetPretty`.

### Playground Focus And Pan Regression Fixes

Question: What did manual playground play reveal after the first visible slice?

Response: First-hover detail window creation appeared to focus the detail window and downgrade playground performance, and right-click drag panning was ignored.

Clarification:

- The log showed `timeline right-button pan ignored outside timeline pan surface` from `src/app/windows_app.rs` while the scene kind was `Timeline Playground`.
- The old right-button pan gate required `SceneWindowKind::Timeline`, a `TimelineDocument`, and `timeline_selection_surface_contains`, which is correct for the editor timeline but wrong for the synthetic playground.
- The sidecar hover detail window should behave like an inspector/tool window. It should not take focus from the playground window when it appears on hover.

Decision:

- Create `TimelinePlaygroundDetail` scene windows with `WS_EX_NOACTIVATE` and `WS_EX_TOOLWINDOW` at `CreateWindowExW` time, not only through show-time no-activate calls.
- Add playground-specific right-drag pan state and hit testing over the playground ruler/content surface.
- Keep the main timeline document panning code separate from playground panning so the editor timeline keeps its document/track-scroll behavior.

References:

- `docs/spec/product/timeline.md`: `timeline[playground.hover-detail-no-activate]`.
- `src/app/windows_app.rs`: `scene_window_ex_style`, `TimelinePlaygroundPanDrag`, `timeline_playground_pan_interaction_at_point`, and `apply_timeline_playground_pan_drag`.

## Current Implementation Commitment

The next implementation should add the first visible synthetic Timeline Playground:

- Add a launcher-accessible Timeline Playground scene window.
- Generate synthetic timeline data from `src/timeline/synthetic.rs`.
- Render the dataset through `TimelineViewportQuery` and `TimelineRenderPlan`.
- Expose regenerate, grouping, folding-threshold, pan, and zoom controls.
- Add hover hit testing and a lazy sidecar detail-window pool with click-to-pin promotion.
- Render resolved hover detail view models with `facet-pretty`.
- Do not wire live logs or jobs in the same slice.

## Known Non-Decisions

- Exact final names for every type and enum variant remain open.
- The first render row grouping modes are GroupKey, SourceKey, Label, and All, matching `TimelineGroupingMode`.
- Whether to support multiple object refs per item immediately remains open.
- The future typed object store location and API remain open.
- The playground launcher exposure is the main launcher.
- The old Jobs system is not removed until Progress Hub has equivalent or better structured observability visibility.

## Preservation Notes

- `docs/notes/timeline-display-model-plan.md` is the implementation roadmap.
- This record is the conversational memory aid.
- If future work changes one of these decisions, add a new dated section here instead of silently rewriting the original rationale.
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
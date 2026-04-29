# Timeline Display Model And Playground Plan

## Goal

Build a reusable, Facet-backed timeline display model that can power an intermediary playground first, then the Progress Hub, live observability timelines, Tracy capture viewing, calendar-like timelines, and later object-backed inspection workflows.

The first implementation slice is model-first and tests-first. It should create strict timeline data/query modules with Arbitrary support, synthetic playground data, compact render-plan projection, and index compaction. A visible playground window comes after the model/query layer proves the invariants.

## Current Status

- Done so far:
  - Read Teamy-Studio repo instructions. Validation must use `./check-all.ps1`; Tracey spec alignment matters.
  - Read the existing spatial design note in `docs/notes/spatial.md`.
  - Reviewed existing timeline code in `src/timeline/mod.rs`, scene timeline rendering in `src/app/windows_scene.rs`, timeline interactions in `src/app/windows_app.rs`, and observability/log capture in `src/logs.rs`.
  - Confirmed Teamy already has `facet`, `facet-json`, `facet-csv`, `facet-pretty`, and `arbitrary` dependencies.
  - Confirmed the current timeline model already has integer nanosecond time and pan/zoom projection, but its range constructor normalizes reversed ranges, which conflicts with the new strict display-model invariant.
  - Reviewed Tracy profiler's timeline implementation for grounding. Tracy keeps raw capture data intact, builds per-viewport draw lists, folds zones/messages below a pixel threshold, and treats aggregation as a render/query projection rather than source mutation.
  - Confirmed `tracing_core::field::Value` is sealed, so Teamy cannot implement custom `tracing::Value` for object refs directly.
  - Implemented Phase 1 in `src/timeline/time.rs`: strict `TimelineInstantNs`, `TimelineDurationNs`, and `TimelineRangeNs`, with Facet derives, valid Arbitrary generation, reversed-range rejection, duration tests, and module exports from `src/timeline/mod.rs`.
  - Validated Phase 1 with `cargo test timeline::time` and `./check-all.ps1`. The first full validation attempt hit a Tracey daemon startup timeout after the initial dashboard build took slightly longer than the client's 5 second wait; rerunning after the daemon was warm passed.
  - Implemented Phase 2 in `src/timeline/dataset.rs`: dataset-owned item IDs and insertion sequences, interned metadata IDs, primitive fields, object refs, checked span/event mutation, write-log tracking, index compaction, full index rebuild, Facet-backed value types, and valid Arbitrary dataset generation.
  - Validated Phase 2 with `cargo test timeline::dataset` and `./check-all.ps1`.
  - Implemented Phase 3 in `src/timeline/query.rs`: viewport queries with explicit `now`, grouping modes, derived compact rows, render-plan revision metadata, open-span materialization, visible span/event projection from compacted indexes, folded span clusters, folded event clusters, and Arbitrary query render safety tests.
  - Validated Phase 3 with `cargo test timeline::query` and `./check-all.ps1`.
  - Implemented Phase 4 in `src/timeline/synthetic.rs`: production synthetic timeline data generation with deterministic config, dense span clusters, event bursts, open job spans, sparse group keys, repeated primitive metadata, object-reference-bearing events, and renderability tests.
  - Validated Phase 4 with `cargo test timeline::synthetic` and `./check-all.ps1`.
  - Implemented Phase 5 by extending `docs/spec/product/timeline.md` with reusable display-model requirements and mapping the new `time`, `dataset`, `query`, and `synthetic` modules/tests to those requirements.
  - Added the missing existing `timeline[add-track.microphone-placeholder]` requirement because `tracey query validate --deny warnings` found old implementation/test references to it.
  - Validated Phase 5 with `tracey query uncovered`, `tracey query validate --deny warnings`, and `./check-all.ps1`. Tracey status reports `teamy-studio-timeline/rust` at 50 of 50 requirements covered, with 41 verification references.
  - Started Phase 6 by making existing editor `TimelineTimeRangeNs::new` strict, adding `TimelineTimeRangeNs::try_new`, and moving pointer-drag boundaries to explicit `TimelineTimeRangeNs::from_unordered` calls.
  - Validated the first Phase 6 migration step with `cargo test editor_time_range` and `./check-all.ps1`.
  - Implemented the first visible synthetic Timeline Playground: launcher entry, synthetic render-plan scene, grouping/folding/regenerate/pan/zoom controls, hover hit testing, pooled sidecar hover details, click-to-pin detail windows, and resolved `facet-pretty` detail output.
  - Added Tracey requirements and focused tests for the playground launcher, controls, synthetic render-plan hit targets, and detail output.
  - Validated the playground slice with `cargo test timeline::playground`, `cargo test timeline_playground`, `tracey query validate --deny warnings`, `tracey query uncovered`, and `./check-all.ps1`. Tracey status reports `teamy-studio-timeline/rust` at 57 of 57 requirements covered, with 48 verification references.
  - Polished the playground interaction slice by adding top ruler tick marks and labels, cursor-anchored mouse-wheel zoom, ease-in-out zoom transitions, and VT-aware rendering for styled `facet-pretty` detail text.
  - Validated the polish pass with `cargo test timeline_playground`, `tracey query validate --deny warnings`, and `./check-all.ps1`. Tracey status reports `teamy-studio-timeline/rust` at 61 of 61 requirements covered, with 52 verification references.
  - Fixed live playground regressions found during manual play: hover detail windows are now created as non-activating tool windows so first hover does not steal focus from the playground, and right-drag panning now has a playground-specific drag path over the ruler/content surface instead of reusing the main timeline document pan gate.
  - Added `timeline[playground.hover-detail-no-activate]` plus focused tests for non-activating detail-window styles and playground right-drag pan hit testing/range movement.
  - Validated the regression fixes with `cargo test timeline_playground`, `tracey query validate --deny warnings`, and `./check-all.ps1`. Tracey status reports `teamy-studio-timeline/rust` at 62 of 62 requirements covered, with 53 verification references.
- Current focus:
  - Decide the next slice after playing with the synthetic Timeline Playground, likely either UX polish for the playground or the first live log/job adapter into the reusable display model.
- Remaining work:
  - Try the Timeline Playground manually and tune first-slice usability issues that are easier to judge in the live UI than in tests.
  - Decide whether the sidecar detail-window pool needs stronger lifetime/ownership handling before broader use, because the first slice intentionally uses a simple shared handle.
  - Consider adding richer synthetic controls later, such as item count, burst density, open-span ratio, and time range presets.
  - Migrate the existing timeline editor model to the new strict types in phases, then delete old normalizing range semantics.
- Next step:
  - Open the Timeline Playground from the launcher and play with grouping, folding, pan/zoom, hover details, and pinned details to choose the next implementation slice.

## Constraints And Assumptions

- Teamy should keep the codebase coherent. Temporary side-by-side types are allowed only as a migration bridge with an explicit deletion path.
- The new model belongs under `src/timeline/`, split into explicit submodules instead of growing `src/timeline/mod.rs` further.
- The reusable display model must not be coupled to `tracing::Id`, Tracy file internals, job IDs, or calendar IDs. Source adapters own external lifecycle IDs and map them to dataset-assigned `TimelineItemId`s.
- Time remains integer nanoseconds internally. UI projection may use floating point only at the viewport/rendering boundary.
- Reversed ranges are illegal in the core display model. Constructors return `eyre::Result` and reject `end < start`.
- Open/running spans are represented with `end: Option<TimelineInstantNs>` in raw items. Render projection closes them using the required `TimelineViewportQuery::now` value.
- `TimelineViewportQuery::now` is required for every query. Model/query code must not read ambient wall-clock time.
- Raw items remain intact in the first implementation. Compaction means index compaction only, not raw-item eviction or lossy retention.
- Render plans read the compacted/indexed view. Pending writes should be short-lived because compaction is intended to be cheap and frequent.
- Object references are lightweight handles to future typed object-store payloads. Large or structured data should not be carried in tracing fields or timeline primitive fields.
- `tracing::Value` is sealed. Object refs in tracing events should be emitted as standardized primitive fields, not custom `Value` implementations.

## Product Requirements

- The intermediary playground must eventually display synthetic randomized timeline data generated from the same production module used by tests.
- The first visible playground slice should be synthetic-only, source-agnostic, and launcher-accessible.
- The first visible playground slice should expose seed regeneration, grouping mode, and folding threshold controls so users can directly exercise row derivation and dense-item clustering.
- The first visible playground slice should support pan and zoom over synthetic timeline data, then recompute the render plan from the updated viewport.
- Right-drag panning in the playground should work over the ruler/content surface even though the playground does not own a `TimelineDocument`.
- Hovering a rendered span, event, folded span cluster, or folded event cluster should open or update a pooled sidecar detail window.
- Hover detail windows should not activate or steal focus from the playground, because focus changes alter the render cadence of the primary window.
- Left-clicking a rendered span, event, folded span cluster, or folded event cluster should promote the current hover detail into a pinned detail window.
- Hover and pinned detail windows should display a resolved Facet-derived detail view model with `facet-pretty`, not raw interned IDs alone.
- The Progress Hub should eventually reuse the display model instead of reviving the hard-coded Jobs window model.
- A job/progress timeline should display spans as duration clips that grow while open, cap when ended, and support hover details and later pinned detail windows.
- Instant events should render as markers/carets and fold into clusters when too dense.
- Duration spans should render as clips and fold into clusters when their projected width is below the current minimum visible pixel threshold.
- Rows are derived from grouping/filter projection, not raw IDs. Sparse source IDs such as `job 1` and `job 207` must not create hundreds of empty visible rows.
- Aggregated render items must remain interactive. Hover can summarize; click/middle-click later can inspect or zoom into the represented range.
- The model must be able to represent timeline data from live tracing, Tracy capture files, synthetic samples, job/progress observations, object-backed audio events, and calendar-like sources.
- The first window should come after model/query tests, and should consume the render plan rather than inferring aggregation in the renderer.

## Architectural Decisions Already Made

- Raw source data and render projection are separate.
- Aggregation is a viewport/query/render artifact, not mutation of raw timeline data.
- `TimelineRenderItem` should be explicit, with variants such as individual span, instant event, folded span cluster, and folded event cluster.
- Row placement is derived from grouping/filter settings. Raw items carry semantic grouping keys, not fixed row numbers.
- Folded clusters should store range/count/query metadata plus a representative item, not a full embedded list of every raw item ID by default.
- `TimelineDataset` is mutable through checked APIs. It owns internal item IDs and sequence numbers.
- Dataset insertion sequence is provenance and a stable tie-breaker, not display ordering.
- Dataset mutation APIs preserve invariants and append write-log entries used for index compaction.
- `compact()` promotes pending writes into indexes and clears the write log. It does not discard raw items.
- `rebuild_index()` is a full-index repair/bulk-import path that scans raw items from scratch.
- Source adapters maintain external lifecycle maps such as `tracing::Id -> TimelineItemId`. The core dataset does not know external IDs.
- Repeated metadata should be interned into dataset-owned IDs rather than repeated as strings on every item.
- `TimelineObjectRef` contains only object ID and type key. Primitive summary fields stay as timeline fields or are discoverable by inspecting the referenced object through Facet later.
- Timeline item fields are primitive/log-compatible. Structured typed bodies live in the future object layer.
- Canonical object refs live on timeline items; primitive fields may mirror `object.id` and `object.type_key` for search/filter/export.
- Standard tracing field names for one primary object ref are:
  - `object.id` as `u64`
  - `object.type_key` as `str`

## Planned Module Layout

Initial new modules:

- `src/timeline/time.rs`
  - Strict `TimelineInstantNs`, `TimelineDurationNs`, and `TimelineRangeNs`.
  - `TimelineRangeNs::try_new(start, end) -> eyre::Result<Self>` rejects reversed ranges.
- `src/timeline/dataset.rs`
  - Facet-backed raw item model, interned metadata tables, checked mutable dataset APIs, write log, and index/revision metadata.
- `src/timeline/query.rs`
  - Viewport query, grouping/filter settings, render plan, render rows, render items, folding thresholds, and visible-range projection.
- `src/timeline/synthetic.rs`
  - Production synthetic/demo data generation used by tests and the later playground window. Do not guard this module behind `cfg(test)`.

Later split/migration modules:

- `src/timeline/editor.rs`
  - Eventual home for current `TimelineDocument` logic after it is migrated out of `mod.rs`.
- `src/timeline/adapters/observability.rs`
  - Future adapter from Teamy tracing/log/span observations into `TimelineDataset`.
- `src/timeline/adapters/tracy.rs`
  - Future adapter from Tracy capture data into `TimelineDataset`.

`src/timeline/mod.rs` should export compatibility names while shrinking over time. It should not become the long-term home of the new implementation.

## Data Model Direction

Core concepts:

- `TimelineDataset`
  - Owns raw items, intern tables, indexes, write log, revisions, and internal ID allocation.
- `TimelineItemId`
  - Assigned by the dataset.
  - Independent of job IDs, tracing IDs, Tracy zone indexes, or calendar UIDs.
- `TimelineItemSequence`
  - Monotonic insertion sequence used for provenance and tie-breaking.
- `TimelineItem`
  - Span item or instant-event item.
  - Contains interned labels, target/source/group keys, primitive fields, and object refs.
- `TimelineSpanItem`
  - Has start and optional end.
  - `finish_span` validates `end >= start` before mutating the raw item.
- `TimelineEventItem`
  - Has one timestamp.
- `TimelineField`
  - Primitive/log-compatible key/value pair.
- `TimelineFieldValue`
  - Expected first variants: bool, i64, u64, f64, and interned string.
- `TimelineObjectRef`
  - Minimal object handle: object ID plus interned type key.
- `TimelineInternTable`
  - Stores repeated strings/type keys/group names once.

Mutation API direction:

```rust
dataset.push_span(...)? -> TimelineItemId
dataset.finish_span(item_id, end)?
dataset.push_event(...)? -> TimelineItemId
dataset.compact()? -> TimelineCompactionReport
dataset.rebuild_index()? -> TimelineCompactionReport
dataset.render_plan(&query)? -> TimelineRenderPlan
```

The concrete signatures should be chosen during implementation, but mutation must preserve invariants and never expose raw `Vec` mutation for items/indexes.

## Query And Render-Plan Direction

`TimelineViewportQuery` should include:

- visible time range
- required `now`
- viewport width or equivalent scale information
- grouping mode
- filters
- minimum visible pixel threshold for folding
- possibly row/vertical range once the UI window is added

`TimelineRenderPlan` should include:

- dataset revision
- compacted/index revision
- pending write count
- render rows
- render items

`TimelineRenderItem` should include variants for:

- individual span clip
- individual open span clip materialized to query `now`
- individual instant event marker
- folded span cluster
- folded event cluster

Folded clusters should include:

- time range
- row/group key
- count
- representative item ID or sequence
- severity/level summary if available
- enough query metadata to resolve details on demand later

The render plan should be an already-decided draw/query plan. Scene rendering should not decide whether raw items need folding.

## Arbitrary And Synthetic Data Strategy

- Implement `Arbitrary` for model types that are useful in tests.
- Derive `Arbitrary` only when every generated value preserves invariants.
- Manually implement `Arbitrary` for constrained or coordinated types such as ranges, non-empty keys, unique IDs, datasets, parent relationships, queries, and any value whose fields must agree.
- Arbitrary datasets must be valid. They should not generate reversed ranges, duplicate internal IDs, invalid parent references, or malformed open spans.
- Tests should be able to generate valid arbitrary instances, mutate them into a targeted scenario, and assert behavior without hand-building elaborate fixtures.
- `timeline::synthetic` should provide production demo/randomized datasets for both tests and the future playground window.

## Object Reference And Object Store Direction

The first slice includes object references only. It does not implement the typed object pool.

Object refs:

```text
TimelineObjectRef
- object_id
- type_key
```

Future object store direction:

- Use runtime `TypeId` for type-safe in-process lookup/downcast.
- Use a portable namespaced type key or Facet shape identity for display, serialization, replay, and cross-process import.
- Store rich payloads such as microphone samples, images, transcript chunks, Tracy capture slices, and render snapshots outside timeline items.
- Let inspection query the Facet shape/value later, for example fields whose name contains `size` and whose type is `uom::Information`.

Tracing convention for events that refer to one primary object:

```rust
tracing::info!(
    object.id = object_id,
    object.type_key = type_key,
    sample_rate = 48_000_u64,
    byte_count = 9_600_u64,
    "microphone data received"
);
```

The observability adapter reconstructs `TimelineObjectRef` from the primitive fields. Do not rely on custom `tracing::Value` implementations for object refs because `tracing_core::field::Value` is sealed.

## Tracey Specification Strategy

This work extends two existing spec areas:

- `docs/spec/product/timeline.md`
  - strict display/query model
  - raw data versus render projection
  - render-plan folding for dense spans/events
  - derived row projection
  - explicit `now` for open spans
  - synthetic playground data once the window exists
- `docs/spec/product/observability.md`
  - span lifecycle capture into timeline objects later
  - standardized object-ref tracing fields
  - future Progress Hub consuming timeline projection instead of the old Jobs board

Create a new dedicated spec only if the object store becomes large enough to be a separate user-facing subsystem. For the first model/query slice, extending timeline and observability is sufficient.

Tracey baseline loop for this plan:

```powershell
tracey query status
tracey query uncovered
tracey query unmapped
tracey query unmapped --path src/timeline
tracey query validate --deny warnings
tracey query untested
```

Final repo validation remains:

```powershell
.\check-all.ps1
```

## Phased Task Breakdown

### Phase 1: Strict Time Types

Objective: Add strict generalized timeline time/range primitives without disturbing existing editor behavior yet.

Tasks:

- Add `src/timeline/time.rs`.
- Define `TimelineInstantNs`, `TimelineDurationNs`, and `TimelineRangeNs`.
- Make reversed ranges impossible through public constructors; return `eyre::Result` on invalid input.
- Add manual `Arbitrary` where needed so generated ranges are always valid.
- Add tests for valid range creation, reversed range rejection, duration calculation, and ordering.
- Wire the module through `src/timeline/mod.rs` without migrating existing editor call sites yet.

Definition of done:

- Strict time/range types compile and are exported.
- Tests prove reversed ranges are rejected.
- Existing timeline tests continue to pass.
- No existing normalizing range behavior has been removed yet.

### Phase 2: Dataset, Interning, And Checked Mutation

Objective: Create a mutable but invariant-preserving raw timeline dataset.

Tasks:

- Add `src/timeline/dataset.rs`.
- Define dataset-owned IDs, sequence numbers, interned names/type keys, primitive fields, object refs, span items, event items, and dataset revisions.
- Add checked APIs for `push_span`, `finish_span`, and `push_event`.
- Track a write log for pending index updates.
- Add `compact()` and `rebuild_index()` as index operations only.
- Implement Facet for value-like structs.
- Implement Arbitrary for all useful model types, manually where invariants require it.
- Add tests for unique dataset-assigned IDs, sequence tie-breakers, finishing open spans, rejecting invalid finishes, pending write counts, compaction, and rebuild equivalence.

Definition of done:

- A valid dataset can be built and mutated without exposing raw item/index mutation.
- Open spans can be closed by internal `TimelineItemId`.
- `compact()` updates indexes without discarding raw items.
- `rebuild_index()` produces equivalent indexed query inputs from raw items.

### Phase 3: Query And Render Plan

Objective: Project compacted timeline data into a viewport-specific render plan.

Tasks:

- Add `src/timeline/query.rs`.
- Define `TimelineViewportQuery` with required `now`.
- Define grouping/filter settings and render rows.
- Define `TimelineRenderPlan` and explicit `TimelineRenderItem` variants.
- Implement row projection as derived, compact, mode-dependent output.
- Implement visible-range querying against the compacted index.
- Implement pixel-threshold folding for span clips and instant-event markers, grounded in Tracy's `MinVisSize` approach.
- Include pending write metadata in render plans.
- Add tests for active-job row compaction, sparse job IDs, folded span clusters, folded event clusters, zooming in to unfold clusters, open span materialization using query `now`, and deterministic tie-breaking.

Definition of done:

- Render plans are deterministic from dataset compacted revision plus query.
- Rows do not contain gaps from sparse source IDs.
- Aggregation is represented as render items and does not mutate raw items.
- Open spans render as growing clips without storing a synthetic end in the raw item.

### Phase 4: Synthetic Production Data

Objective: Provide production demo data for tests and the later playground window.

Tasks:

- Add `src/timeline/synthetic.rs`.
- Define synthetic configuration and generator APIs.
- Use the same checked dataset APIs as real adapters.
- Generate valid but awkward datasets: dense event bursts, many tiny spans, long open spans, sparse job IDs, repeated names, many sources/groups, empty datasets, and object-ref-bearing events.
- Add tests that synthetic datasets can compact and render without panics across generated queries.

Definition of done:

- Synthetic generation is available outside `cfg(test)`.
- Generated datasets are valid by construction.
- Tests use generated data for broad query/render coverage.

### Phase 5: Tracey Spec And Coverage Mapping

Objective: Make the new model behavior observable in the project specification system.

Tasks:

- Extend `docs/spec/product/timeline.md` with display-model requirements.
- Extend `docs/spec/product/observability.md` with object-ref field conventions if implementation lands in this slice.
- Add implementation markers in new modules and tests as requirements are implemented.
- Run the Tracey baseline loop and update this plan with current coverage status.

Definition of done:

- New intentional behavior has Tracey requirements.
- New implementation and tests are mapped to requirements.
- `tracey query validate --deny warnings` passes or exact blockers are recorded.

### Phase 6: Existing Timeline Migration

Objective: Reduce the codebase by migrating current timeline editor code onto the strict model where appropriate.

Tasks:

- Identify current uses of `TimelineTimeNs` and `TimelineTimeRangeNs::new()` that rely on normalization.
- Add explicit adapter or validation paths for selection drag cases where unordered points are expected.
- Migrate current editor document/range code to strict constructors phase by phase.
- Split current `src/timeline/mod.rs` into smaller modules as migration proceeds.
- Delete old normalizing semantics after all call sites are explicit.

Definition of done:

- Existing timeline behavior keeps working.
- Reversed ranges are handled explicitly at interaction/import boundaries, not silently by the core range type.
- The old duplicated type/constructor path is removed.

### Phase 7: Intermediary Playground Window

Objective: Build the visible playground that exercises the reusable UX language before Progress Hub.

Tasks:

- Add a launcher action for the timeline display playground.
- Render synthetic datasets through the render-plan API.
- Reuse existing timeline pan, right-drag, wheel zoom, ruler, and scrollbar behavior where feasible.
- Add toggles for jobs-only, follow latest, and grouping modes.
- Add hover details for spans/events/clusters.
- Add click-to-pin detail windows after hover details are stable.

Definition of done:

- The playground window displays randomized synthetic timeline data.
- Pan/zoom/folding behavior can be visually exercised before live observability is wired.
- The Progress Hub can be designed as a consumer of the same render-plan model rather than a custom Jobs window.

### Phase 8: Observability Adapter And Progress Hub Replacement

Objective: Convert tracing/log/job observations into timeline data and retire the old Jobs board.

Tasks:

- Extend `src/logs.rs` or add an observability adapter to record span lifecycle observations in addition to log events.
- Maintain adapter-owned maps from external tracing IDs to internal `TimelineItemId`s.
- Convert job start/update/finish/fail signals to structured tracing/span events or adapter calls.
- Build Progress Hub as a filtered/grouped timeline view.
- Remove or deprecate `src/app/jobs.rs` once equivalent progress visibility exists.

Definition of done:

- Progress Hub shows running and completed work using the shared timeline display model.
- Existing job feedback is available through structured observability data.
- The hard-coded Jobs board has a removal path or is removed.

## Recommended Implementation Order

1. `timeline::time` strict types and tests.
2. `timeline::dataset` with intern tables, checked mutation, write log, compaction, Facet, and Arbitrary.
3. `timeline::query` render plan and folding tests.
4. `timeline::synthetic` production data generation.
5. Tracey spec updates and implementation mapping for the first model/query slice.
6. Existing timeline type migration plan updates based on concrete call-site friction.
7. Playground window.
8. Observability adapter and Progress Hub replacement.

## Open Decisions

- Exact enum/type names for grouping modes and render item variants.
- Whether row projection should initially support parent/child nesting or only flat grouping.
- Whether string interning should use a custom table immediately or start with simple IDs over `Vec<String>` and optimize later.
- How much pending-write metadata the first render plan should expose beyond count and revisions.
- Whether the first object-ref support should allow multiple refs per item immediately or one primary ref plus future expansion.
- Where the future object store should live and how it should expose Facet-backed inspection APIs.
- Whether the playground should be exposed from the main launcher immediately or hidden behind a diagnostics/development action.

## First Concrete Slice

Implement Phase 1 only:

- Add `src/timeline/time.rs`.
- Export the strict time types from `src/timeline/mod.rs`.
- Add tests for strict range construction, reversed range rejection, duration, ordering, and Arbitrary validity.
- Do not migrate existing timeline editor code yet.
- Run focused tests if possible, then `./check-all.ps1` when the slice is ready.

After Phase 1, update this plan's Current Status before continuing to Phase 2.
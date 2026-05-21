# Timeline Session, Live Ingestion, And Analysis Plan

## Goal

Turn Teamy Studio's timeline into a session-based, multi-window inspection surface that can gracefully handle live data while it is still changing, keep interaction responsive during pan/zoom/hover, render nested spans reliably, and expose Tracy-style Messages and Statistics companion windows as real OS windows.

This plan is intentionally broader than the current Timeline Playground polish work. The next milestone is not one more local tweak. It is a shared timeline-session architecture with scalable live-query behavior, dedicated auxiliary windows, and a profiling workflow that explains the remaining hot paths.

## Current Status

- Done so far:
  - Read Teamy-Studio repository constraints. The current codebase is preservation-first, but the crate split is largely complete, so targeted timeline architecture work is now allowed when it is grounded and validated.
  - Reviewed the existing timeline product and playground plans in [docs/notes/timeline-profiler-plan.md](docs/notes/timeline-profiler-plan.md) and [docs/notes/timeline-display-model-plan.md](docs/notes/timeline-display-model-plan.md).
  - Reviewed the current timeline product spec in [docs/spec/product/timeline.md](docs/spec/product/timeline.md) and the Tracey config in [.config/tracey/config.styx](.config/tracey/config.styx).
  - Confirmed the current Teamy window model includes Timeline, Timeline Playground, and Timeline Detail windows, but does not yet define dedicated Messages or Statistics windows.
  - Confirmed the current playground detail model lives in `teamy_studio_timeline_core` and is surfaced through sidecar detail windows rather than through a shared timeline session model.
  - Profiled the current playground path with Tracy and confirmed the D3D12 renderer is not the dominant bottleneck. Recent captures show `render_thread_render_frame` is sub-millisecond on average while interaction still feels slow.
  - Fixed one live-data cost center by changing the live tracing dataset path to append onto the cached timeline dataset when the bounded log buffers only grow at the tail, instead of rebuilding the whole dataset every revision.
  - Removed one remaining whole-dataset handoff copy by storing the live dataset cache and the playground dataset behind `Arc<TimelineDataset>`.
   - Added focused Tracy spans around live dataset sync, playground query/render-plan building, scene construction, and hit-test query/render-plan work.
   - Added an initial app-host render-plan cache keyed by dataset plus viewport query so hover hit testing can reuse the current render plan instead of always rebuilding it independently.
  - Added dedicated launcher quit flow and timeline playground close-focus behavior in earlier adjacent work.
  - Reviewed Tracy reference surfaces in `TracyView.hpp`, `TracyTimelineItemThread.cpp`, and the existing Teamy notes that cite `TracyView_ZoneTimeline.cpp` and related timeline files.
- Current focus:
   - Re-run the timeline playground capture with the new spans and the first render-plan reuse slice in place so the next decision is based on measured remaining cost, not the pre-cache trace.
- Remaining work:
  - Build a shared timeline-session layer so one timeline can own a canvas window plus separate Messages and Statistics windows.
  - Replace the current fixed-height row and clamped span-lane model with a row layout that can express visible nested spans without collapsing or visually disappearing.
  - Stop recomputing the full render plan on every interaction path that only needs hit testing or companion-window updates.
  - Add dedicated Messages and Statistics windows with selection/focus behavior modeled after Tracy but adapted to Teamy's native multi-window scene model.
  - Extend the timeline spec and Tracey mappings to cover timeline sessions, messages/statistics windows, live-data performance, and nested span-lane behavior.
- Next step:
  - Capture a new Tracy profile with the added live-sync/query/hit-test spans, then use that data to decide the first structural code slice: shared render-plan caching for paint plus hit testing, or the deeper query/index redesign if the cache hit is already cheap.

## Constraints And Assumptions

- The existing timeline display-model work is not throwaway. New architecture should reuse `TimelineDataset`, `TimelineViewportQuery`, `TimelineRenderPlan`, and the current render-item vocabulary where they still fit.
- Teamy is now allowed to improve timeline architecture deliberately, but changes should still be incremental, testable, and avoid broad unrelated churn.
- The timeline product spec already exists in [docs/spec/product/timeline.md](docs/spec/product/timeline.md). This work should extend that spec rather than creating a second timeline-like product namespace.
- Tracy is reference material, not a code donor. Reimplement behavior in Teamy's own Rust and Win32 scene model.
- Multiple timeline windows must be supported. Messages and Statistics are not global singleton panes. They are companion windows attached to a specific timeline session.
- Live data must remain inspectable while still mutating. The system should avoid blocking UI input on full data-model rebuilds.
- Raw timeline data and viewport-derived presentations stay separate. Messages, statistics, and rendered rows are all derived views over shared session data, not independent sources of truth.
- The current `tracey query status` baseline could not be refreshed in this session because the local daemon auto-start failed with `os error 2`. The plan should still include the standard Tracey loop, but the first implementation pass may need to fix or rerun the daemon before recording a new baseline.

## Product Requirements

- The timeline surface must stay responsive while live data is arriving and while the user is panning, zooming, hovering, and selecting.
- A timeline session must support multiple native windows at once: the main timeline canvas, a Messages window, and a Statistics window, with room for later detached tool windows.
- Opening a timeline should open or be able to open session-specific companion windows rather than routing everything through one global app window.
- Clicking a message/event marker in the timeline should be able to focus or synchronize the Messages window to the selected entry.
- Messages must include ordinary point messages and span lifecycle observations needed for inspection. Closed spans should be navigable both as duration clips in the timeline and as entries in the message-oriented companion views.
- Statistics must aggregate spans by a stable key that includes the span label plus its structured parameters. The same keying rule should be explicit for live Teamy observations and imported Tracy data.
- Statistics should support time-range-aware summaries so the user can inspect the visible range or another explicit selection instead of only whole-capture totals.
- Nested spans in one logical thread row must remain visible and legible. The row model cannot assume a fixed bucket height that silently clamps additional lanes into overlap.
- The system must support multiple timelines open at the same time without companion windows leaking state across sessions.
- The main canvas, Messages window, and Statistics window must all derive from shared session data and revision tracking instead of copying or rebuilding unrelated state ad hoc.

## First-Principles Findings

1. The renderer is not the main bottleneck.
   - Recent Tracy captures show [render_thread_render_frame](g:\Programming\Repos\Teamy-Studio\crates\teamy_studio_shell\src\windows_d3d12_renderer.rs#L870) averaging well under 1 ms.
   - That means low interactive frame rate is currently dominated by upstream app-host, query, or interaction work.

2. The timeline query path still scales with total dataset size, not visible content.
   - [TimelineDataset::render_plan](g:\Programming\Repos\Teamy-Studio\crates\teamy_studio_timeline_core\src\timeline_impl\query.rs#L304) linearly scans every span in `span_index()` and every event in `event_index()` for each query.
   - The indexes are sorted, but the current visible-query implementation does not exploit lower bounds or per-row/per-range caches yet.

3. Interaction does duplicate query work outside the paint path.
   - [timeline_playground_target_at_point](g:\Programming\Repos\Teamy-Studio\crates\teamy_studio_app_host\src\windows_app_impl.rs#L3199) builds a fresh query and render plan for hit testing.
   - This is separate from the render-path query in [render_scene_window_frame](g:\Programming\Repos\Teamy-Studio\crates\teamy_studio_app_host\src\windows_app_impl.rs#L8488), so mouse-move interaction can stay laggy even when the window is not repainting continuously.

4. Live ingestion is still coupled to UI-thread view updates.
   - [TimelinePlaygroundState::sync_live_tracing_events](g:\Programming\Repos\Teamy-Studio\crates\teamy_studio_app_host\src\windows_app_impl.rs#L940) runs on the UI-side playground state and updates the session dataset during the view-build path.
   - The recent cache improvements remove one major rebuild cost, but the session still lacks a dedicated background/incremental invalidation model.

5. The current row/lane geometry cannot gracefully express deep nesting.
   - Timeline rows in the shell currently use a fixed [TIMELINE_PLAYGROUND_ROW_HEIGHT](g:\Programming\Repos\Teamy-Studio\crates\teamy_studio_shell\src\windows_scene.rs#L72) of 74 px.
   - Span lanes are mapped through [timeline_playground_span_lane_rect](g:\Programming\Repos\Teamy-Studio\crates\teamy_studio_shell\src\windows_scene.rs#L2624), which clamps the lane top to the row bottom. Once visible lane depth exceeds available height, later lanes collapse into the same vertical space instead of growing the row.

6. Tracy already models the companion views we want.
   - Tracy keeps Messages and Statistics as separate first-class views over shared capture state in `TracyView`.
   - Tracy preprocesses tiny message markers per visible thread in `TimelineItemThread::PreprocessMessages`, and it keeps a `StatisticsCache` keyed by range/accumulation/source-count/thread-count in `TracyView.hpp`.
   - Teamy should copy the architectural lesson, not the UI code: one shared session model, multiple derived windows, explicit per-view preprocessing caches.

## Architectural Direction

### 1. Introduce a shared timeline session layer

Create a session object that owns the shared data and revision state for one timeline bundle.

Likely responsibilities:

- `TimelineSessionId` and session registry
- session-owned raw timeline data
- per-session viewport and selection state for the main canvas
- companion-window state for Messages and Statistics
- revision counters and invalidation notifications
- shared caches for render plan, message list, and statistics summaries

The session should be shareable across multiple scene windows via `Arc`, with interior mutability only where needed.

### 2. Separate raw observations from derived views

Keep raw events and spans as the durable source of truth, but stop treating every window as a place that can independently rebuild all derived state.

Derived views should include:

- main timeline render plan
- hit-test geometry or hit-test-ready render snapshot
- message list projection
- statistics aggregation tables

All of these should be keyed by session revision plus the relevant view parameters.

### 3. Add a query/cache layer that is visible-range-aware

The current `render_plan()` path should evolve toward range-aware indexed queries and cacheable results.

Desired properties:

- visible-query cost should scale with visible candidates, not total dataset size
- hit testing should reuse a cached view snapshot instead of rebuilding a render plan per mouse move
- messages and statistics should each maintain their own derived-cache shape instead of abusing timeline detail windows as a surrogate

### 4. Promote companion windows to first-class scene kinds

Add scene kinds for at least:

- `TimelineMessages`
- `TimelineStatistics`

These windows should open per session, not globally.

The main timeline window should be able to:

- open/focus the companion windows
- send navigation/focus requests to them
- receive selection changes back when needed

### 5. Redesign row height from fixed buckets to visible lane demand

The main timeline/playground span rendering should move away from a fixed 74 px thread bucket.

The row model should:

- compute visible lane depth per row from the current render plan
- derive row height from visible lane demand plus padding
- preserve stable row identity and colors while allowing height changes
- support virtualization and clipping for tall multi-lane rows

### 6. Keep statistics aggregation explicit and canonical

Statistics need a canonical aggregation key.

Recommended default key shape:

- item kind
- normalized label/title
- source or target identity when semantically meaningful
- canonical ordered field list

Statistics should store:

- count
- total duration
- min/max duration
- mean/median where useful
- visible-range and full-session variants when applicable

## Tracey Specification Strategy

Extend the existing timeline spec in [docs/spec/product/timeline.md](docs/spec/product/timeline.md).

New requirement clusters should cover:

- `timeline[session.shared-state]`
- `timeline[session.multi-window]`
- `timeline[messages.window]`
- `timeline[messages.selection-sync]`
- `timeline[statistics.window]`
- `timeline[statistics.aggregation-key]`
- `timeline[statistics.visible-range]`
- `timeline[live.query-cache]`
- `timeline[live.hit-test-cache]`
- `timeline[playground.row-dynamic-height]`
- `timeline[playground.span-lane-stability]`

Standard validation loop once the daemon is healthy:

```powershell
tracey query status
tracey query uncovered
tracey query unmapped
tracey query unmapped --path crates/teamy_studio_app_host/src/windows_app_impl.rs
tracey query unmapped --path crates/teamy_studio_shell/src/windows_scene.rs
tracey query unmapped --path crates/teamy_studio_timeline_core/src/timeline_impl
tracey query validate --deny warnings
tracey query untested
```

## Phased Task Breakdown

### Phase 0: Measurement And Session Baseline

Objective:
Add the minimum profiling and planning scaffolding needed to answer the next architectural question with data.

Tasks:

- Add focused Tracy spans around live dataset sync, playground query/render-plan building, and interaction hit testing.
- Record the first-principles findings from the current captures in this plan.
- Re-run Tracy captures for idle live ingestion, mouse hover, drag pan, wheel zoom, and detail-window hover.
- Refresh Tracey status once the daemon issue is resolved.

Definition of done:

- New captures can distinguish live sync, render-plan build, hit-test query, and scene construction.
- The next structural slice is chosen from measured evidence rather than guesswork.

### Phase 1: Shared Timeline Session Skeleton

Objective:
Give one timeline bundle a stable shared identity across windows.

Tasks:

- Introduce `TimelineSessionId` and a session registry.
- Add session-backed scene creation plumbing in app-host.
- Add placeholder `TimelineMessages` and `TimelineStatistics` scene kinds plus open/focus actions.
- Route the main timeline and companion windows through one session handle rather than isolated per-window state.

Definition of done:

- Opening a timeline creates one session object.
- Messages and Statistics windows can exist as separate OS windows associated with that session.
- Closing one timeline session does not disturb another.

### Phase 2: Query Snapshot And Hit-Test Reuse

Objective:
Stop rebuilding the full render plan independently for paint and mouse interaction.

Tasks:

- Add a session-owned cached view snapshot keyed by session revision plus viewport/grouping parameters.
- Reuse the same snapshot for paint, hover hit testing, and detail-window lookup.
- Invalidate cached snapshots only when the session revision or relevant view parameters change.

Definition of done:

- Mouse move over a static timeline no longer recomputes a full render plan on every event.
- Tracy captures show hit testing reusing cached projection data instead of duplicating paint-path work.

### Phase 3: Visible-Range Query Scalability

Objective:
Make query cost scale with visible content instead of total item count.

Tasks:

- Replace linear full-index scans in `TimelineDataset::render_plan()` with lower-bound/range-aware index walks or another equivalent visible-range strategy.
- Preserve raw item storage and current render-item semantics while improving visible-query complexity.
- Add focused tests for visible-query correctness around open spans, dense events, and sparse rows.

Definition of done:

- Visible-query cost drops substantially on captures with many offscreen events/spans.
- Render-plan timing tracks visible density much more closely than total dataset size.

### Phase 4: Messages Window

Objective:
Add a real session companion window for timestamped messages and span lifecycle entries.

Tasks:

- Define a session-derived message-list model.
- Implement a selectable Messages scene window.
- Add navigation from timeline marker selection to message-list focus.
- Define how closed spans appear in the message list versus only in the statistics window.

Definition of done:

- Clicking a message marker can focus the session's Messages window on the corresponding entry.
- Multiple timeline sessions can each keep their own Messages window state.

### Phase 5: Statistics Window

Objective:
Add a real session companion window for aggregated span/message timing summaries.

Tasks:

- Define canonical statistics aggregation keys.
- Build session-derived statistics summaries for full-session and visible-range scopes.
- Implement a Statistics scene window with selectable entries and range context.
- Wire selection to timeline navigation where appropriate.

Definition of done:

- The Statistics window can summarize spans by stable key and show aggregate timing data.
- The user can inspect either whole-session or range-scoped statistics.

### Phase 6: Dynamic Row Height And Span Lane Stability

Objective:
Fix the row-height model so nested spans remain visible and do not collapse when interaction changes visibility.

Tasks:

- Redesign row layout to use visible lane depth instead of a fixed 74 px bucket.
- Preserve stable row identity, row colors, and transition behavior while row heights change.
- Add virtualization/clipping for tall rows so the canvas remains responsive.

Definition of done:

- Deeply nested or overlapping spans remain visible instead of visually collapsing into one lane.
- Panning and zooming no longer make legitimate visible spans appear to disappear because of row-height clamping.

### Phase 7: Hardening, Spec Mapping, And Regression Coverage

Objective:
Leave the system resumable and measurable, not just locally improved.

Tasks:

- Add Tracey requirements and implementation references for the new session/messages/statistics behavior.
- Add focused tests for session ownership, view-cache invalidation, statistics keys, and dynamic row height.
- Re-profile the target interaction flows and compare captures against the current baseline.

Definition of done:

- Tracey mappings are current.
- The timeline session architecture is covered by executable tests and a repeatable profiling workflow.

## Recommended Implementation Order

1. Finish Phase 0 instrumentation and collect one more capture.
2. Implement Phase 2 before deeper query surgery if the new capture confirms interaction lag is dominated by duplicate paint/hit-test work.
3. If query cost still dominates after Phase 2, do Phase 3 next.
4. Land Phase 1 session ownership once the session/view-cache seam is clear enough to avoid churn.
5. Build Messages and Statistics on top of the shared session rather than as one-off windows.
6. Finish with dynamic row height and hardening once the shared query/session model is stable.

## Open Decisions

- Should span lifecycle entries appear in the Messages window as separate open/close records, or should the Messages window show a single synthesized span entry while the Statistics window owns the duration summary?
- Should statistics keys include source location by default, or only label plus canonical fields unless the data source explicitly marks source location as semantic identity?
- Should the session-owned cached view snapshot live in app-host first, or should it move directly into `teamy_studio_timeline_core` as a reusable query-cache abstraction?
- Should live ingestion continue writing directly into `TimelineDataset`, or is the right next step a smaller append-only observation store that feeds the dataset/query layer asynchronously?
- When opening a timeline, should Messages and Statistics windows open automatically, or should the session only create them on demand while still preserving per-session state?

## First Concrete Slice

1. Re-run the timeline playground capture while hovering, panning, zooming, and toggling live events.
2. Measure how much of the old interaction lag disappeared once hit testing started reusing the current render plan.
3. If visible-range query cost still dominates after the cache reuse slice, move immediately to visible-range query redesign before building Messages and Statistics windows.
4. If the cache reuse slice exposes a clean session-owned snapshot seam, use that seam as the entry point for shared timeline-session state in Phase 1.
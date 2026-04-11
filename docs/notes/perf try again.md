We are working on Teamy-Studio on the main branch.
G:\Programming\Repos\Teamy-Studio

There is the smooth-but-slow branch checked out as a worktree.
G:\Programming\Repos\teamy-studio-smooth-but-slow

This document turns the current investigation into an execution plan.

The core conclusion from the investigation is:

- the current bottleneck is not yet proven to require rewriting libghostty
- the current smooth-vs-fast tradeoff is largely controlled by Teamy-Studio's worker scheduling, display publication cadence, display extraction cost, and renderer reuse policy
- ConPTY/OpenConsole are not the first place to attack this problem
- full cutover is acceptable, so this plan does not preserve intermediate compatibility shims unless they are useful for validation while landing the work

## Goal

Ship a terminal pipeline that keeps the "smooth" feel of frequent visible progress while improving or at least preserving the current main branch's better visual completion behavior.

In practice that means:

- low shell-side backpressure during burst output
- low graphical completion time
- fewer wasted intermediate frames
- explicit observability so the next bottleneck is measurable rather than guessed

## Non-goals

- do not rewrite ConPTY or OpenConsole
- do not rewrite libghostty as the first move
- do not preserve the current branch split as a product surface
- do not keep backwards compatibility for obsolete internal pipeline APIs if a cleaner cutover is simpler

## Summary Of What We Learned

- both Teamy-Studio worktrees use the same `libghostty-vt` revision and the same `portable-pty` version
- `portable-pty` owns PTY creation and loads sideloaded `conpty.dll` when present
- `conpty.dll` and `OpenConsole.exe` define the Windows host-side console behavior, but they are not Teamy-Studio's VT parser or renderer
- `libghostty-vt` is a thin Rust wrapper over the upstream Ghostty terminal engine
- Teamy-Studio's `vt_write_terminal_output_slice` span surrounds the synchronous parser/state-mutation call into libghostty, but the surrounding queueing, chunking, display extraction, snapshot publication, and rendering behavior are still Teamy-owned
- the smooth-but-slow branch feels smoother because it publishes far more intermediate visible states
- the main branch finishes sooner because it coalesces work more aggressively and reuses more render output
- libghostty already exposes global dirty state and per-row dirty state through the render-state API, so we are not blocked on a missing dirtiness concept

## Product Decision

We will keep libghostty for now and do a full cutover of Teamy-Studio's terminal pipeline around it.

The cutover target is:

1. one terminal worker owns VT application and cached terminal display state
2. queue latency is observable end to end
3. display extraction is incremental instead of whole-snapshot-first wherever possible
4. renderer updates are driven by row or region invalidation rather than repeated whole-display rebuild assumptions
5. throughput self-tests measure both shell progress and visual progress under multiple workload shapes

Only after that cutover lands and is measured do we revisit whether a libghostty fork or replacement is justified.

## Checkpoint Status

This note started as a forward-looking plan. It is now also the checkpoint record for the work already landed on `main` during this performance pass.

### Landed So Far

The following changes have already been implemented:

- expanded the throughput self-test from one scenario into a multi-scenario harness
- added machine-readable benchmark result persistence using `facet-json`
- added queue-latency, pending-output, VT-write, and display-publication instrumentation
- timestamped PTY reads so queue latency can be measured from reader completion to VT application
- replaced the UI-thread terminal update backlog drain with a coalesced latest-state bridge
- switched display extraction to use libghostty dirty state and row reuse instead of always rebuilding every visible row
- aligned renderer, hit-testing, and terminal sizing around a shared terminal content rect
- made live move and drag interactions responsive again by avoiding synchronous terminal work on the hot UI interaction path
- changed live resize so the expensive terminal grid resize happens on the consummated resize instead of being replayed repeatedly during the move/size loop
- forced an immediate display publish after a real terminal resize so the next frame does not wait for the background cadence

### Current Perf Shape

The latest captures show a materially better interaction profile than the earlier baseline:

- grabbing and repositioning the window while output is flowing feels good again
- live resize is smoother than before because the terminal is no longer being reflowed continuously during the move/size loop
- `handle_poll_timer` is no longer a dominant UI-thread cost in the latest checkpoint captures
- the dominant remaining wall-clock bucket is still `vt_write_terminal_output_slice`
- consummated terminal resize cost still exists, but it is now much smaller and less disruptive than the earlier live-resize path

The current high-level conclusion remains the same: the next major throughput ceiling is still Teamy-owned scheduling/publication behavior wrapped around libghostty, with parser cost remaining the largest hot bucket after the interaction regressions were reduced.

### Current Known Bug

One blocker remains unresolved at this checkpoint:

- when a consummated resize changes the terminal grid, the shell prompt can disappear even though the resize interaction itself is now smoother

Current evidence suggests this is specifically tied to the real terminal resize and reflow path rather than the live preview path. The most recent attempted fix preserved bottom anchoring when the viewport was already at bottom and refreshed semantic prompt tracking immediately after resize, but that was not sufficient.

The current working theory is that one of these is still happening during the consummated resize:

- libghostty reflow is preserving a viewport/cursor relation that no longer keeps the prompt line visible
- post-resize display extraction is not rebuilding exactly the bottom rows expected after reflow
- semantic prompt markers remain present, but the visible bottom row after resize no longer matches the tracked prompt row

This is the issue to resume from next.

### Best Current Resume Point

When resuming, start by capturing and comparing the pre-resize and post-resize values for:

- `viewport_metrics()`
- cursor viewport row
- visible bottom row content
- semantic prompt tracking state

The key question is whether the prompt is being scrolled out of view by reflow, or whether Teamy's post-resize display extraction is dropping the bottom row after the resize has already completed.

## Full-Cutover Plan

### Phase 0. Lock In A Better Perf Harness

Goal:
Make the next changes falsifiable with repeatable measurements.

Implementation:

1. Expand the existing terminal throughput self-test into multiple named scenarios instead of one `Measure-Command { 1..N | Out-Host }` flow.
2. Add scenario coverage for:
	- burst numeric output to completion
	- continuous chunked output with sleeps to simulate interactive streaming
	- wide-line output to stress row extraction and renderer churn
	- scroll-heavy output that moves the viewport repeatedly
	- prompt-heavy output that exercises semantic prompt tracking with smaller bursts
3. Standardize metrics emitted by every scenario:
	- `measure_command_ms`
	- `graphical_completion_ms`
	- `delta_ms`
	- `frames_rendered`
	- `max_pending_output_bytes`
	- `avg_pending_output_bytes`
	- `max_queue_latency_ms`
	- `vt_write_calls`
	- `vt_write_bytes`
	- `display_publications`
	- `dirty_rows_published`
4. Support repeated sampling on all scenarios from one command.
5. Persist structured self-test output to a machine-readable format in addition to console text.

Files likely affected:

- `src/app/windows_app.rs`
- `src/app/windows_terminal.rs`
- any self-test support modules introduced for organization
- `README.md` or a dedicated note if usage needs documenting

Acceptance criteria:

- one command runs all scenarios
- repeated samples are supported consistently
- output is stable enough to compare medians across revisions

### Phase 1. Add Queue-Latency And Publication Instrumentation

Goal:
Stop treating `vt_write_terminal_output_slice` as one opaque wall-clock bucket.

Implementation:

1. Track timestamps or counters for:
	- PTY reader thread read completion
	- enqueue into pending output
	- start of VT application for each slice
	- completion of VT application for each slice
	- display publication
	- renderer consumption of a published display
2. Add coarse always-on spans for:
	- `service_terminal_background_output`
	- `publish_terminal_display_state`
	- `render_terminal_display_update`
3. Add `tracy`-gated spans for hot detailed points only where needed:
	- `queue_latency_before_vt_write`
	- `extract_dirty_terminal_rows`
	- `publish_terminal_damage`
4. Record queue depth statistics during burst tests.

Files likely affected:

- `src/app/windows_terminal.rs`
- `src/app/windows_d3d12_renderer.rs`

Acceptance criteria:

- Tracy and self-test output can distinguish parser time from time spent waiting to be parsed
- we can see whether main's shell-side slowdown comes from queueing or parsing

### Phase 2. Replace Whole-Snapshot-First Display Extraction

Goal:
Stop rebuilding the entire visible terminal display just to discover what changed.

Implementation:

1. Move the worker's display cache model to a persistent row-oriented representation.
2. During render-state traversal, use libghostty row dirty state to skip unchanged rows.
3. Rebuild only rows reported dirty, plus rows exposed by scroll or resize effects.
4. Publish row-level damage metadata directly from the worker rather than deriving dirty rows only after a fully rebuilt display exists.
5. Preserve a clear fallback path for full invalidation on resize, mode changes, and other global state transitions.

Design rules:

- full invalidation is correct and acceptable when state is globally dirty
- partial invalidation is the default for burst output
- row reuse decisions should happen before expensive row extraction work, not after it

Files likely affected:

- `src/app/windows_terminal.rs`
- helper types around `TerminalDisplayState`

Acceptance criteria:

- unchanged visible rows are not re-extracted during steady burst output
- worker publications include explicit damage metadata
- the old whole-display diff approach is removed after cutover

### Phase 3. Publish Lightweight Terminal Damage Instead Of Frequent Whole Displays

Goal:
Recover smooth visible progress without paying the full cost of smooth-but-slow's publish cadence.

Implementation:

1. Split worker publication into:
	- lightweight progress publication for incremental damage
	- less frequent full snapshot publication for synchronization and fallback
2. Publish incremental row updates at an interactive cadence when the dirty set is small.
3. Coalesce publications under large backlog so the worker does not flood the renderer with redundant intermediate states.
4. Base publication policy on:
	- pending output backlog
	- dirty-row count
	- elapsed time since last visible update
	- whether the renderer is already behind
5. Remove the current smooth-but-slow style of effectively showing every tiny slice as a whole display update.

Files likely affected:

- `src/app/windows_terminal.rs`
- `src/app/windows_app.rs`

Acceptance criteria:

- UI progress remains visibly active under streaming workloads
- burst workloads publish fewer redundant whole-display updates
- graphical completion does not regress back toward smooth-but-slow behavior

### Phase 4. Cut Over The Renderer To Damage-Driven Updates

Goal:
Make renderer work proportional to the actual terminal damage.

Implementation:

1. Replace assumptions that the renderer always receives a newly rebuilt whole display.
2. Treat terminal updates as a stream of:
	- row content replacements
	- scroll shifts
	- cursor changes
	- scrollbar changes
3. Rebuild row scenes only for damaged rows.
4. Keep row-scene caches authoritative across updates.
5. Patch GPU-side vertex ranges only for changed rows when geometry size permits, and fall back cleanly when it does not.
6. Remove obsolete whole-display scene rebuild paths after the damage-driven path is stable.

Files likely affected:

- `src/app/windows_d3d12_renderer.rs`
- any renderer message types shared with terminal worker output

Acceptance criteria:

- terminal scene work scales with damaged rows instead of visible rows
- duplicate-frame elision still works
- cursor and scrollbar updates remain correct under damage-driven updates

### Phase 5. Rebalance Parse Slice Policy For Throughput And Smoothness

Goal:
Tune the worker after the architectural cuts above, not before.

Implementation:

1. Keep backlog-aware slice sizing, but revisit thresholds using the new queue-latency data.
2. Test whether parse slices should be chosen from:
	- backlog size alone
	- backlog size and recent damage size
	- backlog size and renderer lag
3. Tune the worker time budget so continuous streaming does not starve visible updates.
4. Remove any knobs that no longer make sense after damage-driven publishing lands.

Acceptance criteria:

- lower shell-side backpressure than current main
- fewer total frames than smooth-but-slow
- visibly smoother progress than current main on interactive and medium-burst scenarios

### Phase 6. Simplify The Pipeline After Cutover

Goal:
Delete the intermediate compatibility logic and dead paths.

Implementation:

1. Remove obsolete whole-snapshot diff logic.
2. Remove obsolete publish heuristics that only existed for the old pipeline.
3. Collapse internal types that only made sense before row-level damage publication existed.
4. Update docs so they describe the new pipeline, not the migration history.

Files likely affected:

- `src/app/windows_terminal.rs`
- `src/app/windows_d3d12_renderer.rs`
- `docs/notes/windows-terminal-speedy-renderering.md`
- `docs/notes/terminal-stack-roles.md`
- any self-test documentation

Acceptance criteria:

- there is one clear code path for terminal mutation, publication, and rendering
- no backwards-compat shims remain for abandoned internal models

## Self-Test Expansion Plan

The self-test suite should be upgraded as part of the cutover, not after it.

### Scenarios To Add

1. `terminal-throughput burst-out-host`
	- current benchmark shape
2. `terminal-throughput stream-small-batches`
	- many small writes with short delays
3. `terminal-throughput wide-lines`
	- long visible rows stressing extraction and glyph work
4. `terminal-throughput scroll-flood`
	- output volume that continuously moves the viewport
5. `terminal-throughput prompt-bursts`
	- repeated prompt and command output cycles to catch prompt-tracking regressions
6. `terminal-throughput resize-during-output`
	- scripted resize during burst output to validate fallback full invalidation

### Metrics To Report

- shell-side completion
- visual completion
- frame count
- display publications
- dirty rows per publication
- queue latency percentiles if practical
- peak pending output bytes
- peak renderer backlog if tracked

### Validation Usage

Development should use:

- one scenario for tight inner-loop iteration
- all scenarios with repeated samples before merge

## Tracy Plan

Add or refine spans only where they answer a concrete question.

Always-on coarse spans:

- `service_terminal_background_output`
- `drain_pty_reader_messages`
- `publish_terminal_display_state`
- `render_terminal_update`

`tracy`-gated hot spans:

- `queue_latency_before_vt_write`
- `extract_dirty_terminal_rows`
- `publish_terminal_damage`
- `patch_terminal_vertex_ranges`

Questions these spans must answer:

1. are PTY bytes waiting too long before parse begins?
2. are we still rebuilding unchanged rows?
3. are we over-publishing intermediate states?
4. is the renderer falling behind after worker publication?

## Suggested Implementation Order

This is the order to execute without yielding once the plan is approved:

1. expand self-test harness
2. add queue-latency and publication instrumentation
3. refactor worker display cache into row-oriented persistent state
4. publish explicit row damage
5. cut renderer over to damage-driven consumption
6. retune slice and publication policy using new measurements
7. remove old whole-snapshot paths and update docs

## Success Criteria

The cutover is successful when all of the following are true:

- Teamy-Studio no longer needs the smooth-but-slow branch as a behavioral reference branch
- burst workloads show visibly continuous progress without returning to `200+` frames for the current throughput benchmark shape
- shell-side completion is materially better than current main on burst scenarios
- graphical completion is no worse than current main on burst scenarios
- streaming scenarios feel smoother than current main
- traces can clearly distinguish queueing, parsing, display extraction, publication, and rendering time

## Decision Gate After Cutover

After the full Teamy-side cutover lands, re-evaluate the need for libghostty changes only if one of these is true:

1. queue latency is low, display extraction is incremental, renderer damage handling is incremental, and `vt_write` is still the dominant bottleneck
2. libghostty's row-dirty and render-state APIs are insufficient for the final Teamy renderer shape
3. a focused libghostty microbenchmark on captured PTY streams proves the parser itself is now the clear ceiling

If none of those are true, keep the current dependency and continue tuning Teamy's own pipeline.
# Windows Terminal Speedy Renderering

This note captures what we learned by studying the Microsoft Terminal source after profiling Teamy-Studio's terminal output path under bursty workloads such as `1..10000`.

The short version is that Microsoft Terminal is not winning because it found a magic chunk size. It is winning because its architecture keeps PTY reads, VT parsing, dirty-region tracking, and frame presentation more decoupled than Teamy-Studio's current frame-driven path.

## What Microsoft Terminal does differently

### 1. It pipelines PTY reads

In Microsoft Terminal's `ConptyConnection::_OutputThread`, the output thread starts the next `ReadFile` before it finishes dispatching the previous chunk to the rest of the stack.

That matters because output ingestion is not blocked behind downstream work. The pipe keeps moving even if terminal mutation or UI notification takes time.

### 2. It mutates terminal state on output arrival

Microsoft Terminal forwards terminal output into the terminal core as soon as the connection raises it. The VT parser and backing buffer advance on the output side, not only during a later paint pass.

That means the terminal model stays current independently of presentation cadence.

### 3. It renders on a dedicated render thread

Its renderer has an explicit paint thread that waits on redraw and timer signals. Presentation is not multiplexed into the same control flow that drains PTY output.

That prevents output ingestion and frame presentation from serializing each other.

### 4. It tracks dirty regions incrementally

Microsoft Terminal invalidates specific regions and rows instead of conceptually rebuilding the whole visible terminal surface each time output arrives.

That lowers the amount of per-frame work required to keep the UI looking live.

### 5. It partially presents

Its Atlas renderer uses dirty rectangles with `Present1` where possible. That reduces GPU and compositor work for incremental terminal updates.

## What this means for Teamy-Studio

Today Teamy-Studio still couples too much of the terminal pipeline to `render_current_frame`:

- PTY bytes are read on a background thread, but only drained on the UI side during terminal pumping.
- VT writes are budgeted per frame.
- Terminal display extraction and scene population still happen in the same render path.
- The renderer presents a full scene assembled by Teamy-Studio each frame rather than incrementally invalidating only changed terminal regions.

The current one-slice-per-frame change is a useful safety valve for frame time, but it does not solve the deeper architectural gap. It trades long blocking frames for visible quantization.

## Performance plan

The goal is not a single risky rewrite. The goal is a staged sequence of changes where each step is understandable, implementable, and verifiable.

### Phase 1. Make the pipeline stages explicit

Goal:
Split the current terminal path into named stages so we can move them independently.

Concrete steps:

1. Separate PTY read draining from per-frame VT application.
2. Keep explicit state for:
   - unread PTY data queued in Teamy-Studio
   - unpresented terminal mutations
   - render work still pending
3. Ensure poll-timer logic no longer blindly means "parse during render".

Verification:

- `check-all.ps1`
- Tracy capture confirms distinct spans for read draining, VT application, display extraction, and render
- terminal still closes correctly on Ctrl+D and shell exit

### Phase 2. Let ingestion run ahead of presentation

Goal:
Prevent PTY ingestion from stalling just because a frame is expensive.

Concrete steps:

1. Drain PTY output as soon as the app observes it, even if only a bounded amount of VT application can happen before the next frame.
2. Preserve backpressure limits so Teamy-Studio cannot accumulate unbounded memory.
3. Tune read-side buffering using real traces rather than guesswork.

Verification:

- targeted regression tests for queueing and close behavior where practical
- Tracy shows pipe draining separated from `vt_write`
- long output bursts no longer spend most of their time waiting for the next frame just to become eligible for processing

### Phase 3. Move terminal mutation off the frame path

Goal:
Advance the terminal model independently of frame building.

Concrete steps:

1. Introduce a dedicated terminal worker or equivalent ownership model for VT parsing and terminal-state mutation.
2. Publish snapshot-ready data or dirty-state notifications back to the UI thread.
3. Keep cross-thread boundaries explicit and observable with tracing.

Verification:

- thread-safety review for the terminal state ownership model
- Tracy shows terminal mutation and rendering as distinct timelines
- `1..10000` streams continuously without one-slice-per-frame artifacts dominating the experience

### Phase 4. Reduce display extraction cost

Goal:
Stop rebuilding more terminal presentation data than necessary.

Concrete steps:

1. Profile `visible_display_state_with_selection` after Phase 3.
2. Add dirty-row or dirty-region tracking for terminal display extraction.
3. Avoid re-collecting stable terminal rows when only a small region changed.

Verification:

- new regression tests around dirty-region bookkeeping where it can be tested deterministically
- Tracy shows a reduction in `collect_visible_terminal_cells` and related spans under burst output

### Phase 5. Reduce render-scene cost

Goal:
Make Teamy-Studio's renderer behave more like an incremental terminal presenter than a full-scene rebuild loop.

Concrete steps:

1. Identify which parts of scene construction are truly terminal-dependent each frame.
2. Cache or incrementally update terminal geometry/glyph data where possible.
3. Move toward partial invalidation or partial present semantics in the renderer.

Verification:

- Tracy confirms lower frame cost in `populate_render_scene` and renderer spans
- resize and scrolling remain correct

### Phase 6. Re-tune budgets only after architecture improves

Goal:
Avoid using chunk-size tuning as a substitute for architectural fixes.

Concrete steps:

1. Revisit slice sizes and poll intervals only after Phases 1 through 5 have moved the heavy work off the frame path.
2. Tune using repeatable burst-output traces.
3. Keep any hot-path tracing behind `tracy` where appropriate.

Verification:

- side-by-side traces before and after tuning
- no regressions in ordinary interactive typing latency

## Implementation order

The recommended order is:

1. Phase 1: make stages explicit
2. Phase 2: ingestion runs ahead of presentation
3. Phase 3: move terminal mutation off the frame path
4. Phase 4: incremental display extraction
5. Phase 5: incremental render updates
6. Phase 6: final tuning

This order keeps each step small enough to reason about while steadily moving Teamy-Studio toward and eventually past the behavior we observed in Microsoft Terminal.

## Started in this branch

This branch begins Phase 1 by separating PTY read draining from per-frame output application so the app can observe and queue terminal output without requiring that the same code path also build and present a frame.

It also begins Phase 2 by improving read-side buffering and backpressure:

- the PTY reader now reads larger chunks, closer to the approach used by Microsoft Terminal
- the handoff from the reader thread to the UI side is bounded so backlog cannot grow without limit if the UI falls behind

It also begins the Phase 3 transition by moving terminal mutation out of `render_current_frame` and into the idle service path. The work still happens on the UI thread today, but frame construction and `vt_write` no longer share the same direct boundary.

It now also begins Phase 4 by publishing cached no-selection terminal display snapshots from the terminal worker.

- ordinary frames no longer synchronously ask the worker to rebuild terminal display state
- the UI renders from the latest asynchronously published terminal display snapshot instead
- selection rendering still falls back to a synchronous worker query today, so the remaining display-extraction work is now concentrated in explicit selection and cache-refresh paths rather than every normal frame

It also begins Phase 5 by caching the terminal scene fragment in the UI layer.

- when the terminal display snapshot, scrollbar visual state, and layout are unchanged, Teamy-Studio now reuses the previously built terminal scene fragment instead of re-emitting every terminal panel, glyph, and cursor overlay
- this does not remove scene copies yet, but it does remove repeated terminal geometry construction work from steady-state frames

Phase 5 now also lets the renderer consume scene fragments directly instead of requiring the app to flatten chrome, terminal, and output data into one temporary aggregate `RenderScene` first.

- steady-state frames no longer pay for app-side vector extension just to hand the same data back to the renderer
- the remaining render cost is now more concentrated in scene-fragment construction that still changes and in renderer-side vertex upload
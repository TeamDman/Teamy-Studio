---
name: add-tracy-spans
description: 'Add tracing spans for Tracy profiling in Teamy-Studio. Use when instrumenting startup, windowing, rendering, terminal, async work, or other performance-sensitive code paths and deciding which spans should be always-on versus gated behind the tracy feature.'
argument-hint: 'Describe the code path, suspected hot spot, and user flow you want to profile'
---

# Add Tracy Spans

Use this skill when adding or refining tracing spans in Teamy-Studio so Tracy captures are useful for performance analysis without paying unnecessary overhead in normal runs.

This is not a rule to guard every span behind `feature = "tracy"`.
The job is to place spans where they explain time spent, then gate only the spans whose creation cost or volume would distort hot paths.

## Outcome

Produce instrumentation that:

- shows the important phases of the user flow in Tracy
- exposes the slow inner steps once a coarse span identifies the hot region
- avoids high-volume span creation in non-Tracy builds
- keeps ordinary logging and diagnostics useful even when Tracy is disabled

## Teamy-Specific Context

- Teamy-Studio already has a `tracy` cargo feature in `Cargo.toml`.
- Teamy-Studio already wires Tracy through the tracing subscriber.
- Teamy-Studio already has `run-tracing.ps1` for capturing and opening a Tracy profile.
- teamy-mft is the reference repository for existing span style and gating decisions.

## Procedure

### 1. Pick the exact user flow to profile

Decide what the capture should explain before adding spans.

Examples:

- app startup to first visible window
- terminal window creation
- first frame render after opening a window
- resize handling
- text layout or glyph rasterization
- PTY read, parse, and present pipeline

Prefer one concrete flow over "instrument this whole file".

### 2. Start with coarse spans at phase boundaries

Add a small number of spans that divide the flow into meaningful phases.

Good candidates:

- command entrypoints
- major startup phases
- one-time resource loading
- opening files, pipes, devices, or windows
- render pipeline stages
- async task boundaries
- cross-thread handoff points

Use stable snake_case names that describe work, not implementation trivia.

Good names:

- `create_terminal_window`
- `load_font_collection`
- `process_pty_output`
- `build_frame_commands`
- `present_swap_chain`

Avoid dynamic span names.

### 3. Choose always-on versus `tracy`-gated spans

Use this decision rule for every new span.

Leave the span always-on when the span is:

- at a top-level or mid-level phase boundary
- created once per user action or once per startup path
- around a clearly expensive I/O or OS call
- useful for normal structured logging even without Tracy
- low-frequency enough that overhead is negligible

Guard the span behind `feature = "tracy"` when the span is:

- inside a tight loop
- created once per item, cell, glyph, line, packet, event, or record
- inside frame-by-frame rendering work
- inside a per-message or per-chunk processing loop
- inside rayon or other parallel closures with many iterations
- in a tiny function that is called extremely often
- detailed enough that it only matters after a coarse span identifies a hot spot

Do not gate a span only because the code is performance-sensitive.
First ask whether the span is coarse and rare, or fine-grained and high-volume.

### 4. Match the instrumentation style to the cost profile

Use `#[instrument(...)]` for function entry spans when the function itself is a useful boundary and call frequency is moderate.

Prefer:

```rust
#[instrument(level = "info", skip_all)]
fn invoke_and_render(...) -> eyre::Result<()> {
    ...
}
```

When the function is hot and the entry span should only exist for Tracy builds, use `cfg_attr`:

```rust
#[cfg_attr(feature = "tracy", instrument(level = "debug", skip_all))]
fn hot_inner_step(...) {
    ...
}
```

Use block spans like `info_span!` or `debug_span!` when only part of a function needs timing.

Prefer always-on block spans for coarse steps:

```rust
let _span = tracing::info_span!("load_terminal_config").entered();
```

Prefer `tracy`-gated block spans for tight inner work:

```rust
#[cfg(feature = "tracy")]
let _span = tracing::debug_span!("rasterize_visible_glyph_run").entered();
```

Use `info_span!` for major phases you want visible in normal captures.
Use `debug_span!` for more detailed inner work.

### 5. Keep fields useful and cheap

Add fields that help explain scaling behavior without exploding cardinality.

Good fields:

- counts
- sizes in bytes
- visible row or column counts
- drive, path, or window identifiers when cardinality is naturally bounded
- booleans or mode flags

Avoid fields that are large, noisy, or effectively unique on every event unless the boundary is very coarse.

Usually avoid:

- full buffers
- full rendered text payloads
- large structs with `?value`
- per-item unique IDs inside hot loops

If a function takes large or noisy arguments, use `skip_all` or explicit `skip(...)`.

### 6. Instrument hierarchically

Work top down.

1. Add the coarse parent span.
2. Capture a Tracy profile.
3. Identify the hottest child region.
4. Add one more level of detail there.
5. Repeat until the performance question is answerable.

Do not start by adding dozens of fine-grained spans everywhere.
That creates noise and makes it harder to see the bottleneck.

### 7. Be careful in render and terminal hot paths

In Teamy-Studio, assume these areas may be hot until proven otherwise:

- frame construction
- D3D12 command recording and presentation
- text shaping, layout, and rasterization
- PTY output ingestion and parsing
- terminal buffer updates
- input event processing that runs per key or per mouse event

Default approach in those areas:

- add one coarse always-on parent span around the full phase
- add inner diagnostic spans only behind `feature = "tracy"`

Exception:

- if the coarse parent itself runs every frame, every poll tick, or at similarly high frequency, it is acceptable to gate that parent span behind `feature = "tracy"`
- in that case, keep the surrounding one-time or per-user-action spans always-on so normal builds still expose the major lifecycle phases

### 8. Preserve normal logs separately from performance spans

Use logs for state changes, decisions, and failures.
Use spans for timing boundaries.

Do not replace a useful `info!`, `debug!`, `warn!`, or `error!` with a span.
Add the span around the work and keep the log if it explains behavior.

### 9. Follow existing repo patterns

Mirror the style already established in teamy-mft:

- `#[cfg_attr(feature = "tracy", instrument(...))]` for hot function entry spans
- `#[cfg(feature = "tracy")] let _span = debug_span!(...).entered();` in tight loops
- always-on `info_span!` around important higher-level phases
- `skip_all` on functions whose arguments are large or noisy

For Teamy-Studio specifically, Clippy may enforce both `semicolon_outside_block` and `semicolon_if_nothing_returned`.
When a scoped span block triggers both lints, prefer this shape:

```rust
let () = {
    #[cfg(feature = "tracy")]
    let _span = tracing::debug_span!("populate_render_scene").entered();
    do_work();
};
```

The point of teamy-mft is not to copy every span literally.
Use it as a guide for deciding instrumentation granularity.

Representative examples:

- teamy-mft gates hot inner spans in `src/mft/fast_entry.rs`
- teamy-mft keeps higher-level query spans always-on in `src/cli/command/query/query_cli.rs`

### 10. Validate with Tracy, not guesswork

After adding spans:

1. Run the normal validation path for the repo.
2. Capture a Tracy trace for the target flow.
3. Confirm the new spans appear in the expected hierarchy.
4. Confirm the capture answers a specific performance question.
5. If the capture is too noisy, remove or gate the noisy inner spans.
6. If the capture is too shallow, add the next level only in the hot child region.

For Teamy-Studio, use the existing wrapper:

```powershell
.\run-tracing.ps1 window show
```

Or pass the specific subcommand flow you are investigating.

## Span Placement Heuristics

Use this quick table when deciding what to add.

| Situation | Span style |
|---|---|
| App or command entrypoint | Always-on `#[instrument]` or `info_span!` |
| One-time startup phase | Always-on `info_span!` |
| File open, device open, OS call, resource load | Usually always-on `info_span!` |
| Tight per-item loop | `#[cfg(feature = "tracy")]` block span |
| Render inner loop | `#[cfg(feature = "tracy")]` block span |
| Parallel worker closure | Usually `#[cfg(feature = "tracy")]` block span |
| Moderate-cost helper called occasionally | Always-on `#[instrument(skip_all)]` can be fine |
| Tiny helper called extremely often | Avoid or gate the span |

## Review Checklist

Before finishing, verify all of these:

- The span names describe user-visible work or a meaningful internal phase.
- The trace has a clear parent-child structure.
- Hot-loop spans are gated behind `feature = "tracy"`.
- Coarse phase spans remain available without Tracy.
- Span fields are bounded and useful.
- Large arguments are skipped.
- The resulting capture is easier to reason about than the uninstrumented trace.

## Anti-Patterns

Avoid these mistakes:

- adding spans to every function in a file
- guarding every span behind `feature = "tracy"`
- leaving high-volume inner spans always-on in render or parsing loops
- putting huge payloads in span fields
- using spans where a normal log message is the better tool
- adding detail before first adding a coarse parent span
- keeping noisy spans after they stop answering a performance question

## Suggested Workflow For A New Instrumentation Task

When asked to instrument a Teamy-Studio code path:

1. Identify the target user flow and the performance question.
2. Find the entrypoint and add one or two coarse spans.
3. Run `./check-all.ps1`.
4. Capture with `./run-tracing.ps1 ...`.
5. Inspect the hottest child region in Tracy.
6. Add one more layer of detail only there.
7. Gate fine-grained spans if they are on a hot path.
8. Re-capture and verify the trace is now actionable.

## Completion Criteria

The work is complete when:

- Tracy clearly shows where the time is going for the chosen flow
- the added spans distinguish coarse phases from the hottest inner steps
- non-Tracy builds do not pay for high-volume diagnostic spans
- the code remains readable and the instrumentation intent is obvious
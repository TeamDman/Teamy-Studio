# Terminal Engine Replacement Plan

This note replaces the current optimization-first direction with an ownership-first direction.

The new goal is to remove the `libghostty-vt` dependency and replace it with a Teamy-Studio-owned Rust terminal engine that lives in this repository and is instrumentable end to end.

The reason for the change is straightforward:

- the current parser and screen-state core are outside the repo and outside our effective tracing surface
- we are spending time tuning around a black box instead of narrowing the problem inside code we own
- we want terminal behavior, instrumentation, tests, and performance work to all live in one Rust codebase

This is a large change, so the correct approach is not "delete Ghostty and start freehanding ANSI support." The correct approach is to introduce a narrow engine boundary, build a comprehensive test corpus, and replace the current implementation incrementally behind that boundary.

## Decision

We are proceeding with an in-repo Rust terminal engine.

We are not treating OpenConsole or Microsoft Terminal as a runtime dependency for this layer. They remain useful as references for behavior, data flow, and performance ideas, but Teamy-Studio should own the terminal core.

We are also not doing a big-bang cutover. `libghostty-vt` should remain available as the reference engine until the new engine can pass the targeted scenarios and beat or at least match the current behavior on the acceptance gates.

## Primary goals

- make terminal parsing and screen mutation fully instrumentable in Rust
- move terminal semantics under Teamy-Studio control in this repo
- build a test suite that tells us exactly what works, what regressed, and what is still unsupported
- make red-green testing the default engineering loop for terminal work
- make the core engine and renderer testable headlessly without opening visible windows
- preserve the existing Teamy-Studio worker and renderer architecture where it is already useful
- create a path toward the `1..10000` benchmark finishing in under `1000 ms`
- design the hot path around bounded reuse and arena-style allocation instead of opportunistic per-update allocation

## Non-goals for the first slices

- do not aim for full xterm feature parity before replacing the current workload subset
- do not rewrite PTY management first; `portable-pty` is not the current ownership problem
- do not tie the parser rewrite to renderer rewrites unless a specific boundary demands it
- do not delete the existing throughput harness; it becomes part of the acceptance suite

## Current boundary in Teamy-Studio

Today the Teamy-owned integration point is concentrated in `TerminalSession` and `TerminalCore` in [src/app/windows_terminal.rs](src/app/windows_terminal.rs).

That boundary already owns most of the application-specific responsibilities:

- PTY lifecycle
- pending-output queueing
- queue-latency and throughput counters
- display publication policy
- semantic prompt tracking glue
- viewport and resize orchestration
- conversion from terminal render state into Teamy display rows

The main external engine responsibilities still coming from `libghostty-vt` are:

- VT parsing
- screen/grid state
- cursor and style state
- row dirtiness and render snapshot iteration
- keyboard encoding helpers currently used through `libghostty_vt::key`

That means the first engineering move should be to isolate those responsibilities behind an internal Teamy trait and adapter layer.

## Recommended implementation shape

Create a new in-repo crate for the engine. A good starting shape is:

- `crates/teamy-terminal/`
- `crates/teamy-terminal/src/lib.rs`
- `crates/teamy-terminal/src/parser.rs`
- `crates/teamy-terminal/src/screen.rs`
- `crates/teamy-terminal/src/render.rs`
- `crates/teamy-terminal/src/keyboard.rs`
- `crates/teamy-terminal/src/semantic_prompt.rs`

The important point is not the exact filenames. The important point is that Teamy-Studio should depend on a Teamy terminal crate, not directly on Ghostty.

The same ownership rule applies to keyboard translation. The preferred path is to move key translation and control-key policy into a Teamy-owned unit instead of continuing to accumulate ad hoc runtime special cases in the window layer. That unit should be directly testable against plain-text input, control-key behavior, Kitty-mode interactions, and shell-sensitive cases such as `Ctrl+D` and `Ctrl+L`.

## Engine boundary to introduce first

Before implementing new parser logic, define the interface Teamy-Studio actually needs.

The replacement boundary should be shaped by current usage in [src/app/windows_terminal.rs](src/app/windows_terminal.rs), not by Ghostty's API surface.

The engine-facing surface should cover at least:

- create terminal with cols, rows, scrollback limit
- apply output bytes
- resize grid and pixel cell metrics
- collect row-oriented visible display state
- expose cursor visual state
- expose viewport metrics and scrolling
- expose semantic prompt markers used by Teamy-Studio
- encode terminal key events for PTY writes
- expose explicit damage information for incremental rendering

For runtime selection, the engine choice should hang off the visible window launch surface rather than a top-level global CLI flag. The migration target is `window show --vt-engine ghostty|teamy`, with the existing no-command developer flow continuing to reach that window-launch path instead of introducing a second engine-selection surface.

At the start, define this behind an adapter layer with two implementations:

- `GhosttyEngineAdapter`
- `TeamyEngine`

That dual-engine period is what makes incremental migration possible.

The first live Teamy-backed window should not be treated as a minimal feature-chopped preview whose only job is to boot. The acceptance bar for the first `--vt-engine teamy` launch should be that obvious shell behaviors look correct and unsurprising, even if deeper parity work remains. A later first live run is preferable to an earlier run that visibly regresses typing, redraw, scrolling, or prompt behavior.

## Engineering rules

These rules should govern implementation, not just testing.

### Red-green is mandatory

For terminal-engine work, the expected loop is:

1. add or capture a failing test first
2. verify it fails for the intended reason
3. implement the smallest change that makes it pass
4. keep the test as a permanent regression case

That rule applies to parser work, resize behavior, prompt tracking, keyboard encoding, damage tracking, and rendering.

We should prefer adding ten narrowly-scoped tests over one broad test when narrowing behavior. The quantity is a feature here because it keeps regressions local and discoverable.

### Headless first, visible app second

The terminal engine must be testable without:

- creating a visible window
- waiting on message pumps
- requiring interactive user observation

The visible app remains an integration target, but it should not be the first place we discover whether a parser or renderer change is correct.

### Performance contracts belong in tests

If we care about `144 Hz` behavior and sub-`1000 ms` throughput, those expectations need explicit harnesses and thresholds, not comments and intention.

That means:

- engine-only microbenchmarks
- headless render benchmarks
- end-to-end throughput runs
- allocation counters or at least allocation-regression detection where practical

### Spec-first requirements capture is mandatory

When we know a requirement before we have an implementation, it should go into the spec set first and then be tracked through Tracey.

That matters especially for this terminal rewrite because many of the desired behaviors are already known now even though the Teamy engine does not exist yet.

Examples:

- the terminal engine must be Teamy-owned Rust code instead of a Ghostty dependency
- terminal replay benchmarks must exist and be headless
- renderer verification must be possible without a visible window
- the `pwsh.exe -NoProfile` `1..10000` benchmark must have explicit timing and frame-budget metrics
- hot-path allocation growth must be observable and bounded

If those expectations live only in notes or chat, they are easy to forget. If they live in spec documents and Tracey, they become auditable.

## Spec-driven development with Tracey

This repo already has a working Tracey specification in [.config/tracey/config.styx](.config/tracey/config.styx), and the current status is a good baseline:

- `tracey query validate --deny warnings` is clean
- `tracey query status` shows full coverage for CLI and OS specs, but incomplete coverage and incomplete verification in parts of the behavior and tool specs

That means the next step is not to invent a new process. The next step is to use the existing Tracey flow more aggressively for the terminal rewrite.

### Where new requirements should go

Use the existing spec split instead of creating ad hoc requirement files:

- product-visible terminal behavior belongs in [docs/spec/product/behavior.md](docs/spec/product/behavior.md)
- CLI surfaces for replay, benchmark, or artifact commands belong in [docs/spec/product/cli.md](docs/spec/product/cli.md)
- Windows renderer and offscreen-rendering expectations that are OS-specific belong in [docs/spec/product/os.md](docs/spec/product/os.md)
- implementation workflow requirements, benchmark discipline, and testing rules belong in [docs/spec/tools/tool-standards.md](docs/spec/tools/tool-standards.md)

### What to specify before implementation exists

Before landing the Teamy terminal engine, add requirements for at least:

- headless engine transcript replay support
- headless render-to-image or readback support for terminal frames
- `pwsh.exe -NoProfile` benchmark coverage
- allocation and capacity-growth observability for the engine hot path
- differential comparison support while Ghostty remains the reference engine
- artifact writing for failed visual or replay assertions

These requirements can exist before their implementations are mapped. That is a feature, not a problem.

### Required workflow per slice

For each terminal-engine slice, the order should be:

1. write or update the relevant spec requirements
2. run `tracey query validate --deny warnings`
3. inspect `tracey query status`
4. add failing tests for the new requirement
5. implement the slice
6. add implementation annotations and verification annotations
7. run Tracey validation again

This keeps the project honest about whether a change is:

- merely implemented
- specified but not implemented
- implemented but unverified
- both specified and verified

### Tracey as a migration dashboard

During the Ghostty-to-Teamy transition, Tracey should become the dashboard for migration progress.

In practice that means we should be able to answer:

- which future terminal-engine requirements are already specified
- which of them still point only to Ghostty-backed code
- which are covered by replay tests
- which still lack verification references

The plan should therefore treat uncovered and untested Tracey entries as real backlog, not documentation trivia.

### Current spec implication to keep in mind

The current behavior spec still contains a Ghostty-specific product requirement in [docs/spec/product/behavior.md](docs/spec/product/behavior.md): the shell window is specified to render terminal content through `libghostty-vt`.

That is correct for the current product, but it will need to be replaced or split when the Teamy engine migration begins in earnest. The spec should evolve deliberately instead of silently drifting away from the intended architecture.

### Suggested requirement additions

The next spec pass should add requirements along these lines:

- behavior: the launched terminal window must render through the Teamy-owned terminal engine once cutover is complete
- cli: the self-test surface must expose headless replay and headless render benchmark entry points
- os: the D3D12 renderer must support offscreen terminal rendering for automated verification
- tool: terminal-engine changes must add regression coverage before implementation and must keep benchmark and Tracey status auditable

## Test matrix

The project should treat terminal quality as a matrix of independent test layers rather than one catch-all integration story.

### Layer 1. Pure parser and screen tests

These are the fastest tests and should become the majority of the suite.

They should run with:

- no PTY
- no renderer
- no threads required unless the test is explicitly about concurrency
- deterministic byte inputs and deterministic expected state

Primary assertions:

- screen contents
- cursor position
- style state
- scrollback state
- damage state
- semantic prompt markers

### Layer 2. Transcript replay tests

These replay captured real-world transcripts into the engine without launching the shell again.

They should cover:

- `pwsh.exe -NoProfile`
- prompt startup
- `1..10000`
- scroll floods
- prompt bursts
- resize during output
- keyboard-triggered output paths where relevant

This layer is where most behavioral coverage should live once the harness exists.

### Layer 3. Differential engine tests

While Ghostty remains available, compare Teamy and Ghostty over the same transcript fixtures.

This gives us:

- a migration oracle
- a way to reduce uncertainty while rewriting
- a way to identify intentional divergences explicitly instead of accidentally

### Layer 4. Headless renderer tests

The renderer already has useful building blocks:

- snapshot-style image output in [src/app/windows_d3d12_renderer.rs](src/app/windows_d3d12_renderer.rs)
- a window-independent `RenderFrameModel` in [src/app/windows_d3d12_renderer.rs](src/app/windows_d3d12_renderer.rs)

The next step should be to add an offscreen path that can render a `RenderFrameModel` into a texture or CPU-readable image without presenting to an HWND.

The target shape is:

- `render_frame_model_to_image(frame) -> RgbaImage`
- optional `render_frame_model_to_gpu_texture_and_readback(frame)` for exercising the D3D12 path specifically

That lets us add headless assertions for:

- cursor visibility
- selection rendering
- scrollbar position
- row damage correctness
- chrome and terminal composition

### Layer 5. Hidden-window integration tests

Some code paths may still need an HWND for now. When that is unavoidable, prefer:

- hidden windows
- minimized or non-visible windows
- deterministic render completion and readback

Those tests are acceptable as transitional coverage, but they should not be the default test layer.

### Layer 6. Full benchmark and acceptance tests

This is where the existing throughput harness continues to matter.

## Runtime cutover strategy

The runtime cutover should happen in explicit phases.

### Phase 1. Window-scoped engine selection

Add engine selection to `window show`, not to the top-level command surface.

The intended shape is:

- `cargo run -- window show --vt-engine ghostty`
- `cargo run -- window show --vt-engine teamy`

This keeps engine selection attached to the live window launch path instead of creating a parallel routing story.

### Phase 2. Backend abstraction in the live terminal session

`TerminalSession` and `TerminalCore` in [src/app/windows_terminal.rs](src/app/windows_terminal.rs) must stop constructing Ghostty directly and instead accept an engine choice that selects a live backend.

The first abstraction does not need to be elaborate, but it must make these responsibilities engine-selectable:

- output application
- display-state extraction
- viewport metrics
- resize handling
- keyboard translation and PTY writes

### Phase 3. Quality-first first Teamy run

The first `--vt-engine teamy` live window should only be considered ready once baseline shell usage has no obvious problems.

That acceptance bar should explicitly prioritize correctness and user trust for the first live trial:

- prompt redraws should look right
- normal typing should work
- expected control keys should behave sensibly
- visible output should not obviously corrupt, smear, or lose cursor placement

Deeper parity can continue after that point, but the first live Teamy run should not feel visibly broken just to claim earlier runtime integration.

These tests answer:

- does the app still work end to end
- does the PTY behave correctly
- does the renderer keep up under real flow
- are we meeting throughput targets in the full stack

## Test strategy

The test suite is the product here. The engine replacement should be driven by tests and trace corpora, not by faith.

### 1. Differential engine tests

For a bounded subset of behavior, feed the same byte stream into both engines and compare:

- visible rows
- cursor position and style
- viewport metrics
- semantic prompt state
- dirty rows or damage regions

These tests should use real PTY transcripts first, then reduced cases.

This is the safest way to use Ghostty while removing Ghostty.

### 2. Transcript corpus tests

Capture real input and output streams from:

- `pwsh.exe`
- `cmd.exe`
- the existing `terminal-throughput` scenarios
- prompt-heavy flows with OSC 133 markers
- scroll-heavy bursts
- resize-during-output scenarios
- keyboard repro scenarios already covered by the self-test harness

Store those as fixtures and make them replayable without launching a live PTY.

Suggested fixture shape:

- raw byte transcript
- resize events
- local input events
- expected final visible screen excerpt
- expected cursor and viewport metadata

### 3. Reduced regression tests

Use the reproduce-reduce-regress workflow for every discovered bug:

- capture the real failing transcript first
- reduce it to the smallest still-failing chunk sequence
- keep the reduced and original forms as separate tests

That applies especially to:

- resize reflow bugs
- prompt tracking bugs
- Ctrl+D and keyboard encoding bugs
- scrollback anchoring bugs
- alt-screen/TUI bugs once those matter

### 4. Screen semantics tests

Add focused unit tests for:

- printable text
- CR, LF, CRLF behavior
- wrapping at the right margin
- erase commands
- SGR color and attribute handling
- cursor positioning
- scroll regions if and when required
- OSC 133 prompt markers

These should not depend on PTY startup.

### 5. Performance microbenchmarks

Add engine-only benchmarks that measure:

- bytes parsed per second
- time to apply `1..10000` style transcript bytes to screen state
- allocation counts if practical
- cost of producing visible rows from current state

The important distinction is that these benchmarks must run without PTY setup, renderer work, or windowing. They answer whether the engine is actually fast enough in isolation.

### 5a. Headless render benchmarks

Add renderer benchmarks that measure the cost of taking a prepared display state and producing a rendered image without creating a visible app window.

These should measure at least:

- scene build time from `TerminalDisplayState`
- vertex generation time
- GPU render plus readback time for offscreen paths
- rows changed versus total rows visible

This separates parser cost from render cost and gives us a hard answer when one side regresses.

### 5b. End-to-end throughput benchmark contract

The primary benchmark contract should be explicit and stable:

- launch `pwsh.exe -NoProfile`
- run the `1..10000` benchmark scenario
- feed the entire output stream through the Teamy VT engine
- drive render updates at a `144 Hz` target cadence with no intentional sleep-based smoothing beyond what is required to simulate that cadence
- measure shell completion, engine completion, render completion, and final-frame completion separately

The important rule is that this benchmark should exist in two forms:

- engine-only replay benchmark from captured transcript bytes
- end-to-end PTY benchmark through the live shell

If the replay benchmark is fast and the live benchmark is slow, the bottleneck is outside the parser. If both are slow, the engine is still the problem.

### 6. Existing macro acceptance gates

Keep the current Teamy-Studio gates and treat them as the final integration checks:

- `cargo run -- self-test terminal-throughput`
- `cargo run -- self-test terminal-throughput --samples 3`
- keyboard self-test scenarios already wired through [src/app/windows_terminal_self_test.rs](src/app/windows_terminal_self_test.rs)

Those should be extended, not replaced, with:

- a headless terminal replay benchmark command
- a headless render benchmark command
- artifact writing for failed visual assertions so broken states can be inspected without rerunning manually

## Allocation strategy

The new engine should treat allocations as part of the API design, not an afterthought.

### Principle

The hot path should prefer:

- preallocated buffers
- stable row storage
- reusable scratch arenas
- explicit capacity growth policies

The hot path should avoid:

- allocating a new `Vec` per output slice
- allocating per cell during visible-row extraction
- rebuilding whole-display structures when only a few rows changed
- producing transient owned strings where spans, slices, symbols, or interned structures would do

### Arena usage model

A practical starting model is:

- one long-lived arena or slab-like storage for terminal rows and cells
- one reusable scratch arena for parser temporary structures per flush or per worker cycle
- one reusable extraction arena for render-facing row snapshots
- one reusable scene arena for render fragments before GPU upload

That does not require a single monolithic arena for the whole app. It means we should allocate by lifetime domain.

Suggested lifetime domains:

- terminal lifetime: scrollback rows, stable style runs, interned grapheme or attribute data if needed
- worker-cycle lifetime: parser temporaries, dirty-row worklists, state-machine scratch
- frame lifetime: render-scene scratch, temporary vertex staging, readback scratch

### Arena rules

- allocate on creation or on controlled growth points
- clear and reuse scratch arenas between cycles instead of freeing and reallocating
- make capacity changes observable in benchmarks and traces
- treat unexpected steady-state allocations as regressions

### What to benchmark for allocation behavior

For the `1..10000` replay benchmark, measure at least:

- total bytes parsed
- total allocations if measurable
- peak capacity for row storage
- peak capacity for parser scratch
- peak capacity for display extraction scratch
- time spent in output application
- time spent in visible-row extraction

Even if we do not have a perfect allocator probe on day one, we should at minimum expose growth counters and buffer-capacity counters from Teamy-owned structures.

## 144 Hz target interpretation

`144 Hz zero latency` should be read as an engineering target, not magical wording.

Operationally, that means:

- the engine must be able to accept and apply new bytes without waiting on a visible frame boundary
- render publication should be capable of keeping up with a `6.94 ms` frame budget when damage is small
- no artificial coalescing policy should delay visible progress when the system is actually capable of showing the next incremental state
- benchmark harnesses should report whether we missed the frame budget, not just total completion time

The right metrics therefore include:

- median and max VT apply time per slice
- median and max row-extraction time per publish
- median and max render time per frame
- missed-frame count against a `144 Hz` budget
- end-to-end completion time

## Feature staging

The new engine should be built in slices, with each slice closing a useful workload instead of chasing theoretical completeness.

### Slice 0. Define the boundary and dual-engine mode

Deliverables:

- Teamy-owned engine trait
- Ghostty adapter behind that trait
- Teamy-Studio depending on the trait instead of directly on Ghostty types in most of the file
- no behavioral change yet

Acceptance:

- app still runs through the Ghostty adapter
- existing tests still pass
- new engine can be instantiated in tests even if it only supports trivial text initially

### Slice 1. Transcript capture and replay harness

Deliverables:

- a reusable transcript format checked into the repo
- a replay test harness that can drive both engines from the same data
- first captured transcripts from throughput and prompt scenarios

Acceptance:

- one command or test module can replay captured transcripts deterministically
- captured transcripts can produce comparable end-state assertions without a live shell
- failed replay or visual assertions can write artifacts for inspection automatically

### Slice 2. Minimal output parser and screen model

Implement only what is needed for:

- printable ASCII and UTF-8 text already observed in our workloads
- newline and carriage-return handling
- wrapping
- scrollback growth
- cursor movement required by the benchmark and common prompts
- basic SGR colors used by PowerShell and cmd

Acceptance:

- transcript replays for the throughput cases match the reference engine closely enough on visible rows and cursor state
- engine-only microbenchmarks exist
- benchmark output includes allocation and capacity-growth counters for the Teamy engine

### Slice 3. Row damage and render-facing extraction

Deliverables:

- persistent row storage owned by Teamy
- row or region dirtiness emitted directly by the engine
- row-oriented visible display extraction without Ghostty render iterators

Acceptance:

- Teamy-Studio renderer can consume Teamy-engine display rows in the same shape it already expects
- unchanged rows are not rebuilt during replay unless globally invalidated
- a headless render path can render terminal states to an image or readback texture for tests

### Slice 4. Keyboard and prompt behavior

Deliverables:

- Teamy-owned keyboard encoder for the subset already covered by the self-test harness
- Teamy-owned semantic prompt tracking integrated with the parser path

Acceptance:

- keyboard self-tests continue to pass for supported scenarios
- prompt-heavy throughput scenarios match expected prompt state

### Slice 5. Resize and reflow correctness

Deliverables:

- deterministic resize/reflow behavior
- bottom-anchor handling and viewport preservation tests
- transcript fixtures that include resizes

Acceptance:

- the current prompt-disappears-after-resize bug can be reproduced from fixtures and then fixed in Teamy-owned code

### Slice 6. Cutover and dependency removal

Deliverables:

- Teamy engine becomes default
- Ghostty adapter remains only as an optional comparison path until confidence is high
- `libghostty-vt` dependency removed when no longer needed

Acceptance:

- supported flows pass against Teamy engine by default
- throughput and keyboard gates are stable enough to stop using Ghostty as the day-to-day reference
- the `pwsh.exe -NoProfile` `1..10000` benchmark has both replay and live-shell measurements tracked over time

## Progress tracking

Status key:

- `[done]` already present in the repo
- `[next]` immediate focus
- `[later]` important but not the next slice

## Current live Teamy situation

The current live Teamy-backed window can launch, render, accept input, and drive the existing worker and renderer pipeline, but the latest Tracy investigation changed the diagnosis of the remaining usability problem.

The dominant long spans in the live trace are now blocked waits rather than hidden computation inside Teamy parsing or D3D12 rendering. In particular, the UI thread is mostly blocked waiting for messages, the worker is mostly blocked waiting for requests, and the PTY reader thread is mostly blocked waiting for shell output. That means the earlier "missing spans" problem has been addressed; the remaining issue is not that Teamy is visibly spending five seconds computing per keypress inside an uninstrumented hot path.

The current leading hypothesis is that Teamy is still missing terminal-originated reply behavior that Ghostty was previously providing. Ghostty has a PTY write-effect path for terminal responses, while the Teamy engine currently owns only a subset of parsing and screen mutation semantics. If PowerShell, PSReadLine, or the upstream console stack emits terminal queries and waits for replies that Teamy does not send, the shell can appear to stall even though Teamy itself is mostly idle.

That means the next live-performance slices must focus on protocol completeness for query-and-reply behavior, not on more generic chunk-size tuning or render-loop tuning.

The latest prompt-artifact fix narrowed another important gap: Teamy also needs to consume non-rendered OSC control traffic inside the VT engine itself, not only in the worker's semantic-prompt sidecar. Prompt markers and title updates were already being observed for shell-state tracking, but the Teamy display engine was still painting those bytes into the visible grid until the OSC parser path was added. That points to the next architectural cleanup: move non-rendered control-sequence handling toward a shared Teamy-owned parser layer so prompt tracking, title handling, and future control-only channels do not require parallel byte scanning paths.

- `[done]` multi-scenario throughput self-test exists
- `[done]` queue-latency and pending-output instrumentation exists in Teamy-Studio
- `[done]` terminal display publication and damage-oriented row reuse groundwork exists in Teamy-Studio
- `[done]` define a Teamy-owned live engine boundary and Ghostty adapter path in the app runtime
- `[done]` add transcript capture and deterministic replay fixtures
- `[done]` add a headless render-to-image path for `RenderFrameModel`
- `[done]` add differential tests comparing Ghostty and Teamy engines over captured transcripts
- `[done]` add Teamy-owned PTY reply coverage for terminal queries that block shell redraw flows
- `[done]` extend self-tests so `pwsh` redraw scenarios can be run against `--vt-engine teamy` with explicit latency thresholds
- `[done]` consume non-rendered OSC prompt and title sequences inside the Teamy VT engine so prompt markers do not leak into the visible screen
- `[done]` implement Teamy alternate-screen restore, cursor-visibility private modes, and `CSI Ps SP q` cursor-style handling so TUIs like `hx` can exit cleanly without leaving the shell screen corrupted
- `[next]` capture and reduce any remaining unsupported query or styling sequences after the PTY reply path lands
- `[done]` implement and verify Teamy colorization and SGR coverage so shell prompts, command output, selection contrast, and future parity checks stop using the current single-color fallback
- `[next]` capture and reduce any remaining TUI private-mode or buffer-semantics mismatches beyond the current alternate-screen and cursor-style restore subset
- `[next]` collapse the worker-side semantic prompt observer and Teamy VT engine OSC handling toward a shared Teamy-owned control-sequence parser
- `[next]` move more keyboard encoding responsibility behind the Teamy engine boundary so Ghostty-specific key helpers stop defining the live runtime contract
- `[later]` implement Teamy parser and screen model for the benchmark-first subset beyond the current redraw/query subset
- `[later]` move hot-path data structures toward reusable arena/scratch allocation domains
- `[later]` implement Teamy keyboard encoder and full prompt-state ownership
- `[later]` cut over default runtime to Teamy engine and remove Ghostty

## Practical repository changes to make first

1. Add the new Teamy terminal crate.
2. Introduce an adapter trait and move Ghostty-specific types behind it.
3. Add transcript fixtures under a repo-owned test-data directory.
4. Add a headless render-to-image path for renderer assertions.
5. Add differential replay tests.
6. Only then begin replacing parser functionality slice by slice.

If we skip those first steps and jump straight into writing escape-sequence code, we will repeat the current problem in a different form: too much guessing, not enough narrowing.

## OpenConsole and Microsoft Terminal role

OpenConsole remains useful in two ways:

- as a source of ideas for threading, buffering, and damage-driven presentation
- as a behavior reference when deciding how Windows-hosted console flows usually behave

But it should not become the substitute black box. The terminal engine should still be Teamy-owned, Rust-native, and directly testable.

## Success criteria

This effort is successful when all of the following are true:

- Teamy-Studio can run its supported shell scenarios without `libghostty-vt`
- transcript replay tests make regressions obvious and localizable
- keyboard and prompt regressions are reproducible without manual shell sessions
- engine-only benchmarks tell us parser cost independently from rendering cost
- the `terminal-throughput` acceptance gate improves materially, with a path toward sub-`1000 ms`

## Resume point

When resuming this effort, do not start by tuning chunk sizes again.

Start with:

1. keeping the live `pwsh` redraw problem reproducible through the keyboard self-test harness against `--vt-engine teamy`
2. reducing the next unsupported live mismatch from current Teamy warnings into a small permanent regression case using the existing redraw harnesses and transcript fixtures
3. collapsing prompt-marker and other control-only OSC handling into a shared Teamy-owned parser path instead of maintaining parallel worker-side and display-engine observers
4. moving keyboard encoding and other terminal-originated behavior behind the Teamy engine boundary before returning to broader parser completeness and throughput tuning

Those steps turn this from an aspiration into an executable migration.
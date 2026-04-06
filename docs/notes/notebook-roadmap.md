# Notebook Roadmap

This note captures the staged plan for evolving Teamy Studio from a single PTY-backed terminal window into a notebook-style native application built around PowerShell cells.

## Product Direction

The target experience is a workspace of independently positioned cell windows.

Each cell window should eventually contain:

- a left sidecar with drag affordance, cell number, play button, and stop button
- a main code panel hosting a live shell terminal for the cell
- a right sidecar for declared inputs
- a bottom output panel showing the last-result display and persisted output artifacts
- a plus affordance below the output panel to insert a new cell after the current one

The long-term value is not just replayable scripts. It is a flow where users can explore interactively, preserve the useful result, and later rerun a distilled version of that work.

## Proposed Cache Layout

The current cache home is the right place to store notebook workspace state.

```text
$TEAMY_STUDIO_CACHE_DIR/workspaces/{workspace-guid}/workspace_name.txt
$TEAMY_STUDIO_CACHE_DIR/workspaces/{workspace-guid}/workspace_cell_order.txt
$TEAMY_STUDIO_CACHE_DIR/workspaces/{workspace-guid}/cells/{cell-guid}/code.ps1
$TEAMY_STUDIO_CACHE_DIR/workspaces/{workspace-guid}/cells/{cell-guid}/inputs.txt
$TEAMY_STUDIO_CACHE_DIR/workspaces/{workspace-guid}/cells/{cell-guid}/output.xml
$TEAMY_STUDIO_CACHE_DIR/workspaces/{workspace-guid}/cells/{cell-guid}/run1.transcript
```

This layout is intentionally simple, inspectable, and compatible with PowerShell-native persistence formats such as `Export-Clixml` and `Start-Transcript`.

## Design Constraints

- testable functions first, especially path-building and persistence seams
- CLI and path-resolution logic must continue taking resolved homes as parameters instead of reaching into globals inside business logic
- specs must describe only observable behavior that actually exists
- each incremental step should keep the app runnable and demonstrably better than the previous step
- transcript and output artifacts should remain easy to inspect directly on disk

## Phase 0: Stabilize Shell Selection

Goal: make the configured default shell reliable for both inline and windowed launch paths.

Exit criteria:

- `shell default set <program>` persists a simple text argv file
- `shell default show` reports the effective command
- bare Windows names such as `pwsh` work in the PTY-backed window path
- regression tests cover the launch-resolution seam

## Phase 1: Workspace And Cell Path Model

Goal: introduce testable path helpers and plain-data identifiers for workspaces and cells without changing the UI yet.

Work items:

- add cache-path helpers for workspace root, cell root, code path, inputs path, output path, and transcript path
- define a stable on-disk ordering file for cells
- add unit tests for all path helpers and file naming rules
- document environment-variable impact on workspace storage

Exit criteria:

- creating and resolving workspace and cell paths is fully testable without spawning windows
- the spec covers the observable cache layout once it exists

## Phase 2: Single Cell Chrome

Goal: evolve the current single terminal window into one cell-shaped window with visible panel boundaries.

Work items:

- split the current window into layout regions matching the sketch
- reserve transparent gaps between panels instead of drawing a monolithic terminal surface
- keep dragging anchored to the left sidecar top grab area
- preserve terminal rendering inside only the code panel rectangle

Exit criteria:

- one cell window visually matches the intended frame structure
- dragging, painting, and input still work reliably
- layout math is unit-tested where possible

## Phase 3: Multi-Window Workspace Runtime

Goal: each cell becomes its own native window with stable workspace membership and ordering.

Work items:

- create a workspace controller that owns multiple cell windows
- launch each cell terminal in its cell directory under the workspace cache tree
- persist and restore cell ordering
- add insert-after and append-at-end flows for the plus affordance

Exit criteria:

- multiple cell windows can coexist, move independently, and reopen from persisted workspace state
- each cell terminal starts in its own cell directory

## Phase 4: Cell Run Artifacts

Goal: capture reproducible per-run artifacts from a cell session.

Work items:

- save code text as `code.ps1`
- capture transcript output with `Start-Transcript` and `Stop-Transcript`
- capture structured last-result output as `output.xml` via `Export-Clixml`
- define the display contract for what appears in the output panel versus what is only stored on disk

Exit criteria:

- rerunning a cell regenerates transcript and structured output artifacts
- the UI can show the latest persisted output summary

## Phase 5: Interactive Exploration And Distillation

Goal: let a user meander in a live shell session and then distill the useful result into rerunnable cell code.

Work items:

- decide how command history is captured for a cell session
- define what counts as the persisted rerun script versus ephemeral exploration noise
- determine whether distillation is manual, assisted, or automatic
- define failure semantics for partial transcripts and incomplete outputs

This phase should not start until the simpler replayable cell artifact model is stable.

## Testing Strategy

- red-green unit tests for path helpers, shell resolution helpers, and persistence functions
- integration tests for CLI behavior and artifact roundtrips
- manual smoke checks only for window interaction and visual layout until those seams are better isolated
- keep Tracey requirements and verification refs in sync with each finished phase

## Immediate Next Slice

The next implementation slice should stay small:

1. finish reliable default-shell launch behavior for the PTY window path
2. add workspace and cell cache-path helpers with unit tests
3. introduce a single-cell layout model that can be tested without spawning multiple windows

That sequence gives us a stable launch base, a durable storage model, and the first visible notebook-shaped UI step.
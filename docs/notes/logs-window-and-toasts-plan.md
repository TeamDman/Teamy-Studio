# Logs Window And Toasts Plan

## Goal

Replace the job-board-only feedback loop with app-wide observable feedback based on tracing events: a launcher Logs button, a selectable logs window, diagnostic/TUI coverage, and transient severity-colored toast messages.

## Current Status

- Done so far:
  - Read Teamy-Studio instructions and confirmed `./check-all.ps1` is the required validation command.
  - Reviewed Teamy's tracing initialization in `src/logging_init.rs` and scene rendering/input paths in `src/app/windows_app.rs` and `src/app/windows_scene.rs`.
  - Reviewed `cm`'s `egui_tracing`/`egui-toast` pattern and `piing`'s buffered-log replay pattern.
  - Confirmed the worktree was clean before starting.
  - Added a Teamy-native bounded tracing collector and attached it to logging initialization.
  - Added the observability Tracey spec/config entry.
  - Added a launcher Logs button, Logs scene rendering, virtual row scrolling, table controls, hover tooltips, and selectable/copyable normal and diagnostic logs text.
  - Added bottom-right info-level toast rendering from captured tracing events.
  - Replaced per-scene toasts with a process-wide transparent toast host window that renders on its own timer, uses `source_hwnd` when available, and prefers placement outside the source window.
  - Added warning/error toast eligibility, severity-colored toast panels, bottom progress bars, fade/translate animations, and stack-shift animation state.
  - Updated the log collector to inherit `source_hwnd` from active tracing spans and preserve bridged `log.target` fields from compatibility-layer events.
  - Capitalized log table headers, removed the inline Logs title from the scene body, and added severity-colored row bands plus colored level text.
  - Added span-propagating thread spawning for window-originated helper, launcher, audio capture, and transcription workers so their logs inherit source-window context.
  - Validated with `./check-all.ps1`; Rust fmt, clippy, build, and tests passed. Tracey status initially timed out while auto-starting the daemon, then passed on direct rerun.
- Current focus:
  - Completed for this slice.
- Remaining work:
  - Design a real Logs settings/filter panel; the Settings control currently emits a placeholder info log.
  - Expand toast placement from source-window side preference to fuller controlled-window surface packing across all Teamy windows and monitors.
  - Manually validate transparent toast host behavior across maximized windows and multi-monitor layouts.
  - Decide whether the Jobs board should be folded into or supplemented by the Logs window after the observability surface settles.
- Next step:
  - Manually exercise the Logs window and toast behavior in a running app session, then iterate on filtering/search if needed.

## Constraints And Assumptions

- Teamy-Studio uses a custom Win32/D3D scene renderer, not egui, so the `cm` implementation is a behavioral reference rather than a direct dependency target.
- Avoid introducing egui dependencies into Teamy's native renderer path.
- Logs should be captured in process from tracing events and kept bounded in memory.
- Window threads should enter a long-lived tracing span with `source_hwnd` so events created while processing that window inherit toast placement context, and worker threads spawned from that context should propagate the active tracing dispatcher and span.
- The table must flow oldest-to-newest from top to bottom, with newest entries at the bottom.
- The logs window should use existing scene windows, chrome, tooltip, selection, and copy behavior where possible.
- Existing job-board behavior can remain for now; this work adds logs/toasts as the richer feedback channel.

## Product Requirements

- The main launcher/menu exposes a Logs button.
- Activating Logs opens a Logs window.
- Logs window shows a table with time, level, target, and message columns.
- Log rows flow top-down with newest rows at the bottom.
- Logs window uses virtual scrolling and does not attempt to render the whole history at once.
- Logs window exposes to-bottom, clear, and settings controls using compact graphic controls with hover tooltips.
- Normal logs view text is selectable and copyable with Ctrl+C.
- Diagnostic view for the logs window presents logs as selectable TUI/text and is copyable with Ctrl+C.
- Info, warning, and error log events produce floating toast messages.
- Toasts stack from the bottom right as new events arrive, animate appearing/shifting/disappearing, and show remaining time with a bottom progress bar.
- Toasts prefer a floating transparent host outside the source window when a tracing event supplies `source_hwnd`.

## Architectural Direction

- Add a root `logs` module containing:
  - `LogRecordSnapshot` data for time, level, target, message, optional `source_hwnd`, and monotonically increasing id.
  - A `tracing_subscriber::Layer` that records events into a bounded `VecDeque`.
  - Query helpers for snapshots, latest id, clearing, and info events after a seen id.
- Wire the collector layer into `logging_init::init_logging` alongside stderr/json/Tracy layers.
- Extend `SceneWindowKind` and `SceneAction` with `Logs` / `OpenLogs`.
- Render the Logs scene with existing cell-grid text rendering so selection/copy semantics match other diagnostic text surfaces.
- Keep per-window UI state in `SceneAppState`: log scroll offset and follow-tail flag.
- Keep toast delivery in a process-wide transparent host window with its own timer so toasts continue updating independently from scene-window focus.
- Let diagnostic mode reuse `build_scene_diagnostic_render_scene` with log-specific diagnostic text.

## Tracey Specification Strategy

This is a new user-facing observability surface, so create a dedicated `docs/spec/product/observability.md` and add a `teamy-studio-observability` entry to `.config/tracey/config.styx`.

Planned requirements:

- `observability[logs.capture]`: tracing events are retained for in-app display.
- `observability[logs.span-context]`: captured logs inherit `source_hwnd` from spans and preserve bridged log targets.
- `observability[logs.launcher-button]`: launcher exposes a Logs button.
- `observability[logs.table]`: logs render as Time/Level/Target/Message rows.
- `observability[logs.severity-colors]`: logs render with severity row bands and colored level text.
- `observability[logs.virtual-scroll]`: logs table renders only visible rows from the bounded history.
- `observability[logs.controls]`: to-bottom, clear, and settings controls are present with hover tooltips.
- `observability[logs.copy]`: logs text is selectable/copyable in normal and diagnostic views.
- `observability[toasts.levels]`: info, warning, and error logs produce severity-colored toast messages.
- `observability[toasts.progress]`: toasts show remaining display time.
- `observability[toasts.animation]`: toasts animate appearing, shifting, and disappearing.
- `observability[toasts.floating-source]`: toasts render in a floating host and prefer placement outside the source window.

Tracey loop:

```powershell
tracey query status
tracey query uncovered
tracey query unmapped --path src/app/windows_app.rs
tracey query validate --deny warnings
tracey query untested
```

Repository validation remains:

```powershell
.\check-all.ps1
```

## Phased Task Breakdown

### Phase 1: Collector And Spec

Objective: establish durable in-memory log events and requirements tracking.

Tasks:

- Add `src/logs.rs` with bounded event capture and snapshot APIs.
- Add collector layer to `src/logging_init.rs`.
- Add `docs/spec/product/observability.md` and config entry.
- Add focused tests for message capture, bounded history, clear behavior, and info filtering.

Definition of done:

- Tests prove events can be captured and queried without the UI.
- Tracey sees the new observability spec.

### Phase 2: Logs Window

Objective: expose captured logs from the launcher and render a virtualized table.

Tasks:

- Add `SceneWindowKind::Logs` and `SceneAction::OpenLogs`.
- Add launcher button and action dispatch to spawn a Logs window.
- Add log table text formatting in `windows_app.rs`.
- Add `build_logs_render_scene` in `windows_scene.rs`.
- Add scroll state and wheel handling for virtual log rows.

Definition of done:

- Logs window opens from launcher.
- Table has time, level, target, and message columns.
- Wheel scrolling changes the visible row slice without rendering all history.

### Phase 3: Controls, Selection, Diagnostics

Objective: make the logs window useful as an inspection tool.

Tasks:

- Add to-bottom, clear, and settings control rects/visual states.
- Add hover tooltips for the three controls.
- Wire to-bottom to tail-following, clear to the collector clear API, and settings to a safe placeholder action.
- Make normal Logs text selectable and copyable through existing scene selection plumbing.
- Add log-specific diagnostic text so diagnostics mode is also selectable/copyable.

Definition of done:

- Controls work or give clear feedback.
- Ctrl+C copies selected log text in both normal and diagnostic logs views.

### Phase 4: Toasts

Objective: surface user-visible logs as transient floating feedback.

Tasks:

- Track the toast host's last seen log id.
- Add process-wide toast state, expiration, progress, and animation state.
- Render stacked severity-colored toast panels in a transparent floating host window.
- Position toasts outside the source window when `source_hwnd` is available.

Definition of done:

- New info, warning, and error logs produce temporary floating toasts.
- Toasts keep updating while regular scene windows are unfocused.

### Phase 5: Hardening

Objective: finish validation and document remaining decisions.

Tasks:

- Run `./check-all.ps1`.
- Fix clippy, test, or Tracey issues.
- Update this plan's Current Status with completed work and any remaining follow-ups.

Definition of done:

- Full validation passes or blockers are recorded precisely.

## Recommended Implementation Order

1. Collector module and tracing layer.
2. Observability spec/config.
3. Logs scene and launcher action.
4. Selection/copy and diagnostics text.
5. Controls and tooltips.
6. Toast rendering.
7. Validation and plan update.

## Open Decisions

- Whether Logs settings should open a real filter/settings panel in this slice or remain a placeholder until filtering/search is designed.
- How much controlled-window/monitor occupancy logic should feed toast placement beyond the current source-window side preference.
- Whether the Jobs board should be deprecated after Logs proves useful.

## First Concrete Slice

Implement `src/logs.rs`, attach it in `logging_init`, add the observability spec, and add unit tests for bounded capture and info-toast filtering before wiring the scene UI.

# Observability

Teamy Studio should surface internal activity in user-facing form so actions and background work produce visible feedback.

observability[logs.capture]
Tracing events must be retained in a bounded in-process log buffer so scene windows can display recent application activity.

observability[logs.span-context]
Captured log records must inherit source window handles from active tracing spans, including spans propagated into worker threads, and preserve bridged log targets when events originate from the log compatibility layer.

observability[logs.launcher-button]
The launcher must expose a Logs button that opens the logs window.

observability[logs.table]
The logs window must show log rows with Time, Level, Target, and Message columns, ordered from oldest at the top to newest at the bottom.

observability[logs.severity-colors]
The logs window must visually distinguish log severity with row color bands and colored level text.

observability[logs.virtual-scroll]
The logs window must render a visible slice of the bounded log history instead of rendering every retained event at once.

observability[logs.controls]
The logs window must expose to-bottom, clear, and settings controls with hover tooltips.

observability[logs.copy]
Log text must be selectable and copyable with Ctrl+C in both the normal logs view and the diagnostic logs view.

observability[toasts.levels]
Info, warning, and error log events must produce transient toast messages with severity-specific colors.

observability[toasts.progress]
Toast messages must show a bottom progress bar for the remaining display time.

observability[toasts.animation]
Toast messages must animate appearing, shifting, and disappearing with translation and opacity changes.

observability[toasts.floating-source]
Toast messages must render in a process-wide floating window and prefer placement outside the source window while staying within available monitor or virtual-screen bounds.

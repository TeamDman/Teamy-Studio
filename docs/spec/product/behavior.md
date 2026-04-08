# Behavior

This specification covers the Teamy Studio application model that users experience after launching the app: the window, the notebook panels inside it, and the interactions that make the UI feel immediate.

## Workspace Windows

behavior[workspace.plus-button.appends-cell]
Clicking the plus button in a workspace cell window must append a new cell to that workspace and open the new cell in its own window.

## Window Startup

behavior[window.startup.centered]
The launched terminal window must open centered on screen.

behavior[window.startup.size]
The launched terminal window must start at a fixed size suitable for an 80x24-style shell surface.

## Window Appearance

behavior[window.appearance.shell]
The launched window must host a shell backed by a PTY and render terminal content through `libghostty-vt`.

behavior[window.appearance.shell-configured-default]
The launched window must start the effective default shell command rather than a hard-coded shell executable.

behavior[window.appearance.shell-starts-in-workspace-cell-dir]
When a workspace is launched, the PTY-backed shell must start in that workspace's selected cell directory.

behavior[window.appearance.chrome]
The launched terminal window must render a visible accent strip above the terminal grid.

behavior[window.appearance.drag-cursor]
Hovering the pointer over the purple drag strip must show a move-style drag affordance instead of the default arrow cursor.

behavior[window.appearance.panel-borders.absolute-pixels]
Panel edge highlights must use absolute pixel thickness rather than proportional UV scaling so the border treatment stays visually consistent across large and small panels.

behavior[window.appearance.backgrounds.animated-time-based]
Shader-driven panel backgrounds must animate from elapsed time rather than frame count so the motion reads the same at different refresh rates.

behavior[window.appearance.backgrounds.blue-half-transparent]
The blue background panel must render at 50% alpha beneath the opaque notebook panels.

behavior[window.appearance.code-panel.single-surface]
The code area must read as a single panel surface rather than an outer code panel containing a second nested framed terminal panel.

behavior[window.appearance.code-panel.terminal-alignment]
The terminal area must align with the bottom-left of the code area instead of appearing as a separately inset framed region.

behavior[window.appearance.terminal.selection.inverse]
Selected terminal cells must render with visible reverse-video style rather than only dimming the foreground glyphs.

behavior[window.appearance.terminal.cursor.visible]
The terminal caret must be visibly rendered using the terminal's active cursor position and cursor style.

behavior[window.appearance.terminal.cursor.legible-block]
Block-style terminal cursors must keep the glyph beneath them legible instead of fully obliterating the cell contents.

## Window Interaction

behavior[window.interaction.drag]
The launched terminal window must be draggable by clicking and dragging on the top accent strip.

behavior[window.interaction.drag.live]
While the user is holding the top accent strip to reposition the window, the app must keep presenting frames immediately, without a noticeable startup pause, even if the pointer pauses and the window bounds temporarily stop changing.

behavior[window.interaction.resize.live]
While the user is actively resizing the window, the presented UI must continue reacting immediately during the full grab, including moments when the pointer pauses and the client size is temporarily unchanged, instead of freezing and snapping only after the drag ends.

behavior[window.interaction.resize.terminal-live-output]
Interactive resize must not stall terminal output presentation while other app-rendered panels continue updating.

behavior[window.interaction.resize.low-latency]
Interactive resize must prioritize minimal latency so panel layout and terminal presentation track the live window dimensions as closely as possible.

behavior[window.interaction.input]
The launched terminal window must forward keyboard input into the PTY-backed shell session.

behavior[window.interaction.input.numpad-numlock-text]
When NumLock is enabled, numpad digit and operator keys must be forwarded as their text characters rather than being dropped.

behavior[window.interaction.zoom.terminal]
Holding Ctrl while scrolling over the terminal area must adjust the terminal text scale and recompute the terminal column and row count to fit the resized cell grid.

behavior[window.interaction.zoom.output]
Holding Ctrl while scrolling over the output panel must adjust only the output panel text scale and must not change the terminal grid size.
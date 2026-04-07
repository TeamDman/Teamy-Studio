# CLI

This specification covers the current user-facing command line behavior exposed by Teamy Studio.

## Command Surface

cli[command.surface.core]
Invoking `teamy-studio.exe` with no explicit command must behave like `teamy-studio.exe workspace run` with no explicit workspace target.

cli[command.surface.workspace]
The CLI must expose a `workspace` command group.

cli[command.surface.workspace-list]
The `workspace` command group must expose a `list` subcommand.

cli[command.surface.workspace-show]
The `workspace` command group must expose a `show` subcommand.

cli[command.surface.workspace-create]
The `workspace` command group must expose a `create` subcommand with an optional workspace name argument.

cli[command.surface.workspace-run]
The `workspace` command group must expose a `run` subcommand with an optional workspace id-or-name target.

cli[command.surface.shell]
The CLI must expose a `shell` command group.

cli[shell.inline.launches-configured-default]
Invoking `teamy-studio.exe shell` with no explicit shell subcommand must launch the effective default shell inline in the current console.

cli[command.surface.shell-default]
The `shell` command group must expose a `default` subcommand group.

cli[command.surface.shell-default-set]
The `shell default` command group must expose a `set` subcommand that persists a shell program plus trailing arguments.

cli[command.surface.shell-default-show]
The `shell default` command group must expose a `show` subcommand that prints the effective default shell command.

cli[command.surface.self-test]
The CLI must expose a `self-test` command group.

cli[command.surface.self-test-keyboard-input]
The `self-test` command group must expose a `keyboard-input` subcommand.

cli[self-test.keyboard-input.inside-flag]
The `self-test keyboard-input` command must support `--inside` to run the terminal-side probe instead of the outer harness.

cli[command.surface.window]
The CLI must expose a `window` command group.

cli[command.surface.window-show]
The `window` command group must expose a `show` subcommand that launches the main application terminal window.

## Workspaces

cli[workspace.list.prints-id-name-cell-count]
The `workspace list` command must print each workspace with its id, name, and cell count.

cli[workspace.show.bails-when-missing]
The `workspace show` command must fail when the requested workspace id or exact name does not exist.

cli[workspace.show.prints-id-name-cell-count]
The `workspace show` command must print the workspace id, name, and cell count for the resolved workspace.

cli[workspace.create.name-optional]
The `workspace create` command must accept an optional workspace display name.

cli[workspace.run.no-target-creates-workspace]
The `workspace run` command must create a new workspace when no workspace target is provided.

cli[workspace.run.target-by-id-or-name]
The `workspace run` command must resolve an existing workspace by exact id or exact name when a target is provided.

cli[workspace.plus-button.appends-cell]
Clicking the plus button in a workspace cell window must append a new cell to that workspace and open the new cell in its own window.

## Shell Defaults

cli[shell.default.persisted-in-app-home]
The persisted default shell command must be stored as a simple text file under the resolved application home directory.

cli[shell.default.show-effective]
The `shell default show` command must print the effective default shell command as a single formatted command line.

cli[shell.default.fallback.builtin]
If no persisted default shell command exists, Teamy Studio must fall back to a built-in default shell command.

cli[shell.default.fallback.windows-comspec]
On Windows, the built-in default shell command must use `COMSPEC` when it is set or `cmd.exe` otherwise, and it must include `/D`.

cli[shell.default.windows-launch-resolves-program-on-path]
On Windows, when the configured default shell program is a bare executable name without path separators, Teamy Studio must resolve it through `PATH` and `PATHEXT` before launching it inside the PTY-backed window.

cli[shell.default.set.double-dash-trailing-args]
The `shell default set` command must accept dash-prefixed shell arguments after a `--` delimiter so they are treated as trailing shell arguments rather than Teamy Studio CLI flags.

## Window Behavior

cli[window.startup.centered]
The launched terminal window must open centered on screen.

cli[window.startup.size]
The launched terminal window must start at a fixed size suitable for an 80x24-style shell surface.

cli[window.appearance.translucent]
The launched terminal window must use layered-window alpha so the shell surface remains translucent.

cli[window.appearance.os-chrome-none]
The launched terminal window must not show OS-managed chrome or decorations such as the title bar, icon, caption buttons, or user-preference-colored window borders.

cli[window.appearance.shell]
The launched window must host a shell backed by a PTY and render terminal content through `libghostty-vt`.

cli[window.appearance.shell-configured-default]
The launched window must start the effective default shell command rather than a hard-coded shell executable.

cli[window.appearance.shell-starts-in-workspace-cell-dir]
When a workspace is launched, the PTY-backed shell must start in that workspace's selected cell directory.

cli[window.appearance.chrome]
The launched terminal window must render a visible accent strip above the terminal grid.

cli[window.appearance.drag-cursor]
Hovering the pointer over the purple drag strip must show a move-style drag affordance instead of the default arrow cursor.

cli[window.appearance.panel-borders.absolute-pixels]
Panel edge highlights must use absolute pixel thickness rather than proportional UV scaling so the border treatment stays visually consistent across large and small panels.

cli[window.appearance.backgrounds.animated-time-based]
Shader-driven panel backgrounds must animate from elapsed time rather than frame count so the motion reads the same at different refresh rates.

cli[window.appearance.backgrounds.blue-half-transparent]
The blue background panel must render at 50% alpha beneath the opaque notebook panels.

cli[window.appearance.code-panel.single-surface]
The code area must read as a single panel surface rather than an outer code panel containing a second nested framed terminal panel.

cli[window.appearance.code-panel.terminal-alignment]
The terminal area must align with the bottom-left of the code area instead of appearing as a separately inset framed region.

cli[window.interaction.drag]
The launched terminal window must be draggable by clicking and dragging on the top accent strip.

cli[window.interaction.drag.live]
While the user is holding the top accent strip to reposition the window, the app must keep presenting frames even if the pointer pauses and the window bounds temporarily stop changing.

cli[window.interaction.resize.native-edges]
The launched terminal window must still resize from its edges and corners using native OS hit-testing semantics and native resize cursors even though OS-managed chrome is hidden.

cli[window.interaction.resize.live]
While the user is actively resizing the window, the presented UI must continue reacting during the full grab, including moments when the pointer pauses and the client size is temporarily unchanged, instead of freezing and snapping only after the drag ends.

cli[window.interaction.resize.low-latency]
Interactive resize must prioritize minimal latency so panel layout and terminal presentation track the live window dimensions as closely as possible.

cli[window.interaction.input]
The launched terminal window must forward keyboard input into the PTY-backed shell session.

cli[window.interaction.input.numpad-numlock-text]
When NumLock is enabled, numpad digit and operator keys must be forwarded as their text characters rather than being dropped.

cli[window.interaction.zoom.terminal]
Holding Ctrl while scrolling over the terminal area must adjust the terminal text scale and recompute the terminal column and row count to fit the resized cell grid.

cli[window.interaction.zoom.output]
Holding Ctrl while scrolling over the output panel must adjust only the output panel text scale and must not change the terminal grid size.

## Parser Model

cli[parser.args-consistent]
The structured CLI model must serialize to command line arguments consistently for parse-safe values.

cli[parser.roundtrip]
The structured CLI model must roundtrip through argument serialization and parsing for parse-safe values.

## Path Resolution

cli[path.app-home.env-overrides-platform]
If `TEAMY_STUDIO_HOME_DIR` is set to a non-empty value, it must take precedence over the platform-derived application home directory.

cli[path.cache.env-overrides-platform]
If `TEAMY_STUDIO_CACHE_DIR` is set to a non-empty value, it must take precedence over the platform-derived cache directory.

cli[path.cache.workspace-root-under-workspaces-dir]
Notebook workspace state under the cache home must live beneath a `workspaces/{workspace-guid}` directory.

cli[path.cache.workspace-name-file]
The notebook workspace cache layout must store the workspace display name in `workspace_name.txt` at the workspace root.

cli[path.cache.workspace-cell-order-file]
The notebook workspace cache layout must store cell ordering in `workspace_cell_order.txt` at the workspace root.

cli[path.cache.cell-artifact-layout]
Each notebook cell cache layout must place cell artifacts beneath `cells/{cell-guid}` and expose `code.ps1`, `inputs.txt`, and `output.xml` paths in that directory.

cli[path.cache.cell-transcript-numbering]
Per-run notebook cell transcripts must use `run{n}.transcript` naming with a positive run number.
# CLI

This specification covers the current user-facing command line behavior exposed by Teamy Studio.

## Command Surface

cli[command.surface.core]
Invoking `teamy-studio.exe` with no explicit command must launch the main application terminal window.

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

cli[window.appearance.shell]
The launched window must host a shell backed by a PTY and render terminal content through `libghostty-vt`.

cli[window.appearance.shell-configured-default]
The launched window must start the effective default shell command rather than a hard-coded shell executable.

cli[window.appearance.chrome]
The launched terminal window must render a visible accent strip above the terminal grid.

cli[window.interaction.drag]
The launched terminal window must be draggable by clicking and dragging on the top accent strip.

cli[window.interaction.input]
The launched terminal window must forward keyboard input into the PTY-backed shell session.

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
# CLI

This specification covers the current user-facing command line behavior exposed by Teamy Studio.

## Command Surface

cli[command.surface.core]
Invoking `teamy-studio.exe` with no explicit command must launch the main application terminal window.

cli[command.surface.window]
The CLI must expose a `window` command group.

cli[command.surface.window-show]
The `window` command group must expose a `show` subcommand that launches the main application terminal window.

cli[window.startup.centered]
The launched terminal window must open centered on screen.

cli[window.startup.size]
The launched terminal window must start at a fixed size suitable for an 80x24-style shell surface.

cli[window.appearance.translucent]
The launched terminal window must use layered-window alpha so the shell surface remains translucent.

cli[window.appearance.shell]
The launched window must host a shell backed by a PTY and render terminal content through `libghostty-vt`.

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
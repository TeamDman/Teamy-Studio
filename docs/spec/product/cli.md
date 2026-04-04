# CLI

This specification covers the current user-facing command line behavior exposed by Teamy Studio.

## Command Surface

cli[command.surface.core]
Invoking `teamy-studio.exe` with no explicit command must launch the main application window.

cli[command.surface.window]
The CLI must expose a `window` command group.

cli[command.surface.window-show]
The `window` command group must expose a `show` subcommand that launches the main application window.

cli[window.startup.monitor-selection]
The launched window must open on the monitor that currently contains the cursor.

cli[window.startup.size]
The launched window must start at 50% of the width and 50% of the height of the selected monitor.

cli[window.startup.centered]
The launched window must be centered within the selected monitor.

cli[window.appearance.red]
The launched window must render as red at 50% opacity.

cli[window.interaction.drag]
The launched window must be draggable by clicking and dragging on the red surface.

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
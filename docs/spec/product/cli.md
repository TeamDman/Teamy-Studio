# CLI

This specification covers the current Teamy Studio command surface, command-specific behavior, parser model, and path resolution rules.

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

cli[command.surface.self-test-terminal-throughput]
The `self-test` command group must expose a `terminal-throughput` subcommand.

cli[self-test.keyboard-input.inside-flag]
The `self-test keyboard-input` command must support `--inside` to run the terminal-side probe instead of the outer harness.

cli[self-test.keyboard-input.scenario-optional]
The `self-test keyboard-input` command must accept an optional scenario argument so reproducible terminal regressions can be exercised from the outer harness.

cli[self-test.terminal-throughput.mode-optional]
The `self-test terminal-throughput` command must accept an optional benchmark mode argument.

cli[self-test.terminal-throughput.line-count-flag]
The `self-test terminal-throughput` command must support `--line-count` to control the emitted `Out-Host` line count.

cli[self-test.terminal-throughput.samples-flag]
The `self-test terminal-throughput` command must support `--samples` to run multiple benchmark samples and report median results.

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

## Shell Defaults

cli[shell.default.persisted-in-app-home]
The persisted default shell command must be stored as a simple text file under the resolved application home directory.

cli[shell.default.show-effective]
The `shell default show` command must print the effective default shell command as a single formatted command line.

cli[shell.default.fallback.builtin]
If no persisted default shell command exists, Teamy Studio must fall back to a built-in default shell command.

cli[shell.default.set.double-dash-trailing-args]
The `shell default set` command must accept dash-prefixed shell arguments after a `--` delimiter so they are treated as trailing shell arguments rather than Teamy Studio CLI flags.

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
# CLI

This specification covers the current Teamy Studio command surface, command-specific behavior, parser model, and path resolution rules.

## Command Surface

cli[command.surface.core]
Invoking `teamy-studio.exe` with no explicit command must open the launcher window.

cli[command.surface.terminal]
The CLI must expose a `terminal` command group.

cli[command.surface.terminal-default-shell]
The `terminal` command group must expose a `default-shell` subcommand group.

cli[command.surface.terminal-default-shell-set]
The `terminal default-shell` command group must expose a `set` subcommand that persists a shell program plus trailing arguments.

cli[command.surface.terminal-default-shell-show]
The `terminal default-shell` command group must expose a `show` subcommand that reports the effective default shell command.

cli[global.output-format]
The CLI must support a global `--output-format text|json|csv` flag.

cli[global.output-format.default-terminal]
When `--output-format` is omitted and stdout is attached to a terminal, Teamy Studio must render command output as text.

cli[global.output-format.default-redirected]
When `--output-format` is omitted and stdout is redirected, Teamy Studio must render command output as pretty JSON.

cli[command.surface.terminal-open]
The `terminal` command group must expose an `open` subcommand that opens a new terminal window.

cli[command.surface.terminal-list]
The `terminal` command group must expose a `list` subcommand.

cli[command.surface.self-test]
The CLI must expose a `self-test` command group.

cli[command.surface.self-test-keyboard-input]
The `self-test` command group must expose a `keyboard-input` subcommand.

cli[command.surface.self-test-terminal-throughput]
The `self-test` command group must expose a `terminal-throughput` subcommand.

cli[command.surface.self-test-terminal-replay]
The `self-test` command group must expose a headless terminal replay subcommand for deterministic transcript-driven validation.

cli[command.surface.self-test-render-offscreen]
The `self-test` command group must expose a headless offscreen render subcommand for terminal-frame verification without a visible window.

cli[self-test.keyboard-input.inside-flag]
The `self-test keyboard-input` command must support `--inside` to run the terminal-side probe instead of the outer harness.

cli[self-test.keyboard-input.scenario-optional]
The `self-test keyboard-input` command must accept an optional scenario argument so reproducible terminal regressions can be exercised from the outer harness.

cli[self-test.keyboard-input.artifact-output]
The `self-test keyboard-input` command must support optional artifact output so captured keyboard and redraw transcripts can be written to disk for reduction into regression fixtures.

cli[self-test.keyboard-input.vt-engine-flag]
The `self-test keyboard-input` command must support `--vt-engine ghostty|teamy` so live keyboard regressions can be replayed against either terminal backend.

cli[self-test.terminal-throughput.mode-optional]
The `self-test terminal-throughput` command must accept an optional benchmark mode argument.

cli[self-test.terminal-throughput.line-count-flag]
The `self-test terminal-throughput` command must support `--line-count` to control the emitted `Out-Host` line count.

cli[self-test.terminal-throughput.samples-flag]
The `self-test terminal-throughput` command must support `--samples` to run multiple benchmark samples and report median results.

cli[self-test.terminal-replay.artifact-output]
The headless terminal replay self-test must support writing failure artifacts so broken states can be inspected after automated runs.

cli[self-test.render-offscreen.artifact-output]
The headless offscreen render self-test must support writing image artifacts for automated verification and debugging.

cli[self-test.render-offscreen.fixture-flag]
The headless offscreen render self-test must support selecting a named built-in render fixture.

cli[self-test.render-offscreen.list-fixtures-flag]
The headless offscreen render self-test must support listing the available built-in render fixtures without executing one.

cli[self-test.render-offscreen.update-expected-flag]
The headless offscreen render self-test must support updating the expected image used for deterministic render verification.

## Shell Defaults

cli[shell.default.persisted-in-app-home]
The persisted default shell command must be stored as a simple text file under the resolved application home directory.

cli[shell.default.show-effective]
The `terminal default-shell show` command must report the effective default shell argv and formatted command line.

cli[shell.default.fallback.builtin]
If no persisted default shell command exists, Teamy Studio must fall back to a built-in default shell command.

cli[shell.default.set.double-dash-trailing-args]
The `terminal default-shell set` command must accept dash-prefixed shell arguments after a `--` delimiter so they are treated as trailing shell arguments rather than Teamy Studio CLI flags.

## Terminals

cli[terminal.open.default-shell-when-program-omitted]
The `terminal open` command must use the effective default shell command when no explicit program argument is supplied.

cli[terminal.open.double-dash-trailing-args]
The `terminal open` command must accept dash-prefixed terminal arguments after a `--` delimiter so they are treated as trailing program arguments rather than Teamy Studio CLI flags.

cli[terminal.open.stdin-flag]
The `terminal open` command must support `--stdin` so text can be written to the terminal after the window is shown.

cli[terminal.open.title-flag]
The `terminal open` command must support `--title` so callers can seed the terminal chrome title.

cli[terminal.open.vt-engine-flag]
The `terminal open` command must support `--vt-engine ghostty|teamy` so callers can choose the terminal backend for the new window.

cli[terminal.open.current-vt-engine-env]
The `terminal open` command must set `TEAMY_STUDIO_CURRENT_TERMINAL_VT_ENGINE` in the spawned terminal process to `ghostty` or `teamy` for the selected backend.

cli[terminal.list.enumerates-live-windows]
The `terminal list` command must enumerate live Teamy Studio terminal windows from the operating system rather than from on-disk state.

cli[terminal.list.prints-hwnd-pid-and-title]
The `terminal list` command must report each live terminal window with its `HWND`, `PID`, and title.

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
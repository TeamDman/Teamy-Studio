<!-- repo[impl readme.explanation] -->
<!-- repo[impl readme.identity] -->
# Teamy Studio

Teamy Studio is a Windows-first desktop shell initialized from the shared Rust CLI scaffold and tuned for an application-first launch path.

Running `teamy-studio.exe` with no command-line arguments opens a translucent terminal window centered on screen. The window hosts a shell inside a PTY, renders terminal content through `libghostty-vt`, and can be repositioned by dragging the top accent strip.

<!-- repo[impl readme.media-demo] -->
![Teamy Studio media placeholder](resources/main.png)

## Current Behavior

- no arguments launches the desktop terminal window
- `shell` launches the configured default shell inline in the current console
- `shell default set <program> [args...]` persists the default shell command in the Teamy Studio home directory
- `shell default show` prints the effective default shell command
- `window show` launches the same terminal window explicitly
- `--help` and `--version` still work through the shared figue CLI plumbing
- structured logging can still be written to stderr and optional NDJSON files
- on Windows, bare shell names such as `pwsh` are resolved through `PATH` and `PATHEXT` before the PTY-backed window launches them

<!-- repo[impl readme.code-example] -->
## Example Usage

Launch the application:

```powershell
cargo run --
```

Launch the window explicitly through the CLI surface:

```powershell
cargo run -- window show
```

Persist PowerShell as the default shell and show the effective value:

```powershell
cargo run -- shell default set -- pwsh.exe -NoLogo
cargo run -- shell default show
```

Launch the configured default shell inline:

```powershell
cargo run -- shell
```

Inspect the CLI surface:

```powershell
cargo run -- --help
```

Write structured logs to disk while launching the app:

```powershell
cargo run -- --log-file .\logs window show
```

## Environment Variables

- `TEAMY_STUDIO_HOME_DIR`: overrides the resolved application home directory
- `TEAMY_STUDIO_CACHE_DIR`: overrides the resolved cache directory
- `RUST_LOG`: provides a tracing filter when `--log-filter` is not supplied

The home directory now stores the persisted default shell command in a simple text file. The cache directory remains scaffolded for later product work.

## Quality Gate

Run the standard validation flow with:

```powershell
./check-all.ps1
```

That script runs formatting, clippy, build, tests, and local tracey validation.

For Tracy profiling, run:

```powershell
./run-tracing.ps1 window show
```

<!-- repo[impl implementation.present] -->
## Repository Layout

```text
. # Some files omitted
├── .config/tracey/config.styx # Local tracey specification wiring
├── build.rs # Adds exe resources and embeds git revision
├── Cargo.toml # Package metadata and dependency wiring
├── docs/spec # Human-readable requirements for the repository and CLI
├── resources # Windows resources used by build.rs
├── src/app # Application startup and Win32 window logic
├── src/cli # CLI parsing and explicit commands
├── src/paths # Shared path resolution scaffolding kept for later work
└── tests # CLI roundtrip fuzz tests
```

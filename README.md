# Teamy Studio

Teamy Studio is a Windows-first desktop shell initialized from the shared Rust CLI scaffold and tuned for an application-first launch path.

Running `teamy-studio.exe` with no command-line arguments opens a centered translucent red window on the monitor under the cursor. The window starts at 50% of that monitor's width and height, and the full red surface is draggable.

![Teamy Studio media placeholder](resources/main.png)

## Current Behavior

- no arguments launches the desktop window
- `window show` launches the same window explicitly
- `--help` and `--version` still work through the shared figue CLI plumbing
- structured logging can still be written to stderr and optional NDJSON files

## Example Usage

Launch the application:

```powershell
cargo run --
```

Launch the window explicitly through the CLI surface:

```powershell
cargo run -- window show
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

The home and cache directories remain scaffolded for later product work. They are not part of the current user-facing command surface.

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

# Terminal Control Plane And Modular Window Composition

This note captures a concrete direction for evolving Teamy Studio from a single application window with hard-wired UI chrome into a more composable terminal platform.

The immediate motivation is practical: terminal bugs such as scrollback loss, prompt disappearance after zoom, and redraw mismatches are easier to isolate when the terminal surface can be created, manipulated, and snapshotted without unrelated panels or window chrome in the way.

The broader product motivation is also clear: the same seams that make testing and debugging better are the seams that can turn Teamy Studio into a more capable notebook-style application.

## Problem Statement

Today, several important concerns are tightly coupled:

- the terminal session lifecycle
- the terminal window and its decorations
- the output panel and sidecar UI
- ad hoc self-test scripts that manipulate the terminal through internal APIs
- the CLI surface, which can launch the app or run fixed self-tests, but cannot address a long-lived terminal as a first-class resource

That coupling causes two problems.

First, it makes some bugs hard to reproduce with precision. A bug in ctrl+wheel zoom, viewport restore, or prompt rendering may be caused by the terminal engine, the app-level resize path, the layout logic, or the renderer. If the only automation path is a special-purpose self-test harness, then the repro is harder to compose, inspect, and reuse.

Second, it limits the product direction. If the terminal, output surface, and chrome are one fixed shape, then Teamy Studio cannot gradually become a more dynamic notebook shell where windows and panels are optional, remote-controlled, or persisted independently.

## Design Goal

The design goal is to make terminal behavior addressable through a stable command surface.

That means:

- a terminal session becomes a named or identified resource
- terminal interactions become explicit commands or transactions
- optional UI surfaces become separate view resources instead of hard-coded decorations around the terminal
- snapshot, transcript, and screenshot capture become ordinary commands rather than special internal-only hooks

The important distinction is that this should not be a test-only interface.

The same control plane used for self-tests should also be the way Teamy Studio itself reasons about terminal actions. Tests should compose the same commands real product features rely on.

## Proposed Model

### Resource types

Teamy Studio should move toward a small set of addressable runtime resources.

- terminal session
- terminal view window
- output view window
- workspace or notebook cell
- snapshot artifact

Each resource should have a generated id and may also have an optional user-facing name.

Names should be unique per resource type. If the user requests a duplicate name, the command should fail instead of silently reusing or replacing the existing resource.

### MVP binding constraint

For the MVP, Teamy Studio should prefer a simple one-to-one relationship between a terminal session and a live attached window.

The reason is that zoom and window geometry are not purely cosmetic in the current terminal architecture. They determine the terminal's logical grid in rows and columns, which directly affects PTY resize behavior and the cell layout seen by the hosted process.

So the recommended MVP is:

- a terminal may exist headless
- a terminal may later have one live attached window
- that window owns the active zoom and pixel geometry for that terminal
- a second live window attached to the same terminal is out of scope for the MVP

The terminal-versus-window distinction still matters, but the first shipping version should not try to support multi-view fanout.

### Detached headless terminals

The headless terminal case is not just for tests. It is also the right model for background or startup tasks that should begin running before any window is attached.

Examples:

- startup scripts launched when the user signs in
- background bootstrap terminals that prepare a working environment
- long-lived detached shells that the user may want to inspect later

That suggests a detached lifecycle such as:

- `terminal create --name startup-shell --headless --cols 120 --rows 30 -- pwsh.exe -File start.ps1`
- `terminal start startup-shell`
- the terminal begins running immediately with no attached window
- the terminal remains queryable through CLI commands
- a later `window create --terminal <id>` can attach a live window to inspect the running session

Today, `terminal create` only persists the definition. Until detached runtime ownership exists, `terminal start <id>` and `terminal attach <id>` may exist as fail-fast command surfaces, while `terminal show-window <id>` should be treated only as a convenience launcher for opening a fresh window from the saved terminal definition.

This is conceptually similar to starting a virtual machine or container in detached mode and attaching later.

If Teamy Studio later gains a tray icon or startup-service behavior, these detached terminals are the natural runtime unit to hang that behavior from.

### Control plane responsibilities

The control plane should own:

- creating resources
- looking them up by id or exact name
- sending commands to them
- querying their current state
- persisting or exporting artifacts

The current app process can initially own both the runtime and the command handling. It does not need a full external daemon architecture on day one.

The important constraint is that Teamy Studio should avoid inventing a second parallel action hierarchy behind the CLI. The typed CLI commands themselves should be treated as the canonical action model, even when the request stays in-process.

## CLI Direction

The CLI should gradually grow a terminal-focused command group that mirrors the resource model.

Illustrative commands:

```text
teamy-studio.exe terminal create --name prompt-repro --cols 120 --rows 30 -- pwsh.exe -NoProfile
teamy-studio.exe terminal list
teamy-studio.exe terminal show prompt-repro
teamy-studio.exe window create --terminal prompt-repro --bare
teamy-studio.exe terminal send-text prompt-repro "function global:prompt { ... }"
teamy-studio.exe terminal send-keys prompt-repro ctrl+l
teamy-studio.exe terminal send-input-events prompt-repro keydown:ctrl keydown:l keyup:l keyup:ctrl
teamy-studio.exe terminal snapshot visible-text prompt-repro --out visible.txt
teamy-studio.exe terminal snapshot scrollback prompt-repro --out scrollback.txt
teamy-studio.exe window snapshot image --window-id <id> --out before.png
teamy-studio.exe terminal close prompt-repro
```

This would sit alongside the existing `window`, `workspace`, `shell`, and `self-test` command groups.

The point is not to replace self-tests immediately. The point is to make self-tests a thin composition layer over reusable terminal commands.

For the MVP, the most valuable command categories are:

- headless terminal lifecycle
- window attachment
- simple typewriter-style text input
- higher-level key chord input
- raw keydown and keyup injection for exact event ordering
- visible-content capture
- scrollback capture
- rendered image capture

An illustrative detached flow is:

```text
teamy-studio.exe terminal create --name startup-shell --headless --cols 120 --rows 30 -- pwsh.exe -File start.ps1
teamy-studio.exe terminal snapshot visible-text startup-shell --out startup-visible.txt
teamy-studio.exe window create --terminal startup-shell --bare
```

## CLI Invocation As The Action Model

The CLI invocation path should be the primary action model rather than a serialization layer wrapped around some separate internal command enum.

In practice, that means a CLI command's `invoke` implementation should do three things:

1. resolve or acquire the target resource such as a window or terminal
2. decide whether the target is owned by the current process or a foreign process
3. either execute locally or forward the same typed command to the owning process

Conceptually:

```text
match acquire_window(window_id) {
	Local(window) => args.invoke_local(window),
	Foreign(owner) => owner.forward_cli(args),
}
```

The important part is that `args` stays the same in both branches. We do not want one hierarchy of CLI commands and a second hierarchy of internal actions that must be kept in sync.

What may still be worth separating is the already-acquired execution helper, for example `invoke_local(window)` or `run_with_window(window)`, but that is an execution seam, not a second action model.

## Why This Helps The Current Scroll And Zoom Bugs

The current prompt-loss bug is a good example of why a control plane helps.

We want to be able to express a repro as a sequence of stable operations:

1. create a terminal with `pwsh.exe -NoProfile` and an explicit logical size
2. attach a bare window if the repro needs visible rendering
3. override the prompt with a deterministic multiline prompt
4. snapshot the visible text, scrollback, and image state
5. apply a zoom out operation
6. apply a zoom in operation
7. snapshot the visible text, scrollback, and image state again
8. assert that the final visible state matches the initial one and inspect scrollback separately when needed

If those steps are exposed as composable commands, then the same repro can be used in:

- a cargo integration test
- a manual bug-repro script
- a future notebook cell action
- a CI artifact capture job

That is much better than burying the behavior in one bespoke self-test function.

## Modular Window Composition

The terminal should be separable from the rest of the current window chrome.

That does not mean the sidecar and output panel are mistakes. It means they should become optional views rather than unavoidable parts of every terminal repro.

The target decomposition is:

- a terminal surface that can exist alone
- a sidecar or title surface that can be attached when wanted
- an output surface that can be attached when wanted
- a workspace composition layer that arranges those surfaces into a notebook-like cell layout

This creates a simpler debug mode and a richer product architecture at the same time.

For example:

- `terminal create --bare` could launch just the terminal surface with no sidecar and no output panel
- `output create --for <terminal>` could attach a separate output window to a terminal session
- `window compose cell --terminal <id> --output <id>` could produce the current notebook-like presentation as a composition of reusable surfaces

## Output Surface Direction

The output panel should eventually become its own view model and window contract.

That would allow commands such as:

- set output text
- show an image artifact
- clear output
- hide or show output window
- move output window relative to a terminal window

This is useful both for notebook workflows and for diagnostics.

A terminal repro could run with no output surface at all. A notebook cell could run with a paired output surface. A future artifact browser could reuse the same output view independently.

## Testing Strategy

The testing hierarchy should become more explicit.

### 1. Engine tests

These validate terminal state transitions in pure Rust.

Examples:

- scrollback restoration across resize
- cursor visibility and alternate screen behavior
- SGR style preservation

These should remain the fastest and most deterministic tests.

### 2. Control-plane terminal tests

These validate that the terminal resource responds correctly to command sequences.

Examples:

- create a `pwsh -NoProfile` terminal
- send text
- resize or zoom
- inspect visible cells
- assert viewport or cursor state

These should replace much of today's bespoke self-test logic over time.

### 3. Offscreen render tests

These validate that a terminal state renders correctly into an image without opening a visible window.

Examples:

- compare a before and after snapshot for pixel identity
- diff screenshots when a repro changes rendering
- verify scrollbar and cursor painting

This is the right layer for catching "model is correct but screen paint is wrong" failures.

### 4. Full app smoke tests

These validate end-to-end UI integration for the composed notebook window.

Examples:

- ctrl+wheel over the terminal surface changes only terminal zoom
- ctrl+wheel over the output surface changes only output zoom
- sidecar hit-testing does not interfere with terminal selection or scroll behavior

These will be slower and more brittle, so they should be used sparingly.

## IPC Direction

Named pipes are a reasonable first implementation on Windows.

The requirements are modest:

- a stable runtime registry of sessions and views
- a way to address a resource by id or exact name
- request-response commands for queries and mutations
- artifact streaming or file-backed export for snapshots and transcripts

The transport matters less than the command model.

The most useful acquisition shape is something like:

- `acquire_terminal(id_or_name) -> Local(handle) | Foreign(owner)`
- `acquire_window(id_or_name) -> Local(handle) | Foreign(owner)`

If the target is local, the current process executes the request directly.

If the target is foreign, the current process forwards the same typed CLI command to the owning process, which then re-enters the same `invoke` path and succeeds on the local branch.

For the MVP, a headless terminal should still have an explicit logical size. If Teamy Studio allows `terminal create` before `window create`, then `terminal create` should require either:

- `--cols` and `--rows`, or
- some other explicit logical size profile

Without that, the PTY-backed session does not have a well-defined grid before a window exists.

The first version can be intentionally simple:

- one broker endpoint for Teamy Studio instances
- one session registry under the application cache or runtime directory
- JSON request and response payloads

If later we need a background daemon, the command contracts can remain the same while the process topology changes.

## State And Artifact Model

Each terminal should expose a queryable state model.

Useful examples:

- visible text
- visible cells with positions and styles
- viewport metrics
- cursor state
- selection state
- current layout profile
- attached view resources
- capture mode metadata

Artifacts should be first-class outputs of commands.

Useful examples:

- text transcript
- JSON state snapshot
- PNG render snapshot
- diff image against another snapshot

The model should also distinguish between the terminal's always-needed in-memory state and optional retained diagnostic history.

The terminal always needs in-memory screen state, scrollback, cursor state, and other runtime data to function.

What should be optional is extra retained event history such as:

- raw input byte streams
- raw PTY output byte streams
- explicit input-event histories

Those histories should be retained for later export only when the terminal is created in an explicit capture mode.

That keeps the default runtime lean while still allowing high-fidelity diagnostics when the user asks for them.

Good direction for the MVP:

- default mode retains only the normal runtime terminal state
- trace logging can still expose detailed event observations when the app is run with an appropriate log filter
- explicit capture mode is required before commands may export retained raw input or output streams

This gives us a uniform way to support debugging, testing, and notebook-style persisted results.

## Suggested Incremental Plan

The next steps should stay small and should improve the current scroll and zoom debugging story immediately.

### Phase 1: Extend the CLI so it can acquire and forward targets

Teach terminal and window CLI commands how to:

- resolve ids and names
- detect whether the target is owned by the current process
- run locally when the target is local
- forward the same typed command when the target is foreign

This keeps the typed CLI structs as the action model while still allowing cheap local execution.

The first commands worth implementing are:

- `terminal create`
- `window create --terminal <id>`
- `terminal send-text`
- `terminal send-keys`
- `terminal send-input-events`
- `terminal snapshot visible-text`
- `terminal snapshot scrollback`
- `window snapshot image`

Useful follow-on flags for `terminal create` are:

- `--headless`
- `--capture-mode none|trace-assisted|raw-streams`

### Phase 2: Add a bare terminal view mode

Allow the window layer to create a terminal-focused surface without the output panel or sidecar.

This should make scroll, resize, and zoom bugs easier to reproduce and should reduce ambiguity about whether the decorations are involved.

### Phase 3: Rebuild self-tests on top of CLI-addressable terminal actions

Replace bespoke scripted self-test helpers with compositions of the new command surface.

The goal is that every self-test transcript corresponds to a reusable sequence of terminal commands.

The first high-value repro should be the current zoom-related prompt-loss case using:

- `pwsh.exe -NoProfile`
- a deterministic multiline prompt
- visible-text snapshot
- scrollback snapshot
- rendered image snapshot

### Phase 4: Add the `terminal` and richer `window` subcommands

Expose the stable terminal command surface under explicit CLI command groups.

At that point, manual repros, CI automation, command palette actions, and product features can all rely on the same surface.

### Phase 5: Split output and chrome into separate composable views

Once the terminal control plane is stable, move the output surface and decorative sidecar into optional, separately managed views.

That will support both a cleaner debug mode and the broader notebook-style product direction.

## Constraints

- formal specs should only be updated when observable behavior actually ships
- tests should prefer headless and deterministic verification where possible
- control-plane commands should not become test-only escape hatches that bypass real product behavior
- a future daemon is optional; a stable command model is required
- the current app must remain runnable while these seams are introduced gradually

## Immediate Payoff

If this direction is followed, the next zoom bug investigation should be able to answer the right questions quickly:

- is the terminal engine state wrong?
- is the app-level zoom transaction wrong?
- is the renderer painting the wrong pixels?
- is the composed notebook chrome interfering with a terminal-only repro?

That is the practical reason to do this work now.

The longer-term benefit is that the same architecture is a plausible foundation for Teamy Studio becoming a more capable notebook application rather than remaining a single custom terminal window.
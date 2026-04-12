# Voice Note: Scroll Bugs, Control Plane, And Future Direction

This note restructures the scroll and zoom design thoughts from voice dictation into a clearer architecture proposal.

It extends the direction in [docs/notes/terminal-control-plane.md](g:\Programming\Repos\Teamy-Studio\docs\notes\terminal-control-plane.md) with two more emphases:

- input recording and replay as a first-class feature
- a strict distinction between terminal identity and window identity

## Why This Matters Now

The immediate trigger is still practical.

When a terminal bug affects what the user sees on screen, the highest-confidence test is not just checking the scrollback buffer or the terminal model. It is verifying the visible content that is actually rendered for the user.

That means the architecture should make it easy to:

- create a bare terminal surface with no unrelated decorations
- send a controlled sequence of actions to it
- snapshot text state, cell state, and rendered pixels
- compare before and after results after operations like zoom out and zoom in

If zooming out and back in changes nothing, a pixel-diff should be empty. If part of the prompt disappears, the diff should make that obvious.

## Core Direction

Teamy Studio should treat terminal manipulation as a public command surface rather than a set of special internal testing hooks.

That command surface should be reusable across:

- self-tests
- command palette actions
- future notebook features
- external automation
- debugging and repro capture

The key point is that tests should not have private powers that normal product code cannot use.

## Terminal Actions As First-Class Commands

The terminal should expose a discrete action space, but that action space should be parameterized rather than exploded into one giant enum variant per possible key or gesture.

Examples of the right level of abstraction:

- create terminal
- close terminal
- send text
- send key
- send mouse input
- resize terminal
- zoom window
- scroll viewport
- query visible text
- query visible cells
- capture text snapshot
- capture image snapshot
- record input sequence
- replay input sequence

Examples of the wrong level of abstraction:

- one enum variant for every letter key
- one enum variant for every zoom level
- one enum variant for every possible ctrl+wheel combination
- send key sequence
- send raw keydown and keyup events
The parameterized command model is important because it keeps the command palette, CLI, and IPC contracts tractable.

## Terminal ID Versus Window ID

The architecture should explicitly separate terminal identity from window identity.
- query visible cells
- query scrollback buffer

That means:

- a terminal session has its own stable terminal id
- a window has its own stable window id
- in the long term, multiple windows may point at the same terminal
- for the MVP, each terminal should have at most one live attached window
- different windows attached to the same terminal are therefore a future design question, not an MVP requirement

This distinction matters because some actions target the terminal and some target the window.

Terminal-targeted actions:

- send key sequence
- send text
- send mouse event into terminal input mode
- query scrollback or visible cells
- capture transcript

Window-targeted actions:

- zoom in or out
- move window
- resize window
- show or hide sidecar
- show or hide output panel
- bind window to a terminal

The reason to keep terminal id and window id separate even in the MVP is that terminal-targeted commands and window-targeted commands are still different categories of action.

But the MVP should avoid pretending that one terminal can trivially support two windows with independent zoom levels, because the zoom level affects the logical grid dimensions rather than being purely cosmetic.

## Window Binding Model

Creating a window should not necessarily imply creating a new terminal.

Useful model:

- a new window may start unbound
- a window may bind to an existing terminal id
- a window may request that a new terminal be created and bound immediately
- for the MVP, binding a second live window to an already-bound terminal should fail cleanly

This lets Teamy Studio support:

- a bare repro window for debugging
- multiple views into the same terminal
- a future notebook cell that binds an output view and a terminal view together

This also implies that `window create` and `terminal create` should not be the same command, even if one may call the other for convenience.

If Teamy Studio allows headless terminal creation before a window exists, that terminal still needs an explicit logical size. So `terminal create` should likely require `--cols` and `--rows` or an equivalent profile in the MVP.

## Command Palette As A Frontend Over The Same Public Interface

The command palette should not invent a second private action system.

Instead, the command palette should be a frontend over the same public command model that the CLI and automation surface use.

That means:

- focused window context can prefill the relevant window id
- focused terminal context can prefill the relevant terminal id
- commands shown in the palette are filtered by what is valid for the focused object
- invoking a command from the palette should be semantically equivalent to invoking the corresponding CLI or IPC command

This also means the app should feel free to optimize later by short-circuiting in-process dispatch, but the externally visible semantics should remain the same as if a real command had been issued.

## Prefer The Raw Path First, Optimize Later

At the beginning, it is acceptable and even desirable for the public path to be very literal.

For example, a command palette action that invokes a terminal command may visibly spawn a Teamy-controlled command terminal to run that command and show its result.

That is slower, but it provides three advantages:

- it forces the product to rely on a real public interface
- it makes behavior easier to debug because the raw path is visible
- it delays premature optimization until the model is proven useful

Later, Teamy Studio can short-circuit the implementation while preserving the same externally visible behavior.

## Input Recording, Serialization, And Replay

Input automation should be treated as three separate concerns.

### 1. Recording

Recording is the act of observing a sequence of user actions such as:

- key presses and releases
- text input
- mouse input
- timing between events

This could begin with a simple capture window and a paired history view that displays the recorded events as they arrive.

The existing ratatui key-debug tool in [src/main.rs](g:\Programming\Repos\ratatui-key-debug\src\main.rs) is already a rough precursor to this idea.

### 2. Serialization

Serialization is the curated saved format for those recordings.

This should not be the same thing as raw tracing logs.

The current NDJSON logging and trace data are still valuable for diagnostics, but they should be treated as observability output, not as the stable recording format for replayable scenarios.

The saved recording format should be:

- focused on the actions needed for replay
- explicit about timing and targets
- stable enough for tests and stored fixtures

### 3. Replay

Replay is the act of taking the curated recording and playing it back against a target terminal or target window.

That target may be:

- a Teamy terminal
- a Teamy window
- eventually, a more general Windows automation surface in the style of AutoHotkey-class tooling

For now, Teamy-specific replay is the most practical starting point.

The MVP command surface should prioritize three levels of input injection:

- `send-text` for simple typewriter-style automation
- `send-keys` for higher-level chords such as `ctrl+c`
- `send-input-events` for explicit keydown and keyup control when exact event ordering matters

The same surface should also support detached headless terminals that begin running before any window is attached.

That is useful for:

- startup scripts
- background shells
- long-running setup tasks that the user may want to inspect later

The intended flow is:

- create the terminal in headless mode with an explicit logical size
- let it run in the background
- attach a window later if inspection is needed

## Logging Versus Curated Recording

The existing trace logging is still useful.

If Teamy Studio runs with a log filter that emits detailed event traces into NDJSON, that can help with:

- diagnosing regressions
- extracting candidate sequences for future recordings
- understanding what raw events actually occurred

But the trace stream should remain distinct from the curated replay format.

Good rule:

- tracing is for observation
- recordings are for reproduction

The two can be related without being identical.

The same distinction applies to raw terminal event streams.

Teamy Studio does not need to persist every raw key and PTY byte by default just because those events exist at runtime.

The terminal already has to maintain its in-memory state to function. That does not imply it should also retain a full exportable raw-event history for every session.

So the intended policy should be:

- normal mode keeps only the runtime terminal state needed for execution and display
- trace logging remains the default way to observe detailed event flow when diagnostics are needed
- explicit capture mode is required before the user may export retained raw input or output streams later

That means the "most verbose" path is opt-in rather than always-on.

## Snapshotting Priority

For the current class of bugs, screenshot and rendered-surface capture are the highest-confidence verification path.

Why:

- the scrollback buffer may be correct while the visible presentation is wrong
- the terminal model may be correct while the renderer is wrong
- the visible cells may be correct while the final composited window is wrong

That suggests a layered verification approach.

Lowest disruption:

- headless buffer and terminal-model checks

Higher confidence:

- offscreen rendered image snapshot

Highest confidence:

- actual window snapshot or pixel-perfect comparison of the visible composed surface

The current scroll and zoom bug investigation should bias toward the rendered snapshot path when possible.

## Modular Surfaces

The current window mixes several concerns that would be easier to reason about if they were independently created and attached.

Candidate surfaces:

- terminal surface
- output surface
- sidecar or title surface
- command palette surface
- recording-history surface

This creates a cleaner debugging workflow.

Examples:

- spawn a bare terminal for a repro
- spawn a recording canvas plus an event-history window
- spawn a terminal plus output window for notebook-style work
- omit the output surface entirely when isolating a rendering bug

## CLI Direction

The CLI should become the canonical public interface for these manipulations.

The key implementation preference is that the CLI should not just serialize requests into some separate internal action model. The typed CLI commands themselves should be the action model.

That means each command's `invoke` path should:

1. acquire the addressed window or terminal
2. detect whether the owner is local or foreign
3. run locally when local
4. forward the same typed command when foreign

This keeps Teamy Studio from having to maintain two parallel command hierarchies.

Illustrative direction:

```text
teamy-studio.exe terminal create --name pwsh-repro --cols 120 --rows 30 -- pwsh.exe -NoProfile
teamy-studio.exe window create --bind-terminal pwsh-repro --bare
teamy-studio.exe window zoom --window-id <id> --delta -1
teamy-studio.exe window zoom --window-id <id> --delta 1
teamy-studio.exe terminal send-keys --terminal-id <id> ctrl+c
teamy-studio.exe terminal send-input-events --terminal-id <id> keydown:ctrl keydown:c keyup:c keyup:ctrl
teamy-studio.exe terminal send-text --terminal-id <id> "hello"
teamy-studio.exe terminal snapshot visible-text --terminal-id <id> --out before.txt
teamy-studio.exe terminal snapshot scrollback --terminal-id <id> --out scrollback.txt
teamy-studio.exe window snapshot image --window-id <id> --out before.png
teamy-studio.exe input-record start --window-id <id>
teamy-studio.exe input-record stop --window-id <id> --out scenario.teamyinput
teamy-studio.exe input-replay run --window-id <id> --file scenario.teamyinput
teamy-studio.exe terminal create --name startup-shell --headless --cols 120 --rows 30 -- pwsh.exe -File start.ps1
teamy-studio.exe window create --bind-terminal startup-shell --bare
```

This does not need to be implemented all at once. The main point is that the public interface should make terminal and window targets explicit.

The practical pattern is:

```text
match acquire_window(window_id) {
	Local(window) => args.invoke_local(window),
	Foreign(owner) => owner.forward_cli(args),
}
```

The same idea applies to terminal-targeted commands.

## IPC Direction

Named pipes are a reasonable Windows-first starting point.

What matters more than the transport:

- clear resource ids
- clear distinction between terminal-targeted and window-targeted commands
- query and mutation symmetry
- stable payloads that the CLI, command palette, and tests all share

The first version can be intentionally plain.

## Immediate Product And Testing Value

This direction is not just future-looking product work. It directly improves how Teamy Studio can debug the current class of failures.

With the right seams, a repro for prompt disappearance can be expressed as:

1. create a `pwsh.exe -NoProfile` terminal with an explicit logical size
2. create a bare window bound to that terminal
3. inject a deterministic multiline prompt
4. capture the visible content, scrollback, and rendered image
5. zoom out and zoom in by window id
6. capture the visible content, scrollback, and rendered image again
7. assert that the visible content and rendered image match the original, and inspect scrollback separately when needed

That is the kind of public, composable repro surface Teamy Studio should be aiming for.

## Suggested Incremental Next Steps

### 1. CLI-first acquisition and forwarding

Make the typed CLI commands the canonical action model, and add `acquire_window` and `acquire_terminal` seams that choose local execution versus forwarding to the owning process.

### 2. Bare window mode

Allow a terminal window to be created with no output panel and no sidecar so rendering and zoom bugs can be isolated.

The same phase should preserve detached headless terminals as a first-class mode rather than treating headless execution as test-only behavior.

### 3. Window snapshot command

Add a snapshot command that captures the visible rendered content for a specific window id.

In parallel, add separate terminal queries for:

- visible content
- visible cells if needed
- scrollback buffer

### 4. Curated input recording format

Design a small recording format for keys, text, mouse input, and timing rather than overloading the trace log format.

Raw retained event-stream export should remain gated behind explicit capture mode rather than becoming the default for every terminal.

### 5. Command palette integration

Make the command palette a frontend over the same public action surface rather than a separate private mechanism.

## Bottom Line

The most important architectural move is to make Teamy Studio use its own public interface.

If terminal and window actions are real commands with real ids, then:

- tests become more composable
- debugging becomes more honest
- command palette behavior becomes easier to reason about
- future notebook-style composition becomes much more plausible

That is a better path than growing the current self-test harness into a second hidden control system.
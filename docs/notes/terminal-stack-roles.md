# Terminal Stack Roles And Responsibilities

This note explains the main components involved in Teamy-Studio Studio's terminal stack on Windows, what each one is responsible for, and how they fit together.

## High-level picture

At a high level, Teamy-Studio Studio is not itself a shell and it is not itself a full terminal host in the Windows Console sense.

It is better described as:

- a native Win32 desktop application
- that creates a window
- spawns a child process inside a PTY
- reads the child's terminal output stream
- interprets that stream using Ghostty terminal logic
- renders the interpreted result into the Teamy-Studio window
- translates keyboard input from Win32 messages back into bytes or structured key events for the PTY child

## Main layers

### Teamy-Studio Studio application

Teamy-Studio Studio is the overall orchestrator.

Its responsibilities are:

- own the app lifecycle
- parse CLI arguments
- initialize logging and startup behavior
- create the native window
- create and own the terminal session
- connect keyboard and paint events to the terminal layer

It does not implement the PTY itself and it does not implement the terminal parser from scratch.

## Native window and rendering host

### Win32 window

The Win32 window is the actual desktop surface the user sees.

Its responsibilities are:

- register a window class
- create the native window
- receive Windows messages such as:
  - `WM_PAINT`
  - `WM_SIZE`
  - `WM_KEYDOWN`
  - `WM_KEYUP`
  - `WM_CHAR`
  - `WM_SYSKEYDOWN`
  - `WM_SYSKEYUP`
- manage basic interaction like dragging and close behavior

This is the boundary between the operating system's GUI event model and Teamy-Studio's internal terminal model.

### GDI paint path

Today, Teamy-Studio renders through Win32 GDI rather than DirectX.

Its responsibilities are:

- allocate temporary drawing surfaces
- paint the current terminal snapshot into a device context
- copy the finished frame into the real window

This matters because it means Teamy-Studio is currently:

- a Win32 desktop app
- using GDI for presentation
- not a DirectX-rendered terminal yet

### “DirectX window”

This is worth stating carefully because the phrase can mean two different things.

In Teamy-Studio's history, the `DirectX-Learning` repo influenced the window-startup approach and native Windows application structure.

But in the current Teamy-Studio codebase, the terminal window itself is not a DirectX renderer. The actual responsibilities normally implied by a “DirectX window” would be:

- GPU-backed swap-chain presentation
- Direct2D or DirectWrite text rendering
- lower-level graphics resource management

Teamy-Studio is not doing that yet. Right now the window host is Win32, and the presentation path is GDI.

## PTY and console-host side

### portable-pty

`portable-pty` is the Rust crate Teamy-Studio uses to abstract PTY creation and child-process spawning.

Its responsibilities are:

- provide a cross-platform PTY API
- create the PTY pair
- spawn the child process into that PTY
- expose a reader for child output
- expose a writer for child input
- hide most of the platform-specific PTY details from Teamy-Studio

In Teamy-Studio, this means `portable-pty` is the boundary between:

- Teamy-Studio's Rust application code
- and the Windows-specific ConPTY machinery underneath

Teamy-Studio does not call the raw ConPTY APIs directly in its own main terminal logic. It relies on `portable-pty` for that layer.

### ConPTY

ConPTY is the Windows pseudo-terminal facility.

Its responsibilities are:

- host a console-style child process in a pseudoterminal environment
- expose pipes for input and output
- convert between Windows console semantics and VT-style terminal streams
- support features like resize and Win32 input mode

Conceptually, ConPTY is the OS-level console bridge that makes a classic console app look like something a GUI terminal can host.

### conpty.dll

`conpty.dll` is the concrete implementation Teamy-Studio can sideload to get a newer console host behavior.

Its responsibilities are:

- provide the ConPTY implementation that `portable-pty` loads on Windows
- determine host-side keyboard and console semantics for the PTY child
- behave more like modern OpenConsole behavior than the older built-in path on some systems

This component was central to the keyboard-input fix because Teamy-Studio discovered that the host behavior itself affected the exact `cmd.exe -> ratatui_key_debug.exe` repro.

Important detail:

- the `portable-pty` loader Teamy-Studio uses prefers a sideloaded `conpty.dll` next to the executable when present

That is why staging `conpty.dll` into the Cargo output directory changed runtime behavior without rewriting the PTY stack.

### OpenConsole.exe

`OpenConsole.exe` is the host executable built from the Microsoft Terminal repository.

Its responsibilities are:

- act as the newer console host paired with the newer ConPTY implementation
- provide the host-side behavior Teamy-Studio wanted to match more closely
- supply the runtime semantics that Windows Terminal uses under the hood

In practical terms for Teamy-Studio:

- `OpenConsole.exe` plus `conpty.dll` are the locally built host artifacts
- `build-openconsole.cmd` builds them
- `build.rs` stages them next to the Teamy-Studio binary when available

### Windows Terminal

Windows Terminal is not embedded inside Teamy-Studio and Teamy-Studio is not reusing the Windows Terminal UI.

Its role in this story is twofold:

- it is the behavioral reference Teamy-Studio was trying to match
- its source repo provides the newer OpenConsole/ConPTY host Teamy-Studio used for testing and local staging

So Windows Terminal's responsibilities here are mostly indirect:

- define the “known good” user-visible behavior
- provide the upstream OpenConsole implementation that Teamy-Studio can build locally

## Child processes inside the PTY

### cmd.exe

`cmd.exe` is usually the default shell child Teamy-Studio launches inside the PTY.

Its responsibilities are:

- act as the console program Teamy-Studio hosts by default
- receive keyboard input routed through the PTY
- launch additional child apps such as `ratatui_key_debug.exe`

For debugging, `cmd.exe` was important because the failing live repro specifically went through:

- Teamy-Studio
- then `cmd.exe`
- then `ratatui_key_debug.exe`

### Other PTY children

Any console-mode child process Teamy-Studio launches inside the PTY inherits the consequences of the layers above it:

- the PTY host behavior
- Teamy-Studio's keyboard translation behavior
- the terminal-emulation behavior on the rendering side

That is why `crossterm` probes, raw Windows probes, and `ratatui` could all surface different aspects of the same underlying problem.

## Terminal emulation side

### libghostty-rs

`libghostty-rs` is the Rust binding layer Teamy-Studio depends on.

Its responsibilities are:

- expose Ghostty terminal-emulation functionality to Rust
- provide Rust-friendly APIs for terminal state, rendering snapshots, key encoding, and styles
- let Teamy-Studio integrate Ghostty logic without writing Zig or C FFI directly in the app code

In Teamy-Studio's dependency tree, the specific crate in use is `libghostty-vt` from the `libghostty-rs` repository.

### libghostty-vt

`libghostty-vt` is the specific Rust crate Teamy-Studio uses from that repo.

Its responsibilities are:

- maintain terminal state
- parse VT sequences from the PTY output stream
- track cells, rows, cursor state, styles, and dirty regions
- expose key encoding logic used when Teamy-Studio needs to emit terminal key sequences
- provide render snapshots that Teamy-Studio can paint into its own window

In practice, this is the core terminal-emulation engine Teamy-Studio is relying on.

### libghostty (Zig)

The Ghostty project itself, implemented in Zig, is the deeper upstream terminal implementation behind the Rust bindings.

Its responsibilities are:

- define the actual terminal semantics and parser behavior
- implement screen state, rendering-oriented iteration, and keyboard protocol behavior
- serve as the upstream source of truth for the terminal engine Teamy-Studio is embedding

So the layering is:

- `libghostty` in Zig is the upstream engine
- `libghostty-rs` exposes Rust bindings/wrappers around that engine
- Teamy-Studio uses those Rust-facing APIs through `libghostty-vt`

## Teamy-Studio terminal-session layer

### TerminalSession

`TerminalSession` is Teamy-Studio's own integration layer around the PTY and Ghostty terminal engine.

Its responsibilities are:

- create the PTY-backed child session
- read bytes coming from the PTY child
- feed those bytes into the Ghostty terminal state
- track repaint needs and terminal size
- translate Win32 keyboard input into the right outgoing terminal input form
- bridge Teamy-Studio's window/events layer to the PTY and terminal-emulation layers

This is the most important Teamy-Studio-owned abstraction in the stack.

If `portable-pty` is the PTY bridge and `libghostty-vt` is the emulator, `TerminalSession` is the glue that combines them into a usable app runtime.

## Input translation and keyboard semantics

### Win32 keyboard messages

Windows delivers GUI keyboard input as native messages such as:

- `WM_KEYDOWN`
- `WM_KEYUP`
- `WM_CHAR`
- `WM_SYSKEYDOWN`
- `WM_SYSKEYUP`

Their responsibilities are:

- represent what the OS believes happened at the GUI-window level
- carry virtual key, scan code, repeat, and char information

These messages are not automatically the same thing as the bytes or key events a PTY child expects. Teamy-Studio has to translate them.

### Teamy-Studio's key-event routing

Teamy-Studio's responsibilities in this layer are:

- decide whether an input should become text bytes, encoded key sequences, or Win32 input mode events
- preserve modifier state correctly
- suppress duplicate legacy `WM_CHAR` events after handled keydown paths
- adapt behavior based on whether kitty keyboard flags or ConPTY Win32 input mode are active

This is the exact layer that had to be improved for the Windows keyboard bug.

## Testing and diagnosis helpers

### windows_key_probe

This is a diagnostic helper binary.

Its responsibilities are:

- inspect low-level Windows console key events
- verify what the host and ConPTY path are actually producing

### crossterm_key_probe

This is a higher-level diagnostic helper binary.

Its responsibilities are:

- inspect how `crossterm` interprets the resulting input stream
- catch regressions visible to common Rust terminal applications

### windows_terminal_self_test

This is Teamy-Studio's reproducible self-test harness.

Its responsibilities are:

- drive the terminal session from outside or inside
- reproduce the exact `cmd.exe -> ratatui_key_debug.exe` failure path
- validate that live keyboard behavior matches expectations

### keyboard_input_regressions.rs

This is the focused regression-test suite.

Its responsibilities are:

- lock in the reduced probe behavior
- lock in the crossterm-visible behavior
- lock in the real default-shell live repro

## Responsibility summary by component

### portable-pty

- create and manage the PTY abstraction
- spawn PTY child processes
- expose PTY reader and writer handles
- hide raw Windows ConPTY setup behind a Rust API

### conpty.dll

- implement the host-side ConPTY behavior Teamy-Studio runs against
- determine important host semantics such as keyboard behavior
- provide a newer sideloadable ConPTY implementation when staged beside Teamy-Studio

### Windows Terminal

- provide the reference behavior Teamy-Studio compares against
- provide the upstream OpenConsole implementation Teamy-Studio builds locally
- not serve as Teamy-Studio's UI or embedded renderer

### OpenConsole.exe

- act as the newer console host paired with the newer ConPTY path
- improve PTY-host behavior relative to the older default path

### libghostty-rs

- expose Ghostty terminal functionality to Rust
- make the upstream terminal engine usable from Teamy-Studio

### libghostty-vt

- provide terminal parsing, screen state, key encoding, and render snapshots
- act as the terminal-emulation engine Teamy-Studio directly uses

### libghostty (Zig)

- implement the upstream terminal engine and semantics
- define the deep behavior the Rust bindings expose

### directx window

- historically relevant as inspiration from earlier native Windows work
- not the actual current Teamy-Studio rendering path
- Teamy-Studio currently uses Win32 plus GDI rather than DirectX presentation

### Win32 window layer

- own the native app window and message loop
- deliver paint, resize, and keyboard messages into Teamy-Studio

### GDI paint layer

- draw the current terminal snapshot into the window
- perform CPU-side Win32 presentation today

### TerminalSession

- glue PTY I/O, terminal emulation, repaint tracking, and keyboard translation together
- serve as the core Teamy-Studio-owned runtime abstraction for the terminal window

### cmd.exe and other PTY children

- act as the programs being hosted
- consume the translated input Teamy-Studio sends through the PTY
- produce the VT output Teamy-Studio displays

## Bottom line

The easiest way to think about the stack is:

1. Win32 owns the native window and input events.
2. Teamy-Studio translates those events and manages the session.
3. `portable-pty` connects Teamy-Studio to Windows ConPTY.
4. `conpty.dll` and `OpenConsole.exe` define the host-side console behavior.
5. `cmd.exe` or another child process runs inside that PTY.
6. `libghostty-vt` interprets the child's terminal output.
7. Teamy-Studio paints the resulting terminal state into its Win32 window using GDI.

That division of responsibility is why the keyboard bug turned out to be split across multiple layers rather than belonging to only one library.
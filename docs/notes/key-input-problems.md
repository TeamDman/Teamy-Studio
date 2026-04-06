This note captures the Windows keyboard-input problems that showed up in Teamy Studio, why they were hard to pin down, and what finally fixed them.

## Problem statement

The real bug was not just that a synthetic probe looked wrong. The problem was visible in live usage:

- run `cargo run`
- let Teamy launch `cmd.exe`
- from inside that hosted shell, run `ratatui_key_debug.exe`

In that exact path, keyboard handling diverged from Windows Terminal in a couple of important ways:

- some key presses appeared to downstream apps as if release events arrived too early
- `Ctrl+Backspace` could show up as duplicated input
- `Ctrl+Backspace` could lose the `Ctrl` modifier and degrade into older console behavior
- Windows sometimes produced `WM_CHAR` with code unit `127` (`DEL`) instead of only `8` (`Backspace`)

The result was that terminal apps which care about precise keyboard semantics, especially `ratatui_key_debug`, behaved incorrectly inside Teamy even though they behaved correctly in Windows Terminal.

## Why the bug was confusing

There were two overlapping causes.

### 1. Teamy had gaps in its Win32 input translation

The original path mostly treated:

- `WM_CHAR` as the text path
- `WM_KEYDOWN` as a partial special-key path
- key release handling and modifier tracking as secondary details

That worked for simpler cases but was not accurate enough for the Win32 console edge cases exercised by `cmd.exe`, `crossterm`, and `ratatui`.

### 2. The console host itself also mattered

Even after Teamy-side fixes, the exact live `cmd.exe -> ratatui_key_debug.exe` path still differed from Windows Terminal. That pointed to host-side behavior rather than only Teamy-side event routing.

The important discovery was that Teamy uses `portable-pty` on Windows, and its ConPTY loader already prefers a sideloaded `conpty.dll` next to the executable. That made it possible to test with a newer OpenConsole host from the Microsoft Terminal repo without rewriting the PTY stack.

## What was changed

### Win32 message handling was unified

Teamy now routes:

- `WM_KEYDOWN`
- `WM_SYSKEYDOWN`
- `WM_KEYUP`
- `WM_SYSKEYUP`
- `WM_CHAR`

through a more precise terminal-input layer.

The important change is that `WM_CHAR` now carries the original `lparam` into the terminal session so scan code and repeat-count information stays available when deciding how to encode the input.

### Terminal input translation became mode-aware

The terminal session now distinguishes more clearly between:

- legacy text-oriented input paths
- kitty keyboard protocol paths
- ConPTY Win32 input mode

That work included:

- richer virtual-key mapping using scan codes and extended-key state
- explicit key press and key release handling
- improved modifier tracking
- delayed routing of some characters until the corresponding key event is known
- suppression of duplicate legacy `WM_CHAR` messages when a key event already handled the same logical input

The Backspace fix was especially important. In live testing, Windows could emit `WM_CHAR` as `0x7F` rather than `0x08`, so Teamy now suppresses both variants where appropriate instead of assuming only classic Backspace semantics.

### The newer OpenConsole host is staged automatically

The decisive behavior change came from using a newer OpenConsole/ConPTY host.

`build.rs` now stages these binaries into the active Cargo profile directory when available:

- `OpenConsole.exe`
- `conpty.dll`

The lookup order is:

1. `TEAMY_OPENCONSOLE_BUILD_DIR`
2. sibling fallback `../microsoft-terminal/bin/x64/Release`

Because `portable-pty` prefers a sideloaded `conpty.dll`, local `cargo run` and focused tests automatically pick up the newer host once those binaries exist.

### A local OpenConsole build helper was added

`build-openconsole.cmd` exists to build the required host binaries from a local `microsoft-terminal` checkout.

That wrapper handles the awkward pieces that were discovered during debugging:

- setting `VCPKG_ROOT` to the Visual Studio Community `vcpkg` install
- restoring NuGet packages using the Terminal repo's `NuGet.config`
- restoring the solution through `msbuild /t:Restore`
- building both `Host.EXE.vcxproj` and `winconptydll.vcxproj`

Without that extra restore/config work, the build failed on missing Terminal-specific dependencies.

## Testing strategy

The fix was validated by moving away from hand-wavy reasoning and adding reproducible self-tests for the exact problem path.

New coverage includes:

- a Windows self-test harness that can drive the terminal session directly
- a `windows_key_probe` binary for raw console-event inspection
- a `crossterm_key_probe` binary for library-level behavior checks
- focused regression tests for the exact `cmd.exe -> ratatui_key_debug.exe` workflow

The key regression is the live-style default-shell repro:

- launch Teamy with `cmd.exe`
- type the target executable path through the self-test harness
- wait for `ratatui_key_debug` to initialize
- exercise plain key presses and `Ctrl+Backspace`
- assert the resulting event stream matches the expected semantics

## One more subtle issue discovered during testing

After switching to the newer host, some tests failed for a good reason: the old self-test helper typed text using bare `WM_CHAR` events, which was no longer representative enough.

The harness had to be upgraded to synthesize more realistic input:

- keydown
- char
- keyup

for the ASCII characters used by the repro commands.

That was not a regression in Teamy. It was a test harness bug exposed by the more accurate host behavior.

## Final observed behavior

With the newer host and the Teamy-side fixes in place:

- the exact `cmd.exe -> ratatui_key_debug.exe` repro passed
- the simpler `cmd.exe` Enter-path repro passed
- the focused keyboard regression suite passed in full

One detail worth preserving: under the newer host, `Ctrl+Backspace` press may legitimately appear as either of these low-level forms:

- `CH=0008`
- `CH=007F`

The tests now accept both, because both are consistent with the real host behavior seen during validation.

## Practical takeaway

The fix was not a one-line Backspace patch.

It required all of the following to line up:

- more faithful Win32 key-event handling in Teamy
- correct suppression of duplicate legacy character events
- acceptance of both Backspace and DEL forms for the problematic path
- a realistic self-test harness
- a newer OpenConsole host matching Windows Terminal behavior more closely than the older default host

If keyboard behavior regresses again on Windows, start with the exact live path first, not just reduced probes:

1. `cargo run`
2. hosted `cmd.exe`
3. launch `ratatui_key_debug.exe`
4. compare behavior against Windows Terminal

That path is what originally exposed the remaining host-side gap after the earlier Teamy-only fixes.
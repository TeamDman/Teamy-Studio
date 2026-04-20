# OSC Title And Progress Support Plan

## Goal

Add runtime support for terminal title and progress OSC sequences so Teamy Studio behaves more like a modern Windows terminal without changing its existing frameless UI model.

The concrete target is:

- honor `OSC 0` and `OSC 2` title updates from shell output
- display the current terminal title in the purple accent strip
- keep the hidden Win32 window caption synchronized so `terminal list` reports the live title instead of only the launch seed
- honor `OSC 9;4` progress updates and mirror them into Windows taskbar progress state
- continue preventing OSC control sequences from leaking into visible terminal text

## Current Status

- Done so far:
  - Added Tracey requirements for runtime title-strip updates, OSC title/progress non-visibility, and Windows taskbar progress behavior in `docs/spec/product/behavior.md` and `docs/spec/product/os.md`.
  - Replaced the `OSC 133`-only observer in `src/app/windows_terminal.rs` with a shared OSC observer that buffers split sequences, accepts BEL and ST terminators, and parses `OSC 0`, `OSC 2`, `OSC 9;4`, and `OSC 133` from the raw PTY stream for both VT backends.
  - Added terminal chrome metadata state to the terminal worker/session bridge so runtime title and progress changes can wake the UI even when terminal text does not visibly change.
  - Updated `src/app/windows_app.rs` to keep launch title and runtime title separate, render the resolved runtime title in the purple accent strip, and synchronize the hidden HWND caption with that resolved title.
  - Added Windows taskbar progress integration in `src/app/windows_app.rs` for supported `OSC 9;4` states and explicit cleanup on window shutdown.
  - Added regression coverage in `src/app/windows_terminal.rs`, `src/app/windows_app.rs`, and `src/app/teamy_terminal_engine.rs` for title resolution, OSC buffering, semantic prompt preservation, progress parsing, and non-visible OSC title/progress sequences.
  - Verified the final spec state with `tracey query validate --deny warnings`.
  - Verified code compilation with `cargo test --lib --no-run` and full library test execution with `cargo test --lib`, which passed with `181` tests green.
- Current focus:
  - Record the completed implementation state and the remaining repo-level validation caveat in this plan file.
- Remaining work:
  - No feature work remains for OSC title and progress support.
  - Repo-wide `check-all.ps1` is still blocked by an unrelated pre-existing clippy failure in `build.rs` (`unnecessary trailing comma` at `build.rs:37`), so a fully green repo-wide validation run still depends on that separate cleanup.
- Next step:
  - If a repo-wide green validation run is required, fix the unrelated `build.rs` clippy violation and rerun `check-all.ps1`; otherwise the feature itself is complete.

## Constraints And Assumptions

- This is an extension of the existing terminal window behavior surface, not a new subsystem. The correct Tracey move is to extend the current product specs instead of creating a new spec set.
- Runtime title and progress support should work for both `VtEngineChoice::Ghostty` and `VtEngineChoice::Teamy`.
- The right first implementation seam is the shared PTY output observation path in `src/app/windows_terminal.rs`, not the Teamy-only parser in `src/app/teamy_terminal_engine.rs`, because the session layer already sees the raw byte stream for both engines and already handles split `OSC 133` observations.
- The existing launch-time `terminal open --title` flag should remain the initial seed title for a new window until runtime OSC title data overrides it.
- The implementation must distinguish `no runtime title seen yet` from `runtime title explicitly set to an empty string`; otherwise the app cannot preserve correct terminal semantics.
- `OSC` sequences may arrive split across PTY reads and may terminate with either BEL or ST. The implementation must preserve that behavior for title and progress, not just for semantic prompt markers.
- The first slice only needs Windows taskbar progress integration for `OSC 9;4`. There is no current requirement to add a separate in-client progress ring or purple-strip progress UI.
- The app should clear any applied taskbar progress state when the terminal clears it via `OSC 9;4;0` and when the terminal window exits.
- Validation should use `check-all.ps1` rather than ad hoc `cargo check` per repo guidance.

## Product Requirements

- `OSC 0` and `OSC 2` should update the active terminal title at runtime.
- The active terminal title should be rendered in the purple accent strip using the existing title text area.
- The active terminal title should also update the hidden Win32 caption so OS enumeration and the existing `terminal list` command report the same title the user sees in the purple strip.
- `OSC 9;4` should support the full Windows Terminal state set:
  - `0`: clear progress
  - `1`: normal progress with a `0..100` value
  - `2`: error progress with a `0..100` value
  - `3`: indeterminate progress
  - `4`: warning progress with a `0..100` value
- Raw title and progress OSC sequences must not render as visible terminal text.
- Split OSC sequences must remain buffered until complete so partial title or progress payloads do not produce flicker, stale state, or stray visible characters.
- Existing launch-time title seeding must remain intact for windows that never emit a runtime OSC title.
- The feature should not introduce a new top-level CLI surface.

## Architectural Direction

Implement this as shared terminal metadata observation in `src/app/windows_terminal.rs`.

That session layer already owns the backend-neutral PTY stream and already contains the buffering logic needed to recognize `OSC 133` without relying on either terminal engine to expose semantic metadata. Title and progress should follow the same pattern.

Recommended data model:

- Add a small terminal chrome metadata type in `src/app/windows_terminal.rs`, for example:
  - a runtime title state that can distinguish `unset` from `set(String)`
  - a progress state enum such as `Hidden`, `Normal(u8)`, `Error(u8)`, `Indeterminate`, and `Warning(u8)`
- Keep the launch-time title seed separate from the runtime title override in `src/app/windows_app.rs` so empty-string runtime titles remain representable.
- Extend the worker-to-UI snapshot/update path with chrome metadata instead of trying to infer it from render display state. Title/progress changes are window metadata, not terminal grid contents.

Recommended parsing direction:

- Refactor the existing `OSC 133` observer into a more general OSC observation helper that:
  - scans the raw PTY stream for `ESC ]`
  - buffers incomplete payloads across reads
  - accepts both BEL and ST terminators
  - dispatches completed payloads by OSC number
- Keep `OSC 133` semantic prompt tracking on that shared observer.
- Add handlers for:
  - `OSC 0;<title>`
  - `OSC 2;<title>`
  - `OSC 9;4;<state>[;<progress>]`

Recommended UI and OS propagation direction:

- Compute a resolved title in the Windows app from `launch_seed_title` plus `runtime_title_override`.
- Feed that resolved title into the existing `RenderFrameModel.title` path so the purple strip updates without redesigning the renderer.
- Add a `SetWindowTextW` path on the window host so the hidden caption mirrors the resolved title.
- Add a Windows taskbar progress adapter in the app layer and map the `OSC 9;4` states to native taskbar states.
  - Recommended mapping:
    - `0` -> `TBPF_NOPROGRESS`
    - `1` -> `TBPF_NORMAL`
    - `2` -> `TBPF_ERROR`
    - `3` -> `TBPF_INDETERMINATE`
    - `4` -> `TBPF_PAUSED`

## Tracey Specification Strategy

This is a narrow extension of existing tracked window behavior, so extend the current specs instead of introducing a dedicated new spec document.

Recommended spec changes:

- `docs/spec/product/behavior.md`
  - add a requirement that runtime terminal title OSC sequences update the visible purple accent strip title
  - add a requirement that title and progress OSC sequences do not leak into visible terminal text
- `docs/spec/product/os.md`
  - add a requirement that Windows taskbar progress reflects supported `OSC 9;4` states for the active terminal window
- `docs/spec/product/cli.md`
  - do not add a new command surface
  - after implementation, ensure the existing `terminal.list.prints-hwnd-pid-and-title` requirement is verified against the live runtime title behavior

Baseline workflow for each slice:

```powershell
tracey query status
tracey query uncovered
tracey query unmapped
tracey query unmapped --path src/app/windows_terminal.rs
tracey query unmapped --path src/app/windows_app.rs
tracey query validate --deny warnings
```

Follow-up once implementation coverage is in place:

```powershell
tracey query untested
```

Current baseline to carry forward:

- `tracey query validate --deny warnings` is clean
- `tracey query status` already shows broader behavior and tool verification debt, so this feature should add focused mappings and verification refs for touched code instead of trying to solve repo-wide Tracey debt in the same slice
- no existing spec entries were found for runtime OSC title or taskbar progress behavior, so those requirements must be added before or alongside implementation

## Phased Task Breakdown

### Phase 1: Specify The Observable Behavior

Objective:

- Make the runtime title and taskbar progress requirements explicit in Tracey before code lands.

Tasks:

- Extend `docs/spec/product/behavior.md` with runtime title-strip and no-visible-leak requirements.
- Extend `docs/spec/product/os.md` with taskbar progress behavior.
- Review `docs/spec/product/cli.md` only to confirm the existing `terminal list` title requirement is still the correct spec surface.
- Run `tracey query validate --deny warnings` after the spec updates.
- Record the post-spec baseline from `tracey query status` so the next slice starts from an explicit uncovered list.

Definition of done:

- The new behavior is described in Tracey-managed specs.
- Tracey validation is clean.
- The feature can be tracked through uncovered and untested queries without relying on chat history.

### Phase 2: Build A Shared OSC Metadata Observer

Objective:

- Capture runtime title and progress metadata from the PTY stream in a backend-agnostic way.

Tasks:

- Refactor the current `OSC 133` observer in `src/app/windows_terminal.rs` into a generic OSC observation helper that can dispatch completed OSC payloads by code.
- Add shared buffering for split title and progress sequences, reusing the existing BEL/ST terminator logic.
- Add a terminal chrome metadata struct and store it in `TerminalCore`.
- Parse and apply `OSC 0`, `OSC 2`, and `OSC 9;4` payloads.
- Preserve existing semantic prompt behavior for `OSC 133`.
- Add unit tests in `src/app/windows_terminal.rs` for:
  - title updates with BEL termination
  - title updates with ST termination
  - split title sequences across multiple reads
  - progress states `0..4`
  - malformed or incomplete progress payloads being ignored safely

Definition of done:

- Both VT backends can surface the same runtime title and progress metadata.
- Existing `OSC 133` behavior still works.
- The new observer is covered by deterministic unit tests.

### Phase 3: Propagate Title Metadata Into Window Chrome

Objective:

- Make runtime title changes visible in both the app-rendered purple strip and the OS-visible window caption.

Tasks:

- Extend the worker snapshot/update model in `src/app/windows_terminal.rs` to publish terminal chrome metadata changes.
- Separate launch seed title from runtime override state in `src/app/windows_app.rs`.
- Add a resolved-title helper in the app layer.
- Feed the resolved title into `RenderFrameModel.title` using the existing renderer path.
- Add a `SetWindowTextW` wrapper on the window host and update the HWND caption when the resolved title changes.
- Ensure plus-button window duplication and any relaunch paths keep using the correct launch seed or resolved title semantics instead of unintentionally dropping the active title.
- Add tests around title resolution and caption update plumbing where the current window-host tests make that practical.

Definition of done:

- The purple strip title changes when the terminal emits `OSC 0` or `OSC 2`.
- `terminal list` can report the same live title because the HWND caption is synchronized.
- Launch-time `--title` still works for shells that never emit a runtime title.

### Phase 4: Add Windows Taskbar Progress Integration

Objective:

- Mirror `OSC 9;4` states into native Windows taskbar progress for the live terminal window.

Tasks:

- Add a Windows-specific taskbar progress adapter in `src/app/windows_app.rs` or a small neighboring helper module.
- Initialize and own the native taskbar integration on the UI thread.
- Map `OSC 9;4` states `0..4` to the corresponding native taskbar states.
- Clamp or validate progress values to `0..100`.
- Update taskbar progress only when the progress metadata actually changes.
- Clear taskbar progress when the terminal emits `OSC 9;4;0` and when the window closes.
- Add tests for state mapping and lifecycle cleanup where direct taskbar API calls are not easily unit-testable.

Definition of done:

- Supported `OSC 9;4` states produce the expected native taskbar behavior.
- Stale progress does not remain after clear or shutdown.
- No in-client progress UI is required for this phase.

### Phase 5: Verification, Self-Tests, And Coverage Cleanup

Objective:

- Lock the behavior down with regression tests and Tracey mappings.

Tasks:

- Keep or extend the existing `teamy_terminal_engine` tests that assert OSC title data does not leak into visible text.
- Add terminal-session-level tests in `src/app/windows_terminal.rs` for shared observer behavior so Ghostty and Teamy paths stay aligned.
- Add Windows-app-level verification for resolved title sync where practical.
- Add or update self-test coverage if an end-to-end visible smoke path becomes useful for title/caption verification.
- Add implementation refs and verification refs for the new Tracey requirements.
- Run `check-all.ps1`, `tracey query status`, `tracey query validate --deny warnings`, and then `tracey query untested`.

Definition of done:

- The feature is covered by automated tests at the session layer and by Tracey mappings.
- Tracey validation is clean.
- A future agent can see what is implemented, what is verified, and what remains without reconstructing context from chat.

## Recommended Implementation Order

1. Add the new Tracey requirements in `docs/spec/product/behavior.md` and `docs/spec/product/os.md`.
2. Refactor the raw-output observer in `src/app/windows_terminal.rs` into a generic OSC observer with tests.
3. Add terminal chrome metadata state and publish it through the worker/session snapshot path.
4. Resolve runtime title vs launch seed in `src/app/windows_app.rs` and wire it into the purple strip plus HWND caption.
5. Add Windows taskbar progress mapping for `OSC 9;4`.
6. Finish verification refs, focused end-to-end checks, and `check-all.ps1` validation.

This order keeps the risky parsing and state-model work ahead of the UI polish, and it avoids building taskbar code before the metadata source is reliable.

## Open Decisions

- Empty runtime title semantics:
  - Recommendation: once any runtime OSC title has been observed, respect it verbatim, including the empty string, instead of falling back to the launch seed.
- Progress presentation scope:
  - Recommendation: for this slice, update only the Windows taskbar and not the in-client purple strip or diagnostic panel.
- Snapshot shape:
  - Recommendation: extend the worker snapshot/update model with dedicated chrome metadata rather than trying to infer title or progress from terminal display state.

## First Concrete Slice

Start with the smallest slice that gives both spec clarity and implementation traction:

1. Add Tracey requirements for runtime title-strip updates, no visible title/progress leakage, and Windows taskbar progress behavior.
2. Refactor `observe_semantic_prompt_sequences` in `src/app/windows_terminal.rs` into a generic OSC observer helper with unit tests for BEL, ST, and split reads.
3. Teach that observer to capture `OSC 0`, `OSC 2`, and `OSC 9;4` into a new terminal chrome metadata struct while preserving the existing `OSC 133` behavior.

That slice creates the specification, the shared parsing seam, and the metadata contract that every later UI or taskbar change will depend on.
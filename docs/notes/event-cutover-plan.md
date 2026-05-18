# Teamy Studio Workspace Cutover Plan

## Overview

This document describes the implementation plan for the proposed architectural cutover:

- a true Cargo workspace
- a thin root `teamy-studio` composition binary
- a dedicated startup/bootstrap crate for process-boundary orchestration and startup events
- a new event/timeline backbone
- a shell crate that owns Windows/D3D12 scaffolding and shared latency-sensitive behavior
- a `teamy_studio_main_menu` crate for launcher/menu behavior
- a first feature crate, `teamy_studio_cursor_gallery`
- legacy code parked under `legacy/` and excluded from all active builds

The goal is to reduce compile times, improve architectural clarity, and establish a reusable event-driven foundation for future features.

## Current implementation status

Completed in the repository now:

- the repo is a true Cargo workspace
- the planned MVP crates exist under `crates/`
- the root `teamy-studio` package is now a thin MVP composition binary
- the old active `src/` and `tests/` surfaces were moved under `legacy/` with `git mv`
- `legacy/` is excluded from active build and test validation
- the root package now depends only on the MVP stack instead of the previous monolithic dependency set
- `teamy_studio_main_menu` owns button-class registration types and static discovery
- `teamy_studio_registration_core` owns event-definition and trigger registration slices plus startup validation
- `teamy_studio_cursor_gallery` registers its button class and proves the pure event chain from menu click to window-create request
- the root composition publishes that initial event chain into `teamy_studio_timeline_core`
- the startup module now exposes a reusable bootstrapped session surface that accepts externally published events and pumps registered triggers to idle
- `teamy_studio_shell` now owns an explicit `ShellRuntime` abstraction that encapsulates its host scaffold, shell reducer state, and host-stage pumping, and startup consumes that API instead of shell internals directly
- shell window-create requests, hosted-window records, and lifecycle state now carry shell-owned host options for chrome kind, initial visibility, and activation policy
- the shell host scaffold now materializes those host options into concrete native-host plans and renderer-host modes that mirror the legacy launcher/detail-window creation paths
- the shell crate now translates those native-host plans into actual Win32 window style and extended-style flags, reducing the remaining gap to real native window creation
- the shell crate now owns an opt-in native Win32 create/destroy path that registers shell window classes, creates real windows from shell request plans, and records native handles in hosted-window records
- startup now has an explicit native-shell session/composition path that routes shell host stages through the native window-creation API without changing the default simulated MVP boot path
- trigger runtimes now expose a fallible pump contract so cursors only advance after successful epoch processing, matching the intended shell and timeline semantics
- the migrated main menu now carries the full legacy launcher button catalog, including titles and tooltips, via static registrations in the active workspace
- the active binary now opens a lightweight native Win32 main-menu bridge window that displays the migrated launcher catalog instead of exiting after a synthetic startup flow
- the bridge main-menu window now routes clicks into the startup/session event path, with currently migrated buttons left enabled and unmigrated buttons shown but disabled
- the shell crate now owns a reusable scene-model surface for garden-frame, title-bar, custom-chrome, card, text, and sprite composition, and the main-menu crate now builds its launcher visuals against that shell-owned surface instead of raw Win32 child controls
- the main-menu native host now custom-paints the shared scene model and performs hit testing against scene card layouts, which corrects the ownership model even though the renderer backend is not yet D3D12-backed
- the main-menu launch path now uses precompiled bundled shaders and opens directly into the native D3D12-backed renderer path rather than waiting on runtime shader compilation

Still pending:

- restoring the legacy-quality startup bootstrap surface for Figue CLI parsing, `--debug`, `--log-filter`, `--log-file`, structured log collection, and Tracy-backed profiler flows
- moving startup/session orchestration out of the root package into a dedicated startup/bootstrap crate so the root binary is actually thin again
- representing startup bootstrap as a timeline-driven chain beginning from raw process startup inputs and deriving parsed CLI, logging configuration, tracing initialization, validation state, and startup composition requests
- real startup validation UX and gating using explicit per-feature validation events instead of the current synchronous validation call
- actual Win32/D3D12-backed window creation behind the shell host scaffold
- async timeline ingestion, trigger cursors, and runtime pumping
- feature-owned render/input loops and the real cursor-gallery window implementation
- extracting the legacy D3D12 render thread and shader-backed scene renderer into shell-owned helpers so the main menu and feature crates can render the shared scene model through the real backend instead of the current custom-painted stopgap
- richer event definitions with Facet-shaped public canonical event forms

---

# 1. Motivations

## 1.1 Reduce build times

The current repository behaves like a large monolithic crate. Even small edits can force expensive optimized rebuilds.

The new workspace is intended to:

- split stable foundations from frequently edited feature code
- reduce the amount of code touched by any one edit
- improve incremental and release build caching
- make it possible to disable unused feature crates at compile time

## 1.2 Make architecture explicit

The proposal centers on a few clear boundaries:

- process startup, CLI parsing, logging bootstrap, and validation orchestration belong in `startup`
- event identity and publication belong in `event_core`
- registration discovery for event definitions, query triggers, and feature definitions belongs in `registration_core`
- timeline ingestion/query belongs in `timeline_core`
- Windows/D3D12/platform support belongs in `shell`
- launcher/menu behavior belongs in `main_menu`
- feature behavior belongs in feature crates like `cursor_gallery`

This avoids one giant app crate owning everything.

## 1.3 Preserve latency-sensitive behavior

The current cursor-latency work demonstrated that some windows need special presentation policies and cursor-interaction handling.

The design therefore keeps:

- shell-owned support for latency-sensitive behavior
- feature-selected policies per window
- feature-owned message pump and reducer logic
- a single-thread MVP for the first feature slice

## 1.4 Build an event-driven system, not a config-driven one

User preferences, menu layout state, button creation, and feature activation are all intended to be represented as events and reconstructed from timelines.

That means:

- preferences are timeline-derived
- menu state is event-derived
- validation is event-driven
- triggers are registered and pumped from timeline/query infrastructure

---

# 2. Target Architecture

## 2.1 Workspace layout

Use a `crates/` directory with matching crate/package names:

- `crates/teamy_studio_startup`
- `crates/teamy_studio_event_core`
- `crates/teamy_studio_registration_core`
- `crates/teamy_studio_timeline_core`
- `crates/teamy_studio_shell`
- `crates/teamy_studio_main_menu`
- `crates/teamy_studio_cursor_gallery`

The root `teamy-studio` binary remains the product entrypoint, but becomes thin.

## 2.2 Root binary responsibilities

The root binary should only do top-level orchestration:

- gather raw process startup inputs
- hand those inputs to the startup/bootstrap crate
- link in feature crates via Cargo features
- launch the returned product composition

It should not own feature logic.

## 2.3 Startup/bootstrap responsibilities

The startup/bootstrap crate should own process-boundary orchestration that still needs to participate in the event system:

- startup event definitions and registrations
- Figue-backed CLI parsing
- `--debug`, `--log-filter`, and `--log-file` handling
- tracing subscriber installation, structured log collection, and Tracy integration
- startup validation orchestration and activation gating
- construction of the initial session/composition used by the product entrypoint

## 2.4 Legacy code handling

Move the old monolithic implementation into `legacy/` using `git mv`.

Rules:

- `legacy/` is reference-only
- `legacy/` is not a workspace member
- no active build/test command should include it
- it exists to help migration, not as a second supported app

---

# 3. Domain Objects

This section names the key objects that should exist in the new architecture.

## 3.1 Event core objects

### `EventDefinitionId`
A stable, GUID-based identity for a publishable event definition.

### Event definition type
Owned by `teamy_studio_event_core`, includes:

- GUID
- schema identity
- canonical public shape
- graduation/lossless transform hooks

### Arena epoch
A sealed batch of events produced by a privileged arena.

### Writable arena
The mutable, feature-owned event arena used during execution.

### Sealed arena
The immutable version of an arena batch handed to timeline ingestion.

### Publication handshake
The contract between feature code and `timeline_core`:

- seal arena
- hand off sealed batch
- receive fresh writable arena

---

## 3.2 Timeline core objects

### Constructed timeline
A timeline instance created by the app or by tests.

### Timeline origin
An optional `DateTime<Utc>` anchor for wall-clock correlation.

### Canonical time key
A `uom::Time` value with `i128` precision.

### Publication ordinal
A monotonic ingest-assigned ordinal used to order eventually consistent published events.

### Trigger cursor
A trigger’s `last_seen` state, expressed as:

- canonical time key
- publication ordinal

### Trigger runtime
The asynchronous engine that pumps triggers over unseen events.

---

## 3.3 Registration core objects

### Trigger definition registration
A static `linkme` registration describing a query trigger, including a stable opaque trigger-registration identity and auxiliary provenance metadata for diagnostics and implementation discovery.

### Event definition registration
A static `linkme` registration describing an event definition, including auxiliary provenance metadata for diagnostics and implementation discovery.

### Feature definition registration
A static `linkme` registration describing a feature, its stable `FeatureId`, and auxiliary provenance metadata such as repository URL, repo-relative implementation path, and related source-location diagnostics.

### Compatibility validation
A startup/test-time check that a trigger’s declared public view shape matches the registered event definition shape.

This validation should be groupable by feature ownership so startup can validate just the triggers declared by a given feature against the full static set of registered event definitions.

For now this should use exact Facet-shape equality rather than subset matching or implicit projections. The concrete payload type published for an event definition must match the declared public event shape exactly, and trigger subscribed shapes must match that same declared public shape exactly.

Across feature, event-definition, and trigger registrations, provenance metadata should be derived where possible from compile-time context rather than hard-coded, with repository URL as the main intentionally hard-coded field.

Event definitions, event-definition registrations, dispatched global events, trigger definitions, and trigger registrations should remain intentionally separate concepts. Global published events should carry a stable event-definition identity plus a type-erased Facet-reflectable payload so observing crates do not need direct dependencies on the concrete event types emitted by other crates.

---

## 3.4 Startup objects

### `ProcessStartupObservedEvent`
The first canonical startup event containing raw process inputs such as command-line arguments, selected environment variables, and process metadata.

### `CommandLineArgumentsParsedEvent`
The derived typed CLI event emitted after Figue parses the raw startup inputs.

### `LoggingConfigurationResolvedEvent`
The derived startup event that captures the resolved logging configuration, including `--debug`, `--log-filter`, `--log-file`, and environment-driven defaults.

### `TracingInitializedEvent`
The event proving that the tracing subscriber stack and structured log collection are installed.

### Startup/bootstrap runtime
The startup-owned runtime that pumps startup trigger stages until logging, validation, and initial composition are ready.

---

## 3.5 Shell objects

### Feature window host scaffold
An opinionated Windows/D3D12 host scaffold with:

- window creation helpers
- input plumbing
- shared D3D12 context
- cursor/present policy selection
- render scaffolding

### Shared D3D12 context
A process-wide graphics foundation managed by `shell`.

### Cursor/present policies
Shell-provided selectable policies, such as:

- composed
- low-latency HWND
- late-latched pointer visual

### LogicalWindowId
A feature-minted logical window identity carried into shell requests and echoed back by shell lifecycle events for correlation.

---

## 3.6 Main menu objects

### Main menu button class
A declarative registration describing a class of buttons.

### Main menu logical button
A concrete instance of a button class, minted by the menu at startup.

### Layout I-frame
The initial layout declaration establishing button ordering and baseline state.

### Menu layout snapshot
The current derived layout from window size, order, and state events.

### Feature validation state
A launcher-visible state such as:

- pending
- validated
- failed

---

## 3.7 Cursor gallery objects

### Cursor gallery feature arena
The feature-owned event arena and UI thread state for the first feature slice.

### Cursor gallery window
The first feature-owned window that proves the new architecture.

### Cursor gallery frame model
A high-level frame/view model emitted by the feature, consumed by shell rendering.

---

# 4. Implementation Phases

## Phase 0: Freeze the migration scope

Before code changes begin, lock the MVP scope:

- cursor gallery is the first feature crate
- the new architecture is workspace-based
- the root binary stays thin
- `legacy/` is reference-only
- the menu is owned by `teamy_studio_main_menu`
- feature crates own their reducers and windows
- shell provides helpers and shared graphics support
- timelines are constructed, not singleton global objects

This phase is mostly a planning and file-organization decision.

---

## Phase 1: Create the workspace skeleton

### 1.1 Convert the repo into a true Cargo workspace
Add a root `Cargo.toml` with workspace members.

### 1.2 Create the crates directory
Add the following crates:

- `crates/teamy_studio_event_core`
- `crates/teamy_studio_registration_core`
- `crates/teamy_studio_timeline_core`
- `crates/teamy_studio_shell`
- `crates/teamy_studio_main_menu`
- `crates/teamy_studio_cursor_gallery`

### 1.3 Make the root binary thin
The root crate should only wire together the enabled crates.

### 1.4 Park legacy code
Move the old application source into `legacy/` and remove it from active builds.

---

## Phase 2: Build the foundation crates

### 2.1 Implement `event_core`
Start with:

- event definition types
- GUID identity
- arena epoch types
- publication contract types
- lossless graduation/round-trip contract
- typed writable/sealed arena abstractions

### 2.2 Implement `registration_core`
Add:

- `linkme`-backed static registration slices
- event definition registry items
- trigger registration items
- feature definition registry items
- startup validation helpers
- shape compatibility validation using Facet reflection

### 2.3 Implement `timeline_core`
Add:

- constructed timeline instances
- async ingestion of sealed arenas
- publication ordinals
- trigger cursor tracking
- trigger pumping runtime
- optional persistence hooks for future preference timelines

---

## Phase 3: Build the startup/bootstrap crate

### 3.1 Move startup orchestration out of the root package
Create `teamy_studio_startup` and migrate the current startup/session composition logic out of the root crate.

### 3.2 Represent process bootstrap on the timeline
Add startup event definitions and trigger registrations for:

- raw process startup observation
- Figue CLI parsing
- logging configuration resolution
- tracing initialization
- startup validation/gating
- startup composition request/ready events

### 3.3 Restore the legacy-quality logging and profiler surface
Restore enough of the legacy bootstrap to support:

- `--debug`
- `--log-filter`
- `--log-file`
- structured log collection
- Tracy integration used by `run-profiler.ps1`

### 3.4 Keep process-boundary effects explicit
The startup crate may perform imperative process-boundary effects, but those effects should be driven by startup events and captured as derived startup events so the sequence remains reconstructable.

### 3.5 Publish the sealed bootstrap arena losslessly
When bootstrap completes, publish the full sealed bootstrap arena into the primary app timeline rather than reducing it to a smaller canonical startup subset.

This startup history is expected to be noisy, but that is acceptable because the long-term timeline is already intended to carry a very large volume of events. After publication, the private bootstrap arena can be discarded.

### 3.6 Make default app launch an observed behavior
The startup crate should publish both the raw startup observation and the parsed Figue CLI structure into the global timeline.

Default interactive behaviors, including opening the main menu when no subcommand is present, should be derived by observers of that published CLI state rather than hard-coded as root-level fallback logic.

---

## Phase 4: Build the shell crate

### 3.1 Centralize Windows/D3D12 support
Move or rewrite shared Windows machinery into shell-owned helpers and scaffold types.

### 3.2 Provide a feature window host scaffold
Expose a typed host that lets feature crates:

- create windows
- pump input
- select presentation policies
- drive rendering
- access shared D3D12 resources

### 3.3 Keep the shell generic
The shell must not own feature behavior.

It should not know about:

- menu button semantics
- cursor gallery specifics
- application feature policy
- feature-specific reducers

### 3.4 Provide cursor-sensitive behavior policies
Include explicit selectable behaviors for:

- composed presentation
- low-latency presentation
- late-latched cursor interaction

---

## Phase 5: Build the main menu crate

### 4.1 Move menu ownership into `teamy_studio_main_menu`
This crate should own:

- button class registration types
- logical button IDs
- layout derivation
- menu validation presentation state
- menu click event types
- bake/startup layout snapshot logic

### 4.2 Use `linkme` for button class registrations
The menu should discover button classes through static registration.

### 4.3 Derive layout from window size and order
Keep layout deterministic and derived from:

- current window dimensions
- class order / ordinals
- current menu state

### 4.4 Keep logical IDs stable within a session
Logical IDs are session-local for the MVP, but stable through layout rebuilds while the button exists.

### 4.5 Emit wide events
Menu click events should preserve:

- logical button identity
- class identity
- raw click context
- layout context

---

## Phase 6: Build the first feature crate: cursor gallery

### 5.1 Rebuild cursor gallery from scratch
Do not migrate it by dragging the old implementation wholesale.

### 5.2 Keep it feature-owned
`teamy_studio_cursor_gallery` should own:

- window creation logic
- event reducer
- per-window state
- frame model generation
- feature-specific behavior

### 5.3 Keep it single-threaded for the MVP
The feature should process raw events, state updates, and frame production on one thread.

### 5.4 Use high-level frame models
The feature should emit a high-level frame/view model, not raw draw commands.

### 5.5 Use shell helpers for rendering
The shell should render the feature’s frame model using shared graphics support.

---

## Phase 7: Wire menu to feature activation

### 6.1 Register cursor gallery in the menu
Add a button class in `teamy_studio_cursor_gallery` and make the main menu discover it via `linkme`.

### 6.2 Emit menu click events
The main menu should emit a click event for the logical button.

### 6.3 Let the feature trigger respond
The cursor gallery feature should listen for its click event and decide whether to:

- create a new window
- request a shell logical window ID
- emit a create-window request event

### 6.4 Keep the flow pure event-driven
The click path should be an event chain, not direct imperative window creation from the launcher.

---

## Phase 8: Startup validation and gating

### 7.1 Startup validation in the root binary
The root binary should orchestrate validation of linked features and registered queries.

### 7.2 Validate per feature
Validation should be grouped by the feature that owns the query trigger.

Each feature should emit its own validation lifecycle events instead of relying on a single aggregate activation result.

### 7.3 Gate activation
Features may be visible before validation, but activation should be blocked until validation succeeds.

This gating is lazy and dependency-specific: behaviors should wait only on the validation or activation events for the particular features they depend on.

### 7.4 Graceful failure
If validation fails:

- render a clear explanation
- exit gracefully
- do not continue with a broken composition

---

# 5. Concrete First Steps

These are the first practical implementation tasks.

## Step 1: Create the workspace root
- Add workspace `Cargo.toml`
- Declare the new crates
- Ensure the root binary is thin

## Step 2: Move legacy code to `legacy/`
- Use `git mv`
- Remove legacy code from active workspace membership
- Leave it as reference-only source

## Step 3: Stand up the foundation crates
Start with:

- `teamy_studio_startup`
- `teamy_studio_event_core`
- `teamy_studio_registration_core`
- `teamy_studio_timeline_core`
- `teamy_studio_shell`

Implement only the minimum needed API surface.

## Step 4: Build `teamy_studio_main_menu`
- define button classes
- define logical buttons
- implement static class registration
- implement deterministic layout derivation
- implement the launcher startup snapshot

## Step 5: Build `teamy_studio_cursor_gallery`
- define one button class
- define one feature window
- implement a simple high-level frame model
- handle raw input on a single thread
- emit publication events into the app timeline

## Step 6: Wire event publishing and trigger pumping
- add event publication handshake
- add timeline ingestion
- add trigger runtime
- add validation for trigger registrations

## Step 6.5: Restore startup observability before broad feature migration
- rebuild the Figue CLI surface in the startup crate
- restore `--debug`, `--log-filter`, and `--log-file`
- restore structured log collection and Tracy integration used by `run-profiler.ps1`
- represent startup bootstrap on the global timeline from raw startup inputs onward

## Step 7: Verify the first end-to-end slice
The first complete proof should be:

- app starts
- menu appears
- cursor gallery button appears
- click emits event
- cursor gallery opens
- feature renders using shell helpers
- feature events publish into the timeline

---

# 6. Suggested Work Order

If you want the most efficient order, do it like this:

1. workspace skeleton
2. foundation crates
3. startup/bootstrap crate
4. shell scaffold
5. main menu crate
6. cursor gallery crate
7. validation plumbing
8. first end-to-end build
9. move more legacy code out of `legacy/`

This order minimizes the chance of designing abstractions too early.

---

# 7. Risks and Constraints

## 7.1 Over-generalizing too early
Avoid making a universal event model or a universal frame model before the first cursor gallery slice works.

## 7.2 Reintroducing the monolith at the root
Keep the root binary strictly thin.

## 7.3 Letting feature crates depend on each other
Do not allow direct feature-to-feature dependencies.

## 7.4 Letting shell own feature behavior
The shell should provide infrastructure, not application semantics.

## 7.5 Introducing floats into the canonical event model
Keep canonical event types exact and `Eq`-friendly.

## 7.6 Making `legacy/` part of active validation
Do not. It should remain excluded until migration work explicitly pulls code back out.

---

# 8. Success Criteria

The refactor is successful when:

- the repo is a true workspace
- the root binary is thin
- the old monolith is parked in `legacy/`
- `teamy_studio_main_menu` owns menu behavior
- `teamy_studio_cursor_gallery` is the first real feature crate
- feature crates own their own reducers and windows
- shell owns reusable Windows/D3D12 scaffolding
- timelines are constructed and query-driven
- feature validation is startup-gated
- build times improve through crate boundaries

---

# 9. Recommended Starting Checklist

Here is the most practical immediate checklist:

- [ ] create workspace `Cargo.toml`
- [ ] create `crates/` member directories
- [ ] move old code into `legacy/`
- [ ] add empty crate manifests for the new workspace
- [ ] wire the thin root binary
- [ ] implement `event_core` identity and arena contracts
- [ ] implement `registration_core` static registries
- [ ] implement `timeline_core` ingestion and cursors
- [ ] implement `shell` scaffolding and shared D3D12 helpers
- [ ] implement `main_menu`
- [ ] implement the fresh `cursor_gallery`
- [ ] run root validation after each milestone

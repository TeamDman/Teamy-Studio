# Teamy Studio Workspace Cutover Plan

## Overview

This document describes the implementation plan for the proposed architectural cutover:

- a true Cargo workspace
- a thin root `teamy-studio` composition binary
- a new event/timeline backbone
- a shell crate that owns Windows/D3D12 scaffolding and shared latency-sensitive behavior
- a `teamy_studio_main_menu` crate for launcher/menu behavior
- a first feature crate, `teamy_studio_cursor_gallery`
- legacy code parked under `legacy/` and excluded from all active builds

The goal is to reduce compile times, improve architectural clarity, and establish a reusable event-driven foundation for future features.

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

- event identity and publication belong in `event_core`
- registration discovery belongs in `registration_core`
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

- `crates/teamy_studio_event_core`
- `crates/teamy_studio_registration_core`
- `crates/teamy_studio_timeline_core`
- `crates/teamy_studio_shell`
- `crates/teamy_studio_main_menu`
- `crates/teamy_studio_cursor_gallery`

The root `teamy-studio` binary remains the product entrypoint, but becomes thin.

## 2.2 Root binary responsibilities

The root binary should only do top-level orchestration:

- initialize process-wide services
- construct the primary application timeline
- link in feature crates via Cargo features
- initialize the main menu
- start startup validation
- launch the MVP product composition

It should not own feature logic.

## 2.3 Legacy code handling

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
A static `linkme` registration describing a query trigger.

### Event definition registration
A static `linkme` registration describing an event definition.

### Compatibility validation
A startup/test-time check that a trigger’s declared public view shape matches the registered event definition shape.

---

## 3.4 Shell objects

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
A shell-minted window identity used for correlation across feature and shell layers.

---

## 3.5 Main menu objects

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

## 3.6 Cursor gallery objects

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

## Phase 3: Build the shell crate

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

## Phase 4: Build the main menu crate

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

## Phase 5: Build the first feature crate: cursor gallery

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

## Phase 6: Wire menu to feature activation

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

## Phase 7: Startup validation and gating

### 7.1 Startup validation in the root binary
The root binary should orchestrate validation of linked features and registered queries.

### 7.2 Validate per feature
Validation should be grouped by the feature that owns the query trigger.

### 7.3 Gate activation
Features may be visible before validation, but activation should be blocked until validation succeeds.

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
3. shell scaffold
4. main menu crate
5. cursor gallery crate
6. validation plumbing
7. first end-to-end build
8. move more legacy code out of `legacy/`

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

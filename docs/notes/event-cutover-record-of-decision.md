# Record of Decision

This document captures the questions asked and the user’s responses during the architecture/design discussion.

## 1. Architecture split strategy
**Q1.** Do you want the first-class workspace boundaries to be architectural roles or user-facing features?  
**A.** Architectural role first.

**Q2.** Is the timeline itself the synchronous event bus, or is the timeline a canonical observation log fed by a separate synchronous bus?  
**A.** The timeline should be the canonical substrate; control flow/dispatch may go through it.

**Q3.** When an event is submitted into the timeline, do query-trigger handlers run synchronously on the submission path, or do they run from a staged dispatcher over committed timeline entries?  
**A.** Staged dispatcher over committed timeline entries.

**Q4.** Do you want a single globally ordered event stream with one executor policy, or multiple event arenas with independent scheduling rules and explicit bridge events between them?  
**A.** Multiple privileged event arenas.

**Q5.** Within a privileged arena, is state allowed to change directly during dispatch, or must every state change be represented as a derived event that is then applied by a reducer/query?  
**A.** Direct arena-local mutation is allowed; immutability begins only when published to the global timeline.

**Q6.** When events from multiple arenas are published into the global timeline, do you want to force a single total global order, or preserve per-arena order plus explicit causality links?  
**A.** Preserve per-arena order and explicit causality links.

**Q7.** Do you want event types to be fully declared and registered at compile time, or should runtime-defined schemas also be allowed?  
**A.** Compile-time declared event types only.

**Q8.** Should subscriptions match only the exact declared event type, or also structurally compatible Facet shapes across different event types?  
**A.** Exact declared event types first.

**Q9.** How should cancellation or consumption be represented for hot input events like `F3` inside an arena?  
**A.** Cancellation should be represented as a separate published `EventConsumed`-style outcome.

**Q10.** What is the publication unit from a privileged arena into the global timeline: each event immediately, or a sealed batch/epoch?  
**A.** Sealed batches/epochs.

**Q11.** When an epoch is published, should the global timeline receive the arena’s full internal event history for that epoch, or only a curated canonical subset?  
**A.** Canonical subset by default, with optional raw-history mode later.

**Q12.** Is the raw per-arena epoch history required to remain recoverable after transformation, or may the canonical published form be the only retained representation unless diagnostic mode is enabled?  
**A.** The transformation should be lossless and recoverable.

**Q13.** Should the public/global representation decode back into the exact private arena event sequence, or only into an observationally equivalent sequence?  
**A.** Exact sequence reconstruction for v1.

**Q14.** Should the canonical event schemas aggressively avoid floats, or are floats acceptable in the core model?  
**A.** Forbid floats and prefer pure total `Eq`.

## 2. Event identity and registration
**Q15.** What is the first vertical slice we should migrate to prove the architecture without risking the zero-latency cursor path?  
**A.** Start with launcher/menu registration and a non-critical arena.

**Q16.** How should a globally published event type be identified in the registry if there is no enum god-object?  
**A.** With explicit stable keys, not `TypeId`.

**Q17.** Should event definitions have a stable schema version separate from git revision, with git revision recorded only as provenance metadata?  
**A.** Yes, separate schema version from git provenance.

**Q18.** When an event definition’s GUID is rotated because the event changed in kind, should existing query triggers stop matching immediately, or should there be compatibility adapters?  
**A.** Fail closed by default; adapters only when explicitly intended.

**Q19.** How should a query trigger be declared in code: by directly naming the GUID literal, or by referencing an event-definition item that exposes the GUID?  
**A.** Reference the event-definition item.

## 3. Main menu / launcher model
**Q20.** For the first launcher migration, should button providers register only static presentation metadata, or also the behavior that executes when the button is activated?  
**A.** Register both metadata and behavior together.

**Q21.** Should solicitation happen synchronously within the launcher arena epoch, or should main-menu registration discovery happen ahead of time?  
**A.** Precompute the registration snapshot.

**Q22.** For the first launcher slice, should a registration be allowed to contribute arbitrary custom rendering logic, or should it be limited to declarative baked assets and metadata?  
**A.** Declarative baked metadata only.

**Q23.** Should the first launcher slice compile all registered button shaders eagerly at launcher startup, or bake descriptors and lazily compile on first use?  
**A.** Eagerly compile during launcher startup.

**Q24.** For first draw, should every registered button appear immediately with a cheap fallback visual, or may buttons appear progressively?  
**A.** Every button should appear immediately with a cheap fallback visual.

**Q25.** Once the launcher computes its initial baked layout, should button geometry and hit targets be immutable for the life of the window?  
**A.** Yes, immutable for the life of the launcher window.

**Q26.** If two main-menu registrations declare the same ordinal, how should the launcher break ties?  
**A.** Not explicitly answered in the conversation.

**Q27.** Should menu-shape publication be modeled as a full snapshot each time the layout is rebuilt, or as incremental delta events?  
**A.** The launcher should own layout state and can emit thin `ButtonsSwapped` events without reemitting all hidden state; menu shape can be inferred from the event stream plus layout derivation.

**Q28.** If the launcher owns layout state privately and only emits thin swap/layout-intent events, how should a late observer recover the current menu state?  
**A.** The layout is derived from the initial “I-frame” plus subsequent `ButtonsSwapped` and `WindowResized` events.

**Q29.** When the launcher emits a click event, should it contain only the resolved `registration_id`, or also raw click coordinates and layout revision?  
**A.** Wide events are good.

**Q30.** Should a main-menu registration’s `registration_id` be the same GUID as the registration definition itself, or separate definition and runtime IDs?  
**A.** Main menu button class and logical button IDs should be differentiated.

**Q31.** For the first launcher migration, should `MainMenuLogicalButtonId` be supplied by the registering crate, or derived by the launcher?  
**A.** The discussion evolved toward launcher-created logical buttons; later decisions refined this model.

**Q32.** Can a single `MainMenuButtonClassId` produce multiple logical buttons in the same launcher session, or should the MVP require exactly one logical instance per class?  
**A.** Allow multiple logical instances per class in the MVP.

**Q33.** When the launcher recomputes layout because of resize or user preference changes, should existing logical IDs stay stable?  
**A.** Yes, logical IDs should remain stable across layout rebuilds.

**Q34.** What should happen if a class registration disappears between runs but persisted layout preferences still reference it?  
**A.** This was deferred by the later decision to keep preferences timeline-driven rather than a classic config system.

**Q35.** If a logical button is created with no predecessor class registration in the stream, should that be hard error or diagnostic?  
**A.** Not needed for the MVP because the launcher procedural path makes this impossible in normal execution.

**Q36.** For the first extraction, should the launcher logic itself move into a dedicated crate, or should only the registration/event backbone move out while the launcher stays in the shell app?  
**A.** Move the launcher into its own dedicated crate as the first vertical slice.

**Q37.** When a downstream crate observes a click for its button class and wants a new window, should it create the window directly, or emit a `CreateWindowRequestEvent`?  
**A.** The downstream crate should emit a typed request event; the shell creates windows.

**Q38.** When `teamy_studio_shell` emits `WindowCreatedEvent`, should the feature crate receive an opaque capability or the raw window handle/resources?  
**A.** This was later superseded by the Windows-focused decision that feature crates own the window creation through shell helpers.

**Q39.** Should renderer ownership also live entirely in each feature crate, or should shell still provide a standard renderer host abstraction?  
**A.** Shell should provide a standard renderer host abstraction, with feature crates instantiating and driving it.

**Q40.** Should `teamy_studio_shell` expose mostly low-level helpers, or one opinionated feature-window host scaffold?  
**A.** One opinionated host scaffold plus a smaller low-level escape hatch.

**Q41.** For latency-sensitive shell behaviors like cursor sampling and late-latched presentation, should the scaffold expose a fixed policy, or explicit shell-provided policies per window?  
**A.** Feature crates should select among explicit shell-provided policies per window.

**Q42.** Should a feature like `teamy_studio_terminal` be one crate that owns its button registration, click logic, window arena, and scene model, or should those responsibilities split across multiple crates?  
**A.** Keep each feature vertically integrated in one crate at first.

**Q43.** What is the first concrete feature crate to migrate?  
**A.** Cursor gallery is the first target.

**Q44.** When `teamy_studio_cursor_gallery` requests a window, should it use feature-defined identities or a central `SceneWindowKind` space?  
**A.** Feature-defined request/window identities, no central scene enum.

**Q45.** Who should mint `LogicalWindowId` values for feature windows: the feature crate or a shell helper?  
**A.** Shell-provided minting capability.

**Q46.** Should downstream feature crates be allowed to depend on each other directly?  
**A.** No; discourage feature crossing.

**Q47.** Should the current `teamy-studio` crate become a thin composition binary, or remain a broad compatibility facade?  
**A.** Thin composition binary.

**Q48.** How should feature crates be included in the final binary: always linked, or behind Cargo features?  
**A.** Behind Cargo features.

## 4. Event core / timeline core / registration core
**Q49.** Should `teamy_studio_event_core` depend on Facet, or should it stay narrower and let `registration_core` carry most registration machinery?  
**A.** `event_core` should depend on Facet because it owns the graduation mechanism.

**Q50.** Should the global timeline model itself live inside `event_core`, or should there be a separate `teamy_studio_timeline_core`?  
**A.** Separate them.

**Q51.** When an arena seals an epoch, must publication into `timeline_core` complete synchronously before the arena proceeds, or can the arena hand off an immutable batch and continue immediately?  
**A.** Hand off asynchronously; `timeline_core` ingests the sealed batch.

**Q52.** What should happen when asynchronous timeline ingestion falls behind and the handoff queue fills up?  
**A.** Use bounded queues with explicit backpressure, never silent drop.

**Q53.** Should pooled writable arenas handed back by `timeline_core` be strongly typed per arena/event family, or generic erased buffers?  
**A.** Strongly typed arenas per event family; type erasure happens in `timeline_core`.

**Q54.** On the query-trigger side, should a trigger decode the global Facet-backed event all the way back into the feature’s original `TEvent`, or into a separate public trigger-view type?  
**A.** Decode into a new public type, not the private `TEvent`.

**Q55.** How should compatibility between a trigger-view shape and an event definition’s public canonical shape be enforced?  
**A.** Via Facet reflection, unit tests, and `linkme`-registered definitions/triggers.

**Q56.** If a built binary contains a query trigger whose target GUID exists but whose shape is incompatible, should that be only a test failure, or should the app also fail fast at startup?  
**A.** Validation should run at startup in a background sequence and gracefully exit if a problem is found.

**Q57.** While that background validation is still running, should the main menu be interactive, or should the app remain in a validating state?  
**A.** Buttons are visible but feature activation is blocked until validation completes.

**Q58.** While a feature is still unvalidated or has failed validation, should its menu buttons remain visible but disabled, or be hidden?  
**A.** The menu displays button states; later, buttons are visible and blocked until validation completes.

**Q59.** From what point should a newly validated trigger begin replaying history?  
**A.** From an explicit per-trigger cursor, defaulting to session start.

**Q60.** Should the global timeline’s canonical ordering key be wall-clock `DateTime<Utc>`, or a monotonic session key with UTC attached as metadata?  
**A.** Use `uom::Time` with `i128` precision as the canonical key; `Option<DateTime<Utc>>` is the timeline origin.

**Q61.** Should trigger `last_seen` track only the last processed canonical timeline key, or also include a deterministic tie-breaker?  
**A.** Use a pair.

**Q62.** Should `timeline_core` assign a monotonic publication ordinal so triggers can use it as the second half of their cursor?  
**A.** Yes.

**Q63.** Should each registered query trigger be processed with at most one active invocation at a time?  
**A.** Yes, no parallel activation within an individual trigger.

**Q64.** When a trigger handles unseen events and emits side effects, when should its cursor advance?  
**A.** Only after successful completion.

**Q65.** Should side effects first be recorded as emitted events in the timeline and then considered committed, or can some effects complete outside the timeline?  
**A.** A trigger observing a click and creating a window happens within the same handler invocation; it processes part of the slice, yields, and reports progress through a cursor.

**Q66.** Should the trigger runtime’s progress API be defined as count processed or as the last successfully processed cursor?  
**A.** Cursor of the last successfully processed event.

**Q67.** Should `MainMenuLogicalButtonId` values be stable across app restarts?  
**A.** Not persisted for MVP.

**Q68.** During migration, should the new crate-based main menu and cursor-gallery path coexist with the old machinery behind an adapter, or should there be a hard cutover?  
**A.** Hard cutover.

**Q69.** Should the repo become a true Cargo workspace immediately as part of the cutover?  
**A.** Yes.

**Q70.** Before the hard cutover, do you want to freeze current behavior into explicit acceptance tests/specs?  
**A.** Immediate restructure; don’t pre-freeze the interface.

**Q71.** Once the workspace is created, do you want all planned foundation crates up front, or the minimum needed for one vertical slice?  
**A.** Minimum set for the cursor gallery MVP.

**Q72.** During cutover, should non-MVP legacy features be deleted outright or parked under `legacy/`?  
**A.** Park them under `legacy/` with `git mv`.

**Q73.** Should parked legacy code remain in the same repository or move to branch-only archival?  
**A.** Same repo under `legacy/`.

**Q74.** Once parked under `legacy/`, should it still be buildable?  
**A.** Reference-only and excluded from all builds.

**Q75.** After workspace cutover, should docs/spec/Tracey validation stay at the repository root or be copied to each crate?  
**A.** Shared root-level.

**Q76.** Should the root validation/build entrypoint remain one repo command, or should each crate expose its own main validation command?  
**A.** One root orchestration command.

**Q77.** In the new workspace, should the root binary’s default feature set include only the MVP stack, or no optional crates?  
**A.** Default to the MVP stack.

**Q78.** Should the root orchestration command fail if legacy code in `legacy/` stops compiling?  
**A.** Completely ignore `legacy/` in active validation.

**Q79.** Where should persisted app configuration live?  
**A.** Through persistence of a timeline filtered to preference-relevant events.

**Q80.** Should there be one global timeline store for everything, or a separate persisted preference timeline/log?  
**A.** A separate persisted preference timeline/log using the same event model.

**Q81.** Should query triggers be generic definitions attachable to any constructed timeline, or timeline-specific definitions?  
**A.** Generic trigger definitions with explicit attachment policy.

**Q82.** Should manually attached triggers on non-global timelines use the same core trigger contract as `linkme`-registered triggers?  
**A.** Yes, same core contract with different attachment paths.

**Q83.** Should event definitions themselves also support a manual local declaration path?  
**A.** No; static-only event definitions.

**Q84.** Should a feature crate be allowed to define multiple query triggers over the same event definition?  
**A.** Yes, allow multiple query triggers over the same event definition.

**Q85.** Should the new workspace use a crates directory with one crate per package?  
**A.** Yes.

**Q86.** Should package names in Cargo.toml also stay fully prefixed to match the directory names exactly?  
**A.** Yes, match exactly.

**Q87.** Should each enabled feature crate be pulled in purely by optional dependency activation plus a tiny anchor, with `teamy_studio_main_menu` ignorant of concrete features?  
**A.** Yes.

**Q88.** Should `teamy_studio_main_menu` and feature crates depend directly on `teamy_studio_timeline_core`?  
**A.** No; they should depend only on `teamy_studio_event_core`.

**Q89.** Should `event_core` own the traits/types for writable/sealed typed arenas and the publication handshake, while `timeline_core` owns pooling and ingestion?  
**A.** Yes.

**Q90.** Should `event_core` also own the GUID-bearing event definition type and canonical public event-shape traits?  
**A.** Yes.

**Q91.** For `teamy_studio_main_menu`, should button-class registrations reference arbitrary shader sources/assets by path, or shell-managed built-in shader kinds and parameter schemas?  
**A.** Shell owns the shader classes; button classes reference the shader class they want.

**Q92.** Should each shell-owned shader class expose a typed parameter schema?  
**A.** Yes.

**Q93.** Should the primitive first-draw software renderer consume the same typed shader-parameter schema as the eventual GPU shader class?  
**A.** Yes, same schema.

**Q94.** For the first cursor-gallery feature crate, should its main-menu button class live in the same crate as the feature implementation?  
**A.** Yes, same crate.

**Q95.** Should the root executable name remain `teamy-studio`?  
**A.** Yes.

**Q96.** Should the root composition binary contain any substantive app logic beyond feature linking and top-level startup orchestration?  
**A.** Strictly thin.

**Q97.** For the first `teamy_studio_cursor_gallery` implementation, should the existing code be moved or rebuilt from scratch?  
**A.** Rebuild fresh.

**Q98.** Should the feature crate own low-level draw command construction directly, or produce a higher-level frame/view model that shell renders?  
**A.** High-level frame model.

**Q99.** Should the cursor-gallery MVP use a specific frame model or a generic cross-feature one?  
**A.** Specific, not generic.

**Q100.** Should the feature crate own its input-to-state reducer entirely, with shell only delivering raw input events?  
**A.** Yes, shell delivers raw events.

**Q101.** Should the feature process raw input and build frame state on the same thread as the Win32 message pump?  
**A.** Yes, single thread.

**Q102.** Should rendering also stay on that same thread for the MVP?  
**A.** Yes, same thread.

**Q103.** Should `teamy_studio_shell` manage a process-wide shared D3D12/device resource context, or should each feature create its own device stack?  
**A.** Shared process-wide context.

**Q104.** For the first workspace cutover commit, should legacy parking and new workspace scaffolding happen together or separately?  
**A.** Commit choreography not important; focus on design.

**Q105.** Should `MainMenuButtonClassRegistration` live in `teamy_studio_main_menu`, or in `registration_core`?  
**A.** Main menu owns it.

**Q106.** Should `teamy_studio_main_menu` also own the feature-validation state machine?  
**A.** Yes, the menu owns validation presentation/state.

**Q107.** Should `teamy_studio_main_menu` store only current state, or also its own internal event history?  
**A.** Current state only.

**Q108.** Should the startup validation runner live in the root binary or in `teamy_studio_shell`?  
**A.** Root binary owned.

**Q109.** Should the root binary construct one primary app timeline plus selective extra timelines, or one timeline per feature window?  
**A.** One primary app timeline.

**Q110.** Should `teamy_studio_cursor_gallery` publish its arena epochs to the primary app timeline from day one?  
**A.** Yes, publish.

**Q111.** Should the menu click that opens cursor gallery be modeled as a pure event chain or direct creation with observational events afterward?  
**A.** Pure event chain.

**Q112.** When the cursor-gallery open intent repeats while a gallery window already exists, should it focus, create another window, or no-op?  
**A.** Create another independent cursor-gallery window.

**Q113.** When the open intent is reduced into a window-create effect, where should the logical window ID come from?  
**A.** Feature-minted.

**Q114.** If shell fails to create a feature-minted logical window, should that failure be a typed event, a direct error, or both?  
**A.** This was not answered in the conversation excerpt beyond presenting the options.

If you want, I can turn this into a cleaner formal document format next, for example:
- a decision log with timestamps/sections,
- a CSV-style table,
- or a markdown spec with “Decision / Rationale / Follow-up” columns.
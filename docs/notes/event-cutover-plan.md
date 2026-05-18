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
- `teamy_studio_timeline_core` now owns app-owned `TimelineOffset`, `ArenaOffset`, `TimelineId`, `TimelineOrigin`, `Timeline`, `TimelineTransform`, and `EventReference` value types instead of relying on raw `i128`-only time concepts for that model surface
- the timeline model now uses explicit `Repr` and `Unit` generic parameters, with canonical `i128` femtosecond aliases for primary timeline offsets and keys
- `Timeline::transform_to` and `TimelineTransform::apply` now provide the first constrained cross-timeline translation path for same-timeline, direct-relative, sibling-relative, and shared-grounding relationships
- `teamy_studio_event_core::WritableArena` now assigns stable `EventId` values before sealing, and sealed epochs now preserve those IDs alongside their event payloads so later event references can point at exact emitted instances
- `teamy_studio_timeline_core::EventReference` now stores `event_id`, `event_definition_id`, and an exact published timeline-relative offset hint, with helper composition from arena-base plus arena-relative offsets
- `teamy_studio_timeline_core::ConstructedTimeline<PublishedEvent>` now exposes published event-record and event-reference helpers derived from the sealed epoch IDs plus the canonical published time key
- `teamy_studio_timeline_core::TriggerRuntime` now has record-aware unseen/pump APIs so handlers can observe stable `EventId` values alongside the previously exposed cursor and payload data
- the active startup/session/composition path now exposes published event records and event references directly, so the new event-reference machinery is no longer timeline-core-only scaffolding
- the startup trigger stage now exercises the record-aware trigger runtime path even though the current handler logic still ignores the event IDs themselves
- `teamy_studio_event_core` public identity types and the first concrete `teamy_studio_timeline_core` wrapper/value types now derive `Facet`, establishing the initial reflection surface for canonical IDs, canonical time keys, publication ordinals, trigger cursors, exact offsets, arena offsets, and event references
- the GUID-like wrapper IDs in `teamy_studio_event_core` and `teamy_studio_timeline_core` now store `uuid::Uuid` directly while preserving the existing byte-based APIs, and the local `facet` dependency surface now enables its `uuid` support for those reflected forms
- `TimelineOffset`, `ArenaOffset`, and `CanonicalTimeKey` now reflect through a shared tagged-femtosecond Facet proxy surface, so concrete offsets and instants no longer expose their internal unit-marker layout as the primary reflection contract
- the active MVP composition path now publishes a thin registered `StartupSucceededEvent` that carries a real `CanonicalTimeKey` payload in a concrete published event, so the new time-like reflection surface is exercised outside timeline-core-only tests
- the root startup surface now also owns a thin registered `StartupFailedEvent` payload with `EventReference` backlinks plus publish helpers for explicit and latest-reference failure emission, so startup outcome events now have a matched success/failure shape even before the dedicated bootstrap crate exists
- the startup/session/outcome orchestration surface now lives in the dedicated `teamy_studio_startup` crate, and the root `teamy-studio` package has been reduced to a thin re-export layer plus binary shim again
- `teamy_studio_startup` now owns a first typed bootstrap-input slice with raw process startup inputs, startup-global argument parsing for `--debug`, `--log-filter`, and `--log-file`, resolved logging-policy derivation that preserves the legacy conflict/default/path-resolution semantics in executable tests, and a bootstrap-owned tracing initialization helper with stderr plus optional NDJSON file sinks driven from that resolved plan
- the active startup/session/composition builders now accept either explicit raw startup inputs or a pre-derived `StartupBootstrapPlan`, and both `StartupSession` and `AppComposition` preserve that resolved bootstrap plan so process-boundary inputs are no longer discarded before MVP startup/runtime orchestration begins
- startup now publishes concrete early bootstrap events into the startup timeline for raw process observation and resolved logging configuration before the first menu-click event, so the visible event history now begins with bootstrap-state provenance rather than only with later interaction-derived events
- the real `main_with_raw_inputs` bootstrap path now also emits a concrete `TracingInitializedEvent` after subscriber installation succeeds, so tracing initialization is no longer a purely procedural side effect with no corresponding startup-timeline event
- bootstrap now also emits a concrete parsed-global-args event between raw process observation and resolved logging configuration, so the visible startup history preserves the distinction between raw process inputs, parsed startup policy, and derived logging behavior
- bootstrap validation is now beginning to surface as explicit startup stage events: the current startup path publishes registration-validation start/completion events around `validate_registrations()`, so startup validation is no longer completely invisible synchronous work
- feature compatibility validation is now also visible on the startup timeline: the startup path publishes feature-validation start/completion stage events plus a concrete per-feature validated event for the cursor-gallery feature, so the current feature-compatibility pass is no longer represented only by an internal menu-snapshot mutation
- activation gating is now partially explicit as well: the current startup path publishes a per-feature activation-gate-resolved event for the cursor-gallery feature, and the programmatic main-menu click path now enforces the same validated-only gate that the native window hit-testing path already enforced
- tracing initialization failures are now observable from the real startup entry path: `main_with_raw_inputs` builds the startup session before tracing initialization, and an invalid tracing setup now emits a concrete tracing-initialization-failed event plus a linked `StartupFailedEvent` instead of returning only a bare process error
- raw bootstrap derivation failures are now partially observable too: `main_with_raw_inputs` derives the bootstrap plan against a pre-session `StartupRuntime`, so CLI parse failures and logging-plan derivation failures now emit concrete bootstrap failure events plus linked `StartupFailedEvent`s instead of failing before startup can describe what went wrong
- the currently explicit startup failure branches now publish through startup-runtime failure-outcome helpers instead of each call site hand-rolling detail-event plus linked-failure emission, reducing branch-local startup choreography while preserving the same event order and backlink shape for bootstrap, tracing, and default cursor-gallery flow failures
- the reusable raw-input session builders now share the same runtime-aware bootstrap/session helper as the real process entry path, so raw-input bootstrap/session assembly no longer diverges between `main_with_raw_inputs` and the reusable startup construction surface
- reusable raw-input callers can now also retain the pre-session startup timeline when bootstrap derivation fails: the startup crate exposes an observed-bootstrap helper surface that returns either an `ObservedBootstrapPlan` or an `ObservedBootstrapPlanFailure` carrying the populated `StartupRuntime`
- the real `main_with_raw_inputs` path now seeds interactive menu-click timestamps from the session runtime's next publish time instead of restarting at zero after bootstrap publication, preserving a single monotonic startup timeline across bootstrap, tracing init, and first interaction handling

Still pending:

- restoring the remaining legacy-quality startup bootstrap surface for Figue CLI parsing, builtin help/version/completions handling, richer structured log collection policies, and Tracy-backed profiler flows now that the typed startup-global argument, bootstrap-plan threading, and baseline tracing/file-sink installation slice exists in `teamy_studio_startup`
- representing startup bootstrap as a timeline-driven chain beginning from raw process startup inputs and deriving parsed CLI, logging configuration, tracing initialization, validation state, and startup composition requests
- real startup validation UX and gating using explicit per-feature validation events instead of the current synchronous validation call
- actual Win32/D3D12-backed window creation behind the shell host scaffold
- async timeline ingestion, trigger cursors, and runtime pumping
- feature-owned render/input loops and the real cursor-gallery window implementation
- extracting the legacy D3D12 render thread and shader-backed scene renderer into shell-owned helpers so the main menu and feature crates can render the shared scene model through the real backend instead of the current custom-painted stopgap
- richer event definitions with Facet-shaped public canonical event forms
- richer startup failure reporting with thin failure events that point back to concrete prior failure references
- automatic failure publication across all bootstrap failure exits, not just the current root startup surface helpers and MVP cursor-gallery flow failure branch

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

Writable arenas now also own event-instance ID assignment before publication, so sealing preserves stable `EventId` values for each event in the epoch.

### Sealed arena
The immutable version of an arena batch handed to timeline ingestion.

Sealed epochs should preserve both the event payloads and the event IDs assigned while the arena was still private.

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

The active implementation now represents this with an app-owned canonical femtosecond offset wrapper rather than a bare scalar key.

### Publication ordinal
A monotonic ingest-assigned ordinal used to order eventually consistent published events.

### Trigger cursor
A trigger’s `last_seen` state, expressed as:

- canonical time key
- publication ordinal

### Trigger runtime
The asynchronous engine that pumps triggers over unseen events.

The active implementation now has both payload-only and record-aware pump surfaces so callers can opt into receiving stable `EventId` values for unseen published events.

The active startup path now uses the record-aware pump surface for registered triggers.

### Timeline offset model
Application-owned `TimelineOffset<Repr, Unit>` and `ArenaOffset<Repr, Unit>` value types, with canonical `i128` femtosecond aliases for the common case.

### Timeline transform
A first-class `TimelineTransform<Repr, Unit>` built by `Timeline::transform_to(&other)` and applied through `transform.apply(offset)`.

### Event reference
A reusable `EventReference` containing `event_id`, `event_definition_id`, and an exact timeline-relative offset hint derived from arena-base plus arena-relative position.

The current timeline implementation now exposes helper methods that build these references directly from published `PublishedEvent` epochs using the sealed epoch's stable event IDs and the published time key.

The active startup/session/composition surface now also exposes these references directly for the currently published MVP event chain.

---

## 3.3 Registration core objects

### Trigger definition registration
A static `linkme` registration describing a query trigger, including a stable opaque `TriggerRegistrationId`, the corresponding stable `TriggerDefinitionId`, and registration provenance metadata for diagnostics and implementation discovery.

### Event definition registration
A static `linkme` registration describing an event definition, including registration provenance metadata for diagnostics and implementation discovery.

### Feature definition registration
A static `linkme` registration describing a feature, its stable `FeatureId`, and registration provenance metadata.

Feature definitions, event definitions, and trigger definitions should all reuse one common provenance metadata struct shape so the same fields can be copied across definition surfaces even when some fields are unused. Identity fields stay separate from this shared provenance metadata.

Registrations should carry their own provenance metadata rather than being forced to reuse the exact same provenance instance as their corresponding definitions. For common static `linkme` registrations, registration provenance may default to the same values as definition provenance when they are effectively identical.

### Compatibility validation
A startup/test-time check that a trigger’s declared public view shape matches the registered event definition shape.

This validation should be groupable by feature ownership so startup can validate just the triggers declared by a given feature against the full static set of registered event definitions.

For now this should use exact Facet-shape equality rather than subset matching or implicit projections. The concrete payload type published for an event definition must match the declared public event shape exactly, and trigger subscribed shapes must match that same declared public shape exactly.

Validation should walk the full discovered registry set and emit all discovered validation problems.

Those validation problems should be emitted as structured Facet-reflectable failure values. JSON, Facet JSON, terminal display formatting, and other human-facing renderings should be layered on top rather than stored as the primary event contract.

The failure essence should use one common top-level `ValidationFailure` type, with stage-specific detail represented by variants and structured fields rather than separate unrelated top-level failure payloads per validation stage.

`ValidationFailure` should describe only the defect essence. Bootstrap stage, feature identity, registration identity, and other occurrence-specific context should live on the surrounding emitted validation event.

If startup cannot continue, bootstrap should emit a thin `StartupFailedEvent` rather than replaying or re-encoding the detailed failure payloads; consumers that need detailed failures should observe the individual failure events directly. `StartupFailedEvent` should cover any fatal bootstrap-phase failure, not only validation failures, and should carry reusable event references to the exact prior failure events by event ID, event-definition ID, and exact timeline-relative timestamp hint. This in turn implies that private arenas own event-ID assignment before publication to the global timeline, and that event-reference timestamp hints are materialized by combining arena base timeline offset with each event's relative arena offset before publication so the reference stores the composed timeline offset rather than an arena-local offset. The timeline itself may optionally carry a wall-clock UTC start instant, but event references do not publish UTC instants and remain timeline-relative only. The reusable `EventReference` type belongs in `teamy_studio_timeline_core`, though no additional consumers need to be prescribed until concrete uses appear. If the full bootstrap phase completes cleanly, bootstrap should emit `StartupSucceededEvent`, and downstream continuation should depend on that explicit success event. `StartupSucceededEvent` should remain thin and should not mirror the failure-path backlink list.

Timeline time should use application-owned newtypes as the public model rather than directly exposing `uom` quantities throughout the public API. `uom` may still be used internally if helpful, but the externally visible time model should stay application-owned and exact.

That application-owned time model should be generic over representation and unit, with a default app-wide alias for the common case, rather than hard-coding the entire design to one fixed unit in every context.

The default app-wide alias should be an `i128`-backed femtosecond-based instantiation.

In public API spellings, those generic parameters should be named `Repr` and `Unit`, where `Repr` is the numeric carrier such as `i32` or `i128`, and `Unit` is the app-owned time-unit marker such as `Seconds` or `Femtoseconds`.

The first public time model should center on timelines, timeline-relative offsets, arena-relative offsets, and timeline origins, with those types all sharing the same generic `Repr`/`Unit` parameters. `TimelineOrigin` should be an explicit small sum type from the start, and origins may be grounded, relative, or ungrounded rather than being forced to always mean a direct wall-clock instant. Relative origins should start with a constrained single-hop model rather than arbitrary recursive origin graphs, and the first implementation should define an opaque GUID-backed `TimelineId` that relative origins may reference. Every timeline should require a `TimelineId` at creation time, with ordinary creation and `Default` auto-generating one while explicit constructors may accept a supplied ID when needed. `TimelineId` should use the same GUID generation mechanism as the app's other opaque IDs. Human-facing timeline labels can be deferred for now and later supplied through localization keyed by those stable GUID identities. The first implementation should also include a minimal explicit API for translating offsets between timelines when origin relationships make that possible, and that API should return a small result object rather than only a bare converted offset. The relationship/proof metadata in that result should be a small strongly typed structured value that exposes only the resolved relationship used for the conversion. Identity types should implement `Arbitrary`. Dedicated public interval/range types can be deferred for now.

The first implementation should prefer direct inherent methods on these time types rather than introducing a separate public conversion trait before real abstraction pressure exists.

The first implementation should also include dedicated precision-validation tests from the start for arithmetic, offset composition, translation, round-tripping, and non-relatable cases.

Those initial tests should combine focused unit coverage with a small property-style suite that synthesizes arbitrary identities and timelines without expanding immediately into a large generative matrix.

That initial generated suite should cover both relatable and intentionally non-relatable timeline/origin combinations from the start.

For relatable generated cases, the initial property-style suite should require round-trip translation invariants rather than only one-way success assertions.

The first translation result type should keep invertibility implicit in v1 rather than adding a separate explicit invertibility field before the relationship model becomes richer.

The first translation result type should also stay minimal, exposing only the translated offset plus the small resolved relationship metadata rather than a second canonical proof artifact.

The first public translation API should center on a first-class timeline transformation object. Rather than a direct method on `TimelineOffset`, the API should first resolve and construct a transformation object between timelines and then explicitly apply that transformation to offsets. This is intentionally aligned with the design style used by the `sguaba` crate, where typed values remain in their owning coordinate systems while first-class transform objects represent the relationship between systems and get applied to those values.

The first public constructor for that transformation object should live on `Timeline`.

The first transformation type should expose only an apply-style operation in v1, while documentation may explicitly note that composition and inversion are possible future extensions.

The first public names should be `TimelineTransform` for the type, `transform_to` for the constructor on `Timeline`, and `apply` for the operation that applies the transform to an offset.

`Timeline::transform_to` should return a `Result` rather than an `Option`, so failed relationship resolution stays explicit and can preserve structured failure information.

`TimelineTransformError` should preserve as much immediately available structured context as practical in v1, including the source and destination `TimelineId`s plus the resolved failure reason.

Once a `TimelineTransform` has been successfully constructed, `apply` should be infallible in v1 and should directly return the translated offset.

`TimelineTransform` should directly expose its source and destination `TimelineId`s in addition to carrying the minimal resolved data needed to apply the translation.

The small resolved relationship metadata used to build a `TimelineTransform` should remain internal in v1 rather than being exposed directly on the public transform object.

`TimelineTransform::apply` should return only the translated offset rather than bundling the already-exposed timeline IDs back into the return value.

`TimelineTransform::apply` should take `&self` in v1 so transforms behave as reusable borrowed values rather than one-shot consumables.

`TimelineTransform` should implement `Clone` in v1. Timeline IDs and offset types should use cheap value semantics and implement `Copy`.

Timeline identity and other small value-centric public types should live in dedicated files such as `timeline_id.rs` rather than being buried inside one large timeline source file.

That dedicated-file approach should also apply to `TimelineTransform`, `TimelineTransformError`, and other first-class public timeline types as they are introduced.

Those dedicated files should live under a nested `timeline/` directory module rather than remaining as a flat set of files at the crate root.

`timeline/mod.rs` should directly re-export the main public timeline types for callers, but v1 should not introduce a separate prelude module for this area.

Those direct re-exports should cover the full public timeline surface for the subsystem, including error types and other first-class public timeline types that callers are expected to use.

`timeline/mod.rs` should be the single intended stable public import surface for the subsystem, so deeper submodules remain free to change as internal organization details.

Small public timeline value types should commit to a consistent derive surface from the start, including value-semantic derives where appropriate and `Facet` for reflection-oriented use.

The active implementation now satisfies that baseline derive commitment for the initial concrete wrapper/value types, while the stricter canonical Facet proxy-shape work remains a separate follow-up.

The first public timeline types should not commit to `Display` in v1; `Debug` plus explicit accessors should be sufficient until presentation and localization needs are more concrete.

The first public timeline types should also explicitly avoid serde in v1 and rely on `Facet` for reflection-oriented needs instead.

Public timeline types should implement `Default` only where there is an obvious semantically safe default, rather than treating technical defaultability as sufficient reason to expose it.

Public timeline conversions should stay explicit unless they are lossless and semantically obvious; otherwise the API should prefer named constructors and methods over broad `From`/`Into` conveniences.

Timeline offset arithmetic should use explicit named methods in v1 rather than broad `Add`/`Sub` operator overloading.

The first public numeric surface should expose checked arithmetic methods only, with no saturating or wrapping variants in v1.

If checked timeline arithmetic fails, that failure should bail and propagate upward rather than being locally recovered or converted into fallback values.

Checked timeline arithmetic should use a dedicated `TimelineArithmeticError` type rather than being folded immediately into a broader shared timeline error surface.

`TimelineArithmeticError` should preserve the operation kind and operand values that triggered the failure in addition to the failure reason.

Checked timeline arithmetic methods should use standard Rust-style names such as `checked_add` and `checked_sub`.

The initial checked arithmetic surface should also include `checked_neg`.

The initial public offset API should also include small helper methods such as `is_zero`, `is_positive`, and `is_negative`.

`TimelineOffset` should also expose an associated zero constant in v1.

`TimelineOffset::ZERO` should be the only canonical zero surface in v1 rather than being paired with a redundant `zero()` constructor.

`TimelineOffset` should start with one explicit primary constructor rather than multiple early convenience constructors for raw units or literals.

That primary constructor should follow the UOM-style pattern where a generic unit argument determines the supplied time unit, rather than splitting into separate constructor names for femtoseconds, nanoseconds, seconds, and so on.

That constructor should accept only typed unit markers in v1 rather than layering literal-oriented shortcuts on top.

The first public time-unit surface should expose only a curated app-owned set of supported unit markers.

The initial curated public set should start small, covering `Seconds`, `Milliseconds`, `Microseconds`, `Nanoseconds`, and `Femtoseconds`.

That initial public unit set should remain limited to those decimal-step units in v1 rather than adding domain-specific named units.

Conversions between those curated public units should remain explicit in v1 rather than being introduced through implicit or automatic conversion paths.

That explicit conversion surface should use a generic `.get::<Unit>()` pattern rather than unit-specific conversion method names.

`.get::<Unit>()` should return the raw numeric representation directly rather than wrapping the extracted value in another application-owned type.

`.get::<Unit>()` should require exact representability in the requested unit in v1.

V1 should omit lossy extraction APIs entirely.

Exact `.get::<Unit>()` failures should use a dedicated extraction-specific error type rather than reusing `TimelineArithmeticError`.

That extraction-specific error type should preserve the source unit, requested target unit, original raw value, and exactness failure reason.

The public `.get::<Unit>()` surface should be strictly fallible in v1 and should not expose a panicking extraction variant.

That extraction API should live directly on `TimelineOffset` as `.get::<Unit>()`.

`TimelineOffset` should keep its storage-unit concept implicit in v1 rather than exposing a separate public storage-unit query API.

Public timeline value types should expose raw values only through explicit APIs rather than through direct tuple/newtype field access.

Public timeline value types should also avoid `repr(transparent)` and other representation-layout guarantees in v1.

CLI and other input parsing should remain outside the core timeline model; parsing and validation should happen at the boundary before constructing these typed values.

For reflection-oriented proxy forms, time-like values should canonicalize through `Facet` to the femtosecond-based representation rather than exposing multiple unit-shaped serial forms.

The current code now does this for the first concrete time-like public types: `TimelineOffset`, `ArenaOffset`, and `CanonicalTimeKey` all reflect through a minimal tagged proxy containing a stable category token plus canonical femtoseconds.

GUID-like identity types should also share a common Facet proxy shape rather than each defining an ad hoc reflected form.

Canonical Facet proxy commitments in v1 should remain limited to those time-like values and GUID-like identity types, not to more complex non-ID timeline structures such as `TimelineTransform`.

The shared Facet proxy form for GUID-like identity types should be the canonical hyphenated lowercase string representation.

Because Facet already supports `uuid::Uuid`, GUID-like app ID types should use that UUID-backed reflected form transparently rather than adding prefixes or wrapper proxy shapes of their own.

The current code now stores the relevant GUID-like wrapper IDs on `uuid::Uuid` directly and enables Facet's `uuid` support for the affected crates, so those wrappers no longer reflect through ad hoc byte-array storage.

Time-like Facet proxies should still keep separate category envelopes for offsets, durations, and instants even when they canonicalize numerically to femtoseconds.

The current implementation now covers the offset and instant envelopes; a distinct duration envelope remains future work if a first-class public duration type is added.

Those envelopes should stay minimal, containing only the category tag and the canonical femtosecond value.

The category tag in those envelopes should use stable string tokens rather than a second reflected enum-like contract.

The initial stable tokens for the first time-like proxy categories should be `offset`, `duration`, and `instant`.

Those time-like proxy envelopes should also share one field name for the canonical femtosecond value.

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

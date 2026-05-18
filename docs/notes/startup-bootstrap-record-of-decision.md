# Startup Bootstrap Record of Decision

This document captures the decisions made during the May 18, 2026 migration discussion about restoring startup observability, CLI parsing, and profiler support while staying aligned with the event-timeline rewrite.

## Scope

This decision log covers:

- startup/bootstrap ownership
- CLI parsing strategy
- logging and profiler restoration
- how startup should participate in the global timeline

## Decisions

### D1. The next migration milestone is startup observability and CLI restoration.

The next explicit milestone is to restore the legacy-quality startup bootstrap surface before resuming broader feature migration.

This milestone includes:

- Figue-based CLI parsing
- `--debug`
- `--log-filter`
- `--log-file`
- structured log collection
- Tracy integration needed by `run-profiler.ps1`

### D2. Startup behavior should participate in the event system.

The rewrite should remain faithful to the global-timeline philosophy. Startup behavior is not a special side channel that sits outside the event system.

CLI parsing and subscriber setup should happen first in practice, but they should happen through startup/bootstrap plumbing that is modeled on the event system.

### D3. Startup should begin from raw process inputs, not only the parsed CLI.

The recommended startup chain was accepted:

1. publish a raw startup observation event carrying process inputs
2. derive a parsed CLI event using Figue
3. derive logging configuration from parsed CLI plus environment
4. initialize tracing and record that initialization as an event
5. continue into validation and startup composition events

This preserves provenance and keeps startup reconstruction faithful to the timeline model.

### D4. A dedicated startup crate should own startup/bootstrap logic.

Startup/bootstrap logic should move into its own crate rather than continuing to accumulate in the root package.

That crate is intended to own:

- startup event definitions
- startup trigger registrations
- Figue-backed CLI parsing
- logging configuration resolution
- tracing subscriber and structured log setup
- Tracy integration
- startup validation orchestration
- initial session/composition bootstrap

### D5. The root package should become thin again.

After the startup crate exists, the root `teamy-studio` package should return to being a genuinely thin entrypoint that:

- gathers raw process inputs
- delegates startup/bootstrap to the startup crate
- launches the returned composition

It should not continue to own substantive startup/session behavior.

### D6. Help, version, and parse-failure outcomes should also live on the startup timeline.

Ordinary CLI builtins and parse outcomes should not bypass the startup event model.

The startup chain should be able to emit derived outcomes such as:

- `CommandLineHelpRequestedEvent`
- `CommandLineVersionRequestedEvent`
- `CommandLineParsingFailedEvent`

An explicit startup effect stage may then decide to render help/version/error output and exit cleanly.

Only a very small root-level fallback is allowed for catastrophic failures where the startup/bootstrap machinery cannot initialize enough to publish the first startup event at all.

### D7. Early startup events should be buffered and replayed after tracing initialization.

The earliest startup events occur before the tracing subscriber stack is fully installed.

Those events should still be produced as part of the startup event stream, buffered in memory, and replayed once tracing is initialized so the eventual log collector and timeline/log viewer can observe the full startup narrative.

This avoids losing provenance for:

- raw process startup observation
- parse requests and parse outcomes
- logging configuration resolution
- early startup validation/setup events

### D8. Startup should begin in a dedicated bootstrap arena.

Startup should not begin directly on the primary app timeline.

Instead, `teamy_studio_startup` should own a dedicated bootstrap arena or bootstrap timeline that:

- captures raw startup inputs
- drives CLI parsing and startup derivation
- resolves logging/tracing/validation/composition
- buffers and replays early events once tracing is initialized
- seals and publishes a canonical startup epoch into the primary app timeline once bootstrap is ready to hand off

This keeps bootstrap behavior aligned with the privileged-arena model and prevents partially initialized startup behavior from being mistaken for ordinary runtime activity on the primary app timeline.

### D9. Bootstrap runtime performs tracing initialization procedurally, then emits an event.

The bootstrap runtime itself should perform tracing initialization once startup events have resolved the logging configuration.

This is intentionally a privileged bootstrap action rather than a generic trigger-side effect.

The sequencing model is:

1. bootstrap events derive logging configuration
2. bootstrap runtime procedurally installs the tracing subscriber stack, structured log collector, and Tracy layer as needed
3. bootstrap runtime emits `TracingInitializedEvent`
4. early buffered startup events are replayed into the now-live tracing/logging surface

This matches the broader arena philosophy: internal procedural work is acceptable inside a privileged arena as long as the externally published event stream faithfully records the resulting sequence.

### D10. `ProcessStartupObservedEvent` should preserve raw startup inputs as owned OS-string values.

The first startup observation event should capture the raw process startup facts needed for reconstruction, including:

- the argument array
- selected environment variables or environment block
- the current working directory
- other relevant process-boundary metadata

These values should be stored as owned OS-string values rather than eagerly normalized into UTF-8 strings so downstream observers can interpret them however they need.

### D11. `ProcessStartupObservedEvent` should capture the full environment block.

The bootstrap arena should observe the full process environment block rather than guessing up front which variables matter.

That full environment capture remains part of the private bootstrap history. Later derived startup events and the canonical startup epoch published to the primary app timeline may expose only the subset of environment facts that actually influenced behavior.

### D12. The full bootstrap arena history should be published losslessly to the primary app timeline.

When bootstrap completes, the sealed bootstrap arena should be published to the global timeline losslessly rather than being reduced to a smaller canonical subset.

The reasoning is that the primary timeline is already expected to become very large, so bootstrap noise is not considered a meaningful downside. Once the bootstrap arena has been sealed and published, the private bootstrap arena can be dropped.

### D13. Bootstrap owns CLI parsing, logging initialization, and publishes both raw and interpreted startup facts.

The bootstrap sequence is responsible for:

- observing raw process startup inputs
- parsing the command line with Figue
- initializing logging/tracing after validating the startup configuration
- publishing the sealed bootstrap history only after that startup work is complete

When bootstrap publishes its sealed history to the global timeline, it should include both:

- the raw startup observation facts such as raw arguments and environment
- the interpreted typed CLI structure produced by Figue

This allows downstream features to observe the published parsed CLI and decide whether their behavior should follow.

### D14. Default main-menu launch is a behavior derived from the published parsed CLI.

Opening the main menu by default should no longer be an implicit root-level fallback.

Instead, the main-menu behavior should observe the published parsed CLI and decide to open itself only when no subcommand is present.

This keeps the default interactive app launch consistent with the broader event-driven model: the main menu opens because a feature/runtime observer derives that behavior from the published CLI state.

### D15. The CLI schema remains startup-owned for now.

The startup crate should own the CLI schema, including `Cli`, `GlobalArgs`, and the Figue parser surface, for the current migration milestone.

Feature crates should not yet contribute command-line subcommands through registration.

Instead, features can observe the published parsed CLI and derive their behavior from that typed startup state. If feature-contributed subcommands become important later, that can be designed as a separate follow-up once the startup/bootstrap migration is stable.

### D16. Startup validation should be modeled as multiple explicit stages.

Startup validation should not collapse into one opaque validated-or-blocked result.

Instead, the bootstrap arena should publish explicit stage events such as:

- registration validation start/completion
- feature compatibility validation start/completion
- activation gating resolution
- final startup ready or startup blocked outcomes

This keeps the startup history more diagnostic, makes failure modes easier to reason about, and gives downstream observers finer-grained validation state.

### D17. Feature validation results should be emitted as separate per-feature events.

Feature validation should emit separate events keyed by feature identity rather than one aggregate all-features-activated or all-features-validated event.

This matches the lazy activation model:

- the main menu may be visible while many feature buttons exist
- features activate on demand rather than as one synchronized global activation wave
- downstream behaviors should wait only on the validation or activation events for the specific features they depend on

An aggregate all-features-activated event is not considered useful for the intended runtime model.

### D18. Feature validation is grouped by feature and checks trigger-to-event shape compatibility.

Startup validation should be grouped by feature ownership and operate over the static `linkme` registries.

For each feature, validation checks the query triggers that feature declares and verifies that among the globally registered event definitions there exists a compatible published event shape for each subscribed trigger view shape.

This reflects the intended event model where publication and subscription use different concrete types, but the Facet-exposed public contract still matches after type erasure into the global event stream.

### D19. Feature validation and activation use a dedicated `FeatureId` type.

Startup validation and feature activation should not key off crate names or main-menu button-class IDs.

Instead, the system should define a dedicated stable `FeatureId` type used by:

- feature definitions
- startup validation events
- activation and dependency events
- any future feature-level coordination that should not depend on UI concepts

Human-readable names such as crate or package names may still appear as descriptive metadata, but they are not the primary identity contract.

### D20. Feature definitions should have their own static `linkme` registry.

In addition to the existing static registries for event definitions and trigger registrations, the architecture should add a separate static registry for feature definitions.

That registry is intended to provide a static list of features and their `FeatureId` values so startup can:

- enumerate known features
- group validation work by feature
- emit per-feature validation and activation events
- attach descriptive metadata to feature-scoped startup behavior

This means the system will have distinct `linkme` surfaces for:

- event definitions
- query trigger registrations
- feature definitions

### D21. `FeatureId` should be an opaque GUID.

`FeatureId` should be a stable opaque GUID rather than a human-readable slug or name-derived identifier.

The GUID remains fixed even if associated structs, crate names, or feature-facing terminology are renamed during refactors.

### D22. User-presentable names belong to a separate localization system.

Feature definitions should not rely on human-readable identifiers as their stable contract.

Instead, opaque GUID identities should be translated into user-presentable names and descriptions by a separate localization or translation system that can map those stable GUIDs into culture-aware display strings.

This same philosophy is expected to apply more broadly to other opaque identifiers such as event identities.

### D23. Static feature definitions should stay identity-minimal.

The first version of a static feature definition should stay extremely small and center on the opaque `FeatureId` rather than accumulating broad descriptive metadata.

Useful diagnostic metadata such as:

- source file paths
- Facet-reflected shape information
- predicted GitHub implementation URLs tied to the current revision

may still exist, but they should be treated as derived or auxiliary metadata rather than the stable identity contract of the feature definition itself.

This keeps the feature registry from becoming a second semantic configuration object while still allowing richer diagnostics and help surfaces later.

### D24. Feature definitions stay in `registration_core`.

The static feature-definition registry should live in `teamy_studio_registration_core` rather than being split into a separate dedicated crate for now.

This keeps the three related static registration surfaces together:

- event definitions
- query trigger registrations
- feature definitions

while still allowing them to remain distinct types and APIs inside the same crate.

### D25. Feature definitions may include repository URL metadata.

Even though feature identity stays centered on an opaque `FeatureId`, it is acceptable for feature definitions to carry repository URL metadata as auxiliary diagnostic information.

This anticipates cases where a feature or provider may live in:

- a different crate
- a different repository

and where tooling may want to predict documentation or implementation URLs outside the current repository.

That repository URL remains descriptive metadata, not the stable identity contract.

### D26. Feature provenance metadata should separate repository URL, repo-relative path, revision, and local disk path.

For diagnostics and help surfaces, the useful provenance pieces are:

- repository URL
- implementation path relative to that repository
- git revision as a separate value
- optional on-disk local path metadata

Those pieces should remain separate rather than being collapsed into one prebuilt URL field.

This allows tooling to derive a full implementation URL from:

- repository URL
- repo-relative path
- git revision

while also preserving the local on-disk path as separate diagnostic metadata. The local path may be derived from reflected shape metadata or stored explicitly if that proves more reliable.

### D27. Local source paths should be derived rather than hard-coded.

Local on-disk source paths should be derived from reflected shape metadata or other compile-time context rather than being manually hard-coded into registrations.

Similarly, repo-relative implementation paths should be derived from compile-time context such as:

- the current file path
- the cargo manifest directory
- supporting macros such as `file!`

This keeps source-location metadata resilient to files being moved.

### D28. Provenance metadata should apply to features, event definitions, and trigger registrations.

The same general provenance model should apply across all three static registration surfaces:

- feature definitions
- event definitions
- query trigger registrations

For each of these, identity remains the stable opaque ID, while provenance metadata may include:

- hard-coded repository URL
- git revision from build-time context
- derived repo-relative path
- derived local source path

These fields are intended for diagnostics, help surfaces, and implementation discovery, not identity.

### D29. Query trigger registrations should have their own stable opaque ID.

Query trigger registrations are first-class static registrations and should not rely only on owner feature plus subscribed event shape for identity.

Instead, trigger registrations should have their own stable opaque `TriggerRegistrationId` so that:

- diagnostics can identify the exact trigger registration that failed validation
- multiple triggers owned by the same feature can observe the same published shape without identity ambiguity
- future tooling can refer to trigger registrations directly as first-class objects

### D30. Event definitions own the public event shape, and the stable GUID is attached to the definition rather than stored as a payload field.

An event definition type describes the public shape of the event.

The stable `EventDefinitionId` is attached to that definition as metadata, for example through an attribute-style declaration, rather than being treated as an ordinary runtime payload field.

This allows the definition type to be renamed or moved without changing identity so long as the public event shape remains the same. When the public shape changes, the GUID should rotate, and tests should verify that rotation happened when expected.

### D31. Event definitions, registrations, and dispatched global events are intentionally separate concepts.

The intended separation is:

- the event definition describes the public event shape and owns the stable event-definition GUID
- the event-definition registration is the static `linkme` registration instance used for discovery
- the dispatched global event carries the event-definition ID plus a type-erased Facet-backed payload

This separation should remain explicit in both code and terminology.

### D32. Global published events are type-erased on purpose to avoid feature-crate coupling.

Inside a private arena, a feature may still use strongly typed local events, enums, and direct matching.

But once an event is published to the global timeline, it should be represented as the stable event-definition identity paired with a type-erased Facet-reflectable payload.

This deliberate disconnect allows observing features to interpret the published shape without depending directly on the emitting crate’s concrete event type.

### D33. Query trigger definitions mirror the same separation as event definitions.

Query trigger definitions should follow the same model:

- the trigger definition declares the subscribed public view shape
- the trigger definition owns its own stable trigger GUID
- the trigger registration is the static `linkme` registration instance used for discovery

Trigger definitions hard-code the event-definition ID they expect to observe. The relationship between trigger and event is therefore established by matching stable IDs and matching Facet-exposed shapes, with startup validation and unit tests asserting that the intended correspondence exists.

### D34. Public event shapes should validate by exact Facet-shape equality.

For now, the system should stay strict and fail closed.

That means:

- an event definition declares one public Facet-exposed shape
- the concrete payload type published for that event definition must match that declared public shape exactly
- a trigger definition's subscribed public shape must also match the target event definition's public shape exactly

Subset matching, compatible-shape matching, or best-effort field projection should not happen implicitly during startup validation.

If a publisher or observer needs a different shape later, the system should introduce an explicit adapter or a different event definition rather than widening the validation rules.

This keeps the model easy to reason about, makes copy-paste of full field sets straightforward, and preserves the quality bar while the event backbone is still being established.

## Current migration gap being addressed

The active workspace currently has:

- only a minimal `tracing_subscriber::fmt()` setup in `src/main.rs`
- no active Figue CLI surface
- no restored `--debug`, `--log-filter`, or `--log-file` behavior
- no restored structured log collector layer
- no restored Tracy integration even though `run-profiler.ps1` still assumes it

The legacy implementation already contains the reference behavior for these surfaces.

## Follow-up questions still open

- whether trigger definitions should be allowed to declare a distinct trigger-definition ID in addition to the trigger-registration ID, or whether trigger definitions can stay identified only by the matched event-definition ID plus public shape
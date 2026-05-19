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
- the trigger definition owns its own stable `TriggerDefinitionId`
- the trigger registration is the static `linkme` registration instance used for discovery and owns its own stable `TriggerRegistrationId`

Trigger definitions hard-code the event-definition ID they expect to observe. The relationship between trigger and event is therefore established by matching stable IDs and matching Facet-exposed shapes, with startup validation and unit tests asserting that the intended correspondence exists.

### D35. Trigger definitions and trigger registrations should have different identifiers.

The trigger definition and the trigger registration should not share the same identity.

Instead:

- `TriggerDefinitionId` identifies the public subscribed trigger shape and its declared relationship to a target event definition
- `TriggerRegistrationId` identifies the concrete static registration instance discovered through `linkme`

This keeps the model symmetrical with event definitions versus event-definition registrations, supports copying the same structural fields across all definition and registration surfaces, and avoids ambiguity if the same trigger definition shape is ever registered from more than one place.

### D36. Feature definitions, event definitions, and trigger definitions should share one common provenance metadata struct.

To keep the quality bar high and make definition objects easy to copy and paste consistently, the three definition surfaces should all reuse one shared provenance metadata struct shape.

That means:

- feature definitions carry the shared provenance struct
- event definitions carry the shared provenance struct
- trigger definitions carry the shared provenance struct

Some fields may be unused for a given definition kind, but the shape should stay the same. Unused fields can remain unset rather than creating separate per-definition provenance variants.

Identity fields remain separate from provenance metadata.

### D37. Registrations should have their own provenance metadata that may default to the definition provenance.

Registration objects should not be forced to reuse the exact same provenance instance as their corresponding definitions.

Instead, registrations should carry their own provenance metadata, because registration provenance may describe facts specific to the concrete static `linkme` registration site.

At the same time, for the common static-registration case, registration provenance may default to the same values as the definition provenance when they are effectively identical.

This gives the model room for registration-specific provenance without losing the easy copy-paste shape for the usual case.

### D34. Public event shapes should validate by exact Facet-shape equality.

For now, the system should stay strict and fail closed.

That means:

- an event definition declares one public Facet-exposed shape
- the concrete payload type published for that event definition must match that declared public shape exactly
- a trigger definition's subscribed public shape must also match the target event definition's public shape exactly

Subset matching, compatible-shape matching, or best-effort field projection should not happen implicitly during startup validation.

If a publisher or observer needs a different shape later, the system should introduce an explicit adapter or a different event definition rather than widening the validation rules.

This keeps the model easy to reason about, makes copy-paste of full field sets straightforward, and preserves the quality bar while the event backbone is still being established.

### D38. Startup validation should collect and emit all discovered problems before blocking readiness.

When startup validation finds invalid definitions or registrations, it should not stop at the first failure.

Instead, bootstrap validation should:

- continue through the full discovered registry set
- emit validation events for every discovered problem
- emit stage completion and blocked-readiness outcomes after the full defect set is known

Startup should still fail closed. If any validation problems were discovered, readiness should be blocked after the full set has been emitted.

This preserves the quality bar while making a single startup attempt maximally diagnostic.

### D39. Validation failures should be emitted as structured typed data, with presentation handled separately.

Validation problem events should not rely on a preformatted human-readable message string as their primary contract.

Instead, each validation failure should be represented by a Facet-reflectable typed value that captures the essence of the failure in structured data.

Presentation should then be layered on top of that data:

- machine-readable renderers may serialize it to JSON or Facet JSON
- terminal-oriented renderers may implement rich display formatting, including color where appropriate
- other UI surfaces may render the same structured failure in their own way

This keeps the data model stable and reusable while separating display concerns from the failure essence itself.

### D40. Startup validation should use one common top-level `ValidationFailure` type.

Startup validation should not publish unrelated ad hoc failure payload types per stage.

Instead, the failure essence should be modeled as one common top-level `ValidationFailure` type, with stage-specific detail represented as structured variants and fields inside that shared type.

This keeps rendering, export, logging, and UI handling consistent while still allowing each validation stage to report rich structured details.

### D41. `ValidationFailure` should describe only the defect essence, while emitted events carry occurrence metadata.

The shared `ValidationFailure` type should not absorb bootstrap stage, emission site, or other occurrence-specific metadata.

Instead:

- `ValidationFailure` describes only the actual defect
- the emitted validation event carries bootstrap stage, feature identity, relevant definition or registration identities, and any other occurrence metadata

This keeps the defect model reusable and presentation-independent while still allowing emitted events to carry the context needed for diagnostics, logs, and UI.

### D42. Startup should use explicit success and failure events rather than a generic blocked-readiness event or generic summary event.

The preferred startup control-flow language is:

- detailed validation failures are emitted as their own events during validation
- `StartupFailedEvent` is emitted when startup cannot continue
- `StartupSucceededEvent` is emitted when startup may proceed

The stopping of execution should be the observed consequence of a startup failure event or other explicit app-exit event, rather than a specially named blocked-readiness event.

Likewise, continued startup should depend on an explicit startup success event rather than being inferred from a generic summary object.

### D43. `StartupFailedEvent` should stay thin and link out to detailed failure events rather than carrying failure content itself.

`StartupFailedEvent` should not duplicate the full set of detailed failure payloads or re-encode the failure content itself.

Instead, it should carry lightweight terminal-startup metadata plus references to the previously emitted detailed failure events.

Consumers that care about the exact nature of the failure should observe or look up those referenced events directly.

This keeps the startup failure event cheap while preserving the split between detailed defect observations and the terminal event that causes the app to stop.

`StartupFailedEvent` is not limited to validation failures. It should cover any fatal failure within the bootstrap phase, including validation failure, tracing initialization failure, CLI execution failure, or other bootstrap-phase termination conditions.

### D45. `StartupFailedEvent` should reference the exact prior failure events by both event ID and event-definition ID.

When bootstrap emits `StartupFailedEvent`, it should be able to point back to the exact earlier events that contain the detailed failure information.

To support that, `StartupFailedEvent` should carry a list of failure references, where each reference includes:

- the concrete event ID of the emitted failure instance
- the event-definition ID for that failure event

This gives downstream observers a direct path from the terminal startup failure event to the exact detailed events that explain the failure.

The detailed failure events remain the place where failure-shape-specific enums or structured payload variants live. Consumers that want to understand those specifics can subscribe to or inspect those events and interpret their event definitions accordingly.

### D47. Event references should be a reusable value type containing `event_id`, `event_definition_id`, and a timeline-relative timestamp hint.

The reusable event-reference value should contain:

- `event_id`
- `event_definition_id`
- a timeline-relative timestamp hint

This shape should not stay as a startup-specific ad hoc structure.

Instead, the system should define one reusable event-reference value type that can be copied anywhere the architecture needs to point at a previously emitted event instance.

The timestamp hint exists to make later querying easier. If a consumer wants to look up a concrete event instance, knowing the event ID, event-definition ID, and timeline-relative event time gives a better starting point for searches.

`StartupFailedEvent` is the first concrete consumer of that type, but the reference concept should be general rather than startup-specific.

### D46. Private arenas should own event-ID assignment before publication to the global timeline.

If `StartupFailedEvent` is expected to reference exact earlier emitted events, then those events need stable instance identities before the enclosing arena is published to the global timeline.

The private arena should therefore own event-ID assignment during local emission and sealing, rather than delegating instance-ID assignment entirely to an upstream global ingestion step.

Using GUIDs for event IDs is acceptable here. That lets the local emitter generate event IDs with sufficiently low collision risk while preserving stable references when the sealed arena is later published.

### D48. Event-reference timestamp hints should be materialized from arena base offset plus event-relative offset.

The primary timeline should use a `uom` time quantity backed by `i128` precision and should begin at zero.

The timeline may also optionally carry a wall-clock UTC start instant as auxiliary metadata, but event references should not depend on that wall-clock origin.

Sealed arenas should publish a base offset on the primary timeline, while each event within the arena carries its own offset relative to that arena base.

When producing an event reference, the system should combine:

- the arena base offset on the global timeline
- the event's offset within the arena

to produce the timeline-relative timestamp hint stored on the reusable event reference value.

This means event references do not depend on external knowledge of the arena structure in order to be useful. The reference already contains the fully joined timeline-relative offset needed for downstream querying.

### D49. Event references should store the exact published timeline-relative offset, not an arena-local offset.

The reusable event reference should store the exact event offset from the start of the primary timeline.

It should not store only the arena-local offset and require consumers to separately know the arena base.

This keeps event references self-sufficient for lookup while preserving the timeline's model of:

- primary timeline offset from zero
- arena base offset from the timeline
- event offset from the arena base

The event reference stores the composed result of that model rather than exposing the arena-local component alone.

### D50. Event references should not publish wall-clock UTC instants.

Even when the timeline carries an optional wall-clock UTC start instant, that wall-clock mapping should remain timeline-level metadata.

The reusable event reference should publish only:

- `event_id`
- `event_definition_id`
- exact timeline-relative offset

It should not embed a UTC instant.

Consumers that need wall-clock mapping can derive it from timeline metadata plus the stored timeline-relative offset.

### D51. The reusable `EventReference` type should live in `teamy_studio_timeline_core`.

`EventReference` should not live in `teamy_studio_event_core`.

Because the reusable reference now includes the exact timeline-relative offset of a published event instance, it belongs with the timeline concepts rather than with event-definition identity alone.

`teamy_studio_timeline_core` should therefore own the reusable `EventReference` type.

The type should remain generally reusable, but the architecture does not need to prescribe any additional consumers yet. Other flows can adopt it later if and when a concrete use case appears.

### D52. Timeline time should use application-owned newtypes as the public model.

The public time model should not directly expose raw `uom` quantities everywhere.

Instead, the architecture should define application-owned time newtypes for the concepts it actually needs, such as:

- timeline-relative offsets
- arena-relative offsets
- optional wall-clock moments or origins

These newtypes should form the public API surface.

If `uom` remains useful, it may still be used internally as an implementation detail, but the public contract should stay application-owned so the model remains exact, testable, and aligned with the timeline semantics rather than with an external crate's assumptions.

### D53. The application-owned time model should be generic over representation and unit, with a default app-wide alias.

The timeline system should not lock the architecture to one permanently fixed backing unit in every context.

Instead, the application-owned time model should support generic representation and unit choices so the system can represent very different scales cleanly, from application-runtime spans to historical or cosmological timelines.

The intended shape is:

- application-owned generic time types rather than adopting `uom`'s public model
- a default common instantiation for normal app use
- the ability to introduce different numeric carriers and smaller or larger units later when a different range tradeoff is needed

For the common case, the app can expose a standard alias such as an `i128`-backed femtosecond-based instantiation.

This effectively means building the app's own time system for its actual needs rather than bending the design around `uom`'s assumptions and mental model.

### D54. The default app-wide time alias should be `i128` femtoseconds.

While the underlying time model stays generic over representation and unit, the default application-wide instantiation should be an `i128`-backed femtosecond-based alias.

That gives the implementation a concrete common-case type without giving up the ability to introduce other precision-range tradeoffs later when a real use case appears.

### D55. The first public time model should center on offsets and origins rather than richer absolute-moment types.

The initial public model should start with:

- `Timeline`
- `TimelineOffset`
- `TimelineOrigin`
- `ArenaOffset`

rather than introducing a richer family of absolute wall-clock-like moment types immediately.

These types should all participate in the application-owned generic representation/unit model.

### D56. `Timeline`, `TimelineOffset`, `TimelineOrigin`, and related time types should carry the same generic representation/unit parameters.

The timeline object itself should be generic over the same representation and time-unit parameters as its offsets and origins.

That is necessary because the timeline stores offsets and origin information using the same time representation, and because converting or relating timelines should happen through those generic application-owned time semantics rather than through ad hoc external conversions.

### D57. Timeline origins may be relative to other origins and may also remain ungrounded.

`TimelineOrigin` should not be forced to always mean a direct wall-clock instant.

Instead, a timeline origin may be:

- grounded in a wall-clock-like origin
- expressed as a relative offset from some other origin
- effectively ungrounded for tests or synthetic timelines

This allows unit tests and synthetic timelines to use a simple origin such as zero without pretending to know a real wall-clock time, while still supporting future coordinate-translation-style relationships between timelines.

### D58. `TimelineOrigin` should be an explicit small sum type from the start.

The first implementation should not flatten origin semantics into one ambiguous structure.

Instead, `TimelineOrigin` should be modeled explicitly as a small sum type with variants covering at least:

- grounded origins
- relative origins
- ungrounded origins

The payloads can stay minimal at first, but the semantic distinction should exist in the type system from the beginning.

### D59. Relative origins should use a constrained single-hop model at first.

The first implementation should not allow arbitrarily recursive origin graphs.

Instead, relative origins should use a simpler single-hop grounding model so the system can support basic coordinate-translation behavior without immediately taking on the complexity of recursive origin chains, cycle handling, and normalization.

That keeps the model easier to implement, test, and reason about while preserving a path to richer origin composition later if a concrete use case demands it.

### D60. The first implementation should include a minimal explicit API for translating offsets between timelines.

The initial timeline model should not stop at passive data structures alone.

Instead, it should expose a small explicit conversion surface that can translate a `TimelineOffset` from one timeline into another when the relevant origin relationship makes that translation possible.

When no valid relationship exists, the API should return an explicit failure or non-relatable result rather than encouraging consumers to improvise ad hoc translation logic.

This keeps cross-timeline reasoning centralized while still avoiding a large early algebra of ranges, projections, or higher-order operations.

### D61. The first translation API should return a small result object, not just a converted offset.

The first cross-timeline translation API should not collapse its answer to a bare converted offset.

Instead, it should return a small result object that includes:

- the translated offset
- minimal relationship or proof metadata explaining which origin relationship made the translation possible

This keeps translation explainable and extensible without committing the first implementation to a large proof system.

### D62. Translation relationship/proof metadata should be a strongly typed structured value.

The first translation result should not reduce its relationship explanation to a loose descriptive category.

Instead, the relationship or proof metadata should itself be a small strongly typed structured value.

That keeps the translation contract explicit and machine-checkable while still allowing the first implementation to stay minimal.

### D63. The first translation-proof value should expose only the resolved relationship used for conversion.

The first proof shape should not preserve intermediate normalization or origin-rewriting steps.

Instead, it should report only the resolved relationship that actually made the conversion possible.

That keeps the proof value small and aligned with the constrained first implementation. Richer normalization traces can be added later if recursive or multi-step origin composition becomes a real requirement.

### D64. The first implementation should define a `TimelineId`, and relative origins may reference it.

The first implementation should include a distinct `TimelineId` type.

`TimelineOrigin::Relative` may use that identity to express which timeline a relative origin is anchored against.

This gives the time model an explicit handle for cross-timeline relationships without requiring interval/range modeling or a more elaborate identity registry story up front.

### D65. Dedicated public interval/range types should be deferred for now.

The first time-model implementation does not need a public `MomentRange` or similar interval type.

For now, consumers can work with pairs of moments or offsets when they need an implicit range.

That keeps the first implementation focused on offsets, origins, identities, and translation, while deferring inclusive/exclusive boundary semantics and richer range operations until a concrete consumer makes them necessary.

### D66. `TimelineId` should be an opaque GUID-backed type.

`TimelineId` should not use creation-order integers or any other semantically meaningful numbering scheme.

Instead, it should be its own opaque GUID-backed identity type.

That keeps timeline identity aligned with the rest of the system's identity philosophy and avoids coupling timeline identity to storage order, creation order, or presentation concerns.

### D67. Human-facing timeline labels should be deferred and later provided through localization keyed by GUIDs.

The first implementation does not need required human-facing timeline names or labels on the core timeline model.

Instead, presentation-facing naming can be deferred until a concrete UI or query surface needs it.

When that happens, the display representation should follow the same broader philosophy already chosen elsewhere in the app: stable opaque GUID identities in the core model, with culture-aware display names supplied by a localization system keyed from those GUIDs.

### D68. Every timeline should require a `TimelineId` at creation time.

`TimelineId` should not be optional on a created timeline.

Instead, every timeline should carry a `TimelineId` field, and timeline creation should require that identity to be present either by:

- generating it during timeline creation
- accepting it explicitly at timeline creation time

This keeps the model uniform and avoids later state transitions where a timeline becomes identifiable only after creation.

### D70. Timeline creation should auto-generate `TimelineId` by default, with an explicit override path.

The common creation path should make identity assignment easy.

In particular:

- ordinary timeline creation should auto-generate a fresh `TimelineId`
- `Default` for the timeline type should create a timeline with a fresh `TimelineId`
- a separate explicit creation path should allow callers to provide a specific `TimelineId` when needed for tests, imports, or persistence-oriented scenarios

There should be no post-construction path that fills in a missing identity later.

### D71. `TimelineId` should use the same GUID generation mechanism as the app's other opaque IDs.

Timeline creation does not need a special identity-generation strategy.

The default generated `TimelineId` should use the same GUID-generation mechanism already used for the app's other opaque identity types.

If a concrete import, persistence, or deterministic test scenario needs a specific value, that should continue to flow through the explicit supplied-ID creation path rather than by introducing a separate default generator just for timelines.

### D72. The first time implementation should use direct inherent methods rather than a public conversion trait.

The first implementation does not need a separate public trait or interface for time-unit conversion and normalization.

Instead, direct inherent methods on the application-owned time types are sufficient until a real abstraction pressure appears.

This keeps the API smaller and easier to evolve while the first timeline/time model is still being proven out.

### D73. The first time-system implementation should include dedicated precision-validation tests from the start.

The time model is foundational enough that correctness checks should not wait until later.

The first implementation should therefore include dedicated tests for at least:

- exact femtosecond arithmetic
- arena-base plus arena-offset composition into timeline-relative offsets
- cross-timeline translation behavior
- round-tripping between internal representations or conversion helpers
- failure cases where timelines are not relatable

This provides an explicit safety net around the precision and conversion semantics that the rest of the app will depend on.

### D74. The first precision-validation test suite should combine focused unit tests with a small property-style layer.

The initial test strategy should not rely on only one testing style.

Focused unit tests should be the primary safety net for exact arithmetic, explicit edge cases, and hand-picked translation scenarios.

The first implementation should also include a small higher-level property-style suite that synthesizes arbitrary identities and timelines to exercise invariants such as translation consistency and round-tripping.

That generative layer should stay intentionally small in v1 so the first implementation does not turn into a large testing-framework project before the public API settles.

### D75. The initial property-style suite should cover both relatable and intentionally non-relatable timeline combinations.

The generated test coverage should not be restricted to only happy-path relationships.

From the start, the small property-style suite should synthesize both:

- timeline/origin combinations that are relatable and should translate successfully
- timeline/origin combinations that are intentionally non-relatable and should fail in the expected stable way

That keeps the generated coverage aligned with the actual semantics of the translation API, which must model both successful conversions and explicit non-relatability.

### D76. Round-trip translation invariants should be mandatory in the initial property-style suite.

For generated relatable cases, the property-style suite should not stop at one-way translation success.

Instead, v1 should require round-trip invariants so the test suite can detect asymmetry, drift, or other subtle translation bugs early.

Non-relatable generated cases should continue to assert stable failure behavior.

### D77. Invertibility should remain implicit in the constrained v1 translation result model.

The first translation result type does not need an explicit field that declares whether a successful relationship supports round-tripping.

For the constrained v1 relationship model, invertibility should remain implicit and be enforced through the mandatory round-trip test expectations.

If later relationship types introduce lossy, asymmetric, or otherwise non-invertible successful translations, that will be the point to reconsider whether invertibility needs to become an explicit public part of the result model.

### D78. The first translation result type should stay minimal.

The v1 translation result does not need to carry a second normalized or canonical proof artifact.

It should expose only:

- the translated offset
- the small strongly typed resolved relationship metadata already chosen for the result

If later features need proof export, caching, explanation UIs, or more explicit derivation artifacts, the result model can be expanded then.

### D79. The first public translation API should center on a first-class timeline transformation object.

The initial public translation surface should not be a direct "translate from X to Y" method on `TimelineOffset`.

Instead, the API should first construct a transformation object that represents the resolved relationship between two timelines, and that transformation object should then be explicitly applied to offsets.

This keeps the relationship between timelines as its own first-class concept rather than hiding it inside a one-off method call on a value.

### D80. This transformation-object decision is grounded in the design style used by `sguaba`.

The design here is intentionally informed by the `sguaba` crate's approach to coordinate systems and transforms.

In `sguaba`, coordinates and vectors stay typed by the coordinate system they live in, while a first-class transform object represents the relationship between systems and is then applied to values.

In particular, `sguaba`'s README tells users to move between coordinate systems by using transform types from the math module, and the crate docs for `RigidBodyTransform` describe the transform itself as the object that can be combined, inverted, and applied to `Coordinate` or `Vector` values.

That style is a good fit for Teamy Studio's timeline model because it makes the between-timeline relationship explicit, reusable, and harder to misuse than an API that asks every call site to restate the full source/destination pairing ad hoc.

### D81. The first public constructor for timeline transformations should live on `Timeline`.

The initial public API for building a between-timeline transformation should be a method on `Timeline`.

That keeps relationship discovery anchored to the timeline abstraction that owns the origin and identity semantics, while still producing a separate first-class transformation object that can then be applied to offsets.

The transform object remains the thing that gets applied, but `Timeline` is the right first home for asking whether and how another timeline can be related to it.

### D82. The first timeline transformation type should expose only apply-style behavior.

The v1 transformation object should stay minimal.

Its public job is to be applied to offsets, not to expose a larger transform algebra immediately.

Composition and inversion should therefore remain out of the public v1 API for now.

However, the type's documentation should note that composition and inversion are plausible future extensions if later use cases need transform chaining or explicit reverse-relationship operations.

### D83. The first public transform API should use `TimelineTransform`, `transform_to`, and `apply`.

The v1 naming should stay direct and unsurprising.

In particular:

- the transformation type should be named `TimelineTransform`
- the constructor method on `Timeline` should be named `transform_to`
- the application method on the transform object should be named `apply`

That produces a call-site shape like `source_timeline.transform_to(&target_timeline)?` followed by `transform.apply(offset)?`, which is explicit without being overly heavy.

### D84. `Timeline::transform_to` should return a `Result`.

The first transform-construction API should not use `Option`.

If a relationship cannot be formed between two timelines, the API should return a `Result` so that failure remains explicit and can carry structured explanation rather than collapsing into absence.

This fits the broader design direction of making modeled failure states explicit instead of treating them as silent non-results.

### D85. `TimelineTransformError` should preserve as much structured context as practical in v1.

The first transform-construction error should not collapse down to only a bare reason code.

It should carry all of the immediately available structured context that is useful to callers and diagnostics, including at least:

- the source `TimelineId`
- the destination `TimelineId`
- the resolved non-relatability or transform-construction failure reason

If additional small structured context is naturally available at the point of failure and can be included without turning the error into an oversized proof artifact, v1 should preserve that as well.

The goal is for failed relationship resolution to retain the information that callers, logs, and later debugging work are most likely to need.

### D86. `TimelineTransform::apply` should be infallible in v1.

Once `Timeline::transform_to` has successfully constructed a `TimelineTransform`, applying that transform should not introduce a second public failure path.

The transform object should represent a fully validated usable relationship, so `apply` should directly return the translated offset rather than another `Result`.

That keeps construction as the place where relationship failure is modeled, while application remains a straightforward execution of the already-resolved transform.

### D87. `TimelineTransform` should expose source and destination `TimelineId`s directly.

The first transform object should be self-describing rather than carrying only opaque internal data.

In addition to the minimal resolved transform data needed for `apply`, `TimelineTransform` should directly expose:

- the source `TimelineId`
- the destination `TimelineId`

That makes the transform object easier to inspect in diagnostics, logs, tests, and UI-facing tooling without forcing callers to separately retain or re-thread the identity context that was already used to construct it.

### D88. `TimelineTransform` should not expose resolved relationship metadata directly in v1.

The first transform object should stay publicly simple.

It should expose the source and destination `TimelineId`s, but the small resolved relationship metadata used to construct the transform should remain internal in v1.

That keeps the transform self-describing enough for diagnostics and inspection without expanding the public surface to include additional semantic detail before there is a concrete consumer for it.

### D89. `TimelineTransform::apply` should return only the translated offset.

The primary operation on the transform object should stay narrow.

`apply` should return only the translated destination offset rather than a convenience bundle that repeats the source and destination IDs.

When callers need the identity context, they can read it directly from the `TimelineTransform` object itself.

### D90. `TimelineTransform::apply` should borrow the transform in v1.

The first public `apply` operation should take `&self` rather than consuming the transform.

That matches the intended semantics of a reusable transformation object and avoids implying one-shot use.

V1 also does not need parallel owned and borrowed variants.

### D91. `TimelineTransform` should be `Clone`, while timeline IDs and offsets should be `Copy`.

The transform object should be reusable and easy to duplicate when callers want another owned instance, so the first public API should make `TimelineTransform` implement `Clone`.

However, v1 should not additionally commit to `Copy` for `TimelineTransform`.

By contrast, the small identity and value-like timeline types should be cheap value semantics from the start.

In particular, timeline IDs and offset types should implement `Copy`.

### D92. Timeline identity and small value types should live in their own files.

The first implementation should avoid collapsing all timeline types into one large source file.

Small identity and value-centric types should live in dedicated files that keep their responsibilities and names obvious.

For example, types such as `TimelineId` should live in files like `timeline_id.rs` rather than being buried inside a large catch-all module file.

This keeps the relationship between public type names and their implementation locations clear, and it matches the repo preference for file layouts that stay easy to navigate.

### D93. `TimelineTransform`, `TimelineTransformError`, and related public timeline types should each get dedicated files.

The dedicated-file rule should apply not just to IDs and offsets, but also to the new public transform-related types.

In particular, first-class public types such as `TimelineTransform` and `TimelineTransformError` should live in files like `timeline_transform.rs` and `timeline_transform_error.rs` rather than being grouped into a broader catch-all file.

The same principle should hold for other small public timeline types as they are introduced: prefer one clear public type per clearly named file when the type is a first-class concept in the model.

### D94. Timeline model types should live under a nested `timeline/` directory module.

The first public file layout for the timeline model should not stay flat at the crate root.

Instead, the dedicated timeline files should be grouped under a nested `timeline/` directory module from the start.

That keeps the growing timeline model clearly scoped as its own subsystem while still preserving the one-type-per-file preference inside that directory.

### D95. `timeline/mod.rs` should directly re-export the main public types, without a prelude module.

The first public import surface for the timeline subsystem should be straightforward.

Callers should be able to import the key public timeline types from `timeline` via direct re-exports in `timeline/mod.rs`.

However, v1 should not add a separate `prelude` module for this area.

That keeps the public surface convenient without introducing another abstraction layer that callers have to learn or that the crate has to maintain prematurely.

### D96. `timeline/mod.rs` should re-export the full public timeline surface.

The direct re-export policy should not stop at only the happy-path model types.

`timeline/mod.rs` should re-export the full public timeline surface for this subsystem, including related public error types and other first-class public timeline types that callers are expected to use.

That keeps the module boundary coherent: callers learn `timeline` as the public entry point, while the underlying one-type-per-file layout remains an internal organization detail.

### D97. `timeline/mod.rs` should be the single intended stable public import surface.

The direct re-export surface in `timeline/mod.rs` should be the public API boundary that callers are expected to rely on.

Deeper submodules under `timeline/` should not be treated as stable public import contracts in v1.

That preserves freedom to reorganize internal files and submodules later without forcing breaking API changes on callers.

### D98. Small public timeline value types should commit to a consistent derive surface, including `Facet`.

The first public timeline model should make the common trait surface explicit rather than leaving it to drift type by type.

Small value-centric public timeline types such as `TimelineId` and `TimelineOffset` should consistently derive the obvious value traits where semantically valid, including:

- `Debug`
- `Clone`
- `Copy`
- `Eq`
- `Ord`
- `Hash`
- `Facet`

`TimelineTransform` should also derive the trait surface that is semantically valid for its role, including `Debug`, `Clone`, and `Facet`, while still not committing to `Copy`.

The goal is a predictable public model surface whose reflection and inspection capabilities are available from the start rather than added inconsistently later.

### D99. The first public timeline types should not commit to `Display` in v1.

The initial public timeline surface should not freeze a human-facing formatting story too early.

For v1, public timeline types should rely on `Debug` plus explicit accessors and helper APIs rather than committing to `Display` implementations.

That leaves room for later presentation, localization, and formatting decisions without turning an early string form into a de facto long-term contract.

### D100. The first public timeline types should forbid serde and rely on `Facet` instead.

The initial public timeline surface should not merely defer serde; it should explicitly avoid it.

For v1, public timeline types such as `TimelineId`, `TimelineOffset`, and `TimelineTransform` should not derive or otherwise expose serde serialization.

Reflection, structured inspection, and related model-surface needs should go through `Facet` rather than through serde.

That avoids prematurely freezing persistence or interchange formats while keeping the reflection-oriented design centered on the project's chosen metadata and inspection system.

### D101. Public timeline types should implement `Default` only where the default is semantically safe.

The first public timeline surface should not derive or implement `Default` merely because a technical default can be written.

`Default` should be reserved for types where it expresses a real semantically safe default state.

That means some public timeline types may implement `Default` in v1, while others should intentionally omit it when no clear model-valid default exists.

### D102. Public timeline conversions should stay explicit unless they are lossless and semantically obvious.

The first public timeline surface should not add broad `From`/`Into` conveniences just because they are available mechanically.

Conversion traits should be used only where the conversion is both lossless and semantically unsurprising.

When a conversion carries interpretation, changes meaning, or risks blurring the boundary between adjacent model types, v1 should prefer explicit named constructors or methods instead.

### D103. Timeline offset arithmetic should use explicit methods in v1.

The first public timeline offset surface should not expose broad arithmetic operator overloading just for convenience.

Offset arithmetic should use explicit named methods until the semantics have been proven in real usage and the safe operator cases are unambiguous.

That keeps timeline-related math readable and reduces the chance that concise operator syntax hides a semantic mistake.

### D104. Timeline offset arithmetic should expose checked methods only in v1.

The first public numeric surface should not include saturating or wrapping arithmetic variants.

Timeline offset arithmetic should expose only checked methods so out-of-range or otherwise invalid arithmetic remains explicit.

That keeps failure visible and avoids silently turning arithmetic problems into plausible-looking but incorrect values.

### D105. Timeline arithmetic failures should bail and propagate upward.

Checked timeline arithmetic failures should not be treated as locally recoverable convenience cases.

If timeline arithmetic fails, that failure should propagate upward through the caller stack rather than being silently converted into fallback values or absorbed locally.

This keeps arithmetic failure aligned with the broader design preference for explicit failure, explicit bail-out, and no silent corruption of timeline semantics.

### D106. Checked timeline arithmetic should use a dedicated `TimelineArithmeticError` type.

Even though arithmetic failures should bail and propagate upward, they should still remain their own modeled failure domain.

The first public checked arithmetic methods should therefore return a dedicated `TimelineArithmeticError` rather than folding arithmetic failures into a broader shared timeline error type.

That keeps arithmetic failure precise at the point where it occurs, while still allowing higher layers to wrap or propagate it as needed.

### D107. `TimelineArithmeticError` should include operation kind and operand values.

The first arithmetic error surface should preserve the concrete context that caused the failure rather than collapsing down to only a bare reason code.

In addition to the failure reason, `TimelineArithmeticError` should include at least:

- the operation kind
- the left operand
- the right operand when the operation has one

That keeps arithmetic failures far more actionable in logs, debugging, and diagnostics while still remaining a small focused error type.

### D108. Checked timeline arithmetic methods should use standard Rust-style names.

The first checked arithmetic methods should use familiar Rust naming rather than longer domain-redundant names.

For timeline offsets, v1 should prefer names like `checked_add` and `checked_sub`.

The surrounding type context already carries the domain meaning, so longer names that repeat "timeline offset" do not add enough clarity to justify the extra API weight.

### D109. The initial checked arithmetic surface should include `checked_neg`.

Because timeline offsets are signed in the model, negation is part of the fundamental arithmetic surface rather than an optional later convenience.

The first public checked arithmetic API should therefore include `checked_neg` alongside the checked binary arithmetic methods.

That keeps sign inversion explicit and checked without requiring callers to synthesize negation indirectly through other operations.

### D110. The initial public offset API should include small sign and zero helper methods.

The first public offset surface should include a few simple inspection helpers that are cheap, unsurprising, and useful in guard logic.

In particular, v1 should include methods such as:

- `is_zero`
- `is_positive`
- `is_negative`

These helpers improve readability without materially expanding the semantic surface in the way broader operator overloading or conversion conveniences would.

### D111. `TimelineOffset` should expose an associated zero constant.

The first public offset surface should provide a canonical zero value directly rather than forcing callers to build zero through the ordinary constructor path.

V1 should therefore expose an associated constant for zero on `TimelineOffset`.

That keeps comparisons, tests, and guard logic concise while still preserving the explicit typed model.

### D112. `TimelineOffset::ZERO` should be the only canonical zero surface in v1.

The first public offset API does not need both an associated zero constant and a parallel `zero()` constructor.

V1 should use `TimelineOffset::ZERO` as the single canonical zero surface.

That keeps the API smaller and avoids redundant ways of expressing the same value before a concrete need for both forms appears.

### D113. `TimelineOffset` should start with one explicit primary constructor.

The first public offset constructor surface should not immediately branch into multiple convenience constructors for raw units, literals, or alternate entry points.

V1 should instead provide one clear primary constructor and treat that as the canonical way to create offsets.

That keeps the construction story explicit and avoids early ambiguity about which constructor is the real semantic entry point.

### D114. The primary `TimelineOffset` constructor should use a unit-typed generic argument in the style of UOM.

The first public offset constructor should not be named only around the default femtosecond representation.

Instead, the primary constructor should follow the UOM-style pattern where the generic argument implies the time unit being supplied.

In other words, callers should construct offsets through a single primary constructor whose unit is determined by the generic type argument, so call sites can be explicit about seconds, nanoseconds, femtoseconds, and other supported units without requiring a separate constructor name for each one.

That keeps the constructor surface unified while still making the supplied unit explicit at the call site.

### D115. The primary `TimelineOffset` constructor should accept only typed unit markers in v1.

The first constructor surface should not add literal-oriented shortcuts on top of the unit-typed pattern.

V1 should require callers to specify the unit through strongly typed unit markers in the generic position.

That keeps offset construction explicit and prevents unit ambiguity from creeping back in through convenience forms that bypass the chosen UOM-style design.

### D116. The first public time-unit surface should use a curated app-owned set of unit markers.

The initial public unit surface should not expose a broad open-ended universe of possible time units.

Instead, v1 should provide a curated app-owned set of supported time-unit markers.

That keeps the public model intentional, easier to validate, and easier to test, while still leaving room to expand the supported unit set later when concrete use cases justify it.

### D117. The initial curated public time-unit set should be small and focused.

The first app-owned public unit set should not try to cover every plausible time unit.

V1 should start with:

- `Seconds`
- `Milliseconds`
- `Microseconds`
- `Nanoseconds`
- `Femtoseconds`

That set covers human-scale usage along with the app's chosen default high-precision representation without committing the public model to a much larger unit universe prematurely.

### D118. The initial curated unit set should remain limited to decimal-step units.

The first public unit surface should not add domain-specific named units in v1.

The initial curated set should stay limited to the decimal-step units already chosen.

That keeps the public time model generic and avoids smuggling extra domain semantics into the unit layer before there is a concrete need for them.

### D119. Conversions between curated public units should stay explicit.

The first public unit-conversion surface should not rely on implicit or automatic conversion paths.

Conversions between the curated supported time units should use explicit methods so unit changes remain visible at the call site.

That keeps precision changes, representation changes, and unit intent easier to audit in code.

### D120. Explicit unit conversions should use a generic `.get::<Unit>()` pattern.

The first public unit-conversion API should use a generic pattern rather than unit-specific conversion method names.

In particular, callers should request a value in a target unit through a `.get::<Unit>()`-style method.

That keeps the conversion surface compact, matches the unit-typed construction style already chosen, and avoids multiplying the public API with one conversion method per supported unit.

### D121. `.get::<Unit>()` should return the raw numeric representation directly.

The first explicit unit-extraction API should act as the clear boundary where callers intentionally leave the stronger application-owned type surface.

For v1, `.get::<Unit>()` should therefore return the raw numeric representation directly rather than wrapping the result in another application-owned value type.

That keeps the extraction surface simple and makes the escape hatch to raw numeric values explicit without adding another layer of ceremony.

### D122. `.get::<Unit>()` should require exact representability.

The first raw numeric extraction API should not silently round, truncate, or otherwise approximate values when converting into the requested unit.

For v1, `.get::<Unit>()` should require exact representability in the target unit.

That keeps raw extraction aligned with the broader design preference for explicit precision behavior and no silent corruption.

### D123. V1 should omit lossy extraction entirely.

The first public extraction surface should not include a lossy retrieval API.

If a value cannot be represented exactly in the requested unit, v1 should not offer a rounding, truncating, or otherwise approximate extraction path.

That keeps the extraction model simple and makes exactness the only supported numeric escape hatch until a concrete real-world need justifies designing a separate lossy API.

### D124. Exact unit extraction failures should use a dedicated extraction-specific error type.

The first exact extraction API should not reuse `TimelineArithmeticError` for unit-representability failures.

Instead, v1 should use a separate extraction-specific error type for `.get::<Unit>()` failures.

That keeps extraction failure distinct from arithmetic failure, makes caller intent clearer, and avoids turning the arithmetic error surface into an overly broad catch-all.

### D125. The extraction-specific error type should preserve the source unit, target unit, original value, and failure reason.

The first extraction error surface should retain the concrete context needed to understand why exact extraction failed.

In addition to the exactness-failure reason, the dedicated extraction error type should include at least:

- the source stored unit
- the requested target unit
- the original raw value

That keeps exact extraction failures actionable in logs, debugging, and diagnostics without expanding the error into a heavyweight proof object.

### D126. Public `.get::<Unit>()` should be strictly fallible.

The first public extraction API should not expose a panicking variant.

In v1, `.get::<Unit>()` should return `Result<Raw, TimelineUnitExtractionError>`.

If a panicking or unchecked extraction helper is ever useful internally, that can exist as an internal implementation detail, but it should not be part of the public contract.

### D127. The public extraction API should live directly on `TimelineOffset` as `.get::<Unit>()`.

The first public extraction surface does not need a more conversion-oriented or elaborated name.

V1 should keep the extraction API directly on `TimelineOffset` as `.get::<Unit>()`.

That preserves the UOM-style mental model already chosen and keeps the extraction surface compact without introducing extra naming ceremony before a concrete collision or ambiguity appears.

### D128. `TimelineOffset` should keep its storage-unit concept implicit in v1.

The first public offset surface should not expose a separate method for retrieving the storage unit marker.

Construction and `.get::<Unit>()` are already the explicit unit touchpoints, and v1 should rely on those rather than adding a representation-focused query API.

That keeps the public model centered on semantic use rather than on exposing storage details prematurely.

### D129. Raw values should be exposed only through explicit APIs.

The first public timeline value types should not expose their underlying representation through public tuple fields, transparent newtype field access, or similar direct storage access patterns.

If callers need raw numeric values, they should go through explicit APIs such as `.get::<Unit>()`.

That preserves representation freedom while keeping raw-value escape hatches intentional and visible at call sites.

### D130. Public timeline value types should avoid representation-layout guarantees in v1.

The first public timeline surface should not commit to `repr(transparent)` or similar representation guarantees.

V1 should keep representation layout private so the internal storage model can evolve without creating a low-level public contract.

That stays aligned with the broader design preference for semantic stability rather than storage exposure.

### D131. CLI and input parsing should stay outside the core timeline model.

The first public timeline types should not implement Figue-facing parsing or other input-syntax-specific helpers directly.

Parsing and input validation should happen at the boundary, and only then should callers construct the core timeline model types.

That keeps the core timeline surface focused on semantic modeling rather than on front-door syntax concerns.

### D132. Facet proxy forms for time-like values should canonicalize to femtoseconds.

The first public timeline surface should continue to avoid serde and use `Facet` for reflection-oriented needs.

When a time-like value such as a time point, duration, or offset needs a reflected or serialized proxy form through `Facet`, that proxy should canonicalize to the app's femtosecond-based representation.

That keeps proxy forms stable, exact, and aligned with the chosen default precision model instead of introducing multiple reflected wire shapes for different source units.

### D133. GUID-like types should share a common Facet proxy shape.

The first reflection-oriented proxy design should not let each GUID-like type invent its own ad hoc Facet representation.

Instead, GUID-like identity types should share a common proxy shape that they can all use.

That keeps identity reflection consistent across the model, reduces duplicated proxy design work, and makes GUID-like values easier to inspect and process uniformly.

### D134. Canonical Facet proxy commitments in v1 should be limited to time-like values and GUID-like identity types.

The first proxy commitments should stay narrow.

V1 should define canonical Facet proxy shapes for time-like values and for GUID-like identity types, but not for non-ID timeline structures such as `TimelineTransform`.

That keeps the reflection contract aligned with the parts of the model that already have clear canonicalization stories, while avoiding premature exposure of more structurally complex types whose internals are still intentionally constrained.

### D135. GUID-like Facet proxies should use a common hyphenated lowercase string form.

The shared Facet proxy shape for GUID-like identity types should be a canonical string form rather than bytes or a more structural proxy.

In v1, that common proxy should use the standard hyphenated lowercase UUID-style string representation.

That keeps identity reflection easy to inspect, compare, log, and process uniformly across the model.

### D136. GUID-like app ID types should use the UUID-backed Facet form transparently, without extra prefixes or wrapper proxy shapes.

Because Facet already supports `uuid::Uuid`, the app's GUID-like identity types do not need a separate wrapped proxy shape of their own.

Instead, they should reflect through the existing UUID-oriented path transparently and use the canonical hyphenated lowercase UUID string form directly.

They should not add type-name prefixes, tagged wrapper objects, or other extra proxy decoration around that canonical string form.

### D137. Time-like Facet proxies should keep separate category envelopes.

Even though time-like values should canonicalize numerically to the femtosecond representation, v1 should not collapse offsets, durations, and instants into one shared undifferentiated proxy shape.

Instead, each time-like category should keep its own canonical proxy envelope.

That preserves the semantic distinction between different kinds of time-like values while still standardizing the numeric basis used inside those proxy forms.

### D138. The minimal canonical Facet proxy envelope for time-like values should contain only a category tag and the canonical femtosecond value.

The first proxy envelopes for time-like values should stay intentionally small.

Beyond the canonical femtosecond value itself, each envelope should contain only the category tag needed to preserve whether the reflected value is an offset, duration, instant, or other distinct time-like kind.

That keeps the reflection surface semantically meaningful without turning the proxy into a richer serialization contract prematurely.

### D139. Time-like Facet proxy category tags should use stable string tokens.

The first proxy discriminator surface should not introduce a second reflected enum-like contract just to encode the time-like category.

Instead, the category tag in these minimal proxy envelopes should be a stable string token.

That keeps the proxy easy to inspect in logs and tools while remaining self-describing and compact.

### D140. The initial stable string tokens for time-like proxy categories should be `offset`, `duration`, and `instant`.

The first proxy category vocabulary should stay short, semantically direct, and implementation-agnostic.

For v1, the initial stable string tokens should be:

- `offset`
- `duration`
- `instant`

That keeps reflected proxy data easy to inspect while avoiding leakage of more internal or framework-shaped naming into the public reflection surface.

### D141. Time-like Facet proxy envelopes should share one field name for the canonical femtosecond value.

The first proxy envelopes should not vary the numeric field name by category.

Offsets, durations, and instants should all use the same field name for the canonical femtosecond value.

That keeps proxy tooling simpler and reinforces that these envelopes share one canonical numeric basis, while the category tag carries the semantic distinction.

### D142. The two public time-model generic parameters should be named `Repr` and `Unit`.

The first generic parameter should name the underlying numeric representation type, such as `i32` or `i128`.

The second generic parameter should name the semantic time-unit marker, such as `Seconds`, `Nanoseconds`, or `Femtoseconds`.

In public API spellings, these parameters should therefore be named `Repr` and `Unit`, as in `TimelineOffset<Repr, Unit>` and `Timeline<Repr, Unit>`.

`Repr` is preferred over looser names such as transport or precision because the parameter is really the numeric carrier and storage representation, not a serialization contract and not only a statement about precision.

### D69. Identity types should implement `Arbitrary`.

The system's opaque identity types, including `TimelineId`, should implement `Arbitrary`.

That supports the project's testing style and keeps identity-bearing timeline structures easy to synthesize in tests, fuzzing, and arbitrary-data generation workflows.

### D44. `StartupSucceededEvent` should be emitted only when startup validation produced no failure events for the startup path.

Normal app continuation should depend on observing an explicit `StartupSucceededEvent`.

That success event is a consequence of the bootstrap phase completing without any fatal bootstrap failure event that forces termination.

`StartupSucceededEvent` should stay thin and should not mirror the explicit backlink list carried by `StartupFailedEvent`. Backlinking to exact prerequisite events is primarily a failure-path navigation aid.

This makes the startup timeline read as an explicit branch:

- startup failures lead to `StartupFailedEvent` and intentional exit handling
- clean startup leads to `StartupSucceededEvent` and continued app execution

## Current migration gap being addressed

This section described the gap when startup-bootstrap restoration began. It is now partly historical rather than fully current.

At the start of this milestone, the active workspace had:

- only a minimal `tracing_subscriber::fmt()` setup in `src/main.rs`
- no active Figue CLI surface
- no restored `--debug`, `--log-filter`, or `--log-file` behavior
- no restored structured log collector layer
- no restored Tracy integration even though `run-profiler.ps1` still assumes it

The legacy implementation already contains the reference behavior for these surfaces.

As of the active rewrite on May 18, 2026, `--debug`, `--log-filter`, `--log-file`, stderr tracing, optional NDJSON logging, Tracy feature forwarding, and startup timeline events for parsed args/logging/tracing outcomes have partial active implementations. The remaining CLI-specific gap is to replace the current hand-rolled compatibility parser with Figue-backed parsing without splitting the Facet dependency graph.

## Follow-up questions still open

- what that shared canonical femtosecond field name should be in v1

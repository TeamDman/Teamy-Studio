# Timeline-Authoritative Logging Record of Decision

This document captures the May 18, 2026 decisions about how tracing/logging should relate to the event timeline during the rewrite.

## Scope

This decision log covers:

- which system is authoritative for product logs
- how event definitions should express log intent
- how tracing events should be observed into the timeline
- how timeline events should be re-emitted through tracing without creating loops

## Decisions

### D1. The global timeline is the authoritative source of product logging.

The timeline is the canonical substrate for application behavior and observations.

That also applies to logging intent: product-relevant logs should ultimately be represented as timeline events, not only as direct tracing macro calls.

Direct tracing macros are still acceptable as transitional implementation tools during the cutover, but they are not the desired long-term source of truth.

The timeline viewer should therefore view Teamy-owned timelines, not tracing directly.
Tracing records can and should appear in the viewer only after they have been observed into the Teamy timeline model as timeline events.
The viewer should not grow a parallel tracing-specific data path that bypasses the canonical timeline.

### D2. Event definitions should be able to carry explicit log intent and level metadata.

The event system should grow first-class support for whether a published event is intended to surface as a log message and, if so, at what level.

At minimum, the eventual public event-definition surface should be able to express:

- whether the event is log-worthy
- the intended tracing/log level
- enough structured information to produce a stable log rendering from the published event

This keeps log emission policy attached to the authoritative event model rather than scattering that policy across call sites.

Timeline events do not inherently have tracing-style levels.
When an event is intended to participate in logging, level should be explicit decoration on the event definition or nearby publication metadata rather than inferred from the event's existence.
This allows many timeline events to remain non-log events while selected events can be rendered to stderr/file sinks at `trace`, `debug`, `info`, `warn`, or `error`.

### D3. Timeline publication should be able to re-emit log-facing events through tracing.

When a canonical published event has log intent, the system should be able to emit a corresponding tracing record from the published timeline event.

The long-term flow is:

1. feature/bootstrap/runtime code publishes an event into the authoritative timeline path
2. that event is observed as log-worthy according to its definition metadata
3. tracing receives a corresponding log emission derived from the published event

This keeps tracing as an output/view surface rather than the primary owner of application log semantics.

### D4. Tracing output should also be observed back into the event system.

The system should continue to support observing tracing records into the event system so that tracing activity can be incorporated into the timeline.

This means the logging architecture is intentionally bi-directional:

- timeline events may be re-emitted to tracing
- tracing records may be observed back into the event system

That bidirectional model is acceptable as long as loop prevention is explicit and reliable.

### D5. Timeline-to-tracing re-emission must mark observed records so they are not republished.

To prevent infinite loops, tracing records that are produced from already-published timeline events must carry explicit metadata indicating that they are already observed/re-emitted records.

When the tracing-observation side sees that marker, it must not publish a second derived event for the same tracing record.

The intended rule is:

- ordinary tracing records can still be observed into the event system
- tracing records that originated from timeline re-emission must not be published back into the timeline

The loop-prevention marker should be a stable structured tracing field, not only message text.
The currently used `teamy.timeline_reemit = true` field shape is acceptable as the initial marker.
The tracing-to-timeline observation path must check that marker before publishing observed tracing records into the timeline.

### D6. The current trace-level publication logs are a transitional debugging aid, not the final architecture.

The newly added trace-level messages around timeline publication are useful for validating plumbing during the cutover.

However, they are not the end-state design.

The end state is the dedicated timeline-to-tracing bridge described above, driven by event-definition log metadata rather than by ad hoc trace statements in publication call sites.

### D7. The next implementation steps for logging should follow the authoritative-timeline model.

The next logging-focused implementation work should prioritize:

1. defining the event-definition log metadata surface
2. adding the timeline-to-tracing bridge with loop-prevention markers
3. defining how tracing-observed records are published back into the event model
4. migrating direct product logging away from scattered tracing macros toward event-driven publication

### D8. Teamy may grow event-publication convenience macros, but they should publish timeline events first.

Replacing broad use of `tracing::event!`, `info!`, `debug!`, `warn!`, `error!`, and `trace!` with Teamy-owned helpers is a plausible direction, but those helpers should not be thin tracing macro aliases.

The intended direction is:

1. application code publishes a typed Teamy timeline event, possibly through a concise helper
2. the event definition or publication metadata declares whether it is log-worthy and at what level
3. a timeline-to-tracing bridge emits an equivalent tracing record for stderr/file/tracing subscribers
4. the tracing-to-timeline observer ignores records marked as timeline re-emissions

This keeps tracing as an output/integration surface rather than the authority for application behavior.

The exact macro/API shape remains undecided.
Do not commit to a final `event!` replacement surface until the typed event definition metadata, loop-prevention marker, and timeline-to-tracing bridge have landed in code.

## Consequences

- The event system needs a richer definition model than today.
- The tracing subscriber stack will remain important, but more as an integration surface than as the sole owner of product logging semantics.
- Cutover work should avoid baking too much long-term policy into temporary tracing macro call sites.
- Future records of decision about logging should build on this document rather than redefining authority between tracing and the timeline.
- Timeline viewer work should consume Teamy timeline projections even when the visible records originated from tracing.
- Log level policy belongs on Teamy event definitions/publication metadata for Teamy-originated events, and on tracing-observation adapter metadata for externally observed tracing records.

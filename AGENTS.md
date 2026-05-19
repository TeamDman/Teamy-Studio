#using rfc2119
#using this file is primarily intended for consumption by me viewing it in the vscode text editor and by agents consuming it as part of their harness. All grammatical features are for this purpose, and some typos or oddities may be present due to the fallibility of the human condition (while occasionally voice-typing) and yet may serve as useful malignances.

Additional conversation artifact documents are located [here](docs/notes).

Use [.\check-all.ps1](check-all.ps1) instead of `cargo check`

You MUST NOT use alternate `$env:CARGO_TARGET_DIR` values. If a build or something fails because a `.exe` is locked because the user has it open, you SHOULD run `.\stop.ps1` which will terminate the program so you may continue; we are in building phase, no important data will be lost.

---

Ahoy, TeamDman (or Teamy) here. This repo is a collection of experiments with custom terminals, native rendering, rust, and various OS integrations.

This project is currently undergoing a transition from a [legacy](legacy/) monolith application to a new architecture because of the slow compile times.

Taking the rewrite opportunity, we are embracing a new architecture to better align with my desires.
The old implementation successfully used tracing subscribers to track events, and incorporated guidance from tracy to create our own custom nanosecond-precision event timeline viewer.

We use [run-profiler.ps1](run-profiler.ps1) to run our program and subsequently display the tracy-capture results with tracy-profiler, that way we can observe our own app's performance easily until our own event viewer has reached parity.

Tracey-with-an-E was previously used to map markdown requirements to source markers. That history is preserved under [docs/reference/tracey](docs/reference/tracey), but Tracey is no longer an authoritative development gate. Current autonomous development MUST be guided by the newest relevant record-of-decision documents, the current code, and [event-cutover-plan.md](docs/notes/event-cutover-plan.md).

The legacy implementation on `main` used `teamy-figue` with a deliberately compatible Facet stack (`figue = { package = "teamy-figue", version = "2.0.1", features = ["arbitrary"] }` with `facet = "0.44.1"`). When restoring Figue-backed CLI parsing in the active rewrite, preserve a single compatible Facet graph instead of blindly upgrading Figue or Facet.

The most important files right now are:
- [event-cutover-plan.md](docs/notes/event-cutover-plan.md)
    - MUST keep this file up to date with our latest progress
    - Next steps MUST be easily identified with explicit implementation guidance
- [event-cutover-record-of-decision.md](docs/notes/event-cutover-record-of-decision.md)
- [startup-bootstrap-record-of-decision.md](docs/notes/startup-bootstrap-record-of-decision.md)
- [timeline-authoritative-logging-record-of-decision.md](docs/notes/timeline-authoritative-logging-record-of-decision.md)
- [development-workflow-record-of-decision.md](docs/notes/development-workflow-record-of-decision.md)

These are the latest notes where I have provided answers to questions designed to elicit the desired shape of the future of the project.

If any substantive guidance is given by the user or new requirements are surfaced, the agent SHOULD ensure those directives are persisted in a record-of-decision document, and such document MUST be referenced by [AGENTS.md](./AGENTS.md) (you may introduce new references).
The complexity of this approach is intentional and should become self-evident if it is not already.


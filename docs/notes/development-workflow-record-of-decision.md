# Development Workflow Record of Decision

This document captures the May 18, 2026 decision to demote Tracey-with-an-E from an authoritative specification gate and to make record-of-decision documents the primary steering surface for autonomous development.

## Scope

This decision log covers:

- Tracey specification status
- how future agents should choose next implementation steps
- how older Tracey requirement markers should be treated
- the legacy Figue/Facet dependency constraint

## Decisions

### D1. Record-of-decision documents are the primary steering surface.

The current rewrite should be guided by record-of-decision documents and the current code, not by treating `docs/spec/` plus Tracey coverage as the authoritative source of truth.

The project may still keep product/spec documents as useful historical and planning material, but autonomous development should prefer the newest relevant record-of-decision documents when determining what direction is current.

### D2. Tracey is preserved as project history, not active authority.

Teamy Studio previously used Tracey-with-an-E to map requirements from markdown specs into source-code markers such as `tool[...]`, `repo[...]`, and similar requirement IDs.

That history is useful and should not be erased. Existing markers and old spec documents can remain as breadcrumbs for understanding legacy intent.

However, Tracey should no longer be a required quality gate for the active rewrite.

### D3. The Tracey config is demoted to reference documentation.

The old top-level Tracey config has been moved from `.config/tracey/config.styx` to `docs/reference/tracey/.config/tracey/config.styx`.

That location is intentional: it preserves the old requirement map and its original `.config/tracey` shape without presenting it as a live root-level project contract.

### D4. `check-all.ps1` should not run Tracey.

The repository validation command remains `.\check-all.ps1`, but it should validate the active Rust workspace through formatting, clippy, builds, and tests rather than invoking Tracey.

If someone wants to inspect old Tracey coverage, they may do so manually against the reference config, but that is diagnostic archaeology rather than the default development gate.

### D5. Future requirements should become decision records first.

When new substantive requirements or architectural direction appear, agents should write or update a record-of-decision document under `docs/notes/` and make sure `AGENTS.md` references it.

The working algorithm is:

1. read `AGENTS.md`
2. read the referenced current record-of-decision documents
3. inspect the current code
4. update `event-cutover-plan.md` with status and next steps
5. implement the next small aligned change
6. validate with `.\check-all.ps1`

This deliberately relies on agent-managed markdown and current code reality rather than a programmatic requirement mapper.

### D6. Legacy Tracey-era notes should be interpreted through the current decision records.

Older notes may say that new behavior "must" update Tracey specs or markers.

For the active rewrite, those instructions are superseded by this record. Preserve the useful intent, but express current commitments in record-of-decision documents and implementation tests.

### D7. The legacy Figue CLI is a dependency-compatibility reference, not a drop-in migration.

The legacy implementation on `main` used `figue = { package = "teamy-figue", version = "2.0.1", features = ["arbitrary"] }` with `facet = "0.44.1"`.

That pin mattered because newer Figue/Facet combinations can split or mismatch the Facet dependency graph.

The active startup crate should not blindly add or upgrade Figue. Restoring Figue-backed parsing should first preserve a single compatible Facet stack, or explicitly document a new compatibility decision before changing dependency versions.

## Consequences

- Tracey requirement markers may remain in source and docs as historical context.
- `docs/spec/` is no longer the dominant authority over active development.
- Current records of decision plus the codebase are the dominant inputs for planning.
- `check-all.ps1` no longer depends on a globally installed `tracey` executable.
- Future cleanup may remove or rewrite obsolete Tracey-era instructions in older notes, but that is not required before continuing the architecture cutover.

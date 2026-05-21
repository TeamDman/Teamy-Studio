Each subcommand must have its own directory module.
Each subcommand implementation must live in a new `{}_{}_{}_cli.rs` file that `mod.rs` re-exports to ensure fuzzy finders can find the file easily.

Additional documentation that captures historical decision making is located [here](docs/notes).
The active crate-refactor plan is located [here](plan.md).

## Current Refactor Mission

We are breaking the current monolith into separate crates to reduce compile times **without losing any behavior or functionality**.

This is a preservation-first refactor, not a rewrite.

## Preservation Rules

You MUST PRESERVE CODE AS IS TO THE GREATEST EXTENT POSSIBLE.

The current code is the result of hundreds of compounding tweaks from first attempts at implementing features. Omitting or "simplifying" seemingly insignificant lines can, has, and will cause regressions.

Because of that:

- Prefer exact moves over rewrites.
- Prefer thin compatibility re-exports over broad call-site churn.
- Do not make speculative cleanups during extraction.
- Do not redesign APIs during extraction unless required for compilation.
- If a seam is ugly, move it intact first.
- If a change is not required for the active extraction, leave it alone.

## Extraction Strategy

- When preparing a release profiling workflow or handing off to [run-profiler.ps1](run-profiler.ps1) with `-Release`, warm the same artifacts with `cargo build --profile profiling` so the profiler wrapper does not pay a fresh build after test-only validation. The wrapper defaults to debug profiling when `-Release` is not passed.

## First-Class Priorities

1. Preserve behavior.
2. Preserve code shape where practical.
3. Reduce compile-time coupling through crate boundaries.
4. Improve architecture only after the code is safely moved.

Use `cargo clippy -- -D warnings` for fast iteration during this refactor instead of `cargo check`.

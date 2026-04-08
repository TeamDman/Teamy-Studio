Each subcommand must have its own directory module.
Each subcommand implementation must live in a new `{}_{}_{}_cli.rs` file that `mod.rs` re-exports to ensure fuzzy finders can find the file easily.

Tracey spec is located [here](.config\tracey\config.styx), this is where the observable behaviour of the application is formalized.

Additional documentation that captures historical decision making is located [here](docs/notes).

Use [.\check-all.ps1](check-all.ps1) instead of `cargo check`
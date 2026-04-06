# Tool Standards

This document translates the applicable command-line and logging standards into local tracey requirements for Teamy Studio.

## Command Line Interface

tool[cli.version.includes-semver]
The CLI must report the semantic version from the project manifest.

tool[cli.version.includes-git-revision]
The CLI must report the current git revision alongside the semantic version.

tool[cli.help.describes-behavior]
The CLI help output must describe that Teamy Studio launches a workspace run by default and offers explicit window commands.

tool[cli.help.describes-workspace]
The CLI help output must describe the workspace command group and its list, show, create, and run subcommands.

tool[cli.help.describes-shell]
The CLI help output must describe the shell command group and its default-shell management subcommands.

tool[cli.help.describes-self-test]
The CLI help output must describe the self-test command group.

tool[cli.help.describes-argv]
The CLI help output must describe the command line arguments accepted by the program.

tool[cli.help.describes-environment]
The CLI help output must describe environment variables that affect program behavior.

tool[cli.surface.window]
The CLI must expose a `window show` command that launches the main application terminal window.

tool[cli.surface.workspace]
The CLI must expose a `workspace` command surface that supports list, show, create, and run subcommands.

tool[cli.surface.shell]
The CLI must expose a `shell` command surface that supports inline launch and default-shell management subcommands.

tool[cli.surface.self-test]
The CLI must expose a `self-test keyboard-input` command surface.

tool[cli.help.position-independent]
The CLI must support requesting help from nested command positions.

tool[cli.global.debug]
The CLI must accept a `--debug` global flag.

tool[cli.global.log-filter]
The CLI must accept a `--log-filter` global option.

tool[cli.global.log-file]
The CLI must accept a `--log-file` global option.

## Logging

tool[logging.stderr-output]
The program must send logs to stderr.

tool[logging.file-path-option]
The program must support optionally writing logs to a user-provided path on disk.

tool[logging.file-structured-ndjson]
When the program writes logs to disk, the file output must use a structured NDJSON representation.

tool[logging.filter.from-env]
When `--log-filter` is omitted, the program must allow `RUST_LOG` to provide the tracing filter.

tool[logging.filter.defaults]
When no explicit log filter is provided, logging must default to `debug` when `--debug` is set and `info` otherwise.

tool[logging.filter.debug-conflicts-with-log-filter]
The program must reject using `--debug` together with `--log-filter`.

## Quality Gate

tool[tests.exclude-tracy-feature]
The repository quality gate must run tests without enabling the `tracy` feature.

tool[tests.avoid-tracy-firewall-prompt]
The repository quality gate must avoid enabling `tracy` during tests because Tracy can trigger a Windows firewall prompt that is inappropriate for routine automated validation.
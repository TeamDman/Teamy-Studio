# Tool Standards

This document translates the applicable command-line and logging standards into local tracey requirements for Teamy Studio.

## Command Line Interface

tool[cli.version.includes-semver]
The CLI must report the semantic version from the project manifest.

tool[cli.version.includes-git-revision]
The CLI must report the current git revision alongside the semantic version.

tool[cli.help.describes-behavior]
The CLI help output must describe that Teamy Studio launches the main application window by default and offers explicit window commands.

tool[cli.help.describes-argv]
The CLI help output must describe the command line arguments accepted by the program.

tool[cli.help.describes-environment]
The CLI help output must describe environment variables that affect program behavior.

tool[cli.surface.window]
The CLI must expose a `window show` command that launches the same window as the no-argument startup path.

tool[cli.help.position-independent]
The CLI must support requesting help from nested command positions.

## Logging

tool[logging.stderr-output]
The program must send logs to stderr.

tool[logging.file-path-option]
The program must support optionally writing logs to a user-provided path on disk.

tool[logging.file-structured-ndjson]
When the program writes logs to disk, the file output must use a structured NDJSON representation.

## Quality Gate

tool[tests.exclude-tracy-feature]
The repository quality gate must run tests without enabling the `tracy` feature.

tool[tests.avoid-tracy-firewall-prompt]
The repository quality gate must avoid enabling `tracy` during tests because Tracy can trigger a Windows firewall prompt that is inappropriate for routine automated validation.
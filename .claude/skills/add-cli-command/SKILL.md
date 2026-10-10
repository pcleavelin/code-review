---
name: add-cli-command
description: Add or change a review CLI command (the agent's interface, also run from the GUI's Console). Use as a step of add-feature.
---

# Add a CLI command

1. `crates/cli/src/wire.rs`: the clap variant with its `about` line (the help line) and its
   arguments (1-based lines and pull request addresses are parsed here), and every message the
   command prints, success and each error (`impl Display for Failure` lives here
   too). Messages exist only in this file.
2. `crates/cli/src/convert.rs`: `request()` turns the wire command into a `Request` of domain
   values (lines to `domain::Line`, addresses to `PullAddress`, `--old` to `Side`).
3. The command's arm in `exec.rs`: reach a repository only through `Desk::find` and change
   a review only through `domain::Review` methods; if one is missing, add it to `domain` (or
   `session`, when it orchestrates I/O) with a unit test.
4. The scenario: run the command, every error it can give, and a follow-up command that shows
   the effect in the fitting function of `crates/review/tests/cli.rs` (`add-test-scenario`).
   `tests/commands.rs` fails until a scenario runs the new command.
5. `finish`.

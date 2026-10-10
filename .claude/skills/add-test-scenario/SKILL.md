---
name: add-test-scenario
description: Add or extend a CLI scenario in crates/review/tests, or a scripted GUI run. Use when a feature has no scenario that triggers it, or when a change's effect should be shown by the real binary.
---

# Add a test scenario

A scenario drives the real binary against a generated fixture and asserts what it prints.
The behaviour a change adds is stated by a test written before the code.

1. Prefer extending an existing scenario of the same area; a new one costs a fixture setup.
2. CLI: a function `pub(super) fn <name>(fixture: &Fixture)` in the `scenario` module of
   `crates/review/tests/cli.rs`, then add `<name>` to the `scenarios!` table (a hand-written
   `#[test]` there is refused by the linter). `Fixture::new` builds a forge repository with a
   `feature` branch and a clone whose `refs/code-review/1/{base,head}` are fetched, so every
   offline command works on pull request 1; extend it when a scenario needs more. Every
   outcome of a command is asserted (success and each error, with its exit code), followed by
   a command that shows the effect. Nothing in a scenario talks to a real forge.
3. GUI: there is no GUI scenario table yet (TODO.md). Write a script (CLAUDE.md, "Testing the
   GUI"), run it with `REVIEW_SCRIPT=<file> target/release/review`, and read its `DUMP`
   lines and screenshots. A gesture at an element out of view is refused; to check that
   something is not shown, use `absent <id>`.
4. Run it: `cargo test -p review --test cli <name>`, and `cargo test -p review --test commands`.
5. `finish`.

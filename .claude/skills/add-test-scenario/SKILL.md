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
3. GUI: a function `pub(crate) fn <name>() -> Scenario` in `crates/review/tests/common/gui.rs`,
   added to the `gui_scenarios!` table there. Its `script` (CLAUDE.md, "Testing the GUI")
   starts with `OPEN` to open the fixture's pull request, writes screenshots to `{shots}/`,
   and aims inside an element with `<<DUMP rect <id>|dx,dy>>`; its `shows` are the `DUMP`
   lines (or parts of them) the run must print, so the scenario fails when the behaviour does.
   The fixture (`common/mod.rs`: `BASE`, `FIRST`, `SECOND`, the forge's JSON) is shared by
   every scenario; extend it when a scenario needs more. A gesture at an element out of view
   is refused; to check that something is not shown, use `absent <id>`. A GUI scenario is
   written with the change and runs at its very end (`finish` step 6); read the screenshots
   then (the failure message names the folder).
4. Scale: behaviour that runs per changed file, per pull request or per line gets a step in
   the scale scenarios, which run on `common::large_fixture` (`common/large.rs`: a pull
   request of thousands of changed files): `scale_review` / `scale_inbox` in the `at_scale!`
   table of `cli.rs`, `scale` in `common/gui.rs`. Each step's bound is its deadline: measure
   the step (`-- --nocapture` prints its `SCALE` line), give it about three times that, and
   say both numbers in the commit message. Without `REVIEW_BIN` the scenarios run the debug
   build (`CARGO_BIN_EXE_review`), so a before-and-after comparison sets `REVIEW_BIN` to a
   release build on both sides.
5. Every run of the binary, and every program the harness starts, has a deadline
   (`common::COMMAND_DEADLINE`, `common::WINDOW_DEADLINE`, or the scale step's bound): on expiry
   the harness kills the run's process group and fails naming the scenario, the step and
   each process it was still waiting on. A hang therefore fails the gate instead of
   stalling it; start the binary through `common::run_within`, never with `Command::output`.
6. Run it: `cargo test -p review --test cli <name>` or `cargo test -p review --test gui
   <name>`, and `cargo test -p review --test commands`.
7. `finish`.

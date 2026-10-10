---
name: add-feature
description: Add anything a user can do with the review tool - a CLI command, a GUI button, key binding, gesture or panel action. Use whenever a change gives the user a new function or a new way to trigger one.
---

# Add a user-facing function

A function is named where its surface is built, once: a command by its clap variant and
`about` line in `crates/cli/src/wire.rs`, a GUI control by its element and words in
`crates/gui/src/ids.rs`, a key chord in `crates/gui/src/keys.rs`, a palette entry in the
`OFFERED` table of `crates/gui/src/palette.rs`.

1. `understand`: the request restated, why it is needed, how it is meant to work.
2. `prototype`: several designs built and run, one kept. The steps below finish the one
   that was kept.
3. The handler: one function that performs it.
   - A command: follow `add-cli-command`.
   - A GUI function: follow `add-gui-element`.
   - Anything that changes a review is a method on `domain::Review` first, anything that talks
     to git or a forge goes through `session::Repo` or `session::Desk` (`add-domain-type`,
     `add-data-source`).
4. Tests: the behaviour stated before the code, as a unit test of the handler or domain method;
   then a CLI scenario line for every outcome (success and each error), or a GUI script that
   triggers it and a dump that shows its effect (`add-test-scenario`).
5. The case (CLAUDE.md, "Every feature makes its case"), as `understand` and `prototype`
   completed it, is the body of the commit that adds the feature.
6. `finish`.

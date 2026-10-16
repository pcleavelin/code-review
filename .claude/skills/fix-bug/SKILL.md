---
name: fix-bug
description: Fix behaviour that differs from what the owner, CLAUDE.md, or a feature's summary says it should be. Use for every bug report, including the items in TODO.md.
---

# Fix a bug

1. `understand`: the restatement, the spec the behaviour breaks, and the command or script
   that reproduces it.
2. Pin the failure before touching the code: a unit test in the crate at fault that fails
   on the current code, or, for behaviour only the binary reaches, a scenario step
   (`add-test-scenario`) that fails before the fix. A GUI scenario is written here and run at
   the end with every other GUI run (`finish` step 6), on the parent revision's build first to
   watch it fail.
3. Find the cause, not the symptom: follow the data from where the output goes wrong back to
   where it is read (the `io-*` crate's `wire.rs`, then `convert.rs`, then the caller). Fix it
   there, once, where every caller passes through. A fix that also changes what another feature
   does is a behaviour change the owner hears about first.
4. If a lint, a type or a skill step should have kept this bug from being written, run
   `tripped`.
5. If the bug is an item in `TODO.md`, remove the item.
6. `finish`. The commit message states what was wrong, what the spec says, and the cause.

---
name: refactor
description: Change code without changing behaviour - move, rename, restructure, retype. Proves it with the existing tests and scenarios before and after.
---

# Refactor with proof

1. Start from a committed parent: the parent revision (`@-` in jj) is the behaviour the
   refactor must keep. Run `cargo xtask gate --full` there; it must be green.
2. Make the change.
3. `cargo xtask gate --full`: every unit test and CLI scenario passes unchanged. A test that has
   to change is a behaviour change: undo the part that caused it, or, if the change is wanted,
   it is not a refactor: split it into its own commit. GUI code: run the scripts that drive the
   area (`add-gui-element` step 7) on both builds and compare the `DUMP` lines and screenshots.
   (`cargo xtask parity` will do this once its harness is ported, TODO.md.)
4. `finish`.

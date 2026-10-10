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
   it is not a refactor: split it into its own commit. Then `cargo xtask parity`: every GUI
   scenario and the CLI transcript, played by the parent revision's build and this one, print
   the same `DUMP` lines and draw the same screenshots; it lists each one that differs, with
   both outputs kept.
4. `finish`.

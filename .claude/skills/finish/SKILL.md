---
name: finish
description: Finish any change to this repo before stopping or committing. Runs the gate, reviews names and tests, and commits. Use at the end of every task that edited code or tests, and whenever the Stop hook reports the gate red.
---

# Finish a change

The Stop hook runs the gate and blocks until it is green; commits are refused while it is red.
A stop while background tasks are still running, and a subagent's stop, are not gated: the
session's first stop with nothing in flight is. This is the order that gets it green in one pass.

1. The quick checks, which fail in seconds where the gate takes minutes: `cargo fmt`,
   `cargo clippy -p <each crate touched> --all-targets`, `cargo xtask lint`, `cargo xtask api`
   when a library crate's public items changed, and the unit tests and CLI scenarios the change
   touches (`cargo test -p <crate>`, `cargo test -p review --test cli <scenario>`). Fix until
   they pass. Nothing that opens a window (a GUI scenario, a script, a screenshot, `gate --full`)
   runs before step 6.
   Each lint message states the rule and the one way to satisfy it; do not look for a way
   around a message, do what it says. If a rule itself looks wrong for the case, stop and use
   the `tripped` skill instead of working around it. Run `cargo xtask gate --full` once, in
   step 7, not after every fix. A change to markdown alone (README, TODO, CLAUDE.md, the
   skills) or to pictures runs no gate and no checks: nothing compiles it.
2. Naming. Spawn one Sonnet subagent with the change (`jj diff --git`) and this brief: for each
   name the diff introduces (types, variants, functions, fields, modules), find how the code
   already names the same concept (`api/*.api`, the Concepts table in CLAUDE.md, a grep) and
   report only real inconsistencies: one concept under two names, one name for two concepts,
   an abbreviation where the code spells the word out, a name that contradicts the glossary.
   It suggests renames with the reason for each and edits nothing. Apply the ones you agree
   with (a rename that reaches code outside the diff is a `refactor`), and say in your reply
   which you declined and why. Skip this step when the diff introduces no names.
3. Tests. Spawn one Sonnet subagent, in the same turn as the naming one, with the change and
   this brief: for each test the diff adds or changes (a unit test, a scenario step, an
   assertion), name one plausible mistake in the code under test that makes it fail. A test with
   no such mistake is tautological:
   - its expected value comes from the code under test, or from a copy of its logic;
   - it asserts a literal, a constant, or what a type already guarantees (a constructor's
     value read back through its accessor, a list's length restated as a number);
   - it checks a stub or fixture the test built, not what the code did with it;
   - it runs the code and asserts nothing about the result.
   It reports each tautological test with the reason and edits nothing. Rewrite each one to
   pin behaviour the code could get wrong, or delete it; a tautological test is never kept.
   For each test that survives the review, break the line it covers (flip the condition,
   return the default), watch it fail, and restore the line. Skip this step when the diff
   touches no tests.
4. The case. When the diff gives the user a new function (a command, a control, a chord), spawn one Sonnet subagent, in the same
   turn as the two above, with the case you will put in the commit body and CLAUDE.md's "Every
   feature makes its case", and this brief: judge the case as the owner would before agreeing
   to build the feature. Report each part that is missing, asserted without evidence, or
   already answered by something the tool has, and say whether the case convinces. It edits
   nothing. Rewrite the case until it does; a feature whose case cannot be made goes to the
   owner instead of into a commit.
5. `cargo xtask api` if a library crate's public items changed, and include the `api/` diff.
6. A GUI change: the first and only GUI runs of the change. The pinning scenario on the parent
   revision's build (it fails), then the change's GUI scenarios (`cargo test -p review --test
   gui <scenario>`), then the script that shows it (`add-gui-element` steps 6 and 7) on the
   release build, reading the screenshots. A fix found here goes back through steps 1 to 5
   before the GUI runs again.
7. `cargo xtask gate --full` once more, then commit (`jj commit -m ...`). The message says
   what changed and why, for someone who did not see the session; a new feature's message
   body is its case. A change to anything that runs per changed file, per pull request or
   per line states in the message the scale scenarios' step times and slowest frame
   (`cargo test -p review --test cli scale_ -- --nocapture`, `--test gui scale -- --nocapture`
   print them as `SCALE` lines), before and after the change.

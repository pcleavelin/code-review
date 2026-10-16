---
name: understand
description: Restate a feature request or bug report in your own words, then learn why it is needed and how it is meant to work, before any design or edit. Use first for every feature request and every bug fix.
---

# Understand the request

Nothing is designed or edited until this is done. A misreading caught here costs one
message; caught after the change, it costs the change.

1. Restate. Before reading any code, write in the reply, in your own words rather than the
   owner's:
   - a feature: who uses it (CLAUDE.md, opening: the human through the GUI, the agent through
     the CLI), what they do, what they see afterwards, and what stays as it is;
   - a bug: what happens, what should happen instead, and the steps that get there.
   Name every point where the request can be read two ways. A word the owner chose for a
   user-facing concept is not ambiguous because a code rule shares it: lint L1 forbids
   comments in the source, and a feature where the reviewer comments is still a comment.
2. Why it is needed. Find the goal it serves and what it costs the user to go without it:
   - CLAUDE.md, opening (roles, goals, non-goals) and "Why it is built this way";
   - `TODO.md`: the open issues agent sessions and the owner logged because they cost them
     time, what is planned, and each deferred item with the trigger that pulls it in;
   - the `jj log` descriptions of the commits that built the area (a feature's case is the
     body of the commit that added it).
   A request that serves no goal, or runs into a non-goal (a forge-specific feature in the
   core, review state in the repository), goes to the owner before anything else. For a
   feature, write its case in the reply (CLAUDE.md, "Every feature makes its case"): the user
   and the moment, the goal, the cost today shown by a run of the tool in step 3, and why
   nothing the tool has answers it. A case that rests on "it would be nice" or "it looks good"
   is not finished; the owner hears what is missing.
3. How it is meant to work. The spec is, in this order: the owner's words, CLAUDE.md, the
   command's `about` in `crates/cli/src/wire.rs` or control's words in `crates/gui/src/ids.rs`, the scenarios in
   `crates/review/tests/cli.rs` that drive it. Then see the current behaviour yourself: run the
   CLI command, or a unit test of the code that draws or answers it. The GUI runs only at the
   very end of a change (`finish` step 6), never here. A bug is reproduced here, with the exact
   command or test that shows it; a bug only a GUI run shows is pinned by a scenario that runs
   at the end (`fix-bug` step 2). One that does not reproduce goes back to the owner with what
   you ran. Reading a forge is fine on any
   public repository; writing to one (submitting a review) only on a pull request the owner
   named for it.
4. Restate again with what steps 2 and 3 taught: the need in one sentence, the behaviour as
   the user will meet it, the scale it meets (a pull request of thousands of changed files,
   a file of tens of thousands of lines, an inbox of hundreds of pull requests: whichever it
   runs once per), and each place the first restatement was wrong. If a question is
   left that only the owner can answer (which of two readings, a conflict with CLAUDE.md,
   a reported bug that is the spec's own behaviour), ask it and wait for the answer. Where a
   new kind of stored data lives is always the owner's: on the machine, or sent to the forge.
   CLAUDE.md says what is stored today and does not decide it.
5. The need from step 4 has one home: for a feature, its case is the body of the commit that
   adds it; for a bug, the fix's commit message.
6. Go on with `prototype` for a new feature, `fix-bug` for a bug, or the skill for the kind
   of change.

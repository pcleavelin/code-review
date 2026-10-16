# code-review

A local dashboard for reviewing pull requests, whatever forge hosts them (GitHub, GitLab,
Forgejo/Gitea). A review is frictionless (pick a pull request, read each changed file with the
code around it navigable, mark it seen, go to the next unseen one) and stateful (what was seen
and every draft comment survive restarts; a file seen before turns unseen again when the pull
request changes it). The code of the pull request is read the way codemap reads code: the
Source view shows the head of the pull request with go to definition, hover and references
answered by the language's server on that head.

- **The human** reviews in the GUI: `review` with no arguments opens the Inbox (every open
  pull request of every pinned repository, filtered by words and by standing), then Changes,
  the Source view with the diff and the forge's threads on it, the Review tab with the
  description, the conversation and the submit.
- **The agent** (and anyone in a terminal) uses the CLI: the same review, the same state, the
  same commands the GUI's Console runs.

Goals: reviewing a pull request takes less friction here than in the forge's web page; nothing
about a review in progress is lost or has to be remembered; the code under review is navigable
like code in an editor; one small native app that works with any forge.

Never: editing source (it reads code, it does not write it), a web or remote surface of its
own, review state committed into the repository, a forge-specific feature in the core (what
differs per forge lives in `io-forge` alone). What is planned or deferred, and what would pull
a deferred item in, is in `TODO.md`.

**Every feature makes its case.** A feature is built only after a written case that would
convince the owner; that it looks good, or would be interesting to have, is not a case. The
case names the user (the human in the GUI or the agent in the CLI) and the moment they meet the
problem; the goal above that it serves; what going without it costs them today, seen in a run
of the tool rather than supposed; why nothing the tool already has answers it; and, once the
prototypes have run, why the kept design beats the others. It is the body of the commit that
adds the feature. A request whose case cannot be written goes back to the owner with what is
missing.

Every change ends with the `finish` skill. The Stop hook runs `cargo xtask gate` and keeps the
session going until it is green, and commits are refused while it is red; each failure says
what to do. Every feature request and bug fix starts with the `understand` skill, and every
new feature is chosen from several built by the `prototype` skill. The other skills
(`add-feature`, `fix-bug`, `add-cli-command`, `add-gui-element`, `add-domain-type`,
`add-data-source`, `add-test-scenario`, `refactor`, `tripped`) are the steps for each kind of
change. When the owner had to step in and steer, `debrief` turns each step-in into a change to
these skills, the checks or this file, never into one agent's memory, so the next agent builds
what the owner meant without being told.

Work happens in a jj workspace of its own outside the repo directory
(`jj workspace add <dir> --name <name> -r <base>`), committed under a bookmark: the owner
uses the default working copy to try branches and moves its `@` at any time. Every file in
the repo is the agent's to change when a task needs it, CLAUDE.md, the skills, xtask, the
crate graph and the settings included; change it and say so in the reply rather than
proposing and waiting.

## Axioms

Nothing tells an agent ahead of time how code must be shaped: a lint or a type rejects every
other shape and its message names the one way, and a skill carries every multi-step
procedure. An agent learns a rule by breaking it and reading the error, at the moment the
rule applies.

1. **One way.** Each piece of data has exactly one function that reads it from outside the
   program and one that writes it out, and each concept has exactly one type. A second way to
   get or send the same data does not compile or does not pass the gate.
2. **The architecture is checked, not described.** Layering, data ownership and the I/O
   boundary are enforced by the compiler where it can (crates, privacy) and by the
   architecture linter (`cargo xtask lint`) where it cannot. Nothing about the shape of
   the code is left to reading.
3. **No comments.** What a comment would say goes into a type, a name, a test, or this file.
   A comment is text the compiler does not check; when it drifts from the code, a reader
   trusts the wrong one.
4. **Types carry meaning.** Every value that means something has its own type, even when it
   is one integer. Data cannot be handed to a parameter that was not shaped for it.
5. **One description.** Each concept has one name in code, CLI, GUI and docs, and each kind of
   knowledge has one home: what a command does is its clap `about` in `crates/cli/src/wire.rs`,
   what a GUI control does is its words in `crates/gui/src/ids.rs`, why it
   exists is the commit that added it, the vocabulary and the reasons behind the design are
   below.
6. **Every test can fail.** A test names a mistake in the code it would catch. One whose
   expected value comes from the code under test, or that asserts what a type already
   guarantees, passes on every implementation and is deleted.
7. **Nothing is done until the gate is green.** The rules are enforced after every agent
   turn and before every commit.

## Concepts

| Term | Meaning |
|---|---|
| **Forge** | The service hosting the repository's pull requests: its kind (GitHub, GitLab, Forgejo), host and repository slug, read from the `upstream` remote, else `origin`. `REVIEW_FORGE` names the kind for a host the URL does not give away. |
| **Pull request** | A forge's request to merge a head branch into a base branch, by number. GitLab's merge requests are pull requests here. |
| **Base, head** | The two commits a review compares: the merge base of the base branch and the head, and the head. Fetched into `refs/code-review/<n>/base` and `/head`, so every command after `open` works offline. |
| **Tree** | The head of a pull request checked out in a git worktree of its own, `<git common dir>/code-review/trees/<n>`. The Source view, the index and the language servers read it. |
| **Change** | One file the pull request changes: path (and old path when renamed), kind, hunks of added, removed and context lines. |
| **Generated** | A change the head's `.gitattributes` marks `linguist-generated` or `gitlab-generated`, or a well-known lock file (`Cargo.lock`, `package-lock.json`, ...) it does not mark `-linguist-generated`. Read through `git check-attr --source`, so every forge sees it. Its diff starts collapsed in the Source view; it is seen like any change. |
| **Seen** | A change the reviewer marked as read. The mark keeps a hash of the change's diff; when the pull request changes that file again the hash differs and it is **changed since seen**, which counts as unseen. |
| **Draft** | A comment on one line or a run of lines of a change, on the new side or (for removed lines) the old side, kept locally until it is submitted. Only lines inside one hunk take one, because forges refuse others. In the Source view a selection (click, shift-click or drag, on head lines or on removed lines) takes one by typing, the comment button or alt+c; a reply is drawn inside its thread. |
| **Review** | Per pull request, the seen marks and the drafts: `<git common dir>/code-review/reviews/<n>.review`, written at once on every change, read again by an open window when it changes. Submitting sends every draft and the summary as one review (comment, approve, request changes) and clears the drafts. The forge decides which of the three a reviewer may send: GitHub and Forgejo take no approval or request for changes from the author, GitLab says who may approve; comment and request changes need a draft or a summary, and nothing goes out without a login. The Review tab colours the three buttons (approve green, request changes red), disables a refused one with the reason as its tip, and asks before it sends. |
| **Symbol** | A top-level declaration, plus one level of members of impl / mod / trait / class bodies, in the tree. Name, kind, inclusive line range, depth 0 or 1. |
| **Pin** | A repository the Inbox lists: a local clone named by path, or a URL cloned (blobless) into the clones folder. Kept in the pins file: `REVIEW_PINS`, else `<config>/code-review/pins`; clones in `REVIEW_CLONES`, else `<data>/code-review/clones`. Named `owner/name` from its remote; the short `name` works when it is unique. |
| **Address** | A pull request on the command line: `owner/name#12`, `name#12`, `name!12` (GitLab), or `12` / `#12` inside a pinned clone or when one pinned repository has it. |
| **Standing** | Where a pull request is for this reviewer: new (never opened), updated (its head moved since it was last opened), reviewing, reviewed (every change seen, no drafts). |
| **Thread** | A conversation on the forge: on a line of the code (open, resolved, outdated) or on the pull request itself. An outdated one, written on a line the pull request has changed since, is listed in the Review tab only and not drawn in the code. Read on open and on refresh, never stored; a reply is a draft that names its thread. Every note, the description and every draft is drawn as markdown: links, `#12`, `!12`, `@user` and commit hashes link to the forge, pictures are fetched once into a temporary cache and drawn in place; a click opens one over the window, fitted, to zoom (pinch, ctrl+wheel, + and -, 0 to fit, 1 for its own size) and move around (drag, scroll). |
| **Submission** | A review someone submitted on the forge: its author, decision (comment, approve, request changes), summary, and the threads started in it. The Review tab and `discussion` show each as one block, in time order with the threads on the pull request itself; a later reply stays in its thread. A submission that says nothing (no summary, no decision, no thread of its own) shows no block. GitLab groups no notes into reviews, so its threads all stand alone. |
| **Commit** | One commit of the pull request (`git log base..head`). The Commits view lists them; picking one shows its own changes against its parent, unified or split; comments stay on the whole pull request's changes. |
| **Link** | Where an entity lives on the forge: the pull request, a comment, an author, a commit, a file at a revision (a commit, a branch or a tag) with the selected lines. Every one has a button that opens it in the browser and one that copies it; `io-forge` spells each forge's URLs and reads a file link back, so a link in a description or a comment to a file of the tree opens it in the Source view. |
| **Forge login** | A forge's token kept in the system keychain (service `code-review`, account the host), from `login <host or repository>`: GitHub and GitLab by the device flow (a code entered in the browser, no server of our own), Forgejo by pasting a token. On github.com the login goes through a GitHub App, so it may only read and write pull requests (and read metadata) on the repositories the App is installed on; elsewhere the forge answers "Resource not accessible by integration" and the error says to install it. A token that comes with a refresh token (a GitHub App's, eight hours; GitLab's, two) is renewed when it expires or the forge answers 401. The device flow needs the client ID of the app registered on the host: github.com's is built in and gitlab.com has an empty slot, both in `BUILT_IN_APPS` in `crates/io-forge/src/wire.rs`, else `client <host> <id>` in the forges file (`REVIEW_FORGES`, else `<config>/code-review/forges`). `REVIEW_KEYCHAIN` names a scratch keychain; a script or screenshot run uses no other. |
| **Jira card** | The Jira issue a pull request's title or head branch names (PROJ-123): key, type, status, assignee, summary and description (Jira's wiki markup read as markdown, attached pictures fetched with the login), drawn at the top of the Review tab and printed by `card`. Logged in from the Inbox's Jira login form (site, email, API token; empty email for a Data Center personal access token) or `review login <site>` (a `*.atlassian.net` site, or one the forges file names): Jira is asked who the token belongs to, then the site goes into the forges file as `jira <host>` and the token into the keychain. |
| **Xref** | Symbol A calls or references symbol B. Xrefs to a symbol are its callers, xrefs from it its callees. |

## Why it is built this way

- **Git is the common ground; forges differ only at the edge.** Every forge publishes a pull
  request's head as a git ref (`refs/pull/<n>/head`, GitLab `refs/merge-requests/<n>/head`),
  so fetching, diffing and reading code are plain git and need no token. Only listing pull
  requests and submitting a review talk to a forge's API, in `io-forge`, through `curl` with
  the token on curl's stdin and never on a command line. A forge's token comes from its
  variable first (`GITHUB_TOKEN`/`GH_TOKEN`, `GITLAB_TOKEN`, `FORGEJO_TOKEN`/`GITEA_TOKEN`), then
  from a login kept in the system keychain, then (GitHub) from `gh auth token`.
- **Review state is local and written at once.** It is one reviewer's working state, not part
  of the code: it lives in the git common directory, so every worktree of the repo sees it
  and no commit carries it, and every mark and draft is written immediately, so nothing is
  lost to a crash and the CLI and an open window see each other's changes.
- **Seen follows the diff, not the commit.** A force-push that leaves a file's diff alone keeps
  it seen; one that changes it brings it back. The mark also keeps the head commit it was made
  at, so a file changed since seen shows what changed since that commit. No ref holds that
  commit: it lasts as long as git keeps the object, and `--since-seen` says so once it is gone.
- **The diff is drawn on the head file, folded to the change.** The Source view shows every
  changed file in one column, in the order of Changes, each under a header that marks it seen,
  collapses it and links it; picking a file in Changes scrolls to its header, and scrolling makes
  the file in sight the selection. A file the pull request does not change is shown alone. Each
  file is the head file with added lines tinted and removed lines, threads that are not outdated and drafts
  inserted where they belong; lines more than three away from a change fold
  into a row that opens twenty lines at a time toward either change, or all at once, and
  "whole file" drops the folds. Every identifier on a changed line navigates like any other
  code, and a jump into a fold reveals it. A deleted file is shown from its diff alone.
- **One backend per language.** The language's server when on PATH, else a tree-sitter
  resolver written for that language. No language-agnostic resolver: its rules would be
  nobody's. The servers run on the tree of the open pull request; opening another one starts
  them again on its tree.
- **The GUI is one selection** (a file and lines, a symbol when one is focused); every view
  shows it. Opening a pull request fetches, checks out, diffs and indexes in the background,
  then swaps the index and the servers in one step.
- **A split diff needs no second checkout.** The old side of a file is its head side with each
  hunk undone, so the split view (and a commit's view, which reads the commit's file once)
  highlights both sides with the same parsers the index uses, and folds, selects and comments
  like the unified view.
- **Nothing moves out from under the pointer.** A button once clicked keeps its place and
  size until the pointer leaves it or the wheel turns; whatever its click changes moves around
  it, and what slides under a vanished button takes no click until the pointer leaves. A
  button the pointer only rests on holds nothing, so the wheel scrolls under it. Rapid clicks
  on one spot always hit the same button. `crates/ui` holds every `.steady()` element this
  way.
- **An action shows its result.** A control whose result is drawn in another view switches to
  it and scrolls the result into sight; a field that holds the keyboard off screen fails a
  script run.
- **Every displayed text takes one selection.** `crates/ui` holds one text selection for the
  window: a drag over text a view marks selectable (labels, notes, titles, every markdown block,
  the links and code blocks in it) selects across the scroll column it starts in, a double-click
  takes a word, and primary+c copies it ahead of the Source view's selected lines. A press on a
  steady element (a button, a link) or on text not marked selectable (rows, tabs, fields, the
  Source view's code lines) only clicks it; every press clears the last selection. A soft-wrapped
  line copies as a space, a row end as a newline.
- **The UI is its own library** (`crates/ui`, in the shape of odin_editor's): elements opened
  and closed each frame, layout once at frame end, input answered from the previous frame's
  rectangles, one monospace font at whole-pixel sizes in one atlas, icons as Codicons glyphs.
- **Every outside program has a deadline and cannot deadlock on its pipes.** `io-process`
  feeds a program's stdin on its own thread while it reads stdout and stderr, so a program
  that answers as it reads (`git check-attr --stdin` on thousands of paths) never waits on the
  tool while the tool waits on it, and every run ends by its deadline, so a stuck program is
  an error the user reads, not a window or a command that never answers.
- **The gate runs a pull request of thousands of files.** A fixture of a few files hides
  everything that grows with the pull request; the scale scenarios run the inbox, open,
  changes, diff, next, seen and the Source view on a generated pull request of about three
  thousand changed files, each step within a bound, so a feature meets real scale before it
  ships and its cost there is part of its case.

## Crates

`domain` (the model, no I/O), `io-*` (one crate per outside format or program: `io-vcs` git,
`io-forge` forge APIs through curl, `io-review` the review files, `io-cache` the index cache,
`io-config` the user's layout, settings, pins and forges file, `io-keychain` the system keychain
(macOS `security`, the Secret Service's `secret-tool`), `io-fonts` installed fonts, `io-lsp`, `io-source`,
`io-process` (also the browser opener), `io-store`, `io-clipboard` the system clipboard,
`io-picture` pictures fetched through curl, cached in the temporary folder and decoded), `index` (tree-sitter resolvers,
server orchestration), `session` (the pinned repositories and their reviews: the inbox,
finding the forge, opening a pull request, saving a review, submitting it; the one place
`cli` and `gui` share), `cli` (text commands, also the GUI's Console), `ui` (the element tree), `platform` (wgpu,
fonts, winit, the script runner), `gui`, `review` (the binary and integration tests), `xtask`
(the gate). The allowed edges are in `xtask/src/arch.rs`.

## Indexing

Xrefs come from the language's server when it is on PATH (`rust-analyzer` for Rust),
otherwise from a tree-sitter resolver. Results live in `.review-cache` in the tree. The GUI
indexes the repository's working copy until a pull request is opened, then that pull
request's tree.

## Build and test

`cargo build`, `cargo test`, `cargo xtask gate` (`--full` adds the CLI and GUI scenarios),
`cargo xtask parity` (the GUI scenarios and a CLI transcript played by the parent revision's
build and this one; every dump line and screenshot that differs is listed). The GUI is
winit + wgpu + fontdue under the tool's own element tree (`crates/ui`); there is no UI
framework underneath.

`crates/review/tests` holds `cli.rs` (scenarios against a generated forge and clone, offline:
the pull request's refs are fetched by hand, so no forge API is needed; `run_in` gives each
run its own `REVIEW_PINS` and `REVIEW_CLONES`), `commands.rs` (every command `review help`
lists runs in a scenario), `gui.rs` (window scenarios, one window at a time) and `parity.rs`.
A GUI scenario, in the `gui_scenarios!` table of `common/gui.rs`, is a script (below,
"Testing the GUI"; `OPEN` opens the pull request, `{shots}` is its screenshot folder,
`<<DUMP rect <id>|dx,dy>>` a point inside an element) and the lines its output must contain.
It runs against `common/mod.rs`'s forge, offline: a clone whose remote is
`ssh://git@forgejo.test/owner/repo.git`, served by a fake `ssh`, and the forge's API answered
from files by a fake `curl`, both on a `PATH` of their own (no language server on it, so the
tree-sitter resolvers answer). A scenario is registered in its table; a
hand-written `#[test]` in a scenario file is refused by the linter. Behaviour a change adds is
pinned by a test that states it, written before the code.

The scale scenarios (`at_scale!` in `cli.rs`, `gui_scale_scenarios!` in `common/gui.rs`) run
on `common::large_fixture` (`common/large.rs`, built by `git fast-import` in about two
seconds): one pull request of 3003 changed files (modified, added, deleted, renamed, 200
marked `linguist-generated`, paths up to fourteen directories deep, 600-line hunks, three files of
30,000 lines). Each step's deadline is its measured time with about three times headroom, and
the GUI's slowest frame while scrolling the Source view has a bound too; `-- --nocapture`
prints each as a `SCALE` line. Every run of the binary and every program the harness starts
goes through `common::run_within` with a deadline (`COMMAND_DEADLINE` 60 s, `WINDOW_DEADLINE` 120 s,
or the scale step's bound); on expiry it kills the run's process group and fails naming the
scenario, the step and every process still running (pid, elapsed time, command line), so a
hang fails the gate instead of stalling it.

## Testing the GUI without a hand on the mouse

Never claim a visual or interactive behaviour from reading the code; drive it and look.

- `REVIEW_SHOT=<file.png> [REVIEW_SHOT_TAB=review|source|search] [REVIEW_SHOT_SCROLL=n]
  target/release/review` writes the first settled frame and quits. Started inside a clone,
  that clone is listed with the pins; set `REVIEW_PINS`/`REVIEW_CLONES` to a scratch folder so
  a run does not touch the real pins.
- `REVIEW_SCRIPT=<file> target/release/review` plays a script, one command per
  line, as real input, on a clock of its own (8 ms a frame, plus each pause): `wait n`
  (frames), `pause ms` (waits that long in wall time, for the forge and the servers),
  `mouse x y`, `down`, `up`, `click x y [ctrl|alt|shift|primary]`, `dblclick x y`,
  `drag x0 y0 x1 y1`, `wheel dy [ctrl|shift]`, `swipe dx dy`, `pinch n`,
  `key <name> [ctrl] [alt] [primary]`, `text ...`, `quit` (`primary` is the platform's primary
  modifier: cmd on macOS, ctrl elsewhere); app commands `tab <name>`,
  `open <file> [line]`, `scroll <panel> <n>`, `idle` (waits until the forge, the servers and
  every background job have answered), `rect <id>`, `click-id <id> [ctrl|alt|shift]`,
  `dblclick-id <id>`, `hover-id <id>`, `absent <id>`, `shot <file.png>`, `dump`. A `script:`
  line on stderr fails the run. Id names are the ones in `crates/gui/src/ids.rs`: `name`,
  `name/<n>` for rows, `name@<key>` (inbox rows `pull/<n>`, changed files `change/<n>`,
  commits `commit/<n>`, `all-changes` (in the Changes and Source toolbars `all-changes@changes`, `@source`), the split toggle `layout`, thread rows `thread/<n>`,
  a thread's reply button `reply@<thread id>`, a fold's buttons `fold-up@<path>:<first line>`,
  `fold-down@<path>:<last line>` and `fold@<path>:<first line>` (lines from 0), a file's header
  `file-header@<path>` with `collapse@<path>`, `seen@<path>`, `whole-file@<path>`, split rows
  `table@<path>:<row>`, link buttons `link-open@<key>` and `link-copy@<key>` (`pull`, `file`,
  `<path>`, `<thread>/<note>`), a removed line `draft-removed@<path>:<old line>`, a draft's remove button `draft-remove@<id>`, every
  view's tab `tab@<View>`). Rects come from the previous frame, so `wait 1` after anything
  that changes the layout. `dump` prints the selection, the review (pulls, the open pull
  request, the inbox entries, the open pull request, the diff layout and the commit shown, every
  change with its seen state, the threads,
  every draft), the text selection, scroll offsets, the panel tree,
  fields, the backend and the servers to stderr as `DUMP` lines (grep `^DUMP`).
  A script or screenshot run opens a fixed 1600x1000 window at scale 1 and ignores the real
  mouse and keyboard. It starts from the default panel layout and saves none, unless
  `REVIEW_LAYOUT=<file>` names a layout file to load and save. Write script files with the
  Write tool and forward-slash paths: a heredoc mangles backslashes.
- On macOS a script or screenshot run starts with the activation policy `Prohibited`
  (`crates/platform/src/background.rs`), so its window renders without ever taking focus.
- On Linux, run GUI scripts one at a time under a private headless compositor
  (`weston --backend=headless --renderer=pixman --shell=kiosk --socket=<name>`,
  `WAYLAND_DISPLAY=<name>`, a software Vulkan driver).
- Read a PNG with the Read tool; crop or scale it first when the detail matters.

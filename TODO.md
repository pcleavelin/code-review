# TODO

## Review

1. Submitting a review has not been run against a live forge: GitHub, GitLab and Forgejo
   listing were run against public repositories, and the request bodies are unit-tested, but
   no review has been posted. Try each on a pull request you own.
6. The Inbox shows open pull requests only and filters by words and standing; there is no
   "review requested from me" or "mine", which need the forge user. Trigger: an inbox too long
   to scan.
11. Markdown: HTML other than `<img>` is drawn as its text with the tags dropped, footnotes and
    math as plain text. Trigger: a description or comment whose meaning is lost that way (a
    `<details>` block read as running text, a footnote, an equation).
12. The old side of the split view, and a commit's view of a file a later commit changes again,
    draw lines without go to definition or hover: neither text is the tree the index and the
    servers read. Trigger: a review that needs to follow a removed call. Comments are not taken
    in a commit's view (forges anchor them to the pull request's head here).
13. What changed since a file was seen is shown from the CLI only (`diff <pr> <file>
    --since-seen`); the window still shows the whole diff of a file changed since seen. After
    a rebase, the base's own changes to the file show as changed since too.
14. `login` has run only against the offline fake forge: the device flow on github.com (its
    OAuth app's client ID is built in) and a GitLab token's renewal have not met a live forge,
    and gitlab.com has no client ID yet (`BUILT_IN_APPS` in `crates/io-forge/src/wire.rs`).
    Log in to each once.

## Inherited from codemap

2. The call graph view was not carried over. `calltree`, `callers` and `callees` follow calls
   across files as text, from the CLI and from the window's Console (tried on ripgrep#3549:
   `calltree ripgrep#3549 PathPrinter::write 2` prints two levels across four files). A drawn
   view needs a pull request where that text is not enough to follow the change. Trigger: such
   a pull request, seen in a run.

## Deferred

Each waits for its trigger.

- Mapping this repository with codemap (tours, the gate's map check): the owner decides
  whether the case for each feature lives in the map or, as now, in the commit that adds it.
- A daemon holding the forge client and the language servers for the CLI: cold CLI runs on a
  large tree become the bottleneck of an agent's review.

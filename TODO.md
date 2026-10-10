# TODO

## Review

1. Submitting a review has not been run against a live forge: GitHub, GitLab and Forgejo
   listing were run against public repositories, and the request bodies are unit-tested, but
   no review has been posted. Try each on a pull request you own.
3. GitLab: a draft sends only the line of its own side, and GitLab wants both for an unchanged
   (context) line, so a draft on a context line comes back as HTTP 400. A renamed file's
   old-side draft names the new path. The review is several requests (one per draft, then the
   note, then approve); a failure part-way leaves the earlier ones posted.
4. Threads are read from one page per kind (100 comments on GitHub). GitLab threads are never
   marked outdated, Forgejo's are grouped by line rather than by reply chain, and a Forgejo
   reply is a new comment on the same line. An outdated thread is listed in the Review tab but
   its line may have moved, so it is not drawn in the code.
5. "What changed since I looked": the seen mark keeps the head commit it was made at, so a
   file that turned "changed since seen" could show the interdiff against that commit instead
   of the whole diff again. Trigger: re-reviewing a large pull request after a force-push.
6. The Inbox shows open pull requests only and filters by words and standing; there is no
   "review requested from me" or "mine", which need the forge user. Trigger: an inbox too long
   to scan.
7. Opening a pull request runs `index::build` on its tree in the background but the window
   starts with a blocking `index::build` of the working copy (codemap's TODO 5 still holds).
8. The trees under `.git/code-review/trees` are never removed; `git worktree prune` after
   deleting one. Add a command to drop a finished review's tree and refs.
9. A deleted file's view is not virtualised: every removed line is laid out each frame.

10. Pictures are fetched without the forge's token, so a private repository's attachments
    do not load (they show as a failed picture that opens in the browser). GitHub serves them
    behind its web session, not the API token. Animated pictures show their first frame.
11. Markdown: a table wider than its box is clipped rather than wrapped; HTML other than
    `<img>` is dropped to its text; footnotes and math are plain text.
12. The split view and a commit's view draw lines without go to definition or hover, and
    clip long lines instead of scrolling sideways. Comments are not taken in a commit's view
    (forges anchor them to the pull request's head here).

## Inherited from codemap

1. Navigation (`files`, `symbols`, `show`, `search`, `callers`, `callees`, `refs`, `tree`,
   `roots`) was not carried over to the CLI; the agent reads a pull request's code through
   `diff` and the tree's path. Port them over `session`'s tree when an agent reviewing through
   the CLI needs them.
2. The call graph view was not carried over. Trigger: a pull request whose change is only
   understood by following calls across files.
3. No GUI scenario harness (`tests/gui.rs`) and no `cargo xtask parity` comparison: both
   depended on codemap's scenario tables, which were removed with the tours. Port
   `tests/common/gui.rs` and `parity.rs` before the GUI changes again.
4. No copy out of the Source view; ctrl+c/x/v work in text fields only.
5. Hover requests have no cancellation; one for a word the pointer has left is still
   answered first.
6. The file browser should sort by type: directories first, then files grouped by extension.

## Deferred

Each waits for its trigger.

- Mapping this repository with codemap (tours, the gate's map check): the owner decides
  whether the case for each feature lives in the map or, as now, in the commit that adds it.
- A daemon holding the forge client and the language servers for the CLI: cold CLI runs on a
  large tree become the bottleneck of an agent's review.

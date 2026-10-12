use crate::review::{
    Block, BranchName, ChangeKind, CodePlace, CommitId, Content, Conversation, Decision,
    DiffLayout, DiffLine, DraftId, DraftOn, DraftText, FileChange, FileRow, Hunk, LineKind, Login,
    Moment, Note, Pairing, PrNumber, Prose, RefName, Refusal, RepoSlug, Review, Revision, Rights,
    Seen, Side, Standing, Submission, SubmissionId, Thread, ThreadId, ThreadState,
};
use crate::text::{FileText, Line, RelativePath, SourceLine, Span};

fn line(kind: LineKind, text: &str) -> DiffLine {
    DiffLine {
        kind,
        text: SourceLine::new(text),
    }
}

fn change(path: &str, hunks: Vec<Hunk>) -> FileChange {
    FileChange::new(RelativePath::new(path), None, ChangeKind::Modified, hunks)
}

fn commit() -> CommitId {
    CommitId::new("0123456789abcdef").unwrap()
}

fn replace_b_with_x() -> FileChange {
    change(
        "src/lib.rs",
        vec![Hunk::new(
            Line::new(0),
            Line::new(0),
            vec![
                line(LineKind::Context, "a"),
                line(LineKind::Removed, "b"),
                line(LineKind::Added, "x"),
                line(LineKind::Context, "c"),
            ],
        )],
    )
}

#[test]
fn a_removed_line_shows_before_the_head_line_that_replaced_it() {
    let head = FileText::from("a\nx\nc\n");
    let rows = replace_b_with_x().rows(Some(&head));
    assert_eq!(
        rows,
        vec![
            FileRow::Head {
                line: Line::new(0),
                kind: LineKind::Context
            },
            FileRow::Removed {
                old: Line::new(1),
                text: SourceLine::new("b")
            },
            FileRow::Head {
                line: Line::new(1),
                kind: LineKind::Added
            },
            FileRow::Head {
                line: Line::new(2),
                kind: LineKind::Context
            },
        ]
    );
}

#[test]
fn lines_removed_at_the_end_of_the_file_come_after_the_last_head_line() {
    let removed_tail = change(
        "a.txt",
        vec![Hunk::new(
            Line::new(1),
            Line::new(1),
            vec![line(LineKind::Context, "b"), line(LineKind::Removed, "c")],
        )],
    );
    let head = FileText::from("a\nb\n");
    let rows = removed_tail.rows(Some(&head));
    assert_eq!(
        rows.last(),
        Some(&FileRow::Removed {
            old: Line::new(2),
            text: SourceLine::new("c")
        })
    );
    assert_eq!(rows.len(), 3);
}

#[test]
fn a_deleted_file_is_all_removed_rows() {
    let rows = replace_b_with_x().rows(None);
    assert_eq!(
        rows,
        vec![FileRow::Removed {
            old: Line::new(1),
            text: SourceLine::new("b")
        }]
    );
}

#[test]
fn numbering_skips_the_side_a_line_is_not_on() {
    let numbered: Vec<_> = replace_b_with_x()
        .hunks()
        .first()
        .unwrap()
        .numbered()
        .map(|line| (line.old, line.new))
        .collect();
    assert_eq!(
        numbered,
        vec![
            (Some(Line::new(0)), Some(Line::new(0))),
            (Some(Line::new(1)), None),
            (None, Some(Line::new(1))),
            (Some(Line::new(2)), Some(Line::new(2))),
        ]
    );
}

#[test]
fn only_lines_inside_a_hunk_take_comments_and_each_side_counts_its_own_lines() {
    let file = replace_b_with_x();
    let one = |line: u32| Span::line(Line::new(line));
    let lines = |start: u32, end: u32| Span::new(Line::new(start), Line::new(end)).unwrap();
    assert!(file.commentable(Side::New, one(1)));
    assert!(file.commentable(Side::Old, one(1)));
    assert!(!file.commentable(Side::New, one(3)));
    assert!(!file.commentable(Side::Old, one(9)));
    assert!(file.commentable(Side::New, lines(0, 2)));
    assert!(!file.commentable(Side::New, lines(1, 3)));
}

#[test]
fn an_unchanged_line_has_a_counterpart_on_the_other_side_and_a_changed_one_has_none() {
    let file = change(
        "src/lib.rs",
        vec![Hunk::new(
            Line::new(9),
            Line::new(19),
            vec![
                line(LineKind::Context, "a"),
                line(LineKind::Removed, "b"),
                line(LineKind::Added, "x"),
                line(LineKind::Added, "y"),
                line(LineKind::Context, "c"),
            ],
        )],
    );
    let counterpart = |side: Side, at: u32| file.counterpart(side, Line::new(at));
    assert_eq!(counterpart(Side::New, 19), Some(Line::new(9)));
    assert_eq!(counterpart(Side::New, 22), Some(Line::new(11)));
    assert_eq!(counterpart(Side::Old, 11), Some(Line::new(22)));
    assert_eq!(counterpart(Side::New, 20), None);
    assert_eq!(counterpart(Side::Old, 10), None);
    assert_eq!(counterpart(Side::New, 40), None);
}

#[test]
fn a_file_marked_seen_turns_changed_when_its_diff_changes() {
    let mut review = Review::default();
    let before = replace_b_with_x();
    assert_eq!(review.seen(&before), Seen::Unseen);
    review.mark(&before, commit());
    assert_eq!(review.seen(&before), Seen::Seen);
    let after = change(
        "src/lib.rs",
        vec![Hunk::new(
            Line::new(0),
            Line::new(0),
            vec![line(LineKind::Added, "y")],
        )],
    );
    assert_eq!(review.seen(&after), Seen::Changed);
}

#[test]
fn toggling_a_seen_file_unmarks_it() {
    let mut review = Review::default();
    let file = replace_b_with_x();
    assert_eq!(review.toggle(&file, commit()), Seen::Seen);
    assert_eq!(review.toggle(&file, commit()), Seen::Unseen);
    assert!(review.marks().is_empty());
}

#[test]
fn next_unseen_wraps_around_and_skips_seen_files() {
    let files = vec![
        change("a", Vec::new()),
        change("b", Vec::new()),
        change("c", Vec::new()),
    ];
    let mut review = Review::default();
    review.mark(&files[2], commit());
    let after_b = review.next_unseen(&files, Some(&RelativePath::new("b")));
    assert_eq!(after_b.map(FileChange::path), Some(&RelativePath::new("a")));
    let first = review.next_unseen(&files, None);
    assert_eq!(first.map(FileChange::path), Some(&RelativePath::new("a")));
    assert_eq!(review.progress(&files).seen().get(), 1);
}

#[test]
fn draft_ids_never_repeat_after_a_removal() {
    let mut review = Review::default();
    let text = || DraftText::new("why?").unwrap();
    let on = |side: Side, line: u32| DraftOn {
        file: RelativePath::new("a"),
        side,
        line: Line::new(line),
        reply: None,
        start: None,
    };
    let first = review.add_draft(on(Side::New, 3), text());
    let second = review.add_draft(on(Side::Old, 4), text());
    review.remove_draft(first).unwrap();
    let third = review.add_draft(on(Side::New, 5), text());
    assert_ne!(third, second);
    assert!(review.remove_draft(DraftId::new(99)).is_err());
}

#[test]
fn a_pull_request_is_new_until_looked_at_and_updated_once_its_head_moves() {
    let first = commit();
    let moved = CommitId::new("fedcba9876543210").unwrap();
    let files = vec![replace_b_with_x()];
    let mut review = Review::default();
    assert_eq!(
        review.standing(&first, Some(review.progress(&files))),
        Standing::New
    );
    review.look(first.clone());
    assert_eq!(
        review.standing(&first, Some(review.progress(&files))),
        Standing::Reviewing
    );
    review.mark(&files[0], first.clone());
    assert_eq!(
        review.standing(&first, Some(review.progress(&files))),
        Standing::Reviewed
    );
    assert_eq!(
        review.standing(&moved, Some(review.progress(&files))),
        Standing::Updated
    );
}

#[test]
fn a_looked_at_review_whose_changes_are_not_fetched_here_is_never_reviewed() {
    let mut review = Review::default();
    review.look(commit());
    assert_eq!(review.standing(&commit(), None), Standing::Reviewing);
}

#[test]
fn a_pending_draft_keeps_a_fully_seen_review_open() {
    let files = vec![replace_b_with_x()];
    let mut review = Review::default();
    review.look(commit());
    review.mark(&files[0], commit());
    review.add_draft(
        DraftOn {
            file: RelativePath::new("src/lib.rs"),
            side: Side::New,
            line: Line::new(1),
            reply: None,
            start: None,
        },
        DraftText::new("one more thing").unwrap(),
    );
    assert_eq!(
        review.standing(&commit(), Some(review.progress(&files))),
        Standing::Reviewing
    );
}

#[test]
fn a_moment_shows_its_day_and_minute() {
    let when = Moment::new("2026-10-09T14:03:59Z");
    assert_eq!(when.day(), "2026-10-09");
    assert_eq!(when.to_string(), "2026-10-09 14:03");
    assert_eq!(Moment::new("2026-10-09").to_string(), "2026-10-09");
}

#[test]
fn blank_draft_text_is_refused() {
    assert!(DraftText::new("  \n").is_none());
}

#[test]
fn pr_numbers_accept_forge_prefixes_and_refuse_zero() {
    assert_eq!(PrNumber::parse("#12").map(PrNumber::get), Some(12));
    assert_eq!(PrNumber::parse("!7").map(PrNumber::get), Some(7));
    assert!(PrNumber::parse("0").is_none());
}

#[test]
fn a_slug_needs_an_owner_and_a_name() {
    assert_eq!(
        RepoSlug::new("/group/sub/repo/").map(|slug| slug.as_str().to_owned()),
        Some("group/sub/repo".to_owned())
    );
    assert!(RepoSlug::new("owner//repo").is_none());
    assert!(RepoSlug::new("repo").is_none());
}

#[test]
fn the_old_file_is_the_head_file_with_each_hunk_undone() {
    let head = FileText::from("top\na\nx\nc\nend\n");
    let change = change(
        "src/lib.rs",
        vec![Hunk::new(
            Line::new(1),
            Line::new(1),
            vec![
                line(LineKind::Context, "a"),
                line(LineKind::Removed, "b"),
                line(LineKind::Added, "x"),
                line(LineKind::Context, "c"),
            ],
        )],
    );
    assert_eq!(
        change.old_text(&head),
        FileText::from("top\na\nb\nc\nend\n")
    );
}

#[test]
fn split_pairs_a_replaced_line_and_unified_stacks_it() {
    let file = replace_b_with_x();
    let three = FileText::from("a\nx\nc\n").count();
    let pairs = |layout: DiffLayout| -> Vec<(Option<u32>, Option<u32>, Pairing)> {
        file.paired(three, three, layout)
            .iter()
            .map(|pair| {
                (
                    pair.old.map(Line::value),
                    pair.new.map(Line::value),
                    pair.pairing,
                )
            })
            .collect()
    };
    assert_eq!(
        pairs(DiffLayout::Split),
        vec![
            (Some(0), Some(0), Pairing::Same),
            (Some(1), Some(1), Pairing::Changed),
            (Some(2), Some(2), Pairing::Same),
        ]
    );
    assert_eq!(
        pairs(DiffLayout::Unified),
        vec![
            (Some(0), Some(0), Pairing::Same),
            (Some(1), None, Pairing::Removed),
            (None, Some(1), Pairing::Added),
            (Some(2), Some(2), Pairing::Same),
        ]
    );
}

fn thread_on(id: &str, file: &str, state: ThreadState) -> Thread {
    Thread {
        id: ThreadId::new(id).unwrap(),
        place: Some(CodePlace {
            file: RelativePath::new(file),
            side: Side::New,
            line: Line::new(3),
        }),
        state,
        notes: Vec::new(),
        submission: None,
    }
}

#[test]
fn a_file_draws_its_open_and_resolved_threads_but_not_outdated_ones() {
    let conversation = Conversation {
        description: Prose::default(),
        threads: vec![
            thread_on("open", "src/lib.rs", ThreadState::Open),
            thread_on("moved", "src/lib.rs", ThreadState::Outdated),
            thread_on("settled", "src/lib.rs", ThreadState::Resolved),
            thread_on("elsewhere", "src/main.rs", ThreadState::Open),
        ],
        submissions: Vec::new(),
        signed_pictures: Vec::new(),
    };
    let file = RelativePath::new("src/lib.rs");
    let drawn: Vec<&str> = conversation
        .drawn_in(&file)
        .map(|thread| thread.id.as_str())
        .collect();
    assert_eq!(drawn, vec!["open", "settled"]);
}

fn content(id: &str, author: &str, when: &str) -> Thread {
    Thread {
        id: ThreadId::new(id).unwrap(),
        place: None,
        state: ThreadState::Open,
        notes: vec![Note {
            author: Login::new(author),
            body: Prose::new("text"),
            when: Moment::new(when),
            link: None,
        }],
        submission: None,
    }
}

fn in_submission(mut thread: Thread, submission: &str) -> Thread {
    thread.submission = SubmissionId::new(submission);
    thread
}

fn submission(id: &str, decision: Decision, summary: &str, when: &str) -> Submission {
    Submission {
        id: SubmissionId::new(id).unwrap(),
        author: Login::new("dave"),
        decision,
        summary: Prose::new(summary),
        when: Moment::new(when),
        link: None,
    }
}

fn shape(conversation: &Conversation) -> Vec<String> {
    conversation
        .blocks()
        .iter()
        .map(|block| match block {
            Block::Submitted {
                submission,
                threads,
            } => {
                let ids: Vec<&str> = threads.iter().map(|thread| thread.id.as_str()).collect();
                format!("{}[{}]", submission.id, ids.join(" "))
            }
            Block::Alone(thread) => thread.id.to_string(),
        })
        .collect()
}

#[test]
fn a_submission_gathers_the_threads_started_in_it_and_blocks_follow_time() {
    let conversation = Conversation {
        submissions: vec![
            submission("r1", Decision::Comment, "", "2026-05-01T10:30:00Z"),
            submission("r2", Decision::Approve, "", "2026-05-01T12:00:00Z"),
            submission("r3", Decision::Comment, "", "2026-05-01T11:00:00Z"),
            submission(
                "r4",
                Decision::RequestChanges,
                "split it",
                "2026-05-01T08:00:00Z",
            ),
        ],
        threads: vec![
            in_submission(content("t1", "dave", "2026-05-01T10:00:00Z"), "r1"),
            content("issue", "carol", "2026-05-01T09:00:00Z"),
            in_submission(content("t2", "dave", "2026-05-01T10:01:00Z"), "r1"),
            in_submission(content("lost", "octo", "2026-05-01T07:00:00Z"), "pending"),
        ],
        ..Conversation::default()
    };
    assert_eq!(
        shape(&conversation),
        vec!["lost", "r4[]", "issue", "r1[t1 t2]", "r2[]"]
    );
}

#[test]
fn an_empty_review_may_only_approve_and_a_forge_refusal_comes_before_the_empty_one() {
    let open = Rights::default();
    assert_eq!(open.refusal(Decision::Approve, Content::Nothing), None);
    assert_eq!(
        open.refusal(Decision::RequestChanges, Content::Nothing),
        Some(Refusal::NothingToSend)
    );
    assert_eq!(
        open.refusal(Decision::Comment, Content::Nothing),
        Some(Refusal::NothingToSend)
    );
    assert_eq!(open.refusal(Decision::Comment, Content::Something), None);
    let author = Rights {
        approve: Some(Refusal::OwnPullRequest),
        request_changes: Some(Refusal::OwnPullRequest),
        ..Rights::default()
    };
    assert_eq!(
        author.refusal(Decision::RequestChanges, Content::Nothing),
        Some(Refusal::OwnPullRequest)
    );
    assert_eq!(author.refusal(Decision::Comment, Content::Something), None);
}

#[test]
fn a_revision_is_the_head_by_its_branch_or_a_prefix_of_its_commit() {
    let branch = BranchName::new("ann/feature");
    let head = CommitId::new("0123456789abcdef0123456789abcdef01234567").unwrap();
    let is_head = |revision: Revision| revision.is_head(&branch, &head);
    assert!(is_head(Revision::Named(RefName::new("ann/feature"))));
    assert!(!is_head(Revision::Named(RefName::new("main"))));
    assert!(!is_head(Revision::Named(RefName::new("ann"))));
    assert!(is_head(Revision::Commit(commit())));
    assert!(is_head(Revision::Commit(head.clone())));
    assert!(!is_head(Revision::Commit(
        CommitId::new("0123456789abcdee").unwrap()
    )));
}

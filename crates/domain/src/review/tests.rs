use crate::review::{
    ChangeKind, CommitId, DiffLine, DraftId, DraftOn, DraftText, FileChange, FileRow, Hunk,
    LineKind, Moment, PrNumber, RepoSlug, Review, Seen, Side, Standing,
};
use crate::text::{FileText, Line, RelativePath, SourceLine};

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
    assert!(file.commentable(Side::New, Line::new(1)));
    assert!(file.commentable(Side::Old, Line::new(1)));
    assert!(!file.commentable(Side::New, Line::new(3)));
    assert!(!file.commentable(Side::Old, Line::new(9)));
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
        review.standing(&first, review.progress(&files)),
        Standing::New
    );
    review.look(first.clone());
    assert_eq!(
        review.standing(&first, review.progress(&files)),
        Standing::Reviewing
    );
    review.mark(&files[0], first.clone());
    assert_eq!(
        review.standing(&first, review.progress(&files)),
        Standing::Reviewed
    );
    assert_eq!(
        review.standing(&moved, review.progress(&files)),
        Standing::Updated
    );
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
        },
        DraftText::new("one more thing").unwrap(),
    );
    assert_eq!(
        review.standing(&commit(), review.progress(&files)),
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

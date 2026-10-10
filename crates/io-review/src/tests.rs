use std::env;
use std::fs;
use std::process;
use std::sync::atomic::{AtomicU32, Ordering};

use domain::{
    ChangeKind, CommitId, DiffLine, DraftOn, DraftText, FileChange, Hunk, Line, LineKind, PrNumber,
    RelativePath, Review, Seen, Side, SourceLine, ThreadId,
};

use crate::{ReviewError, ReviewStore, wire};

static COUNT: AtomicU32 = AtomicU32::new(0);

fn store() -> ReviewStore {
    let count = COUNT.fetch_add(1, Ordering::Relaxed);
    let directory = env::temp_dir().join(format!("io-review-{}-{count}", process::id()));
    drop(fs::remove_dir_all(&directory));
    ReviewStore::at(directory)
}

fn change(path: &str) -> FileChange {
    FileChange::new(
        RelativePath::new(path),
        None,
        ChangeKind::Modified,
        vec![Hunk::new(
            Line::new(0),
            Line::new(0),
            vec![DiffLine {
                kind: LineKind::Added,
                text: SourceLine::new("x"),
            }],
        )],
    )
}

#[test]
fn a_review_never_saved_loads_empty() {
    let store = store();
    assert_eq!(
        store.load(PrNumber::new(1).unwrap()).unwrap(),
        Review::default()
    );
}

#[test]
fn marks_and_drafts_with_awkward_text_survive_a_round_trip() {
    let store = store();
    let number = PrNumber::new(42).unwrap();
    let file = change("dir with space/a\tb.rs");
    let mut review = Review::default();
    review.mark(&file, CommitId::new("abcdef0123456789").unwrap());
    review.add_draft(
        DraftOn {
            file: file.path().clone(),
            side: Side::Old,
            line: Line::new(9),
            reply: None,
            start: None,
        },
        DraftText::new("first line\n\tsecond \\ line").unwrap(),
    );
    review.add_draft(
        DraftOn {
            file: file.path().clone(),
            side: Side::New,
            line: Line::new(2),
            reply: ThreadId::new("4711"),
            start: None,
        },
        DraftText::new("agreed").unwrap(),
    );
    review.look(CommitId::new("0123456789abcdef").unwrap());
    store.save(number, &review).unwrap();
    let loaded = store.load(number).unwrap();
    assert_eq!(loaded, review);
    assert_eq!(loaded.seen(&file), Seen::Seen);
    assert!(store.stamp(number).is_some());
}

#[test]
fn a_file_from_another_version_is_refused_with_its_path() {
    let store = store();
    let number = PrNumber::new(3).unwrap();
    store.save(number, &Review::default()).unwrap();
    let file = store.file(number);
    io_store::write(&file, "review 1\n").unwrap();
    match store.load(number) {
        Err(ReviewError::Unparsable { file: named, .. }) => assert_eq!(named, file),
        other => panic!("{other:?}"),
    }
}

#[test]
fn an_unknown_record_is_named() {
    let text = format!("{}\nverdict\tapprove\n", wire::VERSION);
    assert_eq!(
        wire::parse(&text),
        Err(wire::WireFault::Record("verdict".to_owned()))
    );
}

#[test]
fn each_pull_request_gets_its_own_tree() {
    let store = store();
    assert_ne!(
        store.tree(PrNumber::new(1).unwrap()),
        store.tree(PrNumber::new(2).unwrap())
    );
}

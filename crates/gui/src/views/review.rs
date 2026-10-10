use domain::{
    ChangeKind, Decision, Draft, DraftOn, Note, NoteKind, RelativePath, Seen, Side, Span, Thread,
    ThreadState,
};
use ui::{Count, Icon, Label, Run};

use crate::action::Action;
use crate::field::Which;
use crate::ids;
use crate::model::Model;
use crate::review::{OpenReview, ReviewAct, Submitting, Talk};
use crate::text::{Counted, Noun};
use crate::theme::{
    ACCENT, COMMENT_BUTTONS_ROOM, COMMENT_FIELD_GUESS, COMMENT_LEAST_FIELD, Cells, GREEN, NOTE,
    ORANGE, RED, ROW_EXTRA, SELECTED, SUMMARY_FIELD, TEXT, WEAK,
};
use crate::widgets::{
    Chosen, CommentLook, Container, Frame, Padding, RowAction, Scroller, TreeRow,
};

use super::inbox::{standing_color, standing_word};

fn seen_mark(seen: Seen) -> Run {
    match seen {
        Seen::Seen => Run::new(Icon::Check, GREEN),
        Seen::Unseen => Run::new(Icon::Dot, ORANGE),
        Seen::Changed => Run::new("~", ORANGE),
    }
}

fn kind_word(kind: ChangeKind) -> Label {
    Label::new(match kind {
        ChangeKind::Added => "added",
        ChangeKind::Deleted => "deleted",
        ChangeKind::Modified => "modified",
        ChangeKind::Renamed => "renamed",
    })
}

pub(super) fn lines_word(lines: Span) -> Label {
    Label::new(if lines.start() == lines.end() {
        format!("line {}", lines.end().number())
    } else {
        format!("lines {}-{}", lines.start().number(), lines.end().number())
    })
}

fn side_word(side: Side) -> Label {
    Label::new(match side {
        Side::Old => " (removed line)",
        Side::New => "",
    })
}

pub(super) fn changes_window(model: &Model, frame: &mut Frame<'_>) {
    let Some(opened) = model.review.opened() else {
        frame.note(
            "open a pull request in the Inbox to list its changed files",
            WEAK,
            Padding::Step,
        );
        return;
    };
    frame.start(Container::ToolbarSmall);
    let progress = opened.review.progress(&opened.changes);
    frame.label(
        format!("{progress} seen"),
        if progress.done() { GREEN } else { ORANGE },
    );
    let next = ids::NEXT_UNSEEN.target();
    if frame.small_button("next unseen", next).clicked() {
        frame.push(Action::Review(ReviewAct::NextUnseen));
    }
    frame.attach_tip(next);
    frame.finish();
    let id = ids::changes();
    let scrolled = frame.scroll_column(id, model.scrolls.get(id), Scroller::Plain, None);
    let row_height = frame.row_height() + ROW_EXTRA;
    let count = Count::new(opened.changes.len());
    let window = frame.rows_window(
        scrolled.offset,
        scrolled.interaction.rect(),
        count,
        row_height,
        Count::new(40),
    );
    let shown = model.shown_path();
    for (position, change) in opened
        .changes
        .iter()
        .enumerate()
        .skip(window.first.get())
        .take(window.visible.get())
    {
        let seen = opened.seen(change);
        let path = change.path();
        let mut runs = vec![seen_mark(seen), Run::new(format!(" {path}"), TEXT)];
        if let Some(old) = change.old_path() {
            runs.push(Run::new(format!("  from {old}"), WEAK));
        }
        let drafts = opened.review.drafts_at(path).count();
        let mut badges = Vec::new();
        if drafts > 0 {
            badges.push(Run::new(format!("{drafts}"), ORANGE));
        }
        badges.push(Run::new(format!("+{}", change.added()), GREEN));
        badges.push(Run::new(format!("-{}", change.removed()), RED));
        let row = TreeRow {
            level: Count::ZERO,
            runs,
            badges,
            marked: (shown.as_ref() == Some(path)).then_some(SELECTED),
            action: Some(RowAction::new(
                if seen == Seen::Seen { "unseen" } else { "seen" },
                ids::SEEN.nth(Count::new(position)),
            )),
        };
        let clicks = frame.tree_row(row, ids::CHANGE_ROW.nth(Count::new(position)));
        if clicks.acted() {
            frame.push(Action::Review(ReviewAct::ToggleSeen(path.clone())));
        } else if clicks.row.clicked() {
            frame.push(Action::Review(ReviewAct::OpenChange(path.clone())));
        }
    }
    frame.rows_after(count, &window, row_height, ui::Px::ZERO);
    frame.finish();
}

pub(super) fn change_controls(model: &Model, frame: &mut Frame<'_>, path: &RelativePath) {
    let Some(change) = model.review.change(path) else {
        return;
    };
    let seen = model.review.seen(change);
    frame.label_runs(vec![
        Run::new(kind_word(change.kind()), WEAK),
        Run::new(format!("  +{}", change.added()), GREEN),
        Run::new(format!(" -{}", change.removed()), RED),
    ]);
    let toggle = ids::SEEN.target();
    let (word, chosen) = match seen {
        Seen::Seen => ("seen", Chosen::Chosen),
        Seen::Changed => ("changed since seen", Chosen::Plain),
        Seen::Unseen => ("mark seen", Chosen::Plain),
    };
    if frame.button(word, toggle, chosen).clicked() {
        frame.push(Action::Review(ReviewAct::ToggleShownSeen));
    }
    frame.attach_tip(toggle);
    for (label, target, act) in [
        ("prev", ids::PREVIOUS_HUNK.target(), ReviewAct::PreviousHunk),
        ("next", ids::NEXT_HUNK.target(), ReviewAct::NextHunk),
        (
            "next unseen",
            ids::NEXT_UNSEEN.target(),
            ReviewAct::NextUnseen,
        ),
        (
            "comment",
            ids::DRAFT_HERE.target(),
            ReviewAct::StartDraftHere,
        ),
    ] {
        if frame.small_button(label, target).clicked() {
            frame.push(Action::Review(act));
        }
        frame.attach_tip(target);
    }
    let whole = ids::WHOLE_FILE.target();
    let shows_whole = model.review.whole.contains(path);
    if frame
        .button(
            "whole file",
            whole,
            if shows_whole {
                Chosen::Chosen
            } else {
                Chosen::Plain
            },
        )
        .clicked()
    {
        frame.push(Action::Review(ReviewAct::ToggleWhole(path.clone())));
    }
    frame.attach_tip(whole);
}

fn note_head(note: &Note) -> Vec<Run> {
    let mut runs = vec![
        Run::new(note.author.as_str(), ACCENT),
        Run::new(format!("  {}", note.when), WEAK),
    ];
    match note.kind {
        NoteKind::Comment => {}
        NoteKind::Approved => runs.push(Run::new("  approved", GREEN)),
        NoteKind::ChangesRequested => runs.push(Run::new("  requested changes", RED)),
    }
    runs
}

fn notes(frame: &mut Frame<'_>, thread: &Thread) {
    for note in &thread.notes {
        frame.label_runs(note_head(note));
        if !note.body.is_empty() {
            frame.note(note.body.as_str(), TEXT, Padding::Step);
        }
    }
}

const fn thread_look(state: ThreadState) -> CommentLook {
    match state {
        ThreadState::Open => CommentLook::Thread,
        ThreadState::Resolved | ThreadState::Outdated => CommentLook::Settled,
    }
}

fn state_word(state: ThreadState) -> Label {
    Label::new(match state {
        ThreadState::Open => "",
        ThreadState::Resolved => "  resolved",
        ThreadState::Outdated => "  outdated",
    })
}

fn pending_reply(frame: &mut Frame<'_>, draft: &Draft) {
    frame.start(Container::FillRow);
    frame.label_runs(vec![
        Run::new("you", ORANGE),
        Run::new("  draft reply, sent with the review", WEAK),
    ]);
    frame.grow();
    let remove = ids::DRAFT_REMOVE.with(&Label::new(draft.id.to_string()));
    if frame.small_button("remove", remove).clicked() {
        frame.push(Action::Review(ReviewAct::RemoveDraft(draft.id)));
    }
    frame.attach_tip(remove);
    frame.finish();
    frame.note(draft.text.as_str(), TEXT, Padding::Step);
}

pub(super) fn replies_to<'model>(model: &'model Model, thread: &Thread) -> Vec<&'model Draft> {
    model
        .review
        .opened()
        .into_iter()
        .flat_map(|opened| opened.review.drafts())
        .filter(|draft| draft.reply.as_ref() == Some(&thread.id))
        .collect()
}

pub(super) fn thread_box(model: &Model, frame: &mut Frame<'_>, thread: &Thread) {
    frame.start(Container::Comment {
        look: thread_look(thread.state),
        id: ids::thread_box().with(thread.id.as_str()),
    });
    frame.start(Container::FillRow);
    frame.label_runs(vec![
        Run::new(
            Counted::new(Count::new(thread.notes.len()), Noun::Comment).to_string(),
            WEAK,
        ),
        Run::new(state_word(thread.state), WEAK),
    ]);
    frame.grow();
    if let Some(place) = &thread.place {
        let reply = ids::REPLY.with(&Label::new(thread.id.as_str()));
        if frame.small_button("reply", reply).clicked() {
            frame.push(Action::Review(ReviewAct::StartDraft(DraftOn {
                file: place.file.clone(),
                side: place.side,
                line: place.line,
                reply: Some(thread.id.clone()),
                start: None,
            })));
        }
        frame.attach_tip(reply);
    }
    frame.finish();
    notes(frame, thread);
    for draft in replies_to(model, thread) {
        pending_reply(frame, draft);
    }
    frame.finish();
}

pub(super) fn draft_box(frame: &mut Frame<'_>, draft: &Draft) {
    frame.start(Container::Comment {
        look: CommentLook::Saved,
        id: ids::draft_box().with(&format!("box{}", draft.id)),
    });
    frame.start(Container::FillRow);
    let what = if draft.reply.is_some() {
        "your reply"
    } else {
        "your comment"
    };
    frame.label_runs(vec![
        Run::new(what, ORANGE),
        Run::new(
            format!(
                " on {}{}  draft, sent with the review",
                lines_word(draft.lines()).as_str(),
                side_word(draft.side).as_str()
            ),
            WEAK,
        ),
    ]);
    frame.grow();
    let remove = ids::DRAFT_REMOVE.with(&Label::new(draft.id.to_string()));
    if frame.small_button("remove", remove).clicked() {
        frame.push(Action::Review(ReviewAct::RemoveDraft(draft.id)));
    }
    frame.attach_tip(remove);
    frame.finish();
    frame.note(draft.text.as_str(), TEXT, Padding::Step);
    frame.finish();
}

pub(super) fn editing_box(
    model: &Model,
    frame: &mut Frame<'_>,
    on: &DraftOn,
    width: Option<ui::Px>,
) {
    let cells = width.map_or(COMMENT_FIELD_GUESS, |width| {
        Count::new(usize::try_from(width.ratio(frame.cell_width())).unwrap_or(0))
    });
    let room = Count::new(
        cells
            .get()
            .saturating_sub(COMMENT_BUTTONS_ROOM.get())
            .max(COMMENT_LEAST_FIELD.get()),
    );
    frame.start(Container::Comment {
        look: CommentLook::Draft,
        id: ids::draft_box().with("editing"),
    });
    let what = if on.reply.is_some() {
        "reply to the thread"
    } else {
        "new comment"
    };
    frame.label_runs(vec![
        Run::new(
            format!(
                "{what} on {}{}",
                lines_word(on.lines()).as_str(),
                side_word(on.side).as_str()
            ),
            TEXT,
        ),
        Run::new("  ctrl+enter adds it, escape cancels", WEAK),
    ]);
    frame.start(Container::StepRow {
        selected: Chosen::Plain,
    });
    frame.field_named(
        &model.fields,
        Which::Draft,
        ids::DRAFT_FIELD.id(),
        &Label::new("what should change here, or a question"),
        Cells::of_count(room.get()),
    );
    let add = ids::DRAFT_ADD.target();
    if frame.small_button("add", add).clicked() {
        frame.push(Action::Review(ReviewAct::SaveDraft));
    }
    frame.attach_tip(add);
    let cancel = ids::DRAFT_CANCEL.target();
    if frame.small_button("cancel", cancel).clicked() {
        frame.push(Action::Review(ReviewAct::CancelDraft));
    }
    frame.attach_tip(cancel);
    frame.finish();
    frame.finish();
}

fn no_review(model: &Model, frame: &mut Frame<'_>) {
    frame.title("Review");
    if let Some(number) = model.review.opening {
        frame.note(
            format!("opening #{number}: fetching it, checking out its head and indexing it"),
            ORANGE,
            Padding::Step,
        );
        return;
    }
    frame.note(
        "pick a pull request in the Inbox. It is fetched and checked out in a tree of its own, \
         so the Source view shows its head with go to definition, hover and references working \
         on it. Its description and every discussion show here; every changed file is listed in \
         Changes; mark each one seen as you go. A seen file turns unseen again when the pull \
         request changes it. Comments and replies are drafts until you submit them here.",
        NOTE,
        Padding::Step,
    );
}

fn heading(frame: &mut Frame<'_>, text: &Label) {
    frame.step_gap();
    frame.label(text.as_str(), ACCENT);
}

fn conversation(model: &Model, frame: &mut Frame<'_>, open: &OpenReview) {
    let talk = match &open.talk {
        Talk::Asking => {
            frame.note("reading the discussion...", WEAK, Padding::Step);
            return;
        }
        Talk::Failed(why) => {
            frame.note(why.as_str(), RED, Padding::Step);
            return;
        }
        Talk::Got(talk) => talk,
    };
    heading(frame, &Label::new("Description"));
    if talk.description.is_empty() {
        frame.note("(no description)", WEAK, Padding::Step);
    } else {
        frame.note(talk.description.as_str(), TEXT, Padding::Step);
    }
    frame.start(Container::ToolbarSmall);
    frame.label(
        format!(
            "Conversation  {}",
            Counted::new(Count::new(talk.threads.len()), Noun::Thread)
        ),
        ACCENT,
    );
    let refresh = ids::TALK_REFRESH.target();
    if frame.small_button("refresh", refresh).clicked() {
        frame.push(Action::Review(ReviewAct::RefreshTalk));
    }
    frame.attach_tip(refresh);
    frame.finish();
    if talk.threads.is_empty() {
        frame.note("nobody has commented yet", WEAK, Padding::Step);
    }
    for (position, thread) in talk.threads.iter().enumerate() {
        if let Some(place) = &thread.place {
            let target = ids::THREAD_ROW.nth(Count::new(position));
            let runs = vec![
                Run::new(
                    format!(
                        "{}:{}{}",
                        place.file,
                        place.line.number(),
                        side_word(place.side).as_str()
                    ),
                    ACCENT,
                ),
                Run::new(state_word(thread.state), WEAK),
            ];
            if frame.row(runs, target, Chosen::Plain).clicked() {
                frame.push(Action::Review(ReviewAct::OpenPlace(place.clone())));
            }
            frame.attach_tip(target);
        }
        thread_box(model, frame, thread);
    }
}

fn your_review(model: &Model, frame: &mut Frame<'_>, open: &OpenReview) {
    let drafts = open.opened.review.drafts();
    heading(
        frame,
        &Label::new(format!(
            "Your review  {}",
            Counted::new(Count::new(drafts.len()), Noun::Draft)
        )),
    );
    for (position, draft) in drafts.iter().enumerate() {
        let lead = if draft.reply.is_some() { "reply " } else { "" };
        let runs = vec![
            Run::new(
                format!(
                    "{lead}{} {}{}  ",
                    draft.file,
                    lines_word(draft.lines()).as_str(),
                    side_word(draft.side).as_str()
                ),
                ACCENT,
            ),
            Run::new(draft.text.as_str().lines().next().unwrap_or_default(), TEXT),
        ];
        let remove = ids::DRAFT_REMOVE.with(&Label::new(draft.id.to_string()));
        let clicks = frame.row_with_action(
            runs,
            ids::REVIEW_DRAFT.nth(Count::new(position)),
            None,
            Some(RowAction::new("remove", remove)),
        );
        if clicks.acted() {
            frame.push(Action::Review(ReviewAct::RemoveDraft(draft.id)));
        } else if clicks.row.clicked() {
            frame.push(Action::Review(ReviewAct::OpenPlace(domain::CodePlace {
                file: draft.file.clone(),
                side: draft.side,
                line: draft.line,
            })));
        }
    }
    frame.field(
        &model.fields,
        Which::Summary,
        &Label::new("what the review says as a whole, or an answer to the conversation"),
        SUMMARY_FIELD,
    );
    frame.start(Container::ToolbarSmall);
    if model.review.submitting == Submitting::Sending {
        frame.label("submitting...", ORANGE);
    } else {
        for (word, target, decision) in [
            ("comment", ids::SUBMIT_COMMENT.target(), Decision::Comment),
            ("approve", ids::SUBMIT_APPROVE.target(), Decision::Approve),
            (
                "request changes",
                ids::SUBMIT_CHANGES.target(),
                Decision::RequestChanges,
            ),
        ] {
            if frame.small_button(word, target).clicked() {
                frame.push(Action::Review(ReviewAct::Submit(decision)));
            }
            frame.attach_tip(target);
        }
    }
    frame.finish();
}

pub(super) fn review_tab(model: &Model, frame: &mut Frame<'_>) {
    let Some(open) = &model.review.open else {
        no_review(model, frame);
        return;
    };
    let pull = &open.pull;
    let opened = &open.opened;
    let id = ids::review();
    frame.scroll_column(id, model.scrolls.get(id), Scroller::Plain, None);
    frame.title(format!(
        "{}#{} {}",
        open.repo.name(),
        pull.number,
        pull.title
    ));
    frame.label_runs(vec![
        Run::new(format!("by {}", pull.author), WEAK),
        Run::new(format!("   {} <- {}", pull.base, pull.head), TEXT),
        Run::new(format!("   {}..{}", opened.base, opened.head), WEAK),
        Run::new(format!("   updated {}", pull.updated), WEAK),
    ]);
    let progress = opened.review.progress(&opened.changes);
    let standing = opened.review.standing(&pull.head_commit, progress);
    frame.label_runs(vec![
        Run::new(standing_word(standing), standing_color(standing)),
        Run::new(
            format!("   {progress} changed files seen"),
            if progress.done() { GREEN } else { ORANGE },
        ),
    ]);
    conversation(model, frame, open);
    your_review(model, frame, open);
    frame.finish();
}

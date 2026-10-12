use domain::{
    Block, ChangeKind, Content, Conversation, Decision, Draft, DraftOn, DraftText, Login, Note,
    RelativePath, Seen, Side, Span, Submission, Thread, ThreadState,
};
use ui::{Color, Count, Icon, Label, Run};

use domain::{CommitId, DiffLayout, Linked, Prose, WebLink};

use crate::diffs::Loading;

use crate::action::Action;
use crate::field::Which;
use crate::ids::{self, Control};
use crate::model::Model;
use crate::review::{OpenReview, ReviewAct, Submitting, Talk};
use crate::text::{Counted, Noun};
use crate::theme::{
    ACCENT, COMMENT_BUTTONS_ROOM, COMMENT_FIELD_GUESS, COMMENT_LEAST_FIELD, Cells, GREEN, NOTE,
    ORANGE, RED, ROW_EXTRA, SELECTED, SUMMARY_FIELD, TEXT, WEAK,
};
use crate::widgets::{
    Chosen, CommentLook, Container, Enabled, Frame, Padding, RowAction, Scroller, TreeRow,
};

use super::inbox::{standing_color, standing_word};
use super::markdown::markdown;

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

fn commit_changes_window(model: &Model, frame: &mut Frame<'_>, commit: &CommitId) {
    let Some(opened) = model.review.opened() else {
        return;
    };
    frame.start(Container::ToolbarSmall);
    let all = ids::ALL_CHANGES.target();
    if frame.small_button("all changes", all).clicked() {
        frame.push(Action::Review(ReviewAct::ViewCommit(None)));
    }
    frame.attach_tip(all);
    let subject = opened
        .commits
        .iter()
        .find(|known| &known.id == commit)
        .map(|known| known.subject.to_string())
        .unwrap_or_default();
    frame.label_runs(vec![
        Run::new(format!(" {commit} "), ORANGE),
        Run::new(subject, TEXT),
    ]);
    frame.finish();
    let Some(changes) = model.review.diffs.commit_changes(commit) else {
        let note = match model.review.diffs.commits.get(commit) {
            Some(Loading::Failed(why)) => why.clone(),
            _ => Label::new("reading the commit's changes..."),
        };
        frame.note(note.as_str(), WEAK, Padding::Step);
        return;
    };
    let id = ids::changes();
    frame.scroll_column(id, model.scrolls.get(id), Scroller::Plain, None);
    let shown = model.shown_path();
    for (position, change) in changes.iter().enumerate() {
        let path = change.path();
        let row = TreeRow {
            level: Count::ZERO,
            runs: vec![Run::new(format!("  {path}"), TEXT)],
            badges: vec![
                Run::new(format!("+{}", change.added()), GREEN),
                Run::new(format!("-{}", change.removed()), RED),
            ],
            marked: (shown.as_ref() == Some(path)).then_some(SELECTED),
            action: None,
        };
        if frame
            .tree_row(row, ids::CHANGE_ROW.nth(Count::new(position)))
            .row
            .clicked()
        {
            frame.push(Action::Review(ReviewAct::OpenChange(path.clone())));
        }
    }
    frame.finish();
}

pub(super) fn commits_window(model: &Model, frame: &mut Frame<'_>) {
    let Some(open) = model.review.open.as_ref() else {
        frame.note(
            "open a pull request in the Inbox to list its commits",
            WEAK,
            Padding::Step,
        );
        return;
    };
    let viewing = model.review.diffs.commit();
    let id = ids::commits();
    frame.scroll_column(id, model.scrolls.get(id), Scroller::Plain, None);
    let all = TreeRow {
        level: Count::ZERO,
        runs: vec![Run::new(
            format!("all changes  {} files", open.opened.changes.len()),
            TEXT,
        )],
        badges: Vec::new(),
        marked: viewing.is_none().then_some(SELECTED),
        action: None,
    };
    if frame.tree_row(all, ids::ALL_CHANGES.target()).row.clicked() {
        frame.push(Action::Review(ReviewAct::ViewCommit(None)));
    }
    for (position, commit) in open.opened.commits.iter().enumerate() {
        let target = ids::COMMIT_ROW.nth(Count::new(position));
        let row = TreeRow {
            level: Count::ZERO,
            runs: vec![
                Run::new(format!("{} ", commit.id), ORANGE),
                Run::new(commit.subject.to_string(), TEXT),
                Run::new(format!("  {}  {}", commit.author, commit.when.day()), WEAK),
            ],
            badges: Vec::new(),
            marked: (viewing == Some(&commit.id)).then_some(SELECTED),
            action: open.link(&Linked::Commit(commit.id.clone())).map(|_| {
                RowAction::new(Icon::LinkExternal, ids::LINK_OPEN.nth(Count::new(position)))
            }),
        };
        let clicks = frame.tree_row(row, target);
        if clicks.acted()
            && let Some(link) = open.link(&Linked::Commit(commit.id.clone()))
        {
            frame.push(Action::Review(ReviewAct::OpenLink(link)));
        } else if clicks.row.clicked() {
            frame.push(Action::Review(ReviewAct::ViewCommit(Some(
                commit.id.clone(),
            ))));
        }
    }
    frame.finish();
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
    if let Some(commit) = model.review.diffs.commit() {
        commit_changes_window(model, frame, commit);
        return;
    }
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

fn layout_toggle(model: &Model, frame: &mut Frame<'_>) {
    let layout = ids::LAYOUT.target();
    let split = model.review.diffs.layout == DiffLayout::Split;
    if frame
        .button(
            "split",
            layout,
            if split { Chosen::Chosen } else { Chosen::Plain },
        )
        .clicked()
    {
        frame.push(Action::Review(ReviewAct::ToggleLayout));
    }
    frame.attach_tip(layout);
}

pub(super) fn change_controls(model: &Model, frame: &mut Frame<'_>, path: &RelativePath) {
    let Some(change) = model.review.shown_change(path) else {
        return;
    };
    frame.label_runs(vec![
        Run::new(kind_word(change.kind()), WEAK),
        Run::new(format!("  +{}", change.added()), GREEN),
        Run::new(format!(" -{}", change.removed()), RED),
    ]);
    layout_toggle(model, frame);
    if let Some(commit) = model.review.diffs.commit() {
        frame.label(format!("commit {commit}"), ORANGE);
        let all = ids::ALL_CHANGES.target();
        if frame.small_button("all changes", all).clicked() {
            frame.push(Action::Review(ReviewAct::ViewCommit(None)));
        }
        frame.attach_tip(all);
        let copy = ids::COPY_LINES.target();
        if frame.small_button("copy", copy).clicked() {
            frame.push(Action::Review(ReviewAct::CopyLines));
        }
        frame.attach_tip(copy);
        return;
    }
    let seen = model.review.seen(change);
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
        ("copy", ids::COPY_LINES.target(), ReviewAct::CopyLines),
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

pub(super) fn link_buttons(frame: &mut Frame<'_>, link: Option<WebLink>, key: &Label) {
    let Some(link) = link else {
        return;
    };
    let open = ids::LINK_OPEN.with(key);
    if frame
        .inline_button(
            vec![Run::new(
                format!(" {} ", Icon::LinkExternal.glyph().get()),
                WEAK,
            )],
            open,
        )
        .clicked()
    {
        frame.push(Action::Review(ReviewAct::OpenLink(link.clone())));
    }
    frame.attach_tip(open);
    let copy = ids::LINK_COPY.with(key);
    if frame
        .inline_button(
            vec![Run::new(format!(" {} ", Icon::Copy.glyph().get()), WEAK)],
            copy,
        )
        .clicked()
    {
        frame.push(Action::Review(ReviewAct::CopyLink(link)));
    }
    frame.attach_tip(copy);
}

fn note_head(model: &Model, frame: &mut Frame<'_>, note: &Note, key: &Label) {
    frame.start(Container::FillRow);
    author_button(model, frame, &note.author, key);
    frame.label_runs(vec![Run::new(format!("  {}", note.when), WEAK)]);
    link_buttons(frame, note.link.clone(), key);
    frame.finish();
}

fn author_button(model: &Model, frame: &mut Frame<'_>, author: &Login, key: &Label) {
    let target = ids::AUTHOR.with(key);
    if frame
        .inline_button(vec![Run::new(author.as_str(), ACCENT)], target)
        .clicked()
        && let Some(link) = model
            .review
            .open
            .as_ref()
            .and_then(|open| open.link(&Linked::User(author.clone())))
    {
        frame.push(Action::Review(ReviewAct::OpenLink(link)));
    }
    frame.attach_tip(target);
}

fn decision_run(decision: Decision) -> Run {
    match decision {
        Decision::Approve => Run::new("  approved", GREEN),
        Decision::RequestChanges => Run::new("  requested changes", RED),
        Decision::Comment => Run::new("  commented", WEAK),
    }
}

fn submission_box(
    model: &Model,
    frame: &mut Frame<'_>,
    talk: &Conversation,
    submission: &Submission,
    threads: &[&Thread],
) {
    let key = Label::new(format!("submission/{}", submission.id));
    frame.start(Container::Comment {
        look: CommentLook::Thread,
        id: ids::thread_box().with(key.as_str()),
    });
    frame.start(Container::FillRow);
    author_button(model, frame, &submission.author, &key);
    frame.label_runs(vec![
        decision_run(submission.decision),
        Run::new(format!("  {}", submission.when), WEAK),
    ]);
    link_buttons(frame, submission.link.clone(), &key);
    frame.finish();
    if !submission.summary.is_empty() {
        markdown(model, frame, &submission.summary, &key);
    }
    for thread in threads {
        place_row(frame, talk, thread);
        thread_box(model, frame, thread);
    }
    frame.finish();
}

fn place_row(frame: &mut Frame<'_>, talk: &Conversation, thread: &Thread) {
    let Some(place) = &thread.place else {
        return;
    };
    let position = talk
        .threads
        .iter()
        .position(|known| known.id == thread.id)
        .unwrap_or_default();
    let target = ids::THREAD_ROW.nth(Count::new(position));
    let outdated = thread.state == ThreadState::Outdated;
    let runs = vec![
        Run::new(
            format!(
                "{}:{}{}",
                place.file,
                place.line.number(),
                side_word(place.side).as_str()
            ),
            if outdated { WEAK } else { ACCENT },
        ),
        Run::new(state_word(thread.state), WEAK),
    ];
    if outdated {
        frame.text_line(runs);
    } else {
        if frame.row(runs, target, Chosen::Plain).clicked() {
            frame.push(Action::Review(ReviewAct::OpenPlace(place.clone())));
        }
        frame.attach_tip(target);
    }
}

fn notes(model: &Model, frame: &mut Frame<'_>, thread: &Thread) {
    for (position, note) in thread.notes.iter().enumerate() {
        note_head(
            model,
            frame,
            note,
            &Label::new(format!("{}/{position}", thread.id)),
        );
        if !note.body.is_empty() {
            markdown(
                model,
                frame,
                &note.body,
                &Label::new(format!("{}/{position}", thread.id)),
            );
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

fn pending_reply(model: &Model, frame: &mut Frame<'_>, draft: &Draft) {
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
    markdown(
        model,
        frame,
        &Prose::new(draft.text.as_str()),
        &Label::new(format!("reply/{}", draft.id)),
    );
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
    notes(model, frame, thread);
    for draft in replies_to(model, thread) {
        pending_reply(model, frame, draft);
    }
    frame.finish();
}

pub(super) fn draft_box(model: &Model, frame: &mut Frame<'_>, draft: &Draft) {
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
    markdown(
        model,
        frame,
        &Prose::new(draft.text.as_str()),
        &Label::new(format!("draft/{}", draft.id)),
    );
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
        markdown(model, frame, &talk.description, &Label::new("description"));
    }
    let blocks = talk.blocks();
    let reviews = blocks
        .iter()
        .filter(|block| matches!(block, Block::Submitted { .. }))
        .count();
    frame.start(Container::ToolbarSmall);
    frame.label(
        format!(
            "Conversation  {}, {}",
            Counted::new(Count::new(reviews), Noun::Submission),
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
    if blocks.is_empty() {
        frame.note("nobody has commented yet", WEAK, Padding::Step);
    }
    for (position, block) in blocks.iter().enumerate() {
        if position > 0 {
            frame.step_gap();
        }
        match block {
            Block::Submitted {
                submission,
                threads,
            } => submission_box(model, frame, talk, submission, threads),
            Block::Alone(thread) => {
                place_row(frame, talk, thread);
                thread_box(model, frame, thread);
            }
        }
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
    match model.review.submitting {
        Submitting::Sending => frame.label("submitting...", ORANGE),
        Submitting::Idle => submit_buttons(model, frame, open),
        Submitting::Asking(decision) => ask_submit(model, frame, open, decision),
    }
    frame.finish();
}

struct SubmitButton {
    word: Label,
    control: Control,
    tint: Color,
}

fn submit_button(decision: Decision) -> SubmitButton {
    let (word, control, tint) = match decision {
        Decision::Comment => ("comment", ids::SUBMIT_COMMENT, TEXT),
        Decision::Approve => ("approve", ids::SUBMIT_APPROVE, GREEN),
        Decision::RequestChanges => ("request changes", ids::SUBMIT_CHANGES, RED),
    };
    SubmitButton {
        word: Label::new(word),
        control,
        tint,
    }
}

fn summary_text(model: &Model) -> Option<DraftText> {
    DraftText::new(model.fields.get(Which::Summary).text().as_str())
}

fn submit_buttons(model: &Model, frame: &mut Frame<'_>, open: &OpenReview) {
    let summary = summary_text(model);
    let content = Content::of(open.opened.review.drafts(), summary.as_ref());
    let rights = open.rights.clone().unwrap_or_default();
    for decision in [
        Decision::Comment,
        Decision::Approve,
        Decision::RequestChanges,
    ] {
        let SubmitButton {
            word,
            control,
            tint,
        } = submit_button(decision);
        let target = control.target();
        let refusal = rights.refusal(decision, content);
        let enabled = if refusal.is_some() {
            Enabled::Disabled
        } else {
            Enabled::Enabled
        };
        if frame.tinted_button(word, target, tint, enabled).clicked() && refusal.is_none() {
            frame.push(Action::Review(ReviewAct::AskSubmit(decision)));
        }
        match refusal {
            Some(refusal) => frame.attach_tip_text(target, Label::new(refusal.to_string())),
            None => frame.attach_tip(target),
        }
    }
}

fn submit_question(model: &Model, open: &OpenReview, decision: Decision) -> Label {
    let verb = match decision {
        Decision::Comment => "comment on",
        Decision::Approve => "approve",
        Decision::RequestChanges => "request changes on",
    };
    let drafts = open.opened.review.drafts().len();
    let comments = Counted::new(Count::new(drafts), Noun::Comment);
    let with = match (drafts, summary_text(model).is_some()) {
        (0, false) => String::new(),
        (0, true) => " with the summary".to_owned(),
        (_, false) => format!(" with {comments}"),
        (_, true) => format!(" with {comments} and the summary"),
    };
    Label::new(format!("{verb} #{}{with}?", open.pull.number))
}

fn ask_submit(model: &Model, frame: &mut Frame<'_>, open: &OpenReview, decision: Decision) {
    let SubmitButton { word, tint, .. } = submit_button(decision);
    frame.label(submit_question(model, open, decision), TEXT);
    let confirm = ids::SUBMIT_CONFIRM.target();
    if frame
        .tinted_button(word, confirm, tint, Enabled::Enabled)
        .clicked()
    {
        frame.push(Action::Review(ReviewAct::Submit(decision)));
    }
    frame.attach_tip(confirm);
    let cancel = ids::SUBMIT_CANCEL.target();
    if frame.small_button("cancel", cancel).clicked() {
        frame.push(Action::Review(ReviewAct::CancelSubmit));
    }
    frame.attach_tip(cancel);
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
    frame.start(Container::FillRow);
    frame.title(format!(
        "{}#{} {}",
        open.repo.name(),
        pull.number,
        pull.title
    ));
    link_buttons(
        frame,
        open.link(&Linked::Pull(pull.number)),
        &Label::new("pull"),
    );
    frame.finish();
    frame.label_runs(vec![
        Run::new(format!("by {}", pull.author), WEAK),
        Run::new(format!("   {} <- {}", pull.base, pull.head), TEXT),
        Run::new(format!("   {}..{}", opened.base, opened.head), WEAK),
        Run::new(format!("   updated {}", pull.updated), WEAK),
    ]);
    let progress = opened.review.progress(&opened.changes);
    let standing = opened.review.standing(&pull.head_commit, Some(progress));
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

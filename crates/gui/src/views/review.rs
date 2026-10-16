use std::rc::Rc;

use domain::{
    Block, CardFamily, CardKey, CardStage, ChangeKind, CodePlace, Content, Conversation, Decision,
    Draft, DraftOn, DraftText, FileChange, Line, Login, Note, RelativePath, Rights, Seen, Side,
    Span, Submission, Thread, ThreadState,
};
use ui::{Canvas, Color, Count, Grid, Icon, Label, Point, Rect, Run};

use domain::{CommitId, DiffLayout, Linked, Prose, WebLink};

use crate::diffs::Loading;

use crate::action::Action;
use crate::field::Which;
use crate::grid::{self, Gutter};
use crate::ids::{self, Control};
use crate::keys;
use crate::model::Model;
use crate::nav::Kept;
use crate::review::{CardCollapse, OpenReview, ReviewAct, Submitting, Talk};
use crate::text::{Counted, Noun};
use crate::theme::{
    self, ACCENT, BLOCK_SPACING, BORDER, COMMENT_BUTTONS_ROOM, COMMENT_FIELD_GUESS,
    COMMENT_LEAST_FIELD, Cells, FIELD, GREEN, LINES_SELECTED, NOTE, NOTE_GAP, ORANGE, RED,
    ROW_EXTRA, SECTION_GAP, SELECTED, SMALL_GAP, STRONG_TEXT, SUMMARY_FIELD, TEXT, VIOLET, WEAK,
};
use crate::widgets::{
    Chosen, CommentLook, Container, Enabled, Frame, Padding, RowAction, Scroller, TreeRow,
};

use super::inbox::{standing_color, standing_word};
use super::markdown::markdown;

pub(super) fn seen_mark(seen: Seen) -> Run {
    match seen {
        Seen::Seen => Run::new(Icon::Check, GREEN),
        Seen::Unseen => Run::new(Icon::Dot, ORANGE),
        Seen::Changed => Run::new("~", ORANGE),
    }
}

pub(super) fn kind_word(kind: ChangeKind) -> Label {
    Label::new(match kind {
        ChangeKind::Added => "added",
        ChangeKind::Deleted => "deleted",
        ChangeKind::Modified => "modified",
        ChangeKind::Renamed => "renamed",
    })
}

fn span_numbers(lines: Span) -> Label {
    Label::new(if lines.start() == lines.end() {
        lines.end().number().to_string()
    } else {
        format!("{}-{}", lines.start().number(), lines.end().number())
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
    let all = ids::ALL_CHANGES.with(&Label::new("changes"));
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
        if opened.generated.contains(path) {
            runs.push(Run::new("  generated", WEAK));
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

fn layout_toggle(model: &Model, frame: &mut Frame<'_>, kept: Option<Kept>) {
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
        frame.push(Action::Review(ReviewAct::ToggleLayout(kept)));
    }
    frame.attach_tip(layout);
}

pub(super) fn diff_controls(model: &Model, frame: &mut Frame<'_>, kept: Option<Kept>) {
    layout_toggle(model, frame, kept);
    if let Some(commit) = model.review.diffs.commit() {
        frame.label(format!("commit {commit}"), ORANGE);
        let all = ids::ALL_CHANGES.with(&Label::new("source"));
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
    for (label, target, act) in [
        ("prev", ids::PREVIOUS_HUNK.target(), ReviewAct::PreviousHunk),
        ("next", ids::NEXT_HUNK.target(), ReviewAct::NextHunk),
        (
            "next unseen",
            ids::NEXT_UNSEEN.with(&Label::new("source")),
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
}

pub(super) fn seen_button(model: &Model, frame: &mut Frame<'_>, change: &FileChange) {
    let toggle = ids::SEEN.with(&Label::new(change.path().as_str()));
    let (word, chosen) = match model.review.seen(change) {
        Seen::Seen => ("seen", Chosen::Chosen),
        Seen::Changed => ("changed since seen", Chosen::Plain),
        Seen::Unseen => ("mark seen", Chosen::Plain),
    };
    if frame.button(word, toggle, chosen).clicked() {
        frame.push(Action::Review(ReviewAct::ToggleSeen(change.path().clone())));
    }
    frame.attach_tip(toggle);
}

pub(super) fn whole_button(model: &Model, frame: &mut Frame<'_>, path: &RelativePath) {
    let whole = ids::WHOLE_FILE.with(&Label::new(path.as_str()));
    let chosen = if model.review.whole.contains(path) {
        Chosen::Chosen
    } else {
        Chosen::Plain
    };
    if frame.button("whole file", whole, chosen).clicked() {
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
        .link(vec![Run::new(author.as_str(), ACCENT)], target)
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
        place_row(model, frame, talk, thread);
        thread_box(model, frame, thread);
    }
    frame.finish();
}

fn card(model: &Model, frame: &mut Frame<'_>, open: &OpenReview) {
    let found = match &open.card {
        Loading::Got(Some(found)) => found,
        Loading::Got(None) => return,
        Loading::Asking => {
            if let Some(key) = CardKey::of_pull(&open.pull) {
                frame.note(format!("asking Jira for {key}..."), WEAK, Padding::Step);
            }
            return;
        }
        Loading::Failed(why) => {
            frame.note(why.as_str(), ORANGE, Padding::Step);
            return;
        }
    };
    let family = family_color(found.family);
    let collapse = model.review.card_collapse;
    frame.start(Container::Card(ids::card_box()));
    frame.stripe(family);
    frame.start(Container::CardBody);
    frame.start(Container::FillRow);
    let toggle = ids::CARD_COLLAPSE.target();
    let chevron = match collapse {
        CardCollapse::Open => Icon::Expanded,
        CardCollapse::Collapsed => Icon::Collapsed,
    };
    if frame
        .inline_button(
            vec![
                Run::new(chevron, WEAK),
                Run::new(" ", WEAK),
                Run::new(family_icon(found.family), family),
                Run::new(format!(" {} ", found.kind), family),
            ],
            toggle,
        )
        .clicked()
    {
        frame.push(Action::Review(ReviewAct::CollapseCard));
    }
    frame.attach_tip(toggle);
    let target = ids::CARD_LINK.target();
    if frame
        .link(vec![Run::new(found.key.as_str(), ACCENT)], target)
        .clicked()
        && let Some(link) = &found.link
    {
        frame.push(Action::Review(ReviewAct::OpenLink(link.clone())));
    }
    frame.attach_tip(target);
    frame.label("", WEAK);
    frame.pill(
        format!(" {} ", found.status.as_str().to_uppercase()),
        stage_color(found.stage),
    );
    frame.grow();
    let assignee = found
        .assignee
        .as_ref()
        .map_or_else(|| "unassigned".to_owned(), ToString::to_string);
    frame.label_runs(vec![
        Run::new(Icon::Person, WEAK),
        Run::new(format!(" {assignee}"), WEAK),
    ]);
    frame.finish();
    frame.title(found.summary.as_str());
    frame.spacer(SMALL_GAP);
    if collapse == CardCollapse::Open && !found.description.as_str().is_empty() {
        markdown(model, frame, &found.description, &Label::new("card"));
    }
    frame.finish();
    frame.finish();
}

const fn family_color(family: CardFamily) -> Color {
    match family {
        CardFamily::Bug => RED,
        CardFamily::Story => GREEN,
        CardFamily::Task => ACCENT,
        CardFamily::Epic => VIOLET,
        CardFamily::Other => WEAK,
    }
}

const fn family_icon(family: CardFamily) -> Icon {
    match family {
        CardFamily::Bug => Icon::Bug,
        CardFamily::Story => Icon::Bookmark,
        CardFamily::Task => Icon::Check,
        CardFamily::Epic => Icon::Zap,
        CardFamily::Other => Icon::Dot,
    }
}

const fn stage_color(stage: CardStage) -> Color {
    match stage {
        CardStage::ToDo => WEAK,
        CardStage::InProgress => ACCENT,
        CardStage::Done => GREEN,
    }
}

fn place_row(model: &Model, frame: &mut Frame<'_>, talk: &Conversation, thread: &Thread) {
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
    let spot = Run::new(
        format!(
            "{}:{}{}",
            place.file,
            span_numbers(place.lines()).as_str(),
            side_word(place.side).as_str()
        ),
        if outdated { WEAK } else { ACCENT },
    );
    let state = Run::new(state_word(thread.state), WEAK);
    if outdated {
        frame.text_line(vec![spot, state]);
    } else {
        frame.start(Container::FillRow);
        if frame.link(vec![spot], target).clicked() {
            frame.push(Action::Review(ReviewAct::OpenPlace(place.clone())));
        }
        frame.attach_tip(target);
        frame.label_runs(vec![state]);
        frame.finish();
        snippet(model, frame, &Label::new(thread.id.as_str()), place);
    }
}

const SNIPPET_ABOVE: Count = Count::new(2);

struct Snippet {
    grids: Vec<Grid>,
    marked: Count,
}

fn snippet_of(model: &Model, place: &CodePlace) -> Option<Snippet> {
    let lines = place.lines();
    match place.side {
        Side::New => {
            let source = model
                .index
                .find_file(&place.file)
                .and_then(|file| model.index.file(file))?;
            let last = source.text().count().last()?;
            let first = lines
                .start()
                .value()
                .saturating_sub(u32::try_from(SNIPPET_ABOVE.get()).unwrap_or(0));
            let span = Span::new(Line::new(first), place.line.min(last))?;
            let marked = lines.start().value() - first;
            Some(Snippet {
                grids: vec![grid::build(source, span, Gutter::Numbered)],
                marked: Count::new(usize::try_from(marked).unwrap_or(0)),
            })
        }
        Side::Old => {
            let change = model.review.change(&place.file)?;
            let grids: Vec<Grid> = change
                .removals()
                .iter()
                .flat_map(|removal| removal.lines.iter())
                .filter(|removed| lines.contains(removed.old))
                .map(|removed| grid::removed(removed.old, removed.text, None))
                .collect();
            (!grids.is_empty()).then_some(Snippet {
                grids,
                marked: Count::ZERO,
            })
        }
    }
}

fn snippet(model: &Model, frame: &mut Frame<'_>, key: &Label, place: &CodePlace) {
    let Some(snippet) = snippet_of(model, place) else {
        return;
    };
    let grids: Vec<Rc<Grid>> = snippet.grids.into_iter().map(Rc::new).collect();
    let row = frame.row_height();
    let font = frame.metrics.font;
    let pad = frame.cell_width() / 2;
    let rows = grids
        .iter()
        .fold(Count::ZERO, |rows, grid| rows + grid.rows());
    let marked = snippet.marked;
    let draw = move |canvas: &mut Canvas<'_>, rect: Rect| {
        canvas.rect(rect, FIELD);
        canvas.rect(
            Rect::new(
                rect.left,
                rect.top + pad + row * marked,
                rect.width,
                row * Count::new(rows.get().saturating_sub(marked.get())),
            ),
            LINES_SELECTED,
        );
        let mut top = rect.top + pad;
        for grid in &grids {
            canvas.grid(Point::new(rect.left + pad, top), font, grid);
            top += row * grid.rows();
        }
    };
    frame.custom_block(
        Box::new(draw),
        row * rows + pad * 2,
        ids::snippet().with(key.as_str()),
    );
}

fn notes(model: &Model, frame: &mut Frame<'_>, thread: &Thread) {
    for (position, note) in thread.notes.iter().enumerate() {
        if position > 0 {
            frame.spacer(NOTE_GAP / 2);
            frame.rule(BORDER);
            frame.spacer(NOTE_GAP / 2);
        }
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
    draft_buttons(frame, draft);
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
        .filter(|draft| {
            draft.reply.as_ref() == Some(&thread.id) && model.review.editing != Some(draft.id)
        })
        .collect()
}

fn draft_buttons(frame: &mut Frame<'_>, draft: &Draft) {
    let key = Label::new(draft.id.to_string());
    let edit = ids::DRAFT_EDIT.with(&key);
    if frame.small_button("edit", edit).clicked() {
        frame.push(Action::Review(ReviewAct::EditDraft(draft.id)));
    }
    frame.attach_tip(edit);
    let remove = ids::DRAFT_REMOVE.with(&key);
    if frame.small_button("remove", remove).clicked() {
        frame.push(Action::Review(ReviewAct::RemoveDraft(draft.id)));
    }
    frame.attach_tip(remove);
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
    draft_buttons(frame, draft);
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
    let what = match (model.review.editing, on.reply.is_some()) {
        (Some(id), _) => format!("draft {id}"),
        (None, true) => "reply to the thread".to_owned(),
        (None, false) => "new comment".to_owned(),
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
        Run::new(
            format!(
                "  {}+enter {} it, escape cancels",
                keys::PRIMARY_NAME.as_str(),
                if model.review.editing.is_some() {
                    "saves"
                } else {
                    "adds"
                }
            ),
            WEAK,
        ),
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
    let word = if model.review.editing.is_some() {
        "save"
    } else {
        "add"
    };
    if frame.small_button(word, add).clicked() {
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
    frame.spacer(SECTION_GAP);
    let size = theme::heading_font(frame.metrics.font, Count::new(2));
    frame.sized_line(vec![Run::new(text.as_str(), STRONG_TEXT)], size);
    frame.rule(BORDER);
    frame.spacer(SMALL_GAP);
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
    heading(frame, &Label::new("Conversation"));
    frame.start(Container::ToolbarSmall);
    frame.label(
        format!(
            "{}, {}",
            Counted::new(Count::new(reviews), Noun::Submission),
            Counted::new(Count::new(talk.threads.len()), Noun::Thread)
        ),
        WEAK,
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
            frame.spacer(BLOCK_SPACING);
        }
        match block {
            Block::Submitted {
                submission,
                threads,
            } => submission_box(model, frame, talk, submission, threads),
            Block::Alone(thread) => {
                place_row(model, frame, talk, thread);
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
        let place = CodePlace {
            file: draft.file.clone(),
            side: draft.side,
            start: draft.start,
            line: draft.line,
        };
        if clicks.acted() {
            frame.push(Action::Review(ReviewAct::RemoveDraft(draft.id)));
        } else if clicks.row.clicked() {
            frame.push(Action::Review(ReviewAct::OpenPlace(place.clone())));
        }
        if draft.reply.is_none() {
            snippet(
                model,
                frame,
                &Label::new(format!("draft/{}", draft.id)),
                &place,
            );
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
    let asking = Rights::default();
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
        let refusal = match &open.rights {
            Some(Err(unknown)) => Some(unknown.clone()),
            known => known
                .as_ref()
                .and_then(|rights| rights.as_ref().ok())
                .unwrap_or(&asking)
                .refusal(decision, content)
                .map(|refusal| Label::new(refusal.to_string())),
        };
        let enabled = if refusal.is_some() {
            Enabled::Disabled
        } else {
            Enabled::Enabled
        };
        if frame.tinted_button(word, target, tint, enabled).clicked() && refusal.is_none() {
            frame.push(Action::Review(ReviewAct::AskSubmit(decision)));
        }
        match refusal {
            Some(refusal) => frame.attach_tip_text(target, refusal),
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
    card(model, frame, open);
    conversation(model, frame, open);
    your_review(model, frame, open);
    frame.finish();
}

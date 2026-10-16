use domain::{
    CodePlace, Draft, DraftOn, FileId, Line, LineCount, Linked, LinkedFile, RelativePath, Removal,
    Revision, Side, SourceFile, SourceLine, Span, Thread,
};
use ui::{Count, Icon, Id, Label, Px, Rect, Run};

use crate::action::Action;
use crate::diffs::{Loading, SidesKey};
use crate::field::Which;
use crate::grid::GUTTER;
use crate::ids::{self, Target};
use crate::model::{HIT_LIMIT, HitsShown, Model};
use crate::review::ReviewAct;
use crate::text::{Counted, Noun};
use crate::theme::{
    ACCENT, COMMENT_FRAME, DRAFT_FRAME, DRAFT_ROWS, EDGE_SCROLL_BAND, EDGE_SCROLL_MOST, ORANGE,
    PIXEL, ROW_EXTRA, SEARCH_FIELD, TEXT, WEAK,
};
use crate::widgets::{Chosen, Container, Frame, Padding, Scroller};

use super::review;

pub(super) fn source_toolbar(model: &Model, frame: &mut Frame<'_>, path: &RelativePath) {
    frame.start(Container::Toolbar);
    frame.label(path.as_str(), TEXT);
    let link = model.review.open.as_ref().and_then(|open| {
        open.link(&Linked::File(LinkedFile {
            at: Revision::Commit(open.opened.head.clone()),
            path: path.clone(),
            lines: model
                .nav
                .lines()
                .and_then(|lines| Span::new(lines.low(), lines.high())),
        }))
    });
    review::link_buttons(frame, link, &Label::new("file"));
    frame.finish();
}

pub(super) fn edge_scroll(vertical: Px, rect: Rect, row_height: Px) -> Px {
    let most = row_height * EDGE_SCROLL_MOST;
    if vertical < rect.top {
        -EDGE_SCROLL_BAND
            .apply(rect.top - vertical)
            .clamp(PIXEL, most)
    } else if vertical >= rect.bottom() {
        EDGE_SCROLL_BAND
            .apply(vertical - rect.bottom() + PIXEL)
            .clamp(PIXEL, most)
    } else {
        Px::ZERO
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum InsetAt {
    Top,
    After(Line),
}

impl InsetAt {
    fn before(line: Line) -> Self {
        line.previous().map_or(Self::Top, Self::After)
    }

    pub(super) const fn anchor(self) -> Option<Line> {
        match self {
            Self::Top => None,
            Self::After(line) => Some(line),
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct GoneLine<'rows> {
    pub(super) old: Line,
    pub(super) text: &'rows SourceLine,
}

struct RemovedBlock<'rows> {
    at: InsetAt,
    lines: Vec<GoneLine<'rows>>,
}

pub(super) fn gone_lines<'rows>(removal: &Removal<'rows>) -> Vec<GoneLine<'rows>> {
    removal
        .lines
        .iter()
        .map(|removed| GoneLine {
            old: removed.old,
            text: removed.text,
        })
        .collect()
}

fn removed_blocks<'rows>(removals: &[Removal<'rows>], last: Line) -> Vec<RemovedBlock<'rows>> {
    removals
        .iter()
        .map(|removal| RemovedBlock {
            at: if removal.before > last {
                InsetAt::After(last)
            } else {
                InsetAt::before(removal.before)
            },
            lines: gone_lines(removal),
        })
        .collect()
}

pub(super) enum InsetKind<'inset> {
    Removed(Vec<GoneLine<'inset>>),
    Thread(&'inset Thread),
    Draft(&'inset Draft),
    Editing(&'inset DraftOn),
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Layer {
    Removed,
    Thread,
    Draft,
    Editing,
}

impl InsetKind<'_> {
    pub(super) const fn layer(&self) -> Layer {
        match self {
            Self::Removed(_) => Layer::Removed,
            Self::Thread(_) => Layer::Thread,
            Self::Draft(_) => Layer::Draft,
            Self::Editing(_) => Layer::Editing,
        }
    }
}

pub(super) struct Inset<'inset> {
    pub(super) at: InsetAt,
    pub(super) height: Px,
    pub(super) kind: InsetKind<'inset>,
}

impl Inset<'_> {
    pub(super) fn row_id(&self) -> Option<Id> {
        match &self.kind {
            InsetKind::Removed(_) => None,
            InsetKind::Thread(thread) => {
                Some(ids::thread_box().with(thread.id.as_str()).with("row"))
            }
            InsetKind::Draft(draft) => {
                Some(ids::draft_box().nth(usize::try_from(draft.id.get()).unwrap_or(0)))
            }
            InsetKind::Editing(_) => Some(ids::draft_box().with("new")),
        }
    }
}

fn place_at(blocks: &[RemovedBlock<'_>], side: Side, line: Line) -> InsetAt {
    match side {
        Side::New => InsetAt::After(line),
        Side::Old => blocks
            .iter()
            .find(|block| block.lines.iter().any(|gone| gone.old == line))
            .map_or(InsetAt::After(line), |block| block.at),
    }
}

pub(super) struct Noted<'inset> {
    pub(super) side: Side,
    pub(super) line: Line,
    pub(super) inset: Inset<'inset>,
}

pub(super) fn noted<'inset>(
    model: &'inset Model,
    frame: &Frame<'_>,
    path: &'inset RelativePath,
) -> Vec<Noted<'inset>> {
    let row = frame.row_height();
    let mut all: Vec<Noted<'inset>> = Vec::new();
    let mut add = |side: Side, line: Line, guess: Px, kind: InsetKind<'inset>| {
        let inset = Inset {
            at: InsetAt::Top,
            height: guess,
            kind,
        };
        let height = inset
            .row_id()
            .and_then(|id| frame.ui.remembered(id))
            .map_or(inset.height, |extent| extent.height);
        all.push(Noted {
            side,
            line,
            inset: Inset { height, ..inset },
        });
    };
    let conversation = model
        .review
        .open
        .as_ref()
        .and_then(|open| open.conversation());
    let mut threaded = Vec::new();
    for thread in conversation
        .into_iter()
        .flat_map(|known| known.drawn_in(path))
    {
        let Some(CodePlace { side, line, .. }) = thread.place.clone() else {
            continue;
        };
        threaded.push(&thread.id);
        let replies: usize = review::replies_to(model, thread)
            .iter()
            .map(|draft| 1 + draft.text.as_str().lines().count())
            .sum();
        let tall: usize = thread
            .notes
            .iter()
            .map(|note| 1 + note.body.as_str().lines().count())
            .sum::<usize>()
            + replies;
        add(
            side,
            line,
            row * Count::new(tall + 1) + COMMENT_FRAME,
            InsetKind::Thread(thread),
        );
    }
    for draft in model.review.drafts_in(path).filter(|draft| {
        model.review.editing != Some(draft.id)
            && draft
                .reply
                .as_ref()
                .is_none_or(|id| !threaded.contains(&id))
    }) {
        let tall = 1 + draft.text.as_str().lines().count().max(1);
        add(
            draft.side,
            draft.line,
            row * Count::new(tall) + COMMENT_FRAME,
            InsetKind::Draft(draft),
        );
    }
    if let Some(on) = model.review.draft.as_ref().filter(|on| &on.file == path) {
        add(
            on.side,
            on.line,
            row * DRAFT_ROWS + DRAFT_FRAME,
            InsetKind::Editing(on),
        );
    }
    all
}

pub(super) fn insets<'inset>(
    model: &'inset Model,
    frame: &Frame<'_>,
    removals: &[Removal<'inset>],
    path: &'inset RelativePath,
    last: Line,
) -> Vec<Inset<'inset>> {
    let row = frame.row_height();
    let within = |at: InsetAt| match at {
        InsetAt::Top => InsetAt::Top,
        InsetAt::After(after) => InsetAt::After(after.min(last)),
    };
    let blocks = removed_blocks(removals, last);
    let mut all: Vec<Inset<'inset>> = Vec::new();
    for block in &blocks {
        all.push(Inset {
            at: within(block.at),
            height: row * Count::new(block.lines.len()),
            kind: InsetKind::Removed(block.lines.clone()),
        });
    }
    all.extend(noted(model, frame, path).into_iter().map(|noted| Inset {
        at: within(place_at(&blocks, noted.side, noted.line)),
        ..noted.inset
    }));
    all.sort_by_key(|inset| (inset.at, inset.kind.layer()));
    all
}

pub(super) fn comment_row(
    model: &Model,
    frame: &mut Frame<'_>,
    inset: &Inset<'_>,
    width: Option<Px>,
) {
    let Some(id) = inset.row_id() else {
        return;
    };
    let gutter = frame.cell_width() * GUTTER;
    frame.start(Container::CommentRow(id));
    frame.indent(gutter);
    match &inset.kind {
        InsetKind::Removed { .. } => {}
        InsetKind::Thread(thread) => review::thread_box(model, frame, thread),
        InsetKind::Draft(draft) => review::draft_box(model, frame, draft),
        InsetKind::Editing(on) => {
            review::editing_box(model, frame, on, width.map(|width| width - gutter));
        }
    }
    frame.finish();
}

pub(super) fn old_side<'model>(
    model: &'model Model,
    frame: &mut Frame<'_>,
    path: &RelativePath,
) -> Option<&'model SourceFile> {
    let key = SidesKey {
        commit: None,
        path: path.clone(),
    };
    match model.review.diffs.sides.get(&key) {
        Some(Loading::Got(sides)) => Some(&sides.old),
        Some(Loading::Asking | Loading::Failed(_)) => None,
        None => {
            frame.push(Action::Review(ReviewAct::NeedSides(key)));
            None
        }
    }
}

pub(super) fn text_columns(model: &Model, file: FileId, span: Span) -> Count {
    model.index.file(file).map_or(Count::ZERO, |source| {
        let text = source.text();
        span.lines()
            .filter_map(|line| text.line(line))
            .map(|line| Count::new(line.as_str().chars().count()))
            .max()
            .map_or(Count::ZERO, |widest| widest + GUTTER)
    })
}

pub(super) fn removed_columns<'inset>(
    insets: impl Iterator<Item = &'inset Inset<'inset>>,
) -> Count {
    insets
        .filter_map(|inset| match &inset.kind {
            InsetKind::Removed(lines) => lines
                .iter()
                .map(|gone| gone.text.as_str().chars().count())
                .max(),
            InsetKind::Thread(_) | InsetKind::Draft(_) | InsetKind::Editing(_) => None,
        })
        .max()
        .map_or(Count::ZERO, |widest| Count::new(widest) + GUTTER)
}

pub(super) fn line_count(line: Line) -> Count {
    Count::new(usize::try_from(line.value()).unwrap_or(0))
}

pub(super) fn removed_target(path: &RelativePath, old: Line) -> Target {
    ids::DRAFT_REMOVED.with(&ids::in_file(path, line_count(old) + Count::new(1)))
}

const FOLD_STEP: LineCount = LineCount::new(20);

#[derive(Debug, PartialEq, Eq)]
struct FoldButton {
    target: Target,
    run: Run,
    span: Span,
}

fn fold_buttons(path: &RelativePath, start: Line, end: Line, last: Line) -> Vec<FoldButton> {
    let count = end.value() - start.value() + 1;
    let all = Span::new(start, end).unwrap_or_else(|| Span::line(start));
    let at = |line: Line| ids::in_file(path, line_count(line));
    let step = count.min(FOLD_STEP.value());
    let mut buttons = Vec::new();
    if end < last
        && let Some(span) = Span::new(Line::new(end.value() + 1 - step), end)
    {
        buttons.push(FoldButton {
            target: ids::FOLD_UP.with(&at(start)),
            run: Run::new(
                format!(" {} {step} ", Icon::MoreAbove.glyph().get()),
                ACCENT,
            ),
            span,
        });
    }
    if start > Line::new(0)
        && let Some(span) = Span::new(start, Line::new(start.value() + step - 1))
    {
        buttons.push(FoldButton {
            target: ids::FOLD_DOWN.with(&at(end)),
            run: Run::new(
                format!(" {} {step} ", Icon::MoreBelow.glyph().get()),
                ACCENT,
            ),
            span,
        });
    }
    if count > FOLD_STEP.value() || buttons.is_empty() {
        buttons.push(FoldButton {
            target: ids::FOLD.with(&at(start)),
            run: Run::new(" all ", ACCENT),
            span: all,
        });
    }
    buttons
}

pub(super) fn fold(frame: &mut Frame<'_>, path: &RelativePath, start: Line, end: Line, last: Line) {
    let count = end.value() - start.value() + 1;
    frame.start(Container::Folded);
    frame.text_line(vec![Run::new(format!("{:>5} ", "\u{2026}"), WEAK)]);
    for FoldButton { target, run, span } in fold_buttons(path, start, end, last) {
        if frame.inline_button(vec![run], target).clicked() {
            frame.push(Action::Review(ReviewAct::Unfold(path.clone(), span)));
        }
        frame.attach_tip(target);
    }
    frame.text_line(vec![Run::new(
        format!(
            " {count} unchanged line{} ({}-{})",
            if count == 1 { "" } else { "s" },
            start.number(),
            end.number()
        ),
        WEAK,
    )]);
    frame.finish();
}

#[derive(Clone, Copy)]
pub(super) struct Band {
    pub(super) offset: Px,
    pub(super) bottom: Px,
}

pub(super) fn gap(frame: &mut Frame<'_>, height: Px) {
    if height > Px::ZERO {
        frame.spacer(height);
    }
}

pub(super) fn rows_into(top: Px, down: Px, row: Px) -> Count {
    Count::new(usize::try_from((down - top).ratio(row).max(0)).unwrap_or(0))
}

#[derive(Clone, Copy)]
pub(super) struct Stretch {
    pub(super) top: Px,
    pub(super) height: Px,
}

pub(super) enum Aimed {
    At(Px),
    Folded,
    Missing,
}

pub(super) fn into_view(top: Px, height: Px, row: Px, offset: Px, view: Px) -> Px {
    let bottom = top + height + row;
    if top < offset {
        (top - row * Count::new(2)).max(Px::ZERO)
    } else if bottom > offset + view {
        (bottom - view).min(top)
    } else {
        offset
    }
}

pub(super) fn band(frame: &Frame<'_>, offset: Px, view: Px) -> Band {
    if frame.ui.holding() {
        Band {
            offset: (offset - view).max(Px::ZERO),
            bottom: offset + view * 2,
        }
    } else {
        Band {
            offset,
            bottom: offset + view,
        }
    }
}

enum HitLine {
    File { file: FileId, hits: Count },
    Hit(Count),
}

fn hit_lines(model: &Model) -> Vec<HitLine> {
    let mut lines = Vec::with_capacity(model.hits.len());
    let mut header: Option<usize> = None;
    for (position, hit) in model.hits.iter().enumerate() {
        match header.and_then(|at| lines.get_mut(at)) {
            Some(HitLine::File { file, hits }) if *file == hit.file => {
                *hits = Count::new(hits.get() + 1);
            }
            _ => {
                header = Some(lines.len());
                lines.push(HitLine::File {
                    file: hit.file,
                    hits: Count::new(1),
                });
            }
        }
        lines.push(HitLine::Hit(Count::new(position)));
    }
    lines
}

pub(super) fn search_view(model: &Model, frame: &mut Frame<'_>) {
    frame.start(Container::ToolbarSmall);
    frame.field(
        &model.fields,
        Which::Search,
        &Label::new("regex"),
        SEARCH_FIELD,
    );
    frame.finish();
    if model.hits.is_empty() {
        let search = model.fields.get(Which::Search).text();
        frame.note(
            if search.is_empty() {
                "type a regex in the field above and press enter"
            } else {
                "no hits"
            },
            WEAK,
            Padding::Step,
        );
        return;
    }
    if model.hits_shown == HitsShown::First {
        frame.start(Container::Toolbar);
        frame.note(
            format!(
                "showing the first {HIT_LIMIT} hits; the search stops there, a narrower regex finds the rest",
            ),
            ORANGE,
            Padding::Step,
        );
        frame.finish();
    }
    let lines = hit_lines(model);
    let id = ids::search();
    let row_height = frame.row_height() + ROW_EXTRA;
    let scrolled = frame.scroll_column(id, model.scrolls.get(id), Scroller::Plain, None);
    let count = Count::new(lines.len());
    let window = frame.rows_window(
        scrolled.offset,
        scrolled.interaction.rect(),
        count,
        row_height,
        Count::new(60),
    );
    let width = model
        .hits
        .iter()
        .map(|hit| hit.line.number())
        .max()
        .map_or(1, |number| number.to_string().len());
    for line in lines
        .iter()
        .skip(window.first.get())
        .take(window.visible.get())
    {
        match line {
            HitLine::File { file, hits } => {
                let Some(source) = model.index.file(*file) else {
                    continue;
                };
                let runs = vec![
                    Run::new(source.path().as_str(), TEXT),
                    Run::new(format!("  {}", Counted::new(*hits, Noun::Hit)), WEAK),
                ];
                if frame
                    .row(
                        runs,
                        ids::HIT_FILE_ROW.nth(Count::new(file.number())),
                        Chosen::Plain,
                    )
                    .clicked()
                {
                    frame.push(Action::GoTo(*file, Line::new(0)));
                }
            }
            HitLine::Hit(position) => {
                let Some(hit) = model.hits.get(position.get()) else {
                    continue;
                };
                let text = model
                    .index
                    .file(hit.file)
                    .and_then(|source| source.text().line(hit.line))
                    .map_or("", |shown| shown.as_str().trim());
                let runs = vec![
                    Run::new(format!("  {:>width$}  ", hit.line.number()), WEAK),
                    Run::new(text, TEXT),
                ];
                if frame
                    .row(runs, ids::HIT_ROW.nth(*position), Chosen::Plain)
                    .clicked()
                {
                    frame.push(Action::GoTo(hit.file, hit.line));
                }
            }
        }
    }
    frame.rows_after(count, &window, row_height, Px::ZERO);
    frame.finish();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shown(start: u32, end: u32, last: u32) -> Vec<(Target, Span)> {
        fold_buttons(
            &RelativePath::new("a.rs"),
            Line::new(start),
            Line::new(end),
            Line::new(last),
        )
        .into_iter()
        .map(|button| (button.target, button.span))
        .collect()
    }

    fn span(start: u32, end: u32) -> Span {
        Span::new(Line::new(start), Line::new(end)).unwrap()
    }

    #[test]
    fn a_long_gap_between_changes_opens_twenty_lines_from_either_side_or_all() {
        assert_eq!(
            shown(100, 199, 500),
            vec![
                (ids::FOLD_UP.with(&Label::new("a.rs:100")), span(180, 199)),
                (ids::FOLD_DOWN.with(&Label::new("a.rs:199")), span(100, 119)),
                (ids::FOLD.with(&Label::new("a.rs:100")), span(100, 199)),
            ]
        );
    }

    #[test]
    fn the_top_and_the_end_of_a_file_open_only_toward_their_change() {
        assert_eq!(
            shown(0, 99, 500).first(),
            Some(&(ids::FOLD_UP.with(&Label::new("a.rs:0")), span(80, 99)))
        );
        assert_eq!(
            shown(400, 500, 500).first(),
            Some(&(ids::FOLD_DOWN.with(&Label::new("a.rs:500")), span(400, 419)))
        );
    }

    #[test]
    fn a_short_gap_keeps_its_arrow_where_it_was_and_opens_whole() {
        assert_eq!(
            shown(486, 500, 500),
            vec![(ids::FOLD_DOWN.with(&Label::new("a.rs:500")), span(486, 500))]
        );
    }
}

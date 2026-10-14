use std::mem;
use std::rc::Rc;

use domain::{
    CodePlace, Draft, DraftOn, FileChange, FileId, Line, LineCount, Linked, LinkedFile,
    RelativePath, Removal, Revision, Side, SourceFile, SourceLine, Span, Thread,
};
use ui::{Button, Count, Icon, Id, Label, Px, Rect, Run, Scrollbar};

use crate::action::Action;
use crate::diffs::{Loading, SidesKey};
use crate::field::Which;
use crate::grid::{self, GUTTER};
use crate::ids::{self, SlotKind, Target};
use crate::keys::{self, LineGesture};
use crate::model::{HIT_LIMIT, HitsShown, Model};
use crate::review::{Picking, ReviewAct, merged};
use crate::text::{Counted, Noun};
use crate::theme::{
    ACCENT, ADDED_FILL, COMMENT_FRAME, DRAFT_FRAME, DRAFT_ROWS, DRAFTED_BAR, EDGE_SCROLL_BAND,
    EDGE_SCROLL_MOST, LINES_SELECTED, ORANGE, PIXEL, ROW_EXTRA, SEARCH_FIELD, TEXT, THREAD_BAR,
    WEAK,
};
use crate::widgets::{
    AcrossBar, AcrossScroll, Chosen, CodeBlock, Coded, Container, Frame, Marks, Padding, Scroller,
    Width,
};

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
enum InsetAt {
    Top,
    After(Line),
}

impl InsetAt {
    fn before(line: Line) -> Self {
        line.previous().map_or(Self::Top, Self::After)
    }

    const fn anchor(self) -> Option<Line> {
        match self {
            Self::Top => None,
            Self::After(line) => Some(line),
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct GoneLine<'rows> {
    old: Line,
    text: &'rows SourceLine,
}

struct RemovedBlock<'rows> {
    at: InsetAt,
    lines: Vec<GoneLine<'rows>>,
}

fn gone_lines<'rows>(removal: &Removal<'rows>) -> Vec<GoneLine<'rows>> {
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
    Removed {
        path: &'inset RelativePath,
        block: Count,
        lines: Vec<GoneLine<'inset>>,
    },
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
            Self::Removed { .. } => Layer::Removed,
            Self::Thread(_) => Layer::Thread,
            Self::Draft(_) => Layer::Draft,
            Self::Editing(_) => Layer::Editing,
        }
    }
}

pub(super) struct Inset<'inset> {
    at: InsetAt,
    pub(super) height: Px,
    pub(super) kind: InsetKind<'inset>,
}

impl Inset<'_> {
    pub(super) fn row_id(&self) -> Id {
        match &self.kind {
            InsetKind::Removed { path, block, .. } => {
                ids::removed().with(ids::in_file(path, *block).as_str())
            }
            InsetKind::Thread(thread) => ids::thread_box().with(thread.id.as_str()).with("row"),
            InsetKind::Draft(draft) => {
                ids::draft_box().nth(usize::try_from(draft.id.get()).unwrap_or(0))
            }
            InsetKind::Editing(_) => ids::draft_box().with("new"),
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
        let height = frame
            .ui
            .remembered(inset.row_id())
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

fn insets<'inset>(
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
    for (number, block) in blocks.iter().enumerate() {
        let inset = Inset {
            at: within(block.at),
            height: row * Count::new(block.lines.len()),
            kind: InsetKind::Removed {
                path,
                block: Count::new(number),
                lines: block.lines.clone(),
            },
        };
        all.push(inset);
    }
    all.extend(noted(model, frame, path).into_iter().map(|noted| Inset {
        at: within(place_at(&blocks, noted.side, noted.line)),
        ..noted.inset
    }));
    all.sort_by_key(|inset| (inset.at, inset.kind.layer()));
    all
}

enum Piece {
    Code { start: Line, end: Line },
    Fold { start: Line, end: Line },
    Inset(Count),
}

struct Placed {
    top: Px,
    height: Px,
    piece: Piece,
}

struct Sheet {
    pieces: Vec<Placed>,
    total: Px,
    row: Px,
}

impl Sheet {
    fn build(shown: &[Span], last: Line, insets: &[Inset<'_>], row: Px) -> Self {
        let mut pieces: Vec<Placed> = Vec::new();
        let mut top = Px::ZERO;
        let mut push = |piece: Piece, height: Px, into: &mut Vec<Placed>| {
            into.push(Placed { top, height, piece });
            top += height;
        };
        let mut next_inset = 0;
        while let Some(inset) = insets
            .get(next_inset)
            .filter(|inset| inset.at == InsetAt::Top)
        {
            push(
                Piece::Inset(Count::new(next_inset)),
                inset.height,
                &mut pieces,
            );
            next_inset += 1;
        }
        let mut line = Line::new(0);
        let lines_tall = |start: Line, end: Line| {
            row * Count::new(usize::try_from(end.value() - start.value() + 1).unwrap_or(0))
        };
        for span in shown {
            if span.start() > line
                && let Some(before) = span.start().previous()
            {
                push(
                    Piece::Fold {
                        start: line,
                        end: before,
                    },
                    row,
                    &mut pieces,
                );
            }
            let mut start = span.start();
            let mut rest = true;
            while let Some(anchor) = insets
                .get(next_inset)
                .and_then(|inset| inset.at.anchor())
                .filter(|anchor| *anchor <= span.end())
            {
                if rest && anchor >= start {
                    push(
                        Piece::Code { start, end: anchor },
                        lines_tall(start, anchor),
                        &mut pieces,
                    );
                    match anchor.next() {
                        Some(after) => start = after,
                        None => rest = false,
                    }
                }
                if let Some(inset) = insets.get(next_inset) {
                    push(
                        Piece::Inset(Count::new(next_inset)),
                        inset.height,
                        &mut pieces,
                    );
                }
                next_inset += 1;
            }
            if rest && start <= span.end() {
                push(
                    Piece::Code {
                        start,
                        end: span.end(),
                    },
                    lines_tall(start, span.end()),
                    &mut pieces,
                );
            }
            line = span.end().next().unwrap_or(span.end());
        }
        if line <= last && shown.last().is_none_or(|span| span.end() < last) {
            push(
                Piece::Fold {
                    start: line,
                    end: last,
                },
                row,
                &mut pieces,
            );
        }
        while next_inset < insets.len() {
            if let Some(inset) = insets.get(next_inset) {
                push(
                    Piece::Inset(Count::new(next_inset)),
                    inset.height,
                    &mut pieces,
                );
            }
            next_inset += 1;
        }
        let total = top + bar_room(&mut pieces);
        Self { pieces, total, row }
    }

    fn last_code(&self) -> Option<Count> {
        self.pieces
            .iter()
            .rposition(|placed| matches!(placed.piece, Piece::Code { .. }))
            .map(Count::new)
    }

    fn line_top(&self, line: Line) -> Option<Px> {
        self.pieces.iter().find_map(|placed| match placed.piece {
            Piece::Code { start, end } if start <= line && line <= end => Some(
                placed.top
                    + self.row
                        * Count::new(usize::try_from(line.value() - start.value()).unwrap_or(0)),
            ),
            _ => None,
        })
    }

    fn shows(&self, line: Line) -> bool {
        self.line_top(line).is_some()
    }
}

fn bar_room(pieces: &mut [Placed]) -> Px {
    let Some(last_code) = pieces
        .iter()
        .rposition(|placed| matches!(placed.piece, Piece::Code { .. }))
    else {
        return Px::ZERO;
    };
    for (position, placed) in pieces.iter_mut().enumerate().skip(last_code) {
        if position == last_code {
            placed.height += Scrollbar::WIDTH;
        } else {
            placed.top += Scrollbar::WIDTH;
        }
    }
    Scrollbar::WIDTH
}

pub(super) struct Segment<'path> {
    id: Id,
    path: &'path RelativePath,
    start: Line,
    end: Line,
    coded: Coded,
}

fn line_under_mouse(
    frame: &Frame<'_>,
    segments: &[Segment<'_>],
    path: &RelativePath,
) -> Option<Line> {
    let mouse = frame.ui.pointer().mouse.vertical;
    let row = frame.row_height();
    segments
        .iter()
        .rev()
        .filter(|segment| segment.path == path)
        .find_map(|segment| {
            let rect = frame.ui.placement(segment.id)?.rect;
            (mouse >= rect.top).then(|| {
                let down = u32::try_from((mouse - rect.top).ratio(row).max(0)).unwrap_or(0);
                Line::new(segment.start.value().saturating_add(down)).min(segment.end)
            })
        })
}

pub(super) fn select_lines(
    model: &Model,
    frame: &mut Frame<'_>,
    segments: &[Segment<'_>],
    scrollbar: Option<Scrollbar>,
) {
    let pointer = frame.ui.pointer();
    let on_scrollbar = scrollbar.is_some_and(|shown| shown.track.contains(pointer.mouse));
    let mut acted = false;
    for segment in segments {
        let coded = &segment.coded;
        match (
            keys::selects_line(coded.interaction, pointer),
            coded.spot,
            coded.dragged_to,
        ) {
            (Some(gesture @ (LineGesture::Press | LineGesture::Extend)), Some(spot), _)
                if !on_scrollbar =>
            {
                frame.push(Action::SelectLine(segment.path.clone(), spot.line, gesture));
                acted = true;
            }
            (Some(LineGesture::Drag), _, Some(line)) => {
                let line = line_under_mouse(frame, segments, segment.path).unwrap_or(line);
                frame.push(Action::SelectLine(
                    segment.path.clone(),
                    line,
                    LineGesture::Drag,
                ));
                acted = true;
            }
            _ => {}
        }
    }
    let clicked = segments
        .iter()
        .any(|segment| segment.coded.interaction.clicked());
    let down = segments
        .iter()
        .any(|segment| segment.coded.interaction.down());
    if !acted && model.line_grab.is_some() && (clicked || !down) {
        frame.push(Action::ClearLineSelection);
    }
}

pub(super) fn inset(
    model: &Model,
    frame: &mut Frame<'_>,
    inset: &Inset<'_>,
    path: &RelativePath,
    width: Option<Px>,
    viewed: Viewed,
) {
    let gutter = frame.cell_width() * GUTTER;
    match &inset.kind {
        InsetKind::Removed { lines, .. } => {
            let row = frame.row_height();
            let first_old = lines
                .first()
                .map_or(Count::ZERO, |gone| line_count(gone.old));
            frame.start(Container::Slot {
                id: ids::slot(path, SlotKind::Removed, first_old),
                height: row * Count::new(lines.len()),
            });
            frame.start(Container::Removed(inset.row_id()));
            let RowsShown { first, past } = removed_rows(Count::new(lines.len()), viewed, row);
            gap(frame, row * first);
            let picked = model
                .review
                .picked
                .as_ref()
                .filter(|picked| &picked.file == path);
            let origin = picked.map(|picked| removed_target(path, picked.from).id());
            let held = origin.is_some() && frame.ui.active() == origin;
            let side = old_side(model, frame, path);
            let content = frame.cell_width() * viewed.columns;
            for gone in lines.iter().take(past.get()).skip(first.get()) {
                let old = gone.old;
                let marked = picked
                    .filter(|picked| picked.lines().contains(old))
                    .map(|_| LINES_SELECTED);
                let target = removed_target(path, old);
                let line = frame.grid_line(
                    Rc::new(grid::removed(old, gone.text, side)),
                    viewed.across,
                    target,
                    marked,
                );
                if line.hovered() && frame.scrolled_across(target.id()) {
                    let moved = frame.wheel_across(target.id(), viewed.across, content);
                    if moved != viewed.across {
                        frame.push(Action::ScrollAcross(ids::LINES.id(), moved));
                    }
                }
                let pointer = frame.ui.pointer();
                let picking = if line.double_clicked() {
                    frame.push(Action::Review(ReviewAct::StartDraftHere));
                    None
                } else if line.clicked() && pointer.mods.shift() {
                    Some(Picking::Extend)
                } else if line.clicked() {
                    Some(Picking::Press)
                } else if held && line.hovered() && pointer.down.contains(Button::Left) {
                    Some(Picking::Drag)
                } else {
                    None
                };
                if let Some(picking) = picking {
                    frame.push(Action::Review(ReviewAct::PickRemoved(
                        path.clone(),
                        old,
                        picking,
                    )));
                }
            }
            gap(
                frame,
                row * Count::new(lines.len().saturating_sub(past.get())),
            );
            frame.finish();
            frame.finish();
        }
        InsetKind::Thread(thread) => {
            frame.start(Container::CommentRow(inset.row_id()));
            frame.indent(gutter);
            review::thread_box(model, frame, thread);
            frame.finish();
        }
        InsetKind::Draft(draft) => {
            frame.start(Container::CommentRow(inset.row_id()));
            frame.indent(gutter);
            review::draft_box(model, frame, draft);
            frame.finish();
        }
        InsetKind::Editing(on) => {
            frame.start(Container::CommentRow(inset.row_id()));
            frame.indent(gutter);
            review::editing_box(model, frame, on, width.map(|width| width - gutter));
            frame.finish();
        }
    }
}

fn old_side<'model>(
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

fn text_columns(model: &Model, file: FileId, span: Span) -> Count {
    model.index.file(file).map_or(Count::ZERO, |source| {
        let text = source.text();
        span.lines()
            .filter_map(|line| text.line(line))
            .map(|line| Count::new(line.as_str().chars().count()))
            .max()
            .map_or(Count::ZERO, |widest| widest + GUTTER)
    })
}

fn removed_columns<'inset>(insets: impl Iterator<Item = &'inset Inset<'inset>>) -> Count {
    insets
        .filter_map(|inset| match &inset.kind {
            InsetKind::Removed { lines, .. } => lines
                .iter()
                .map(|gone| gone.text.as_str().chars().count())
                .max(),
            InsetKind::Thread(_) | InsetKind::Draft(_) | InsetKind::Editing(_) => None,
        })
        .max()
        .map_or(Count::ZERO, |widest| Count::new(widest) + GUTTER)
}

fn line_count(line: Line) -> Count {
    Count::new(usize::try_from(line.value()).unwrap_or(0))
}

fn removed_target(path: &RelativePath, old: Line) -> Target {
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

fn code_id(path: &RelativePath, start: Line) -> Id {
    ids::LINES.with(&ids::in_file(path, line_count(start))).id()
}

struct Shown<'shown> {
    model: &'shown Model,
    file: FileId,
    path: &'shown RelativePath,
    width: Option<Px>,
    last: Line,
}

struct Cut {
    first: Line,
    end: Line,
    top: Px,
}

#[derive(Clone, Copy)]
pub(super) struct Band {
    pub(super) offset: Px,
    pub(super) bottom: Px,
}

impl Band {
    pub(super) fn below(self, top: Px) -> Self {
        Self {
            offset: self.offset - top,
            bottom: self.bottom - top,
        }
    }

    pub(super) fn reaches(self, top: Px, height: Px) -> bool {
        top + height > self.offset && top < self.bottom
    }
}

#[derive(Clone, Copy)]
pub(super) struct Viewed {
    pub(super) top: Px,
    pub(super) band: Band,
    pub(super) across: Px,
    pub(super) columns: Count,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RowsShown {
    first: Count,
    past: Count,
}

fn removed_rows(count: Count, viewed: Viewed, row: Px) -> RowsShown {
    let Viewed { top, band, .. } = viewed;
    let rows = |height: Px| Count::new(usize::try_from(height.ratio(row)).unwrap_or(0));
    let first = rows(band.offset - top).min(count);
    let past = (rows(band.bottom - top) + Count::new(1))
        .min(count)
        .max(first);
    RowsShown { first, past }
}

pub(super) fn gap(frame: &mut Frame<'_>, height: Px) {
    if height > Px::ZERO {
        frame.spacer(height);
    }
}

fn cut(placed: &Placed, row: Px, range: Band) -> Option<Cut> {
    let Piece::Code { start, end } = placed.piece else {
        return None;
    };
    let Band { offset, bottom } = range;
    let skip = if placed.top < offset {
        (offset - placed.top).ratio(row).max(0)
    } else {
        0
    };
    let first = Line::new(
        start
            .value()
            .saturating_add(u32::try_from(skip).unwrap_or(0)),
    );
    let room = (bottom - placed.top.max(offset)).ratio(row).max(0) + 1;
    Some(Cut {
        first,
        end: Line::new(
            first
                .value()
                .saturating_add(u32::try_from(room).unwrap_or(0)),
        )
        .min(end),
        top: placed.top + rows(start, first, row),
    })
}

pub(super) fn rows_into(top: Px, down: Px, row: Px) -> Count {
    Count::new(usize::try_from((down - top).ratio(row).max(0)).unwrap_or(0))
}

fn rows(from: Line, to: Line, row: Px) -> Px {
    row * Count::new(usize::try_from(to.value().saturating_sub(from.value())).unwrap_or(0))
}

fn code_piece<'path>(
    frame: &mut Frame<'_>,
    shown: &Shown<'path>,
    lines: &Cut,
    columns: Count,
    bar: AcrossBar,
    marks: &Marks<'_>,
) -> Segment<'path> {
    let id = code_id(shown.path, lines.first);
    let coded = frame.code_block(
        shown.model,
        &CodeBlock {
            file: shown.file,
            start: lines.first,
            end: lines.end,
            id,
            width: Width::Wide,
            across: AcrossScroll::Shared {
                offset: ids::LINES.id(),
                columns,
                bar,
            },
            marks: Marks {
                background: marks.background,
                bar: marks.bar,
            },
        },
    );
    Segment {
        id,
        path: shown.path,
        start: lines.first,
        end: lines.end,
        coded,
    }
}

fn draw<'path>(
    frame: &mut Frame<'_>,
    shown: &Shown<'path>,
    sheet: &Sheet,
    insets: &[Inset<'_>],
    marks: &Marks<'_>,
    range: Band,
) -> Drawn<'path> {
    let row = sheet.row;
    let visible: Vec<(usize, &Placed)> = sheet
        .pieces
        .iter()
        .enumerate()
        .filter(|(_, placed)| range.reaches(placed.top, placed.height))
        .collect();
    let columns = sheet
        .pieces
        .iter()
        .filter_map(|placed| match placed.piece {
            Piece::Code { start, end } => Span::new(start, end),
            Piece::Fold { .. } | Piece::Inset(_) => None,
        })
        .fold(Count::ZERO, |widest, span| {
            widest.max(text_columns(shown.model, shown.file, span))
        })
        .max(removed_columns(insets.iter()));
    let across = shown.model.across.get(ids::LINES.id());
    let last_code = sheet.last_code();
    let first_top = visible.first().map_or(Px::ZERO, |(_, placed)| placed.top);
    gap(frame, first_top);
    let mut drawn = first_top;
    let mut segments = Vec::new();
    for (position, placed) in visible.iter().copied() {
        match placed.piece {
            Piece::Code { start, .. } => {
                frame.start(Container::Slot {
                    id: ids::slot(shown.path, SlotKind::Code, line_count(start)),
                    height: placed.height,
                });
                if let Some(lines) = cut(placed, row, range) {
                    gap(frame, lines.top - placed.top);
                    let bar = if last_code == Some(Count::new(position)) {
                        AcrossBar::Drawn
                    } else {
                        AcrossBar::Omitted
                    };
                    segments.push(code_piece(frame, shown, &lines, columns, bar, marks));
                }
                frame.finish();
            }
            Piece::Fold { start, end } => {
                frame.start(Container::Slot {
                    id: ids::slot(shown.path, SlotKind::Fold, line_count(start)),
                    height: placed.height,
                });
                fold(frame, shown.path, start, end, shown.last);
                frame.finish();
            }
            Piece::Inset(number) => {
                if let Some(shown_inset) = insets.get(number.get()) {
                    inset(
                        shown.model,
                        frame,
                        shown_inset,
                        shown.path,
                        shown.width,
                        Viewed {
                            top: placed.top,
                            band: range,
                            across,
                            columns,
                        },
                    );
                }
            }
        }
        drawn += placed.height;
    }
    Drawn {
        segments,
        height: drawn,
    }
}

pub(super) struct Drawn<'path> {
    pub(super) segments: Vec<Segment<'path>>,
    pub(super) height: Px,
}

struct Lit {
    commented: Vec<Line>,
    threaded: Vec<Line>,
}

fn lit(model: &Model, all: &[Inset<'_>], path: &RelativePath) -> Lit {
    Lit {
        commented: model
            .review
            .drafts_in(path)
            .filter(|draft| draft.side == Side::New)
            .flat_map(|draft| draft.lines().lines())
            .collect(),
        threaded: all
            .iter()
            .filter_map(|inset| match &inset.kind {
                InsetKind::Thread(thread) => thread
                    .place
                    .as_ref()
                    .filter(|place| place.side == Side::New)
                    .map(CodePlace::lines),
                _ => None,
            })
            .flat_map(Span::lines)
            .collect(),
    }
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

const CONTEXT_ABOVE: Count = Count::new(3);

pub(super) struct CodeBody<'body> {
    file: FileId,
    path: &'body RelativePath,
    last: Line,
    insets: Vec<Inset<'body>>,
    sheet: Sheet,
    added: Vec<Line>,
}

impl<'body> CodeBody<'body> {
    pub(super) fn build(
        model: &'body Model,
        frame: &Frame<'_>,
        file: FileId,
        change: Option<&'body FileChange>,
    ) -> Option<Self> {
        let source = model.index.file(file)?;
        let path = source.path();
        let last = source.text().count().last().unwrap_or(Line::new(0));
        let removals = change.map(FileChange::removals).unwrap_or_default();
        let insets = insets(model, frame, &removals, path, last);
        let mut spans = model.review.shown_spans(path, last);
        spans.extend(
            insets
                .iter()
                .filter_map(|inset| inset.at.anchor())
                .map(Span::line),
        );
        let sheet = Sheet::build(&merged(spans), last, &insets, frame.row_height());
        Some(Self {
            file,
            path,
            last,
            insets,
            sheet,
            added: change.map(FileChange::added_lines).unwrap_or_default(),
        })
    }

    pub(super) const fn total(&self) -> Px {
        self.sheet.total
    }

    pub(super) fn aim(&self, line: Line) -> Aimed {
        if !self.sheet.shows(line) {
            return Aimed::Folded;
        }
        let above = Line::new(line.value().saturating_sub(3));
        Aimed::At(
            self.sheet
                .line_top(above)
                .or_else(|| self.sheet.line_top(line))
                .unwrap_or(Px::ZERO),
        )
    }

    pub(super) fn line_at(&self, down: Px) -> Option<Line> {
        self.sheet
            .pieces
            .iter()
            .filter(|placed| placed.top + placed.height > down)
            .find_map(|placed| match placed.piece {
                Piece::Code { start, end } => Some(
                    Line::new(
                        start.value().saturating_add(
                            u32::try_from(rows_into(placed.top, down, self.sheet.row).get())
                                .unwrap_or(0),
                        ),
                    )
                    .min(end),
                ),
                Piece::Fold { .. } | Piece::Inset(_) => None,
            })
    }

    pub(super) fn editor(&self) -> Option<Stretch> {
        self.sheet
            .pieces
            .iter()
            .find(|placed| match placed.piece {
                Piece::Inset(number) => self
                    .insets
                    .get(number.get())
                    .is_some_and(|inset| matches!(inset.kind, InsetKind::Editing(_))),
                Piece::Code { .. } | Piece::Fold { .. } => false,
            })
            .map(|placed| Stretch {
                top: placed.top,
                height: placed.height,
            })
    }

    pub(super) fn draw(
        &self,
        model: &'body Model,
        frame: &mut Frame<'_>,
        width: Option<Px>,
        range: Band,
    ) -> Drawn<'body> {
        let selection = model
            .nav
            .lines()
            .filter(|_| model.nav.file() == Some(self.file));
        let shining = lit(model, &self.insets, self.path);
        let bar = |line: Line| {
            if shining.commented.contains(&line) {
                Some(DRAFTED_BAR)
            } else if shining.threaded.contains(&line) {
                Some(THREAD_BAR)
            } else {
                None
            }
        };
        let background = |line: Line| {
            selection
                .filter(|chosen| chosen.low() <= line && line <= chosen.high())
                .map(|_| LINES_SELECTED)
                .or_else(|| {
                    self.added
                        .binary_search(&line)
                        .is_ok()
                        .then_some(ADDED_FILL)
                })
        };
        let shown = Shown {
            model,
            file: self.file,
            path: self.path,
            width,
            last: self.last,
        };
        let marks = Marks {
            background: &background,
            bar: &bar,
        };
        draw(frame, &shown, &self.sheet, &self.insets, &marks, range)
    }
}

struct GonePlaced<'inset> {
    top: Px,
    inset: Inset<'inset>,
}

pub(super) struct GoneBody<'inset> {
    path: &'inset RelativePath,
    pieces: Vec<GonePlaced<'inset>>,
    total: Px,
    row: Px,
}

impl<'inset> GoneBody<'inset> {
    pub(super) fn build(
        model: &'inset Model,
        frame: &Frame<'_>,
        change: &'inset FileChange,
    ) -> Self {
        let path = change.path();
        let mut notes = noted(model, frame, path);
        notes.sort_by_key(|noted| (noted.line, noted.inset.kind.layer()));
        let mut notes = notes.into_iter().peekable();
        let mut body = Self {
            path,
            pieces: Vec::new(),
            total: Px::ZERO,
            row: frame.row_height(),
        };
        let mut lines = Vec::new();
        for gone in change.removals().iter().flat_map(gone_lines) {
            let old = gone.old;
            lines.push(gone);
            if notes.peek().is_some_and(|noted| noted.line <= old) {
                body.removed(mem::take(&mut lines));
                while let Some(noted) = notes.next_if(|noted| noted.line <= old) {
                    body.push(noted.inset);
                }
            }
        }
        body.removed(lines);
        for noted in notes {
            body.push(noted.inset);
        }
        body
    }

    fn removed(&mut self, lines: Vec<GoneLine<'inset>>) {
        if lines.is_empty() {
            return;
        }
        let block = Count::new(
            self.pieces
                .iter()
                .filter(|piece| piece.inset.kind.layer() == Layer::Removed)
                .count(),
        );
        self.push(Inset {
            at: InsetAt::Top,
            height: self.row * Count::new(lines.len()),
            kind: InsetKind::Removed {
                path: self.path,
                block,
                lines,
            },
        });
    }

    fn push(&mut self, inset: Inset<'inset>) {
        let height = inset.height;
        self.pieces.push(GonePlaced {
            top: self.total,
            inset,
        });
        self.total += height;
    }

    pub(super) const fn total(&self) -> Px {
        self.total
    }

    pub(super) fn aim(&self, line: Line) -> Aimed {
        self.pieces
            .iter()
            .find_map(|piece| match &piece.inset.kind {
                InsetKind::Removed { lines, .. } => lines
                    .iter()
                    .position(|gone| gone.old == line)
                    .map(|down| piece.top + self.row * Count::new(down)),
                InsetKind::Thread(_) | InsetKind::Draft(_) | InsetKind::Editing(_) => None,
            })
            .map_or(Aimed::Missing, |top| {
                Aimed::At((top - self.row * CONTEXT_ABOVE).max(Px::ZERO))
            })
    }

    pub(super) fn line_at(&self, down: Px) -> Option<Line> {
        self.pieces
            .iter()
            .filter(|piece| piece.top + piece.inset.height > down)
            .find_map(|piece| match &piece.inset.kind {
                InsetKind::Removed { lines, .. } => {
                    let skipped = rows_into(piece.top, down, self.row);
                    lines
                        .get(skipped.get())
                        .or_else(|| lines.last())
                        .map(|gone| gone.old)
                }
                InsetKind::Thread(_) | InsetKind::Draft(_) | InsetKind::Editing(_) => None,
            })
    }

    pub(super) fn editor(&self) -> Option<Stretch> {
        self.pieces
            .iter()
            .find(|piece| matches!(piece.inset.kind, InsetKind::Editing(_)))
            .map(|piece| Stretch {
                top: piece.top,
                height: piece.inset.height,
            })
    }

    pub(super) fn draw(
        &self,
        model: &Model,
        frame: &mut Frame<'_>,
        width: Option<Px>,
        range: Band,
    ) -> Px {
        let mut drawn = Px::ZERO;
        let columns = removed_columns(self.pieces.iter().map(|piece| &piece.inset));
        let across = model.across.get(ids::LINES.id());
        for piece in self
            .pieces
            .iter()
            .filter(|piece| range.reaches(piece.top, piece.inset.height))
        {
            gap(frame, piece.top - drawn);
            inset(
                model,
                frame,
                &piece.inset,
                self.path,
                width,
                Viewed {
                    top: piece.top,
                    band: range,
                    across,
                    columns,
                },
            );
            drawn = piece.top + piece.inset.height;
        }
        drawn
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

    fn rows_of(count: usize, top: i32, offset: i32, bottom: i32) -> (usize, usize) {
        let shown = removed_rows(
            Count::new(count),
            Viewed {
                top: Px::new(top),
                band: Band {
                    offset: Px::new(offset),
                    bottom: Px::new(bottom),
                },
                across: Px::ZERO,
                columns: Count::ZERO,
            },
            Px::new(16),
        );
        (shown.first.get(), shown.past.get())
    }

    #[test]
    fn a_removed_block_lays_out_only_the_rows_the_band_reaches() {
        assert_eq!(rows_of(400, 0, 0, 160), (0, 11));
        assert_eq!(rows_of(400, 0, 6240, 6400), (390, 400));
        assert_eq!(rows_of(400, 1600, 0, 1760), (0, 11));
        assert_eq!(rows_of(400, 0, 800, 960), (50, 61));
        assert_eq!(rows_of(400, 0, 808, 968), (50, 61));
        assert_eq!(rows_of(10, 0, 400, 560), (10, 10));
        assert_eq!(rows_of(3, 0, 0, 800), (0, 3));
    }

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

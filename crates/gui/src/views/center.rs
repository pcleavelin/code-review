use std::mem;

use domain::{
    CodePlace, DiffLayout, Draft, DraftOn, FileId, FileRow, FileText, Line, LineCount, LineKind,
    Linked, LinkedFile, RelativePath, Revision, Side, SourceLine, Span, Thread,
};
use ui::{Axis, Button, Count, Icon, Id, Label, Px, Rect, Run, Scrollbar};

use crate::action::Action;
use crate::field::Which;
use crate::grid::GUTTER;
use crate::ids::{self, Target};
use crate::keys::{self, LineGesture};
use crate::model::{HIT_LIMIT, HitsShown, Model};
use crate::nav::Aim;
use crate::review::{Picking, ReviewAct, merged};
use crate::text::{Counted, Noun};
use crate::theme::{
    ACCENT, ADDED_FILL, COMMENT_FRAME, DRAFT_FRAME, DRAFT_ROWS, DRAFTED_BAR, EDGE_SCROLL_BAND,
    EDGE_SCROLL_MOST, FIELD, LINES_SELECTED, ORANGE, PIXEL, RED, ROW_EXTRA, SEARCH_FIELD,
    SOURCE_GUESS, TEXT, THREAD_BAR, WEAK,
};
use crate::widgets::{
    AcrossBar, AcrossScroll, Chosen, CodeBlock, Coded, Container, Frame, Marks, Padding, Scroller,
    Width,
};

use super::{review, table};

pub(super) fn source_toolbar(model: &Model, frame: &mut Frame<'_>, path: &RelativePath) {
    frame.start(Container::Toolbar);
    frame.label(path.as_str(), TEXT);
    let link = model.review.open.as_ref().and_then(|open| {
        open.link(&Linked::File(LinkedFile {
            at: Revision::Commit(
                model
                    .review
                    .diffs
                    .commit()
                    .unwrap_or(&open.opened.head)
                    .clone(),
            ),
            path: path.clone(),
            lines: model
                .nav
                .lines()
                .and_then(|lines| Span::new(lines.low(), lines.high())),
        }))
    });
    review::link_buttons(frame, link, &Label::new("file"));
    review::change_controls(model, frame, path);
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

fn removed_blocks(rows: &[FileRow], last: Line) -> Vec<RemovedBlock<'_>> {
    let mut blocks = Vec::new();
    let mut lines = Vec::new();
    for shown in rows {
        match shown {
            FileRow::Removed { old, text } => lines.push(GoneLine { old: *old, text }),
            FileRow::Head { line, .. } => {
                if !lines.is_empty() {
                    blocks.push(RemovedBlock {
                        at: InsetAt::before(*line),
                        lines: mem::take(&mut lines),
                    });
                }
            }
        }
    }
    if !lines.is_empty() {
        blocks.push(RemovedBlock {
            at: InsetAt::After(last),
            lines,
        });
    }
    blocks
}

pub(super) enum InsetKind<'inset> {
    Removed {
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
            InsetKind::Removed { block, .. } => ids::removed().nth(block.get()),
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
            .placement(inset.row_id())
            .map_or(inset.height, |placement| placement.rect.height);
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
        draft
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
    rows: &'inset [FileRow],
    path: &'inset RelativePath,
    last: Line,
) -> Vec<Inset<'inset>> {
    let row = frame.row_height();
    let within = |at: InsetAt| match at {
        InsetAt::Top => InsetAt::Top,
        InsetAt::After(after) => InsetAt::After(after.min(last)),
    };
    let blocks = removed_blocks(rows, last);
    let mut all: Vec<Inset<'inset>> = Vec::new();
    for (number, block) in blocks.iter().enumerate() {
        let inset = Inset {
            at: within(block.at),
            height: row * Count::new(block.lines.len()),
            kind: InsetKind::Removed {
                block: Count::new(number),
                lines: block.lines.clone(),
            },
        };
        let height = frame
            .ui
            .placement(inset.row_id())
            .map_or(inset.height, |placement| placement.rect.height);
        all.push(Inset { height, ..inset });
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
        Self {
            pieces,
            total: top,
            row,
        }
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

struct Segment {
    id: Id,
    start: Line,
    end: Line,
    coded: Coded,
}

fn line_under_mouse(frame: &Frame<'_>, segments: &[Segment]) -> Option<Line> {
    let mouse = frame.ui.pointer().mouse.vertical;
    let row = frame.row_height();
    segments.iter().rev().find_map(|segment| {
        let rect = frame.ui.placement(segment.id)?.rect;
        (mouse >= rect.top).then(|| {
            let down = u32::try_from((mouse - rect.top).ratio(row).max(0)).unwrap_or(0);
            Line::new(segment.start.value().saturating_add(down)).min(segment.end)
        })
    })
}

fn select_lines(
    model: &Model,
    frame: &mut Frame<'_>,
    segments: &[Segment],
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
            (Some(LineGesture::Press), Some(spot), _) if !on_scrollbar => {
                frame.push(Action::SelectLine(spot.line, LineGesture::Press));
                acted = true;
            }
            (Some(LineGesture::Drag), _, Some(line)) => {
                let line = line_under_mouse(frame, segments).unwrap_or(line);
                frame.push(Action::SelectLine(line, LineGesture::Drag));
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
            frame.start(Container::Removed(inset.row_id()));
            let row = frame.row_height();
            let RowsShown { first, past } = removed_rows(Count::new(lines.len()), viewed, row);
            gap(frame, row * first);
            let picked = model
                .review
                .picked
                .as_ref()
                .filter(|picked| &picked.file == path);
            let origin = picked.map(|picked| removed_target(picked.from).id());
            let held = origin.is_some() && frame.ui.active() == origin;
            for gone in lines.iter().take(past.get()).skip(first.get()) {
                let old = gone.old;
                let runs = vec![
                    Run::new(format!("{:5}-", old.number()), RED),
                    Run::new(gone.text.as_str(), TEXT),
                ];
                let marked = picked
                    .filter(|picked| picked.lines().contains(old))
                    .map(|_| LINES_SELECTED);
                let line = frame.marked_code_line(runs, removed_target(old), marked);
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

fn removed_target(old: Line) -> Target {
    ids::DRAFT_REMOVED.with(&Label::new(format!("{}", old.number())))
}

const FOLD_STEP: LineCount = LineCount::new(20);

#[derive(Debug, PartialEq, Eq)]
struct FoldButton {
    target: Target,
    run: Run,
    span: Span,
}

fn fold_buttons(start: Line, end: Line, last: Line) -> Vec<FoldButton> {
    let count = end.value() - start.value() + 1;
    let all = Span::new(start, end).unwrap_or_else(|| Span::line(start));
    let at = |line: Line| Count::new(usize::try_from(line.value()).unwrap_or(0));
    let step = count.min(FOLD_STEP.value());
    let mut buttons = Vec::new();
    if end < last
        && let Some(span) = Span::new(Line::new(end.value() + 1 - step), end)
    {
        buttons.push(FoldButton {
            target: ids::FOLD_UP.nth(at(start)),
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
            target: ids::FOLD_DOWN.nth(at(end)),
            run: Run::new(
                format!(" {} {step} ", Icon::MoreBelow.glyph().get()),
                ACCENT,
            ),
            span,
        });
    }
    if count > FOLD_STEP.value() || buttons.is_empty() {
        buttons.push(FoldButton {
            target: ids::FOLD.nth(at(start)),
            run: Run::new(" all ", ACCENT),
            span: all,
        });
    }
    buttons
}

pub(super) fn fold(frame: &mut Frame<'_>, path: &RelativePath, start: Line, end: Line, last: Line) {
    let count = end.value() - start.value() + 1;
    frame.start(Container::Folded);
    frame.label_runs(vec![Run::new(format!("{:>5} ", "\u{2026}"), WEAK)]);
    for FoldButton { target, run, span } in fold_buttons(start, end, last) {
        if frame.inline_button(vec![run], target).clicked() {
            frame.push(Action::Review(ReviewAct::Unfold(path.clone(), span)));
        }
        frame.attach_tip(target);
    }
    frame.label_runs(vec![Run::new(
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

fn code_id(start: Line) -> Id {
    ids::LINES
        .id()
        .nth(usize::try_from(start.value()).unwrap_or(0))
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

#[derive(Clone, Copy)]
pub(super) struct Viewed {
    pub(super) top: Px,
    pub(super) band: Band,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RowsShown {
    first: Count,
    past: Count,
}

fn removed_rows(count: Count, viewed: Viewed, row: Px) -> RowsShown {
    let Viewed { top, band } = viewed;
    let rows = |height: Px| Count::new(usize::try_from(height.ratio(row)).unwrap_or(0));
    let first = rows(band.offset - top).min(count);
    let past = (rows(band.bottom - top) + Count::new(1))
        .min(count)
        .max(first);
    RowsShown { first, past }
}

fn gap(frame: &mut Frame<'_>, height: Px) {
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

fn rows(from: Line, to: Line, row: Px) -> Px {
    row * Count::new(usize::try_from(to.value().saturating_sub(from.value())).unwrap_or(0))
}

fn code_piece(
    frame: &mut Frame<'_>,
    shown: &Shown<'_>,
    lines: &Cut,
    columns: Count,
    bar: AcrossBar,
    marks: &Marks<'_>,
) -> Segment {
    let id = code_id(lines.first);
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
        start: lines.first,
        end: lines.end,
        coded,
    }
}

fn draw(
    frame: &mut Frame<'_>,
    shown: &Shown<'_>,
    sheet: &Sheet,
    insets: &[Inset<'_>],
    marks: &Marks<'_>,
    range: Band,
) -> Drawn {
    let Band { offset, bottom } = range;
    let row = sheet.row;
    let visible: Vec<&Placed> = sheet
        .pieces
        .iter()
        .filter(|placed| placed.top + placed.height > offset && placed.top < bottom)
        .collect();
    let columns = visible
        .iter()
        .filter_map(|placed| match placed.piece {
            Piece::Code { start, end } => Span::new(start, end),
            Piece::Fold { .. } | Piece::Inset(_) => None,
        })
        .fold(Count::ZERO, |widest, span| {
            widest.max(frame.code_columns(shown.model, shown.file, span))
        });
    let last_code = visible
        .iter()
        .rposition(|placed| matches!(placed.piece, Piece::Code { .. }));
    let first_top = visible.first().map_or(Px::ZERO, |placed| {
        cut(placed, row, range).map_or(placed.top, |lines| lines.top)
    });
    frame.spacer(first_top);
    let mut drawn = first_top;
    let mut segments = Vec::new();
    for (position, placed) in visible.iter().enumerate() {
        match placed.piece {
            Piece::Code { .. } => {
                let Some(lines) = cut(placed, row, range) else {
                    continue;
                };
                let bar = if last_code == Some(position) {
                    AcrossBar::Drawn
                } else {
                    AcrossBar::Omitted
                };
                let segment = code_piece(frame, shown, &lines, columns, bar, marks);
                drawn += rows(lines.first, lines.end, row) + row;
                segments.push(segment);
            }
            Piece::Fold { start, end } => {
                fold(frame, shown.path, start, end, shown.last);
                drawn += placed.height;
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
                        },
                    );
                }
                drawn += placed.height;
            }
        }
    }
    Drawn {
        segments,
        height: drawn,
    }
}

struct Drawn {
    segments: Vec<Segment>,
    height: Px,
}

struct Lit {
    added: Vec<Line>,
    commented: Vec<Line>,
    threaded: Vec<Line>,
}

fn lit(model: &Model, rows: &[FileRow], all: &[Inset<'_>], path: &RelativePath) -> Lit {
    Lit {
        added: rows
            .iter()
            .filter_map(|shown| match shown {
                FileRow::Head {
                    line,
                    kind: LineKind::Added,
                } => Some(*line),
                _ => None,
            })
            .collect(),
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
                    .map(|place| place.line),
                _ => None,
            })
            .collect(),
    }
}

fn editor_offset(sheet: &Sheet, insets: &[Inset<'_>], offset: Px, height: Px) -> Option<Px> {
    let editor = sheet.pieces.iter().find(|placed| match placed.piece {
        Piece::Inset(number) => insets
            .get(number.get())
            .is_some_and(|inset| matches!(inset.kind, InsetKind::Editing(_))),
        Piece::Code { .. } | Piece::Fold { .. } => false,
    })?;
    Some(into_view(
        editor.top,
        editor.height,
        sheet.row,
        offset,
        height,
    ))
}

fn into_view(top: Px, height: Px, row: Px, offset: Px, view: Px) -> Px {
    let bottom = top + height + row;
    if top < offset {
        (top - row * Count::new(2)).max(Px::ZERO)
    } else if bottom > offset + view {
        (bottom - view).min(top)
    } else {
        offset
    }
}

fn scroll_offset(
    model: &Model,
    frame: &mut Frame<'_>,
    sheet: &Sheet,
    insets: &[Inset<'_>],
    path: &RelativePath,
) -> Px {
    let column_id = ids::source();
    let column = frame.ui.placement(column_id);
    let mut offset = model.scrolls.get(column_id);
    let dragging = model.line_grab.is_some()
        && sheet.pieces.iter().any(|placed| match placed.piece {
            Piece::Code { start, .. } => frame.ui.interaction(code_id(start)).down(),
            _ => false,
        });
    if dragging && let Some(column) = column {
        offset += edge_scroll(frame.ui.pointer().mouse.vertical, column.rect, sheet.row);
    }
    if let Some(request) = model.nav.scroll_to() {
        if sheet.shows(request.line) {
            let height = column.map_or(SOURCE_GUESS, |placement| placement.rect.height);
            let above = Line::new(request.line.value().saturating_sub(3));
            let target = sheet
                .line_top(above)
                .or_else(|| sheet.line_top(request.line))
                .unwrap_or(Px::ZERO);
            let editor = match request.aim {
                Aim::Editor => editor_offset(sheet, insets, offset, height),
                Aim::Line => None,
            };
            offset = editor
                .unwrap_or(target)
                .min((sheet.total - height).max(Px::ZERO));
            frame.push(Action::ScrolledToLine(request.ticket));
        } else {
            frame.push(Action::Review(ReviewAct::Reveal(
                path.clone(),
                request.line,
            )));
        }
    }
    offset
}

fn tabled(model: &Model) -> Option<RelativePath> {
    let path = model.shown_path()?;
    let split = model.review.diffs.layout == DiffLayout::Split;
    (model.review.diffs.commit().is_some() || (split && model.review.change(&path).is_some()))
        .then_some(path)
}

fn band(frame: &Frame<'_>, offset: Px, view: Px) -> Band {
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

pub(super) fn source(model: &Model, frame: &mut Frame<'_>) {
    if let Some(path) = tabled(model) {
        table::table(model, frame, &path);
        return;
    }
    let Some(file) = model.nav.file() else {
        match model.nav.gone() {
            Some(path) => gone(model, frame, path),
            None => frame.note(
                "open a changed file from Changes, or click a symbol or a file",
                WEAK,
                Padding::Step,
            ),
        }
        return;
    };
    let Some(source) = model.index.file(file) else {
        return;
    };
    let path = source.path();
    let text: &FileText = source.text();
    let row_height = frame.row_height();
    let last = text.count().last().unwrap_or(Line::new(0));
    let rows = model.review.rows(path, Some(text));
    let all = insets(model, frame, &rows, path, last);
    let mut spans = model.review.shown_spans(path, last);
    spans.extend(
        all.iter()
            .filter_map(|inset| inset.at.anchor())
            .map(Span::line),
    );
    let sheet = Sheet::build(&merged(spans), last, &all, row_height);
    let offset = scroll_offset(model, frame, &sheet, &all, path);
    source_toolbar(model, frame, path);
    let scrolled = frame.scroll_column(ids::source(), offset, Scroller::Plain, Some(FIELD));
    let selection = model.nav.lines();
    let shining = lit(model, &rows, &all, path);
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
                shining
                    .added
                    .binary_search(&line)
                    .is_ok()
                    .then_some(ADDED_FILL)
            })
    };
    let view = scrolled
        .interaction
        .rect()
        .map_or(SOURCE_GUESS, |rect| rect.height);
    let shown = Shown {
        model,
        file,
        path,
        width: scrolled.interaction.rect().map(|rect| rect.width),
        last,
    };
    let marks = Marks {
        background: &background,
        bar: &bar,
    };
    let Drawn {
        segments,
        height: drawn,
    } = draw(
        frame,
        &shown,
        &sheet,
        &all,
        &marks,
        band(frame, scrolled.offset, view),
    );
    if !segments.is_empty() {
        let scrollbar = frame
            .ui
            .placement(ids::source())
            .and_then(|placement| placement.scrollbar(Axis::Vertical, offset));
        select_lines(model, frame, &segments, scrollbar);
    }
    frame.spacer((sheet.total - drawn).max(Px::ZERO) + row_height);
    frame.finish();
}

struct GonePlaced<'inset> {
    top: Px,
    inset: Inset<'inset>,
}

struct GoneSheet<'inset> {
    pieces: Vec<GonePlaced<'inset>>,
    total: Px,
    row: Px,
}

impl<'inset> GoneSheet<'inset> {
    fn build(
        model: &'inset Model,
        frame: &Frame<'_>,
        rows: &'inset [FileRow],
        path: &'inset RelativePath,
    ) -> Self {
        let mut notes = noted(model, frame, path);
        notes.sort_by_key(|noted| (noted.line, noted.inset.kind.layer()));
        let mut notes = notes.into_iter().peekable();
        let mut sheet = Self {
            pieces: Vec::new(),
            total: Px::ZERO,
            row: frame.row_height(),
        };
        let mut lines = Vec::new();
        for shown in rows {
            let FileRow::Removed { old, text } = shown else {
                continue;
            };
            lines.push(GoneLine { old: *old, text });
            if notes.peek().is_some_and(|noted| noted.line <= *old) {
                sheet.removed(mem::take(&mut lines));
                while let Some(noted) = notes.next_if(|noted| noted.line <= *old) {
                    sheet.push(noted.inset);
                }
            }
        }
        sheet.removed(lines);
        for noted in notes {
            sheet.push(noted.inset);
        }
        sheet
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
            kind: InsetKind::Removed { block, lines },
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

    fn line_top(&self, line: Line) -> Option<Px> {
        self.pieces
            .iter()
            .find_map(|piece| match &piece.inset.kind {
                InsetKind::Removed { lines, .. } => lines
                    .iter()
                    .position(|gone| gone.old == line)
                    .map(|down| piece.top + self.row * Count::new(down)),
                InsetKind::Thread(_) | InsetKind::Draft(_) | InsetKind::Editing(_) => None,
            })
    }

    fn editor(&self) -> Option<&GonePlaced<'inset>> {
        self.pieces
            .iter()
            .find(|piece| matches!(piece.inset.kind, InsetKind::Editing(_)))
    }

    fn offset(&self, model: &Model, frame: &mut Frame<'_>) -> Px {
        let id = ids::source();
        let offset = model.scrolls.get(id);
        let Some(request) = model.nav.scroll_to() else {
            return offset;
        };
        let column = frame.ui.placement(id);
        let height = column.map_or(SOURCE_GUESS, |placement| placement.rect.height);
        let reachable = column.map_or(Px::ZERO, |placement| {
            (placement.content.height - placement.rect.height).max(Px::ZERO)
        });
        let wanted = match request.aim {
            Aim::Editor => self
                .editor()
                .map(|piece| into_view(piece.top, piece.inset.height, self.row, offset, height)),
            Aim::Line => self
                .line_top(request.line)
                .map(|top| top - self.row * Count::new(3)),
        };
        let target = wanted
            .unwrap_or(offset)
            .min((self.total - height).max(Px::ZERO))
            .max(Px::ZERO);
        if target <= reachable {
            frame.push(Action::ScrolledToLine(request.ticket));
        }
        target
    }
}

fn gone(model: &Model, frame: &mut Frame<'_>, path: &RelativePath) {
    source_toolbar(model, frame, path);
    frame.note(
        "this file is deleted by the pull request",
        ORANGE,
        Padding::Step,
    );
    let rows = model.review.rows(path, None);
    let sheet = GoneSheet::build(model, frame, &rows, path);
    let offset = sheet.offset(model, frame);
    let scrolled = frame.scroll_column(ids::source(), offset, Scroller::Plain, Some(FIELD));
    let view = scrolled
        .interaction
        .rect()
        .map_or(SOURCE_GUESS, |rect| rect.height);
    let width = scrolled.interaction.rect().map(|rect| rect.width);
    let range = band(frame, scrolled.offset, view);
    let mut drawn = Px::ZERO;
    for piece in sheet
        .pieces
        .iter()
        .filter(|piece| piece.top + piece.inset.height > range.offset && piece.top < range.bottom)
    {
        gap(frame, piece.top - drawn);
        inset(
            model,
            frame,
            &piece.inset,
            path,
            width,
            Viewed {
                top: piece.top,
                band: range,
            },
        );
        drawn = piece.top + piece.inset.height;
    }
    gap(frame, sheet.total - drawn + sheet.row);
    frame.finish();
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
        fold_buttons(Line::new(start), Line::new(end), Line::new(last))
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
                (ids::FOLD_UP.nth(Count::new(100)), span(180, 199)),
                (ids::FOLD_DOWN.nth(Count::new(199)), span(100, 119)),
                (ids::FOLD.nth(Count::new(100)), span(100, 199)),
            ]
        );
    }

    #[test]
    fn the_top_and_the_end_of_a_file_open_only_toward_their_change() {
        assert_eq!(
            shown(0, 99, 500).first(),
            Some(&(ids::FOLD_UP.nth(Count::new(0)), span(80, 99)))
        );
        assert_eq!(
            shown(400, 500, 500).first(),
            Some(&(ids::FOLD_DOWN.nth(Count::new(500)), span(400, 419)))
        );
    }

    #[test]
    fn a_short_gap_keeps_its_arrow_where_it_was_and_opens_whole() {
        assert_eq!(
            shown(486, 500, 500),
            vec![(ids::FOLD_DOWN.nth(Count::new(500)), span(486, 500))]
        );
    }
}

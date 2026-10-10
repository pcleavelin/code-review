use std::rc::Rc;

use domain::{DiffLayout, FileChange, Line, Paired, Pairing, RelativePath, Side, SourceFile, Span};
use ui::{Button, Canvas, Count, Grid, Px, Rect};

use crate::action::Action;
use crate::diffs::{Loading, Sides, SidesKey};
use crate::grid::{self, Gutter};
use crate::ids;
use crate::keys::LineGesture;
use crate::model::Model;
use crate::nav::Aim;
use crate::review::{Picking, RemovedPick, ReviewAct};
use crate::theme::{
    ADDED_FILL, BORDER, FIELD, LINES_SELECTED, ORANGE, PIXEL, RED, REMOVED_FILL, SOURCE_GUESS, WEAK,
};
use crate::widgets::{Frame, Padding, Scroller};

use super::center::{self, Inset, Noted, Viewed};

const CONTEXT: Count = Count::new(3);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct RowAt(usize);

impl RowAt {
    fn of(pairs: &[Paired], side: Side, line: Line) -> Option<Self> {
        pairs
            .iter()
            .position(|pair| match side {
                Side::New => pair.new == Some(line),
                Side::Old => pair.old == Some(line),
            })
            .map(Self)
    }

    fn pair(self, pairs: &[Paired]) -> Option<&Paired> {
        pairs.get(self.0)
    }

    const fn next(self) -> Self {
        Self(self.0 + 1)
    }

    fn down(self, rows: usize) -> Self {
        Self(self.0 + rows)
    }

    fn rows_to(self, end: Self) -> Count {
        Count::new(end.0.saturating_sub(self.0) + 1)
    }

    fn near(self, pairs: &[Paired]) -> impl Iterator<Item = Self> {
        let last = pairs.len().saturating_sub(1);
        (self.0.saturating_sub(CONTEXT.get())..=(self.0 + CONTEXT.get()).min(last)).map(Self)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Showing {
    Shown,
    Folded,
}

struct Anchored<'inset> {
    at: RowAt,
    inset: Inset<'inset>,
}

enum Piece {
    Rows { from: RowAt, to: RowAt },
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
}

fn showing(
    model: &Model,
    path: &RelativePath,
    pairs: &[Paired],
    anchored: &[Anchored<'_>],
) -> Vec<Showing> {
    let start = if model.review.whole.contains(path) {
        Showing::Shown
    } else {
        Showing::Folded
    };
    let mut shown = vec![start; pairs.len()];
    let unfolded = model.review.unfolded.get(path);
    let mut show = |at: RowAt| {
        if let Some(cell) = shown.get_mut(at.0) {
            *cell = Showing::Shown;
        }
    };
    for (row, pair) in pairs.iter().enumerate() {
        let at = RowAt(row);
        if pair.pairing != Pairing::Same {
            at.near(pairs).for_each(&mut show);
        } else if pair.new.is_some_and(|new| {
            unfolded
                .into_iter()
                .flatten()
                .any(|span| span.contains(new))
        }) {
            show(at);
        }
    }
    for anchor in anchored {
        show(anchor.at);
    }
    shown
}

impl Sheet {
    fn build(pairs: &[Paired], shown: &[Showing], anchored: &[Anchored<'_>], row: Px) -> Self {
        let mut pieces = Vec::new();
        let mut top = Px::ZERO;
        let mut push = |piece: Piece, height: Px, into: &mut Vec<Placed>| {
            into.push(Placed { top, height, piece });
            top += height;
        };
        let state = |at: RowAt| shown.get(at.0).copied().unwrap_or(Showing::Shown);
        let mut at = RowAt(0);
        while at.0 < pairs.len() {
            let here = state(at);
            let mut end = at;
            while end.next().0 < pairs.len()
                && state(end.next()) == here
                && (here == Showing::Folded || !anchored.iter().any(|anchor| anchor.at == end))
            {
                end = end.next();
            }
            match here {
                Showing::Shown => {
                    push(
                        Piece::Rows { from: at, to: end },
                        row * at.rows_to(end),
                        &mut pieces,
                    );
                    for (number, anchor) in anchored
                        .iter()
                        .enumerate()
                        .filter(|(_, anchor)| anchor.at == end)
                    {
                        push(
                            Piece::Inset(Count::new(number)),
                            anchor.inset.height,
                            &mut pieces,
                        );
                    }
                }
                Showing::Folded => {
                    let start = at.pair(pairs).and_then(|pair| pair.new);
                    let last = end.pair(pairs).and_then(|pair| pair.new);
                    if let (Some(start), Some(last)) = (start, last) {
                        push(Piece::Fold { start, end: last }, row, &mut pieces);
                    }
                }
            }
            at = end.next();
        }
        Self { pieces, total: top }
    }

    fn row_top(&self, wanted: RowAt, row: Px) -> Option<Px> {
        self.pieces.iter().find_map(|placed| match placed.piece {
            Piece::Rows { from, to } if from <= wanted && wanted <= to => {
                Some(placed.top + row * from.rows_to(wanted) - row)
            }
            Piece::Rows { .. } | Piece::Fold { .. } | Piece::Inset(_) => None,
        })
    }
}

struct Table<'table> {
    pairs: &'table [Paired],
    sides: &'table Sides,
    layout: DiffLayout,
    path: &'table RelativePath,
    selected_new: Option<Span>,
    selected_old: Option<Span>,
}

struct Cell {
    grid: Rc<Grid>,
    fill: Option<ui::Color>,
}

struct RowLook {
    old: Option<Cell>,
    new: Option<Cell>,
}

fn cell(source: &SourceFile, line: Line, fill: Option<ui::Color>, selected: Option<Span>) -> Cell {
    Cell {
        grid: Rc::new(grid::build(source, Span::line(line), Gutter::Numbered)),
        fill: if selected.is_some_and(|span| span.contains(line)) {
            Some(LINES_SELECTED)
        } else {
            fill
        },
    }
}

impl Table<'_> {
    fn look(&self, pair: &Paired) -> RowLook {
        let (old_fill, new_fill) = match pair.pairing {
            Pairing::Same => (None, None),
            Pairing::Changed | Pairing::Removed | Pairing::Added => {
                (Some(REMOVED_FILL), Some(ADDED_FILL))
            }
        };
        let old = pair
            .old
            .map(|line| cell(&self.sides.old, line, old_fill, self.selected_old));
        let new = pair
            .new
            .map(|line| cell(&self.sides.new, line, new_fill, self.selected_new));
        match (self.layout, pair.pairing) {
            (DiffLayout::Unified, Pairing::Same) => RowLook { old: None, new },
            (DiffLayout::Unified | DiffLayout::Split, _) => RowLook { old, new },
        }
    }

    fn rows(&self, frame: &mut Frame<'_>, from: RowAt, to: RowAt) {
        let row = frame.row_height();
        let font = frame.metrics.font;
        let layout = self.layout;
        let looks: Vec<RowLook> = self
            .pairs
            .get(from.0..=to.0)
            .into_iter()
            .flatten()
            .map(|pair| self.look(pair))
            .collect();
        let draw = move |canvas: &mut Canvas<'_>, rect: Rect| {
            paint(canvas, rect, &looks, layout, row, font);
        };
        let interaction = frame.custom_block(
            Box::new(draw),
            row * from.rows_to(to),
            ids::TABLE.id().nth(from.0),
        );
        if interaction.hovered() {
            self.clicks(frame, interaction, from);
        }
    }

    fn clicks(&self, frame: &mut Frame<'_>, interaction: ui::Interaction, from: RowAt) {
        let Some(rect) = interaction.rect() else {
            return;
        };
        let row = frame.row_height();
        let pointer = frame.ui.pointer();
        let down =
            usize::try_from((pointer.mouse.vertical - rect.top).ratio(row).max(0)).unwrap_or(0);
        let Some(pair) = from.down(down).pair(self.pairs) else {
            return;
        };
        let left = pointer.mouse.horizontal < rect.left + rect.width / 2;
        let half = match (self.layout, pair.old, pair.new) {
            (DiffLayout::Split, Some(_), _) if left => Side::Old,
            (_, Some(_), None) => Side::Old,
            _ => Side::New,
        };
        if interaction.double_clicked() {
            frame.push(Action::Review(ReviewAct::StartDraftHere));
            return;
        }
        let dragging =
            pointer.down.contains(Button::Left) && !interaction.clicked() && interaction.down();
        match (half, pair.old, pair.new) {
            (Side::Old, Some(old), _) => {
                let picking = if interaction.clicked() && pointer.mods.shift() {
                    Some(Picking::Extend)
                } else if interaction.clicked() {
                    Some(Picking::Press)
                } else if dragging {
                    Some(Picking::Drag)
                } else {
                    None
                };
                if let Some(picking) = picking {
                    frame.push(Action::Review(ReviewAct::PickRemoved(
                        self.path.clone(),
                        old,
                        picking,
                    )));
                }
            }
            (Side::New, _, Some(new)) => {
                if interaction.clicked() {
                    frame.push(Action::SelectLine(new, LineGesture::Press));
                } else if dragging {
                    frame.push(Action::SelectLine(new, LineGesture::Drag));
                }
            }
            _ => {}
        }
    }
}

fn paint(
    canvas: &mut Canvas<'_>,
    rect: Rect,
    looks: &[RowLook],
    layout: DiffLayout,
    row: Px,
    font: ui::FontSize,
) {
    let half = match layout {
        DiffLayout::Split => rect.width / 2,
        DiffLayout::Unified => rect.width,
    };
    let right = match layout {
        DiffLayout::Split => rect.left + half,
        DiffLayout::Unified => rect.left,
    };
    let mut top = rect.top;
    for look in looks {
        for (shown, left) in [(look.old.as_ref(), rect.left), (look.new.as_ref(), right)] {
            let Some(shown) = shown else {
                continue;
            };
            let area = Rect::new(left, top, half, row);
            if let Some(fill) = shown.fill {
                canvas.rect(area, fill);
            }
            canvas.push_clip(area.intersect(canvas.clip()));
            canvas.grid(area.origin(), font, &shown.grid);
            canvas.pop_clip();
        }
        top += row;
    }
    if layout == DiffLayout::Split {
        canvas.rect(Rect::new(right, rect.top, PIXEL, rect.height), BORDER);
    }
}

fn offset_for(model: &Model, frame: &mut Frame<'_>, sheet: &Sheet, pairs: &[Paired]) -> Px {
    let id = ids::source();
    let column = frame.ui.placement(id);
    let height = column.map_or(SOURCE_GUESS, |placement| placement.rect.height);
    let row = frame.row_height();
    let mut offset = model.scrolls.get(id);
    if let Some(request) = model.nav.scroll_to() {
        let wanted =
            RowAt::of(pairs, Side::New, request.line).and_then(|at| sheet.row_top(at, row));
        if let Some(top) = wanted {
            offset = match request.aim {
                Aim::Line => top - row * CONTEXT,
                Aim::Editor => top - row * Count::new(2),
            }
            .min((sheet.total - height).max(Px::ZERO))
            .max(Px::ZERO);
        }
        frame.push(Action::ScrolledToLine(request.ticket));
    }
    offset
}

struct Band {
    from: Px,
    until: Px,
    width: Option<Px>,
}

fn sides_or_note<'model>(
    model: &'model Model,
    frame: &mut Frame<'_>,
    key: SidesKey,
) -> Option<&'model Sides> {
    let reading = "reading both sides of the file...";
    match model.review.diffs.sides.get(&key) {
        None => {
            frame.push(Action::Review(ReviewAct::NeedSides(key)));
            frame.note(reading, WEAK, Padding::Step);
            None
        }
        Some(Loading::Asking) => {
            frame.note(reading, WEAK, Padding::Step);
            None
        }
        Some(Loading::Failed(why)) => {
            frame.note(why.as_str(), RED, Padding::Step);
            None
        }
        Some(Loading::Got(sides)) => Some(sides),
    }
}

fn change_or_note<'model>(
    model: &'model Model,
    frame: &mut Frame<'_>,
    path: &RelativePath,
) -> Option<&'model FileChange> {
    let change = model.review.shown_change(path);
    if change.is_none() {
        let reading = model
            .review
            .diffs
            .commit()
            .is_some_and(|commit| model.review.diffs.commit_changes(commit).is_none());
        frame.note(
            if reading {
                "reading the commit's changes..."
            } else {
                "this file is not changed here"
            },
            WEAK,
            Padding::Step,
        );
    }
    change
}

fn anchored<'model>(
    model: &'model Model,
    frame: &Frame<'_>,
    path: &'model RelativePath,
    pairs: &[Paired],
) -> Vec<Anchored<'model>> {
    if model.review.diffs.commit().is_some() {
        return Vec::new();
    }
    let noted: Vec<Noted<'model>> = center::noted(model, frame, path);
    let mut all: Vec<Anchored<'model>> = noted
        .into_iter()
        .filter_map(|noted| {
            RowAt::of(pairs, noted.side, noted.line).map(|at| Anchored {
                at,
                inset: noted.inset,
            })
        })
        .collect();
    all.sort_by_key(|anchor| (anchor.at, anchor.inset.kind.layer()));
    all
}

pub(super) fn table(model: &Model, frame: &mut Frame<'_>, path: &RelativePath) {
    center::source_toolbar(model, frame, path);
    let commit = model.review.diffs.commit().cloned();
    let Some(change) = change_or_note(model, frame, path) else {
        return;
    };
    let key = SidesKey {
        commit: commit.clone(),
        path: path.clone(),
    };
    let Some(sides) = sides_or_note(model, frame, key) else {
        return;
    };
    let layout = model.review.diffs.layout;
    let pairs = change.paired(sides.old.text().count(), sides.new.text().count(), layout);
    let insets = anchored(model, frame, path, &pairs);
    let shown = showing(model, path, &pairs, &insets);
    let sheet = Sheet::build(&pairs, &shown, &insets, frame.row_height());
    let offset = offset_for(model, frame, &sheet, &pairs);
    if commit.is_some() {
        frame.note(
            "one commit's changes; comments go on the whole pull request's changes",
            ORANGE,
            Padding::Step,
        );
    }
    let scrolled = frame.scroll_column(ids::source(), offset, Scroller::Plain, Some(FIELD));
    let view = scrolled
        .interaction
        .rect()
        .map_or(SOURCE_GUESS, |rect| rect.height);
    let width = scrolled.interaction.rect().map(|rect| rect.width);
    let band = if frame.ui.holding() {
        Band {
            from: (scrolled.offset - view).max(Px::ZERO),
            until: scrolled.offset + view * 2,
            width,
        }
    } else {
        Band {
            from: scrolled.offset,
            until: scrolled.offset + view,
            width,
        }
    };
    let drawn = Table {
        pairs: &pairs,
        sides,
        layout,
        path,
        selected_new: model
            .nav
            .lines()
            .and_then(|lines| Span::new(lines.low(), lines.high())),
        selected_old: model
            .review
            .picked
            .as_ref()
            .filter(|picked| &picked.file == path)
            .map(RemovedPick::lines),
    };
    draw(model, frame, &drawn, &sheet, &insets, &band);
    frame.finish();
    if model.line_grab.is_some() && !frame.ui.pointer().down.contains(Button::Left) {
        frame.push(Action::ClearLineSelection);
    }
}

fn draw(
    model: &Model,
    frame: &mut Frame<'_>,
    drawn: &Table<'_>,
    sheet: &Sheet,
    insets: &[Anchored<'_>],
    band: &Band,
) {
    let row = frame.row_height();
    let last = drawn
        .sides
        .new
        .text()
        .count()
        .last()
        .unwrap_or(Line::new(0));
    let rows_above = |placed: &Placed| {
        if placed.top < band.from {
            usize::try_from((band.from - placed.top).ratio(row)).unwrap_or(0)
        } else {
            0
        }
    };
    let visible: Vec<&Placed> = sheet
        .pieces
        .iter()
        .filter(|placed| placed.top + placed.height > band.from && placed.top < band.until)
        .collect();
    let first_top = visible
        .first()
        .map_or(Px::ZERO, |placed| match placed.piece {
            Piece::Rows { .. } => placed.top + row * Count::new(rows_above(placed)),
            Piece::Fold { .. } | Piece::Inset(_) => placed.top,
        });
    frame.spacer(first_top);
    let mut drawn_to = first_top;
    for placed in visible {
        match placed.piece {
            Piece::Rows { from: start, to } => {
                let room = usize::try_from((band.until - placed.top.max(band.from)).ratio(row))
                    .unwrap_or(0)
                    + 1;
                let first = start.down(rows_above(placed)).min(to);
                let end = first.down(room).min(to);
                drawn.rows(frame, first, end);
                drawn_to += row * first.rows_to(end);
            }
            Piece::Fold { start, end } => {
                center::fold(frame, drawn.path, start, end, last);
                drawn_to += placed.height;
            }
            Piece::Inset(number) => {
                if let Some(anchor) = insets.get(number.get()) {
                    center::inset(
                        model,
                        frame,
                        &anchor.inset,
                        drawn.path,
                        band.width,
                        Viewed {
                            top: placed.top,
                            band: center::Band {
                                offset: band.from,
                                bottom: band.until,
                            },
                        },
                    );
                }
                drawn_to += placed.height;
            }
        }
    }
    frame.spacer((sheet.total - drawn_to).max(Px::ZERO) + row);
}

use std::rc::Rc;

use domain::{
    Column, DiffLayout, FileChange, FileId, Line, LineCount, Paired, Pairing, RelativePath, Side,
    SourceFile, Span,
};
use ui::{Button, Canvas, Count, Grid, Point, Px, Rect};

use crate::action::Action;
use crate::diffs::{Loading, Sides, SidesKey};
use crate::grid::{self, GUTTER, Gutter};
use crate::ids;
use crate::keys::{self, CodeGesture, LineGesture};
use crate::model::Model;
use crate::peek::Intent;
use crate::review::{Picking, RemovedPick, ReviewAct};
use crate::theme::{ADDED_FILL, BORDER, LINES_SELECTED, PIXEL, RED, REMOVED_FILL, WEAK};
use crate::widgets::{Frame, Padding};

use super::center::{self, Aimed, Band, Inset, Noted, Stretch, Viewed};

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
    indexed: Option<FileId>,
    across: Px,
    content_width: Px,
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

    fn rows(&self, model: &Model, frame: &mut Frame<'_>, from: RowAt, to: RowAt) {
        let row = frame.row_height();
        let font = frame.metrics.font;
        let layout = self.layout;
        let across = self.across;
        let looks: Vec<RowLook> = self
            .pairs
            .get(from.0..=to.0)
            .into_iter()
            .flatten()
            .map(|pair| self.look(pair))
            .collect();
        let draw = move |canvas: &mut Canvas<'_>, rect: Rect| {
            paint(canvas, rect, &looks, layout, row, font, across);
        };
        let id = ids::TABLE
            .with(&ids::in_file(self.path, Count::new(from.0)))
            .id();
        let interaction = frame.custom_block(Box::new(draw), row * from.rows_to(to), id);
        if let Some(rect) = interaction.rect() {
            let content = self.content_width + rect.width - half_width(rect, layout);
            let moved = frame.wheel_across(id, across, content);
            if moved != across {
                frame.push(Action::ScrollAcross(ids::TABLE.id(), moved));
            }
        }
        if interaction.hovered() {
            self.clicks(model, frame, interaction, from);
        }
    }

    fn clicks(
        &self,
        model: &Model,
        frame: &mut Frame<'_>,
        interaction: ui::Interaction,
        from: RowAt,
    ) {
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
        let start = match (self.layout, half) {
            (DiffLayout::Split, Side::New) => rect.left + half_width(rect, self.layout),
            _ => rect.left,
        };
        let column = column_at(
            pointer.mouse.horizontal - start + self.across,
            frame.cell_width(),
        );
        if let (Side::New, Some(file), Some(line), Some(column)) =
            (half, self.indexed, pair.new, column)
        {
            match keys::code_gesture(interaction, pointer) {
                Some(CodeGesture::Peek) => {
                    frame.push(Action::Definition(file, line, column, Intent::Peek));
                    return;
                }
                Some(CodeGesture::Jump) => {
                    frame.push(Action::Definition(file, line, column, Intent::Jump));
                    return;
                }
                None if !pointer.down.contains(Button::Left) => {
                    frame.hover(model, file, line, column);
                }
                None => {}
            }
        }
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
                    frame.push(Action::SelectLine(
                        self.path.clone(),
                        new,
                        if pointer.mods.shift() {
                            LineGesture::Extend
                        } else {
                            LineGesture::Press
                        },
                    ));
                } else if dragging {
                    frame.push(Action::SelectLine(
                        self.path.clone(),
                        new,
                        LineGesture::Drag,
                    ));
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
    across: Px,
) {
    let half = half_width(rect, layout);
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
            canvas.grid(Point::new(left - across, top), font, &shown.grid);
            canvas.pop_clip();
        }
        top += row;
    }
    if layout == DiffLayout::Split {
        canvas.rect(Rect::new(right, rect.top, PIXEL, rect.height), BORDER);
    }
}

fn half_width(rect: Rect, layout: DiffLayout) -> Px {
    match layout {
        DiffLayout::Split => rect.width / 2,
        DiffLayout::Unified => rect.width,
    }
}

fn column_at(inside: Px, cell: Px) -> Option<Column> {
    usize::try_from(inside.ratio(cell))
        .ok()
        .and_then(|cells| cells.checked_sub(GUTTER.get()))
        .and_then(|column| u32::try_from(column).ok())
        .map(Column::new)
}

fn content_width(sides: &Sides, cell: Px) -> Px {
    let columns = [&sides.old, &sides.new]
        .into_iter()
        .flat_map(|source| source.text().all().iter())
        .map(|line| line.as_str().chars().count())
        .max()
        .unwrap_or(0);
    cell * (Count::new(columns) + GUTTER)
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

fn past_hunks(change: &FileChange, side: Side) -> LineCount {
    let end = change
        .hunks()
        .iter()
        .flat_map(domain::Hunk::numbered)
        .filter_map(|line| match side {
            Side::Old => line.old,
            Side::New => line.new,
        })
        .max()
        .map_or(0, |line| line.value() + 1);
    LineCount::new(end)
}

struct Counts {
    old: LineCount,
    new: LineCount,
}

fn guessed_counts(model: &Model, change: &FileChange, head: Option<FileId>) -> Counts {
    let head = head
        .filter(|_| model.review.diffs.commit().is_none())
        .and_then(|file| model.index.file(file))
        .map(|source| source.text().count());
    match head {
        Some(new) => Counts {
            old: LineCount::new(
                (new.value() + change.removed().value()).saturating_sub(change.added().value()),
            ),
            new,
        },
        None => Counts {
            old: past_hunks(change, Side::Old),
            new: past_hunks(change, Side::New),
        },
    }
}

pub(super) struct TableBody<'body> {
    path: &'body RelativePath,
    key: SidesKey,
    head: Option<FileId>,
    sides: Option<&'body Sides>,
    pairs: Vec<Paired>,
    insets: Vec<Anchored<'body>>,
    sheet: Sheet,
    last: Line,
    row: Px,
}

impl<'body> TableBody<'body> {
    pub(super) fn build(
        model: &'body Model,
        frame: &Frame<'_>,
        change: &'body FileChange,
        head: Option<FileId>,
    ) -> Self {
        let path = change.path();
        let key = SidesKey {
            commit: model.review.diffs.commit().cloned(),
            path: path.clone(),
        };
        let sides = match model.review.diffs.sides.get(&key) {
            Some(Loading::Got(sides)) => Some(sides),
            Some(Loading::Asking | Loading::Failed(_)) | None => None,
        };
        let Counts { old, new } = sides.map_or_else(
            || guessed_counts(model, change, head),
            |sides| Counts {
                old: sides.old.text().count(),
                new: sides.new.text().count(),
            },
        );
        let pairs = change.paired(old, new, model.review.diffs.layout);
        let insets = anchored(model, frame, path, &pairs);
        let shown = showing(model, path, &pairs, &insets);
        let row = frame.row_height();
        let sheet = Sheet::build(&pairs, &shown, &insets, row);
        Self {
            path,
            key,
            head,
            sides,
            pairs,
            insets,
            sheet,
            last: new.last().unwrap_or(Line::new(0)),
            row,
        }
    }

    pub(super) const fn total(&self) -> Px {
        self.sheet.total
    }

    pub(super) fn aim(&self, line: Line) -> Aimed {
        let Some(at) = RowAt::of(&self.pairs, Side::New, line)
            .or_else(|| RowAt::of(&self.pairs, Side::Old, line))
        else {
            return Aimed::Missing;
        };
        self.sheet
            .row_top(at, self.row)
            .map_or(Aimed::Folded, |top| {
                Aimed::At((top - self.row * CONTEXT).max(Px::ZERO))
            })
    }

    pub(super) fn line_at(&self, down: Px) -> Option<Line> {
        self.sheet
            .pieces
            .iter()
            .filter(|placed| placed.top + placed.height > down)
            .find_map(|placed| match placed.piece {
                Piece::Rows { from, to } => {
                    let skipped = center::rows_into(placed.top, down, self.row);
                    from.down(skipped.get())
                        .min(to)
                        .pair(&self.pairs)
                        .and_then(|pair| pair.new.or(pair.old))
                }
                Piece::Fold { .. } | Piece::Inset(_) => None,
            })
    }

    pub(super) fn editor(&self) -> Option<Stretch> {
        self.sheet
            .pieces
            .iter()
            .find(|placed| match placed.piece {
                Piece::Inset(number) => self.insets.get(number.get()).is_some_and(|anchor| {
                    matches!(anchor.inset.kind, center::InsetKind::Editing(_))
                }),
                Piece::Rows { .. } | Piece::Fold { .. } => false,
            })
            .map(|placed| Stretch {
                top: placed.top,
                height: placed.height,
            })
    }

    fn waiting(&self, model: &Model, frame: &mut Frame<'_>) {
        match model.review.diffs.sides.get(&self.key) {
            Some(Loading::Failed(why)) => frame.note(why.as_str(), RED, Padding::Step),
            Some(Loading::Got(_)) => {}
            Some(Loading::Asking) => {
                frame.note("reading both sides of the file...", WEAK, Padding::Step);
            }
            None => {
                frame.push(Action::Review(ReviewAct::NeedSides(self.key.clone())));
                frame.note("reading both sides of the file...", WEAK, Padding::Step);
            }
        }
    }

    pub(super) fn draw(
        &self,
        model: &Model,
        frame: &mut Frame<'_>,
        width: Option<Px>,
        band: Band,
    ) -> Px {
        let Some(sides) = self.sides else {
            self.waiting(model, frame);
            return Px::ZERO;
        };
        let layout = model.review.diffs.layout;
        let content_width = content_width(sides, frame.cell_width());
        let half = width.map_or(Px::ZERO, |width| match layout {
            DiffLayout::Split => width / 2,
            DiffLayout::Unified => width,
        });
        let indexed = self.head.filter(|file| {
            model
                .index
                .file(*file)
                .is_some_and(|source| source.text().all() == sides.new.text().all())
        });
        let shown_here = model.shown_path().as_ref() == Some(self.path);
        let table = Table {
            pairs: &self.pairs,
            sides,
            layout,
            path: self.path,
            indexed,
            across: model
                .across
                .get(ids::TABLE.id())
                .min((content_width - half).max(Px::ZERO)),
            content_width,
            selected_new: model
                .nav
                .lines()
                .filter(|_| shown_here)
                .and_then(|lines| Span::new(lines.low(), lines.high())),
            selected_old: model
                .review
                .picked
                .as_ref()
                .filter(|picked| &picked.file == self.path)
                .map(RemovedPick::lines),
        };
        self.pieces(model, frame, &table, width, band)
    }

    fn pieces(
        &self,
        model: &Model,
        frame: &mut Frame<'_>,
        table: &Table<'_>,
        width: Option<Px>,
        band: Band,
    ) -> Px {
        let row = self.row;
        let rows_above = |placed: &Placed| {
            if placed.top < band.offset {
                usize::try_from((band.offset - placed.top).ratio(row)).unwrap_or(0)
            } else {
                0
            }
        };
        let visible: Vec<&Placed> = self
            .sheet
            .pieces
            .iter()
            .filter(|placed| band.reaches(placed.top, placed.height))
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
                    let room =
                        usize::try_from((band.bottom - placed.top.max(band.offset)).ratio(row))
                            .unwrap_or(0)
                            + 1;
                    let first = start.down(rows_above(placed)).min(to);
                    let end = first.down(room).min(to);
                    table.rows(model, frame, first, end);
                    drawn_to += row * first.rows_to(end);
                }
                Piece::Fold { start, end } => {
                    center::fold(frame, self.path, start, end, self.last);
                    drawn_to += placed.height;
                }
                Piece::Inset(number) => {
                    if let Some(anchor) = self.insets.get(number.get()) {
                        center::inset(
                            model,
                            frame,
                            &anchor.inset,
                            self.path,
                            width,
                            Viewed {
                                top: placed.top,
                                band,
                                across: table.across,
                                columns: Count::ZERO,
                            },
                        );
                    }
                    drawn_to += placed.height;
                }
            }
        }
        drawn_to
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_column_is_counted_after_the_line_number_gutter() {
        let cell = Px::new(8);
        assert_eq!(column_at(Px::new(116), cell), Some(Column::new(8)));
        assert_eq!(column_at(Px::new(48), cell), Some(Column::new(0)));
        assert_eq!(column_at(Px::new(40), cell), None);
        assert_eq!(column_at(Px::new(-64), cell), None);
    }
}

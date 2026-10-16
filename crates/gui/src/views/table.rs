use std::rc::Rc;

use domain::{
    Column, DiffLayout, FileChange, FileId, Line, LineCount, Paired, Pairing, RelativePath, Side,
    SourceFile, Span,
};
use ui::{Canvas, Count, Grid, Id, Point, Px, Rect};

use crate::action::Action;
use crate::diffs::{Loading, Sides, SidesKey};
use crate::grid::{self, GUTTER, Gutter};
use crate::ids;
use crate::keys::{self, CodeGesture};
use crate::model::Model;
use crate::peek::Intent;
use crate::review::{RemovedPick, ReviewAct};
use crate::theme::{ADDED_FILL, BORDER, LINES_SELECTED, PIXEL, RED, REMOVED_FILL, WEAK};
use crate::widgets::{Frame, Padding};

use super::center::{self, Inset, Noted};
use super::sheet::{Body, Laid, Measured, Piece};

const CONTEXT: Count = Count::new(3);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct RowAt(usize);

impl RowAt {
    pub(super) fn of(pairs: &[Paired], side: Side, line: Line) -> Option<Self> {
        pairs
            .iter()
            .position(|pair| match side {
                Side::New => pair.new == Some(line),
                Side::Old => pair.old == Some(line),
            })
            .map(Self)
    }

    pub(super) fn pair(self, pairs: &[Paired]) -> Option<&Paired> {
        pairs.get(self.0)
    }

    const fn next(self) -> Self {
        Self(self.0 + 1)
    }

    pub(super) fn down(self, rows: Count) -> Self {
        Self(self.0 + rows.get())
    }

    pub(super) fn rows_to(self, end: Self) -> Count {
        Count::new(end.0.saturating_sub(self.0) + 1)
    }

    pub(super) fn through(self, end: Self) -> impl Iterator<Item = Self> {
        (self.0..=end.0).map(Self)
    }

    fn near(self, pairs: &[Paired]) -> impl Iterator<Item = Self> {
        let last = pairs.len().saturating_sub(1);
        (self.0.saturating_sub(CONTEXT.get())..=(self.0 + CONTEXT.get()).min(last)).map(Self)
    }
}

pub(super) fn row_id(path: &RelativePath, at: RowAt) -> Id {
    ids::TABLE.with(&ids::in_file(path, Count::new(at.0))).id()
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

fn pieces(
    pairs: &[Paired],
    shown: &[Showing],
    anchored: &[Anchored<'_>],
    row: Px,
) -> Vec<Measured> {
    let mut pieces = Vec::new();
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
                pieces.push(Measured::new(
                    Piece::Pairs { from: at, to: end },
                    row * at.rows_to(end),
                ));
                for (number, anchor) in anchored
                    .iter()
                    .enumerate()
                    .filter(|(_, anchor)| anchor.at == end)
                {
                    pieces.push(Measured::new(
                        Piece::Inset(Count::new(number)),
                        anchor.inset.height,
                    ));
                }
            }
            Showing::Folded => {
                let start = at.pair(pairs).and_then(|pair| pair.new);
                let last = end.pair(pairs).and_then(|pair| pair.new);
                if let (Some(start), Some(last)) = (start, last) {
                    pieces.push(Measured::new(Piece::Fold { start, end: last }, row));
                }
            }
        }
        at = end.next();
    }
    pieces
}

pub(super) struct Table<'table> {
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

    pub(super) fn row(&self, model: &Model, frame: &mut Frame<'_>, at: RowAt) {
        let Some(pair) = at.pair(self.pairs) else {
            return;
        };
        let row = frame.row_height();
        let font = frame.metrics.font;
        let layout = self.layout;
        let across = self.across;
        let look = self.look(pair);
        let draw = move |canvas: &mut Canvas<'_>, rect: Rect| {
            paint(canvas, rect, &look, layout, row, font, across);
        };
        let id = row_id(self.path, at);
        let interaction = frame.line_row(Box::new(draw), id);
        if let Some(rect) = interaction.rect() {
            let content = self.content_width + rect.width - half_width(rect, layout);
            let moved = frame.wheel_across(id, across, content);
            if moved != across {
                frame.push(Action::ScrollAcross(ids::TABLE.id(), moved));
            }
        }
        if interaction.hovered() {
            self.points(model, frame, interaction, pair);
        }
    }

    fn points(
        &self,
        model: &Model,
        frame: &mut Frame<'_>,
        interaction: ui::Interaction,
        pair: &Paired,
    ) {
        let Some(rect) = interaction.rect() else {
            return;
        };
        let pointer = frame.ui.pointer();
        let half = half_at(rect, self.layout, pair, pointer.mouse.horizontal);
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
                None if !pointer.down.contains(ui::Button::Left) => {
                    frame.hover(model, file, line, column);
                }
                None => {}
            }
        }
        if interaction.double_clicked() {
            frame.push(Action::Review(ReviewAct::StartDraftHere));
        }
    }
}

pub(super) fn half_at(rect: Rect, layout: DiffLayout, pair: &Paired, mouse: Px) -> Side {
    let left = mouse < rect.left + rect.width / 2;
    match (layout, pair.old, pair.new) {
        (DiffLayout::Split, Some(_), _) if left => Side::Old,
        (_, Some(_), None) => Side::Old,
        _ => Side::New,
    }
}

fn paint(
    canvas: &mut Canvas<'_>,
    rect: Rect,
    look: &RowLook,
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
    for (shown, left) in [(look.old.as_ref(), rect.left), (look.new.as_ref(), right)] {
        let Some(shown) = shown else {
            continue;
        };
        let area = Rect::new(left, rect.top, half, row);
        if let Some(fill) = shown.fill {
            canvas.rect(area, fill);
        }
        canvas.push_clip(area.intersect(canvas.clip()));
        canvas.grid(Point::new(left - across, rect.top), font, &shown.grid);
        canvas.pop_clip();
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
    key: SidesKey,
    head: Option<FileId>,
    sides: Option<&'body Sides>,
    pub(super) pairs: Vec<Paired>,
    pub(super) last: Line,
}

pub(super) fn laid<'body>(
    model: &'body Model,
    frame: &Frame<'_>,
    change: &'body FileChange,
    head: Option<FileId>,
) -> Laid<'body> {
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
    let anchored = anchored(model, frame, path, &pairs);
    let shown = showing(model, path, &pairs, &anchored);
    let row = frame.row_height();
    let mut laid_out = pieces(&pairs, &shown, &anchored, row);
    if sides.is_none() {
        let tall = laid_out
            .iter()
            .fold(Px::ZERO, |tall, measured| tall + measured.height);
        laid_out = vec![Measured::new(Piece::Waiting, tall.max(row * Count::new(2)))];
    }
    Laid {
        body: Body::Table(TableBody {
            key,
            head,
            sides,
            pairs,
            last: new.last().unwrap_or(Line::new(0)),
        }),
        insets: anchored.into_iter().map(|anchor| anchor.inset).collect(),
        pieces: laid_out,
    }
}

impl TableBody<'_> {
    pub(super) fn waiting(&self, model: &Model, frame: &mut Frame<'_>) {
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

    pub(super) fn table<'table>(
        &'table self,
        model: &'table Model,
        frame: &Frame<'_>,
        path: &'table RelativePath,
        width: Option<Px>,
    ) -> Option<Table<'table>> {
        let sides = self.sides?;
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
        let shown_here = model.shown_path().as_ref() == Some(path);
        Some(Table {
            pairs: &self.pairs,
            sides,
            layout,
            path,
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
                .filter(|picked| &picked.file == path)
                .map(RemovedPick::lines),
        })
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

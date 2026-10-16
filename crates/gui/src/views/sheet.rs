use std::collections::{BTreeMap, BTreeSet};
use std::mem;

use domain::{DiffLayout, FileChange, FileId, Line, RelativePath, Side, Span};
use ui::{Count, Id, Px, Scrollbar};

use crate::ids;
use crate::model::Model;
use crate::nav::Kept;
use crate::review::merged;
use crate::theme::FILE_HEADER_EXTRA;
use crate::widgets::Frame;

use super::center::{self, Aimed, GoneLine, Inset, InsetAt, InsetKind, Stretch, gone_lines};
use super::table::{self, RowAt, TableBody};

const CONTEXT_ABOVE: Count = Count::new(3);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Piece {
    Header,
    Lines { start: Line, end: Line },
    Removed(Count),
    Pairs { from: RowAt, to: RowAt },
    Fold { start: Line, end: Line },
    Inset(Count),
    Scrollbar,
    Waiting,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Measured {
    pub(super) piece: Piece,
    pub(super) height: Px,
}

impl Measured {
    pub(super) const fn new(piece: Piece, height: Px) -> Self {
        Self { piece, height }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Placed {
    pub(super) top: Px,
    pub(super) height: Px,
    pub(super) section: Count,
    pub(super) piece: Piece,
}

impl Placed {
    pub(super) fn bottom(&self) -> Px {
        self.top + self.height
    }
}

pub(super) enum Body<'body> {
    Code {
        file: FileId,
        last: Line,
        added: Vec<Line>,
    },
    Gone,
    Table(TableBody<'body>),
    Collapsed,
}

pub(super) struct Laid<'body> {
    pub(super) body: Body<'body>,
    pub(super) insets: Vec<Inset<'body>>,
    pub(super) pieces: Vec<Measured>,
}

impl<'body> Laid<'body> {
    fn add(&mut self, inset: Inset<'body>) {
        let number = Count::new(self.insets.len());
        let piece = match inset.kind {
            InsetKind::Removed(_) => Piece::Removed(number),
            InsetKind::Thread(_) | InsetKind::Draft(_) | InsetKind::Editing(_) => {
                Piece::Inset(number)
            }
        };
        self.pieces.push(Measured::new(piece, inset.height));
        self.insets.push(inset);
    }
}

pub(super) struct Section<'body> {
    pub(super) path: &'body RelativePath,
    pub(super) change: Option<&'body FileChange>,
    pub(super) body: Body<'body>,
    pub(super) insets: Vec<Inset<'body>>,
    first: Count,
    past: Count,
}

impl Section<'_> {
    pub(super) fn gone(&self, number: Count) -> &[GoneLine<'_>] {
        match self.insets.get(number.get()).map(|inset| &inset.kind) {
            Some(InsetKind::Removed(lines)) => lines,
            _ => &[],
        }
    }

    pub(super) fn pairs(&self) -> &[domain::Paired] {
        match &self.body {
            Body::Table(table) => &table.pairs,
            Body::Code { .. } | Body::Gone | Body::Collapsed => &[],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Spot {
    Head(Line),
    Old(Line),
    Pair(RowAt),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Pointed {
    pub(super) section: Count,
    pub(super) spot: Spot,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct HeaderPin {
    pub(super) section: Count,
    pub(super) height: Px,
    pub(super) push: Px,
}

pub(super) struct Sheet<'body> {
    pub(super) sections: Vec<Section<'body>>,
    pub(super) pieces: Vec<Placed>,
    pub(super) total: Px,
    pub(super) row: Px,
}

fn below(line: Line, rows: Count) -> Line {
    Line::new(
        line.value()
            .saturating_add(u32::try_from(rows.get()).unwrap_or(0)),
    )
}

fn lines_tall(start: Line, end: Line, row: Px) -> Px {
    row * Count::new(usize::try_from(end.value() - start.value() + 1).unwrap_or(0))
}

fn code_pieces(shown: &[Span], last: Line, insets: &[Inset<'_>], row: Px) -> Vec<Measured> {
    let mut pieces = Vec::new();
    let inset_piece = |number: usize| {
        insets.get(number).map(|inset| {
            let at = Count::new(number);
            let piece = match inset.kind {
                InsetKind::Removed(_) => Piece::Removed(at),
                InsetKind::Thread(_) | InsetKind::Draft(_) | InsetKind::Editing(_) => {
                    Piece::Inset(at)
                }
            };
            Measured::new(piece, inset.height)
        })
    };
    let mut next = 0;
    while insets
        .get(next)
        .is_some_and(|inset| inset.at == InsetAt::Top)
    {
        pieces.extend(inset_piece(next));
        next += 1;
    }
    let mut line = Line::new(0);
    for span in shown {
        if span.start() > line
            && let Some(before) = span.start().previous()
        {
            pieces.push(Measured::new(
                Piece::Fold {
                    start: line,
                    end: before,
                },
                row,
            ));
        }
        let mut start = span.start();
        let mut rest = true;
        while let Some(anchor) = insets
            .get(next)
            .and_then(|inset| inset.at.anchor())
            .filter(|anchor| *anchor <= span.end())
        {
            if rest && anchor >= start {
                pieces.push(Measured::new(
                    Piece::Lines { start, end: anchor },
                    lines_tall(start, anchor, row),
                ));
                match anchor.next() {
                    Some(after) => start = after,
                    None => rest = false,
                }
            }
            pieces.extend(inset_piece(next));
            next += 1;
        }
        if rest && start <= span.end() {
            pieces.push(Measured::new(
                Piece::Lines {
                    start,
                    end: span.end(),
                },
                lines_tall(start, span.end(), row),
            ));
        }
        line = span.end().next().unwrap_or(span.end());
    }
    if line <= last && shown.last().is_none_or(|span| span.end() < last) {
        pieces.push(Measured::new(
            Piece::Fold {
                start: line,
                end: last,
            },
            row,
        ));
    }
    while next < insets.len() {
        pieces.extend(inset_piece(next));
        next += 1;
    }
    if let Some(position) = pieces
        .iter()
        .rposition(|measured| matches!(measured.piece, Piece::Lines { .. }))
    {
        pieces.insert(
            position + 1,
            Measured::new(Piece::Scrollbar, Scrollbar::WIDTH),
        );
    }
    pieces
}

fn code<'body>(
    model: &'body Model,
    frame: &Frame<'_>,
    file: FileId,
    change: Option<&'body FileChange>,
) -> Option<Laid<'body>> {
    let source = model.index.file(file)?;
    let path = source.path();
    let last = source.text().count().last().unwrap_or(Line::new(0));
    let removals = change.map(FileChange::removals).unwrap_or_default();
    let insets = center::insets(model, frame, &removals, path, last);
    let mut spans = model.review.shown_spans(path, last);
    spans.extend(
        insets
            .iter()
            .filter_map(|inset| inset.at.anchor())
            .map(Span::line),
    );
    let pieces = code_pieces(&merged(spans), last, &insets, frame.row_height());
    Some(Laid {
        body: Body::Code {
            file,
            last,
            added: change.map(FileChange::added_lines).unwrap_or_default(),
        },
        insets,
        pieces,
    })
}

fn gone<'body>(model: &'body Model, frame: &Frame<'_>, change: &'body FileChange) -> Laid<'body> {
    let row = frame.row_height();
    let mut notes = center::noted(model, frame, change.path());
    notes.sort_by_key(|noted| (noted.line, noted.inset.kind.layer()));
    let mut notes = notes.into_iter().peekable();
    let mut laid = Laid {
        body: Body::Gone,
        insets: Vec::new(),
        pieces: Vec::new(),
    };
    let removed = |into: &mut Laid<'body>, lines: Vec<GoneLine<'body>>| {
        if !lines.is_empty() {
            into.add(Inset {
                at: InsetAt::Top,
                height: row * Count::new(lines.len()),
                kind: InsetKind::Removed(lines),
            });
        }
    };
    let mut lines = Vec::new();
    for gone in change.removals().iter().flat_map(gone_lines) {
        let old = gone.old;
        lines.push(gone);
        if notes.peek().is_some_and(|noted| noted.line <= old) {
            removed(&mut laid, mem::take(&mut lines));
            while let Some(noted) = notes.next_if(|noted| noted.line <= old) {
                laid.add(noted.inset);
            }
        }
    }
    removed(&mut laid, lines);
    for noted in notes {
        laid.add(noted.inset);
    }
    laid
}

impl<'body> Sheet<'body> {
    fn empty(frame: &Frame<'_>) -> Self {
        Self {
            sections: Vec::new(),
            pieces: Vec::new(),
            total: Px::ZERO,
            row: frame.row_height(),
        }
    }

    pub(super) fn of(model: &'body Model, frame: &Frame<'_>, changes: &'body [FileChange]) -> Self {
        let header = frame.row_height() + FILE_HEADER_EXTRA;
        let tabled =
            model.review.diffs.commit().is_some() || model.review.diffs.layout == DiffLayout::Split;
        let paths: BTreeSet<&RelativePath> = changes.iter().map(FileChange::path).collect();
        let files: BTreeMap<&RelativePath, FileId> = model
            .index
            .file_entries()
            .filter(|entry| paths.contains(entry.item.path()))
            .map(|entry| (entry.item.path(), entry.id))
            .collect();
        let mut sheet = Self::empty(frame);
        for change in changes {
            let path = change.path();
            let indexed = files.get(path).copied();
            let laid = if model.review.collapsed(path) {
                Laid {
                    body: Body::Collapsed,
                    insets: Vec::new(),
                    pieces: Vec::new(),
                }
            } else if tabled {
                table::laid(model, frame, change, indexed)
            } else {
                indexed
                    .and_then(|file| code(model, frame, file, Some(change)))
                    .unwrap_or_else(|| gone(model, frame, change))
            };
            sheet.push(path, Some(change), Some(header), laid);
        }
        sheet
    }

    pub(super) fn alone(model: &'body Model, frame: &Frame<'_>, file: FileId) -> Option<Self> {
        let path = model.index.file(file)?.path();
        let laid = code(model, frame, file, model.review.change(path))?;
        let mut sheet = Self::empty(frame);
        sheet.push(path, None, None, laid);
        Some(sheet)
    }

    fn push(
        &mut self,
        path: &'body RelativePath,
        change: Option<&'body FileChange>,
        header: Option<Px>,
        laid: Laid<'body>,
    ) {
        let section = Count::new(self.sections.len());
        let first = Count::new(self.pieces.len());
        for Measured { piece, height } in header
            .map(|height| Measured::new(Piece::Header, height))
            .into_iter()
            .chain(laid.pieces)
        {
            self.pieces.push(Placed {
                top: self.total,
                height,
                section,
                piece,
            });
            self.total += height;
        }
        self.sections.push(Section {
            path,
            change,
            body: laid.body,
            insets: laid.insets,
            first,
            past: Count::new(self.pieces.len()),
        });
    }

    pub(super) fn section(&self, section: Count) -> Option<&Section<'body>> {
        self.sections.get(section.get())
    }

    pub(super) fn find(&self, path: &RelativePath) -> Option<Count> {
        self.sections
            .iter()
            .position(|section| section.path == path)
            .map(Count::new)
    }

    pub(super) fn pieces_of(&self, section: Count) -> &[Placed] {
        self.section(section)
            .and_then(|found| self.pieces.get(found.first.get()..found.past.get()))
            .unwrap_or(&[])
    }

    pub(super) fn top_of(&self, section: Count) -> Px {
        self.pieces_of(section)
            .first()
            .map_or(self.total, |placed| placed.top)
    }

    fn bottom_of(&self, section: Count) -> Px {
        self.pieces_of(section)
            .last()
            .map_or_else(|| self.top_of(section), Placed::bottom)
    }

    pub(super) fn reaching(&self, top: Px, bottom: Px) -> impl Iterator<Item = &Placed> {
        let first = self.pieces.partition_point(|placed| placed.bottom() <= top);
        self.pieces
            .iter()
            .skip(first)
            .take_while(move |placed| placed.top < bottom)
    }

    fn at(&self, down: Px) -> Option<&Placed> {
        let index = self
            .pieces
            .partition_point(|placed| placed.bottom() <= down);
        self.pieces.get(index).filter(|placed| placed.top <= down)
    }

    pub(super) fn spot(&self, down: Px) -> Option<Pointed> {
        let placed = self.at(down)?;
        let rows = center::rows_into(placed.top, down, self.row);
        let spot = match placed.piece {
            Piece::Lines { start, end } => Spot::Head(below(start, rows).min(end)),
            Piece::Removed(number) => {
                let lines = self.section(placed.section)?.gone(number);
                Spot::Old(lines.get(rows.get()).or_else(|| lines.last())?.old)
            }
            Piece::Pairs { from, to } => Spot::Pair(from.down(rows).min(to)),
            Piece::Header
            | Piece::Fold { .. }
            | Piece::Inset(_)
            | Piece::Scrollbar
            | Piece::Waiting => {
                return None;
            }
        };
        Some(Pointed {
            section: placed.section,
            spot,
        })
    }

    pub(super) fn row_id(&self, section: Count, spot: Spot) -> Option<Id> {
        let path = self.section(section)?.path;
        Some(match spot {
            Spot::Head(line) => head_id(path, line),
            Spot::Old(old) => center::removed_target(path, old).id(),
            Spot::Pair(at) => table::row_id(path, at),
        })
    }

    pub(super) fn old_row_id(&self, section: Count, old: Line) -> Option<Id> {
        let found = self.section(section)?;
        match &found.body {
            Body::Table(body) => {
                RowAt::of(&body.pairs, Side::Old, old).map(|at| table::row_id(found.path, at))
            }
            Body::Code { .. } | Body::Gone => Some(center::removed_target(found.path, old).id()),
            Body::Collapsed => None,
        }
    }

    pub(super) fn old_line(&self, section: Count, spot: Spot) -> Option<Line> {
        match spot {
            Spot::Old(old) => Some(old),
            Spot::Pair(at) => at.pair(self.section(section)?.pairs())?.old,
            Spot::Head(_) => None,
        }
    }

    pub(super) fn head_near(&self, section: Count, down: Px) -> Option<Line> {
        let pairs = self.section(section)?.pairs();
        let mut first = None;
        let mut near = None;
        for placed in self.pieces_of(section) {
            let mut take = |top: Px, line: Line| {
                first.get_or_insert(line);
                if top <= down {
                    near = Some(line);
                }
            };
            match placed.piece {
                Piece::Lines { start, end } => {
                    for (rows, line) in Span::new(start, end)
                        .into_iter()
                        .flat_map(Span::lines)
                        .enumerate()
                    {
                        take(placed.top + self.row * Count::new(rows), line);
                    }
                }
                Piece::Pairs { from, to } => {
                    for (rows, at) in from.through(to).enumerate() {
                        if let Some(line) = at.pair(pairs).and_then(|pair| pair.new) {
                            take(placed.top + self.row * Count::new(rows), line);
                        }
                    }
                }
                Piece::Header
                | Piece::Removed(_)
                | Piece::Fold { .. }
                | Piece::Inset(_)
                | Piece::Scrollbar
                | Piece::Waiting => {}
            }
            if placed.bottom() > down {
                break;
            }
        }
        near.or(first)
    }

    pub(super) fn header_of(&self, section: Count) -> Px {
        self.pieces_of(section)
            .first()
            .filter(|placed| placed.piece == Piece::Header)
            .map_or(Px::ZERO, |placed| placed.height)
    }

    pub(super) fn pinned(&self, offset: Px) -> Option<HeaderPin> {
        let section = self.holding(offset)?;
        let header = self
            .pieces_of(section)
            .first()
            .filter(|placed| placed.piece == Piece::Header && placed.top <= offset)?;
        let push = (header.height - (self.bottom_of(section) - offset)).max(Px::ZERO);
        (push < header.height).then_some(HeaderPin {
            section,
            height: header.height,
            push,
        })
    }

    pub(super) fn holding(&self, offset: Px) -> Option<Count> {
        self.at(offset).map(|placed| placed.section).or_else(|| {
            self.pieces
                .last()
                .filter(|placed| placed.top <= offset)
                .map(|placed| placed.section)
        })
    }

    pub(super) fn kept(&self, offset: Px) -> Option<Kept> {
        let section = self.holding(offset)?;
        let found = self.section(section)?;
        let line = self
            .pieces_of(section)
            .iter()
            .filter(|placed| placed.bottom() > offset)
            .find_map(|placed| {
                let rows = center::rows_into(placed.top, offset.max(placed.top), self.row);
                match (&found.body, placed.piece) {
                    (Body::Code { .. }, Piece::Lines { start, end }) => {
                        Some(below(start, rows).min(end))
                    }
                    (Body::Gone, Piece::Removed(number)) => {
                        let lines = found.gone(number);
                        lines
                            .get(rows.get())
                            .or_else(|| lines.last())
                            .map(|gone| gone.old)
                    }
                    (Body::Table(body), Piece::Pairs { from, to }) => from
                        .down(rows)
                        .min(to)
                        .pair(&body.pairs)
                        .and_then(|pair| pair.new.or(pair.old)),
                    _ => None,
                }
            })?;
        Some(Kept {
            path: found.path.clone(),
            line: below(line, CONTEXT_ABOVE),
        })
    }

    fn above(&self, top: Px) -> Px {
        (top - self.row * CONTEXT_ABOVE).max(Px::ZERO)
    }

    pub(super) fn aim(&self, section: Count, line: Line) -> Aimed {
        let Some(found) = self.section(section) else {
            return Aimed::Missing;
        };
        let pieces = self.pieces_of(section);
        match &found.body {
            Body::Code { .. } => {
                let line_top = |wanted: Line| {
                    pieces.iter().find_map(|placed| match placed.piece {
                        Piece::Lines { start, end } if start <= wanted && wanted <= end => Some(
                            placed.top
                                + self.row
                                    * Count::new(
                                        usize::try_from(wanted.value() - start.value())
                                            .unwrap_or(0),
                                    ),
                        ),
                        _ => None,
                    })
                };
                if line_top(line).is_none() {
                    return Aimed::Folded;
                }
                let context = Line::new(line.value().saturating_sub(3));
                Aimed::At(
                    line_top(context)
                        .or_else(|| line_top(line))
                        .unwrap_or(Px::ZERO),
                )
            }
            Body::Gone => pieces
                .iter()
                .find_map(|placed| match placed.piece {
                    Piece::Removed(number) => found
                        .gone(number)
                        .iter()
                        .position(|gone| gone.old == line)
                        .map(|down| placed.top + self.row * Count::new(down)),
                    _ => None,
                })
                .map_or(Aimed::Missing, |top| Aimed::At(self.above(top))),
            Body::Table(body) => {
                let Some(at) = RowAt::of(&body.pairs, Side::New, line)
                    .or_else(|| RowAt::of(&body.pairs, Side::Old, line))
                else {
                    return Aimed::Missing;
                };
                pieces
                    .iter()
                    .find_map(|placed| match placed.piece {
                        Piece::Pairs { from, to } if from <= at && at <= to => {
                            Some(placed.top + self.row * from.rows_to(at) - self.row)
                        }
                        _ => None,
                    })
                    .map_or(Aimed::Folded, |top| Aimed::At(self.above(top)))
            }
            Body::Collapsed => Aimed::Folded,
        }
    }

    pub(super) fn editor(&self, section: Count) -> Option<Stretch> {
        let found = self.section(section)?;
        self.pieces_of(section)
            .iter()
            .find(|placed| match placed.piece {
                Piece::Inset(number) => found
                    .insets
                    .get(number.get())
                    .is_some_and(|inset| matches!(inset.kind, InsetKind::Editing(_))),
                _ => false,
            })
            .map(|placed| Stretch {
                top: placed.top,
                height: placed.height,
            })
    }

    pub(super) fn extent(&self, view: Px) -> Px {
        let last = self.sections.len().checked_sub(1).map_or(Px::ZERO, |last| {
            let last = Count::new(last);
            self.bottom_of(last) - self.top_of(last)
        });
        let room = if self.sections.len() > 1 {
            (view - last).max(self.row)
        } else {
            self.row
        };
        self.total + room
    }
}

pub(super) fn head_id(path: &RelativePath, line: Line) -> Id {
    ids::LINES
        .with(&ids::in_file(path, center::line_count(line)))
        .id()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct RowsShown {
    pub(super) first: Count,
    pub(super) past: Count,
}

pub(super) fn rows_shown(placed: &Placed, count: Count, top: Px, bottom: Px, row: Px) -> RowsShown {
    let rows = |height: Px| Count::new(usize::try_from(height.ratio(row)).unwrap_or(0));
    let first = rows(top - placed.top).min(count);
    let past = (rows(bottom - placed.top) + Count::new(1))
        .min(count)
        .max(first);
    RowsShown { first, past }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows_of(count: usize, top: i32, offset: i32, bottom: i32) -> (usize, usize) {
        let row = Px::new(16);
        let placed = Placed {
            top: Px::new(top),
            height: row * Count::new(count),
            section: Count::ZERO,
            piece: Piece::Scrollbar,
        };
        let shown = rows_shown(
            &placed,
            Count::new(count),
            Px::new(offset),
            Px::new(bottom),
            row,
        );
        (shown.first.get(), shown.past.get())
    }

    #[test]
    fn a_run_of_rows_lays_out_only_the_rows_the_band_reaches() {
        assert_eq!(rows_of(400, 0, 0, 160), (0, 11));
        assert_eq!(rows_of(400, 0, 6240, 6400), (390, 400));
        assert_eq!(rows_of(400, 1600, 0, 1760), (0, 11));
        assert_eq!(rows_of(400, 0, 800, 960), (50, 61));
        assert_eq!(rows_of(400, 0, 808, 968), (50, 61));
        assert_eq!(rows_of(10, 0, 400, 560), (10, 10));
        assert_eq!(rows_of(3, 0, 0, 800), (0, 3));
    }

    fn line(number: u32) -> Line {
        Line::new(number)
    }

    fn sheet_of(pieces: &[(Piece, i32)]) -> Sheet<'static> {
        let mut sheet = Sheet {
            sections: Vec::new(),
            pieces: Vec::new(),
            total: Px::ZERO,
            row: Px::new(16),
        };
        for (piece, height) in pieces {
            sheet.pieces.push(Placed {
                top: sheet.total,
                height: Px::new(*height),
                section: Count::ZERO,
                piece: *piece,
            });
            sheet.total += Px::new(*height);
        }
        sheet
    }

    #[test]
    fn a_point_on_a_line_finds_that_line_and_a_point_on_anything_else_finds_none() {
        let sheet = sheet_of(&[
            (Piece::Header, 30),
            (
                Piece::Lines {
                    start: line(3),
                    end: line(8),
                },
                96,
            ),
            (Piece::Inset(Count::ZERO), 150),
            (
                Piece::Lines {
                    start: line(10),
                    end: line(10),
                },
                16,
            ),
            (Piece::Scrollbar, 8),
        ]);
        let at = |down: i32| sheet.spot(Px::new(down)).map(|hit| hit.spot);
        assert_eq!(at(10), None);
        assert_eq!(at(30), Some(Spot::Head(line(3))));
        assert_eq!(at(125), Some(Spot::Head(line(8))));
        assert_eq!(at(126), None);
        assert_eq!(at(276), Some(Spot::Head(line(10))));
        assert_eq!(at(291), Some(Spot::Head(line(10))));
        assert_eq!(at(292), None);
        assert_eq!(at(400), None);
    }

    fn two_files(path: &RelativePath) -> Sheet<'_> {
        let mut sheet = Sheet {
            sections: Vec::new(),
            pieces: Vec::new(),
            total: Px::ZERO,
            row: Px::new(16),
        };
        for _ in 0..2 {
            sheet.push(
                path,
                None,
                Some(Px::new(30)),
                Laid {
                    body: Body::Collapsed,
                    insets: Vec::new(),
                    pieces: vec![Measured::new(
                        Piece::Lines {
                            start: line(0),
                            end: line(9),
                        },
                        Px::new(160),
                    )],
                },
            );
        }
        sheet
    }

    #[test]
    fn the_header_of_the_file_at_the_top_is_pinned_until_the_next_header_pushes_it_off() {
        let path = RelativePath::new("a.rs");
        let sheet = two_files(&path);
        let pin = |offset: i32| {
            sheet
                .pinned(Px::new(offset))
                .map(|pin| (pin.section.get(), pin.push.get()))
        };
        assert_eq!(pin(0), Some((0, 0)));
        assert_eq!(pin(100), Some((0, 0)));
        assert_eq!(pin(160), Some((0, 0)));
        assert_eq!(pin(175), Some((0, 15)));
        assert_eq!(pin(189), Some((0, 29)));
        assert_eq!(pin(190), Some((1, 0)));
        assert_eq!(pin(370), Some((1, 20)));
        assert_eq!(pin(380), None);
    }
}

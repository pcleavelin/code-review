use std::collections::BTreeMap;

use crate::geometry::{Count, Extent, Point, Px, Rect};
use crate::id::Id;
use crate::input::Glyph;
use crate::layout::Hold;
use crate::text::Label;
use crate::tree::Layer;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(crate) struct LeafKey {
    pub(crate) owner: Option<Id>,
    pub(crate) ordinal: Count,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Spot {
    leaf: LeafKey,
    line: Count,
    at: Count,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
struct Order {
    leaf: Count,
    line: Count,
    at: Count,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Range {
    anchor: Spot,
    focus: Spot,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Stretch {
    start: Order,
    end: Order,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Piece {
    leaf: Count,
    line: Count,
    from: Count,
    to: Count,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
struct Away {
    down: Px,
    across: Px,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum Dragging {
    #[default]
    Still,
    Extending,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Passed {
    Before,
    Inside,
    After,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Sight {
    InView,
    OutOfView,
}

pub(crate) struct Gathering {
    ends: Option<Range>,
    passed: Passed,
}

impl Gathering {
    pub(crate) fn keeps(&mut self, key: LeafKey, sight: Sight) -> bool {
        let visible = sight == Sight::InView;
        let Some(ends) = self.ends else {
            return visible;
        };
        let anchor = ends.anchor.leaf == key;
        let focus = ends.focus.leaf == key;
        let before = self.passed;
        let end = anchor || focus;
        self.passed = match (before, anchor && focus, end) {
            (_, true, _) | (Passed::Inside, false, true) => Passed::After,
            (Passed::Before, false, true) => Passed::Inside,
            (unchanged, _, _) => unchanged,
        };
        visible || end || before == Passed::Inside
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum PressCount {
    Single,
    Double,
}

pub(crate) struct Shown {
    pub(crate) key: LeafKey,
    pub(crate) region: Option<Id>,
    pub(crate) hold: Hold,
    pub(crate) layer: Layer,
    pub(crate) rect: Rect,
    pub(crate) clip: Rect,
    pub(crate) origin: Point,
    pub(crate) cell: Extent,
    pub(crate) lines: Vec<ShownLine>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Join {
    Row,
    Wrap,
}

pub(crate) struct ShownLine {
    pub(crate) text: Label,
    pub(crate) join: Join,
}

fn characters(line: &Label) -> Count {
    Count::new(line.as_str().chars().count())
}

fn in_word(glyph: Glyph) -> bool {
    glyph.get().is_alphanumeric() || glyph.get() == '_'
}

impl Shown {
    fn last_line(&self) -> Count {
        Count::new(self.lines.len().saturating_sub(1))
    }

    fn line(&self, line: Count) -> Label {
        self.lines
            .get(line.get())
            .map(|shown| shown.text.clone())
            .unwrap_or_default()
    }

    fn line_end(&self, line: Count) -> Count {
        characters(&self.line(line))
    }

    fn join(&self, line: Count) -> Join {
        self.lines
            .get(line.get())
            .map_or(Join::Row, |shown| shown.join)
    }

    fn top(&self, line: Count) -> Px {
        self.origin.vertical + self.cell.height * line
    }

    fn x_of(&self, line: Count, at: Count) -> Px {
        let columns: usize = self
            .line(line)
            .as_str()
            .chars()
            .take(at.get())
            .map(|character| Glyph::new(character).columns())
            .sum();
        self.origin.horizontal + self.cell.width * Count::new(columns)
    }

    fn line_rect(&self, line: Count) -> Rect {
        let left = self.origin.horizontal;
        let right = self.x_of(line, self.line_end(line));
        Rect::new(left, self.top(line), right - left, self.cell.height)
    }

    fn reaches(&self, mouse: Point) -> bool {
        self.rect.contains(mouse) && self.clip.contains(mouse)
    }

    fn line_at(&self, mouse: Point) -> Count {
        let below = (mouse.vertical - self.origin.vertical).max(Px::ZERO);
        let row = usize::try_from(below.ratio(self.cell.height.max(Px::new(1)))).unwrap_or(0);
        Count::new(row.min(self.last_line().get()))
    }

    fn boundary_at(&self, line: Count, across: Px) -> Count {
        let mut pen = self.origin.horizontal;
        for (at, character) in self.line(line).as_str().chars().enumerate() {
            let width = self.cell.width * Count::new(Glyph::new(character).columns());
            if across < pen + width / 2 {
                return Count::new(at);
            }
            pen += width;
        }
        self.line_end(line)
    }

    fn character_at(&self, line: Count, across: Px) -> Option<Count> {
        let mut pen = self.origin.horizontal;
        let text = self.line(line);
        for (at, character) in text.as_str().chars().enumerate() {
            pen += self.cell.width * Count::new(Glyph::new(character).columns());
            if across < pen {
                return Some(Count::new(at));
            }
        }
        characters(&text).get().checked_sub(1).map(Count::new)
    }

    const fn spot(&self, line: Count, at: Count) -> Spot {
        Spot {
            leaf: self.key,
            line,
            at,
        }
    }

    fn word(&self, mouse: Point) -> Range {
        let line = self.line_at(mouse);
        let text: Vec<Glyph> = self.line(line).as_str().chars().map(Glyph::new).collect();
        let Some(under) = self.character_at(line, mouse.horizontal).map(Count::get) else {
            let spot = self.spot(line, Count::ZERO);
            return Range {
                anchor: spot,
                focus: spot,
            };
        };
        let wordy = |at: usize| text.get(at).copied().is_some_and(in_word);
        let (from, to) = if wordy(under) {
            let from = (0..under)
                .rev()
                .take_while(|at| wordy(*at))
                .last()
                .unwrap_or(under);
            let to = (under..text.len())
                .take_while(|at| wordy(*at))
                .last()
                .map_or(under + 1, |last| last + 1);
            (from, to)
        } else {
            (under, under + 1)
        };
        Range {
            anchor: self.spot(line, Count::new(from)),
            focus: self.spot(line, Count::new(to)),
        }
    }

    fn away(&self, line: Count, mouse: Point) -> Away {
        let rect = self.line_rect(line);
        let outside = |low: Px, high: Px, at: Px| {
            if at < low {
                low - at
            } else if at >= high {
                at - high
            } else {
                Px::ZERO
            }
        };
        Away {
            down: outside(rect.top, rect.bottom(), mouse.vertical),
            across: outside(rect.left, rect.right(), mouse.horizontal),
        }
    }

    fn visible(&self) -> bool {
        !self.rect.intersect(self.clip).is_empty()
    }

    fn line_numbers(&self) -> impl Iterator<Item = Count> {
        (0..=self.last_line().get()).map(Count::new)
    }
}

#[derive(Default)]
pub(crate) struct Selection {
    shown: Vec<Shown>,
    range: Option<Range>,
    dragging: Dragging,
}

impl Selection {
    pub(crate) fn replace(&mut self, shown: Vec<Shown>) {
        self.shown = shown;
    }

    fn pressable(&self, mouse: Point, reach: &dyn Fn(&Shown) -> bool) -> Option<&Shown> {
        self.shown
            .iter()
            .filter(|shown| shown.reaches(mouse) && reach(shown))
            .max_by_key(|shown| shown.layer)
            .filter(|shown| shown.hold != Hold::Steady)
    }

    pub(crate) fn over(&self, mouse: Point, reach: &dyn Fn(&Shown) -> bool) -> bool {
        self.pressable(mouse, reach).is_some()
    }

    pub(crate) fn press(
        &mut self,
        mouse: Point,
        press: PressCount,
        reach: &dyn Fn(&Shown) -> bool,
    ) {
        self.range = None;
        self.dragging = Dragging::Still;
        let Some(shown) = self.pressable(mouse, reach) else {
            return;
        };
        match press {
            PressCount::Double => self.range = Some(shown.word(mouse)),
            PressCount::Single => {
                let line = shown.line_at(mouse);
                let spot = shown.spot(line, shown.boundary_at(line, mouse.horizontal));
                self.range = Some(Range {
                    anchor: spot,
                    focus: spot,
                });
                self.dragging = Dragging::Extending;
            }
        }
    }

    pub(crate) fn drag(&mut self, mouse: Point) {
        if self.dragging != Dragging::Extending {
            return;
        }
        let Some(range) = self.range else {
            return;
        };
        let Some(region) = self
            .shown
            .iter()
            .find(|shown| shown.key == range.anchor.leaf)
            .map(|shown| (shown.region, shown.layer))
        else {
            return;
        };
        let same: Vec<&Shown> = self
            .shown
            .iter()
            .filter(|shown| shown.visible() && (shown.region, shown.layer) == region)
            .collect();
        let (Some(first), Some(last)) = (same.first(), same.last()) else {
            return;
        };
        let top = same.iter().map(|shown| shown.top(Count::ZERO)).min();
        let bottom = same
            .iter()
            .map(|shown| shown.line_rect(shown.last_line()).bottom())
            .max();
        let focus = if top.is_some_and(|top| mouse.vertical < top) {
            first.spot(Count::ZERO, Count::ZERO)
        } else if bottom.is_some_and(|bottom| mouse.vertical >= bottom) {
            let line = last.last_line();
            last.spot(line, last.line_end(line))
        } else {
            let nearest = same
                .iter()
                .flat_map(|shown| shown.line_numbers().map(move |line| (*shown, line)))
                .min_by_key(|(shown, line)| shown.away(*line, mouse));
            match nearest {
                Some((shown, line)) => shown.spot(line, shown.boundary_at(line, mouse.horizontal)),
                None => return,
            }
        };
        self.range = Some(Range { focus, ..range });
    }

    pub(crate) const fn release(&mut self) {
        self.dragging = Dragging::Still;
    }

    fn order(&self, spot: Spot) -> Option<Order> {
        let leaf = self.shown.iter().position(|shown| shown.key == spot.leaf)?;
        let shown = self.shown.get(leaf)?;
        let line = spot.line.min(shown.last_line());
        Some(Order {
            leaf: Count::new(leaf),
            line,
            at: spot.at.min(shown.line_end(line)),
        })
    }

    fn stretch(&self) -> Option<Stretch> {
        let range = self.range?;
        let anchor = self.order(range.anchor)?;
        let focus = self.order(range.focus)?;
        (anchor != focus).then(|| Stretch {
            start: anchor.min(focus),
            end: anchor.max(focus),
        })
    }

    fn pieces(&self) -> Vec<Piece> {
        let Some(Stretch { start, end }) = self.stretch() else {
            return Vec::new();
        };
        let mut pieces = Vec::new();
        for leaf in (start.leaf.get()..=end.leaf.get()).map(Count::new) {
            let Some(shown) = self.shown.get(leaf.get()) else {
                continue;
            };
            let first = if leaf == start.leaf {
                start.line
            } else {
                Count::ZERO
            };
            let last = if leaf == end.leaf {
                end.line
            } else {
                shown.last_line()
            };
            for line in (first.get()..=last.get()).map(Count::new) {
                let from = if (leaf, line) == (start.leaf, start.line) {
                    start.at
                } else {
                    Count::ZERO
                };
                let to = if (leaf, line) == (end.leaf, end.line) {
                    end.at
                } else {
                    shown.line_end(line)
                };
                pieces.push(Piece {
                    leaf,
                    line,
                    from,
                    to,
                });
            }
        }
        pieces
    }

    pub(crate) const fn gathering(&self) -> Gathering {
        Gathering {
            ends: self.range,
            passed: Passed::Before,
        }
    }

    pub(crate) fn marks(&self) -> BTreeMap<LeafKey, Vec<Rect>> {
        let mut marks: BTreeMap<LeafKey, Vec<Rect>> = BTreeMap::new();
        for piece in self.pieces() {
            let Some(shown) = self
                .shown
                .get(piece.leaf.get())
                .filter(|_| piece.to > piece.from)
            else {
                continue;
            };
            let left = shown.x_of(piece.line, piece.from);
            let right = shown.x_of(piece.line, piece.to);
            marks.entry(shown.key).or_default().push(Rect::new(
                left,
                shown.top(piece.line),
                right - left,
                shown.cell.height,
            ));
        }
        marks
    }

    pub(crate) fn text(&self) -> Option<Label> {
        let mut out = String::new();
        let mut before: Option<(Rect, Px)> = None;
        for piece in self.pieces() {
            let Some(shown) = self.shown.get(piece.leaf.get()) else {
                continue;
            };
            let rect = shown.line_rect(piece.line);
            if let Some((previous, cell)) = before {
                let same_row = previous.top < rect.bottom() && rect.top < previous.bottom();
                if !same_row {
                    out.truncate(out.trim_end_matches(' ').len());
                    out.push(match shown.join(piece.line) {
                        Join::Row => '\n',
                        Join::Wrap => ' ',
                    });
                } else if rect.left - previous.right() >= cell {
                    out.push(' ');
                }
            }
            out.extend(
                shown
                    .line(piece.line)
                    .as_str()
                    .chars()
                    .skip(piece.from.get())
                    .take(piece.to.get().saturating_sub(piece.from.get())),
            );
            before = Some((rect, shown.cell.width));
        }
        out.truncate(out.trim_end_matches(' ').len());
        (!out.is_empty()).then(|| Label::new(out))
    }
}

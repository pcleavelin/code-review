use std::collections::BTreeMap;

use domain::{ColumnCells, ColumnKey, ColumnWidth};
use strum::VariantArray;
use ui::{Label, Px};

use crate::theme::PIXEL;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, VariantArray)]
pub(crate) enum InboxColumn {
    Standing,
    Pull,
    Title,
    Author,
    Branch,
    Seen,
    Updated,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Literal(&'static str);

impl InboxColumn {
    const fn key(self) -> Literal {
        Literal(match self {
            Self::Standing => "standing",
            Self::Pull => "pull",
            Self::Title => "title",
            Self::Author => "author",
            Self::Branch => "branch",
            Self::Seen => "seen",
            Self::Updated => "updated",
        })
    }

    pub(crate) fn name(self) -> Label {
        Label::new(self.key().0)
    }

    fn of_key(key: &ColumnKey) -> Option<Self> {
        Self::VARIANTS
            .iter()
            .copied()
            .find(|column| column.key().0 == key.as_str())
    }

    fn default_cells(self) -> ColumnCells {
        let cells = match self {
            Self::Standing => 11,
            Self::Pull | Self::Seen => 16,
            Self::Title => 48,
            Self::Author => 12,
            Self::Branch => 24,
            Self::Updated => 18,
        };
        ColumnCells::new(cells).unwrap_or(ColumnCells::LEAST)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Order {
    Ascending,
    Descending,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Sorting {
    pub(crate) column: InboxColumn,
    pub(crate) order: Order,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Resizing {
    pub(crate) column: InboxColumn,
    pub(crate) from: ColumnCells,
    pub(crate) at: Px,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GridAct {
    Sort(InboxColumn),
    Grab(InboxColumn, Px),
    Drag(Px),
    Drop,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct InboxGrid {
    widths: BTreeMap<InboxColumn, ColumnCells>,
    pub(crate) sorting: Option<Sorting>,
    pub(crate) resizing: Option<Resizing>,
}

impl InboxGrid {
    pub(crate) fn of_saved(saved: &[ColumnWidth]) -> Self {
        Self {
            widths: saved
                .iter()
                .filter_map(|width| {
                    InboxColumn::of_key(&width.column).map(|column| (column, width.cells))
                })
                .collect(),
            sorting: None,
            resizing: None,
        }
    }

    pub(crate) fn saved(&self) -> Vec<ColumnWidth> {
        self.widths
            .iter()
            .filter_map(|(column, cells)| {
                Some(ColumnWidth {
                    column: ColumnKey::new(column.key().0)?,
                    cells: *cells,
                })
            })
            .collect()
    }

    pub(crate) fn cells(&self, column: InboxColumn) -> ColumnCells {
        self.widths
            .get(&column)
            .copied()
            .unwrap_or_else(|| column.default_cells())
    }

    pub(crate) fn act(&mut self, act: GridAct, cell: Px) {
        match act {
            GridAct::Sort(column) => self.sort_by(column),
            GridAct::Grab(column, at) => {
                self.resizing = Some(Resizing {
                    column,
                    from: self.cells(column),
                    at,
                });
            }
            GridAct::Drag(to) => {
                if let Some(Resizing { column, from, at }) = self.resizing {
                    let moved = (to - at).ratio(cell.max(PIXEL));
                    let wanted = i64::from(from.get()) + i64::from(moved);
                    let cells = u16::try_from(wanted.clamp(
                        i64::from(ColumnCells::LEAST.get()),
                        i64::from(ColumnCells::MOST.get()),
                    ))
                    .ok()
                    .and_then(ColumnCells::new)
                    .unwrap_or(from);
                    self.widths.insert(column, cells);
                }
            }
            GridAct::Drop => self.resizing = None,
        }
    }

    pub(crate) fn sort_by(&mut self, column: InboxColumn) {
        self.sorting = Some(match self.sorting {
            Some(Sorting {
                column: sorted,
                order: Order::Ascending,
            }) if sorted == column => Sorting {
                column,
                order: Order::Descending,
            },
            Some(_) | None => Sorting {
                column,
                order: Order::Ascending,
            },
        });
    }
}

#[cfg(test)]
mod tests;

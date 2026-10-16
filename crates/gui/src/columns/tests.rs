use super::*;

fn width(name: &str, cells: u16) -> ColumnWidth {
    ColumnWidth {
        column: ColumnKey::new(name).unwrap(),
        cells: ColumnCells::new(cells).unwrap(),
    }
}

#[test]
fn a_header_sorts_ascending_then_descending_and_another_header_starts_ascending() {
    let mut grid = InboxGrid::default();
    grid.sort_by(InboxColumn::Title);
    assert_eq!(
        grid.sorting,
        Some(Sorting {
            column: InboxColumn::Title,
            order: Order::Ascending
        })
    );
    grid.sort_by(InboxColumn::Title);
    assert_eq!(
        grid.sorting.map(|sorting| sorting.order),
        Some(Order::Descending)
    );
    grid.sort_by(InboxColumn::Title);
    assert_eq!(
        grid.sorting.map(|sorting| sorting.order),
        Some(Order::Ascending)
    );
    grid.sort_by(InboxColumn::Author);
    assert_eq!(
        grid.sorting,
        Some(Sorting {
            column: InboxColumn::Author,
            order: Order::Ascending
        })
    );
}

#[test]
fn dragging_an_edge_widens_its_column_by_whole_cells_within_the_bounds() {
    let mut grid = InboxGrid::of_saved(&[width("author", 12)]);
    let cell = Px::new(8);
    grid.act(GridAct::Grab(InboxColumn::Author, Px::new(100)), cell);
    grid.act(GridAct::Drag(Px::new(140)), cell);
    assert_eq!(grid.cells(InboxColumn::Author).get(), 17);
    grid.act(GridAct::Drag(Px::new(20)), cell);
    assert_eq!(grid.cells(InboxColumn::Author).get(), 4);
    grid.act(GridAct::Drop, cell);
    grid.act(GridAct::Drag(Px::new(900)), cell);
    assert_eq!(grid.cells(InboxColumn::Author).get(), 4);
    assert_eq!(grid.saved(), [width("author", 4)]);
}

#[test]
fn saved_widths_come_back_and_unknown_columns_are_dropped() {
    let grid = InboxGrid::of_saved(&[width("title", 80), width("gone", 20)]);
    assert_eq!(grid.cells(InboxColumn::Title).get(), 80);
    assert_eq!(grid.saved(), [width("title", 80)]);
}

#[test]
fn each_column_has_one_filter_and_it_names_that_column() {
    for column in InboxColumn::VARIANTS.iter().copied() {
        let named = match column.filter() {
            ColumnFilter::Words(words) => words.column(),
            ColumnFilter::Choices(dropdown) => dropdown.column(),
        };
        assert_eq!(named, column);
    }
    assert_eq!(
        InboxColumn::Status.filter(),
        ColumnFilter::Choices(Dropdown::InboxStatus)
    );
}

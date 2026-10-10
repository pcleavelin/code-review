use domain::{FileId, Seen, SymbolId};
use ui::{Count, Icon, Label, Px, Run};

use crate::action::Action;
use crate::field::Which;
use crate::ids;
use crate::model::Model;
use crate::review::ReviewAct;
use crate::text::{Clipped, Counted, Needle, Noun};
use crate::theme::{
    Cells, FILTER_FIELD, GREEN, ORANGE, PANEL_TEXT_ROOM, PENDING, PIXEL, ROW_EXTRA, SELECTED,
    SYMBOL_FIXED, SYMBOL_NAME, SYMBOL_NAME_LEAST, SYMBOLS_GUESS, SYMBOLS_LEAST, TEXT, WEAK,
};
use crate::widgets::{Chosen, Container, Frame, Scroller, TreeRow};

fn columns(frame: &Frame<'_>, rect: Option<ui::Rect>, guess: Cells, least: Cells) -> Count {
    let cell = frame.cell_width().max(PIXEL);
    let columns = rect.map_or(guess.get(), |rect| {
        (rect.width - PANEL_TEXT_ROOM - Frame::scrollbar_width())
            .ratio(cell)
            .max(least.get())
    });
    Count::new(usize::try_from(columns).unwrap_or(0))
}

enum SymbolLine {
    File(FileId),
    Symbol(SymbolId),
}

fn symbol_lines(model: &Model, needle: &Needle) -> Vec<SymbolLine> {
    let index = &model.index;
    let mut lines = Vec::new();
    let mut current: Option<(FileId, bool)> = None;
    for symbol in index.symbol_ids() {
        let file = symbol.file();
        let file_found = match current {
            Some((open, found)) if open == file => found,
            _ => {
                let found = index
                    .file(file)
                    .is_some_and(|source| needle.found_in(source.path().as_str()));
                current = Some((file, found));
                lines.push(SymbolLine::File(file));
                found
            }
        };
        if file_found
            || index
                .symbol(symbol)
                .is_some_and(|found| needle.found_in(found.name().as_str()))
        {
            lines.push(SymbolLine::Symbol(symbol));
        }
    }
    let mut kept: Vec<SymbolLine> = Vec::with_capacity(lines.len());
    for line in lines {
        if matches!(line, SymbolLine::File(_)) && matches!(kept.last(), Some(SymbolLine::File(_))) {
            kept.pop();
        }
        kept.push(line);
    }
    if matches!(kept.last(), Some(SymbolLine::File(_))) {
        kept.pop();
    }
    kept
}

pub(super) fn symbols_window(model: &Model, frame: &mut Frame<'_>) {
    let needle = Needle::new(model.fields.get(Which::SymbolFilter).text().as_str());
    let lines = symbol_lines(model, &needle);
    let shown = lines
        .iter()
        .filter(|line| matches!(line, SymbolLine::Symbol(_)))
        .count();
    let total = model.index.symbol_ids().count();
    frame.start(Container::ToolbarSmall);
    frame.field(
        &model.fields,
        Which::SymbolFilter,
        &Label::new("filter"),
        FILTER_FIELD,
    );
    frame.label(
        if needle.is_empty() {
            Counted::new(Count::new(total), Noun::Symbol).to_string()
        } else {
            format!("{shown} of {total}")
        },
        WEAK,
    );
    frame.finish();
    if lines.is_empty() {
        frame.label(
            if needle.is_empty() {
                "no symbols indexed".to_owned()
            } else {
                format!("no symbol or file matches '{}'", needle.as_str())
            },
            WEAK,
        );
        return;
    }
    let id = ids::symbols();
    let scrolled = frame.scroll_column(id, model.scrolls.get(id), Scroller::Plain, None);
    let row_height = frame.row_height() + ROW_EXTRA;
    let count = Count::new(lines.len());
    let window = frame.rows_window(
        scrolled.offset,
        scrolled.interaction.rect(),
        count,
        row_height,
        Count::new(40),
    );
    let columns = columns(
        frame,
        scrolled.interaction.rect(),
        SYMBOLS_GUESS,
        SYMBOLS_LEAST,
    );
    for line in lines
        .iter()
        .skip(window.first.get())
        .take(window.visible.get())
    {
        match line {
            SymbolLine::File(file) => symbol_file_row(model, frame, *file, columns),
            SymbolLine::Symbol(symbol) => symbol_row(model, frame, *symbol, columns),
        }
    }
    frame.rows_after(count, &window, row_height, Px::ZERO);
    frame.finish();
}

fn open_file(model: &Model, file: FileId) -> Action {
    match model
        .index
        .file(file)
        .filter(|source| model.review.change(source.path()).is_some())
    {
        Some(source) => Action::Review(ReviewAct::OpenChange(source.path().clone())),
        None => Action::GoTo(file, domain::Line::new(0)),
    }
}

fn change_badge(model: &Model, file: FileId) -> Option<Run> {
    let source = model.index.file(file)?;
    let change = model.review.change(source.path())?;
    let color = match model.review.seen(change) {
        Seen::Seen => GREEN,
        Seen::Unseen | Seen::Changed => ORANGE,
    };
    Some(Run::new(
        format!("+{} -{}", change.added(), change.removed()),
        color,
    ))
}

fn symbol_file_row(model: &Model, frame: &mut Frame<'_>, file: FileId, columns: Count) {
    let Some(source) = model.index.file(file) else {
        return;
    };
    let badge = change_badge(model, file);
    let room = columns
        .get()
        .saturating_sub(badge.as_ref().map_or(0, |run| run.text.as_str().len() + 2));
    let row = TreeRow {
        level: Count::ZERO,
        runs: vec![Run::new(
            Clipped::left(source.path().as_str(), room).to_string(),
            if source.is_pending() { PENDING } else { TEXT },
        )],
        badges: badge.into_iter().collect(),
        marked: None,
        action: None,
    };
    if frame
        .tree_row(row, ids::SYMBOL_FILE_ROW.nth(Count::new(file.number())))
        .row
        .clicked()
    {
        frame.push(open_file(model, file));
    }
}

fn symbol_row(model: &Model, frame: &mut Frame<'_>, symbol_id: SymbolId, columns: Count) {
    let index = &model.index;
    let (Some(symbol), Some(file)) = (index.symbol(symbol_id), index.file(symbol_id.file())) else {
        return;
    };
    let fixed = usize::try_from(SYMBOL_FIXED.get()).unwrap_or(0);
    let name_width = columns.get().saturating_sub(fixed).clamp(
        usize::try_from(SYMBOL_NAME_LEAST.get()).unwrap_or(0),
        usize::try_from(SYMBOL_NAME.get()).unwrap_or(0),
    );
    let name = format!(
        " {:<name_width$} ",
        Clipped::right(symbol.name().as_str(), name_width).to_string()
    );
    let place = format!(
        "{:>5}  {}",
        symbol.span().start().number(),
        symbol.kind().as_str()
    );
    let pending = file.is_pending();
    let runs = vec![
        Run::new(name, if pending { PENDING } else { TEXT }),
        Run::new(place, if pending { PENDING } else { WEAK }),
    ];
    let chosen = if model.nav.focus() == Some(symbol_id) {
        Chosen::Chosen
    } else {
        Chosen::Plain
    };
    let key = Label::new(format!("{}:{}", symbol_id.file(), symbol_id.symbol()));
    let row = TreeRow {
        level: Count::new(1 + usize::try_from(symbol.depth().value()).unwrap_or(0)),
        runs,
        badges: Vec::new(),
        marked: (chosen == Chosen::Chosen).then_some(SELECTED),
        action: None,
    };
    if frame
        .tree_row(row, ids::SYMBOL_ROW.with(&key))
        .row
        .clicked()
    {
        frame.push(Action::Focus(symbol_id));
    }
}

pub(super) fn files_window(model: &Model, frame: &mut Frame<'_>) {
    frame.start(Container::Header);
    frame.label(
        model.review.opened().map_or_else(
            || "the repository's files".to_owned(),
            |opened| {
                format!(
                    "#{} at {}; +/- marks a changed file",
                    opened.number, opened.head
                )
            },
        ),
        WEAK,
    );
    frame.finish();
    let id = ids::files();
    frame.scroll_column(id, model.scrolls.get(id), Scroller::Plain, None);
    let all: Vec<FileId> = model.index.file_entries().map(|entry| entry.id).collect();
    files_tree(model, frame, &all, Count::ZERO);
    frame.finish();
}

fn component(model: &Model, file: FileId, depth: Count) -> Label {
    Label::new(
        model
            .index
            .file(file)
            .and_then(|source| source.path().as_str().split('/').nth(depth.get()))
            .unwrap_or_default(),
    )
}

fn is_file(model: &Model, file: FileId, depth: Count) -> bool {
    model
        .index
        .file(file)
        .is_some_and(|source| source.path().as_str().split('/').count() == depth.get() + 1)
}

fn changed_inside(model: &Model, files: &[FileId]) -> Count {
    Count::new(
        files
            .iter()
            .filter(|file| change_badge(model, **file).is_some())
            .count(),
    )
}

fn files_tree(model: &Model, frame: &mut Frame<'_>, files: &[FileId], depth: Count) {
    let mut rest = files;
    while let Some((first, _)) = rest.split_first() {
        let first = *first;
        if is_file(model, first, depth) {
            let name = component(model, first, depth);
            let badge = change_badge(model, first);
            let color = if badge.is_some() { TEXT } else { WEAK };
            let row = TreeRow {
                level: depth,
                runs: vec![Run::new(name.as_str(), color)],
                badges: badge.into_iter().collect(),
                marked: (model.nav.file() == Some(first)).then_some(SELECTED),
                action: None,
            };
            if frame
                .tree_row(row, ids::FILE_ROW.nth(Count::new(first.number())))
                .row
                .clicked()
            {
                frame.push(open_file(model, first));
            }
            rest = rest.get(1..).unwrap_or(&[]);
            continue;
        }
        let directory = component(model, first, depth);
        let inside = rest
            .iter()
            .take_while(|file| {
                !is_file(model, **file, depth) && component(model, **file, depth) == directory
            })
            .count();
        let (group, after) = rest.split_at(inside);
        let changed = changed_inside(model, group);
        let prefix = model
            .index
            .file(first)
            .map(|source| {
                source
                    .path()
                    .as_str()
                    .split('/')
                    .take(depth.get() + 1)
                    .collect::<Vec<_>>()
                    .join("/")
            })
            .unwrap_or_default();
        let open = (depth.get() == 0) ^ model.directories.contains(&Label::new(prefix.clone()));
        let marker = if open {
            Icon::Expanded
        } else {
            Icon::Collapsed
        };
        let badges = if changed.get() > 0 {
            vec![Run::new(
                Counted::new(changed, Noun::File).to_string(),
                ORANGE,
            )]
        } else {
            Vec::new()
        };
        let row = TreeRow {
            level: depth,
            runs: vec![Run::new(
                format!("{} {}/", marker.glyph().get(), directory.as_str()),
                TEXT,
            )],
            badges,
            marked: None,
            action: None,
        };
        if frame
            .tree_row(row, ids::DIRECTORY_ROW.with(&Label::new(prefix.as_str())))
            .row
            .clicked()
        {
            frame.push(Action::ToggleDirectory(Label::new(prefix)));
        }
        if open {
            files_tree(model, frame, group, Count::new(depth.get() + 1));
        }
        rest = after;
    }
}

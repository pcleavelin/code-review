use std::rc::Rc;

use domain::{CodePlace, FileChange, FileId, Line, Linked, LinkedFile, Revision, Side, Span};
use ui::{Axis, Button, Count, Icon, Label, Px, Run};

use crate::action::Action;
use crate::grid;
use crate::ids::{self, SlotKind};
use crate::keys::{self, LineGesture};
use crate::model::Model;
use crate::nav::{Aim, Kept, LineRequest};
use crate::review::{Picking, ReviewAct};
use crate::text::{Counted, Noun};
use crate::theme::{
    ADDED_FILL, DRAFTED_BAR, FIELD, GREEN, LINES_SELECTED, ORANGE, PANEL_PADDING, RED,
    REMOVED_FILL, SOURCE_GUESS, TEXT, THREAD_BAR, WEAK,
};
use crate::widgets::{CodeLine, Container, Frame, Padding, Scroller};

use super::center::{self, Aimed, Band, InsetKind, gap, into_view};
use super::review;
use super::sheet::{Body, Piece, Placed, Pointed, Section, Sheet, Spot, head_id, rows_shown};
use super::table::Table;

enum Showing<'model> {
    Changes(&'model [FileChange]),
    Reading,
    Alone,
}

fn showing(model: &Model) -> Showing<'_> {
    let Some(open) = model.review.open.as_ref() else {
        return Showing::Alone;
    };
    let changes = match model.review.diffs.commit() {
        Some(commit) => match model.review.diffs.commit_changes(commit) {
            Some(changes) => changes,
            None => return Showing::Reading,
        },
        None => open.opened.changes.as_slice(),
    };
    match model.shown_path() {
        Some(path) if !changes.iter().any(|change| change.path() == &path) => Showing::Alone,
        Some(_) | None => Showing::Changes(changes),
    }
}

pub(super) fn source(model: &Model, frame: &mut Frame<'_>) {
    match showing(model) {
        Showing::Changes(changes) => {
            let sheet = Sheet::of(model, frame, changes);
            changes_toolbar(
                model,
                frame,
                changes,
                sheet.kept(model.scrolls.get(ids::source())),
            );
            column(model, frame, &sheet);
        }
        Showing::Reading => {
            frame.start(Container::Toolbar);
            review::diff_controls(model, frame, None);
            frame.finish();
            frame.note("reading the commit's changes...", WEAK, Padding::Step);
        }
        Showing::Alone => {
            let Some(sheet) = model
                .nav
                .file()
                .and_then(|file| Sheet::alone(model, frame, file))
            else {
                frame.note(
                    "open a changed file from Changes, or click a symbol or a file",
                    WEAK,
                    Padding::Step,
                );
                return;
            };
            if let Some(section) = sheet.sections.first() {
                center::source_toolbar(model, frame, section.path);
            }
            column(model, frame, &sheet);
        }
    }
}

fn changes_toolbar(
    model: &Model,
    frame: &mut Frame<'_>,
    changes: &[FileChange],
    kept: Option<Kept>,
) {
    frame.start(Container::Toolbar);
    frame.label(
        Counted::new(Count::new(changes.len()), Noun::ChangedFile).to_string(),
        TEXT,
    );
    review::diff_controls(model, frame, kept);
    frame.finish();
}

fn reachable(frame: &Frame<'_>) -> Px {
    frame
        .ui
        .placement(ids::source())
        .map_or(Px::ZERO, |placement| {
            (placement.content.height - placement.rect.height).max(Px::ZERO)
        })
}

fn aimed(
    model: &Model,
    frame: &mut Frame<'_>,
    sheet: &Sheet<'_>,
    request: LineRequest,
    offset: Px,
) -> Option<Px> {
    let path = model.shown_path();
    let Some(at) = path.as_ref().and_then(|path| sheet.find(path)) else {
        frame.push(Action::ScrolledToLine(request.ticket));
        return None;
    };
    let section = sheet.section(at)?;
    if request.aim == Aim::Top {
        return Some(sheet.top_of(at));
    }
    if matches!(section.body, Body::Collapsed) {
        frame.push(Action::Review(ReviewAct::Uncollapse(section.path.clone())));
        return None;
    }
    let view = frame
        .ui
        .placement(ids::source())
        .map_or(SOURCE_GUESS, |placement| placement.rect.height);
    let row = frame.row_height();
    let editor = match request.aim {
        Aim::Editor => sheet
            .editor(at)
            .map(|stretch| into_view(stretch.top, stretch.height, row, offset, view)),
        Aim::Line | Aim::Top => None,
    };
    if editor.is_some() {
        return editor;
    }
    match sheet.aim(at, request.line) {
        Aimed::At(top) => Some(top),
        Aimed::Folded => {
            frame.push(Action::Review(ReviewAct::Reveal(
                section.path.clone(),
                request.line,
            )));
            None
        }
        Aimed::Missing => Some(sheet.top_of(at)),
    }
}

fn scroll_offset(model: &Model, frame: &mut Frame<'_>, sheet: &Sheet<'_>) -> Px {
    let id = ids::source();
    let column = frame.ui.placement(id);
    let mut offset = model.scrolls.get(id);
    let pointer = frame.ui.pointer();
    if model.line_grab.is_some()
        && pointer.down.contains(Button::Left)
        && let Some(column) = column
    {
        offset += center::edge_scroll(pointer.mouse.vertical, column.rect, frame.row_height());
    }
    let Some(request) = model.nav.scroll_to() else {
        return offset;
    };
    let Some(wanted) = aimed(model, frame, sheet, request, offset) else {
        return offset;
    };
    let view = column.map_or(SOURCE_GUESS, |placement| placement.rect.height);
    let target = wanted
        .min((sheet.extent(view) - view).max(Px::ZERO))
        .max(Px::ZERO);
    if target <= reachable(frame) {
        frame.push(Action::ScrolledToLine(request.ticket));
    }
    target
}

fn file_header(model: &Model, frame: &mut Frame<'_>, section: &Section<'_>, height: Px) {
    let Some(change) = section.change else {
        return;
    };
    let path = section.path;
    let key = Label::new(path.as_str());
    frame.start(Container::FileHeader {
        id: ids::file_header().with(path.as_str()),
        height,
    });
    let collapse = ids::COLLAPSE.with(&key);
    let icon = if matches!(section.body, Body::Collapsed) {
        Icon::Collapsed
    } else {
        Icon::Expanded
    };
    if frame
        .inline_button(vec![Run::new(icon, WEAK)], collapse)
        .clicked()
    {
        frame.push(Action::Review(ReviewAct::ToggleCollapsed(path.clone())));
    }
    frame.attach_tip(collapse);
    let commit = model.review.diffs.commit();
    let mut runs = Vec::new();
    if commit.is_none() {
        runs.push(review::seen_mark(model.review.seen(change)));
    }
    runs.push(Run::new(format!(" {path}"), TEXT));
    if let Some(old) = change.old_path() {
        runs.push(Run::new(format!("  from {old}"), WEAK));
    }
    runs.push(Run::new(
        format!("  {}", review::kind_word(change.kind()).as_str()),
        WEAK,
    ));
    if model.review.generated(path) {
        runs.push(Run::new("  generated", WEAK));
    }
    runs.push(Run::new(format!("  +{}", change.added()), GREEN));
    runs.push(Run::new(format!(" -{}", change.removed()), RED));
    let drafts = model.review.drafts_in(path).count();
    if drafts > 0 && commit.is_none() {
        runs.push(Run::new(
            format!("  {}", Counted::new(Count::new(drafts), Noun::Draft)),
            ORANGE,
        ));
    }
    frame.label_runs(runs);
    frame.grow();
    if commit.is_none() {
        review::seen_button(model, frame, change);
    }
    if !matches!(section.body, Body::Gone | Body::Collapsed) {
        review::whole_button(model, frame, path);
    }
    let link = model.review.open.as_ref().and_then(|open| {
        let shown = model.shown_path().as_ref() == Some(path);
        open.link(&Linked::File(LinkedFile {
            at: Revision::Commit(commit.unwrap_or(&open.opened.head).clone()),
            path: path.clone(),
            lines: model
                .nav
                .lines()
                .filter(|_| shown)
                .and_then(|lines| Span::new(lines.low(), lines.high())),
        }))
    });
    review::link_buttons(frame, link, &key);
    frame.finish();
}

struct CodeLook<'look> {
    file: FileId,
    selection: Option<Span>,
    added: &'look [Line],
    commented: Vec<Line>,
    threaded: Vec<Line>,
}

impl CodeLook<'_> {
    fn background(&self, line: Line) -> Option<ui::Color> {
        if self.selection.is_some_and(|chosen| chosen.contains(line)) {
            Some(LINES_SELECTED)
        } else {
            self.added
                .binary_search(&line)
                .is_ok()
                .then_some(ADDED_FILL)
        }
    }

    fn bar(&self, line: Line) -> Option<ui::Color> {
        if self.commented.contains(&line) {
            Some(DRAFTED_BAR)
        } else if self.threaded.contains(&line) {
            Some(THREAD_BAR)
        } else {
            None
        }
    }
}

enum Look<'look> {
    Code(CodeLook<'look>),
    Table(Option<Table<'look>>),
    Plain,
}

struct Painting<'look> {
    section: Count,
    look: Look<'look>,
    columns: Count,
    across: Px,
}

fn painting<'look>(
    model: &'look Model,
    frame: &Frame<'_>,
    sheet: &'look Sheet<'look>,
    section: Count,
    width: Option<Px>,
) -> Option<Painting<'look>> {
    let found = sheet.section(section)?;
    let path = found.path;
    let removed = center::removed_columns(found.insets.iter());
    let (look, columns) = match &found.body {
        Body::Code { file, added, .. } => {
            let widest = sheet
                .pieces_of(section)
                .iter()
                .filter_map(|placed| match placed.piece {
                    Piece::Lines { start, end } => Span::new(start, end),
                    _ => None,
                })
                .fold(Count::ZERO, |widest, span| {
                    widest.max(center::text_columns(model, *file, span))
                });
            let look = CodeLook {
                file: *file,
                selection: model
                    .nav
                    .lines()
                    .filter(|_| model.nav.file() == Some(*file))
                    .and_then(|lines| Span::new(lines.low(), lines.high())),
                added,
                commented: model
                    .review
                    .drafts_in(path)
                    .filter(|draft| draft.side == Side::New)
                    .flat_map(|draft| draft.lines().lines())
                    .collect(),
                threaded: found
                    .insets
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
            };
            (Look::Code(look), widest.max(removed))
        }
        Body::Table(body) => (
            Look::Table(body.table(model, frame, path, width)),
            Count::ZERO,
        ),
        Body::Gone | Body::Collapsed => (Look::Plain, removed),
    };
    Some(Painting {
        section,
        look,
        columns,
        across: model.across.get(ids::LINES.id()),
    })
}

struct Drawing<'draw> {
    model: &'draw Model,
    sheet: &'draw Sheet<'draw>,
    band: Band,
    width: Option<Px>,
}

impl<'draw> Drawing<'draw> {
    fn piece(&self, frame: &mut Frame<'_>, placed: &Placed, painting: &Painting<'draw>) {
        let Some(section) = self.sheet.section(placed.section) else {
            return;
        };
        let model = self.model;
        match placed.piece {
            Piece::Header => file_header(model, frame, section, placed.height),
            Piece::Lines { start, end } => {
                let count =
                    Count::new(usize::try_from(end.value() - start.value() + 1).unwrap_or(0));
                self.rows(frame, placed, count, |frame, rows| {
                    let line = Line::new(
                        start
                            .value()
                            .saturating_add(u32::try_from(rows.get()).unwrap_or(0)),
                    );
                    self.head_line(frame, section, painting, line);
                });
            }
            Piece::Removed(number) => {
                let lines = section.gone(number);
                let side = center::old_side(model, frame, section.path);
                self.rows(frame, placed, Count::new(lines.len()), |frame, rows| {
                    if let Some(gone) = lines.get(rows.get()) {
                        self.removed_line(frame, section, painting, gone, side);
                    }
                });
            }
            Piece::Pairs { from, to } => {
                if let Look::Table(Some(table)) = &painting.look {
                    self.rows(frame, placed, from.rows_to(to), |frame, rows| {
                        table.row(model, frame, from.down(rows));
                    });
                }
            }
            Piece::Fold { start, end } => {
                let last = match &section.body {
                    Body::Code { last, .. } => *last,
                    Body::Table(body) => body.last,
                    Body::Gone | Body::Collapsed => end,
                };
                frame.start(Container::Slot {
                    id: ids::slot(section.path, SlotKind::Fold, center::line_count(start)),
                    height: placed.height,
                });
                center::fold(frame, section.path, start, end, last);
                frame.finish();
            }
            Piece::Inset(number) => {
                if let Some(inset) = section.insets.get(number.get()) {
                    center::comment_row(model, frame, inset, self.width);
                }
            }
            Piece::Scrollbar => {
                let content = frame.cell_width() * painting.columns;
                let id = ids::slot(section.path, SlotKind::Scrollbar, Count::ZERO);
                let moved = frame.across_scrollbar(id, painting.across, content);
                if moved != painting.across {
                    frame.push(Action::ScrollAcross(ids::LINES.id(), moved));
                }
            }
            Piece::Waiting => {
                frame.start(Container::Slot {
                    id: ids::slot(section.path, SlotKind::Waiting, Count::ZERO),
                    height: placed.height,
                });
                if let Body::Table(body) = &section.body {
                    body.waiting(model, frame);
                }
                frame.finish();
            }
        }
    }

    fn rows(
        &self,
        frame: &mut Frame<'_>,
        placed: &Placed,
        count: Count,
        mut each: impl FnMut(&mut Frame<'_>, Count),
    ) {
        let row = self.sheet.row;
        let shown = rows_shown(placed, count, self.band.offset, self.band.bottom, row);
        gap(frame, row * shown.first);
        for rows in shown.first.get()..shown.past.get() {
            each(frame, Count::new(rows));
        }
        gap(
            frame,
            row * Count::new(count.get().saturating_sub(shown.past.get())),
        );
    }

    fn head_line(
        &self,
        frame: &mut Frame<'_>,
        section: &Section<'_>,
        painting: &Painting<'_>,
        line: Line,
    ) {
        let Look::Code(look) = &painting.look else {
            return;
        };
        let id = head_id(section.path, line);
        let interaction = frame.code_line(
            self.model,
            &CodeLine {
                file: look.file,
                line,
                id,
                across: painting.across,
                background: look.background(line),
                bar: look.bar(line),
            },
        );
        if interaction.hovered() && frame.scrolled_across(id) {
            let content = frame.cell_width() * painting.columns;
            let moved = frame.wheel_across(id, painting.across, content);
            if moved != painting.across {
                frame.push(Action::ScrollAcross(ids::LINES.id(), moved));
            }
        }
    }

    fn removed_line(
        &self,
        frame: &mut Frame<'_>,
        section: &Section<'_>,
        painting: &Painting<'_>,
        gone: &center::GoneLine<'_>,
        side: Option<&domain::SourceFile>,
    ) {
        let old = gone.old;
        let marked = self
            .model
            .review
            .picked
            .as_ref()
            .filter(|picked| &picked.file == section.path && picked.lines().contains(old))
            .map(|_| LINES_SELECTED);
        let target = center::removed_target(section.path, old);
        let line = frame.grid_line(
            Rc::new(grid::removed(old, gone.text, side)),
            painting.across,
            target,
            marked,
            REMOVED_FILL,
        );
        if line.hovered() && frame.scrolled_across(target.id()) {
            let content = frame.cell_width() * painting.columns;
            let moved = frame.wheel_across(target.id(), painting.across, content);
            if moved != painting.across {
                frame.push(Action::ScrollAcross(ids::LINES.id(), moved));
            }
        }
        if line.double_clicked() {
            frame.push(Action::Review(ReviewAct::StartDraftHere));
        }
    }
}

fn scrolled_to_file(model: &Model, frame: &mut Frame<'_>, sheet: &Sheet<'_>, offset: Px) {
    if model.nav.scroll_to().is_some() || sheet.sections.len() < 2 || model.line_grab.is_some() {
        return;
    }
    let Some(section) = sheet
        .holding(offset + frame.row_height())
        .and_then(|at| sheet.section(at))
    else {
        return;
    };
    if model.shown_path().as_ref() != Some(section.path) {
        frame.push(Action::Review(ReviewAct::ScrolledTo(section.path.clone())));
    }
}

const fn picking(gesture: LineGesture) -> Picking {
    match gesture {
        LineGesture::Press => Picking::Press,
        LineGesture::Extend => Picking::Extend,
        LineGesture::Drag => Picking::Drag,
    }
}

fn press(model: &Model, frame: &mut Frame<'_>, sheet: &Sheet<'_>, down: Px) {
    let pointer = frame.ui.pointer();
    let Some(Pointed { section, spot }) = sheet.spot(down) else {
        return;
    };
    let Some(found) = sheet.section(section) else {
        return;
    };
    let Some(interaction) = sheet
        .row_id(section, spot)
        .map(|id| frame.ui.interaction(id))
    else {
        return;
    };
    let Some(gesture) =
        keys::selects_line(interaction, pointer).filter(|gesture| *gesture != LineGesture::Drag)
    else {
        return;
    };
    let path = found.path.clone();
    let head = match spot {
        Spot::Head(line) => Some(line),
        Spot::Old(_) => None,
        Spot::Pair(at) => {
            let Some((pair, rect)) = at.pair(found.pairs()).zip(interaction.rect()) else {
                return;
            };
            match super::table::half_at(
                rect,
                model.review.diffs.layout,
                pair,
                pointer.mouse.horizontal,
            ) {
                Side::New => pair.new,
                Side::Old => None,
            }
        }
    };
    match (head, sheet.old_line(section, spot)) {
        (Some(line), _) => frame.push(Action::SelectLine(path, line, gesture)),
        (None, Some(old)) => frame.push(Action::Review(ReviewAct::PickRemoved(
            path,
            old,
            picking(gesture),
        ))),
        (None, None) => {}
    }
}

fn drag(model: &Model, frame: &mut Frame<'_>, sheet: &Sheet<'_>, down: Px) {
    if model.line_grab.is_some() {
        if let Some(path) = model.shown_path()
            && let Some(section) = sheet.find(&path)
            && let Some(line) = sheet.head_near(section, down)
        {
            frame.push(Action::SelectLine(path, line, LineGesture::Drag));
        }
        return;
    }
    let Some(picked) = model.review.picked.as_ref() else {
        return;
    };
    let Some(section) = sheet.find(&picked.file) else {
        return;
    };
    if sheet
        .old_row_id(section, picked.from)
        .is_none_or(|origin| frame.ui.active() != Some(origin))
    {
        return;
    }
    if let Some(hit) = sheet.spot(down)
        && hit.section == section
        && let Some(old) = sheet.old_line(section, hit.spot)
    {
        frame.push(Action::Review(ReviewAct::PickRemoved(
            picked.file.clone(),
            old,
            Picking::Drag,
        )));
    }
}

fn select(model: &Model, frame: &mut Frame<'_>, sheet: &Sheet<'_>) {
    let pointer = frame.ui.pointer();
    let held = pointer.down.contains(Button::Left);
    if let Some(column) = frame.ui.placement(ids::source())
        && !frame.ui.dragging()
    {
        let down = pointer.mouse.vertical - column.rect.top - PANEL_PADDING
            + column.scroll_offset.vertical;
        let on_scrollbar = column
            .scrollbar(Axis::Vertical, column.scroll_offset.vertical)
            .is_some_and(|bar| bar.track.contains(pointer.mouse));
        if pointer.pressed.contains(Button::Left) {
            if !on_scrollbar {
                press(model, frame, sheet, down);
            }
        } else if held {
            drag(model, frame, sheet, down);
        }
    }
    if model.line_grab.is_some() && !held {
        frame.push(Action::ClearLineSelection);
    }
}

fn column<'body>(model: &'body Model, frame: &mut Frame<'_>, sheet: &'body Sheet<'body>) {
    let offset = scroll_offset(model, frame, sheet);
    let scrolled = frame.scroll_column(ids::source(), offset, Scroller::Plain, Some(FIELD));
    if scrolled.offset != offset {
        scrolled_to_file(model, frame, sheet, scrolled.offset);
    }
    let rect = scrolled.interaction.rect();
    let view = rect.map_or(SOURCE_GUESS, |rect| rect.height);
    let width = rect.map(|rect| rect.width);
    let band = center::band(frame, scrolled.offset, view);
    let drawing = Drawing {
        model,
        sheet,
        band,
        width,
    };
    let mut drawn = Px::ZERO;
    let mut painted: Option<Painting<'body>> = None;
    for placed in sheet.reaching(band.offset, band.bottom) {
        gap(frame, placed.top - drawn);
        if painted
            .as_ref()
            .is_none_or(|painting| painting.section != placed.section)
        {
            painted = painting(model, frame, sheet, placed.section, width);
        }
        if let Some(painting) = &painted {
            drawing.piece(frame, placed, painting);
        }
        drawn = placed.bottom();
    }
    select(model, frame, sheet);
    gap(frame, sheet.extent(view) - drawn);
    frame.finish();
}

use std::collections::{BTreeMap, BTreeSet};

use domain::{
    DiffLayout, FileChange, FileId, Line, Linked, LinkedFile, RelativePath, Revision, Span,
};
use ui::{Axis, Button, Count, Icon, Label, Px, Run};

use crate::action::Action;
use crate::ids;
use crate::model::Model;
use crate::nav::{Aim, LineRequest};
use crate::review::ReviewAct;
use crate::text::{Counted, Noun};
use crate::theme::{FIELD, FILE_HEADER_EXTRA, GREEN, ORANGE, RED, SOURCE_GUESS, TEXT, WEAK};
use crate::widgets::{Container, Frame, Padding, Scroller};

use super::center::{
    self, Aimed, Band, CodeBody, Drawn, GoneBody, Segment, Stretch, gap, into_view,
};
use super::review;
use super::table::TableBody;

enum Body<'body> {
    Code(CodeBody<'body>),
    Gone(GoneBody<'body>),
    Table(TableBody<'body>),
    Collapsed,
}

impl Body<'_> {
    const fn total(&self) -> Px {
        match self {
            Self::Code(body) => body.total(),
            Self::Gone(body) => body.total(),
            Self::Table(body) => body.total(),
            Self::Collapsed => Px::ZERO,
        }
    }

    fn aim(&self, line: Line) -> Aimed {
        match self {
            Self::Code(body) => body.aim(line),
            Self::Gone(body) => body.aim(line),
            Self::Table(body) => body.aim(line),
            Self::Collapsed => Aimed::Folded,
        }
    }

    fn editor(&self) -> Option<Stretch> {
        match self {
            Self::Code(body) => body.editor(),
            Self::Gone(body) => body.editor(),
            Self::Table(body) => body.editor(),
            Self::Collapsed => None,
        }
    }
}

struct Section<'body> {
    path: &'body RelativePath,
    change: Option<&'body FileChange>,
    top: Px,
    header: Px,
    body: Body<'body>,
}

impl Section<'_> {
    fn body_top(&self) -> Px {
        self.top + self.header
    }

    fn height(&self) -> Px {
        self.header + self.body.total()
    }
}

struct Stack<'body> {
    sections: Vec<Section<'body>>,
    total: Px,
}

impl<'body> Stack<'body> {
    fn of(model: &'body Model, frame: &Frame<'_>, changes: &'body [FileChange]) -> Self {
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
        let mut top = Px::ZERO;
        let mut sections = Vec::with_capacity(changes.len());
        for change in changes {
            let path = change.path();
            let indexed = files.get(path).copied();
            let body = if model.review.collapsed(path) {
                Body::Collapsed
            } else if tabled {
                Body::Table(TableBody::build(model, frame, change, indexed))
            } else {
                indexed
                    .and_then(|file| CodeBody::build(model, frame, file, Some(change)))
                    .map_or_else(
                        || Body::Gone(GoneBody::build(model, frame, change)),
                        Body::Code,
                    )
            };
            let section = Section {
                path,
                change: Some(change),
                top,
                header,
                body,
            };
            top += section.height();
            sections.push(section);
        }
        Self {
            sections,
            total: top,
        }
    }

    fn alone(body: CodeBody<'body>, path: &'body RelativePath) -> Self {
        let total = body.total();
        Self {
            sections: vec![Section {
                path,
                change: None,
                top: Px::ZERO,
                header: Px::ZERO,
                body: Body::Code(body),
            }],
            total,
        }
    }

    fn extent(&self, view: Px, row: Px) -> Px {
        let last = self.sections.last().map_or(Px::ZERO, Section::height);
        let room = if self.sections.len() > 1 {
            (view - last).max(row)
        } else {
            row
        };
        self.total + room
    }

    fn holding(&self, offset: Px) -> Option<&Section<'body>> {
        self.sections
            .iter()
            .find(|section| section.top <= offset && offset < section.top + section.height())
            .or_else(|| self.sections.last().filter(|section| section.top <= offset))
    }
}

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
            changes_toolbar(model, frame, changes);
            let stack = Stack::of(model, frame, changes);
            column(model, frame, &stack);
        }
        Showing::Reading => {
            frame.start(Container::Toolbar);
            review::diff_controls(model, frame);
            frame.finish();
            frame.note("reading the commit's changes...", WEAK, Padding::Step);
        }
        Showing::Alone => {
            let Some(body) = model.nav.file().and_then(|file| {
                let path = model.index.file(file).map(domain::SourceFile::path);
                CodeBody::build(
                    model,
                    frame,
                    file,
                    path.and_then(|path| model.review.change(path)),
                )
            }) else {
                frame.note(
                    "open a changed file from Changes, or click a symbol or a file",
                    WEAK,
                    Padding::Step,
                );
                return;
            };
            let Some(path) = model
                .nav
                .file()
                .and_then(|file| model.index.file(file))
                .map(domain::SourceFile::path)
            else {
                return;
            };
            center::source_toolbar(model, frame, path);
            column(model, frame, &Stack::alone(body, path));
        }
    }
}

fn changes_toolbar(model: &Model, frame: &mut Frame<'_>, changes: &[FileChange]) {
    frame.start(Container::Toolbar);
    frame.label(
        Counted::new(Count::new(changes.len()), Noun::ChangedFile).to_string(),
        TEXT,
    );
    review::diff_controls(model, frame);
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
    stack: &Stack<'_>,
    request: LineRequest,
    offset: Px,
) -> Option<Px> {
    let path = model.shown_path();
    let Some(section) = stack
        .sections
        .iter()
        .find(|section| Some(section.path) == path.as_ref())
    else {
        frame.push(Action::ScrolledToLine(request.ticket));
        return None;
    };
    if request.aim == Aim::Top {
        return Some(section.top);
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
        Aim::Editor => section.body.editor().map(|stretch| {
            into_view(
                section.body_top() + stretch.top,
                stretch.height,
                row,
                offset,
                view,
            )
        }),
        Aim::Line | Aim::Top => None,
    };
    if editor.is_some() {
        return editor;
    }
    match section.body.aim(request.line) {
        Aimed::At(top) => Some(section.body_top() + top),
        Aimed::Folded => {
            frame.push(Action::Review(ReviewAct::Reveal(
                section.path.clone(),
                request.line,
            )));
            None
        }
        Aimed::Missing => Some(section.top),
    }
}

fn scroll_offset(model: &Model, frame: &mut Frame<'_>, stack: &Stack<'_>) -> Px {
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
    let Some(wanted) = aimed(model, frame, stack, request, offset) else {
        return offset;
    };
    let view = column.map_or(SOURCE_GUESS, |placement| placement.rect.height);
    let target = wanted
        .min((stack.extent(view, frame.row_height()) - view).max(Px::ZERO))
        .max(Px::ZERO);
    if target <= reachable(frame) {
        frame.push(Action::ScrolledToLine(request.ticket));
    }
    target
}

fn file_header(model: &Model, frame: &mut Frame<'_>, section: &Section<'_>) {
    let Some(change) = section.change else {
        return;
    };
    let path = section.path;
    let key = Label::new(path.as_str());
    frame.start(Container::FileHeader {
        id: ids::file_header().with(path.as_str()),
        height: section.header,
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
    if !matches!(section.body, Body::Gone(_) | Body::Collapsed) {
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

fn body<'body>(
    model: &'body Model,
    frame: &mut Frame<'_>,
    section: &Section<'body>,
    width: Option<Px>,
    band: Band,
) -> Drawn<'body> {
    let local = band.below(section.body_top());
    match &section.body {
        Body::Code(body) => body.draw(model, frame, width, local),
        Body::Gone(body) => Drawn {
            segments: Vec::new(),
            height: body.draw(model, frame, width, local),
        },
        Body::Table(body) => Drawn {
            segments: Vec::new(),
            height: body.draw(model, frame, width, local),
        },
        Body::Collapsed => Drawn {
            segments: Vec::new(),
            height: Px::ZERO,
        },
    }
}

fn scrolled_to_file(model: &Model, frame: &mut Frame<'_>, stack: &Stack<'_>, offset: Px) {
    if model.nav.scroll_to().is_some() || stack.sections.len() < 2 {
        return;
    }
    let Some(section) = stack.holding(offset + frame.row_height()) else {
        return;
    };
    if model.shown_path().as_ref() != Some(section.path) {
        frame.push(Action::Review(ReviewAct::ScrolledTo(section.path.clone())));
    }
}

fn column<'body>(model: &'body Model, frame: &mut Frame<'_>, stack: &Stack<'body>) {
    let offset = scroll_offset(model, frame, stack);
    let scrolled = frame.scroll_column(ids::source(), offset, Scroller::Plain, Some(FIELD));
    if scrolled.offset != offset {
        scrolled_to_file(model, frame, stack, scrolled.offset);
    }
    let rect = scrolled.interaction.rect();
    let view = rect.map_or(SOURCE_GUESS, |rect| rect.height);
    let width = rect.map(|rect| rect.width);
    let band = center::band(frame, scrolled.offset, view);
    let mut drawn = Px::ZERO;
    let mut segments: Vec<Segment<'body>> = Vec::new();
    for section in stack
        .sections
        .iter()
        .filter(|section| band.reaches(section.top, section.height()))
    {
        gap(frame, section.top - drawn);
        file_header(model, frame, section);
        let shown = body(model, frame, section, width, band);
        segments.extend(shown.segments);
        drawn = section.body_top() + shown.height;
    }
    if !segments.is_empty() {
        let scrollbar = frame
            .ui
            .placement(ids::source())
            .and_then(|placement| placement.scrollbar(Axis::Vertical, scrolled.offset));
        center::select_lines(model, frame, &segments, scrollbar);
    } else if model.line_grab.is_some() && !frame.ui.pointer().down.contains(Button::Left) {
        frame.push(Action::ClearLineSelection);
    }
    gap(frame, stack.extent(view, frame.row_height()) - drawn);
    frame.finish();
}

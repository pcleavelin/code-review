use domain::{Column, FileId, Language, Line, Span};
use ui::{
    Button, Canvas, Color, Count, Draw, Extent, Id, Interaction, Kind, Layout, Point, Px, Rect,
    ScrollAxes, Style,
};

use crate::action::Action;
use crate::grid::GUTTER;
use crate::keys::{self, CodeGesture};
use crate::model::Model;
use crate::peek::{Hovering, Intent, Probe};
use crate::theme::SELECTED_BAR;
use crate::widgets::{Frame, Scrolled, TipAt};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Width {
    Wide,
    Fit,
}

pub(crate) struct Marks<'marks> {
    pub(crate) background: &'marks dyn Fn(Line) -> Option<Color>,
    pub(crate) bar: &'marks dyn Fn(Line) -> Option<Color>,
}

pub(crate) struct CodeBlock<'marks> {
    pub(crate) file: FileId,
    pub(crate) start: Line,
    pub(crate) end: Line,
    pub(crate) id: Id,
    pub(crate) width: Width,
    pub(crate) marks: Marks<'marks>,
}

pub(crate) struct CodeLine {
    pub(crate) file: FileId,
    pub(crate) line: Line,
    pub(crate) id: Id,
    pub(crate) across: Px,
    pub(crate) background: Option<Color>,
    pub(crate) bar: Option<Color>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CodeSpot {
    line: Line,
    column: Option<Column>,
}

struct RowMarks {
    background: Option<Color>,
    bar: Option<Color>,
}

fn column_at(mouse: Px, left: Px, cell: Px, across: Px) -> Option<Column> {
    let column = (mouse - left + across).ratio(cell).max(0);
    usize::try_from(column)
        .unwrap_or(0)
        .checked_sub(GUTTER.get())
        .map(|column| Column::new(u32::try_from(column).unwrap_or(0)))
}

fn spot_at(mouse: ui::Point, rect: Rect, span: Span, cell: ui::Extent, across: Px) -> CodeSpot {
    let last_row = i32::try_from(span.count().value()).unwrap_or(1) - 1;
    let row = (mouse.vertical - rect.top)
        .ratio(cell.height)
        .clamp(0, last_row);
    CodeSpot {
        line: Line::new(span.start().value() + u32::try_from(row).unwrap_or(0)),
        column: column_at(mouse.horizontal, rect.left, cell.width, across),
    }
}

fn paint_marks(canvas: &mut Canvas<'_>, rect: Rect, cell: Extent, marks: &RowMarks, top: Px) {
    if let Some(color) = marks.background {
        canvas.rect(Rect::new(rect.left, top, rect.width, cell.height), color);
    }
    if let Some(color) = marks.bar {
        canvas.rect(
            Rect::new(rect.left, top, cell.width - SELECTED_BAR, cell.height),
            color,
        );
    }
}

impl Frame<'_> {
    pub(crate) fn code_block(&mut self, model: &Model, block: &CodeBlock<'_>) {
        let Some(source) = model.index.file(block.file) else {
            return;
        };
        let Some(last) = source.text().count().last() else {
            return;
        };
        let end = block.end.min(last);
        let Some(span) = Span::new(block.start, end) else {
            return;
        };
        let grid = self.grids.get(block.file, source, span);
        let size = self.metrics.font;
        let cell = self.metrics.cell;
        let extent = Extent::new(cell.width * grid.columns(), cell.height * grid.rows());
        let pointer = self.ui.pointer();
        let rows: Vec<RowMarks> = span
            .lines()
            .map(|line| RowMarks {
                background: (block.marks.background)(line),
                bar: (block.marks.bar)(line),
            })
            .collect();
        let marks = move |canvas: &mut Canvas<'_>, rect: Rect| {
            let mut top = rect.top;
            for marks in &rows {
                paint_marks(canvas, rect, cell, marks, top);
                top += cell.height;
            }
        };
        let code = move |canvas: &mut Canvas<'_>, rect: Rect| {
            canvas.grid(rect.origin(), size, &grid);
        };
        let Scrolled {
            interaction,
            offset: across,
        } = self.scrolled_code(model, block, extent, Box::new(marks), Box::new(code));
        let spot = interaction
            .rect()
            .filter(|_| interaction.hovered())
            .map(|rect| spot_at(pointer.mouse, rect, span, cell, across));
        if let Some(CodeSpot {
            line,
            column: Some(column),
        }) = spot
        {
            self.point_at_code(model, interaction, block.file, line, column);
        }
    }

    pub(crate) fn code_line(&mut self, model: &Model, shown: &CodeLine) -> Interaction {
        let row = self.row_height();
        let Some(source) = model.index.file(shown.file) else {
            return self.line_row(Box::new(|_: &mut Canvas<'_>, _: Rect| {}), shown.id);
        };
        let grid = self.grids.get(shown.file, source, Span::line(shown.line));
        let font = self.metrics.font;
        let cell = Extent::new(self.cell_width(), row);
        let across = shown.across;
        let marks = RowMarks {
            background: shown.background,
            bar: shown.bar,
        };
        let draw = move |canvas: &mut Canvas<'_>, rect: Rect| {
            paint_marks(canvas, rect, cell, &marks, rect.top);
            canvas.grid(Point::new(rect.left - across, rect.top), font, &grid);
        };
        let interaction = self.line_row(Box::new(draw), shown.id);
        let mouse = self.ui.pointer().mouse.horizontal;
        if let Some(rect) = interaction.rect().filter(|_| interaction.hovered())
            && let Some(column) = column_at(mouse, rect.left, cell.width, across)
        {
            self.point_at_code(model, interaction, shown.file, shown.line, column);
        }
        interaction
    }

    fn point_at_code(
        &mut self,
        model: &Model,
        interaction: Interaction,
        file: FileId,
        line: Line,
        column: Column,
    ) {
        let pointer = self.ui.pointer();
        match keys::code_gesture(interaction, pointer) {
            Some(CodeGesture::Peek) => {
                self.push(Action::Definition(file, line, column, Intent::Peek));
            }
            Some(CodeGesture::Jump) => {
                self.push(Action::Definition(file, line, column, Intent::Jump));
            }
            None if !pointer.down.contains(Button::Left) => {
                self.hover(model, file, line, column);
            }
            None => {}
        }
    }

    fn scrolled_code(
        &mut self,
        model: &Model,
        block: &CodeBlock<'_>,
        extent: Extent,
        marks: Box<dyn Draw>,
        code: Box<dyn Draw>,
    ) -> Scrolled {
        let kept = model.across.get(block.id);
        let mut offset = Point::new(kept, Px::ZERO);
        let across = self.ui.scroll_by_wheel(block.id, &mut offset).horizontal;
        if across != kept {
            self.push(Action::ScrollAcross(block.id, across));
        }
        let layout = match block.width {
            Width::Wide => Layout::row().grow_width(),
            Width::Fit => Layout::row().width(extent.width),
        };
        let interaction = self.ui.open(
            Kind::Custom(marks),
            layout
                .scroll(Point::new(across, Px::ZERO))
                .scroll_axes(ScrollAxes::HORIZONTAL),
            Style::NONE,
            Some(block.id),
        );
        self.ui.leaf(
            Kind::Custom(code),
            Layout::row().width(extent.width).height(extent.height),
            Style::NONE,
            None,
        );
        self.ui.close();
        let interaction = if self.ui.on_scrollbar(block.id) || self.ui.dragging() {
            Interaction::default()
        } else {
            interaction
        };
        Scrolled {
            interaction,
            offset: across,
        }
    }

    pub(crate) fn wheel_across(&self, id: Id, kept: Px, content: Px) -> Px {
        let interaction = self.ui.interaction(id);
        let Some(rect) = interaction.rect() else {
            return kept;
        };
        let most = (content - rect.width).max(Px::ZERO);
        (kept - interaction.wheel().horizontal.truncate())
            .min(most)
            .max(Px::ZERO)
    }

    pub(crate) fn scrolled_across(&self, id: Id) -> bool {
        self.ui.interaction(id).wheel().horizontal.get() != 0.0 || self.ui.dragging()
    }

    pub(crate) fn across_scrollbar(&mut self, id: Id, kept: Px, content: Px) -> Px {
        let mut offset = Point::new(kept, Px::ZERO);
        let across = self.ui.scroll_by_wheel(id, &mut offset).horizontal;
        self.ui.open(
            Kind::None,
            Layout::row()
                .grow_width()
                .slot(ui::Scrollbar::WIDTH)
                .scroll(Point::new(across, Px::ZERO))
                .scroll_axes(ScrollAxes::HORIZONTAL),
            Style::NONE,
            Some(id),
        );
        self.ui.leaf(
            Kind::None,
            Layout::row().width(content).height(ui::Scrollbar::WIDTH),
            Style::NONE,
            None,
        );
        self.ui.close();
        across
    }

    pub(crate) fn hover(&mut self, model: &Model, file: FileId, line: Line, column: Column) {
        match model.hovering(file, line, column) {
            Hovering::Tip(tip) => {
                self.tooltip = Some(TipAt {
                    tip,
                    at: self.ui.pointer().mouse,
                });
            }
            Hovering::Ask { language, probe } => self.push_hover(model, language, probe),
            Hovering::Reopen => self.push(Action::ReopenHover),
            Hovering::Nothing => {}
        }
    }

    pub(crate) fn push_hover(&mut self, model: &Model, language: Language, probe: Probe) {
        if model.queries.asks_now(&probe, model.now) && !model.work.no_server(language) {
            self.overlay.asked += Count::new(1);
        }
        self.push(Action::Hover(language, probe));
    }
}

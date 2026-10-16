use ui::{Button, Count, Icon, Interaction, Kind, Layout, Point, Px, Run, Sides, Style, Wrap};

use crate::dropdown::{Checks, Choice};
use crate::ids::{self, Target};
use crate::theme::{
    ACCENT, BORDER, CHOICE_MARKS, Cells, DROPDOWN_MARK, FIELD, FIELD_PADDING, HOVER, ROW_PADDING,
    SELECTED, TEXT, TOOLTIP_PADDING, WEAK,
};
use crate::widgets::{Container, Frame, text_kind};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Picked<Value> {
    Toggled(Value),
    Outside,
    Kept,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Tally<Value> {
    pub(crate) value: Value,
    pub(crate) count: Count,
}

#[derive(Clone, Copy, Debug)]
struct Literal(&'static str);

const SEPARATOR: Literal = Literal(", ");

fn trigger_runs<Value: Choice>(checks: &Checks<Value>, room: Cells) -> Vec<Run> {
    if checks.every() {
        return vec![Run::new("all", WEAK)];
    }
    let shown = checks.shown();
    if shown.is_empty() {
        return vec![Run::new("none", TEXT)];
    }
    let mut runs = Vec::new();
    for (position, value) in shown.iter().enumerate() {
        if position > 0 {
            runs.push(Run::new(SEPARATOR.0, WEAK));
        }
        runs.push(Run::new(value.word(), value.color()));
    }
    let length: usize = runs.iter().map(|run| run.text.columns()).sum();
    if Cells::of_count(length).get() > room.get() {
        return vec![Run::new(
            format!("{} of {}", shown.len(), Value::VARIANTS.len()),
            TEXT,
        )];
    }
    runs
}

impl Frame<'_> {
    pub(crate) fn dropdown_trigger<Value: Choice>(
        &mut self,
        target: Target,
        checks: &Checks<Value>,
        width: Cells,
    ) -> Interaction {
        let id = target.id();
        let hovered = self.ui.interaction(id).hovered();
        let filtered = !checks.every();
        let room = Cells::new(width.get() - DROPDOWN_MARK.get());
        let background = if hovered {
            HOVER
        } else if filtered {
            SELECTED
        } else {
            FIELD
        };
        let edge = if filtered { ACCENT } else { BORDER };
        let interaction = self.ui.open(
            Kind::None,
            Layout::row()
                .width(width.of(self.cell_width()))
                .padding(FIELD_PADDING)
                .steady(),
            Style::background(background).border(Sides::ALL, edge),
            Some(id),
        );
        let size = self.metrics.font;
        self.ui.leaf(
            text_kind(trigger_runs(checks, room), size, Wrap::Clip),
            Layout::row().grow_width(),
            Style::NONE,
            None,
        );
        self.ui.leaf(
            text_kind(
                vec![Run::new(
                    Icon::Expanded,
                    if filtered { ACCENT } else { WEAK },
                )],
                size,
                Wrap::None,
            ),
            Layout::row(),
            Style::NONE,
            None,
        );
        self.ui.close();
        self.attach_tip(target);
        interaction
    }

    pub(crate) fn dropdown_choices<Value: Choice>(
        &mut self,
        at: Point,
        tallies: &[Tally<Value>],
        checks: &Checks<Value>,
    ) -> Picked<Value> {
        let longest = tallies
            .iter()
            .map(|tally| tally.value.word().as_str().chars().count())
            .max()
            .unwrap_or(0);
        let widest = tallies
            .iter()
            .map(|tally| tally.count.get().to_string().len())
            .max()
            .unwrap_or(0);
        let cells = Cells::of_count(longest + widest) + CHOICE_MARKS;
        let width = cells.of(self.cell_width()) + ROW_PADDING * 2 + TOOLTIP_PADDING * 2;
        let window = self.ui.size();
        let at = Point::new(
            at.horizontal.min(window.width - width).max(Px::ZERO),
            at.vertical,
        );
        self.start(Container::Choices { at, width });
        let mut picked = Picked::Kept;
        for tally in tallies {
            let word = tally.value.word();
            let target = ids::CHOICE.with(&word);
            let id = target.id();
            let hovered = self.ui.interaction(id).hovered();
            let mark = if checks.shows(tally.value) {
                Run::new(Icon::Check, ACCENT)
            } else {
                Run::new(" ", WEAK)
            };
            let runs = vec![
                mark,
                Run::new(" ", WEAK),
                Run::new(format!("{:<longest$}", word.as_str()), tally.value.color()),
                Run::new(format!("  {:>widest$}", tally.count.get()), WEAK),
            ];
            let size = self.metrics.font;
            let row = self.ui.leaf(
                text_kind(runs, size, Wrap::Clip),
                Layout::row().grow_width().padding(ROW_PADDING).steady(),
                Style {
                    background: hovered.then_some(HOVER),
                    ..Style::NONE
                },
                Some(id),
            );
            if row.clicked() {
                picked = Picked::Toggled(tally.value);
            }
            self.attach_tip(target);
        }
        let shown = self.ui.interaction(ids::choices()).rect();
        self.finish();
        let pointer = self.ui.pointer();
        if pointer.pressed.contains(Button::Left)
            && shown.is_some_and(|rect| !rect.contains(pointer.mouse))
        {
            picked = Picked::Outside;
        }
        picked
    }
}

#[cfg(test)]
mod tests;

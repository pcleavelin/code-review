use std::collections::BTreeSet;

use strum::VariantArray;
use ui::{Color, Label};

use crate::app::App;
use crate::columns::InboxColumn;
use crate::ids::{self, Target};

pub(crate) trait Choice: Copy + Ord + VariantArray + 'static {
    fn word(self) -> Label;

    fn color(self) -> Color;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Checks<Value: Ord> {
    hidden: BTreeSet<Value>,
}

impl<Value: Ord> Default for Checks<Value> {
    fn default() -> Self {
        Self {
            hidden: BTreeSet::new(),
        }
    }
}

impl<Value: Choice> Checks<Value> {
    pub(crate) fn shows(&self, value: Value) -> bool {
        !self.hidden.contains(&value)
    }

    pub(crate) fn toggle(&mut self, value: Value) {
        if !self.hidden.remove(&value) {
            self.hidden.insert(value);
        }
    }

    pub(crate) fn shown(&self) -> Vec<Value> {
        Value::VARIANTS
            .iter()
            .copied()
            .filter(|value| self.shows(*value))
            .collect()
    }

    pub(crate) fn every(&self) -> bool {
        self.hidden.is_empty()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Dropdown {
    InboxStatus,
}

impl Dropdown {
    pub(crate) fn column(self) -> InboxColumn {
        match self {
            Self::InboxStatus => InboxColumn::Status,
        }
    }

    pub(crate) fn target(self) -> Target {
        ids::DROPDOWN.with(&self.column().name())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DropdownAct {
    Toggle(Dropdown),
    Close,
}

impl DropdownAct {
    fn open_after(self, open: Option<Dropdown>) -> Option<Dropdown> {
        match self {
            Self::Toggle(dropdown) if open != Some(dropdown) => Some(dropdown),
            Self::Toggle(_) | Self::Close => None,
        }
    }
}

impl App {
    pub(crate) fn dropdown(&mut self, act: DropdownAct) {
        self.model.dropdown = act.open_after(self.model.dropdown);
    }
}

#[cfg(test)]
mod tests;

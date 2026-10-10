use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use domain::{FileId, Index, Line, Settings};
use io_fonts::Fonts;
use strum::VariantArray;
use ui::{Count, Extent, FontSize, Id, Label, Px};

use crate::element_tip::Resting;
use crate::field::Fields;
use crate::nav::Nav;
use crate::palette::Palette;
use crate::panels::Panels;
use crate::peek::{Peek, Queries};
use crate::review::Reviewing;
use crate::settings::SettingsMenu;
use crate::status::{ConsoleLog, Status};
use crate::theme;
use crate::work::WorkState;

#[derive(Clone, Copy, Debug, PartialEq, Eq, VariantArray)]
pub(crate) enum Tab {
    Inbox,
    Review,
    Source,
    Search,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TabName(String);

impl TabName {
    pub(crate) fn new(name: &str) -> Self {
        Self(name.to_owned())
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Literal(&'static str);

impl Literal {
    const fn as_str(self) -> &'static str {
        self.0
    }
}

impl Tab {
    const fn name(self) -> Literal {
        Literal(match self {
            Self::Inbox => "inbox",
            Self::Review => "review",
            Self::Source => "source",
            Self::Search => "search",
        })
    }

    pub(crate) fn from_name(name: &TabName) -> Self {
        Self::VARIANTS
            .iter()
            .copied()
            .find(|tab| tab.name().as_str() == name.as_str())
            .unwrap_or(Self::Inbox)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct LineSelection {
    pub(crate) from: Line,
    pub(crate) to: Line,
}

impl LineSelection {
    pub(crate) const fn one(line: Line) -> Self {
        Self {
            from: line,
            to: line,
        }
    }

    pub(crate) fn low(self) -> Line {
        self.from.min(self.to)
    }

    pub(crate) fn high(self) -> Line {
        self.from.max(self.to)
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Scrolls(BTreeMap<Id, Px>);

impl Scrolls {
    pub(crate) fn get(&self, id: Id) -> Px {
        self.0.get(&id).copied().unwrap_or(Px::ZERO)
    }

    pub(crate) fn set(&mut self, id: Id, offset: Px) {
        self.0.insert(id, offset);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Hit {
    pub(crate) file: FileId,
    pub(crate) line: Line,
}

pub(crate) const HIT_LIMIT: Count = Count::new(5000);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HitsShown {
    All,
    First,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Metrics {
    pub(crate) font: FontSize,
    pub(crate) cell: Extent,
}

impl Metrics {
    pub(crate) const fn cell_width(self) -> Px {
        self.cell.width
    }

    pub(crate) const fn row_height(self) -> Px {
        self.cell.height
    }
}

pub(crate) struct Model {
    pub(crate) index: Index,
    pub(crate) review: Reviewing,
    pub(crate) nav: Nav,
    pub(crate) directories: BTreeSet<Label>,
    pub(crate) peek: Option<Peek>,
    pub(crate) queries: Queries,
    pub(crate) panels: Panels,
    pub(crate) scrolls: Scrolls,
    pub(crate) across: Scrolls,
    pub(crate) tip_shown: Option<Label>,
    pub(crate) resting: Resting,
    pub(crate) fields: Fields,
    pub(crate) palette: Option<Palette>,
    pub(crate) line_grab: Option<Line>,
    pub(crate) hits: Vec<Hit>,
    pub(crate) hits_shown: HitsShown,
    pub(crate) console: ConsoleLog,
    pub(crate) console_bottom: Count,
    pub(crate) status: Status,
    pub(crate) work: WorkState,
    pub(crate) metrics: Metrics,
    pub(crate) now: Duration,
    pub(crate) settings: Settings,
    pub(crate) fonts: Option<Fonts>,
    pub(crate) settings_menu: Option<SettingsMenu>,
}

impl Model {
    pub(crate) fn new(index: Index, review: Reviewing) -> Self {
        Self {
            index,
            review,
            nav: Nav::default(),
            directories: BTreeSet::new(),
            peek: None,
            queries: Queries::default(),
            panels: Panels::default(),
            scrolls: Scrolls::default(),
            across: Scrolls::default(),
            tip_shown: None,
            resting: Resting::default(),
            fields: Fields::default(),
            palette: None,
            line_grab: None,
            hits: Vec::new(),
            hits_shown: HitsShown::All,
            console: ConsoleLog::default(),
            console_bottom: Count::ZERO,
            status: Status::Nothing,
            work: WorkState::default(),
            metrics: Metrics {
                font: theme::start_font(),
                cell: theme::START_CELL,
            },
            now: Duration::ZERO,
            settings: Settings::default(),
            fonts: None,
            settings_menu: None,
        }
    }
}

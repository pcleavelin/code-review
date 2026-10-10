use std::cmp::Reverse;

use domain::{FileId, Line, SymbolId};
use strum::VariantArray;
use ui::{Count, Label};

use crate::action::Action;
use crate::field::{FieldText, Which};
use crate::keys::{self, Chord, PaletteKey, Walk};
use crate::model::Model;
use crate::panels::{Direction, View};
use crate::review::{Inbox, ReviewAct};
use crate::settings::SettingsAct;

pub(crate) const PALETTE_LIMIT: Count = Count::new(50);
pub(crate) const PALETTE_ROWS: Count = Count::new(16);
const ACTIONS_ONLY: Prefix = Prefix('>');

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PaletteAction {
    Toggle,
    Close,
    Key(PaletteKey),
    Run(Count),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum EntryKind {
    Action,
    Pull,
    Change,
    View,
    Symbol,
    File,
}

impl EntryKind {
    pub(crate) fn tag(self) -> Label {
        Label::new(match self {
            Self::Action => "action",
            Self::Pull => "pull",
            Self::Change => "changed",
            Self::View => "view",
            Self::Symbol => "symbol",
            Self::File => "file",
        })
    }

    const fn boost(self) -> Score {
        match self {
            Self::Action | Self::Pull | Self::Change | Self::View => Score::MAPPED,
            Self::Symbol | Self::File => Score::ZERO,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PaletteCommand {
    Back,
    Forward,
    Search,
    Split(Direction),
    Settings,
    Inbox,
    RefreshPulls,
    ToggleSeen,
    NextUnseen,
    NextHunk,
    Comment,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Words(&'static str);

struct Offered {
    label: Words,
    detail: Words,
    command: PaletteCommand,
    chord: Option<Chord>,
}

const OFFERED: [Offered; 12] = [
    Offered {
        label: Words("go back"),
        detail: Words("go back to the previous place"),
        command: PaletteCommand::Back,
        chord: Some(keys::GO_BACK),
    },
    Offered {
        label: Words("go forward"),
        detail: Words("go forward again"),
        command: PaletteCommand::Forward,
        chord: Some(keys::GO_FORWARD),
    },
    Offered {
        label: Words("search every file"),
        detail: Words("search the tree for a regex in the Search tab"),
        command: PaletteCommand::Search,
        chord: None,
    },
    Offered {
        label: Words("settings"),
        detail: Words("the theme, the font and its size"),
        command: PaletteCommand::Settings,
        chord: Some(keys::SETTINGS),
    },
    Offered {
        label: Words("split panel right"),
        detail: Words("split the panel showing the current view in two"),
        command: PaletteCommand::Split(Direction::Right),
        chord: None,
    },
    Offered {
        label: Words("split panel down"),
        detail: Words("split the panel showing the current view in two"),
        command: PaletteCommand::Split(Direction::Down),
        chord: None,
    },
    Offered {
        label: Words("inbox"),
        detail: Words("every open pull request of the pinned repositories"),
        command: PaletteCommand::Inbox,
        chord: None,
    },
    Offered {
        label: Words("refresh pull requests"),
        detail: Words("ask every pinned repository's forge again"),
        command: PaletteCommand::RefreshPulls,
        chord: None,
    },
    Offered {
        label: Words("toggle seen"),
        detail: Words("mark the shown file seen, or unseen again"),
        command: PaletteCommand::ToggleSeen,
        chord: Some(keys::TOGGLE_SEEN),
    },
    Offered {
        label: Words("next unseen file"),
        detail: Words("show the next changed file not seen yet"),
        command: PaletteCommand::NextUnseen,
        chord: Some(keys::NEXT_UNSEEN),
    },
    Offered {
        label: Words("next change"),
        detail: Words("go to the next change in this file, then the next file"),
        command: PaletteCommand::NextHunk,
        chord: Some(keys::NEXT_HUNK),
    },
    Offered {
        label: Words("comment on the selected line"),
        detail: Words("draft a comment on the selected line"),
        command: PaletteCommand::Comment,
        chord: Some(keys::COMMENT),
    },
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Goal {
    Run(PaletteCommand),
    Pull(Count),
    Change(Count),
    Symbol(SymbolId),
    File(FileId),
    View(View),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
struct Score(i32);

#[derive(Clone, Debug)]
pub(crate) struct Entry {
    pub(crate) goal: Goal,
    pub(crate) kind: EntryKind,
    pub(crate) name: Label,
    pub(crate) detail: Label,
    pub(crate) chord: Option<Label>,
    score: Score,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Palette {
    pub(crate) entries: Vec<Entry>,
    pub(crate) selected: Count,
    pub(crate) top: Count,
    ranked: Option<FieldText>,
}

impl Palette {
    pub(crate) fn walk(&mut self, walk: Walk) {
        let last = self.entries.len().saturating_sub(1);
        let at = self.selected.get();
        self.selected = Count::new(match walk {
            Walk::Down if at >= last => 0,
            Walk::Down => at + 1,
            Walk::Up if at == 0 => last,
            Walk::Up => at - 1,
        });
        self.keep_in_view();
    }

    fn keep_in_view(&mut self) {
        let (selected, top, rows) = (self.selected.get(), self.top.get(), PALETTE_ROWS.get());
        if selected < top {
            self.top = self.selected;
        } else if selected >= top + rows {
            self.top = Count::new(selected + 1 - rows);
        }
    }

    pub(crate) fn goal(&self, row: Count) -> Option<Goal> {
        self.entries.get(row.get()).map(|entry| entry.goal)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Prefix(char);

impl Prefix {
    const fn get(self) -> char {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Narrowing {
    Everything,
    ActionsOnly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Leap {
    Next,
    WordStart,
}

impl Score {
    const ZERO: Self = Self(0);
    const MAPPED: Self = Self(50);
    const CONTIGUOUS: Self = Self(1000);
    const AT_WORD_START: Self = Self(300);
    const AT_START: Self = Self(100);
    const WHOLE: Self = Self(400);
    const LETTER: Self = Self(10);
    const RUN: Self = Self(15);
    const LETTER_AT_WORD_START: Self = Self(20);
    const LONGEST_GAP: usize = 10;

    const fn plus(self, other: Self) -> Self {
        Self(self.0 + other.0)
    }

    fn minus_count(self, count: usize) -> Self {
        Self(self.0 - i32::try_from(count).unwrap_or_default())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Spelling(Vec<char>);

impl Spelling {
    fn of(text: &str) -> Self {
        Self(text.chars().collect())
    }

    fn lowered(&self) -> Self {
        Self(
            self.0
                .iter()
                .map(|character| character.to_lowercase().next().unwrap_or(*character))
                .collect(),
        )
    }

    fn len(&self) -> usize {
        self.0.len()
    }

    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn word_start(&self, at: usize) -> bool {
        let Some(previous) = at.checked_sub(1).and_then(|before| self.0.get(before)) else {
            return true;
        };
        let current = self.0.get(at).copied().unwrap_or(' ');
        !previous.is_alphanumeric() || (previous.is_lowercase() && current.is_uppercase())
    }

    fn found_at(&self, from: usize, character: char) -> Option<usize> {
        (from..self.len()).find(|at| self.0.get(*at) == Some(&character))
    }

    fn score(&self, wanted: &Self) -> Option<Score> {
        if wanted.is_empty() {
            return Some(Score::default());
        }
        let lower = self.lowered();
        if wanted.len() > lower.len() {
            return None;
        }
        let contiguous = (0..=lower.len() - wanted.len())
            .filter(|start| lower.0.get(*start..*start + wanted.len()) == Some(wanted.0.as_slice()))
            .map(|start| {
                let mut score = Score::CONTIGUOUS;
                if self.word_start(start) {
                    score = score.plus(Score::AT_WORD_START);
                }
                if start == 0 {
                    score = score.plus(Score::AT_START);
                }
                if wanted.len() == lower.len() {
                    score = score.plus(Score::WHOLE);
                }
                score
            })
            .max();
        contiguous
            .or_else(|| {
                [Leap::Next, Leap::WordStart]
                    .into_iter()
                    .filter_map(|leap| self.subsequence(wanted, &lower, leap))
                    .max()
            })
            .map(|best| best.minus_count(lower.len()))
    }

    fn subsequence(&self, wanted: &Self, lower: &Self, leap: Leap) -> Option<Score> {
        let mut from = 0;
        let mut previous: Option<usize> = None;
        let mut score = Score::default();
        for character in &wanted.0 {
            let next = lower.found_at(from, *character)?;
            let adjacent = previous.is_some_and(|before| before + 1 == next);
            let at = if adjacent || leap == Leap::Next {
                next
            } else {
                (next..lower.len())
                    .find(|at| lower.0.get(*at) == Some(character) && self.word_start(*at))
                    .unwrap_or(next)
            };
            score = score.plus(Score::LETTER);
            if previous.is_some_and(|before| before + 1 == at) {
                score = score.plus(Score::RUN);
            }
            if self.word_start(at) {
                score = score.plus(Score::LETTER_AT_WORD_START);
            }
            score = score.minus_count((at - from).min(Score::LONGEST_GAP));
            previous = Some(at);
            from = at + 1;
        }
        Some(score)
    }
}

struct Query {
    wanted: Spelling,
    narrowing: Narrowing,
}

impl Query {
    fn new(text: &FieldText) -> Self {
        let trimmed = text.as_str().trim_start();
        let prefix = ACTIONS_ONLY.get();
        let (narrowing, rest) = match trimmed.strip_prefix(prefix) {
            Some(rest) => (Narrowing::ActionsOnly, rest.trim_start_matches(prefix)),
            None => (Narrowing::Everything, trimmed),
        };
        Self {
            wanted: Spelling::of(rest.trim()).lowered(),
            narrowing,
        }
    }
}

struct Shown {
    name: Label,
    detail: Label,
    chord: Option<Label>,
}

impl Shown {
    const fn plain(name: Label, detail: Label) -> Self {
        Self {
            name,
            detail,
            chord: None,
        }
    }
}

struct Gathered {
    query: Query,
    entries: Vec<Entry>,
}

impl Gathered {
    fn offer(
        &mut self,
        kind: EntryKind,
        goal: Goal,
        against: &Spelling,
        shown: impl FnOnce() -> Shown,
    ) {
        if self.query.narrowing == Narrowing::ActionsOnly && kind != EntryKind::Action {
            return;
        }
        let Some(score) = against.score(&self.query.wanted) else {
            return;
        };
        let score = score.plus(kind.boost());
        let Shown {
            name,
            detail,
            chord,
        } = shown();
        self.entries.push(Entry {
            goal,
            kind,
            name,
            detail,
            chord,
            score,
        });
    }

    fn actions(&mut self) {
        for offered in &OFFERED {
            let against = Spelling::of(offered.label.0);
            self.offer(
                EntryKind::Action,
                Goal::Run(offered.command),
                &against,
                || Shown {
                    name: Label::new(offered.label.0),
                    detail: Label::new(offered.detail.0),
                    chord: offered.chord.map(Chord::label),
                },
            );
        }
    }

    fn review(&mut self, model: &Model) {
        if let Inbox::Listed { entries, .. } = &model.review.inbox {
            for (position, entry) in entries.iter().enumerate() {
                let pull = &entry.pull;
                let name = format!("{}#{} {}", entry.repo, pull.number, pull.title);
                self.offer(
                    EntryKind::Pull,
                    Goal::Pull(Count::new(position)),
                    &Spelling::of(&name),
                    || {
                        Shown::plain(
                            Label::new(name.as_str()),
                            Label::new(format!("{} by {}", pull.head, pull.author)),
                        )
                    },
                );
            }
        }
        let Some(opened) = model.review.opened() else {
            return;
        };
        for (position, change) in opened.changes.iter().enumerate() {
            let path = change.path().as_str();
            self.offer(
                EntryKind::Change,
                Goal::Change(Count::new(position)),
                &Spelling::of(path),
                || {
                    Shown::plain(
                        Label::new(path),
                        Label::new(format!("+{} -{}", change.added(), change.removed())),
                    )
                },
            );
        }
    }

    fn views(&mut self) {
        for view in View::VARIANTS {
            let against = Spelling::of(view.name().as_str());
            self.offer(EntryKind::View, Goal::View(*view), &against, || {
                Shown::plain(
                    Label::new(view.name().as_str()),
                    Label::new(format!("show the {} tab", view.name())),
                )
            });
        }
    }

    fn index(&mut self, model: &Model) {
        for symbol_id in model.index.symbol_ids() {
            let (Some(symbol), Some(file)) = (
                model.index.symbol(symbol_id),
                model.index.file(symbol_id.file()),
            ) else {
                continue;
            };
            let against = Spelling::of(symbol.name().as_str());
            self.offer(EntryKind::Symbol, Goal::Symbol(symbol_id), &against, || {
                Shown::plain(
                    Label::new(symbol.name().as_str()),
                    Label::new(format!(
                        "{}:{}  {}",
                        file.path().as_str(),
                        symbol.span().start().number(),
                        symbol.kind().as_str()
                    )),
                )
            });
        }
        for entry in model.index.file_entries() {
            let path = entry.item.path().as_str();
            self.offer(
                EntryKind::File,
                Goal::File(entry.id),
                &Spelling::of(path),
                || {
                    Shown::plain(
                        Label::new(path),
                        Label::new(format!("{} symbols", entry.item.symbols().count())),
                    )
                },
            );
        }
    }
}

fn gather(model: &Model, text: &FieldText) -> Vec<Entry> {
    let mut gathered = Gathered {
        query: Query::new(text),
        entries: Vec::new(),
    };
    gathered.actions();
    gathered.review(model);
    gathered.views();
    if !gathered.query.wanted.is_empty() && gathered.query.narrowing == Narrowing::Everything {
        gathered.index(model);
    }
    let mut entries = gathered.entries;
    entries.sort_by_key(|entry| (Reverse(entry.score), entry.kind));
    entries.truncate(PALETTE_LIMIT.get());
    entries
}

impl Model {
    pub(crate) fn refresh_palette(&mut self) {
        let Some(shown) = &self.palette else {
            return;
        };
        let text = self.fields.get(Which::Palette).text().clone();
        if shown.ranked.as_ref() == Some(&text) {
            return;
        }
        let entries = gather(self, &text);
        let Some(palette) = &mut self.palette else {
            return;
        };
        palette.entries = entries;
        palette.ranked = Some(text);
        palette.selected = Count::ZERO;
        palette.top = Count::ZERO;
    }

    pub(crate) fn palette_actions(&self, goal: Goal) -> Vec<Action> {
        match goal {
            Goal::Run(command) => match command {
                PaletteCommand::Back => vec![Action::Back],
                PaletteCommand::Forward => vec![Action::Forward],
                PaletteCommand::Search => vec![Action::ShowView(View::Search)],
                PaletteCommand::Settings => vec![Action::Settings(SettingsAct::Toggle)],
                PaletteCommand::Inbox => vec![Action::ShowView(View::Inbox)],
                PaletteCommand::RefreshPulls => vec![Action::Review(ReviewAct::RefreshPulls)],
                PaletteCommand::ToggleSeen => vec![Action::Review(ReviewAct::ToggleShownSeen)],
                PaletteCommand::NextUnseen => vec![Action::Review(ReviewAct::NextUnseen)],
                PaletteCommand::NextHunk => vec![Action::Review(ReviewAct::NextHunk)],
                PaletteCommand::Comment => vec![Action::Review(ReviewAct::StartDraftHere)],
                PaletteCommand::Split(direction) => {
                    let shown = View::of_tab(self.nav.tab());
                    self.panels
                        .holder(shown)
                        .or_else(|| self.panels.panels().first().map(|panel| panel.id()))
                        .map(|panel| Action::SplitPanel(panel, direction))
                        .into_iter()
                        .collect()
                }
            },
            Goal::Pull(position) => match &self.review.inbox {
                Inbox::Listed { entries, .. } => entries
                    .get(position.get())
                    .map(|entry| {
                        Action::Review(ReviewAct::OpenPull(entry.repo.clone(), entry.pull.number))
                    })
                    .into_iter()
                    .collect(),
                Inbox::Asking => Vec::new(),
            },
            Goal::Change(position) => self
                .review
                .opened()
                .and_then(|opened| opened.changes.get(position.get()))
                .map(|change| Action::Review(ReviewAct::OpenChange(change.path().clone())))
                .into_iter()
                .collect(),
            Goal::Symbol(symbol) => vec![Action::Jump(symbol)],
            Goal::File(file) => vec![Action::GoTo(file, Line::new(0))],
            Goal::View(view) => vec![Action::ShowView(view)],
        }
    }
}

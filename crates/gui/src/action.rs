use domain::{Column, FileId, Language, Line, RelativePath, SymbolId};
use platform::ClipboardRequest;
use ui::{Count, Id, Label, Point, Px, Typed};

use crate::app::App;
use crate::columns::GridAct;
use crate::element_tip::Pressing;
use crate::field::{AfterSubmit, Edit, FieldAct, FieldText, Which};
use crate::ids;
use crate::keys::{LineGesture, PaletteKey};
use crate::login::{Browser, LoginAct};
use crate::model::{HIT_LIMIT, Hit, HitsShown, Tab};
use crate::nav::{Going, Ticket};
use crate::palette::{Palette, PaletteAction};
use crate::panels::{BranchId, Direction, DropTarget, Ratio, View};
use crate::peek::{HoverStep, Intent, Peek, Probe, Probing, WantedDefinition};
use crate::review::ReviewAct;
use crate::settings::SettingsAct;
use crate::status::Status;
use crate::work::Request;

pub(crate) enum Action {
    Focus(SymbolId),
    Jump(SymbolId),
    GoTo(FileId, Line),
    Definition(FileId, Line, Column, Intent),
    SelectLine(RelativePath, Line, LineGesture),
    ClearLineSelection,
    ClosePeek,
    ToggleDirectory(Label),
    ShowView(View),
    Back,
    Forward,
    Scroll(Id, Px),
    ScrollAcross(Id, Px),
    ScrolledToLine(Ticket),
    ConsoleScrolled,
    TipShown(Option<Label>),
    Rest(Option<Id>, Pressing),
    ReopenHover,
    Hover(Language, Probe),
    AskReferences(Language, Probe),
    FocusField(Which, Id),
    Grab(View, Point),
    Resize(BranchId, Ratio),
    DragView(Point),
    Release(Option<DropTarget>),
    SplitPanel(BranchId, Direction),
    ClosePanel(BranchId),
    TogglePicker(BranchId),
    Pick(BranchId, View),
    CloseView(View),
    ClosePicker,
    Palette(PaletteAction),
    Type(Which, Vec<Edit>, Typed),
    Field(Which, FieldAct),
    Settings(SettingsAct),
    Review(ReviewAct),
    Login(LoginAct),
    Grid(GridAct),
}

impl App {
    pub(crate) fn apply(&mut self, action: Action) {
        let model = &mut self.model;
        match action {
            Action::Focus(symbol) => model.go_to_symbol(symbol),
            Action::Jump(symbol) => model.jumped_to_symbol(symbol),
            Action::GoTo(file, line) => model.open_line(file, line),
            Action::Definition(file, line, column, intent) => {
                self.definition(file, line, column, intent);
            }
            Action::SelectLine(path, line, gesture) => model.select_line(&path, line, gesture),
            Action::ClearLineSelection => model.clear_line_selection(),
            Action::ClosePeek => model.peek = None,
            Action::ToggleDirectory(directory) => {
                if !model.directories.remove(&directory) {
                    model.directories.insert(directory);
                }
            }
            Action::ShowView(view) => model.show_view(view),
            Action::Back => model.go_at_frame_end(Going::Back),
            Action::Forward => model.go_at_frame_end(Going::Forward),
            Action::Scroll(id, offset) => model.scrolls.set(id, offset),
            Action::ScrollAcross(id, offset) => model.across.set(id, offset),
            Action::Grid(act) => {
                let cell = model.metrics.cell.width;
                model.inbox_grid.act(act, cell);
            }
            Action::ScrolledToLine(ticket) => model.scrolled_to_line(ticket),
            Action::ConsoleScrolled => {
                model.console_bottom = Count::new(model.console_bottom.get().saturating_sub(1));
            }
            Action::TipShown(tip) => model.tip_shown = tip,
            Action::Rest(on, pressing) => model.resting.rest(on, pressing, model.now),
            Action::ReopenHover => model.queries.reopen_hover(),
            Action::Hover(language, probe) => self.hover(language, &probe),
            Action::AskReferences(language, probe) => {
                if self.ask(language, Request::References(probe.clone())) {
                    self.model.queries.references_sent(probe);
                }
            }
            Action::FocusField(which, id) => model.fields.focus_at(which, id),
            Action::Grab(view, at) => model.panels.take_hold(view, at),
            Action::Resize(split, ratio) => model.panels.resize(split, ratio),
            Action::DragView(at) => model.panels.drag_to(at),
            Action::Release(target) => {
                if let Some(grab) = model.panels.release()
                    && let Some(target) = target
                {
                    model.panels.drop_view(grab.view, target);
                }
            }
            Action::SplitPanel(panel, direction) => {
                model.panels.split_panel(panel, direction);
                model.fields.start_empty(Which::ViewSearch);
            }
            Action::ClosePanel(panel) => model.panels.close(panel),
            Action::TogglePicker(panel) => {
                model.panels.toggle_picker(panel);
                if model.panels.picker().is_some() {
                    model.fields.start_empty(Which::ViewSearch);
                } else {
                    model.fields.release(Which::ViewSearch);
                }
            }
            Action::Pick(panel, view) => model.pick_view(panel, view),
            Action::CloseView(view) => model.panels.close_view(view),
            Action::ClosePicker => {
                model.panels.close_picker();
                model.fields.release(Which::ViewSearch);
            }
            Action::Palette(action) => self.palette(action),
            Action::Type(which, edits, typed) => self.typed(which, &edits, &typed),
            Action::Field(which, act) => model.fields.act(which, act),
            Action::Settings(act) => self.change_setting(act),
            Action::Review(act) => self.review(act),
            Action::Login(act) => self.login(act),
        }
    }

    fn palette(&mut self, action: PaletteAction) {
        let model = &mut self.model;
        match action {
            PaletteAction::Toggle => {
                if model.palette.take().is_some() {
                    model.fields.release(Which::Palette);
                } else {
                    model.palette = Some(Palette::default());
                    model.fields.start_empty(Which::Palette);
                    model.refresh_palette();
                }
            }
            PaletteAction::Close => {
                model.palette = None;
                model.fields.release(Which::Palette);
            }
            PaletteAction::Key(PaletteKey::Move(walk)) => {
                if let Some(palette) = model.palette.as_mut() {
                    palette.walk(walk);
                }
            }
            PaletteAction::Key(PaletteKey::Run) => {
                let row = model.palette.as_ref().map(|palette| palette.selected);
                if let Some(row) = row {
                    self.run_palette(row);
                }
            }
            PaletteAction::Key(PaletteKey::Close) => self.palette(PaletteAction::Close),
            PaletteAction::Run(row) => self.run_palette(row),
        }
    }

    fn run_palette(&mut self, row: Count) {
        let goal = self
            .model
            .palette
            .as_ref()
            .and_then(|palette| palette.goal(row));
        self.palette(PaletteAction::Close);
        let Some(goal) = goal else {
            return;
        };
        for action in self.model.palette_actions(goal) {
            self.apply(action);
        }
    }

    fn hover(&mut self, language: Language, probe: &Probe) {
        let now = self.model.now;
        if matches!(self.model.queries.hover_step(probe, now), HoverStep::Ask)
            && self.ask(language, Request::Hover(probe.clone()))
        {
            self.model.queries.hover_sent(probe);
        }
    }

    fn definition(&mut self, file: FileId, line: Line, column: Column, intent: Intent) {
        if intent == Intent::Peek {
            self.model.close_hover(file, line, column);
        }
        let Some(word) = self.model.word_at(file, line, column) else {
            return;
        };
        match self.model.probing(file, line, word.start) {
            Probing::Server { language, probe } => {
                if self.ask(language, Request::Definition(probe.clone())) {
                    self.model
                        .queries
                        .definition_sent(WantedDefinition { probe, intent });
                    self.model.status = Status::LookingUp(word.text);
                }
            }
            Probing::Resolver => match self.model.symbol_at(file, line, column) {
                Some(symbol) => self.land(Peek::Symbol(symbol), intent),
                None => self.model.status = Status::NoDefinitionOf(word.text),
            },
            Probing::Nothing => {}
        }
    }

    fn typed(&mut self, which: Which, edits: &[Edit], typed: &Typed) {
        let after = match which {
            Which::Search
            | Which::SymbolFilter
            | Which::Palette
            | Which::Draft
            | Which::Summary
            | Which::Inbox
            | Which::Pin
            | Which::Token
            | Which::JiraSite
            | Which::JiraEmail
            | Which::Column(_) => AfterSubmit::Keep,
            Which::Command | Which::ViewSearch => AfterSubmit::Clear,
        };
        let escaped = which == Which::Draft
            && self.model.fields.focused() == Some(which)
            && edits.contains(&Edit::Escape);
        let before = self.model.fields.get(which).text().clone();
        let handled = self.model.fields.handle(which, edits, typed, after);
        if handled.clipboard != ClipboardRequest::Keep {
            self.clipboard = handled.clipboard;
        }
        let entered = handled.submitted;
        if which == Which::SymbolFilter && *self.model.fields.get(which).text() != before {
            self.model.scrolls.set(ids::symbols(), Px::ZERO);
        }
        if escaped {
            self.review(ReviewAct::CancelDraft);
            return;
        }
        let Some(line) = entered else {
            return;
        };
        match which {
            Which::Command => self.run_command(&line),
            Which::Search => self.search(),
            Which::ViewSearch => self.pick_first(&line),
            Which::Draft => self.review(ReviewAct::SaveDraft),
            Which::Pin => self.review(ReviewAct::Pin),
            Which::Token => self.login(LoginAct::Save),
            Which::JiraSite => self.model.fields.focus(Which::JiraEmail),
            Which::JiraEmail => self.model.fields.focus(Which::Token),
            Which::SymbolFilter
            | Which::Palette
            | Which::Summary
            | Which::Inbox
            | Which::Column(_) => {}
        }
    }

    fn search(&mut self) {
        let model = &mut self.model;
        let pattern = model.fields.get(Which::Search).text().label();
        let expression = match regex::Regex::new(pattern.as_str()) {
            Ok(expression) => expression,
            Err(error) => {
                let text = error.to_string();
                model.status =
                    Status::RegexRefused(Label::new(text.lines().last().unwrap_or("").trim()));
                return;
            }
        };
        model.hits = model
            .index
            .file_entries()
            .flat_map(|entry| {
                entry
                    .item
                    .text()
                    .all()
                    .iter()
                    .enumerate()
                    .filter(|pair| expression.is_match(pair.1.as_str()))
                    .map(move |pair| Hit {
                        file: entry.id,
                        line: Line::new(u32::try_from(pair.0).unwrap_or(0)),
                    })
                    .collect::<Vec<_>>()
            })
            .take(HIT_LIMIT.get() + 1)
            .collect();
        model.hits_shown = if model.hits.len() > HIT_LIMIT.get() {
            model.hits.truncate(HIT_LIMIT.get());
            HitsShown::First
        } else {
            HitsShown::All
        };
        model.status = Status::Hits {
            count: Count::new(model.hits.len()),
            pattern,
            shown: model.hits_shown,
        };
        model.set_tab(Tab::Search);
    }

    fn pick_first(&mut self, line: &FieldText) {
        let model = &mut self.model;
        let Some(panel) = model.panels.picker() else {
            return;
        };
        if let Some(view) = View::matching(&line.label()).first() {
            model.pick_view(panel, *view);
        }
    }

    fn run_command(&mut self, line: &FieldText) {
        let line = line.as_str();
        let model = &mut self.model;
        if line.trim() == ConsoleCommand::Clear.name().as_str() {
            model.console.clear();
            return;
        }
        model.console.command(line);
        model.console_bottom = Count::new(2);
        let invocation = match cli::Invocation::parse(&cli::CommandLine::new(line).arguments()) {
            Ok(invocation) => invocation,
            Err(failure) => {
                let text = failure.text();
                model.status =
                    Status::CommandRejected(Label::new(text.as_str().lines().next().unwrap_or("")));
                model.console.rejected(text.as_str());
                return;
            }
        };
        let mut output = cli::Output::new();
        let result = cli::exec(&mut model.review.desk, invocation, &mut output);
        model.console.append(output.as_str());
        match result {
            Err(failure) => {
                let text = failure.to_string();
                model.status = Status::CommandFailed(Label::new(text.as_str()));
                model.console.failed(&text);
            }
            Ok(cli::Touched::Pins | cli::Touched::Keychain) => self.list_pulls(),
            Ok(cli::Touched::Dropped(name, number)) => self.dropped(&name, number),
            Ok(cli::Touched::LoginStarted(started)) => {
                if matches!(started, session::StartedLogin::Paste { .. }) {
                    self.model.console.append(
                        "in this window, paste the token in the Inbox's token field instead; the Console keeps every line typed in it\n",
                    );
                }
                self.carry_login(started, Browser::Opened);
            }
            Ok(cli::Touched::JiraLoginStarted(site)) => {
                self.model.console.append(
                    "in this window, fill in the Inbox's Jira login form instead; the Console keeps every line typed in it\n",
                );
                self.login(LoginAct::StartJira(Some(site)));
            }
            Ok(cli::Touched::Nothing | cli::Touched::Review(_)) => {}
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Literal(&'static str);

impl Literal {
    const fn as_str(self) -> &'static str {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ConsoleCommand {
    Clear,
}

impl ConsoleCommand {
    const fn name(self) -> Literal {
        Literal(match self {
            Self::Clear => "clear",
        })
    }
}

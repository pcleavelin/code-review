use std::mem;

use domain::{FileId, Line, RelativePath, SymbolId, SymbolKey};
use ui::{Count, Px};

use crate::field::Which;
use crate::ids;
use crate::keys::LineGesture;
use crate::model::{LineSelection, Model, Tab};
use crate::panels::{BranchId, View};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Ticket(u64);

impl Ticket {
    pub(crate) fn next(self) -> Self {
        Self(self.0.wrapping_add(1))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Aim {
    Line,
    Editor,
    Top,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LineRequest {
    pub(crate) line: Line,
    pub(crate) ticket: Ticket,
    pub(crate) aim: Aim,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Move {
    Select,
    Jump,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Going {
    Back,
    Forward,
}

#[derive(Clone, Debug, PartialEq)]
struct Viewport {
    tab: Tab,
    lines: Option<LineSelection>,
    source: Px,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Place {
    focus: Option<SymbolKey>,
    file: Option<RelativePath>,
    view: Viewport,
}

impl Place {
    fn moved_from(&self, other: &Self, how: Move) -> bool {
        self.focus != other.focus
            || self.file != other.file
            || self.view.tab != other.view.tab
            || (how == Move::Jump && self.view.lines != other.view.lines)
    }
}

#[derive(Clone, Debug, Default)]
struct History {
    back: Vec<Place>,
    forward: Vec<Place>,
    last: Option<Place>,
}

impl History {
    const LONGEST: Count = Count::new(200);
}

#[derive(Clone, Debug)]
pub(crate) struct Nav {
    tab: Tab,
    focus: Option<SymbolId>,
    file: Option<FileId>,
    gone: Option<RelativePath>,
    lines: Option<LineSelection>,
    scroll_to: Option<LineRequest>,
    ticket: Ticket,
    asked: Ticket,
    moving: Move,
    going: Option<Going>,
    history: History,
}

impl Default for Nav {
    fn default() -> Self {
        Self {
            tab: Tab::Inbox,
            focus: None,
            file: None,
            gone: None,
            lines: None,
            scroll_to: None,
            ticket: Ticket(0),
            asked: Ticket(0),
            moving: Move::Select,
            going: None,
            history: History::default(),
        }
    }
}

impl Nav {
    pub(crate) const fn tab(&self) -> Tab {
        self.tab
    }

    pub(crate) const fn focus(&self) -> Option<SymbolId> {
        self.focus
    }

    pub(crate) const fn file(&self) -> Option<FileId> {
        self.file
    }

    pub(crate) const fn gone(&self) -> Option<&RelativePath> {
        self.gone.as_ref()
    }

    pub(crate) const fn lines(&self) -> Option<LineSelection> {
        self.lines
    }

    pub(crate) const fn scroll_to(&self) -> Option<LineRequest> {
        self.scroll_to
    }

    pub(crate) fn can_go_back(&self) -> bool {
        !self.history.back.is_empty()
    }

    pub(crate) fn can_go_forward(&self) -> bool {
        !self.history.forward.is_empty()
    }

    fn ticket(&mut self) -> Ticket {
        self.ticket = self.ticket.next();
        self.ticket
    }

    fn show(&mut self, tab: Tab) {
        self.tab = tab;
        self.asked = self.ticket();
    }

    fn scroll_to_top(&mut self) {
        let ticket = self.ticket();
        self.scroll_to = Some(LineRequest {
            line: Line::new(0),
            ticket,
            aim: Aim::Top,
        });
    }

    fn scroll_to_line(&mut self, line: Line) {
        let ticket = self.ticket();
        self.scroll_to = Some(LineRequest {
            line,
            ticket,
            aim: Aim::Line,
        });
    }
}

impl Model {
    fn here(&self) -> Place {
        let nav = &self.nav;
        Place {
            focus: nav.focus.and_then(|symbol| self.index.symbol_key(symbol)),
            file: nav
                .file
                .and_then(|file| self.index.file(file))
                .map(|file| file.path().clone())
                .or_else(|| nav.gone.clone()),
            view: Viewport {
                tab: nav.tab,
                lines: nav.lines,
                source: self.scrolls.get(ids::source()),
            },
        }
    }

    pub(crate) const fn go_at_frame_end(&mut self, going: Going) {
        self.nav.going = Some(going);
    }

    pub(crate) fn track_navigation(&mut self) {
        match self.nav.going.take() {
            Some(Going::Back) => self.back(),
            Some(Going::Forward) => self.forward(),
            None => {}
        }
        let how = mem::replace(&mut self.nav.moving, Move::Select);
        let now = self.here();
        let history = &mut self.nav.history;
        if let Some(previous) = history.last.take()
            && now.moved_from(&previous, how)
        {
            history.back.push(previous);
            if history.back.len() > History::LONGEST.get() {
                history.back.remove(0);
            }
            history.forward.clear();
        }
        history.last = Some(now);
    }

    fn reachable(&self, stack: &mut Vec<Place>) -> Option<Place> {
        let here = self.here();
        while let Some(place) = stack.pop() {
            if place.moved_from(&here, Move::Jump) {
                return Some(place);
            }
        }
        None
    }

    pub(crate) fn back(&mut self) {
        let mut back = mem::take(&mut self.nav.history.back);
        let found = self.reachable(&mut back);
        self.nav.history.back = back;
        let Some(place) = found else {
            return;
        };
        let here = self.here();
        self.nav.history.forward.push(here);
        self.go(place);
    }

    pub(crate) fn forward(&mut self) {
        let mut forward = mem::take(&mut self.nav.history.forward);
        let found = self.reachable(&mut forward);
        self.nav.history.forward = forward;
        let Some(place) = found else {
            return;
        };
        let here = self.here();
        self.nav.history.back.push(here);
        self.go(place);
    }

    fn go(&mut self, place: Place) {
        match place.focus.and_then(|key| self.index.by_key(&key)) {
            Some(symbol) => self.focus_symbol(symbol),
            None => self.nav.focus = None,
        }
        let file = place
            .file
            .as_ref()
            .and_then(|path| self.index.find_file(path));
        self.nav.gone = place.file.filter(|_| file.is_none());
        self.nav.file = file;
        self.nav.lines = place.view.lines;
        self.nav.scroll_to = None;
        self.scrolls.set(ids::source(), place.view.source);
        self.set_tab(place.view.tab);
        self.nav.history.last = Some(self.here());
    }

    pub(crate) fn focus_symbol(&mut self, symbol: SymbolId) {
        let Some(span) = self.index.symbol(symbol).map(domain::Symbol::span) else {
            return;
        };
        self.nav.file = Some(symbol.file());
        self.nav.gone = None;
        self.nav.lines = Some(LineSelection {
            from: span.start(),
            to: span.end(),
        });
        self.nav.scroll_to_line(span.start());
        self.nav.focus = Some(symbol);
    }

    pub(crate) fn go_to_symbol(&mut self, symbol: SymbolId) {
        self.focus_symbol(symbol);
        self.nav.show(Tab::Source);
    }

    pub(crate) fn jumped_to_symbol(&mut self, symbol: SymbolId) {
        self.nav.moving = Move::Jump;
        self.go_to_symbol(symbol);
    }

    pub(crate) fn open_top(&mut self, file: FileId, line: Line) {
        self.open_line(file, line);
        self.nav.scroll_to_top();
    }

    pub(crate) fn select_file(&mut self, path: &RelativePath) {
        let file = self.index.find_file(path);
        self.nav.gone = file.is_none().then(|| path.clone());
        self.nav.file = file;
        self.nav.focus = None;
        self.clear_lines();
    }

    pub(crate) fn scrolled_to_file(&mut self, path: &RelativePath) {
        self.select_file(path);
        self.review.picked = None;
        self.nav.history.last = Some(self.here());
    }

    pub(crate) fn open_line(&mut self, file: FileId, line: Line) {
        self.nav.moving = Move::Jump;
        self.nav.file = Some(file);
        self.nav.gone = None;
        self.nav.focus = None;
        self.nav.lines = Some(LineSelection::one(line));
        self.nav.scroll_to_line(line);
        self.nav.show(Tab::Source);
    }

    pub(crate) fn show_editor(&mut self, file: FileId, line: Line) {
        if self.nav.file != Some(file) {
            self.open_line(file, line);
        }
        let ticket = self.nav.ticket();
        self.nav.scroll_to = Some(LineRequest {
            line,
            ticket,
            aim: Aim::Editor,
        });
        self.nav.show(Tab::Source);
    }

    pub(crate) fn open_gone(&mut self, path: RelativePath) {
        self.nav.moving = Move::Jump;
        self.nav.file = None;
        self.nav.focus = None;
        self.nav.gone = Some(path);
        self.nav.lines = None;
        self.nav.scroll_to_top();
        self.nav.show(Tab::Source);
    }

    pub(crate) fn show_gone(&mut self, path: RelativePath, old: Line, aim: Aim) {
        if self.nav.gone.as_ref() != Some(&path) {
            self.open_gone(path);
        }
        let ticket = self.nav.ticket();
        self.nav.scroll_to = Some(LineRequest {
            line: old,
            ticket,
            aim,
        });
        self.nav.show(Tab::Source);
    }

    pub(crate) fn clear_lines(&mut self) {
        self.nav.lines = None;
        self.line_grab = None;
    }

    pub(crate) fn select_line(&mut self, path: &RelativePath, line: Line, gesture: LineGesture) {
        if self.shown_path().as_ref() != Some(path) {
            if gesture == LineGesture::Drag {
                return;
            }
            self.select_file(path);
        }
        match (gesture, self.line_grab) {
            (LineGesture::Press, _) => {
                self.review.picked = None;
                self.line_grab = Some(line);
                self.nav.lines = Some(LineSelection::one(line));
            }
            (LineGesture::Drag, Some(from)) => {
                self.nav.lines = Some(LineSelection { from, to: line });
            }
            (LineGesture::Drag, None) => {}
        }
    }

    pub(crate) const fn clear_line_selection(&mut self) {
        self.line_grab = None;
    }

    pub(crate) fn set_tab(&mut self, tab: Tab) {
        self.nav.show(tab);
        if tab == Tab::Search {
            self.fields.focus(Which::Search);
        }
    }

    pub(crate) fn show_view(&mut self, view: View) {
        match view.tab() {
            Some(tab) => self.set_tab(tab),
            None => self.panels.activate(view),
        }
    }

    pub(crate) fn pick_view(&mut self, panel: BranchId, view: View) {
        self.panels.pick(panel, view);
        self.fields.release(Which::ViewSearch);
        if let Some(tab) = view.tab() {
            self.set_tab(tab);
        }
    }

    pub(crate) fn reveal_tab(&mut self) {
        let (tab, asked) = (self.nav.tab, self.nav.asked);
        self.panels.reveal(tab, asked);
        if !self.panels.is_shown(View::Search) {
            self.fields.release(Which::Search);
        }
    }

    pub(crate) fn refocus(&mut self, focus: Option<SymbolId>, file: Option<FileId>) {
        self.nav.focus = focus;
        self.nav.file = file;
    }

    pub(crate) fn forget_place(&mut self) {
        self.nav.focus = None;
        self.nav.file = None;
        self.nav.gone = None;
        self.nav.lines = None;
        self.nav.history = History::default();
    }

    pub(crate) fn scrolled_to_line(&mut self, ticket: Ticket) {
        if self
            .nav
            .scroll_to
            .is_some_and(|request| request.ticket == ticket)
        {
            self.nav.scroll_to = None;
        }
    }
}

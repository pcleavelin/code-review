use std::collections::BTreeSet;
use std::env;
use std::mem;
use std::path::PathBuf;
use std::time::Duration;

use domain::Layout;
use domain::{Index, Line, RelativePath, Root};
use io_config::{LayoutStore, Reach, SettingsStore};
use io_picture::PictureCache;
use platform::{
    ClipboardRequest, Cursor, Exit, Frame as PlatformFrame, Outcome, Renderer, ScriptLine,
    Visibility,
};
use strum::VariantArray;
use ui::{Button, Capture, Count, Id, Input, Label, Measure, Px, Ui};

use crate::action::Action;
use crate::columns::{InboxColumn, InboxGrid};
use crate::dump::{self, Context, Dump, DumpLines};
use crate::field::Which;
use crate::grid::Grids;
use crate::ids;
use crate::keys::{self, Going};
use crate::model::{Metrics, Model, Tab, TabName};
use crate::palette::PaletteAction;
use crate::panels::{self, View};
use crate::review::{ReviewAct, ReviewJobs, Reviewing};
use crate::settings::{KeptSettings, SettingsAct};
use crate::status::Status;
use crate::theme::{self, BACKGROUND, CONSOLE_ROWS, PANEL_PADDING, TEXT};
use crate::views;
use crate::widgets::{Frame, Overlay};
use crate::work::Services;

struct Shot {
    path: PathBuf,
    frame: Count,
}

pub(crate) struct App {
    pub(crate) model: Model,
    pub(crate) services: Services,
    pub(crate) jobs: ReviewJobs,
    pub(crate) ui: Ui,
    pub(crate) grids: Grids,
    shot: Option<Shot>,
    shot_next: Option<PathBuf>,
    layout: KeptLayout,
    pub(crate) clipboard: ClipboardRequest,
    pub(crate) settings: KeptSettings,
    cursor: Cursor,
    hidden_focus: Count,
    reported_duplicates: BTreeSet<Id>,
}

#[derive(Debug, Default)]
struct KeptLayout {
    store: Option<LayoutStore>,
    saved: Option<Layout>,
}

const SHOT_TAB_FRAME: Count = Count::new(3);
const SHOT_SCROLL_FRAME: Count = Count::new(6);
const SHOT_FRAME: Count = Count::new(20);
const FOCUS_GRACE: Count = Count::new(4);
const ANIMATING: Duration = Duration::from_millis(8);
const BUSY: Duration = Duration::from_millis(50);
const IDLE: Duration = Duration::from_secs(1);

const PICTURES: Folder = Folder("code-review-pictures");

#[derive(Clone, Copy, Debug)]
struct Folder(&'static str);

pub(crate) fn picture_cache() -> PictureCache {
    PictureCache::new(env::temp_dir().join(PICTURES.0))
}

fn scripted() -> bool {
    env::var_os("REVIEW_SCRIPT").is_some() || env::var_os("REVIEW_SHOT").is_some()
}

impl App {
    pub(crate) fn new(root: &Root) -> Self {
        let reach = if scripted() {
            Reach::OverrideOnly
        } else {
            Reach::User
        };
        let mut model = Model::new(Index::default(), Reviewing::find(root, reach));
        let settings = KeptSettings::load(SettingsStore::find(reach));
        model.settings = settings.chosen();
        let layout_store = LayoutStore::find(reach);
        if let Some(saved) = layout_store.as_ref().and_then(LayoutStore::load) {
            model.panels = panels::Panels::from_layout(&saved.tree);
            model.inbox_grid = InboxGrid::of_saved(&saved.columns);
        }
        let layout = KeptLayout {
            saved: Some(model.layout()),
            store: layout_store,
        };
        let mut app = Self {
            model,
            services: Services::new(root),
            jobs: ReviewJobs::default(),
            ui: Ui::default(),
            grids: Grids::default(),
            shot: env::var_os("REVIEW_SHOT").map(|path| Shot {
                path: PathBuf::from(path),
                frame: Count::ZERO,
            }),
            shot_next: None,
            layout,
            clipboard: ClipboardRequest::Keep,
            cursor: Cursor::Default,
            settings,
            hidden_focus: Count::ZERO,
            reported_duplicates: BTreeSet::new(),
        };
        app.model.set_tab(Tab::Inbox);
        app.index_working_copy();
        app.list_pulls();
        app
    }

    #[cfg(test)]
    pub(crate) fn of_model(model: Model, root: &Root) -> Self {
        Self {
            model,
            services: Services::new(root),
            jobs: ReviewJobs::default(),
            ui: Ui::default(),
            grids: Grids::default(),
            shot: None,
            shot_next: None,
            layout: KeptLayout::default(),
            clipboard: ClipboardRequest::Keep,
            settings: KeptSettings::default(),
            cursor: Cursor::Default,
            hidden_focus: Count::ZERO,
            reported_duplicates: BTreeSet::new(),
        }
    }

    fn shoot(&mut self, renderer: &mut Renderer) -> Exit {
        let mut exit = Exit::Stay;
        if let Some(shot) = self.shot.as_mut() {
            shot.frame += Count::new(1);
            let frame = shot.frame;
            let path = shot.path.clone();
            if frame == SHOT_SCROLL_FRAME
                && let Some(offset) = env::var("REVIEW_SHOT_SCROLL")
                    .ok()
                    .and_then(|value| value.parse::<i32>().ok())
            {
                self.apply(Action::Scroll(ids::source(), Px::new(offset)));
            }
            if frame == SHOT_TAB_FRAME {
                let name = env::var("REVIEW_SHOT_TAB").unwrap_or_default();
                self.apply(Action::ShowView(View::of_tab(Tab::from_name(
                    &TabName::new(&name),
                ))));
            }
            if frame == SHOT_FRAME {
                renderer.shoot(path);
            }
            if frame > SHOT_FRAME && !self.working() {
                exit = Exit::Quit;
            }
        }
        if let Some(path) = self.shot_next.take() {
            renderer.shoot(path);
        }
        exit
    }

    fn palette_keys(&mut self, input: &Input) {
        self.apply(Action::Type(
            Which::Palette,
            keys::palette_edits(input),
            input.typed.clone(),
        ));
        self.model.refresh_palette();
        for key in keys::palette_keys(input) {
            self.apply(Action::Palette(PaletteAction::Key(key)));
        }
    }

    pub(crate) fn keys(&mut self, input: &Input) {
        if keys::settings_toggled(input) {
            self.apply(Action::Settings(SettingsAct::Toggle));
            return;
        }
        if self.model.settings_menu.is_some() {
            if keys::escaped(input) {
                self.apply(Action::Settings(SettingsAct::Close));
            }
            return;
        }
        if keys::palette_toggled(input) {
            self.apply(Action::Palette(PaletteAction::Toggle));
            return;
        }
        if self.model.palette.is_some() {
            if self.model.fields.focused() == Some(Which::Palette) {
                self.palette_keys(input);
                return;
            }
            self.apply(Action::Palette(PaletteAction::Close));
        }
        let edits = keys::edits(input);
        let typed = &input.typed;
        let mut actions = Vec::new();
        let typing = self.model.fields.focused();
        if keys::going(input, Going::Back, typing) {
            actions.push(Action::Back);
        }
        if keys::going(input, Going::Forward, typing) {
            actions.push(Action::Forward);
        }
        actions.extend(
            keys::review_keys(input, typing)
                .into_iter()
                .map(Action::Review),
        );
        if typing.is_none() && self.model.starts_comment(typed) {
            actions.push(Action::Review(ReviewAct::StartDraftHere));
        }
        for which in [
            Which::Command,
            Which::Search,
            Which::SymbolFilter,
            Which::ViewSearch,
            Which::Draft,
            Which::Summary,
            Which::Inbox,
            Which::Pin,
            Which::Token,
        ]
        .into_iter()
        .chain(InboxColumn::VARIANTS.iter().copied().map(Which::Column))
        {
            actions.push(Action::Type(which, edits.clone(), typed.clone()));
        }
        for action in actions {
            self.apply(action);
        }
        if self.model.panels.picker().is_some()
            && self.model.fields.focused() != Some(Which::ViewSearch)
        {
            self.apply(Action::ClosePicker);
        }
    }

    fn dump(&self) {
        let mut lines = DumpLines::default();
        let model = &self.model;
        let context = Context {
            model,
            ui: &self.ui,
            services: &self.services,
        };
        let parts: [&dyn Dump; 11] = [
            &model.nav,
            &model.review,
            &model.review.logins,
            &model.scrolls,
            &model.panels,
            &model.status,
            &model.fields,
            &model.work,
            &model.queries,
            &model.palette,
            &model.settings_menu,
        ];
        for part in parts {
            part.dump(&context, &mut lines);
        }
        if self.cursor != Cursor::Default {
            lines.line(format_args!("cursor={:?}", self.cursor));
        }
        lines.print();
    }
}

impl platform::App for App {
    fn frame(&mut self, renderer: &mut Renderer, input: &mut Input) -> PlatformFrame {
        self.use_settings(renderer);
        let font = theme::scaled_font_size(self.model.settings.size(), renderer.scale());
        self.model.metrics = Metrics {
            font,
            cell: renderer.cell(font),
        };
        let exit = self.shoot(renderer);
        self.poll_backend();
        self.poll_reindex();
        self.poll_link();
        self.poll_sources();
        self.poll_review();
        self.poll_login();
        self.model.now = input.time;
        self.model.review.pictures.wake = None;
        self.keys(input);
        self.model.refresh_palette();
        self.ui.capture(if self.model.settings_menu.is_some() {
            Capture::Floating
        } else {
            Capture::Everything
        });
        keys::turn_wheel(&mut input.pointer);
        self.ui.begin(input);
        if self.ui.pointer().pressed.contains(Button::Left) {
            self.model.fields.press(self.ui.hot());
        }
        for action in views::panel_input(&self.model, &self.ui) {
            self.apply(action);
        }
        let laid_out = self.ui.interaction(ids::body()).rect().is_some();
        let mut queue = Vec::new();
        let mut frame = Frame {
            ui: &mut self.ui,
            queue: &mut queue,
            grids: &mut self.grids,
            metrics: self.model.metrics,
            overlay: Overlay::default(),
            tooltip: None,
            attached_tip: None,
            cursor: Cursor::Default,
        };
        views::build(&self.model, &mut frame);
        let cursor = frame.cursor;
        self.cursor = cursor;
        self.ui.end(renderer);
        self.check_focus_shown();
        self.check_ids_unique();
        if laid_out {
            self.fit_console();
        }
        let drawing = self.ui.draw(renderer, TEXT);
        for action in queue {
            self.apply(action);
        }
        self.model.reveal_tab();
        self.keep_layout();
        self.keep_settings();
        self.model.track_navigation();
        let busy =
            self.working() || self.shot.is_some() || self.model.resting.pending(self.model.now);
        let after = if self.model.line_grab.is_some() || self.ui.settling() {
            ANIMATING
        } else if busy {
            BUSY
        } else {
            IDLE
        };
        let next_frame = self
            .model
            .review
            .pictures
            .wake
            .map(|wake| wake.saturating_sub(self.model.now).max(ANIMATING));
        PlatformFrame {
            redraw_after: next_frame.map_or(after, |next| next.min(after)),
            exit,
            clear: BACKGROUND,
            cursor,
            drawing,
            clipboard: mem::take(&mut self.clipboard),
        }
    }

    fn script(&mut self, line: &ScriptLine) -> Outcome {
        match AppCommand::named(line) {
            Some(AppCommand::Idle) => {
                if self.working() || self.model.work.unmerged().get() > 0 {
                    return Outcome::Retry;
                }
            }
            Some(AppCommand::Rect) => {
                let mut lines = DumpLines::default();
                lines.rect(&self.ui, line);
                lines.print();
            }
            Some(AppCommand::Tab) => {
                let name = Label::new(line.word(1).unwrap_or_default());
                if let Some(view) = View::named(&name) {
                    self.apply(Action::ShowView(view));
                }
            }
            Some(AppCommand::Scroll) => {
                if let (Some(name), Some(offset)) = (
                    line.word(1),
                    line.word(2).and_then(|value| value.parse::<i32>().ok()),
                ) {
                    self.apply(Action::Scroll(Id::new(name), Px::new(offset)));
                }
            }
            Some(AppCommand::Shot) => self.shot_next = line.word(1).map(PathBuf::from),
            Some(AppCommand::Open) => {
                if let Some(file) = line
                    .word(1)
                    .and_then(|path| self.model.index.find_file(&RelativePath::new(path)))
                {
                    let number = line
                        .word(2)
                        .and_then(|value| value.parse::<u32>().ok())
                        .unwrap_or(0);
                    self.apply(Action::GoTo(file, Line::new(number)));
                }
            }
            Some(AppCommand::Dump) => self.dump(),
            None => dump::report(format_args!("script: unknown command '{line}'")),
        }
        Outcome::Done
    }

    fn locate(&mut self, id: Id) -> Visibility {
        self.ui
            .placement(id)
            .map_or(Visibility::Absent, |placement| {
                placement
                    .visible_center()
                    .map_or(Visibility::OutOfView, Visibility::Visible)
            })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Literal(&'static str);

impl Literal {
    const fn as_str(self) -> &'static str {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, VariantArray)]
enum AppCommand {
    Idle,
    Rect,
    Tab,
    Scroll,
    Shot,
    Open,
    Dump,
}

impl AppCommand {
    const fn name(self) -> Literal {
        Literal(match self {
            Self::Idle => "idle",
            Self::Rect => "rect",
            Self::Tab => "tab",
            Self::Scroll => "scroll",
            Self::Shot => "shot",
            Self::Open => "open",
            Self::Dump => "dump",
        })
    }

    fn named(line: &ScriptLine) -> Option<Self> {
        let word = line.word(0).unwrap_or_default();
        Self::VARIANTS
            .iter()
            .copied()
            .find(|command| command.name().as_str() == word)
    }
}

impl App {
    fn check_focus_shown(&mut self) {
        let hidden = self.model.fields.focused().filter(|_| {
            self.model
                .fields
                .focused_at()
                .and_then(|id| self.ui.placement(id))
                .and_then(ui::Placement::visible_center)
                .is_none()
        });
        let Some(which) = hidden else {
            self.hidden_focus = Count::ZERO;
            return;
        };
        self.hidden_focus += Count::new(1);
        if self.hidden_focus == FOCUS_GRACE && scripted() {
            dump::report(format_args!(
                "script: the {which:?} field has the keyboard but is not on screen: the action that focused it must also show it"
            ));
        }
    }

    fn check_ids_unique(&mut self) {
        if !scripted() {
            return;
        }
        for duplicate in self.ui.duplicate_ids() {
            if self.reported_duplicates.insert(duplicate.id) {
                dump::report(format_args!(
                    "script: two elements were laid out under one id, at {:?} and at {:?}: only the second takes clicks, so give each its own id (`with` or `nth` on its control in ids.rs)",
                    duplicate.first, duplicate.second
                ));
            }
        }
    }

    fn fit_console(&mut self) {
        let body = self.ui.interaction(ids::body()).rect();
        let log = self.ui.interaction(ids::console()).rect();
        if let (Some(body), Some(log)) = (body, log) {
            let metrics = self.model.metrics;
            let room = body.extent().height - panels::divider_width(metrics.font);
            let target = CONSOLE_ROWS.of(metrics.row_height()) + PANEL_PADDING * 2;
            self.model
                .panels
                .fit_console(room, log.extent().height, target);
        }
    }

    fn keep_layout(&mut self) {
        let Some(store) = &self.layout.store else {
            return;
        };
        if self.model.panels.grab().is_some() || self.ui.pointer().down.contains(Button::Left) {
            return;
        }
        let now = self.model.layout();
        if self.layout.saved.as_ref() == Some(&now) {
            return;
        }
        if let Err(error) = store.save(&now) {
            self.model.status = Status::LayoutUnsaved(Label::new(error.to_string()));
        }
        self.layout.saved = Some(now);
    }
}

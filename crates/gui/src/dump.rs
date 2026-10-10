use std::fmt::{self, Write as _};
use std::io::{self, Write as _};
use std::iter;

use platform::ScriptLine;
use ui::{Axis, Id, Px, Ui};

use crate::field::{Fields, Which};
use crate::ids;
use crate::model::{Model, Scrolls};
use crate::nav::Nav;
use crate::palette::Palette;
use crate::panels::Panels;
use crate::peek::{Peek, Probe, Queries};
use crate::review::{Inbox, Reviewing, Submitting, Talk};
use crate::settings::{SettingsMenu, ShownFont, ThemeName};
use crate::status::Status;
use crate::work::{Services, WorkState};

#[derive(Default)]
pub(crate) struct DumpLines(String);

impl DumpLines {
    pub(crate) fn line(&mut self, arguments: fmt::Arguments<'_>) {
        self.0.push_str("DUMP ");
        self.0.write_fmt(arguments).unwrap_or_default();
        self.0.push('\n');
    }

    pub(crate) fn rect(&mut self, ui: &Ui, line: &ScriptLine) {
        if let Some(name) = line.word(1) {
            self.line(format_args!(
                "rect {name} = {:?}",
                ui.interaction(Id::from_name(name)).rect()
            ));
        }
    }

    pub(crate) fn print(&self) {
        let mut output = io::stderr().lock();
        drop(output.write_all(self.0.as_bytes()));
    }
}

pub(crate) fn report(arguments: fmt::Arguments<'_>) {
    let mut output = io::stderr().lock();
    drop(writeln!(output, "{arguments}"));
}

pub(crate) struct Context<'dump> {
    pub(crate) model: &'dump Model,
    pub(crate) ui: &'dump Ui,
    pub(crate) services: &'dump Services,
}

pub(crate) trait Dump {
    fn dump(&self, context: &Context<'_>, lines: &mut DumpLines);
}

struct Optional<T>(Option<T>);

impl<T: fmt::Display> fmt::Display for Optional<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            Some(value) => write!(formatter, "Some({value})"),
            None => formatter.write_str("None"),
        }
    }
}

struct Quoted<T>(T);

impl<T: fmt::Display> fmt::Display for Quoted<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{:?}", self.0.to_string())
    }
}

impl Dump for Nav {
    fn dump(&self, context: &Context<'_>, lines: &mut DumpLines) {
        let index = &context.model.index;
        let focus = self
            .focus()
            .and_then(|symbol| index.symbol(symbol))
            .map(|symbol| Quoted(symbol.name().as_str()));
        let file = self
            .file()
            .and_then(|file| index.file(file))
            .map(|file| Quoted(file.path().as_str()));
        let selection = self
            .lines()
            .map(|chosen| format!("({}, {})", chosen.from.value(), chosen.to.value()));
        lines.line(format_args!(
            "tab={:?} focus={} file={} gone={} sel={}",
            self.tab(),
            Optional(focus),
            Optional(file),
            Optional(self.gone().map(|path| Quoted(path.as_str()))),
            Optional(selection)
        ));
    }
}

impl Dump for Reviewing {
    fn dump(&self, _: &Context<'_>, lines: &mut DumpLines) {
        let inbox = match &self.inbox {
            Inbox::Asking => "asking".to_owned(),
            Inbox::Listed { entries, broken } => {
                format!("{} broken={}", entries.len(), broken.len())
            }
        };
        let pins: Vec<String> = self
            .desk
            .repos()
            .map(|repo| repo.name().to_string())
            .collect();
        lines.line(format_args!(
            "review inbox={inbox} repos=[{}] standing={:?} opening={} open={} draft={} submitting={}",
            pins.join(" "),
            self.standing,
            Optional(self.opening),
            Optional(self.opened().map(|opened| opened.number)),
            Optional(
                self.draft
                    .as_ref()
                    .map(|on| format!("{}:{}:{:?}", on.file, on.line.number(), on.side))
            ),
            self.submitting == Submitting::Sending
        ));
        if let Inbox::Listed { entries, .. } = &self.inbox {
            for entry in entries {
                lines.line(format_args!(
                    "entry {}#{} {:?} drafts={} {}",
                    entry.repo,
                    entry.pull.number,
                    entry.standing,
                    entry.drafts.len(),
                    Quoted(entry.pull.title.as_str())
                ));
            }
        }
        let Some(open) = &self.open else {
            return;
        };
        let opened = &open.opened;
        let talk = match &open.talk {
            Talk::Asking => "asking".to_owned(),
            Talk::Failed(why) => format!("failed {:?}", why.as_str()),
            Talk::Got(talk) => format!(
                "{} threads, {} signed pictures",
                talk.threads.len(),
                talk.signed_pictures.len()
            ),
        };
        lines.line(format_args!("talk {talk}"));
        if let Some(conversation) = open.conversation() {
            for thread in &conversation.threads {
                let place = thread.place.as_ref().map(|place| {
                    format!("{}:{}:{:?}", place.file, place.line.number(), place.side)
                });
                lines.line(format_args!(
                    "thread {} {:?} notes={} at={}",
                    thread.id,
                    thread.state,
                    thread.notes.len(),
                    Optional(place)
                ));
            }
        }
        for change in &opened.changes {
            lines.line(format_args!(
                "change {} {:?} +{} -{} {:?}",
                change.path(),
                change.kind(),
                change.added(),
                change.removed(),
                opened.seen(change)
            ));
        }
        for draft in opened.review.drafts() {
            lines.line(format_args!(
                "draft {} {}:{}-{} {:?} reply={} {}",
                draft.id,
                draft.file,
                draft.lines().start().number(),
                draft.line.number(),
                draft.side,
                Optional(draft.reply.as_ref().map(ToString::to_string)),
                Quoted(draft.text.as_str())
            ));
        }
    }
}

impl Dump for Scrolls {
    fn dump(&self, context: &Context<'_>, lines: &mut DumpLines) {
        for (name, id) in [
            ("source", ids::source()),
            ("inbox", ids::inbox()),
            ("changes", ids::changes()),
            ("review", ids::review()),
            ("console", ids::console()),
        ] {
            if let Some(placement) = context.ui.placement(id) {
                lines.line(format_args!(
                    "scroll {name} off={} rect={:?} content={:?}",
                    self.get(id),
                    placement.rect,
                    placement.content
                ));
            }
        }
        let source = (String::from("lines"), ids::LINES.id());
        for (name, id) in iter::once(source) {
            if let Some(placement) = context.ui.placement(id)
                && let Some(bar) =
                    placement.scrollbar(Axis::Horizontal, placement.scroll_offset.horizontal)
            {
                lines.line(format_args!(
                    "across {name} thumb={:?} off={} rect={:?} content={:?}",
                    bar.thumb,
                    placement.scroll_offset.horizontal,
                    placement.rect,
                    placement.content
                ));
            }
        }
        let table = context.model.across.get(ids::TABLE.id());
        if table > Px::ZERO {
            lines.line(format_args!("across table off={table}"));
        }
    }
}

impl Dump for Panels {
    fn dump(&self, _: &Context<'_>, lines: &mut DumpLines) {
        lines.line(format_args!("panels {self}"));
    }
}

struct PeekText<'dump>(&'dump Peek);

impl fmt::Display for PeekText<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Peek::Symbol(symbol) => write!(
                formatter,
                "Sym(SymRef {{ file: {}, sym: {} }})",
                symbol.file(),
                symbol.symbol()
            ),
            Peek::Line { file, line } => write!(formatter, "Line({file}, {})", line.value()),
            Peek::Outside {
                file,
                line,
                first,
                grid,
            } => write!(
                formatter,
                "Outside({}, {}, {}, {} rows)",
                file.display(),
                line.value(),
                first.value(),
                grid.rows()
            ),
        }
    }
}

impl Dump for Status {
    fn dump(&self, context: &Context<'_>, lines: &mut DumpLines) {
        let model = context.model;
        lines.line(format_args!(
            "tip={} peek={} status={}",
            Optional(model.tip_shown.as_ref().map(|tip| Quoted(tip.as_str()))),
            Optional(model.peek.as_ref().map(PeekText)),
            Quoted(self)
        ));
    }
}

struct ProbeAt<'probe>(&'probe Probe);

impl fmt::Display for ProbeAt<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let position = &self.0.position;
        write!(
            formatter,
            "{}:{}:{}",
            position.file,
            position.line.number(),
            position.character.value()
        )
    }
}

impl Dump for Queries {
    fn dump(&self, _: &Context<'_>, lines: &mut DumpLines) {
        lines.line(format_args!(
            "hover waiting={} asked={}",
            Optional(self.waiting().map(ProbeAt)),
            self.asked()
        ));
    }
}

impl Dump for Fields {
    fn dump(&self, _: &Context<'_>, lines: &mut DumpLines) {
        lines.line(format_args!(
            "field focused={}",
            Optional(
                self.focused()
                    .map(|which| which.control().element().as_str())
            )
        ));
        if let Some(which) = self.focused() {
            let field = self.get(which);
            let selection = field.text_selection().map_or_else(String::new, |chosen| {
                format!(" selection={}..{}", chosen.from.get(), chosen.to.get())
            });
            lines.line(format_args!(
                "field {which:?} caret={}{selection} first={} rows={} text={}",
                field.caret().get(),
                field.first(),
                field.rows(which.shape()).len(),
                Quoted(field.text().as_str())
            ));
        }
    }
}

impl Dump for WorkState {
    fn dump(&self, context: &Context<'_>, lines: &mut DumpLines) {
        lines.line(format_args!(
            "backend progress={} indexing={} unmerged={} reindexing={} linking={}",
            Quoted(self.progress().as_str()),
            self.indexing(),
            self.unmerged(),
            context.services.reindexing(),
            context.services.linking()
        ));
        let servers: Vec<String> = self
            .server_states(&context.model.index)
            .into_iter()
            .map(|server| format!("{}={}", server.language.program(), server.state))
            .collect();
        lines.line(format_args!("servers {}", servers.join(" ")));
    }
}

impl Dump for Option<Palette> {
    fn dump(&self, context: &Context<'_>, lines: &mut DumpLines) {
        let Some(palette) = self else {
            return;
        };
        let chosen = palette.entries.get(palette.selected.get()).map(|entry| {
            format!(
                "{} {}",
                entry.kind.tag().as_str(),
                Quoted(entry.name.as_str())
            )
        });
        lines.line(format_args!(
            "palette query={} rows={} selected={} top={} chosen={}",
            Quoted(context.model.fields.get(Which::Palette).text().as_str()),
            palette.entries.len(),
            palette.selected.get(),
            palette.top.get(),
            Optional(chosen)
        ));
    }
}

impl Dump for Option<SettingsMenu> {
    fn dump(&self, context: &Context<'_>, lines: &mut DumpLines) {
        let settings = &context.model.settings;
        lines.line(format_args!(
            "settings open={} theme={} font={} size={}",
            self.is_some(),
            ThemeName::new(settings.theme()),
            ShownFont::new(settings),
            settings.size().get()
        ));
    }
}

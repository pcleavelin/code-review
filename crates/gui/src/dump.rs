use std::fmt::{self, Write as _};
use std::io::{self, Write as _};
use std::iter;

use domain::{Block, Card, Conversation};
use platform::ScriptLine;
use session::Entry;
use ui::{Axis, Id, Px, Ui};

use crate::diffs::Loading;
use crate::field::{Fields, Shown, Which};
use crate::ids;
use crate::login::{Held, JiraForm, Logins, Stage};
use crate::model::{Model, Scrolls};
use crate::nav::Nav;
use crate::palette::Palette;
use crate::panels::Panels;
use crate::peek::{Peek, Probe, Queries};
use crate::review::{Inbox, Requests, Reviewing, Talk};
use crate::settings::{SettingsMenu, ShownFont, ThemeName};
use crate::status::Status;
use crate::viewer::{Fitting, Viewer};
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

struct CardShown<'card>(&'card Loading<Option<Card>>);

impl fmt::Display for CardShown<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Loading::Asking => formatter.write_str("asking"),
            Loading::Failed(why) => write!(formatter, "failed {}", Quoted(why.as_str())),
            Loading::Got(None) => formatter.write_str("none"),
            Loading::Got(Some(card)) => write!(
                formatter,
                "{} {} status={} {:?} kind={} {:?}",
                card.key,
                Quoted(card.summary.as_str()),
                Quoted(card.status.as_str()),
                card.stage,
                Quoted(card.kind.as_str()),
                card.family
            ),
        }
    }
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

fn dump_conversation(conversation: &Conversation, lines: &mut DumpLines) {
    for thread in &conversation.threads {
        let place = thread
            .place
            .as_ref()
            .map(|place| format!("{}:{}:{:?}", place.file, place.line.number(), place.side));
        lines.line(format_args!(
            "thread {} {:?} notes={} at={}",
            thread.id,
            thread.state,
            thread.notes.len(),
            Optional(place)
        ));
    }
    for block in conversation.blocks() {
        match block {
            Block::Submitted {
                submission,
                threads,
            } => lines.line(format_args!(
                "block submitted {} {} {:?} [{}]",
                submission.id,
                submission.author,
                submission.decision,
                threads
                    .iter()
                    .map(|thread| thread.id.to_string())
                    .collect::<Vec<_>>()
                    .join(" ")
            )),
            Block::Alone(thread) => lines.line(format_args!("block alone {}", thread.id)),
        }
    }
}

fn dump_listing(review: &Reviewing, lines: &mut DumpLines) {
    for (repo, waiting) in &review.waiting {
        lines.line(format_args!(
            "waiting {repo} {}",
            Quoted(&waiting.to_string())
        ));
    }
    if let Inbox::Listed { broken, .. } = &review.inbox {
        for gone in broken {
            lines.line(format_args!(
                "broken {} {}",
                gone.place.as_str(),
                Quoted(gone.why.as_str())
            ));
        }
    }
}

fn dump_card(review: &Reviewing, card: &Loading<Option<Card>>, lines: &mut DumpLines) {
    lines.line(format_args!(
        "card {} collapse={:?}",
        CardShown(card),
        review.card_collapse
    ));
    for (link, state) in review.pictures.loaded() {
        let state = match state {
            Loading::Asking => "asking".to_owned(),
            Loading::Got(_) => "got".to_owned(),
            Loading::Failed(why) => format!("failed {}", Quoted(why.as_str())),
        };
        lines.line(format_args!("picture {} {state}", link.as_str()));
    }
}

fn dump_requests(requests: &Requests, entries: &[Entry], lines: &mut DumpLines) {
    let pulls: Vec<String> = requests
        .pulls
        .iter()
        .flat_map(|(repo, numbers)| numbers.iter().map(move |number| format!("{repo}#{number}")))
        .collect();
    let unanswered: Vec<String> = requests
        .unanswered
        .iter()
        .map(|unanswered| {
            format!(
                "{}:{:?}:{:?}",
                unanswered.broken.place.as_str(),
                unanswered.cure,
                unanswered.broken.why.as_str()
            )
        })
        .collect();
    lines.line(format_args!(
        "requests chip={} answered={} pulls=[{}] unanswered=[{}]",
        Quoted(requests.chip(entries).as_str()),
        requests.answered,
        pulls.join(" "),
        unanswered.join(" ")
    ));
}

impl Dump for Reviewing {
    fn dump(&self, _: &Context<'_>, lines: &mut DumpLines) {
        let inbox = match &self.inbox {
            Inbox::Asking => "asking".to_owned(),
            Inbox::Listed {
                entries, broken, ..
            } => {
                format!("{} broken={}", entries.len(), broken.len())
            }
        };
        let pins: Vec<String> = self
            .desk
            .repos()
            .map(|repo| repo.name().to_string())
            .collect();
        lines.line(format_args!(
            "review inbox={inbox} repos=[{}] standing={:?} requested={:?} opening={} open={} draft={} submitting={:?}",
            pins.join(" "),
            self.standing,
            self.requested,
            Optional(self.opening),
            Optional(self.opened().map(|opened| opened.number)),
            Optional(
                self.draft
                    .as_ref()
                    .map(|on| format!("{}:{}:{:?}", on.file, on.line.number(), on.side))
            ),
            self.submitting
        ));
        dump_listing(self, lines);
        if let Inbox::Listed {
            entries, requests, ..
        } = &self.inbox
        {
            dump_requests(requests, entries, lines);
            for entry in entries {
                lines.line(format_args!(
                    "entry {}#{} {:?} drafts={} requested={} {}",
                    entry.repo,
                    entry.pull.number,
                    entry.standing,
                    entry.drafts.len(),
                    requests.of(entry),
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
        lines.line(format_args!("rights {:?}", open.rights));
        dump_card(self, &open.card, lines);
        lines.line(format_args!(
            "diffs layout={:?} commit={}",
            self.diffs.layout,
            Optional(self.diffs.commit())
        ));
        if let Some(conversation) = open.conversation() {
            dump_conversation(conversation, lines);
        }
        for change in &opened.changes {
            lines.line(format_args!(
                "change {} {:?} +{} -{} {:?}{}",
                change.path(),
                change.kind(),
                change.added(),
                change.removed(),
                opened.seen(change),
                if opened.generated.contains(change.path()) {
                    " generated"
                } else {
                    ""
                }
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

impl Dump for Logins {
    fn dump(&self, context: &Context<'_>, lines: &mut DumpLines) {
        let sites: Vec<String> = self
            .sites(&context.model.review.desk)
            .into_iter()
            .map(|site| {
                let held = match self.held.get(&site) {
                    Some(Held::In) => "in".to_owned(),
                    Some(Held::Out) => "out".to_owned(),
                    Some(Held::TimedOut(why)) => format!("timed-out={}", Quoted(why.as_str())),
                    None => "unknown".to_owned(),
                };
                let failed = self.failed.get(&site).map_or_else(String::new, |why| {
                    format!(" failed={}", Quoted(why.as_str()))
                });
                format!("{site}={held}{failed}")
            })
            .collect();
        let stage = match &self.login {
            None => "none".to_owned(),
            Some(login) => match &login.stage {
                Stage::Device { authorization, .. } => format!(
                    "{} device code={} page={}",
                    login.site,
                    authorization.user_code(),
                    authorization.verification()
                ),
                Stage::Paste { page } => {
                    format!("{} paste page={}", login.site, Optional(page.as_ref()))
                }
            },
        };
        lines.line(format_args!(
            "login sites=[{}] stage={stage}",
            sites.join(" ")
        ));
        {
            let jira: Vec<String> =
                self.jira
                    .iter()
                    .map(|(site, held)| {
                        let held = match held {
                            Held::In => "in".to_owned(),
                            Held::Out => "out".to_owned(),
                            Held::TimedOut(why) => format!("timed-out={}", Quoted(why.as_str())),
                        };
                        let name = self.jira_names.get(site).map_or_else(String::new, |name| {
                            format!(" as {}", Quoted(name.as_str()))
                        });
                        format!("{site}={held}{name}")
                    })
                    .collect();
            let form = match &self.jira_form {
                None => "none".to_owned(),
                Some(JiraForm::Editing) => "editing".to_owned(),
                Some(JiraForm::Checking(site)) => format!("checking {site}"),
                Some(JiraForm::Refused(why)) => format!("refused {}", Quoted(why.as_str())),
            };
            lines.line(format_args!("jira sites=[{}] form={form}", jira.join(" ")));
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
        let kept = context.model.across.get(ids::LINES.id());
        if kept > Px::ZERO {
            lines.line(format_args!("across lines kept={kept}"));
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
            let text = match which.shown() {
                Shown::Clear => Quoted(field.text().as_str()).to_string(),
                Shown::Masked => format!("<{} hidden>", field.text().as_str().chars().count()),
            };
            lines.line(format_args!(
                "field {which:?} caret={}{selection} first={} rows={} text={text}",
                field.caret().get(),
                field.first(),
                field.rows(which.shape()).len(),
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
        let index = &context.model.index;
        lines.line(format_args!(
            "index root={} files={}",
            index.root().as_path().display(),
            index.files().count()
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

impl Dump for Option<Viewer> {
    fn dump(&self, context: &Context<'_>, lines: &mut DumpLines) {
        let Some(viewer) = self else {
            return;
        };
        let zoom = match viewer.fitting {
            Fitting::Fit => "fit".to_owned(),
            Fitting::Zoomed(zoom) => format!("{}%", zoom.percent()),
        };
        let placed = context
            .model
            .review
            .pictures
            .get(&viewer.link)
            .and_then(|loading| match loading {
                Loading::Got(animation) => Some(animation.size()),
                Loading::Asking | Loading::Failed(_) => None,
            })
            .zip(context.ui.placement(ids::viewer_stage()))
            .map(|(size, stage)| format!("{:?}", viewer.placed(size, stage.rect)));
        lines.line(format_args!(
            "viewer {} zoom={zoom} pan=({}, {}) drawn={}",
            viewer.link.as_str(),
            viewer.pan.horizontal,
            viewer.pan.vertical,
            Optional(placed)
        ));
    }
}

impl Dump for Ui {
    fn dump(&self, _: &Context<'_>, lines: &mut DumpLines) {
        let selected = self.selected_text();
        lines.line(format_args!(
            "text-selection={}",
            Optional(selected.as_ref().map(|text| Quoted(text.as_str())))
        ));
    }
}

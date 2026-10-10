use std::mem;
use std::sync::LazyLock;

use domain::{CommitId, Linked, Login, PrNumber, Prose, WebLink};
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use regex::{Captures, Regex};
use ui::{Count, Label};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tone {
    Plain,
    Strong,
    Emphasis,
    Code,
    Struck,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Goal {
    Web(WebLink),
    Forge(Linked),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Piece {
    pub(crate) text: Label,
    pub(crate) tone: Tone,
    pub(crate) goal: Option<Goal>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Lead {
    Paragraph,
    Heading(Count),
    Item { depth: Count, marker: Label },
    Quote(Count),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Block {
    Text { lead: Lead, pieces: Vec<Piece> },
    Code { language: Label, text: Label },
    Picture { link: WebLink, alt: Label },
    Rule,
}

static AUTOLINK: LazyLock<Option<Regex>> = LazyLock::new(|| {
    Regex::new(
        r"(?P<url>https?://[^\s<>()\[\]]+[^\s<>()\[\].,;:!?'\x22])|(?:^|(?P<before>[\s(]))(?:(?P<pull>[#!])(?P<number>\d+)|@(?P<user>[A-Za-z0-9][A-Za-z0-9-]{0,38})|(?P<sha>[0-9a-f]{8,40}))\b",
    )
    .ok()
});

static IMG: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r#"(?i)<img\b[^>]*?\bsrc\s*=\s*["']([^"']+)["'][^>]*>"#).ok());

static ALT: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r#"(?i)\balt\s*=\s*["']([^"']*)["']"#).ok());

static TAG: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"<[^>]+>").ok());

fn goal_of(captures: &Captures<'_>) -> Option<Goal> {
    if let Some(url) = captures.name("url") {
        return WebLink::new(url.as_str()).map(Goal::Web);
    }
    if let (Some(mark), Some(number)) = (captures.name("pull"), captures.name("number")) {
        let number = PrNumber::parse(number.as_str())?;
        return Some(Goal::Forge(if mark.as_str().starts_with('!') {
            Linked::Pull(number)
        } else {
            Linked::Issue(number)
        }));
    }
    if let Some(user) = captures.name("user") {
        return Some(Goal::Forge(Linked::User(Login::new(user.as_str()))));
    }
    let sha = captures.name("sha")?.as_str();
    let mixed = sha.chars().any(|character| character.is_ascii_digit())
        && sha.chars().any(|character| character.is_ascii_alphabetic());
    mixed
        .then(|| CommitId::new(sha))
        .flatten()
        .map(|commit| Goal::Forge(Linked::Commit(commit)))
}

#[derive(Clone, Copy)]
struct Words<'text>(&'text str);

impl Words<'_> {
    fn pieces(self, tone: Tone) -> Vec<Piece> {
        let text = self.0;
        let plain = |part: &str| Piece {
            text: Label::new(part),
            tone,
            goal: None,
        };
        let Some(pattern) = AUTOLINK.as_ref() else {
            return vec![plain(text)];
        };
        let mut pieces = Vec::new();
        let mut at = 0;
        for captures in pattern.captures_iter(text) {
            let Some(whole) = captures.get(0) else {
                continue;
            };
            let start = captures
                .name("before")
                .map_or(whole.start(), |before| before.end());
            let Some(goal) = goal_of(&captures) else {
                continue;
            };
            if let Some(before) = text.get(at..start).filter(|before| !before.is_empty()) {
                pieces.push(plain(before));
            }
            if let Some(linked) = text.get(start..whole.end()) {
                pieces.push(Piece {
                    text: Label::new(linked),
                    tone,
                    goal: Some(goal),
                });
            }
            at = whole.end();
        }
        if let Some(rest) = text.get(at..).filter(|rest| !rest.is_empty()) {
            pieces.push(plain(rest));
        }
        pieces
    }
}

const fn heading_level(level: HeadingLevel) -> Count {
    Count::new(match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Numbering {
    Bullets,
    Next(Count),
}

#[derive(Default)]
struct Gathered(String);

impl Gathered {
    fn push(&mut self, text: &str) {
        self.0.push_str(text);
    }

    fn label(&self) -> Label {
        Label::new(self.0.trim_end_matches('\n'))
    }
}

struct CodeDraft {
    language: Label,
    text: Gathered,
}

struct PictureDraft {
    link: WebLink,
    alt: Gathered,
}

struct Cell(Vec<Piece>);

#[derive(Default)]
struct State {
    blocks: Vec<Block>,
    pieces: Vec<Piece>,
    lead: Option<Lead>,
    tones: Vec<Tone>,
    links: Vec<Option<Goal>>,
    lists: Vec<Numbering>,
    item: Option<Label>,
    quotes: Count,
    code: Option<CodeDraft>,
    picture: Option<PictureDraft>,
    cells: Vec<Cell>,
}

#[derive(Default)]
struct Reader(State);

impl Reader {
    fn tone(&self) -> Tone {
        self.0.tones.last().copied().unwrap_or(Tone::Plain)
    }

    fn depth(&self) -> Count {
        Count::new(self.0.lists.len().saturating_sub(1))
    }

    fn context(&mut self) -> Lead {
        let depth = self.depth();
        let state = &mut self.0;
        match state.item.take() {
            Some(marker) => Lead::Item { depth, marker },
            None if state.quotes > Count::ZERO => Lead::Quote(state.quotes),
            None if state.lists.is_empty() => Lead::Paragraph,
            None => Lead::Item {
                depth,
                marker: Label::default(),
            },
        }
    }

    fn flush(&mut self) {
        if self
            .0
            .pieces
            .iter()
            .all(|piece| piece.text.as_str().trim().is_empty())
        {
            self.0.pieces.clear();
            return;
        }
        let lead = match self.0.lead.take() {
            Some(lead) => lead,
            None => self.context(),
        };
        let pieces = mem::take(&mut self.0.pieces);
        self.0.blocks.push(Block::Text { lead, pieces });
    }

    fn push_text(&mut self, text: &str) {
        if let Some(picture) = self.0.picture.as_mut() {
            picture.alt.push(text);
            return;
        }
        if let Some(code) = self.0.code.as_mut() {
            code.text.push(text);
            return;
        }
        let tone = self.tone();
        match self.0.links.last().cloned().flatten() {
            Some(goal) => self.0.pieces.push(Piece {
                text: Label::new(text),
                tone,
                goal: Some(goal),
            }),
            None if tone == Tone::Code => self.0.pieces.push(Piece {
                text: Label::new(text),
                tone,
                goal: None,
            }),
            None => self.0.pieces.extend(Words(text).pieces(tone)),
        }
    }

    fn html(&mut self, html: &str) {
        if let Some(image) = IMG.as_ref() {
            for found in image.captures_iter(html) {
                let Some(link) = found
                    .get(1)
                    .and_then(|source| WebLink::new(source.as_str()))
                else {
                    continue;
                };
                let tag = found.get(0).map_or("", |tag| tag.as_str());
                let alt = ALT
                    .as_ref()
                    .and_then(|alt| alt.captures(tag))
                    .and_then(|alt| alt.get(1))
                    .map_or("", |alt| alt.as_str());
                self.flush();
                self.0.blocks.push(Block::Picture {
                    link,
                    alt: Label::new(alt),
                });
            }
        }
        let text = TAG.as_ref().map_or_else(
            || html.to_owned(),
            |tag| tag.replace_all(html, "").into_owned(),
        );
        if !text.trim().is_empty() {
            self.push_text(&text);
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Heading { level, .. } => {
                self.flush();
                self.0.lead = Some(Lead::Heading(heading_level(level)));
            }
            Tag::BlockQuote(_) => {
                self.flush();
                self.0.quotes += Count::new(1);
            }
            Tag::CodeBlock(kind) => {
                self.flush();
                let language = match kind {
                    CodeBlockKind::Fenced(info) => {
                        Label::new(info.split_whitespace().next().unwrap_or_default())
                    }
                    CodeBlockKind::Indented => Label::default(),
                };
                self.0.code = Some(CodeDraft {
                    language,
                    text: Gathered::default(),
                });
            }
            Tag::List(start) => {
                self.flush();
                self.0.lists.push(match start {
                    Some(first) => Numbering::Next(Count::new(usize::try_from(first).unwrap_or(1))),
                    None => Numbering::Bullets,
                });
            }
            Tag::Item => {
                self.flush();
                let marker = match self.0.lists.last_mut() {
                    Some(Numbering::Next(number)) => {
                        let marker = format!("{number}.");
                        *number += Count::new(1);
                        marker
                    }
                    _ => "\u{2022}".to_owned(),
                };
                self.0.item = Some(Label::new(marker));
            }
            Tag::Emphasis => self.0.tones.push(Tone::Emphasis),
            Tag::Strong => self.0.tones.push(Tone::Strong),
            Tag::Strikethrough => self.0.tones.push(Tone::Struck),
            Tag::Link { dest_url, .. } => {
                self.0.links.push(WebLink::new(&dest_url).map(Goal::Web));
            }
            Tag::Image { dest_url, .. } => {
                self.0.picture = WebLink::new(&dest_url).map(|link| PictureDraft {
                    link,
                    alt: Gathered::default(),
                });
            }
            Tag::TableRow | Tag::TableHead => {
                self.flush();
                self.0.cells.clear();
            }
            Tag::TableCell => self.0.pieces.clear(),
            _ => {}
        }
    }

    fn end_row(&mut self) {
        let cells = mem::take(&mut self.0.cells);
        for (position, cell) in cells.into_iter().enumerate() {
            if position > 0 {
                self.0.pieces.push(Piece {
                    text: Label::new(" \u{2502} "),
                    tone: Tone::Struck,
                    goal: None,
                });
            }
            self.0.pieces.extend(cell.0);
        }
        self.0.lead = Some(Lead::Paragraph);
        self.flush();
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph | TagEnd::Heading(_) | TagEnd::Item => self.flush(),
            TagEnd::BlockQuote(_) => {
                self.flush();
                self.0.quotes = Count::new(self.0.quotes.get().saturating_sub(1));
            }
            TagEnd::CodeBlock => {
                if let Some(code) = self.0.code.take() {
                    self.0.blocks.push(Block::Code {
                        language: code.language,
                        text: code.text.label(),
                    });
                }
            }
            TagEnd::List(_) => {
                self.flush();
                self.0.lists.pop();
            }
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough => {
                self.0.tones.pop();
            }
            TagEnd::Link => {
                self.0.links.pop();
            }
            TagEnd::Image => {
                if let Some(picture) = self.0.picture.take() {
                    self.flush();
                    self.0.blocks.push(Block::Picture {
                        link: picture.link,
                        alt: picture.alt.label(),
                    });
                }
            }
            TagEnd::TableCell => {
                let cell = Cell(mem::take(&mut self.0.pieces));
                self.0.cells.push(cell);
            }
            TagEnd::TableRow | TagEnd::TableHead => self.end_row(),
            _ => {}
        }
    }

    fn event(&mut self, event: Event<'_>) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(text) => self.push_text(&text),
            Event::Code(code) => {
                let goal = self.0.links.last().cloned().flatten();
                self.0.pieces.push(Piece {
                    text: Label::new(&*code),
                    tone: Tone::Code,
                    goal,
                });
            }
            Event::Html(html) | Event::InlineHtml(html) => self.html(&html),
            Event::SoftBreak => self.push_text(" "),
            Event::HardBreak => self.0.pieces.push(Piece {
                text: Label::new("\n"),
                tone: Tone::Plain,
                goal: None,
            }),
            Event::Rule => {
                self.flush();
                self.0.blocks.push(Block::Rule);
            }
            Event::TaskListMarker(done) => self.push_text(if done { "[x] " } else { "[ ] " }),
            _ => {}
        }
    }
}

pub(crate) fn parse(text: &Prose) -> Vec<Block> {
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let mut reader = Reader::default();
    for event in Parser::new_ext(text.as_str(), options) {
        reader.event(event);
    }
    reader.flush();
    reader.0.blocks
}

#[cfg(test)]
mod tests;

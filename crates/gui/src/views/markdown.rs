use std::mem;
use std::rc::Rc;

use strum::VariantArray;

use domain::{Prose, RelativePath, WebLink};
use index::{Contents, Parsers};
use ui::{Canvas, Count, Label, Px, Rect, Run};

use crate::action::Action;
use crate::diffs::Loading;
use crate::grid::{self, Gutter};
use crate::ids;
use crate::markdown::{self, Block, Goal, Lead, Piece, Tone};
use crate::model::Model;
use crate::review::ReviewAct;
use crate::theme::{
    ACCENT, BLOCK_GAP, BORDER, CODE_TEXT, EMPHASIS_TEXT, FIELD, LABEL_PADDING, MARKDOWN_GUESS,
    PICTURE_TALLEST, PIXEL, RED, STRONG_TEXT, TEXT, WEAK,
};
use crate::widgets::{Container, Frame};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Literal(&'static str);

const PLAIN: Literal = Literal("txt");

#[derive(Clone, Copy, Debug, PartialEq, Eq, VariantArray)]
enum Fence {
    Rust,
    Rs,
    Python,
    Py,
    JavaScript,
    Js,
    Jsx,
    TypeScript,
    Ts,
    Tsx,
    Clang,
    Header,
    Cpp,
    Odin,
}

impl Fence {
    const fn name(self) -> Literal {
        Literal(match self {
            Self::Rust => "rust",
            Self::Rs => "rs",
            Self::Python => "python",
            Self::Py => "py",
            Self::JavaScript => "javascript",
            Self::Js => "js",
            Self::Jsx => "jsx",
            Self::TypeScript => "typescript",
            Self::Ts => "ts",
            Self::Tsx => "tsx",
            Self::Clang => "c",
            Self::Header => "h",
            Self::Cpp => "cpp",
            Self::Odin => "odin",
        })
    }

    const fn extension(self) -> Literal {
        Literal(match self {
            Self::Rust | Self::Rs => "rs",
            Self::Python | Self::Py => "py",
            Self::JavaScript | Self::Js | Self::Jsx => "js",
            Self::TypeScript | Self::Ts | Self::Tsx => "ts",
            Self::Clang | Self::Header | Self::Cpp => "c",
            Self::Odin => "odin",
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Word,
    Space,
    Break,
}

#[derive(Clone, Debug)]
struct Token {
    text: Label,
    tone: Tone,
    goal: Option<Count>,
    kind: Kind,
}

impl Token {
    fn columns(&self) -> Count {
        Count::new(self.text.as_str().chars().count())
    }
}

struct Tokens {
    all: Vec<Token>,
    goals: Vec<Goal>,
}

fn tokens(pieces: &[Piece]) -> Tokens {
    let mut all = Vec::new();
    let mut goals = Vec::new();
    for piece in pieces {
        let goal = piece.goal.clone().map(|goal| {
            goals.push(goal);
            Count::new(goals.len() - 1)
        });
        let mut word = String::new();
        let flush = |gathered: &mut String, into: &mut Vec<Token>| {
            if !gathered.is_empty() {
                into.push(Token {
                    text: Label::new(mem::take(gathered)),
                    tone: piece.tone,
                    goal,
                    kind: Kind::Word,
                });
            }
        };
        for character in piece.text.as_str().chars() {
            match character {
                '\n' => {
                    flush(&mut word, &mut all);
                    all.push(Token {
                        text: Label::default(),
                        tone: piece.tone,
                        goal,
                        kind: Kind::Break,
                    });
                }
                ' ' | '\t' => {
                    flush(&mut word, &mut all);
                    all.push(Token {
                        text: Label::new(" "),
                        tone: piece.tone,
                        goal,
                        kind: Kind::Space,
                    });
                }
                other => word.push(other),
            }
        }
        flush(&mut word, &mut all);
    }
    Tokens { all, goals }
}

fn wrap(tokens: &[Token], width: Count) -> Vec<Vec<Token>> {
    let width = width.get().max(8);
    let mut lines: Vec<Vec<Token>> = vec![Vec::new()];
    let mut used = 0;
    for token in tokens {
        let columns = token.columns().get();
        match token.kind {
            Kind::Break => {
                lines.push(Vec::new());
                used = 0;
            }
            Kind::Space => {
                if used > 0 && used < width {
                    if let Some(line) = lines.last_mut() {
                        line.push(token.clone());
                    }
                    used += 1;
                }
            }
            Kind::Word => {
                if used > 0 && used + columns > width {
                    if let Some(line) = lines.last_mut() {
                        while line.last().is_some_and(|last| last.kind == Kind::Space) {
                            line.pop();
                        }
                    }
                    lines.push(Vec::new());
                    used = 0;
                }
                let characters: Vec<char> = token.text.as_str().chars().collect();
                for chunk in characters.chunks(width) {
                    if used > 0 && used + chunk.len() > width {
                        lines.push(Vec::new());
                        used = 0;
                    }
                    if let Some(line) = lines.last_mut() {
                        line.push(Token {
                            text: Label::new(chunk.iter().collect::<String>()),
                            ..token.clone()
                        });
                    }
                    used += chunk.len();
                }
            }
        }
    }
    lines
}

const fn tone_color(tone: Tone, base: ui::Color) -> ui::Color {
    match tone {
        Tone::Plain => base,
        Tone::Strong => STRONG_TEXT,
        Tone::Emphasis => EMPHASIS_TEXT,
        Tone::Code => CODE_TEXT,
        Tone::Struck => WEAK,
    }
}

fn resolve(model: &Model, goal: &Goal) -> Option<WebLink> {
    match goal {
        Goal::Web(link) => Some(link.clone()),
        Goal::Forge(linked) => model.review.open.as_ref()?.link(linked),
    }
}

struct Writer<'model> {
    model: &'model Model,
    key: Label,
    width: Count,
}

impl Writer<'_> {
    fn line(
        &self,
        frame: &mut Frame<'_>,
        lead: Vec<Run>,
        line: &[Token],
        goals: &[Goal],
        base: ui::Color,
        number: Count,
    ) {
        frame.start(Container::FillRow);
        if !lead.is_empty() {
            frame.text_line(lead);
        }
        let mut at = 0;
        while let Some(first) = line.get(at) {
            let goal = first.goal;
            let mut runs = Vec::new();
            while let Some(token) = line.get(at).filter(|token| token.goal == goal) {
                let color = if goal.is_some() {
                    ACCENT
                } else {
                    tone_color(token.tone, base)
                };
                runs.push(Run::new(token.text.clone(), color));
                at += 1;
            }
            match goal.and_then(|goal| goals.get(goal.get())) {
                Some(found) => {
                    let target = ids::MD_LINK.with(&Label::new(format!(
                        "{}/{}/{}",
                        self.key.as_str(),
                        goal.map_or(0, Count::get),
                        number.get()
                    )));
                    if frame.inline_button(runs, target).clicked()
                        && let Some(link) = resolve(self.model, found)
                    {
                        frame.push(Action::Review(ReviewAct::OpenLink(link)));
                    }
                }
                None => frame.text_line(runs),
            }
        }
        frame.finish();
    }

    fn text(&self, frame: &mut Frame<'_>, lead: &Lead, pieces: &[Piece], number: &mut Count) {
        let (indent, marker, base) = match lead {
            Lead::Paragraph => (Count::ZERO, Label::default(), TEXT),
            Lead::Heading(_) => (Count::ZERO, Label::default(), ACCENT),
            Lead::Item { depth, marker } => (
                Count::new(depth.get() * 2),
                Label::new(format!("{} ", marker.as_str())),
                TEXT,
            ),
            Lead::Quote(depth) => (
                Count::ZERO,
                Label::new("\u{2502} ".repeat(depth.get())),
                WEAK,
            ),
        };
        let lead_columns = indent.get() + marker.as_str().chars().count();
        let found = tokens(pieces);
        let lines = wrap(
            &found.all,
            Count::new(self.width.get().saturating_sub(lead_columns)),
        );
        for (position, line) in lines.iter().enumerate() {
            let lead_text = if position == 0 || matches!(lead, Lead::Quote(_)) {
                format!("{}{}", " ".repeat(indent.get()), marker.as_str())
            } else {
                " ".repeat(lead_columns)
            };
            let runs = if lead_text.is_empty() {
                Vec::new()
            } else {
                vec![Run::new(lead_text, WEAK)]
            };
            self.line(frame, runs, line, &found.goals, base, *number);
            *number += Count::new(1);
        }
    }

    fn code(&self, frame: &mut Frame<'_>, language: &Label, text: &Label, number: Count) {
        let extension = Fence::VARIANTS
            .iter()
            .find(|fence| fence.name().0.eq_ignore_ascii_case(language.as_str()))
            .map_or(PLAIN.0, |fence| fence.extension().0);
        let source = Parsers::default().parse(
            RelativePath::new(&format!("snippet.{extension}")),
            &Contents::new(text.as_str()),
        );
        let Some(span) = source.text().span() else {
            return;
        };
        let grid = Rc::new(grid::build(&source, span, Gutter::Bare));
        let row = frame.row_height();
        let font = frame.metrics.font;
        let rows = Count::new(span.lines().count());
        let pad = frame.cell_width() / 2;
        let draw = move |canvas: &mut Canvas<'_>, rect: Rect| {
            canvas.rect(rect, FIELD);
            canvas.grid(ui::Point::new(rect.left + pad, rect.top + pad), font, &grid);
        };
        frame.custom_block(
            Box::new(draw),
            row * rows + pad * 2,
            ids::MD_CODE.id().with(self.key.as_str()).nth(number.get()),
        );
    }

    fn picture(&self, frame: &mut Frame<'_>, link: &WebLink, alt: &Label, number: Count) {
        let target = ids::PICTURE.with(&Label::new(format!(
            "{}/{}",
            self.key.as_str(),
            number.get()
        )));
        match self.model.review.pictures.get(link) {
            None => {
                frame.push(Action::Review(ReviewAct::NeedPicture(link.clone())));
                frame.text_line(vec![Run::new(
                    format!("[picture {}] loading...", alt.as_str()),
                    WEAK,
                )]);
            }
            Some(Loading::Asking) => {
                frame.text_line(vec![Run::new(
                    format!("[picture {}] loading...", alt.as_str()),
                    WEAK,
                )]);
            }
            Some(Loading::Failed(why)) => {
                let runs = vec![Run::new(
                    format!("[picture {}] {}", alt.as_str(), why.as_str()),
                    RED,
                )];
                if frame.inline_button(runs, target).clicked() {
                    frame.push(Action::Review(ReviewAct::OpenLink(link.clone())));
                }
            }
            Some(Loading::Got(picture)) => {
                let room = frame.cell_width() * self.width;
                let size = picture.size;
                let wide = size.width.max(PIXEL);
                let mut width = size.width.min(room);
                let mut height = Px::of_wide(size.height.wide() * width.wide() / wide.wide());
                if height > PICTURE_TALLEST {
                    width =
                        Px::of_wide(width.wide() * PICTURE_TALLEST.wide() / height.wide().max(1));
                    height = PICTURE_TALLEST;
                }
                let shown = Rc::clone(picture);
                let draw = move |canvas: &mut Canvas<'_>, rect: Rect| {
                    let area = Rect::new(rect.left, rect.top, width, height);
                    canvas.picture(area, &shown);
                    canvas.outline(area, PIXEL, BORDER);
                };
                if frame
                    .custom_block(Box::new(draw), height, target.id())
                    .clicked()
                {
                    frame.push(Action::Review(ReviewAct::OpenLink(link.clone())));
                }
            }
        }
    }
}

pub(super) fn markdown(model: &Model, frame: &mut Frame<'_>, written: &Prose, key: &Label) {
    let id = ids::MARKDOWN.id().with(key.as_str());
    let width = frame.ui.placement(id).map_or(MARKDOWN_GUESS, |placement| {
        let inner = placement.rect.width - LABEL_PADDING * 2;
        Count::new(
            usize::try_from(inner.ratio(frame.cell_width().max(PIXEL)))
                .unwrap_or(0)
                .saturating_sub(1),
        )
    });
    let writer = Writer {
        model,
        key: key.clone(),
        width,
    };
    frame.start(Container::Markdown(id));
    let mut number = Count::ZERO;
    let mut previous: Option<Block> = None;
    let listed = |block: &Block| {
        matches!(
            block,
            Block::Text {
                lead: Lead::Item { .. },
                ..
            }
        )
    };
    for block in markdown::parse(written) {
        if previous
            .as_ref()
            .is_some_and(|before| !(listed(before) && listed(&block)))
        {
            frame.spacer(BLOCK_GAP);
        }
        match &block {
            Block::Text { lead, pieces } => writer.text(frame, lead, pieces, &mut number),
            Block::Code { language, text } => writer.code(frame, language, text, number),
            Block::Picture { link, alt } => writer.picture(frame, link, alt, number),
            Block::Rule => {
                let draw = |canvas: &mut Canvas<'_>, rect: Rect| {
                    canvas.rect(
                        Rect::new(rect.left, rect.top + rect.height / 2, rect.width, PIXEL),
                        BORDER,
                    );
                };
                frame.custom_block(
                    Box::new(draw),
                    frame.row_height(),
                    ids::MD_CODE.id().with(key.as_str()).nth(number.get()),
                );
            }
        }
        number += Count::new(1);
        previous = Some(block);
    }
    frame.finish();
}

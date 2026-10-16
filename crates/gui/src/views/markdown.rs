use std::mem;

use strum::VariantArray;

use domain::{Prose, RelativePath, SourceFile, WebLink};
use index::{Contents, Parsers};
use ui::{Canvas, Count, Label, Px, Rect, Run, Select};

use crate::action::Action;
use crate::diffs::Loading;
use crate::grid::{self, Gutter};
use crate::ids;
use crate::markdown::{self, Align, Block, Cell, Goal, Lead, Piece, Row, Tone};
use crate::model::Model;
use crate::review::{LinkTarget, ReviewAct};
use crate::theme;
use crate::theme::{
    ACCENT, BLOCK_GAP, BORDER, CODE_TEXT, EMPHASIS_TEXT, LABEL_PADDING, MARKDOWN_GUESS,
    PICTURE_TALLEST, PIXEL, PROSE_COLUMNS, PROSE_LEADING, RED, STRONG_TEXT, TEXT, WEAK,
};
use crate::viewer::ViewerAct;
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
    Sql,
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
            Self::Sql => "sql",
        })
    }

    const fn extension(self) -> Literal {
        Literal(match self {
            Self::Rust | Self::Rs => "rs",
            Self::Python | Self::Py => "py",
            Self::JavaScript | Self::Js | Self::Jsx => "js",
            Self::TypeScript | Self::Ts => "ts",
            Self::Tsx => "tsx",
            Self::Clang | Self::Header | Self::Cpp => "c",
            Self::Odin => "odin",
            Self::Sql => "sql",
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
    let width = width.get().max(NARROWEST.get());
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

fn cell_columns(cell: &Cell) -> Count {
    Count::new(
        cell.pieces()
            .iter()
            .map(|piece| piece.text.as_str().chars().count())
            .sum(),
    )
}

const NARROWEST: Count = Count::new(8);

fn fit_widths(widths: &[Count], room: Count) -> Vec<Count> {
    let mut order: Vec<usize> = (0..widths.len()).collect();
    order.sort_by_key(|column| widths.get(*column).copied().unwrap_or(Count::ZERO));
    let mut shrunk = widths.to_vec();
    let mut left = room.get();
    for (placed, column) in order.iter().enumerate() {
        let width = widths.get(*column).copied().unwrap_or(Count::ZERO);
        let share = left / (order.len() - placed);
        let given = width.get().min(share.max(width.get().min(NARROWEST.get())));
        if let Some(slot) = shrunk.get_mut(*column) {
            *slot = Count::new(given);
        }
        left = left.saturating_sub(given);
    }
    shrunk
}

fn cell_lines(cell: &Cell, width: Count, tone: Tone) -> Vec<Vec<Piece>> {
    let pieces: Vec<Piece> = cell
        .pieces()
        .iter()
        .map(|piece| Piece {
            text: Label::new(piece.text.as_str().replace('\n', " ")),
            tone: if piece.tone == Tone::Plain {
                tone
            } else {
                piece.tone
            },
            goal: piece.goal.clone(),
        })
        .collect();
    let found = tokens(&pieces);
    wrap(&found.all, width)
        .into_iter()
        .map(|line| {
            line.into_iter()
                .map(|token| Piece {
                    text: token.text,
                    tone: token.tone,
                    goal: token
                        .goal
                        .and_then(|goal| found.goals.get(goal.get()).cloned()),
                })
                .collect()
        })
        .collect()
}

fn row_lines(row: &Row, widths: &[Count], aligns: &[Align], tone: Tone) -> Vec<Vec<Piece>> {
    let space = |columns: usize| Piece {
        text: Label::new(" ".repeat(columns)),
        tone: Tone::Plain,
        goal: None,
    };
    let empty = Cell::default();
    let cells: Vec<Vec<Vec<Piece>>> = widths
        .iter()
        .enumerate()
        .map(|(column, width)| cell_lines(row.cells().get(column).unwrap_or(&empty), *width, tone))
        .collect();
    let tall = cells.iter().map(Vec::len).max().unwrap_or(0).max(1);
    (0..tall)
        .map(|at| {
            let mut pieces = Vec::new();
            for (column, width) in widths.iter().enumerate() {
                if column > 0 {
                    pieces.push(Piece {
                        text: Label::new(" \u{2502} "),
                        tone: Tone::Struck,
                        goal: None,
                    });
                }
                let line = cells
                    .get(column)
                    .and_then(|lines| lines.get(at))
                    .cloned()
                    .unwrap_or_default();
                let used: usize = line
                    .iter()
                    .map(|piece| piece.text.as_str().chars().count())
                    .sum();
                let slack = width.get().saturating_sub(used);
                let before = match aligns.get(column).copied().unwrap_or(Align::Left) {
                    Align::Left => 0,
                    Align::Right => slack,
                    Align::Center => slack / 2,
                };
                pieces.push(space(before));
                pieces.extend(line);
                pieces.push(space(slack - before));
            }
            pieces
        })
        .collect()
}

fn grid_runs(grid: &ui::Grid, row: Count) -> Vec<Run> {
    let cells: Vec<Option<ui::Cell>> = grid.row(row).collect();
    let used = cells
        .iter()
        .rposition(Option::is_some)
        .map_or(0, |last| last + 1);
    let mut runs: Vec<Run> = Vec::new();
    for cell in cells.into_iter().take(used) {
        let (character, color) = cell.map_or((' ', TEXT), |cell| (cell.glyph.get(), cell.color));
        match runs.last_mut() {
            Some(last) if last.color == color || character == ' ' => {
                last.text = Label::new(format!("{}{character}", last.text.as_str()));
            }
            _ => runs.push(Run::new(String::from(character), color)),
        }
    }
    runs
}

const RULED_LEVELS: Count = Count::new(2);

#[derive(Clone, Copy)]
struct Look {
    color: ui::Color,
    size: ui::FontSize,
    opening: Select,
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
        look: Look,
        number: Count,
    ) {
        let Look {
            color: base,
            size,
            opening,
        } = look;
        frame.start(Container::FillRow);
        if !lead.is_empty() {
            let shown = match opening {
                Select::Wrapped => Select::Off,
                Select::Off | Select::On => Select::On,
            };
            frame.sized_line(lead, size, shown);
        }
        let mut select = opening;
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
                    let key = Label::new(format!(
                        "{}/{}/{}",
                        self.key.as_str(),
                        goal.map_or(0, Count::get),
                        number.get()
                    ));
                    let target = resolve(self.model, found).map(|web| self.model.link_target(web));
                    let control = match target {
                        Some(LinkTarget::Source(..)) => ids::MD_SOURCE,
                        Some(LinkTarget::Browser(_)) | None => ids::MD_LINK,
                    };
                    if frame
                        .sized_link(runs, control.with(&key), size, select)
                        .clicked()
                        && let Some(target) = target
                    {
                        frame.push(Action::Review(target.act()));
                    }
                }
                None => frame.sized_line(runs, size, select),
            }
            select = Select::On;
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
        let base_size = frame.metrics.font;
        let size = match lead {
            Lead::Heading(level) => theme::heading_font(base_size, *level),
            Lead::Paragraph | Lead::Item { .. } | Lead::Quote(_) => base_size,
        };
        let width = Count::new(
            usize::try_from(
                u64::try_from(self.width.min(PROSE_COLUMNS).get()).unwrap_or(0)
                    * u64::from(base_size.get())
                    / u64::from(size.get().max(1)),
            )
            .unwrap_or(0),
        );
        let lead_columns = indent.get() + marker.as_str().chars().count();
        let found = tokens(pieces);
        let lines = wrap(
            &found.all,
            Count::new(width.get().saturating_sub(lead_columns)),
        );
        for (position, line) in lines.iter().enumerate() {
            if position > 0 {
                frame.spacer(PROSE_LEADING);
            }
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
            self.line(
                frame,
                runs,
                line,
                &found.goals,
                Look {
                    color: base,
                    size,
                    opening: if position == 0 {
                        Select::On
                    } else {
                        Select::Wrapped
                    },
                },
                *number,
            );
            *number += Count::new(1);
        }
        if let Lead::Heading(level) = lead
            && level.get() <= RULED_LEVELS.get()
        {
            self.rule(frame, *number);
        }
    }

    fn rule(&self, frame: &mut Frame<'_>, number: Count) {
        let draw = |canvas: &mut Canvas<'_>, rect: Rect| {
            canvas.rect(
                Rect::new(rect.left, rect.top + rect.height / 2, rect.width, PIXEL),
                BORDER,
            );
        };
        frame.custom_block(
            Box::new(draw),
            BLOCK_GAP,
            ids::MD_CODE
                .id()
                .with(self.key.as_str())
                .with("rule")
                .nth(number.get()),
        );
    }

    fn table(
        &self,
        frame: &mut Frame<'_>,
        aligns: &[Align],
        header: &Row,
        rows: &[Row],
        number: &mut Count,
    ) {
        let columns = rows
            .iter()
            .chain([header])
            .map(|row| row.cells().len())
            .max()
            .unwrap_or(0);
        let natural: Vec<Count> = (0..columns)
            .map(|column| {
                rows.iter()
                    .chain([header])
                    .map(|row| row.cells().get(column).map_or(Count::ZERO, cell_columns))
                    .max()
                    .unwrap_or(Count::ZERO)
            })
            .collect();
        let between = Count::new(3 * columns.saturating_sub(1));
        let widths = fit_widths(
            &natural,
            Count::new(self.width.get().saturating_sub(between.get())),
        );
        let rule: Vec<Piece> = vec![Piece {
            text: Label::new(
                widths
                    .iter()
                    .map(|width| "\u{2500}".repeat(width.get()))
                    .collect::<Vec<_>>()
                    .join("\u{2500}\u{253c}\u{2500}"),
            ),
            tone: Tone::Struck,
            goal: None,
        }];
        let lines = row_lines(header, &widths, aligns, Tone::Strong)
            .into_iter()
            .chain([rule])
            .chain(
                rows.iter()
                    .flat_map(|row| row_lines(row, &widths, aligns, Tone::Plain)),
            );
        for pieces in lines {
            let found = tokens(&pieces);
            let line: Vec<Token> = found
                .all
                .into_iter()
                .filter(|token| token.kind != Kind::Break)
                .collect();
            self.line(
                frame,
                Vec::new(),
                &line,
                &found.goals,
                Look {
                    color: TEXT,
                    size: frame.metrics.font,
                    opening: Select::On,
                },
                *number,
            );
            *number += Count::new(1);
        }
    }

    fn code(&self, frame: &mut Frame<'_>, language: &Label, text: &Label, number: Count) {
        let source = snippet(language, text);
        let Some(span) = source.text().span() else {
            return;
        };
        let grid = grid::build(&source, span, Gutter::Bare);
        frame.start(Container::CodeBlock(
            ids::MD_CODE.id().with(self.key.as_str()).nth(number.get()),
        ));
        for row in 0..grid.rows().get() {
            frame.text_line(grid_runs(&grid, Count::new(row)));
        }
        frame.finish();
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
            Some(Loading::Got(animation)) => {
                let Some(shown) = animation.at(self.model.now) else {
                    return;
                };
                if let Some(next) = shown.next {
                    frame.push(Action::Review(ReviewAct::PictureWake(next)));
                }
                let room = frame.cell_width() * self.width;
                let size = animation.size();
                let wide = size.width.max(PIXEL);
                let mut width = size.width.min(room);
                let mut height = Px::of_wide(size.height.wide() * width.wide() / wide.wide());
                if height > PICTURE_TALLEST {
                    width =
                        Px::of_wide(width.wide() * PICTURE_TALLEST.wide() / height.wide().max(1));
                    height = PICTURE_TALLEST;
                }
                let shown = shown.picture;
                let draw = move |canvas: &mut Canvas<'_>, rect: Rect| {
                    let area = Rect::new(rect.left, rect.top, width, height);
                    canvas.picture(area, &shown);
                    canvas.outline(area, PIXEL, BORDER);
                };
                if frame
                    .custom_block(Box::new(draw), height, target.id())
                    .clicked()
                {
                    frame.push(Action::Viewer(ViewerAct::Open(link.clone())));
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
        if let Some(before) = previous.as_ref() {
            frame.spacer(if listed(before) && listed(&block) {
                PROSE_LEADING
            } else {
                BLOCK_GAP
            });
        }
        match &block {
            Block::Text { lead, pieces } => writer.text(frame, lead, pieces, &mut number),
            Block::Table {
                aligns,
                header,
                rows,
            } => writer.table(frame, aligns, header, rows, &mut number),
            Block::Code { language, text } => writer.code(frame, language, text, number),
            Block::Picture { link, alt } => writer.picture(frame, link, alt, number),
            Block::Rule => writer.rule(frame, number),
        }
        number += Count::new(1);
        previous = Some(block);
    }
    frame.finish();
}

fn snippet(language: &Label, text: &Label) -> SourceFile {
    let extension = Fence::VARIANTS
        .iter()
        .find(|fence| fence.name().0.eq_ignore_ascii_case(language.as_str()))
        .map_or(PLAIN.0, |fence| fence.extension().0);
    Parsers::default().parse(
        RelativePath::new(&format!("snippet.{extension}")),
        &Contents::new(text.as_str()),
    )
}

#[cfg(test)]
mod tests {
    use domain::HighlightClass;

    use super::*;

    #[test]
    fn a_tsx_fence_reads_its_markup() {
        let text = "const tile = <div className=\"tile\">{area}</div>;";
        let source = snippet(&Label::new("tsx"), &Label::new(text));
        let tag = source.highlights()[0]
            .iter()
            .find(|span| {
                text.get(span.start.value() as usize..span.end.value() as usize) == Some("div")
            })
            .map(|span| span.class);
        assert_eq!(tag, Some(HighlightClass::Property));
    }

    fn counts(widths: &[usize]) -> Vec<Count> {
        widths.iter().copied().map(Count::new).collect()
    }

    fn plain(lines: &[Vec<Piece>]) -> Vec<String> {
        lines
            .iter()
            .map(|line| line.iter().map(|piece| piece.text.as_str()).collect())
            .collect()
    }

    #[test]
    fn a_table_that_fits_keeps_its_widths_and_a_wide_one_shrinks_its_widest_columns() {
        assert_eq!(
            fit_widths(&counts(&[3, 10]), Count::new(20)),
            counts(&[3, 10])
        );
        assert_eq!(
            fit_widths(&counts(&[3, 40, 100]), Count::new(60)),
            counts(&[3, 28, 29])
        );
        assert_eq!(
            fit_widths(&counts(&[20, 20]), Count::new(10)),
            counts(&[8, 8])
        );
    }

    #[test]
    fn a_cell_wider_than_its_column_wraps_onto_more_lines_of_the_row() {
        let parsed = markdown::parse(&Prose::new(
            "| a | notes |\n|---|---|\n| 1 | one two three four five |",
        ));
        let [Block::Table { rows, aligns, .. }] = parsed.as_slice() else {
            panic!("{parsed:?}");
        };
        let row = rows.first().unwrap();
        assert_eq!(
            plain(&row_lines(row, &counts(&[1, 9]), aligns, Tone::Plain)),
            [
                "1 \u{2502} one two  ",
                "  \u{2502} three    ",
                "  \u{2502} four five",
            ]
        );
    }
}

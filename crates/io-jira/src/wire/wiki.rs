use std::mem;

use crate::wire::WireAttachment;

const ESCAPED: [char; 13] = [
    '\\', '`', '*', '_', '[', ']', '<', '>', '#', '!', '|', '~', '&',
];
const PICTURE_ENDINGS: [&str; 7] = [".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp", ".svg"];
const QUOTE: &str = "{quote}";
const PANEL: &str = "{panel";
const DROPPED: [&str; 2] = ["{color", "{anchor"];
const LANGUAGE_KEYS: [&str; 2] = ["language", "lang"];
const LEAST_FENCE: usize = 3;
const RULE_LEAST: usize = 4;
const BULLET: &str = "-";
const NUMBERED: &str = "1.";
const HARD_BREAK: &str = "  ";
const EN_DASH: &str = "\u{2013}";
const EM_DASH: &str = "\u{2014}";
const SOMEONE: &str = "someone";
const ACCOUNT: &str = "accountid:";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Spelling {
    Scheme,
    Mail,
    Bullet,
    BlockQuote,
    WindowsBreak,
}

impl Spelling {
    const fn name(self) -> &'static str {
        match self {
            Self::Scheme => "://",
            Self::Mail => "mailto:",
            Self::Bullet => "- ",
            Self::BlockQuote => "bq. ",
            Self::WindowsBreak => "\r\n",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Verbatim {
    Code,
    Plain,
}

impl Verbatim {
    const ALL: [Self; 2] = [Self::Code, Self::Plain];

    const fn name(self) -> &'static str {
        match self {
            Self::Code => "code",
            Self::Plain => "noformat",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Within {
    Text,
    Table,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Last {
    Blank,
    Paragraph,
    List,
    Table,
    Block,
}

struct Taken {
    text: String,
    length: usize,
}

impl Taken {
    fn of(text: impl Into<String>, length: usize) -> Self {
        Self {
            text: text.into(),
            length,
        }
    }
}

struct Fenced<'text> {
    before: &'text str,
    language: String,
    body: &'text str,
    after: &'text str,
}

enum Part<'text> {
    Line(&'text str),
    Toggle,
}

enum Target<'text> {
    Mention(Option<&'text str>),
    Attachment(&'text str),
    Anchor(&'text str),
    Web(&'text str),
}

fn escaped(character: char) -> String {
    if ESCAPED.contains(&character) {
        format!("\\{character}")
    } else {
        character.to_string()
    }
}

fn escaped_text(text: &str) -> String {
    text.chars().map(escaped).collect()
}

fn destination(url: &str) -> String {
    url.replace('<', "%3C")
        .replace('>', "%3E")
        .replace(' ', "%20")
}

fn longest_run(text: &str, mark: char) -> usize {
    text.split(|character| character != mark)
        .map(str::len)
        .max()
        .unwrap_or(0)
}

fn code_span(inner: &str, within: Within) -> String {
    let fence = "`".repeat(longest_run(inner, '`') + 1);
    let pad = if inner.starts_with('`') || inner.ends_with('`') {
        " "
    } else {
        ""
    };
    let inner = match within {
        Within::Text => inner.to_owned(),
        Within::Table => inner.replace('|', "\\|"),
    };
    format!("{fence}{pad}{inner}{pad}{fence}")
}

fn target_of(text: &str) -> Option<Target<'_>> {
    let text = text.trim();
    if let Some(who) = text.strip_prefix('~') {
        return Some(Target::Mention(
            (!who.starts_with(ACCOUNT) && !who.is_empty()).then_some(who),
        ));
    }
    if let Some(name) = text.strip_prefix('^') {
        return Some(Target::Attachment(name));
    }
    if let Some(anchor) = text.strip_prefix('#') {
        return Some(Target::Anchor(anchor));
    }
    (text.contains(Spelling::Scheme.name()) || text.starts_with(Spelling::Mail.name()))
        .then_some(Target::Web(text))
}

fn is_word(character: Option<&char>) -> bool {
    character.is_some_and(|found| found.is_alphanumeric())
}

fn find(text: &[char], from: usize, wanted: char) -> Option<usize> {
    (from..text.len()).find(|at| text.get(*at) == Some(&wanted))
}

fn gathered(text: &[char], from: usize, to: usize) -> String {
    text.get(from..to).unwrap_or_default().iter().collect()
}

struct Inline<'card> {
    attachments: &'card [WireAttachment],
    within: Within,
}

impl Inline<'_> {
    fn attachment(&self, name: &str) -> Option<&str> {
        self.attachments
            .iter()
            .find(|attachment| attachment.filename == name)
            .map(|attachment| attachment.content.as_str())
    }

    fn convert(&self, text: &[char]) -> String {
        let mut out = String::new();
        let mut at = 0;
        while let Some(character) = text.get(at) {
            if let Some(taken) = self.special(text, at) {
                out.push_str(&taken.text);
                at += taken.length.max(1);
            } else {
                out.push_str(&escaped(*character));
                at += 1;
            }
        }
        out
    }

    fn convert_str(&self, text: &str) -> String {
        self.convert(&text.chars().collect::<Vec<char>>())
    }

    fn special(&self, text: &[char], at: usize) -> Option<Taken> {
        match text.get(at)? {
            '\\' => self.backslash(text, at),
            '{' => self.brace(text, at),
            '[' => self.link(text, at),
            '!' => self.picture(text, at),
            '-' => dash(text, at).or_else(|| self.delimited(text, at)),
            _ => self.delimited(text, at),
        }
    }

    fn backslash(&self, text: &[char], at: usize) -> Option<Taken> {
        match text.get(at + 1)? {
            '\\' => Some(Taken::of(
                match self.within {
                    Within::Text => format!("{HARD_BREAK}\n"),
                    Within::Table => " ".to_owned(),
                },
                2,
            )),
            other => Some(Taken::of(escaped(*other), 2)),
        }
    }

    fn brace(&self, text: &[char], at: usize) -> Option<Taken> {
        if text.get(at + 1) == Some(&'{') {
            let close = (at + 2..text.len())
                .find(|end| text.get(*end) == Some(&'}') && text.get(end + 1) == Some(&'}'))?;
            let inner = gathered(text, at + 2, close);
            if inner.is_empty() {
                return None;
            }
            return Some(Taken::of(code_span(&inner, self.within), close + 2 - at));
        }
        let rest = gathered(text, at, text.len());
        if DROPPED.iter().any(|tag| rest.starts_with(tag)) {
            let close = find(text, at, '}')?;
            return Some(Taken::of("", close + 1 - at));
        }
        None
    }

    fn link(&self, text: &[char], at: usize) -> Option<Taken> {
        let close = find(text, at + 1, ']')?;
        let inside = gathered(text, at + 1, close);
        let length = close + 1 - at;
        let mut parts = inside.split('|');
        let first = parts.next()?;
        let (words, target) = match parts.next() {
            Some(second) => (Some(first), target_of(second)?),
            None => (None, target_of(first)?),
        };
        let words = words.filter(|words| !words.trim().is_empty());
        let shown = |fallback: &str| {
            words.map_or_else(|| escaped_text(fallback), |words| self.convert_str(words))
        };
        let markdown = match target {
            Target::Mention(who) => format!("**{}**", shown(who.unwrap_or(SOMEONE))),
            Target::Anchor(anchor) => shown(anchor),
            Target::Attachment(name) => match self.attachment(name) {
                Some(url) => format!("[{}](<{}>)", shown(name), destination(url)),
                None => shown(name),
            },
            Target::Web(url) => format!("[{}](<{}>)", shown(url), destination(url)),
        };
        Some(Taken::of(markdown, length))
    }

    fn picture(&self, text: &[char], at: usize) -> Option<Taken> {
        let first = text.get(at + 1)?;
        if first.is_whitespace() || *first == '!' {
            return None;
        }
        let close = find(text, at + 1, '!')?;
        let inside = gathered(text, at + 1, close);
        let source = inside.split('|').next()?.trim();
        let length = close + 1 - at;
        if source.contains(Spelling::Scheme.name()) {
            return Some(Taken::of(
                format!("![{}](<{}>)", escaped_text(source), destination(source)),
                length,
            ));
        }
        if let Some(url) = self.attachment(source) {
            return Some(Taken::of(
                format!("![{}](<{}>)", escaped_text(source), destination(url)),
                length,
            ));
        }
        let lower = source.to_lowercase();
        PICTURE_ENDINGS
            .iter()
            .any(|ending| lower.ends_with(ending))
            .then(|| Taken {
                text: format!("*{}*", escaped_text(source)),
                length,
            })
    }

    fn delimited(&self, text: &[char], at: usize) -> Option<Taken> {
        let mark = *text.get(at)?;
        let (width, wrap) = match mark {
            '*' => (1, "**"),
            '_' => (1, "*"),
            '-' => (1, "~~"),
            '+' | '^' | '~' => (1, ""),
            '?' if text.get(at + 1) == Some(&'?') => (2, "*"),
            _ => return None,
        };
        if at > 0 && is_word(text.get(at - 1)) {
            return None;
        }
        let first = text.get(at + width)?;
        if first.is_whitespace() || *first == mark {
            return None;
        }
        let closes = |end: usize| {
            (0..width).all(|step| text.get(end + step) == Some(&mark))
                && text
                    .get(end - 1)
                    .is_some_and(|before| !before.is_whitespace() && *before != mark)
                && !is_word(text.get(end + width))
        };
        let close = (at + width + 1..text.len()).find(|end| closes(*end))?;
        let inner = self.convert(text.get(at + width..close)?);
        if inner.is_empty() {
            return None;
        }
        Some(Taken::of(
            format!("{wrap}{inner}{wrap}"),
            close + width - at,
        ))
    }
}

fn dash(text: &[char], at: usize) -> Option<Taken> {
    let run = text
        .iter()
        .skip(at)
        .take_while(|character| **character == '-')
        .count();
    let spaced = |character: Option<&char>| character.is_none_or(|found| found.is_whitespace());
    let alone = (at == 0 || spaced(text.get(at - 1))) && spaced(text.get(at + run));
    match run {
        2 if alone => Some(Taken::of(EN_DASH, run)),
        3 if alone => Some(Taken::of(EM_DASH, run)),
        _ => None,
    }
}

fn opening(text: &str, name: &str) -> Option<usize> {
    let bare = format!("{{{name}}}");
    let with = format!("{{{name}:");
    [text.find(&bare), text.find(&with)]
        .into_iter()
        .flatten()
        .min()
}

fn language_of(parameters: &str) -> String {
    parameters
        .split('|')
        .find_map(|part| match part.split_once('=') {
            Some((key, value)) => LANGUAGE_KEYS.contains(&key.trim()).then_some(value.trim()),
            None => Some(part.trim()),
        })
        .unwrap_or_default()
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || "+#-_".contains(*character))
        .collect()
}

fn next_verbatim(text: &str) -> Option<Fenced<'_>> {
    let (start, kind) = Verbatim::ALL
        .iter()
        .filter_map(|kind| opening(text, kind.name()).map(|start| (start, *kind)))
        .min_by_key(|(start, _)| *start)?;
    let tag = text.get(start..)?;
    let tag_end = start + tag.find('}')? + 1;
    let parameters = text
        .get(start + 1 + kind.name().len()..tag_end - 1)?
        .trim_start_matches(':');
    let language = match kind {
        Verbatim::Code => language_of(parameters),
        Verbatim::Plain => String::new(),
    };
    let rest = text.get(tag_end..)?;
    let closing = format!("{{{}}}", kind.name());
    let (body, after) = rest.split_once(&closing).unwrap_or((rest, ""));
    Some(Fenced {
        before: text.get(..start)?,
        language,
        body,
        after,
    })
}

fn quote_tag(line: &str) -> Option<(&str, &str)> {
    let quote = line.find(QUOTE).map(|at| (at, at + QUOTE.len()));
    let panel = line.find(PANEL).and_then(|at| {
        let rest = line.get(at + PANEL.len()..)?;
        rest.starts_with([':', '}'])
            .then(|| {
                rest.find('}')
                    .map(|close| (at, at + PANEL.len() + close + 1))
            })
            .flatten()
    });
    let (start, end) = [quote, panel].into_iter().flatten().min()?;
    Some((line.get(..start)?, line.get(end..)?))
}

fn parts(text: &str) -> Vec<Part<'_>> {
    let mut parts = Vec::new();
    for line in text.split('\n') {
        let mut rest = line;
        let mut toggled = false;
        while let Some((before, after)) = quote_tag(rest) {
            if !before.trim().is_empty() {
                parts.push(Part::Line(before));
            }
            parts.push(Part::Toggle);
            toggled = true;
            rest = after;
        }
        if !toggled || !rest.trim().is_empty() {
            parts.push(Part::Line(rest));
        }
    }
    parts
}

fn heading(line: &str) -> Option<(usize, &str)> {
    let rest = line.strip_prefix('h')?;
    let mut characters = rest.chars();
    let level = characters.next()?.to_digit(10)?;
    let text = characters.as_str().strip_prefix('.')?;
    let level = usize::try_from(level)
        .ok()
        .filter(|level| (1..=6).contains(level))?;
    (text.is_empty() || text.starts_with(' ')).then(|| (level, text.trim()))
}

fn list_item(line: &str) -> Option<(&str, &str)> {
    let marker_length = if line.starts_with(Spelling::Bullet.name()) {
        1
    } else {
        line.chars()
            .take_while(|character| *character == '*' || *character == '#')
            .count()
    };
    let marker = line
        .get(..marker_length)
        .filter(|marker| !marker.is_empty())?;
    let rest = line.get(marker_length..)?.strip_prefix(' ')?.trim();
    (!rest.is_empty()).then_some((marker, rest))
}

fn table_cells(line: &str) -> Vec<String> {
    let mut cells = Vec::new();
    let mut cell = String::new();
    let mut depth = 0_usize;
    for character in line.trim().trim_start_matches('|').chars() {
        match character {
            '[' | '{' => depth += 1,
            ']' | '}' => depth = depth.saturating_sub(1),
            _ => {}
        }
        if character == '|' && depth == 0 {
            if !cell.is_empty() || cells.is_empty() {
                cells.push(mem::take(&mut cell));
            }
            continue;
        }
        cell.push(character);
    }
    if !cell.trim().is_empty() {
        cells.push(cell);
    }
    cells
}

fn guarded(line: String) -> String {
    let digits = line.chars().take_while(char::is_ascii_digit).count();
    let after = line.chars().nth(digits);
    if digits > 0 && matches!(after, Some('.' | ')')) {
        let (number, rest) = line.split_at(digits);
        return format!("{number}\\{rest}");
    }
    if line.starts_with(['-', '+', '=']) {
        return format!("\\{line}");
    }
    line
}

struct Writer<'card> {
    attachments: &'card [WireAttachment],
    lines: Vec<String>,
    lists: Vec<usize>,
    last: Last,
    quoted: bool,
}

impl<'card> Writer<'card> {
    const fn new(attachments: &'card [WireAttachment]) -> Self {
        Self {
            attachments,
            lines: Vec::new(),
            lists: Vec::new(),
            last: Last::Blank,
            quoted: false,
        }
    }

    const fn inline(&self, within: Within) -> Inline<'card> {
        Inline {
            attachments: self.attachments,
            within,
        }
    }

    const fn prefix(&self) -> &'static str {
        if self.quoted { "> " } else { "" }
    }

    fn push(&mut self, line: &str, last: Last) {
        self.lines.push(format!("{}{line}", self.prefix()));
        self.last = last;
    }

    fn blank(&mut self) {
        if self.last != Last::Blank {
            self.lines.push(self.prefix().trim_end().to_owned());
            self.last = Last::Blank;
        }
        self.lists.clear();
    }

    fn apart(&mut self, next: Last) {
        if self.last != next {
            self.blank();
        }
    }

    fn prose(&mut self, text: &str) {
        for part in parts(text) {
            match part {
                Part::Toggle => {
                    if self.last != Last::Blank {
                        self.lines.push(String::new());
                        self.last = Last::Blank;
                    }
                    self.lists.clear();
                    self.quoted = !self.quoted;
                }
                Part::Line(line) => self.line(line.trim()),
            }
        }
    }

    fn line(&mut self, line: &str) {
        let text = self.inline(Within::Text);
        if line.is_empty() {
            self.blank();
        } else if let Some((level, rest)) = heading(line) {
            self.apart(Last::Block);
            self.push(
                &format!("{} {}", "#".repeat(level), text.convert_str(rest)),
                Last::Block,
            );
        } else if let Some(rest) = line.strip_prefix(Spelling::BlockQuote.name()) {
            self.apart(Last::Block);
            self.push(&format!("> {}", text.convert_str(rest)), Last::Block);
        } else if line.len() >= RULE_LEAST && line.chars().all(|character| character == '-') {
            self.apart(Last::Block);
            self.push("***", Last::Block);
        } else if let Some((marker, rest)) = list_item(line) {
            self.item(marker, rest);
        } else if line.starts_with('|') {
            self.row(line);
        } else {
            self.paragraph(line);
        }
    }

    fn paragraph(&mut self, line: &str) {
        let converted = guarded(self.inline(Within::Text).convert_str(line));
        if self.last == Last::Paragraph {
            if let Some(previous) = self.lines.last_mut() {
                previous.push_str(HARD_BREAK);
            }
        } else {
            self.blank();
        }
        self.push(&converted, Last::Paragraph);
    }

    fn item(&mut self, marker: &str, rest: &str) {
        if self.last != Last::List {
            self.blank();
        }
        let depth = marker.chars().count();
        self.lists.truncate(depth - 1);
        while self.lists.len() < depth - 1 {
            self.lists.push(BULLET.len() + 1);
        }
        let shown = if marker.ends_with('#') {
            NUMBERED
        } else {
            BULLET
        };
        let indent = " ".repeat(self.lists.iter().sum());
        self.lists.push(shown.len() + 1);
        let converted = self.inline(Within::Text).convert_str(rest);
        self.push(&format!("{indent}{shown} {converted}"), Last::List);
    }

    fn row(&mut self, line: &str) {
        let cells: Vec<String> = table_cells(line)
            .iter()
            .map(|cell| self.inline(Within::Table).convert_str(cell.trim()))
            .collect();
        let row = format!("| {} |", cells.join(" | "));
        if self.last == Last::Table {
            self.push(&row, Last::Table);
            return;
        }
        self.blank();
        let delimiter = format!("|{}", " --- |".repeat(cells.len().max(1)));
        self.push(&row, Last::Table);
        self.push(&delimiter, Last::Table);
    }

    fn verbatim(&mut self, language: &str, body: &str) {
        let body = body.trim_start_matches(['\r', '\n']).trim_end();
        if body.is_empty() {
            return;
        }
        self.apart(Last::Blank);
        let fence = "`".repeat(longest_run(body, '`').max(LEAST_FENCE - 1) + 1);
        self.push(&format!("{fence}{language}"), Last::Block);
        for line in body.lines() {
            self.push(line, Last::Block);
        }
        self.push(&fence, Last::Block);
        self.blank();
    }

    fn finish(self) -> String {
        self.lines.join("\n").trim().to_owned()
    }
}

pub(crate) fn markdown(wiki: &str, attachments: &[WireAttachment]) -> String {
    let text = wiki
        .replace(Spelling::WindowsBreak.name(), "\n")
        .replace('\r', "\n");
    let mut writer = Writer::new(attachments);
    let mut rest = text.as_str();
    while let Some(verbatim) = next_verbatim(rest) {
        writer.prose(verbatim.before);
        writer.verbatim(&verbatim.language, verbatim.body);
        rest = verbatim.after;
    }
    writer.prose(rest);
    writer.finish()
}

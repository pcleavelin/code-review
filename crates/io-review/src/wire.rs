use strum::VariantArray;

pub(crate) const VERSION: &str = "review 2";

pub(crate) const SHARED_DIRECTORY: &str = "code-review";

pub(crate) const REVIEWS: &str = "reviews";

pub(crate) const TREES: &str = "trees";

pub(crate) const EXTENSION: &str = "review";

const SEPARATOR: char = '\t';

#[derive(Clone, Copy, Debug, PartialEq, Eq, VariantArray)]
pub(crate) enum Record {
    Mark,
    Draft,
    Looked,
}

impl Record {
    const fn name(self) -> &'static str {
        match self {
            Self::Mark => "mark",
            Self::Draft => "draft",
            Self::Looked => "looked",
        }
    }

    fn named(word: &str) -> Option<Self> {
        Self::VARIANTS
            .iter()
            .copied()
            .find(|record| record.name() == word)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, VariantArray)]
pub(crate) enum WireSide {
    Old,
    New,
}

impl WireSide {
    const fn name(self) -> &'static str {
        match self {
            Self::Old => "old",
            Self::New => "new",
        }
    }

    fn named(word: &str) -> Option<Self> {
        Self::VARIANTS
            .iter()
            .copied()
            .find(|side| side.name() == word)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WireMark {
    pub(crate) hash: u64,
    pub(crate) commit: String,
    pub(crate) path: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WireDraft {
    pub(crate) id: u32,
    pub(crate) side: WireSide,
    pub(crate) line: u32,
    pub(crate) path: String,
    pub(crate) text: String,
    pub(crate) reply: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct WireReview {
    pub(crate) marks: Vec<WireMark>,
    pub(crate) drafts: Vec<WireDraft>,
    pub(crate) looked: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum WireFault {
    Version(String),
    Record(String),
    Field(String),
}

fn escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace(SEPARATOR, "\\t")
}

fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut characters = value.chars();
    while let Some(character) = characters.next() {
        if character == '\\' {
            match characters.next() {
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push(SEPARATOR),
                Some(escaped) => out.push(escaped),
                None => out.push('\\'),
            }
        } else {
            out.push(character);
        }
    }
    out
}

fn mark(fields: &[&str]) -> Option<WireMark> {
    let [hash, commit, path] = fields else {
        return None;
    };
    Some(WireMark {
        hash: u64::from_str_radix(hash, 16).ok()?,
        commit: (*commit).to_owned(),
        path: unescape(path),
    })
}

fn draft(fields: &[&str]) -> Option<WireDraft> {
    let (reply, fields) = match fields {
        [id, side, line, path, text] => (None, [*id, *side, *line, *path, *text]),
        [id, side, line, path, text, reply] => {
            (Some(unescape(reply)), [*id, *side, *line, *path, *text])
        }
        _ => return None,
    };
    let [id, side, line, path, text] = fields;
    Some(WireDraft {
        id: id.parse().ok()?,
        side: WireSide::named(side)?,
        line: line.parse().ok()?,
        path: unescape(path),
        text: unescape(text),
        reply,
    })
}

pub(crate) fn parse(text: &str) -> Result<WireReview, WireFault> {
    let mut lines = text.lines().filter(|line| !line.is_empty());
    match lines.next() {
        Some(first) if first == VERSION => {}
        other => return Err(WireFault::Version(other.unwrap_or_default().to_owned())),
    }
    let mut review = WireReview::default();
    for line in lines {
        let fields: Vec<&str> = line.split(SEPARATOR).collect();
        let Some((record, rest)) = fields.split_first() else {
            continue;
        };
        let bad = || WireFault::Field(line.to_owned());
        match Record::named(record) {
            Some(Record::Mark) => review.marks.push(mark(rest).ok_or_else(bad)?),
            Some(Record::Draft) => review.drafts.push(draft(rest).ok_or_else(bad)?),
            Some(Record::Looked) => {
                let [commit] = rest else {
                    return Err(bad());
                };
                review.looked = Some((*commit).to_owned());
            }
            None => return Err(WireFault::Record((*record).to_owned())),
        }
    }
    Ok(review)
}

pub(crate) fn render(review: &WireReview) -> String {
    let tab = SEPARATOR;
    let marks = review.marks.iter().map(|mark| {
        format!(
            "{}{tab}{:016x}{tab}{}{tab}{}",
            Record::Mark.name(),
            mark.hash,
            mark.commit,
            escape(&mark.path)
        )
    });
    let drafts = review.drafts.iter().map(|draft| {
        let reply = draft
            .reply
            .as_ref()
            .map_or_else(String::new, |reply| format!("{tab}{}", escape(reply)));
        format!(
            "{}{tab}{}{tab}{}{tab}{}{tab}{}{tab}{}{reply}",
            Record::Draft.name(),
            draft.id,
            draft.side.name(),
            draft.line,
            escape(&draft.path),
            escape(&draft.text)
        )
    });
    let mut lines: Vec<String> = vec![VERSION.to_owned()];
    lines.extend(
        review
            .looked
            .iter()
            .map(|commit| format!("{}{tab}{commit}", Record::Looked.name())),
    );
    lines.extend(marks);
    lines.extend(drafts);
    lines.push(String::new());
    lines.join("\n")
}

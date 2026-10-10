use std::fmt;
use std::path::PathBuf;

use domain::Line;
use platform::BUNDLED_FAMILY;
use ui::{Count, Label};

use crate::model::HitsShown;
use crate::review::ReviewStatus;
use crate::text::{Counted, Noun};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct IndexCounts {
    pub(crate) files: Count,
    pub(crate) symbols: Count,
}

impl fmt::Display for IndexCounts {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} files, {} symbols indexed",
            self.files, self.symbols
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tone {
    Plain,
    Done,
    Warning,
    Problem,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Status {
    Nothing,
    Indexed(IndexCounts),
    LayoutUnsaved(Label),
    SettingsUnsaved(Label),
    FontNotUsed(FontError),
    CommandRejected(Label),
    CommandFailed(Label),
    RegexRefused(Label),
    Hits {
        count: Count,
        pattern: Label,
        shown: HitsShown,
    },
    NoDefinitionOf(Label),
    DefinedOutside {
        file: PathBuf,
        line: Line,
    },
    NoDefinition,
    LookingUp(Label),
    Reindexing,
    Reindexed(IndexCounts),
    ReindexFailed,
    Review(ReviewStatus),
}

impl Status {
    fn hits(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Hits {
                count,
                pattern,
                shown: HitsShown::All,
            } => write!(
                formatter,
                "{} for /{}/",
                Counted::new(*count, Noun::Hit),
                pattern.as_str()
            ),
            Self::Hits {
                count,
                pattern,
                shown: HitsShown::First,
            } => write!(
                formatter,
                "the first {count} hits for /{}/; the search stops there, a narrower regex finds the rest",
                pattern.as_str()
            ),
            _ => Ok(()),
        }
    }

    pub(crate) const fn tone(&self) -> Tone {
        match self {
            Self::Reindexed(_) => Tone::Done,
            Self::Hits {
                shown: HitsShown::First,
                ..
            }
            | Self::NoDefinitionOf(_)
            | Self::NoDefinition => Tone::Warning,
            Self::LayoutUnsaved(_)
            | Self::SettingsUnsaved(_)
            | Self::FontNotUsed(_)
            | Self::CommandRejected(_)
            | Self::CommandFailed(_)
            | Self::RegexRefused(_)
            | Self::ReindexFailed => Tone::Problem,
            Self::Review(status) => status.tone(),
            _ => Tone::Plain,
        }
    }

    pub(crate) const fn is_indexed(&self) -> bool {
        matches!(self, Self::Indexed(_))
    }

    pub(crate) fn line(&self) -> Label {
        let text = self.to_string();
        Label::new(
            text.lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .collect::<Vec<_>>()
                .join(" "),
        )
    }
}

impl fmt::Display for Status {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Nothing => Ok(()),
            Self::Indexed(counts) => write!(formatter, "{counts}"),
            Self::LayoutUnsaved(error) => {
                write!(formatter, "layout not saved: {}", error.as_str())
            }
            Self::SettingsUnsaved(error) => {
                write!(formatter, "settings not saved: {}", error.as_str())
            }
            Self::FontNotUsed(error) => error.fmt(formatter),
            Self::CommandRejected(first) => formatter.write_str(first.as_str()),
            Self::CommandFailed(error) => write!(formatter, "error: {}", error.as_str()),
            Self::RegexRefused(reason) => write!(formatter, "bad regex: {}", reason.as_str()),
            Self::Hits { .. } => self.hits(formatter),
            Self::NoDefinitionOf(word) => {
                write!(
                    formatter,
                    "no definition of '{}' in this tree",
                    word.as_str()
                )
            }
            Self::DefinedOutside { file, line } => write!(
                formatter,
                "defined outside the tree: {}:{}",
                file.display(),
                line.number()
            ),
            Self::NoDefinition => formatter.write_str("no definition found"),
            Self::LookingUp(word) => write!(
                formatter,
                "looking up '{}'; the language server has not answered yet",
                word.as_str()
            ),
            Self::Reindexing => formatter.write_str("re-indexing (source changed)"),
            Self::Reindexed(counts) => write!(formatter, "re-indexed: {counts} (source changed)"),
            Self::ReindexFailed => {
                formatter.write_str("re-index failed; the index in use is the old one")
            }
            Self::Review(status) => status.fmt(formatter),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ConsoleLog(String);

impl Default for ConsoleLog {
    fn default() -> Self {
        Self("type 'help' for commands; they act on the review this window shows\n".to_owned())
    }
}

impl ConsoleLog {
    pub(crate) fn lines(&self) -> impl Iterator<Item = &str> {
        self.0.lines()
    }

    pub(crate) fn line_count(&self) -> Count {
        Count::new(self.0.lines().count())
    }

    pub(crate) fn clear(&mut self) {
        self.0.clear();
    }

    pub(crate) fn command(&mut self, line: &str) {
        self.0.push_str("> ");
        self.0.push_str(line);
        self.0.push('\n');
    }

    pub(crate) fn append(&mut self, text: &str) {
        self.0.push_str(text);
    }

    pub(crate) fn rejected(&mut self, text: &str) {
        self.0.push_str(text.trim_end());
        self.0.push('\n');
    }

    pub(crate) fn failed(&mut self, error: &str) {
        self.0.push_str("error: ");
        self.0.push_str(error);
        self.0.push('\n');
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum FontError {
    Missing(Label),
    Unloadable(Label, Label),
}

impl fmt::Display for FontError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let bundled = BUNDLED_FAMILY.as_str();
        match self {
            Self::Missing(family) => write!(
                formatter,
                "font {} is not installed; showing {bundled}",
                family.as_str()
            ),
            Self::Unloadable(family, reason) => write!(
                formatter,
                "font {} does not load ({}); showing {bundled}",
                family.as_str(),
                reason.as_str()
            ),
        }
    }
}

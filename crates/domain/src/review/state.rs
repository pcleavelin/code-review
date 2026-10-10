use std::collections::BTreeMap;
use std::fmt;

use strum::VariantArray;

use crate::review::diff::FileChange;
use crate::review::{CommitId, Spelling, ThreadId};
use crate::text::{Line, RelativePath, Span, TextHash};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, VariantArray)]
pub enum Side {
    Old,
    New,
}

impl Side {
    pub const fn name(self) -> Spelling {
        Spelling::new(match self {
            Self::Old => "old",
            Self::New => "new",
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DraftId(u32);

impl DraftId {
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u32 {
        self.0
    }
}

impl fmt::Display for DraftId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DraftText(String);

impl DraftText {
    pub fn new(text: &str) -> Option<Self> {
        let text = text.trim();
        (!text.is_empty()).then(|| Self(text.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DraftText {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Draft {
    pub id: DraftId,
    pub file: RelativePath,
    pub side: Side,
    pub start: Option<Line>,
    pub line: Line,
    pub text: DraftText,
    pub reply: Option<ThreadId>,
}

impl Draft {
    pub fn lines(&self) -> Span {
        lines_of(self.start, self.line)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DraftOn {
    pub file: RelativePath,
    pub side: Side,
    pub start: Option<Line>,
    pub line: Line,
    pub reply: Option<ThreadId>,
}

impl DraftOn {
    pub fn lines(&self) -> Span {
        lines_of(self.start, self.line)
    }
}

fn lines_of(start: Option<Line>, line: Line) -> Span {
    start
        .and_then(|first| Span::new(first, line))
        .unwrap_or_else(|| Span::line(line))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Standing {
    New,
    Updated,
    Reviewing,
    Reviewed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mark {
    pub hash: TextHash,
    pub commit: CommitId,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Seen {
    Unseen,
    Changed,
    Seen,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct FileCount(usize);

impl FileCount {
    pub const fn new(count: usize) -> Self {
        Self(count)
    }

    pub const fn get(self) -> usize {
        self.0
    }
}

impl fmt::Display for FileCount {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Progress {
    seen: FileCount,
    total: FileCount,
}

impl Progress {
    pub const fn seen(self) -> FileCount {
        self.seen
    }

    pub const fn total(self) -> FileCount {
        self.total
    }

    pub const fn done(self) -> bool {
        self.seen.0 == self.total.0
    }
}

impl fmt::Display for Progress {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}/{}", self.seen, self.total)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DraftError {
    NoSuchDraft(DraftId),
}

impl fmt::Display for DraftError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSuchDraft(id) => write!(formatter, "no draft comment {id}"),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Review {
    marks: BTreeMap<RelativePath, Mark>,
    drafts: Vec<Draft>,
    looked: Option<CommitId>,
}

impl Review {
    pub fn new(
        marks: BTreeMap<RelativePath, Mark>,
        drafts: Vec<Draft>,
        looked: Option<CommitId>,
    ) -> Self {
        Self {
            marks,
            drafts,
            looked,
        }
    }

    pub const fn looked(&self) -> Option<&CommitId> {
        self.looked.as_ref()
    }

    pub fn look(&mut self, head: CommitId) {
        self.looked = Some(head);
    }

    pub fn standing(&self, head: &CommitId, progress: Progress) -> Standing {
        match &self.looked {
            None => Standing::New,
            Some(looked) if looked != head => Standing::Updated,
            Some(_) if progress.done() && self.drafts.is_empty() => Standing::Reviewed,
            Some(_) => Standing::Reviewing,
        }
    }

    pub const fn marks(&self) -> &BTreeMap<RelativePath, Mark> {
        &self.marks
    }

    pub fn drafts(&self) -> &[Draft] {
        &self.drafts
    }

    pub fn seen(&self, change: &FileChange) -> Seen {
        match self.marks.get(change.path()) {
            None => Seen::Unseen,
            Some(mark) if mark.hash == change.hash() => Seen::Seen,
            Some(_) => Seen::Changed,
        }
    }

    pub fn mark(&mut self, change: &FileChange, commit: CommitId) {
        self.marks.insert(
            change.path().clone(),
            Mark {
                hash: change.hash(),
                commit,
            },
        );
    }

    pub fn unmark(&mut self, path: &RelativePath) -> bool {
        self.marks.remove(path).is_some()
    }

    pub fn toggle(&mut self, change: &FileChange, commit: CommitId) -> Seen {
        if self.seen(change) == Seen::Seen {
            self.unmark(change.path());
            Seen::Unseen
        } else {
            self.mark(change, commit);
            Seen::Seen
        }
    }

    pub fn progress(&self, changes: &[FileChange]) -> Progress {
        Progress {
            seen: FileCount(
                changes
                    .iter()
                    .filter(|change| self.seen(change) == Seen::Seen)
                    .count(),
            ),
            total: FileCount(changes.len()),
        }
    }

    pub fn next_unseen<'changes>(
        &self,
        changes: &'changes [FileChange],
        after: Option<&RelativePath>,
    ) -> Option<&'changes FileChange> {
        let start = after
            .and_then(|path| changes.iter().position(|change| change.path() == path))
            .map_or(0, |at| at + 1);
        changes
            .iter()
            .skip(start)
            .chain(changes.iter().take(start))
            .find(|change| self.seen(change) != Seen::Seen)
    }

    pub fn add_draft(&mut self, on: DraftOn, text: DraftText) -> DraftId {
        let id = DraftId(
            self.drafts
                .iter()
                .map(|draft| draft.id.0)
                .max()
                .map_or(1, |last| last.saturating_add(1)),
        );
        self.drafts.push(Draft {
            id,
            file: on.file,
            side: on.side,
            start: on.start.filter(|first| *first < on.line),
            line: on.line,
            text,
            reply: on.reply,
        });
        id
    }

    pub fn remove_draft(&mut self, id: DraftId) -> Result<Draft, DraftError> {
        let at = self
            .drafts
            .iter()
            .position(|draft| draft.id == id)
            .ok_or(DraftError::NoSuchDraft(id))?;
        Ok(self.drafts.remove(at))
    }

    pub fn clear_drafts(&mut self) {
        self.drafts.clear();
    }

    pub fn drafts_at<'review>(
        &'review self,
        file: &'review RelativePath,
    ) -> impl Iterator<Item = &'review Draft> {
        self.drafts.iter().filter(move |draft| &draft.file == file)
    }
}

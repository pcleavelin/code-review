use std::collections::{BTreeMap, BTreeSet};
use std::iter;

use crate::review::{Side, Spelling};
use crate::text::{FileText, Line, LineCount, RelativePath, SourceLine, Span, TextHash};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LineKind {
    Context,
    Added,
    Removed,
}

impl LineKind {
    pub const fn marker(self) -> Spelling {
        Spelling::new(match self {
            Self::Context => " ",
            Self::Added => "+",
            Self::Removed => "-",
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffLine {
    pub kind: LineKind,
    pub text: SourceLine,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hunk {
    old_start: Line,
    new_start: Line,
    lines: Vec<DiffLine>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NumberedLine<'diff> {
    pub kind: LineKind,
    pub text: &'diff SourceLine,
    pub old: Option<Line>,
    pub new: Option<Line>,
}

impl Hunk {
    pub const fn new(old_start: Line, new_start: Line, lines: Vec<DiffLine>) -> Self {
        Self {
            old_start,
            new_start,
            lines,
        }
    }

    pub fn lines(&self) -> &[DiffLine] {
        &self.lines
    }

    pub const fn old_start(&self) -> Line {
        self.old_start
    }

    pub const fn new_start(&self) -> Line {
        self.new_start
    }

    pub fn numbered(&self) -> impl Iterator<Item = NumberedLine<'_>> {
        let mut old = Some(self.old_start);
        let mut new = Some(self.new_start);
        self.lines.iter().map(move |line| {
            let (at_old, at_new) = match line.kind {
                LineKind::Context => (old, new),
                LineKind::Added => (None, new),
                LineKind::Removed => (old, None),
            };
            if at_old.is_some() {
                old = old.and_then(Line::next);
            }
            if at_new.is_some() {
                new = new.and_then(Line::next);
            }
            NumberedLine {
                kind: line.kind,
                text: &line.text,
                old: at_old,
                new: at_new,
            }
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ChangeKind {
    Added,
    Deleted,
    Modified,
    Renamed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileRow {
    Head { line: Line, kind: LineKind },
    Removed { old: Line, text: SourceLine },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileChange {
    path: RelativePath,
    old_path: Option<RelativePath>,
    kind: ChangeKind,
    hunks: Vec<Hunk>,
}

impl FileChange {
    pub const fn new(
        path: RelativePath,
        old_path: Option<RelativePath>,
        kind: ChangeKind,
        hunks: Vec<Hunk>,
    ) -> Self {
        Self {
            path,
            old_path,
            kind,
            hunks,
        }
    }

    pub const fn path(&self) -> &RelativePath {
        &self.path
    }

    pub const fn old_path(&self) -> Option<&RelativePath> {
        self.old_path.as_ref()
    }

    pub const fn kind(&self) -> ChangeKind {
        self.kind
    }

    pub fn hunks(&self) -> &[Hunk] {
        &self.hunks
    }

    fn count(&self, kind: LineKind) -> LineCount {
        let lines = self
            .hunks
            .iter()
            .flat_map(Hunk::lines)
            .filter(|line| line.kind == kind)
            .count();
        LineCount::of(lines)
    }

    pub fn added(&self) -> LineCount {
        self.count(LineKind::Added)
    }

    pub fn removed(&self) -> LineCount {
        self.count(LineKind::Removed)
    }

    pub fn hash(&self) -> TextHash {
        let path = self.path.as_str().bytes().chain(iter::once(b'\n'));
        let body = self.hunks.iter().flat_map(Hunk::lines).flat_map(|line| {
            line.kind
                .marker()
                .as_str()
                .bytes()
                .chain(line.text.as_str().bytes())
                .chain(iter::once(b'\n'))
        });
        TextHash::of_bytes(path.chain(body))
    }

    pub fn commentable(&self, side: Side, lines: Span) -> bool {
        self.hunks.iter().any(|hunk| {
            let on: Vec<Line> = hunk
                .numbered()
                .filter_map(|numbered| match side {
                    Side::New => numbered.new,
                    Side::Old => numbered.old,
                })
                .collect();
            on.contains(&lines.start()) && on.contains(&lines.end())
        })
    }

    pub fn rows(&self, head: Option<&FileText>) -> Vec<FileRow> {
        let mut before: BTreeMap<Line, Vec<FileRow>> = BTreeMap::new();
        let mut added = BTreeSet::new();
        for hunk in &self.hunks {
            let mut cursor = hunk.new_start;
            for line in hunk.numbered() {
                match (line.kind, line.old, line.new) {
                    (LineKind::Removed, Some(old), _) => {
                        before.entry(cursor).or_default().push(FileRow::Removed {
                            old,
                            text: line.text.clone(),
                        });
                    }
                    (kind, _, Some(new)) => {
                        if kind == LineKind::Added {
                            added.insert(new);
                        }
                        cursor = new.next().unwrap_or(new);
                    }
                    _ => {}
                }
            }
        }
        let mut rows = Vec::new();
        for line in head
            .and_then(FileText::span)
            .into_iter()
            .flat_map(Span::lines)
        {
            rows.extend(before.remove(&line).into_iter().flatten());
            let kind = if added.contains(&line) {
                LineKind::Added
            } else {
                LineKind::Context
            };
            rows.push(FileRow::Head { line, kind });
        }
        rows.extend(before.into_values().flatten());
        rows
    }
}

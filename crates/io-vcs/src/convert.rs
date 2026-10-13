use std::collections::BTreeMap;

use domain::{
    ChangeKind, Commit, CommitId, DiffLine, FileChange, Hunk, Line, LineKind, Login, Moment, Prose,
    Provenance, RelativePath, SourceLine,
};
use io_process::{Argument, Captured, Deadline, Program};

use crate::wire::{self, CommandLine, WireAttribute, WireCommit, WireFile, WireHunk, WireKind};

pub(crate) struct Invocation {
    pub(crate) program: Program,
    pub(crate) arguments: Vec<Argument>,
    pub(crate) deadline: Deadline,
}

impl From<&CommandLine> for Invocation {
    fn from(line: &CommandLine) -> Self {
        Self {
            program: Program::new(line.program),
            arguments: line
                .arguments
                .iter()
                .map(|argument| Argument::new(argument))
                .collect(),
            deadline: line.deadline,
        }
    }
}

pub(crate) fn commit_of(wire: &WireCommit) -> Option<Commit> {
    Some(Commit {
        id: CommitId::new(&wire.id)?,
        author: Login::new(&wire.author),
        when: Moment::new(&wire.when),
        subject: Prose::new(&wire.subject),
    })
}

pub(crate) fn commit(stdout: &Captured) -> Option<CommitId> {
    CommitId::new(&stdout.text())
}

fn hunk(wire: WireHunk) -> Hunk {
    let lines = wire
        .lines
        .into_iter()
        .map(|(marker, text)| DiffLine {
            kind: match marker {
                '+' => LineKind::Added,
                '-' => LineKind::Removed,
                _ => LineKind::Context,
            },
            text: SourceLine::new(&text.replace('\t', "    ")),
        })
        .collect();
    Hunk::new(Line::new(wire.old_start), Line::new(wire.new_start), lines)
}

pub(crate) fn file_change(wire: WireFile) -> FileChange {
    let kind = match wire.kind {
        WireKind::Added => ChangeKind::Added,
        WireKind::Deleted => ChangeKind::Deleted,
        WireKind::Modified => ChangeKind::Modified,
        WireKind::Renamed => ChangeKind::Renamed,
    };
    FileChange::new(
        RelativePath::new(&wire.path),
        wire.old_path.as_deref().map(RelativePath::new),
        kind,
        wire.hunks.into_iter().map(hunk).collect(),
    )
}

pub(crate) fn provenances(attributes: &[WireAttribute]) -> BTreeMap<RelativePath, Provenance> {
    let mut by_path: BTreeMap<&str, Vec<&WireAttribute>> = BTreeMap::new();
    for attribute in attributes {
        by_path
            .entry(attribute.path.as_str())
            .or_default()
            .push(attribute);
    }
    by_path
        .into_iter()
        .map(|(path, given)| {
            let said_of = |name: &str| {
                given
                    .iter()
                    .find(|attribute| attribute.name == name)
                    .map(|attribute| attribute.value.as_str())
            };
            let generated = [wire::LINGUIST_GENERATED, wire::GITLAB_GENERATED].map(said_of);
            let provenance = if generated
                .iter()
                .any(|state| matches!(state, Some(wire::SET | wire::TRUE)))
            {
                Provenance::Generated
            } else if generated
                .iter()
                .any(|state| matches!(state, Some(wire::UNSET | wire::FALSE)))
            {
                Provenance::Handwritten
            } else {
                Provenance::Undeclared
            };
            (RelativePath::new(path), provenance)
        })
        .collect()
}

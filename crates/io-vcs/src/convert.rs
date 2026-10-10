use domain::{
    ChangeKind, Commit, CommitId, DiffLine, FileChange, Hunk, Line, LineKind, Login, Moment, Prose,
    RelativePath, SourceLine,
};
use io_process::{Argument, Captured, Program};

use crate::wire::{CommandLine, WireCommit, WireFile, WireHunk, WireKind};

pub(crate) struct Invocation {
    pub(crate) program: Program,
    pub(crate) arguments: Vec<Argument>,
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

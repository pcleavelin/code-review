mod convert;
mod wire;

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitStatus;

use domain::{BranchName, CommitId, FileChange, PrNumber, RefName, Root};
use io_process::Process;

use crate::convert::{Invocation, commit, file_change};
use crate::wire::CommandLine;

pub use io_process::{Captured, ProcessError, Program};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum VcsKind {
    Jj,
    Git,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Remote(String);

impl Remote {
    pub fn new(name: &str) -> Self {
        Self(name.to_owned())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteUrl(String);

impl RemoteUrl {
    pub fn new(text: &str) -> Self {
        Self(text.trim().to_owned())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fetched {
    pub base: CommitId,
    pub head: CommitId,
}

#[derive(Debug)]
pub enum VcsError {
    NoRepository,
    UnreadableRoot(io::Error),
    UnreadableMarker {
        path: PathBuf,
        error: io::Error,
    },
    Process(ProcessError),
    Failure {
        program: Program,
        status: ExitStatus,
        stderr: Captured,
    },
    NoCommit(RefName),
}

impl fmt::Display for VcsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoRepository => formatter.write_str("not inside a git repository"),
            Self::UnreadableRoot(error) => write!(formatter, "cannot read the directory: {error}"),
            Self::UnreadableMarker { path, error } => {
                write!(formatter, "cannot read {}: {error}", path.display())
            }
            Self::Process(error) => write!(formatter, "{error:?}"),
            Self::Failure {
                program, stderr, ..
            } => write!(formatter, "{program} failed: {}", stderr.text().trim()),
            Self::NoCommit(reference) => write!(formatter, "{reference} names no commit"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Vcs {
    kind: VcsKind,
    root: Root,
    marker: PathBuf,
}

impl Vcs {
    pub fn detect(root: &Root) -> Result<Self, VcsError> {
        let absolute = root
            .as_path()
            .canonicalize()
            .map_err(VcsError::UnreadableRoot)?;
        let (kind, marker) = absolute
            .ancestors()
            .find_map(|directory| {
                let jj = directory.join(wire::JJ_MARKER);
                let git = directory.join(wire::GIT_MARKER);
                if jj.is_dir() {
                    Some((VcsKind::Jj, jj))
                } else if git.exists() {
                    Some((VcsKind::Git, git))
                } else {
                    None
                }
            })
            .ok_or(VcsError::NoRepository)?;
        Ok(Self {
            kind,
            root: root.clone(),
            marker,
        })
    }

    pub fn shared_directory(&self) -> Result<PathBuf, VcsError> {
        self.shared().map(io_process::plain)
    }

    fn shared(&self) -> Result<PathBuf, VcsError> {
        let unreadable = |path: &Path| {
            let path = path.to_path_buf();
            move |error| VcsError::UnreadableMarker { path, error }
        };
        let pointed = |file: &Path, text: &str, base: &Path| -> Result<PathBuf, VcsError> {
            base.join(text.trim())
                .canonicalize()
                .map_err(unreadable(file))
        };
        match self.kind {
            VcsKind::Jj => {
                let repo = self.marker.join(wire::JJ_REPO);
                if repo.is_dir() {
                    return repo.canonicalize().map_err(unreadable(&repo));
                }
                let text = fs::read_to_string(&repo).map_err(unreadable(&repo))?;
                pointed(&repo, &text, &self.marker)
            }
            VcsKind::Git => {
                if self.marker.is_dir() {
                    return self.marker.canonicalize().map_err(unreadable(&self.marker));
                }
                let text = fs::read_to_string(&self.marker).map_err(unreadable(&self.marker))?;
                let base = self.marker.parent().unwrap_or(&self.marker);
                let directory = pointed(
                    &self.marker,
                    text.trim().trim_start_matches(wire::GIT_DIR_PREFIX),
                    base,
                )?;
                let common = directory.join(wire::GIT_COMMON_DIR);
                match fs::read_to_string(&common) {
                    Ok(relative) => pointed(&common, &relative, &directory),
                    Err(_) => Ok(directory),
                }
            }
        }
    }

    pub fn kind(&self) -> VcsKind {
        self.kind
    }

    pub fn clone_from(url: &RemoteUrl, into: &Path) -> Result<Root, VcsError> {
        if !into.join(wire::GIT_MARKER).exists() {
            if let Some(parent) = into.parent() {
                fs::create_dir_all(parent).map_err(VcsError::UnreadableRoot)?;
            }
            let place = into.to_string_lossy();
            let here = into.parent().unwrap_or(into);
            Self::run(&wire::clone(url.as_str(), &place), here)?;
        }
        Ok(Root::new(into))
    }

    pub fn remote_url(&self, remote: &Remote) -> Result<RemoteUrl, VcsError> {
        let stdout = Self::run(&wire::remote_url(remote.as_str()), self.root.as_path())?;
        Ok(RemoteUrl(stdout.text().trim().to_owned()))
    }

    pub fn fetch(
        &self,
        remote: &Remote,
        number: PrNumber,
        head: &RefName,
        base: &BranchName,
    ) -> Result<Fetched, VcsError> {
        let line = wire::fetch(remote.as_str(), number.get(), head.as_str(), base.as_str());
        Self::run(&line, self.root.as_path())?;
        Ok(Fetched {
            base: self.resolve(&RefName::new(&wire::local_ref(
                number.get(),
                wire::BASE_END,
            )))?,
            head: self.resolve(&RefName::new(&wire::local_ref(
                number.get(),
                wire::HEAD_END,
            )))?,
        })
    }

    pub fn fetched(&self, number: PrNumber) -> Result<Fetched, VcsError> {
        Ok(Fetched {
            base: self.resolve(&RefName::new(&wire::local_ref(
                number.get(),
                wire::BASE_END,
            )))?,
            head: self.resolve(&RefName::new(&wire::local_ref(
                number.get(),
                wire::HEAD_END,
            )))?,
        })
    }

    fn resolve(&self, reference: &RefName) -> Result<CommitId, VcsError> {
        let stdout = Self::run(&wire::resolve(reference.as_str()), self.root.as_path())?;
        commit(&stdout).ok_or_else(|| VcsError::NoCommit(reference.clone()))
    }

    pub fn diff(&self, base: &CommitId, head: &CommitId) -> Result<Vec<FileChange>, VcsError> {
        let stdout = Self::run(
            &wire::diff(base.as_str(), head.as_str()),
            self.root.as_path(),
        )?;
        Ok(wire::parse_diff(&stdout.text())
            .into_iter()
            .map(file_change)
            .collect())
    }

    pub fn check_out(&self, tree: &Path, commit: &CommitId) -> Result<(), VcsError> {
        if tree.join(wire::GIT_MARKER).exists() {
            Self::run(&wire::check_out(commit.as_str()), tree)?;
        } else {
            let place = tree.to_string_lossy();
            Self::run(
                &wire::add_worktree(&place, commit.as_str()),
                self.root.as_path(),
            )?;
        }
        Ok(())
    }

    fn run(line: &CommandLine, directory: &Path) -> Result<Captured, VcsError> {
        let invocation = Invocation::from(line);
        let output = Process::run(&invocation.program, &invocation.arguments, directory)
            .map_err(VcsError::Process)?;
        if output.success() {
            Ok(output.stdout)
        } else {
            Err(VcsError::Failure {
                program: invocation.program,
                status: output.status,
                stderr: output.stderr,
            })
        }
    }
}

#[cfg(test)]
mod tests;

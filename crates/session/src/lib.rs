mod desk;
mod repo;

use std::fmt;
use std::io;

use domain::{CommitId, PrNumber};
use io_forge::ForgeError;
use io_review::ReviewError;
use io_vcs::VcsError;

pub use crate::desk::{Desk, PinTarget, Pinned, PullAddress, Why};
pub use crate::repo::{Entry, Opened, Repo, RepoName};
pub use io_config::Reach;
pub use io_forge::Submitted;

#[derive(Debug)]
pub enum SessionError {
    Vcs(VcsError),
    NoRemote,
    Forge(ForgeError),
    Review(ReviewError),
    NotOpened(PrNumber, VcsError),
    Moved {
        number: PrNumber,
        reviewed: CommitId,
        now: CommitId,
    },
    NothingToSubmit(PrNumber),
    NothingToDrop(PrNumber),
    NoPins,
    NoPinStore,
    Pins(io::Error),
    AlreadyPinned(RepoName),
    NoSuchRepo(RepoName, Vec<RepoName>),
    Ambiguous(PrNumber, Vec<RepoName>),
}

struct Listed<'names>(&'names [RepoName]);

impl fmt::Display for Listed<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0.is_empty() {
            return formatter.write_str("none is pinned");
        }
        let names: Vec<&str> = self.0.iter().map(RepoName::as_str).collect();
        formatter.write_str(&names.join(", "))
    }
}

impl fmt::Display for SessionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Vcs(error) => write!(formatter, "git: {error}"),
            Self::NoRemote => formatter
                .write_str("the repository has neither an 'upstream' nor an 'origin' remote"),
            Self::Forge(error) => write!(formatter, "{error}"),
            Self::Review(error) => write!(formatter, "{error}"),
            Self::NotOpened(number, _) => write!(
                formatter,
                "pull request {number} has not been fetched yet: open it first"
            ),
            Self::Moved {
                number,
                reviewed,
                now,
            } => write!(
                formatter,
                "pull request {number} moved from {reviewed} to {now} since it was opened: open it again and look at what changed before submitting"
            ),
            Self::NothingToSubmit(number) => write!(
                formatter,
                "pull request {number} has no draft comments and no summary to submit"
            ),
            Self::NothingToDrop(number) => write!(
                formatter,
                "pull request {number} has no tree and no base or head ref here"
            ),
            Self::NoPins => formatter.write_str(
                "no repository: run this inside a clone, or pin one with 'review pin <path or URL>'",
            ),
            Self::NoPinStore => formatter.write_str(
                "there is no configuration folder to keep pins in: set REVIEW_PINS and REVIEW_CLONES",
            ),
            Self::Pins(error) => write!(formatter, "the pins could not be saved: {error}"),
            Self::AlreadyPinned(name) => write!(formatter, "{name} is pinned already"),
            Self::NoSuchRepo(asked, known) => write!(
                formatter,
                "no pinned repository is called {asked}; known: {}",
                Listed(known)
            ),
            Self::Ambiguous(number, known) => write!(
                formatter,
                "#{number} could be in {}: write it as <repository>#{number}",
                Listed(known)
            ),
        }
    }
}

impl From<VcsError> for SessionError {
    fn from(error: VcsError) -> Self {
        Self::Vcs(error)
    }
}

impl From<ForgeError> for SessionError {
    fn from(error: ForgeError) -> Self {
        Self::Forge(error)
    }
}

impl From<ReviewError> for SessionError {
    fn from(error: ReviewError) -> Self {
        Self::Review(error)
    }
}

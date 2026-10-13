mod access;
mod desk;
mod repo;

use std::fmt;
use std::io;
use std::path::PathBuf;

use domain::{CommitId, ForgeHost, Host, JiraSite, PrNumber, Refusal, RelativePath};
use io_forge::{ForgeError, MissingClient};
use io_jira::JiraError;
use io_keychain::KeychainError;
use io_review::ReviewError;
use io_vcs::VcsError;

pub use crate::access::{Access, Holds, StartedLogin};
pub use crate::desk::{Desk, LoginTarget, PinTarget, Pinned, PullAddress, Why};
pub use crate::repo::{Entry, Opened, Repo, RepoName, RequestedFrom, Waiting};
pub use io_config::Reach;
pub use io_forge::{Cancel, DeviceAuthorization, Submitted};
pub use io_keychain::Forgotten;

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
    SubmitRefused {
        number: PrNumber,
        refusal: Refusal,
    },
    NothingToDrop(PrNumber),
    SeenHeadGone {
        file: RelativePath,
        head: CommitId,
    },
    NoPins,
    NoPinStore,
    Pins(io::Error),
    AlreadyPinned(RepoName),
    NoSuchRepo(RepoName, Vec<RepoName>),
    Ambiguous(PrNumber, Vec<RepoName>),
    Keychain(KeychainError),
    NoKeychain,
    NoClient {
        host: Host,
        gap: MissingClient,
        file: Option<PathBuf>,
    },
    Renewal {
        site: ForgeHost,
        cause: ForgeError,
    },
    TokenRefused {
        site: ForgeHost,
        cause: ForgeError,
    },
    NoSite(LoginTarget),
    Jira(JiraError),
    JiraLoggedOut(JiraSite),
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
            Self::SubmitRefused { number, refusal } => write!(
                formatter,
                "cannot submit to pull request {number}: {refusal}"
            ),
            Self::SeenHeadGone { file, head } => write!(
                formatter,
                "{file} was seen at {head}, which is no longer in the clone; 'diff <pr> {file}' shows its whole diff"
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
            Self::Keychain(error) => write!(formatter, "{error}"),
            Self::NoKeychain => formatter.write_str(
                "a script or screenshot run keeps away from the login keychain: set REVIEW_KEYCHAIN to a scratch keychain",
            ),
            Self::NoClient { host, gap, file } => {
                write!(
                    formatter,
                    "logging in to {host} needs the client ID of an OAuth app registered on {host} with the device flow enabled, and {gap}: add the line `client {host} <client id>` to "
                )?;
                match file {
                    Some(file) => write!(formatter, "{}", file.display()),
                    None => formatter.write_str("the forges file (set REVIEW_FORGES)"),
                }
            }
            Self::Renewal { site, cause } => write!(
                formatter,
                "the login to {site} expired and could not be renewed ({cause}): log in again with 'review login {site}'"
            ),
            Self::TokenRefused { site, cause } => write!(
                formatter,
                "{cause}: the forge refused the token the keychain holds for {site}; log in again with 'review login {site}'"
            ),
            Self::Jira(error) => write!(formatter, "{error}"),
            Self::JiraLoggedOut(site) => write!(
                formatter,
                "not logged in to {site}: `review login {site}` and paste email:api-token (Jira Cloud) or a personal access token"
            ),
            Self::NoSite(target) => write!(
                formatter,
                "{} is neither a pinned repository nor a forge's host",
                target.as_str()
            ),
        }
    }
}

impl SessionError {
    pub const fn lacks_token(&self) -> bool {
        matches!(self, Self::Forge(ForgeError::NoToken(..)))
    }

    pub const fn keychain_timed_out(&self) -> bool {
        matches!(self, Self::Keychain(KeychainError::TimedOut { .. }))
    }
}

impl From<JiraError> for SessionError {
    fn from(error: JiraError) -> Self {
        Self::Jira(error)
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

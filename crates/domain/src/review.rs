mod access;
mod diff;
mod link;
mod state;
mod talk;

use std::fmt;

use strum::VariantArray;

pub use access::{
    ClientId, ForgeHost, Grant, Lifetime, OAuthApp, RefreshToken, Renewal, Token, UnixTime,
    UserCode,
};
pub use diff::{
    ChangeKind, DiffLayout, DiffLine, FileChange, FileRow, Hunk, LineKind, NumberedLine, Paired,
    Pairing,
};
pub use link::{Linked, LinkedFile, Revision, WebLink};
pub use state::{
    Draft, DraftError, DraftId, DraftOn, DraftText, FileCount, Mark, Progress, Review, Seen, Side,
    Standing,
};
pub use talk::{
    CodePlace, Commit, Conversation, Moment, Note, NoteKind, Prose, Thread, ThreadId, ThreadState,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Spelling(&'static str);

impl Spelling {
    pub(crate) const fn new(text: &'static str) -> Self {
        Self(text)
    }

    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, VariantArray)]
pub enum ForgeKind {
    GitHub,
    GitLab,
    Forgejo,
}

impl ForgeKind {
    pub const fn name(self) -> Spelling {
        Spelling(match self {
            Self::GitHub => "github",
            Self::GitLab => "gitlab",
            Self::Forgejo => "forgejo",
        })
    }
}

impl fmt::Display for ForgeKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.name().as_str())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Host(String);

impl Host {
    pub fn new(name: &str) -> Option<Self> {
        let name = name.trim().to_ascii_lowercase();
        (!name.is_empty() && !name.contains(['/', ' '])).then_some(Self(name))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Host {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RepoSlug(String);

impl RepoSlug {
    pub fn new(path: &str) -> Option<Self> {
        let path = path.trim_matches('/');
        (path.contains('/') && path.split('/').all(|part| !part.is_empty()))
            .then(|| Self(path.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RepoSlug {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Forge {
    kind: ForgeKind,
    host: Host,
    slug: RepoSlug,
}

impl Forge {
    pub const fn new(kind: ForgeKind, host: Host, slug: RepoSlug) -> Self {
        Self { kind, host, slug }
    }

    pub const fn kind(&self) -> ForgeKind {
        self.kind
    }

    pub const fn host(&self) -> &Host {
        &self.host
    }

    pub const fn slug(&self) -> &RepoSlug {
        &self.slug
    }

    pub fn forge_host(&self) -> ForgeHost {
        ForgeHost::new(self.kind, self.host.clone())
    }
}

impl fmt::Display for Forge {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} {}/{}", self.kind, self.host, self.slug)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PrNumber(u64);

impl PrNumber {
    pub fn new(number: u64) -> Option<Self> {
        (number > 0).then_some(Self(number))
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::new(text.trim().trim_start_matches(['#', '!']).parse().ok()?)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for PrNumber {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

macro_rules! text_type {
    ($name:ident) => {
        #[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            pub fn new(text: &str) -> Self {
                Self(text.to_owned())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }
    };
}

text_type!(PrTitle);
text_type!(Login);
text_type!(BranchName);
text_type!(RefName);

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CommitId(String);

impl CommitId {
    const SHORT: usize = 8;

    pub fn new(text: &str) -> Option<Self> {
        let text = text.trim();
        (text.len() >= Self::SHORT && text.chars().all(|digit| digit.is_ascii_hexdigit()))
            .then(|| Self(text.to_ascii_lowercase()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn short(&self) -> &str {
        self.0.get(..Self::SHORT).unwrap_or(&self.0)
    }
}

impl fmt::Display for CommitId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.short())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PullRequest {
    pub number: PrNumber,
    pub title: PrTitle,
    pub author: Login,
    pub base: BranchName,
    pub head: BranchName,
    pub head_commit: CommitId,
    pub description: Prose,
    pub updated: Moment,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, VariantArray)]
pub enum Decision {
    Comment,
    Approve,
    RequestChanges,
}

impl Decision {
    pub const fn name(self) -> Spelling {
        Spelling(match self {
            Self::Comment => "comment",
            Self::Approve => "approve",
            Self::RequestChanges => "request-changes",
        })
    }
}

impl fmt::Display for Decision {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.name().as_str())
    }
}

#[cfg(test)]
mod tests;

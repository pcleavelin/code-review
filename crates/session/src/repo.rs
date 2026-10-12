use std::fmt;
use std::path::PathBuf;

use domain::{
    Commit, CommitId, Conversation, Decision, Draft, DraftText, FileChange, FileText, Forge, Mark,
    PrNumber, Progress, PullRequest, RelativePath, Review, Root, Seen, Standing,
};
use io_forge::{ForgeClient, ForgeError, Submitted, TokenSource};
use io_review::ReviewStore;
use io_vcs::{Remote, RemoteUrl, Vcs};

use crate::SessionError;
use crate::access::Access;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Word(&'static str);

const REMOTES: [Word; 2] = [Word("upstream"), Word("origin")];
const GIT_SUFFIX: Word = Word(".git");
const SCHEME: Word = Word("://");

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RepoName(String);

impl RepoName {
    pub fn new(text: &str) -> Self {
        Self(text.trim().to_owned())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn of(url: &RemoteUrl, root: &Root) -> Self {
        let text = url.as_str().trim_end_matches('/');
        let text = text.strip_suffix(GIT_SUFFIX.0).unwrap_or(text);
        let path = match text.split_once(SCHEME.0) {
            Some((_, rest)) => rest.split_once('/').map_or(rest, |(_, path)| path),
            None => text.rsplit_once(':').map_or(text, |(_, path)| path),
        };
        let mut segments = path.rsplit(['/', '\\']).filter(|part| !part.is_empty());
        match (segments.next(), segments.next()) {
            (Some(name), Some(owner)) => Self(format!("{owner}/{name}")),
            (Some(name), None) => Self(name.to_owned()),
            _ => Self(
                root.as_path()
                    .file_name()
                    .map_or_else(String::new, |name| name.to_string_lossy().into_owned()),
            ),
        }
    }

    pub fn answers_to(&self, asked: &Self) -> bool {
        self.0.eq_ignore_ascii_case(&asked.0)
            || self
                .0
                .rsplit('/')
                .next()
                .is_some_and(|short| short.eq_ignore_ascii_case(&asked.0))
    }
}

impl fmt::Display for RepoName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Opened {
    pub number: PrNumber,
    pub base: CommitId,
    pub head: CommitId,
    pub changes: Vec<FileChange>,
    pub commits: Vec<Commit>,
    pub review: Review,
    pub tree: PathBuf,
}

impl Opened {
    pub fn change(&self, path: &RelativePath) -> Option<&FileChange> {
        self.changes.iter().find(|change| change.path() == path)
    }

    pub fn seen(&self, change: &FileChange) -> Seen {
        self.review.seen(change)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub repo: RepoName,
    pub pull: PullRequest,
    pub progress: Option<Progress>,
    pub drafts: Vec<Draft>,
    pub standing: Standing,
}

#[derive(Clone, Debug)]
pub struct Repo {
    root: Root,
    name: RepoName,
    vcs: Vcs,
    store: ReviewStore,
    remote: Remote,
    url: RemoteUrl,
    access: Access,
}

impl Repo {
    pub fn find(root: &Root, access: &Access) -> Result<Self, SessionError> {
        let vcs = Vcs::detect(root)?;
        let store = ReviewStore::new(root)?;
        let (remote, url) = REMOTES
            .iter()
            .map(|word| Remote::new(word.0))
            .find_map(|remote| vcs.remote_url(&remote).ok().map(|url| (remote, url)))
            .ok_or(SessionError::NoRemote)?;
        Ok(Self {
            name: RepoName::of(&url, root),
            root: root.clone(),
            vcs,
            store,
            remote,
            url,
            access: access.clone(),
        })
    }

    pub const fn root(&self) -> &Root {
        &self.root
    }

    pub const fn name(&self) -> &RepoName {
        &self.name
    }

    pub const fn remote(&self) -> &Remote {
        &self.remote
    }

    pub fn web(&self) -> Option<Forge> {
        io_forge::forge_of(&self.url).ok()
    }

    pub fn forge(&self) -> Result<ForgeClient, SessionError> {
        let mut trouble = None;
        let client = ForgeClient::connect(&self.url, |forge| {
            self.access.token(forge).unwrap_or_else(|error| {
                trouble = Some(error);
                None
            })
        })?;
        match trouble {
            Some(error) if client.token_source().is_none() => Err(error),
            _ => Ok(client),
        }
    }

    fn with_forge<T>(
        &self,
        ask: impl Fn(&ForgeClient) -> Result<T, ForgeError>,
    ) -> Result<T, SessionError> {
        let client = self.forge()?;
        match ask(&client) {
            Err(cause)
                if cause.refused_token()
                    && client.token_source() == Some(TokenSource::Keychain) =>
            {
                let site = client.forge().forge_host();
                if self.access.renew(&site)?.is_none() {
                    return Err(SessionError::TokenRefused { site, cause });
                }
                Ok(ask(&self.forge()?)?)
            }
            answered => Ok(answered?),
        }
    }

    pub fn pulls(&self) -> Result<Vec<PullRequest>, SessionError> {
        self.with_forge(ForgeClient::open_pulls)
    }

    pub fn inbox(&self) -> Result<Vec<Entry>, SessionError> {
        Ok(self
            .pulls()?
            .into_iter()
            .map(|pull| {
                let state = self.state(pull.number).ok();
                let review = state.as_ref().map_or_else(
                    || self.review(pull.number).unwrap_or_default(),
                    |opened| opened.review.clone(),
                );
                let progress = state
                    .as_ref()
                    .map(|opened| opened.review.progress(&opened.changes));
                Entry {
                    repo: self.name.clone(),
                    standing: review.standing(&pull.head_commit, progress),
                    drafts: review.drafts().to_vec(),
                    progress,
                    pull,
                }
            })
            .collect())
    }

    pub fn pull(&self, number: PrNumber) -> Result<PullRequest, SessionError> {
        self.with_forge(|forge| forge.pull(number))
    }

    pub fn conversation(&self, pull: &PullRequest) -> Result<Conversation, SessionError> {
        self.with_forge(|forge| forge.conversation(pull))
    }

    pub fn fetch(&self, pull: &PullRequest) -> Result<Opened, SessionError> {
        let forge = self.forge()?;
        self.vcs.fetch(
            &self.remote,
            pull.number,
            &forge.head_ref(pull.number),
            &pull.base,
        )?;
        let mut opened = self.reopen(pull.number)?;
        opened.review.look(opened.head.clone());
        self.save(&opened)?;
        Ok(opened)
    }

    pub fn state(&self, number: PrNumber) -> Result<Opened, SessionError> {
        let fetched = self
            .vcs
            .fetched(number)
            .map_err(|error| SessionError::NotOpened(number, error))?;
        Ok(Opened {
            number,
            changes: self.vcs.diff(&fetched.base, &fetched.head)?,
            commits: self.vcs.commits(&fetched.base, &fetched.head)?,
            base: fetched.base,
            head: fetched.head,
            review: self.store.load(number)?,
            tree: self.store.tree(number),
        })
    }

    pub fn reopen(&self, number: PrNumber) -> Result<Opened, SessionError> {
        let opened = self.state(number)?;
        self.vcs.check_out(&opened.tree, &opened.head)?;
        Ok(opened)
    }

    pub fn drop_fetched(&self, number: PrNumber) -> Result<PathBuf, SessionError> {
        let tree = self.store.tree(number);
        if self.vcs.fetched(number).is_err() && !tree.exists() {
            return Err(SessionError::NothingToDrop(number));
        }
        self.vcs.drop_fetched(number, &tree)?;
        Ok(tree)
    }

    pub fn since_seen(
        &self,
        opened: &Opened,
        change: &FileChange,
        mark: &Mark,
    ) -> Result<Option<FileChange>, SessionError> {
        if !self.vcs.has_commit(&mark.commit) {
            return Err(SessionError::SeenHeadGone {
                file: change.path().clone(),
                head: mark.commit.clone(),
            });
        }
        Ok(self
            .vcs
            .file_change(&mark.commit, &opened.head, change.path())?)
    }

    pub fn commit_changes(&self, commit: &CommitId) -> Result<Vec<FileChange>, SessionError> {
        Ok(self.vcs.commit_changes(commit)?)
    }

    pub fn read_at(
        &self,
        commit: &CommitId,
        path: &RelativePath,
    ) -> Result<FileText, SessionError> {
        Ok(self.vcs.read_at(commit, path)?)
    }

    pub fn review(&self, number: PrNumber) -> Result<Review, SessionError> {
        Ok(self.store.load(number)?)
    }

    pub fn save(&self, opened: &Opened) -> Result<(), SessionError> {
        self.store.save(opened.number, &opened.review)?;
        Ok(())
    }

    pub const fn store(&self) -> &ReviewStore {
        &self.store
    }

    pub fn submit(
        &self,
        opened: &mut Opened,
        decision: Decision,
        body: Option<&DraftText>,
    ) -> Result<Submitted, SessionError> {
        if opened.review.drafts().is_empty() && body.is_none() && decision == Decision::Comment {
            return Err(SessionError::NothingToSubmit(opened.number));
        }
        let (forge, pull) =
            self.with_forge(|forge| Ok((forge.clone(), forge.pull(opened.number)?)))?;
        if pull.head_commit != opened.head {
            return Err(SessionError::Moved {
                number: opened.number,
                reviewed: opened.head.clone(),
                now: pull.head_commit,
            });
        }
        let submitted = forge.submit(
            &pull,
            opened.review.drafts(),
            &opened.changes,
            decision,
            body,
        );
        match submitted {
            Ok(submitted) => {
                opened.review.clear_drafts();
                self.save(opened)?;
                Ok(submitted)
            }
            Err(error) => {
                for draft in error.sent() {
                    drop(opened.review.remove_draft(*draft));
                }
                if !error.sent().is_empty() {
                    self.save(opened)?;
                }
                Err(error.into())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn name(url: &str) -> String {
        RepoName::of(&RemoteUrl::new(url), &Root::new(Path::new("/x/dir")))
            .as_str()
            .to_owned()
    }

    #[test]
    fn a_repository_is_named_owner_slash_name_from_any_remote_form() {
        assert_eq!(
            name("https://github.com/sharkdp/hexyl.git"),
            "sharkdp/hexyl"
        );
        assert_eq!(name("git@github.com:sharkdp/hexyl.git"), "sharkdp/hexyl");
        assert_eq!(name("ssh://git@host:22/group/sub/repo/"), "sub/repo");
        assert_eq!(name("C:\\work\\forge"), "work/forge");
    }

    #[test]
    fn a_short_name_or_the_full_one_finds_a_repository_in_any_case() {
        let full = RepoName::new("sharkdp/hexyl");
        assert!(full.answers_to(&RepoName::new("hexyl")));
        assert!(full.answers_to(&RepoName::new("SharkDP/Hexyl")));
        assert!(!full.answers_to(&RepoName::new("hex")));
    }
}

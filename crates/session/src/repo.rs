use std::collections::BTreeSet;
use std::fmt;
use std::path::PathBuf;

use domain::{
    Commit, CommitId, Content, Conversation, Decision, Draft, DraftText, FileChange, FileText,
    Forge, Host, Mark, PrNumber, Progress, Provenance, PullRequest, PullStatus, RelativePath,
    Review, Rights, Root, Seen,
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
    pub generated: BTreeSet<RelativePath>,
}

impl Opened {
    pub fn change(&self, path: &RelativePath) -> Option<&FileChange> {
        self.changes.iter().find(|change| change.path() == path)
    }

    pub fn seen(&self, change: &FileChange) -> Seen {
        self.review.seen(change)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RequestedFrom {
    #[default]
    Any,
    You,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Waiting {
    Keychain(Host),
    Forge(Host),
    Review(PrNumber),
}

impl fmt::Display for Waiting {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Keychain(host) => {
                write!(formatter, "reading the login for {host} from the keychain")
            }
            Self::Forge(host) => write!(formatter, "waiting for {host} to answer"),
            Self::Review(number) => write!(formatter, "reading the review of #{number}"),
        }
    }
}

struct Connected {
    client: ForgeClient,
    timed_out: Option<SessionError>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub repo: RepoName,
    pub pull: PullRequest,
    pub progress: Option<Progress>,
    pub drafts: Vec<Draft>,
    pub status: PullStatus,
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

    fn connect(&self, report: &mut dyn FnMut(Waiting)) -> Result<Connected, SessionError> {
        let mut trouble = None;
        let client = ForgeClient::connect(&self.url, |forge| {
            report(Waiting::Keychain(forge.host().clone()));
            self.access.token(forge).unwrap_or_else(|error| {
                trouble = Some(error);
                None
            })
        })?;
        let timed_out = match trouble {
            Some(error) if client.token_source().is_none() && !error.keychain_timed_out() => {
                return Err(error);
            }
            Some(error) if client.token_source().is_none() => Some(error),
            _ => None,
        };
        Ok(Connected { client, timed_out })
    }

    pub fn forge(&self) -> Result<ForgeClient, SessionError> {
        self.connect(&mut |_| {}).map(|connected| connected.client)
    }

    fn with_forge<T>(
        &self,
        ask: impl Fn(&ForgeClient) -> Result<T, ForgeError>,
        report: &mut dyn FnMut(Waiting),
    ) -> Result<T, SessionError> {
        let Connected { client, timed_out } = self.connect(report)?;
        report(Waiting::Forge(client.forge().host().clone()));
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
            Err(cause @ ForgeError::NoToken(..)) => {
                Err(timed_out.unwrap_or(SessionError::Forge(cause)))
            }
            answered => Ok(answered?),
        }
    }

    pub fn inbox(&self, report: &mut dyn FnMut(Waiting)) -> Result<Vec<Entry>, SessionError> {
        let pulls = self.with_forge(ForgeClient::open_pulls, report)?;
        let mut entries = Vec::with_capacity(pulls.len());
        for pull in pulls {
            report(Waiting::Review(pull.number));
            let state = self.state(pull.number).ok();
            let review = state.as_ref().map_or_else(
                || self.review(pull.number).unwrap_or_default(),
                |opened| opened.review.clone(),
            );
            let progress = state
                .as_ref()
                .map(|opened| opened.review.progress(&opened.changes));
            entries.push(Entry {
                repo: self.name.clone(),
                status: review.status(&pull.head_commit, progress),
                drafts: review.drafts().to_vec(),
                progress,
                pull,
            });
        }
        Ok(entries)
    }

    pub fn requested(
        &self,
        report: &mut dyn FnMut(Waiting),
    ) -> Result<Vec<PrNumber>, SessionError> {
        self.with_forge(ForgeClient::requested, report)
    }

    pub fn pull(&self, number: PrNumber) -> Result<PullRequest, SessionError> {
        self.with_forge(|forge| forge.pull(number), &mut |_| {})
    }

    pub fn conversation(&self, pull: &PullRequest) -> Result<Conversation, SessionError> {
        self.with_forge(|forge| forge.conversation(pull), &mut |_| {})
    }

    pub fn rights(&self, pull: &PullRequest) -> Result<Rights, SessionError> {
        self.with_forge(|forge| forge.rights(pull), &mut |_| {})
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
        let mut changes = self.vcs.diff(&fetched.base, &fetched.head)?;
        let generated = self.generated_last(&fetched.head, &mut changes)?;
        Ok(Opened {
            number,
            changes,
            generated,
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
        let mut changes = self.vcs.commit_changes(commit)?;
        self.generated_last(commit, &mut changes)?;
        Ok(changes)
    }

    fn generated_last(
        &self,
        at: &CommitId,
        changes: &mut [FileChange],
    ) -> Result<BTreeSet<RelativePath>, SessionError> {
        let paths: Vec<RelativePath> = changes.iter().map(|change| change.path().clone()).collect();
        let marked = self.vcs.provenances(at, &paths)?;
        let generated: BTreeSet<RelativePath> = paths
            .into_iter()
            .filter(|path| {
                marked
                    .get(path)
                    .copied()
                    .unwrap_or(Provenance::Undeclared)
                    .generated(path)
            })
            .collect();
        changes.sort_by_key(|change| generated.contains(change.path()));
        Ok(generated)
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
        let content = Content::of(opened.review.drafts(), body);
        let refused = |rights: &Rights| {
            rights
                .refusal(decision, content)
                .map(|refusal| SessionError::SubmitRefused {
                    number: opened.number,
                    refusal,
                })
        };
        if let Some(refused) = refused(&Rights::default()) {
            return Err(refused);
        }
        let (forge, pull, rights) = self.with_forge(
            |forge| {
                let pull = forge.pull(opened.number)?;
                let rights = forge.rights(&pull)?;
                Ok((forge.clone(), pull, rights))
            },
            &mut |_| {},
        )?;
        if let Some(refused) = refused(&rights) {
            return Err(refused);
        }
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

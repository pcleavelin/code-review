use std::str::FromStr;

use std::fs;
use std::path::{Path, PathBuf};

use domain::{PrNumber, Root};
use io_config::{PinsStore, Reach};
use io_vcs::{RemoteUrl, Vcs};

use crate::SessionError;
use crate::repo::{Repo, RepoName};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PinTarget(String);

impl PinTarget {
    pub fn new(text: &str) -> Self {
        Self(text.trim().to_owned())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PullAddress {
    pub repo: Option<RepoName>,
    pub number: PrNumber,
}

impl FromStr for PullAddress {
    type Err = ();

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let text = text.trim();
        let (repo, number) = match text.rsplit_once(['#', '!']) {
            Some((repo, number)) if !repo.is_empty() => (Some(RepoName::new(repo)), number),
            _ => (None, text),
        };
        PrNumber::parse(number)
            .map(|number| Self { repo, number })
            .ok_or(())
    }
}

#[derive(Clone, Debug)]
pub enum Pinned {
    Ready(Repo),
    Broken { root: Root, why: Why },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Why(String);

impl Why {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Word(&'static str);

const SCHEME: Word = Word("://");
const GIT_SUFFIX: Word = Word(".git");
const UP: Word = Word("..");

#[derive(Clone, Debug)]
pub struct Desk {
    store: Option<PinsStore>,
    here: Option<Repo>,
    pinned: Vec<Pinned>,
}

fn pinned(root: &Root) -> Pinned {
    match Repo::find(root) {
        Ok(repo) => Pinned::Ready(repo),
        Err(error) => Pinned::Broken {
            root: root.clone(),
            why: Why(error.to_string()),
        },
    }
}

fn absolute(path: &Path) -> PathBuf {
    fs::canonicalize(path).map_or_else(|_| path.to_path_buf(), io_process::plain)
}

fn clone_place(clones: &Path, url: &PinTarget) -> PathBuf {
    let url = url.as_str();
    let tail = url
        .rsplit_once(SCHEME.0)
        .map_or(url, |(_, rest)| rest)
        .replace([':', '@'], "/");
    let mut place = clones.to_path_buf();
    for part in tail
        .split('/')
        .filter(|part| !part.is_empty() && *part != UP.0)
    {
        place.push(part.strip_suffix(GIT_SUFFIX.0).unwrap_or(part));
    }
    place
}

impl Desk {
    pub fn load(here: Option<&Root>, reach: Reach) -> Self {
        let store = PinsStore::find(reach);
        let pinned: Vec<Pinned> = store
            .as_ref()
            .map(PinsStore::load)
            .unwrap_or_default()
            .iter()
            .map(pinned)
            .collect();
        let here = here.and_then(|root| Repo::find(root).ok()).filter(|repo| {
            !pinned.iter().any(|pin| match pin {
                Pinned::Ready(ready) => ready.name() == repo.name(),
                Pinned::Broken { .. } => false,
            })
        });
        Self {
            store,
            here,
            pinned,
        }
    }

    pub fn of(store: Option<PinsStore>, here: Option<Repo>, roots: &[Root]) -> Self {
        Self {
            store,
            here,
            pinned: roots.iter().map(pinned).collect(),
        }
    }

    pub fn pinned(&self) -> &[Pinned] {
        &self.pinned
    }

    pub const fn here(&self) -> Option<&Repo> {
        self.here.as_ref()
    }

    pub fn repos(&self) -> impl Iterator<Item = &Repo> {
        self.here
            .iter()
            .chain(self.pinned.iter().filter_map(|pin| match pin {
                Pinned::Ready(repo) => Some(repo),
                Pinned::Broken { .. } => None,
            }))
    }

    pub fn names(&self) -> Vec<RepoName> {
        self.repos().map(|repo| repo.name().clone()).collect()
    }

    fn roots(&self) -> Vec<Root> {
        self.pinned
            .iter()
            .map(|pin| match pin {
                Pinned::Ready(repo) => repo.root().clone(),
                Pinned::Broken { root, .. } => root.clone(),
            })
            .collect()
    }

    fn save(&self) -> Result<(), SessionError> {
        let store = self.store.as_ref().ok_or(SessionError::NoPinStore)?;
        store.save(&self.roots()).map_err(SessionError::Pins)
    }

    pub fn pin(&mut self, target: &PinTarget) -> Result<RepoName, SessionError> {
        let given = Path::new(target.as_str());
        let root = if given.exists() {
            Root::new(&absolute(given))
        } else {
            let clones = self
                .store
                .as_ref()
                .and_then(PinsStore::clones)
                .ok_or(SessionError::NoPinStore)?;
            let place = clone_place(clones, target);
            Vcs::clone_from(&RemoteUrl::new(target.as_str()), &place)?
        };
        let repo = Repo::find(&root)?;
        let name = repo.name().clone();
        if self.roots().contains(repo.root()) {
            return Err(SessionError::AlreadyPinned(name));
        }
        if self
            .here
            .as_ref()
            .is_some_and(|here| here.root() == repo.root())
        {
            self.here = None;
        }
        self.pinned.push(Pinned::Ready(repo));
        self.save()?;
        Ok(name)
    }

    pub fn unpin(&mut self, asked: &RepoName) -> Result<RepoName, SessionError> {
        let at = self
            .pinned
            .iter()
            .position(|pin| match pin {
                Pinned::Ready(repo) => repo.name().answers_to(asked),
                Pinned::Broken { root, .. } => root.as_path().to_string_lossy() == asked.as_str(),
            })
            .ok_or_else(|| SessionError::NoSuchRepo(asked.clone(), self.names()))?;
        self.pinned.remove(at);
        self.save()?;
        Ok(asked.clone())
    }

    pub fn find(&self, address: &PullAddress) -> Result<&Repo, SessionError> {
        let found: Vec<&Repo> = match &address.repo {
            Some(asked) => self
                .repos()
                .filter(|repo| repo.name().answers_to(asked))
                .collect(),
            None => match &self.here {
                Some(here) => vec![here],
                None => self.repos().collect(),
            },
        };
        match found.as_slice() {
            [one] => Ok(one),
            [] => match &address.repo {
                Some(asked) => Err(SessionError::NoSuchRepo(asked.clone(), self.names())),
                None => Err(SessionError::NoPins),
            },
            _ => Err(SessionError::Ambiguous(
                address.number,
                found.iter().map(|repo| repo.name().clone()).collect(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_address_takes_an_optional_repository_and_a_forge_prefix() {
        let full = "sharkdp/hexyl#296".parse::<PullAddress>().unwrap();
        assert_eq!(full.repo, Some(RepoName::new("sharkdp/hexyl")));
        assert_eq!(full.number.get(), 296);
        let gitlab = "cli!12".parse::<PullAddress>().unwrap();
        assert_eq!(gitlab.repo, Some(RepoName::new("cli")));
        let bare = "#7".parse::<PullAddress>().unwrap();
        assert_eq!((bare.repo, bare.number.get()), (None, 7));
        assert!("hexyl#".parse::<PullAddress>().is_err());
        assert!("twelve".parse::<PullAddress>().is_err());
    }

    #[test]
    fn a_clone_lands_under_host_and_path_without_its_suffix() {
        let base = Path::new("/clones");
        assert_eq!(
            clone_place(
                base,
                &PinTarget::new("https://github.com/sharkdp/hexyl.git")
            ),
            Path::new("/clones/github.com/sharkdp/hexyl")
        );
        assert_eq!(
            clone_place(base, &PinTarget::new("git@gitlab.com:group/../cli.git")),
            Path::new("/clones/git/gitlab.com/group/cli")
        );
    }
}

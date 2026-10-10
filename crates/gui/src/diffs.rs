use std::collections::BTreeMap;

use domain::{
    ChangeKind, CommitId, DiffLayout, FileChange, FileText, RelativePath, SourceFile, SourceLine,
};
use index::{Contents, Parsers};
use session::Repo;
use ui::Label;

use crate::app::App;
use crate::panels::View;
use crate::runtime::{Job, Landing, landed};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Viewing {
    All,
    Commit(CommitId),
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct SidesKey {
    pub(crate) commit: Option<CommitId>,
    pub(crate) path: RelativePath,
}

pub(crate) struct Sides {
    pub(crate) old: SourceFile,
    pub(crate) new: SourceFile,
}

pub(crate) enum Loading<T> {
    Asking,
    Got(T),
    Failed(Label),
}

pub(crate) struct Diffs {
    pub(crate) layout: DiffLayout,
    pub(crate) viewing: Viewing,
    pub(crate) commits: BTreeMap<CommitId, Loading<Vec<FileChange>>>,
    pub(crate) sides: BTreeMap<SidesKey, Loading<Sides>>,
}

impl Default for Diffs {
    fn default() -> Self {
        Self {
            layout: DiffLayout::Unified,
            viewing: Viewing::All,
            commits: BTreeMap::new(),
            sides: BTreeMap::new(),
        }
    }
}

impl Diffs {
    pub(crate) fn forget(&mut self) {
        self.viewing = Viewing::All;
        self.commits.clear();
        self.sides.clear();
    }

    pub(crate) const fn commit(&self) -> Option<&CommitId> {
        match &self.viewing {
            Viewing::All => None,
            Viewing::Commit(commit) => Some(commit),
        }
    }

    pub(crate) fn commit_changes(&self, commit: &CommitId) -> Option<&[FileChange]> {
        match self.commits.get(commit) {
            Some(Loading::Got(changes)) => Some(changes),
            Some(Loading::Asking | Loading::Failed(_)) | None => None,
        }
    }
}

pub(crate) struct Landed<K, T> {
    key: K,
    result: Result<T, Label>,
}

#[derive(Default)]
pub(crate) struct DiffJobs {
    commit: Option<Job<Landed<CommitId, Vec<FileChange>>>>,
    sides: Option<Job<Landed<SidesKey, Sides>>>,
    waiting: Vec<SidesKey>,
}

impl DiffJobs {
    pub(crate) const fn busy(&self) -> bool {
        self.commit.is_some() || self.sides.is_some()
    }
}

fn contents(text: &FileText) -> Contents {
    let lines: Vec<&str> = text.all().iter().map(SourceLine::as_str).collect();
    Contents::new(&lines.join("\n"))
}

fn parse(path: &RelativePath, text: &FileText) -> SourceFile {
    Parsers::default().parse(path.clone(), &contents(text))
}

fn sides_of(
    repo: &Repo,
    key: &SidesKey,
    change: &FileChange,
    head: Option<FileText>,
) -> Result<Sides, Label> {
    let new = match (&key.commit, head) {
        (_, Some(head)) => head,
        (Some(_), None) if change.kind() == ChangeKind::Deleted => FileText::default(),
        (Some(commit), None) => repo
            .read_at(commit, &key.path)
            .map_err(|error| Label::new(error.to_string()))?,
        (None, None) => FileText::default(),
    };
    let old = change.old_text(&new);
    Ok(Sides {
        old: parse(change.old_path().unwrap_or(&key.path), &old),
        new: parse(&key.path, &new),
    })
}

impl App {
    pub(crate) fn view_commit(&mut self, commit: Option<CommitId>) {
        let Some(open) = self.model.review.open.as_ref() else {
            return;
        };
        let diffs = &mut self.model.review.diffs;
        let Some(commit) = commit else {
            diffs.viewing = Viewing::All;
            self.model.panels.activate(View::Changes);
            return;
        };
        diffs.viewing = Viewing::Commit(commit.clone());
        self.model.panels.activate(View::Changes);
        if diffs.commits.contains_key(&commit) || self.jobs.diffs.commit.is_some() {
            return;
        }
        diffs.commits.insert(commit.clone(), Loading::Asking);
        let repo = open.repo.clone();
        self.jobs.diffs.commit = Some(Job::start(move || {
            let result = repo
                .commit_changes(&commit)
                .map_err(|error| Label::new(error.to_string()));
            Landed {
                key: commit,
                result,
            }
        }));
    }

    pub(crate) fn need_sides(&mut self, key: SidesKey) {
        if self.model.review.diffs.sides.contains_key(&key) {
            return;
        }
        self.model
            .review
            .diffs
            .sides
            .insert(key.clone(), Loading::Asking);
        self.jobs.diffs.waiting.push(key);
        self.start_sides();
    }

    fn start_sides(&mut self) {
        if self.jobs.diffs.sides.is_some() || self.jobs.diffs.waiting.is_empty() {
            return;
        }
        let key = self.jobs.diffs.waiting.remove(0);
        let Some(open) = self.model.review.open.as_ref() else {
            return;
        };
        let change = match &key.commit {
            Some(commit) => self
                .model
                .review
                .diffs
                .commit_changes(commit)
                .and_then(|changes| changes.iter().find(|change| change.path() == &key.path)),
            None => open.opened.change(&key.path),
        }
        .cloned();
        let Some(change) = change else {
            self.model.review.diffs.sides.insert(
                key,
                Loading::Failed(Label::new("this file is not changed there")),
            );
            return;
        };
        let head = match key.commit {
            Some(_) => None,
            None => self
                .model
                .index
                .find_file(&key.path)
                .and_then(|file| self.model.index.file(file))
                .map(|source| source.text().clone()),
        };
        let repo = open.repo.clone();
        self.jobs.diffs.sides = Some(Job::start(move || {
            let result = sides_of(&repo, &key, &change, head);
            Landed { key, result }
        }));
    }

    pub(crate) fn poll_diffs(&mut self) {
        if let Landing::Landed(Landed { key, result }) = landed(&mut self.jobs.diffs.commit) {
            let loaded = match result {
                Ok(changes) => Loading::Got(changes),
                Err(why) => Loading::Failed(why),
            };
            self.model.review.diffs.commits.insert(key, loaded);
        }
        match landed(&mut self.jobs.diffs.sides) {
            Landing::Waiting => {}
            Landing::Landed(Landed { key, result }) => {
                let loaded = match result {
                    Ok(sides) => Loading::Got(sides),
                    Err(why) => Loading::Failed(why),
                };
                self.model.review.diffs.sides.insert(key, loaded);
                self.start_sides();
            }
            Landing::Failed => self.start_sides(),
        }
    }
}

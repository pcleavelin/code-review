use domain::{CommitId, DraftId, FileChange, PrNumber, RelativePath};
use session::{Desk, Pinned, PullAddress, Repo, SessionError};

use crate::convert::{Files, Filter, Marking, Request};
use crate::failure::Failure;
use crate::output::Output;
use crate::wire;

pub(crate) struct Run<'run> {
    pub(crate) desk: &'run mut Desk,
    pub(crate) output: &'run mut Output,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Touched {
    Nothing,
    Pins,
    Review(PrNumber),
}

fn change_of<'changes>(
    changes: &'changes [FileChange],
    number: PrNumber,
    file: &RelativePath,
) -> Result<&'changes FileChange, Failure> {
    changes
        .iter()
        .find(|change| change.path() == file)
        .ok_or_else(|| Failure::NotChanged {
            number,
            file: file.clone(),
        })
}

impl Run<'_> {
    fn repo(&self, address: &PullAddress) -> Result<Repo, Failure> {
        Ok(self.desk.find(address)?.clone())
    }

    pub(crate) fn run(&mut self, request: Request) -> Result<Touched, Failure> {
        match request {
            Request::Pins => {
                let here = self.desk.here().map(|repo| repo.name().clone());
                wire::pins(self.output, here.as_ref(), self.desk.pinned());
                Ok(Touched::Nothing)
            }
            Request::Pin(target) => {
                let name = self.desk.pin(&target)?;
                wire::pinned(self.output, &name);
                Ok(Touched::Pins)
            }
            Request::Unpin(name) => {
                let name = self.desk.unpin(&name)?;
                wire::unpinned(self.output, &name);
                Ok(Touched::Pins)
            }
            Request::Pulls(filter) => self.pulls(&filter).map(|()| Touched::Nothing),
            Request::Open(address) => {
                let repo = self.repo(&address)?;
                let pull = repo.pull(address.number)?;
                let opened = repo.fetch(&pull)?;
                wire::opened(self.output, &opened);
                Ok(Touched::Review(address.number))
            }
            Request::Discussion(address) => {
                let repo = self.repo(&address)?;
                let pull = repo.pull(address.number)?;
                wire::discussion(self.output, &repo.conversation(&pull)?);
                Ok(Touched::Nothing)
            }
            Request::Changes(address) => {
                let repo = self.repo(&address)?;
                wire::changes(self.output, &repo.state(address.number)?);
                Ok(Touched::Nothing)
            }
            Request::Diff(address, file) => {
                self.diff(&address, file.as_ref())?;
                Ok(Touched::Nothing)
            }
            Request::Next(address) => {
                let repo = self.repo(&address)?;
                let opened = repo.state(address.number)?;
                match opened.review.next_unseen(&opened.changes, None) {
                    Some(change) => {
                        wire::diff(self.output, change, opened.seen(change), &opened.review);
                    }
                    None => wire::all_seen(self.output, &opened),
                }
                Ok(Touched::Nothing)
            }
            Request::Mark(address, files, marking) => self.mark(&address, &files, marking),
            Request::Comment { address, on, text } => {
                let text = text.ok_or(Failure::EmptyComment)?;
                let repo = self.repo(&address)?;
                let mut opened = repo.state(address.number)?;
                let change = change_of(&opened.changes, address.number, &on.file)?;
                if !change.commentable(on.side, on.line) {
                    return Err(Failure::NotInDiff {
                        file: on.file,
                        side: on.side,
                        line: on.line,
                    });
                }
                let id = opened.review.add_draft(on, text);
                repo.save(&opened)?;
                if let Some(draft) = opened.review.drafts().iter().find(|draft| draft.id == id) {
                    wire::draft(self.output, draft);
                }
                Ok(Touched::Review(address.number))
            }
            Request::Drafts(address) => {
                let repo = self.repo(&address)?;
                wire::drafts(self.output, &repo.review(address.number)?);
                Ok(Touched::Nothing)
            }
            Request::Uncomment(address, id) => self.uncomment(&address, id),
            Request::Submit {
                address,
                decision,
                summary,
            } => {
                let repo = self.repo(&address)?;
                let mut opened = repo.state(address.number)?;
                let submitted = repo.submit(&mut opened, decision, summary.as_ref())?;
                wire::submitted(
                    self.output,
                    address.number,
                    decision,
                    submitted.comments.get(),
                );
                Ok(Touched::Review(address.number))
            }
        }
    }

    fn pulls(&mut self, filter: &Filter) -> Result<(), Failure> {
        let mut entries = Vec::new();
        let mut broken: Vec<(String, String)> = self
            .desk
            .pinned()
            .iter()
            .filter_map(|pin| match pin {
                Pinned::Broken { root, why } => Some((
                    root.as_path().display().to_string(),
                    why.as_str().to_owned(),
                )),
                Pinned::Ready(_) => None,
            })
            .collect();
        let repos: Vec<Repo> = self.desk.repos().cloned().collect();
        if repos.is_empty() && broken.is_empty() {
            return Err(Failure::Session(SessionError::NoPins));
        }
        for repo in &repos {
            match repo.inbox() {
                Ok(found) => entries.extend(found),
                Err(error) => broken.push((repo.name().to_string(), error.to_string())),
            }
        }
        entries.retain(|entry| filter.keeps(&wire::entry_text(entry)));
        entries.sort_by(|left, right| right.pull.updated.cmp(&left.pull.updated));
        wire::inbox(self.output, &entries, &broken);
        Ok(())
    }

    fn diff(&mut self, address: &PullAddress, file: Option<&RelativePath>) -> Result<(), Failure> {
        let repo = self.repo(address)?;
        let opened = repo.state(address.number)?;
        let shown: Vec<&FileChange> = match file {
            Some(file) => vec![change_of(&opened.changes, address.number, file)?],
            None => opened.changes.iter().collect(),
        };
        for change in shown {
            wire::diff(self.output, change, opened.seen(change), &opened.review);
        }
        Ok(())
    }

    fn mark(
        &mut self,
        address: &PullAddress,
        files: &Files,
        marking: Marking,
    ) -> Result<Touched, Failure> {
        let repo = self.repo(address)?;
        let mut opened = repo.state(address.number)?;
        let chosen: Vec<&FileChange> = match files {
            Files::All => opened.changes.iter().collect(),
            Files::These(paths) => paths
                .iter()
                .map(|path| change_of(&opened.changes, address.number, path))
                .collect::<Result<_, _>>()?,
        };
        let head: CommitId = opened.head.clone();
        for change in chosen {
            match marking {
                Marking::Seen => opened.review.mark(change, head.clone()),
                Marking::Unseen => {
                    opened.review.unmark(change.path());
                }
            }
            let seen = opened.review.seen(change);
            wire::marked(self.output, change, seen);
        }
        repo.save(&opened)?;
        Ok(Touched::Review(address.number))
    }

    fn uncomment(&mut self, address: &PullAddress, id: DraftId) -> Result<Touched, Failure> {
        let repo = self.repo(address)?;
        let mut opened = repo.state(address.number)?;
        let removed = opened.review.remove_draft(id)?;
        repo.save(&opened)?;
        wire::removed(self.output, &removed);
        Ok(Touched::Review(address.number))
    }
}

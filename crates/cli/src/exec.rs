use std::collections::BTreeSet;

use domain::{
    CommitId, Depth, DraftId, FileChange, FileId, Index, Language, Line, PrNumber, RelativePath,
    Root, Seen, SourceFile, Span, Symbol, SymbolId, SymbolName, SymbolQuery,
};
use index::Servers;
use session::{Desk, Pinned, PullAddress, Repo, RepoName, SessionError};

use crate::convert::{Files, Filter, Marking, Navigation, Request};
use crate::failure::Failure;
use crate::output::Output;
use crate::wire::{self, OldSide};

pub(crate) struct Run<'run> {
    pub(crate) desk: &'run mut Desk,
    pub(crate) output: &'run mut Output,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Touched {
    Nothing,
    Pins,
    Review(PrNumber),
    Dropped(RepoName, PrNumber),
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
            Request::Diff(address, file, old) => {
                self.diff(&address, file.as_ref(), old)?;
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
                if !change.commentable(on.side, on.lines()) {
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
            Request::Drop(address) => self.drop_fetched(&address),
            Request::Navigate(address, navigation) => self
                .navigate(&address, navigation)
                .map(|()| Touched::Nothing),
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

    fn diff(
        &mut self,
        address: &PullAddress,
        file: Option<&RelativePath>,
        old: OldSide,
    ) -> Result<(), Failure> {
        let repo = self.repo(address)?;
        let opened = repo.state(address.number)?;
        let shown: Vec<&FileChange> = match file {
            Some(file) => vec![change_of(&opened.changes, address.number, file)?],
            None => opened.changes.iter().collect(),
        };
        for change in shown {
            let seen = opened.seen(change);
            let mark = opened.review.marks().get(change.path());
            match (old, seen, mark) {
                (OldSide::SeenHead, Seen::Changed, Some(mark)) => {
                    let since = repo.since_seen(&opened, change, mark)?;
                    wire::since_seen(self.output, change, mark, since.as_ref(), &opened.review);
                }
                (OldSide::SeenHead, Seen::Seen, _) => wire::marked(self.output, change, seen),
                _ => wire::diff(self.output, change, seen, &opened.review),
            }
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

    fn drop_fetched(&mut self, address: &PullAddress) -> Result<Touched, Failure> {
        let repo = self.repo(address)?;
        let tree = repo.drop_fetched(address.number)?;
        wire::dropped(self.output, address.number, &tree);
        Ok(Touched::Dropped(repo.name().clone(), address.number))
    }

    fn navigate(&mut self, address: &PullAddress, navigation: Navigation) -> Result<(), Failure> {
        let repo = self.repo(address)?;
        let opened = repo.reopen(address.number)?;
        let root = Root::new(&opened.tree);
        let mut index = index::build(&root).index;
        let mut servers = Servers::new(&root, |_| {});
        Navigator {
            index: &mut index,
            servers: &mut servers,
            output: self.output,
        }
        .run(navigation)
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

struct Navigator<'browse> {
    index: &'browse mut Index,
    servers: &'browse mut Servers,
    output: &'browse mut Output,
}

fn find_file(index: &Index, path: &RelativePath) -> Result<FileId, Failure> {
    index
        .find_file(path)
        .or_else(|| {
            index
                .file_entries()
                .find(|entry| entry.item.path().ends_with(path.as_str()))
                .map(|entry| entry.id)
        })
        .ok_or_else(|| Failure::NoSuchFile(path.clone()))
}

fn find_symbols(index: &Index, symbol: &SymbolName) -> Result<Vec<SymbolId>, Failure> {
    let found = index.find_symbols(&SymbolQuery::from(symbol.as_str()));
    if found.is_empty() {
        return Err(Failure::NoSuchSymbol(symbol.clone()));
    }
    Ok(found)
}

impl Navigator<'_> {
    fn run(&mut self, navigation: Navigation) -> Result<(), Failure> {
        match navigation {
            Navigation::TreeFiles(filter) => {
                for file in self
                    .index
                    .files()
                    .filter(|file| filter.keeps(file.path().as_str()))
                {
                    wire::file_row(self.output, file);
                }
            }
            Navigation::Symbols(filter) => {
                for file in self.index.files() {
                    for symbol in file.symbols().filter(|symbol| {
                        filter.keeps(symbol.name().as_str()) || filter.keeps(file.path().as_str())
                    }) {
                        wire::symbol_row(self.output, file, symbol);
                    }
                }
            }
            Navigation::Show { file, start, end } => self.show(&file, start, end)?,
            Navigation::Search(regex) => {
                for file in self.index.files() {
                    for (line, text) in (0..).map(Line::new).zip(file.text().all()) {
                        if regex.is_match(text.as_str()) {
                            wire::search_hit(self.output, file, line, text);
                        }
                    }
                }
            }
            Navigation::Callers(symbol) => self.callers(&symbol)?,
            Navigation::Callees(symbol) => self.callees(&symbol)?,
            Navigation::Refs(symbol) => self.references(&symbol)?,
            Navigation::Calltree(symbol, depth) => {
                self.index_calltree(&symbol, depth)?;
                for id in find_symbols(self.index, &symbol)? {
                    for entry in self.index.call_tree(id, depth) {
                        wire::symbol_line(self.output, self.index, entry.symbol, entry.depth);
                    }
                }
            }
            Navigation::Roots(count) => {
                for id in self.index.roots().into_iter().take(count.get()) {
                    wire::root_row(self.output, self.index, id);
                }
            }
        }
        Ok(())
    }

    fn show(
        &mut self,
        file: &RelativePath,
        start: Option<Line>,
        end: Option<Line>,
    ) -> Result<(), Failure> {
        let id = find_file(self.index, file)?;
        if let Some(source) = self.index.file(id)
            && let Some(last) = source.text().count().last()
            && let Some(span) = Span::new(
                start.unwrap_or(Line::new(0)).min(last),
                end.unwrap_or(last).min(last),
            )
        {
            wire::numbered_lines(self.output, source, span);
        }
        Ok(())
    }

    fn index_files(&mut self, files: Vec<RelativePath>) {
        let files: Vec<RelativePath> = files
            .into_iter()
            .filter(|path| {
                self.index
                    .find_file(path)
                    .and_then(|file| self.index.file(file))
                    .is_some_and(SourceFile::is_pending)
            })
            .collect();
        if !files.is_empty() {
            self.servers.index(self.index, &files);
        }
    }

    fn index_calltree(&mut self, symbol: &SymbolName, depth: Depth) -> Result<(), Failure> {
        let mut asked: BTreeSet<RelativePath> = BTreeSet::new();
        loop {
            let found: Vec<RelativePath> = find_symbols(self.index, symbol)?
                .into_iter()
                .flat_map(|id| self.index.call_tree(id, depth))
                .filter_map(|entry| self.index.file(entry.symbol.file()))
                .filter(|file| file.is_pending() && !asked.contains(file.path()))
                .map(|file| file.path().clone())
                .collect();
            if found.is_empty() {
                return Ok(());
            }
            asked.extend(found.iter().cloned());
            self.index_files(found);
        }
    }

    fn callees(&mut self, symbol: &SymbolName) -> Result<(), Failure> {
        let files = find_symbols(self.index, symbol)?
            .into_iter()
            .filter_map(|id| self.index.file(id.file()))
            .map(|file| file.path().clone())
            .collect();
        self.index_files(files);
        for id in find_symbols(self.index, symbol)? {
            wire::symbol_line(self.output, self.index, id, Depth::new(0));
            let callees = self
                .index
                .symbol(id)
                .map(Symbol::callees)
                .unwrap_or_default();
            for callee in callees {
                wire::symbol_line(self.output, self.index, *callee, Depth::new(1));
            }
        }
        Ok(())
    }

    fn callers(&mut self, symbol: &SymbolName) -> Result<(), Failure> {
        for id in find_symbols(self.index, symbol)? {
            wire::symbol_line(self.output, self.index, id, Depth::new(0));
            let callers: Vec<SymbolId> = match self.servers.incoming_calls(self.index, id) {
                Some(calls) => {
                    let mut found: Vec<SymbolId> = calls
                        .iter()
                        .filter_map(|call| self.index.by_line(&call.file, call.line))
                        .collect();
                    found.dedup();
                    found
                }
                None => self
                    .index
                    .symbol(id)
                    .map(|found| found.callers().to_vec())
                    .unwrap_or_default(),
            };
            for caller in callers {
                wire::symbol_line(self.output, self.index, caller, Depth::new(1));
            }
        }
        Ok(())
    }

    fn references(&mut self, symbol: &SymbolName) -> Result<(), Failure> {
        for id in find_symbols(self.index, symbol)? {
            wire::symbol_line(self.output, self.index, id, Depth::new(0));
            let Some(references) = self.servers.references(self.index, id) else {
                let program = self
                    .index
                    .file(id.file())
                    .and_then(SourceFile::language)
                    .map(Language::program);
                wire::server_missing(self.output, program);
                continue;
            };
            for location in &references {
                let text = self
                    .index
                    .find_file(&location.file)
                    .and_then(|file| self.index.file(file))
                    .and_then(|file| file.text().line(location.line));
                wire::reference_row(self.output, location, text);
            }
        }
        Ok(())
    }
}

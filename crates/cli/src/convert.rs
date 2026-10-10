use domain::{
    Decision, Depth, DraftId, DraftOn, DraftText, Line, RelativePath, Side, SymbolName, ThreadId,
};
use regex::Regex;
use session::{PinTarget, PullAddress, RepoName};

use crate::failure::{Failure, Given};
use crate::wire::{Command, CommentArguments, FilesArguments, OldSide};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Marking {
    Seen,
    Unseen,
}

#[derive(Debug)]
pub(crate) enum Files {
    All,
    These(Vec<RelativePath>),
}

#[derive(Debug, Default)]
pub(crate) struct Filter(Vec<String>);

impl Filter {
    pub(crate) fn keeps(&self, text: &str) -> bool {
        let text = text.to_lowercase();
        self.0.iter().all(|word| text.contains(word.as_str()))
    }

    fn of(words: &[String]) -> Self {
        Self(words.iter().map(|word| word.to_lowercase()).collect())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Limit(usize);

impl Limit {
    pub(crate) const fn get(self) -> usize {
        self.0
    }
}

#[derive(Debug)]
pub(crate) enum Navigation {
    TreeFiles(Filter),
    Symbols(Filter),
    Show {
        file: RelativePath,
        start: Option<Line>,
        end: Option<Line>,
    },
    Search(Regex),
    Callers(SymbolName),
    Callees(SymbolName),
    Refs(SymbolName),
    Calltree(SymbolName, Depth),
    Roots(Limit),
}

#[derive(Debug)]
pub(crate) enum Request {
    Pins,
    Pin(PinTarget),
    Unpin(RepoName),
    Pulls(Filter),
    Open(PullAddress),
    Discussion(PullAddress),
    Changes(PullAddress),
    Diff(PullAddress, Option<RelativePath>, OldSide),
    Next(PullAddress),
    Mark(PullAddress, Files, Marking),
    Comment {
        address: PullAddress,
        on: DraftOn,
        text: Option<DraftText>,
    },
    Drafts(PullAddress),
    Uncomment(PullAddress, DraftId),
    Submit {
        address: PullAddress,
        decision: Decision,
        summary: Option<DraftText>,
    },
    Drop(PullAddress),
    Navigate(PullAddress, Navigation),
}

fn files(arguments: &FilesArguments) -> Files {
    if arguments.all {
        Files::All
    } else {
        Files::These(
            arguments
                .files
                .iter()
                .map(|file| RelativePath::new(file))
                .collect(),
        )
    }
}

fn comment(arguments: CommentArguments) -> Result<Request, Failure> {
    let reply = match &arguments.reply {
        Some(text) => Some(ThreadId::new(text).ok_or_else(|| Failure::NoThread(Given::new(text)))?),
        None => None,
    };
    Ok(Request::Comment {
        on: DraftOn {
            file: RelativePath::new(&arguments.file),
            side: if arguments.old { Side::Old } else { Side::New },
            start: arguments
                .from
                .map(|first| Line::new(first.saturating_sub(1))),
            line: Line::new(arguments.line.saturating_sub(1)),
            reply,
        },
        text: DraftText::new(&arguments.text),
        address: arguments.pr,
    })
}

pub(crate) fn request(command: Command) -> Result<Request, Failure> {
    Ok(match command {
        Command::Pins => Request::Pins,
        Command::Pin(arguments) => Request::Pin(PinTarget::new(&arguments.target)),
        Command::Unpin(arguments) => Request::Unpin(RepoName::new(&arguments.repository)),
        Command::Prs(arguments) => Request::Pulls(Filter::of(&arguments.filter)),
        Command::Open(arguments) => Request::Open(arguments.pr),
        Command::Discussion(arguments) => Request::Discussion(arguments.pr),
        Command::Changes(arguments) => Request::Changes(arguments.pr),
        Command::Diff(arguments) => Request::Diff(
            arguments.pr,
            arguments.file.as_deref().map(RelativePath::new),
            if arguments.since_seen {
                OldSide::SeenHead
            } else {
                OldSide::Base
            },
        ),
        Command::Next(arguments) => Request::Next(arguments.pr),
        Command::Seen(arguments) => {
            let chosen = files(&arguments);
            Request::Mark(arguments.pr, chosen, Marking::Seen)
        }
        Command::Unseen(arguments) => {
            let chosen = files(&arguments);
            Request::Mark(arguments.pr, chosen, Marking::Unseen)
        }
        Command::Comment(arguments) => return comment(arguments),
        Command::Drafts(arguments) => Request::Drafts(arguments.pr),
        Command::Uncomment(arguments) => {
            Request::Uncomment(arguments.pr, DraftId::new(arguments.id))
        }
        Command::Drop(arguments) => Request::Drop(arguments.pr),
        Command::Files(arguments) => Request::Navigate(
            arguments.pr,
            Navigation::TreeFiles(Filter::of(&arguments.filter)),
        ),
        Command::Symbols(arguments) => Request::Navigate(
            arguments.pr,
            Navigation::Symbols(Filter::of(&arguments.filter)),
        ),
        Command::Show(arguments) => Request::Navigate(
            arguments.pr,
            Navigation::Show {
                file: RelativePath::new(&arguments.file),
                start: arguments
                    .start
                    .map(|first| Line::new(first.saturating_sub(1))),
                end: arguments.end.map(|last| Line::new(last.saturating_sub(1))),
            },
        ),
        Command::Search(arguments) => Request::Navigate(
            arguments.pr,
            Navigation::Search(Regex::new(&arguments.regex).map_err(Failure::BadPattern)?),
        ),
        Command::Callers(arguments) => Request::Navigate(
            arguments.pr,
            Navigation::Callers(SymbolName::new(&arguments.symbol)),
        ),
        Command::Callees(arguments) => Request::Navigate(
            arguments.pr,
            Navigation::Callees(SymbolName::new(&arguments.symbol)),
        ),
        Command::Refs(arguments) => Request::Navigate(
            arguments.pr,
            Navigation::Refs(SymbolName::new(&arguments.symbol)),
        ),
        Command::Calltree(arguments) => Request::Navigate(
            arguments.pr,
            Navigation::Calltree(
                SymbolName::new(&arguments.symbol),
                Depth::new(arguments.depth),
            ),
        ),
        Command::Roots(arguments) => {
            Request::Navigate(arguments.pr, Navigation::Roots(Limit(arguments.limit)))
        }
        Command::Submit(arguments) => Request::Submit {
            address: arguments.pr,
            decision: if arguments.approve {
                Decision::Approve
            } else if arguments.request_changes {
                Decision::RequestChanges
            } else {
                Decision::Comment
            },
            summary: arguments.summary.as_deref().and_then(DraftText::new),
        },
    })
}

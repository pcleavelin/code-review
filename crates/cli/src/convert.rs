use domain::{Decision, DraftId, DraftOn, DraftText, Line, RelativePath, Side, ThreadId};
use session::{PinTarget, PullAddress, RepoName};

use crate::failure::{Failure, Given};
use crate::wire::{Command, CommentArguments, FilesArguments};

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
    Diff(PullAddress, Option<RelativePath>),
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
        Command::Prs(arguments) => Request::Pulls(Filter(
            arguments
                .filter
                .iter()
                .map(|word| word.to_lowercase())
                .collect(),
        )),
        Command::Open(arguments) => Request::Open(arguments.pr),
        Command::Discussion(arguments) => Request::Discussion(arguments.pr),
        Command::Changes(arguments) => Request::Changes(arguments.pr),
        Command::Diff(arguments) => Request::Diff(
            arguments.pr,
            arguments.file.as_deref().map(RelativePath::new),
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

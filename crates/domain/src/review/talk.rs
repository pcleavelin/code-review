use std::fmt;

use crate::picture::SignedPicture;
use crate::review::{CommitId, Decision, Login, Side, WebLink};
use crate::text::{Line, RelativePath};

#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Moment(String);

impl Moment {
    pub fn new(text: &str) -> Self {
        Self(text.trim().to_owned())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn day(&self) -> &str {
        self.0.get(..10).unwrap_or(&self.0)
    }

    pub fn minute(&self) -> String {
        let date = self.day();
        let time = self.0.get(11..16).unwrap_or_default();
        if time.is_empty() {
            date.to_owned()
        } else {
            format!("{date} {time}")
        }
    }
}

impl fmt::Display for Moment {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.minute())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Prose(String);

impl Prose {
    pub fn new(text: &str) -> Self {
        Self(
            text.lines()
                .collect::<Vec<_>>()
                .join("\n")
                .trim()
                .to_owned(),
        )
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Display for Prose {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Commit {
    pub id: CommitId,
    pub author: Login,
    pub when: Moment,
    pub subject: Prose,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ThreadId(String);

impl ThreadId {
    pub fn new(text: &str) -> Option<Self> {
        let text = text.trim();
        (!text.is_empty() && !text.contains(char::is_whitespace)).then(|| Self(text.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ThreadId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note {
    pub author: Login,
    pub body: Prose,
    pub when: Moment,
    pub link: Option<WebLink>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CodePlace {
    pub file: RelativePath,
    pub side: Side,
    pub line: Line,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ThreadState {
    Open,
    Resolved,
    Outdated,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SubmissionId(String);

impl SubmissionId {
    pub fn new(text: &str) -> Option<Self> {
        let text = text.trim();
        (!text.is_empty() && !text.contains(char::is_whitespace)).then(|| Self(text.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SubmissionId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Submission {
    pub id: SubmissionId,
    pub author: Login,
    pub decision: Decision,
    pub summary: Prose,
    pub when: Moment,
    pub link: Option<WebLink>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Thread {
    pub id: ThreadId,
    pub place: Option<CodePlace>,
    pub state: ThreadState,
    pub notes: Vec<Note>,
    pub submission: Option<SubmissionId>,
}

impl Thread {
    pub fn started(&self) -> Option<&Moment> {
        self.notes.first().map(|note| &note.when)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Block<'talk> {
    Submitted {
        submission: &'talk Submission,
        threads: Vec<&'talk Thread>,
    },
    Alone(&'talk Thread),
}

impl Block<'_> {
    pub fn when(&self) -> Option<&Moment> {
        match self {
            Self::Submitted { submission, .. } => Some(&submission.when),
            Self::Alone(thread) => thread.started(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Conversation {
    pub description: Prose,
    pub submissions: Vec<Submission>,
    pub threads: Vec<Thread>,
    pub signed_pictures: Vec<SignedPicture>,
}

impl Conversation {
    pub fn blocks(&self) -> Vec<Block<'_>> {
        let known = |thread: &Thread| {
            thread.submission.as_ref().is_some_and(|id| {
                self.submissions
                    .iter()
                    .any(|submission| &submission.id == id)
            })
        };
        let mut blocks: Vec<Block<'_>> = self
            .submissions
            .iter()
            .filter_map(|submission| {
                let threads: Vec<&Thread> = self
                    .threads
                    .iter()
                    .filter(|thread| thread.submission.as_ref() == Some(&submission.id))
                    .collect();
                let said = submission.decision != Decision::Comment
                    || !submission.summary.is_empty()
                    || !threads.is_empty();
                said.then_some(Block::Submitted {
                    submission,
                    threads,
                })
            })
            .chain(
                self.threads
                    .iter()
                    .filter(|thread| !known(thread))
                    .map(Block::Alone),
            )
            .collect();
        blocks.sort_by(|left, right| left.when().cmp(&right.when()));
        blocks
    }

    pub fn on_code(&self) -> impl Iterator<Item = &Thread> {
        self.threads.iter().filter(|thread| thread.place.is_some())
    }

    pub fn on_pull(&self) -> impl Iterator<Item = &Thread> {
        self.threads.iter().filter(|thread| thread.place.is_none())
    }

    pub fn drawn_in<'talk>(
        &'talk self,
        file: &'talk RelativePath,
    ) -> impl Iterator<Item = &'talk Thread> {
        self.threads.iter().filter(move |thread| {
            thread.state != ThreadState::Outdated
                && thread
                    .place
                    .as_ref()
                    .is_some_and(|place| &place.file == file)
        })
    }

    pub fn thread(&self, id: &ThreadId) -> Option<&Thread> {
        self.threads.iter().find(|thread| &thread.id == id)
    }
}

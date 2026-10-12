use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::time::{Duration, Instant};

use domain::{
    CodePlace, CommitId, Conversation, Decision, DiffLayout, Draft, DraftId, DraftOn, DraftText,
    FileChange, FileRow, FileText, Forge, ForgeHost, Hunk, Index, Line, LineKind, Linked,
    LinkedFile, PrNumber, PullRequest, RelativePath, Revision, Rights, Root, Seen, Side, Span,
    Standing, WebLink,
};
use index::Indexed;
use io_process::{Argument, Opener};
use io_review::ReviewStamp;
use platform::ClipboardRequest;
use session::{
    Access, Desk, Entry, Opened, PinTarget, Pinned, Reach, Repo, RepoName, RequestedFrom,
};
use ui::{Count, Label, Typed};

use crate::app::App;
use crate::diffs::{DiffJobs, Diffs, Loading, SidesKey};
use crate::field::Which;
use crate::keys::Going;
use crate::login::{self, Ended, Held, LoginStatus, Logins};
use crate::model::{LineSelection, Model, Tab};
use crate::nav::Aim;
use crate::panels::View;
use crate::pictures::{PictureJobs, Pictures};
use crate::runtime::{Job, Landing, landed};
use crate::status::{Status, Tone};
use crate::text::{Counted, Noun};
use crate::work::{Services, WorkState};

const CONTEXT: Line = Line::new(3);
const REVEALED: Line = Line::new(8);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Broken {
    pub(crate) place: Label,
    pub(crate) why: Label,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Cure {
    LogIn,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Unanswered {
    pub(crate) broken: Broken,
    pub(crate) cure: Cure,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Requests {
    pub(crate) pulls: BTreeMap<RepoName, BTreeSet<PrNumber>>,
    pub(crate) answered: Count,
    pub(crate) unanswered: Vec<Unanswered>,
}

impl Requests {
    pub(crate) fn of(&self, entry: &Entry) -> bool {
        self.pulls
            .get(&entry.repo)
            .is_some_and(|numbers| numbers.contains(&entry.pull.number))
    }

    pub(crate) fn locked(&self) -> bool {
        self.answered == Count::ZERO
            && self
                .unanswered
                .iter()
                .any(|unanswered| unanswered.cure == Cure::LogIn)
    }

    pub(crate) fn chip(&self, entries: &[Entry]) -> Label {
        if self.locked() {
            Label::new("requested: log in")
        } else {
            let count = entries.iter().filter(|entry| self.of(entry)).count();
            Label::new(format!("requested {count}"))
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Inbox {
    Asking,
    Listed {
        entries: Vec<Entry>,
        broken: Vec<Broken>,
        requests: Requests,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Talk {
    Asking,
    Got(Conversation),
    Failed(Label),
}

pub(crate) enum LinkTarget {
    Browser(WebLink),
    Source(LinkedFile, WebLink),
}

impl LinkTarget {
    pub(crate) fn act(self) -> ReviewAct {
        match self {
            Self::Browser(web) => ReviewAct::OpenLink(web),
            Self::Source(file, web) => ReviewAct::ShowLinkedFile(file, web),
        }
    }
}

pub(crate) struct OpenReview {
    pub(crate) repo: Repo,
    pub(crate) pull: PullRequest,
    pub(crate) opened: Opened,
    pub(crate) talk: Talk,
    pub(crate) rights: Option<Result<Rights, Label>>,
    pub(crate) web: Option<Forge>,
}

impl OpenReview {
    pub(crate) fn link(&self, linked: &Linked) -> Option<WebLink> {
        self.web
            .as_ref()
            .and_then(|forge| io_forge::web_link(forge, linked))
    }

    fn linked_file(&self, link: &WebLink) -> Option<LinkedFile> {
        let branches = [self.pull.head.clone(), self.pull.base.clone()];
        self.web
            .as_ref()
            .and_then(|forge| io_forge::linked_file(forge, link, &branches))
    }

    pub(crate) fn conversation(&self) -> Option<&Conversation> {
        match &self.talk {
            Talk::Got(conversation) => Some(conversation),
            Talk::Asking | Talk::Failed(_) => None,
        }
    }
}

pub(crate) struct Landed {
    repo: Repo,
    pull: PullRequest,
    opened: Opened,
    indexed: Indexed,
}

pub(crate) struct Submitted {
    opened: Opened,
    decision: Decision,
    comments: Count,
}

pub(crate) struct Listed {
    entries: Vec<Entry>,
    broken: Vec<Broken>,
    requests: Requests,
    held: BTreeMap<ForgeHost, Held>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Relist {
    #[default]
    Settled,
    Again,
}

#[derive(Default)]
pub(crate) struct ReviewJobs {
    list: Option<Job<Listed>>,
    open: Option<Job<Result<Landed, Label>>>,
    talk: Option<Job<Result<Conversation, Label>>>,
    rights: Option<Job<Result<Rights, Label>>>,
    submit: Option<Job<Result<Submitted, Label>>>,
    pin: Option<Job<Result<PinLanded, Label>>>,
    pub(crate) login: Option<Job<Ended>>,
    relist: Relist,
    pub(crate) diffs: DiffJobs,
    pub(crate) pictures: PictureJobs,
}

struct PinLanded {
    desk: Desk,
    name: RepoName,
}

impl ReviewJobs {
    pub(crate) fn busy(&self) -> bool {
        self.list.is_some()
            || self.open.is_some()
            || self.talk.is_some()
            || self.rights.is_some()
            || self.submit.is_some()
            || self.pin.is_some()
            || self.diffs.busy()
            || self.pictures.busy()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Submitting {
    Idle,
    Sending,
    Asking(Decision),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Pinning {
    Idle,
    Busy,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RemovedPick {
    pub(crate) file: RelativePath,
    pub(crate) from: Line,
    pub(crate) to: Line,
}

impl RemovedPick {
    pub(crate) fn lines(&self) -> Span {
        Span::new(self.from.min(self.to), self.from.max(self.to))
            .unwrap_or_else(|| Span::line(self.from))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Picking {
    Press,
    Extend,
    Drag,
}

pub(crate) struct Reviewing {
    pub(crate) desk: Desk,
    pub(crate) inbox: Inbox,
    pub(crate) standing: Option<Standing>,
    pub(crate) requested: RequestedFrom,
    pub(crate) open: Option<OpenReview>,
    pub(crate) opening: Option<PrNumber>,
    pub(crate) pinning: Pinning,
    pub(crate) draft: Option<DraftOn>,
    pub(crate) picked: Option<RemovedPick>,
    pub(crate) submitting: Submitting,
    pub(crate) whole: BTreeSet<RelativePath>,
    pub(crate) unfolded: BTreeMap<RelativePath, Vec<Span>>,
    pub(crate) diffs: Diffs,
    pub(crate) pictures: Pictures,
    pub(crate) logins: Logins,
    stamp: Option<ReviewStamp>,
    last_poll: Instant,
}

impl Reviewing {
    pub(crate) fn find(here: &Root, reach: Reach) -> Self {
        Self::of(Desk::load(Some(here), reach))
    }

    pub(crate) fn of(desk: Desk) -> Self {
        Self {
            desk,
            inbox: Inbox::Asking,
            standing: None,
            requested: RequestedFrom::Any,
            open: None,
            opening: None,
            pinning: Pinning::Idle,
            draft: None,
            picked: None,
            submitting: Submitting::Idle,
            whole: BTreeSet::new(),
            unfolded: BTreeMap::new(),
            diffs: Diffs::default(),
            pictures: Pictures::default(),
            logins: Logins::default(),
            stamp: None,
            last_poll: Instant::now(),
        }
    }

    pub(crate) fn opened(&self) -> Option<&Opened> {
        self.open.as_ref().map(|open| &open.opened)
    }

    fn opened_mut(&mut self) -> Option<&mut Opened> {
        self.open.as_mut().map(|open| &mut open.opened)
    }

    pub(crate) fn change(&self, path: &RelativePath) -> Option<&FileChange> {
        self.opened()?.change(path)
    }

    pub(crate) fn shown_change(&self, path: &RelativePath) -> Option<&FileChange> {
        match self.diffs.commit() {
            Some(commit) => self
                .diffs
                .commit_changes(commit)?
                .iter()
                .find(|change| change.path() == path),
            None => self.change(path),
        }
    }

    pub(crate) fn seen(&self, change: &FileChange) -> Seen {
        self.opened()
            .map_or(Seen::Unseen, |opened| opened.seen(change))
    }

    pub(crate) fn drafts_in<'review>(
        &'review self,
        path: &'review RelativePath,
    ) -> impl Iterator<Item = &'review Draft> {
        self.opened()
            .into_iter()
            .flat_map(move |opened| opened.review.drafts_at(path))
    }

    pub(crate) fn rows(&self, path: &RelativePath, head: Option<&FileText>) -> Vec<FileRow> {
        self.change(path)
            .map(|change| change.rows(head))
            .unwrap_or_default()
    }

    pub(crate) fn shown_spans(&self, path: &RelativePath, last: Line) -> Vec<Span> {
        let whole = || Span::new(Line::new(0), last).into_iter().collect();
        let Some(change) = self.change(path) else {
            return whole();
        };
        if self.whole.contains(path) {
            return whole();
        }
        let mut spans: Vec<Span> = change
            .hunks()
            .iter()
            .filter_map(|hunk| {
                let lines: Vec<Line> = hunk.numbered().filter_map(|line| line.new).collect();
                let first = lines.first().copied().unwrap_or(hunk.new_start());
                let end = lines.last().copied().unwrap_or(hunk.new_start());
                Span::new(
                    Line::new(first.value().saturating_sub(CONTEXT.value())).min(last),
                    Line::new(end.value().saturating_add(CONTEXT.value())).min(last),
                )
            })
            .collect();
        spans.extend(
            self.unfolded
                .get(path)
                .into_iter()
                .flatten()
                .filter_map(|span| Span::new(span.start().min(last), span.end().min(last))),
        );
        spans.extend(
            self.drafts_in(path)
                .filter(|draft| draft.side == Side::New)
                .filter_map(|draft| {
                    let lines = draft.lines();
                    Span::new(lines.start().min(last), lines.end().min(last))
                }),
        );
        merged(spans)
    }

    fn reveal(&mut self, path: &RelativePath, line: Line) {
        if let Some(span) = Span::new(
            Line::new(line.value().saturating_sub(REVEALED.value())),
            Line::new(line.value().saturating_add(REVEALED.value())),
        ) {
            self.unfolded.entry(path.clone()).or_default().push(span);
        }
    }

    pub(crate) fn place_line(&self, place: &CodePlace, head: Option<&FileText>) -> Line {
        match place.side {
            Side::New => place.line,
            Side::Old => {
                let rows = self.rows(&place.file, head);
                let at = rows.iter().position(
                    |row| matches!(row, FileRow::Removed { old, .. } if *old == place.line),
                );
                at.and_then(|at| {
                    rows.iter().skip(at).find_map(|row| match row {
                        FileRow::Head { line, .. } => Some(*line),
                        FileRow::Removed { .. } => None,
                    })
                })
                .or_else(|| head.and_then(|text| text.count().last()))
                .unwrap_or(Line::new(0))
            }
        }
    }
}

pub(crate) struct CopiedLines {
    pub(crate) text: Label,
    pub(crate) lines: Count,
}

fn line_labels(text: &FileText, span: Span) -> Vec<Label> {
    text.lines(span)
        .into_iter()
        .flatten()
        .map(|line| Label::new(line.as_str()))
        .collect()
}

pub(crate) fn merged(mut spans: Vec<Span>) -> Vec<Span> {
    spans.sort();
    let mut out: Vec<Span> = Vec::new();
    for span in spans {
        match out.last_mut() {
            Some(last) if span.start().value() <= last.end().value().saturating_add(1) => {
                if let Some(joined) = Span::new(last.start(), last.end().max(span.end())) {
                    *last = joined;
                }
            }
            _ => out.push(span),
        }
    }
    out
}

fn block_starts(hunk: &Hunk) -> Vec<Line> {
    let mut starts = Vec::new();
    let mut cursor = hunk.new_start();
    let mut inside = false;
    for line in hunk.numbered() {
        match (line.kind, line.new) {
            (LineKind::Context, Some(new)) => {
                inside = false;
                cursor = new.next().unwrap_or(new);
            }
            (LineKind::Added, Some(new)) => {
                if !inside {
                    starts.push(new);
                }
                inside = true;
                cursor = new.next().unwrap_or(new);
            }
            (LineKind::Removed, _) => {
                if !inside {
                    starts.push(cursor);
                }
                inside = true;
            }
            _ => {}
        }
    }
    starts
}

pub(crate) fn change_starts(change: &FileChange) -> Vec<Line> {
    change.hunks().iter().flat_map(block_starts).collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ReviewAct {
    RefreshPulls,
    OpenPull(RepoName, PrNumber),
    Pin,
    Unpin(RepoName),
    ShowStanding(Option<Standing>),
    ShowRequested(RequestedFrom),
    OpenChange(RelativePath),
    OpenPlace(CodePlace),
    ToggleSeen(RelativePath),
    ToggleShownSeen,
    NextUnseen,
    NextHunk,
    PreviousHunk,
    ToggleWhole(RelativePath),
    Unfold(RelativePath, Span),
    PickRemoved(RelativePath, Line, Picking),
    Reveal(RelativePath, Line),
    StartDraft(DraftOn),
    StartDraftHere,
    CopyLines,
    SaveDraft,
    CancelDraft,
    RemoveDraft(DraftId),
    RefreshTalk,
    AskSubmit(Decision),
    CancelSubmit,
    Submit(Decision),
    OpenLink(WebLink),
    ShowLinkedFile(LinkedFile, WebLink),
    CopyLink(WebLink),
    ViewCommit(Option<CommitId>),
    NeedPicture(WebLink),
    PictureWake(Duration),
    NeedSides(SidesKey),
    ToggleLayout,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ReviewStatus {
    Listed(Count),
    Pinned(RepoName),
    Unpinned(RepoName),
    PinFailed(Label),
    Opening(PrNumber),
    Opened {
        number: PrNumber,
        files: Count,
    },
    OpenFailed(Label),
    TalkFailed(Label),
    Marked {
        path: RelativePath,
        seen: Seen,
    },
    AllSeen,
    LastChange,
    NotOpened,
    NotAChange,
    SelectALine,
    NotInDiff,
    EmptyDraft,
    Drafted(DraftId),
    DraftRemoved(DraftId),
    NotSaved(Label),
    Busy,
    Submitting,
    Submitted {
        decision: Decision,
        comments: Count,
    },
    SubmitFailed(Label),
    Reloaded,
    LinkOpened(WebLink),
    LinkedFileShown(RelativePath),
    LinkedFileShownAtHead {
        path: RelativePath,
        linked: Revision,
    },
    LinkCopied(WebLink),
    LinkFailed(Label),
    CommitComment,
    LinesCopied(Count),
    NothingToCopy,
}

impl ReviewStatus {
    pub(crate) const fn tone(&self) -> Tone {
        match self {
            Self::Listed(_)
            | Self::Pinned(_)
            | Self::Unpinned(_)
            | Self::Opened { .. }
            | Self::Marked { .. }
            | Self::AllSeen
            | Self::Drafted(_)
            | Self::DraftRemoved(_)
            | Self::Submitted { .. }
            | Self::LinkOpened(_)
            | Self::LinkedFileShown(_)
            | Self::LinkCopied(_)
            | Self::LinesCopied(_)
            | Self::Reloaded => Tone::Done,
            Self::NotOpened
            | Self::NotAChange
            | Self::SelectALine
            | Self::NothingToCopy
            | Self::NotInDiff
            | Self::EmptyDraft
            | Self::LastChange
            | Self::CommitComment
            | Self::LinkedFileShownAtHead { .. }
            | Self::Busy => Tone::Warning,
            Self::PinFailed(_)
            | Self::OpenFailed(_)
            | Self::TalkFailed(_)
            | Self::NotSaved(_)
            | Self::LinkFailed(_)
            | Self::SubmitFailed(_) => Tone::Problem,
            Self::Opening(_) | Self::Submitting => Tone::Plain,
        }
    }
}

impl fmt::Display for ReviewStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Listed(count) => write!(formatter, "{count} open pull requests"),
            Self::Pinned(name) => write!(formatter, "pinned {name}"),
            Self::Unpinned(name) => {
                write!(formatter, "unpinned {name}; its reviews stay on disk")
            }
            Self::PinFailed(why) => write!(formatter, "not pinned: {}", why.as_str()),
            Self::Opening(number) => write!(
                formatter,
                "opening #{number}: fetching, checking out its head and indexing it"
            ),
            Self::Opened { number, files } => {
                write!(formatter, "#{number} open: {files} changed files")
            }
            Self::OpenFailed(why) => write!(formatter, "open failed: {}", why.as_str()),
            Self::TalkFailed(why) => {
                write!(formatter, "the discussion could not be read: {}", why.as_str())
            }
            Self::Marked { path, seen } => match seen {
                Seen::Seen => write!(formatter, "{path} seen"),
                Seen::Unseen | Seen::Changed => write!(formatter, "{path} unseen"),
            },
            Self::AllSeen => formatter.write_str("every changed file is seen"),
            Self::LastChange => formatter.write_str("no change after this one"),
            Self::NotOpened => formatter.write_str("open a pull request from the Inbox first"),
            Self::NotAChange => {
                formatter.write_str("the file shown is not changed by this pull request")
            }
            Self::SelectALine => formatter.write_str("click a line of a changed file first"),
            Self::NotInDiff => formatter.write_str(
                "that line is outside the diff; a comment goes on a tinted line, a removed line or the lines around them",
            ),
            Self::EmptyDraft => formatter.write_str("type the comment first; escape cancels it"),
            Self::Drafted(id) => write!(formatter, "draft {id} saved; it is sent with the review"),
            Self::DraftRemoved(id) => write!(formatter, "draft {id} removed"),
            Self::NotSaved(why) => write!(formatter, "review not saved: {}", why.as_str()),
            Self::Busy => formatter.write_str("still working on the last request"),
            Self::Submitting => formatter.write_str("submitting the review"),
            Self::Submitted { decision, comments } => {
                let what = match decision {
                    Decision::Comment => "review sent",
                    Decision::Approve => "approved",
                    Decision::RequestChanges => "changes requested",
                };
                write!(formatter, "{what} with {comments} inline comments")
            }
            Self::SubmitFailed(why) => write!(formatter, "submit failed: {}", why.as_str()),
            Self::Reloaded => formatter.write_str("review reloaded (changed outside the window)"),
            Self::LinkOpened(link) => write!(formatter, "opened {link}"),
            Self::LinkedFileShown(path) => write!(formatter, "{path} from the link"),
            Self::LinkedFileShownAtHead { path, linked } => write!(
                formatter,
                "{path} from the link, shown at the head of the pull request: the link points at {linked}"
            ),
            Self::CommitComment => formatter.write_str(
                "comments go on the whole pull request's changes: pick 'all changes' first",
            ),
            Self::LinesCopied(count) => write!(formatter, "copied {}", Counted::new(*count, Noun::Line)),
            Self::NothingToCopy => formatter.write_str("select lines to copy first"),
            Self::LinkCopied(link) => write!(formatter, "copied {link}"),
            Self::LinkFailed(why) => {
                write!(formatter, "the browser did not start: {}", why.as_str())
            }
        }
    }
}

fn list(repos: &[Repo], pinned: &[Pinned], access: &Access, sites: &BTreeSet<ForgeHost>) -> Listed {
    let held = login::holding(access, sites);
    let mut entries = Vec::new();
    let mut broken: Vec<Broken> = pinned
        .iter()
        .filter_map(|pin| match pin {
            Pinned::Broken { root, why } => Some(Broken {
                place: Label::new(root.as_path().display().to_string()),
                why: Label::new(why.as_str()),
            }),
            Pinned::Ready(_) => None,
        })
        .collect();
    let mut requests = Requests::default();
    for repo in repos {
        match repo.inbox() {
            Ok(found) => {
                entries.extend(found);
                ask_requests(repo, &mut requests);
            }
            Err(error) => broken.push(Broken {
                place: Label::new(repo.name().as_str()),
                why: Label::new(error.to_string()),
            }),
        }
    }
    entries.sort_by(|left, right| right.pull.updated.cmp(&left.pull.updated));
    Listed {
        entries,
        broken,
        requests,
        held,
    }
}

fn ask_requests(repo: &Repo, requests: &mut Requests) {
    match repo.requested() {
        Ok(numbers) => {
            requests.answered += Count::new(1);
            requests
                .pulls
                .entry(repo.name().clone())
                .or_default()
                .extend(numbers);
        }
        Err(error) => requests.unanswered.push(Unanswered {
            broken: Broken {
                place: Label::new(repo.name().as_str()),
                why: Label::new(error.to_string()),
            },
            cure: if error.lacks_token() {
                Cure::LogIn
            } else {
                Cure::Unknown
            },
        }),
    }
}

fn open(repo: Repo, pull: PullRequest) -> Result<Landed, Label> {
    let opened = repo
        .fetch(&pull)
        .map_err(|error| Label::new(error.to_string()))?;
    let indexed = index::build(&Root::new(&opened.tree));
    Ok(Landed {
        repo,
        pull,
        opened,
        indexed,
    })
}

fn submit(
    repo: &Repo,
    mut opened: Opened,
    decision: Decision,
    body: Option<&DraftText>,
) -> Result<Submitted, Label> {
    let comments = opened.review.drafts().len();
    repo.submit(&mut opened, decision, body)
        .map_err(|error| Label::new(error.to_string()))?;
    Ok(Submitted {
        opened,
        decision,
        comments: Count::new(comments),
    })
}

impl Model {
    fn say(&mut self, status: ReviewStatus) {
        self.status = Status::Review(status);
    }

    pub(crate) fn link_target(&self, link: WebLink) -> LinkTarget {
        match self
            .review
            .open
            .as_ref()
            .and_then(|open| open.linked_file(&link))
            .filter(|file| self.index.find_file(&file.path).is_some())
        {
            Some(file) => LinkTarget::Source(file, link),
            None => LinkTarget::Browser(link),
        }
    }

    pub(crate) fn shown_path(&self) -> Option<RelativePath> {
        self.nav
            .file()
            .and_then(|file| self.index.file(file))
            .map(|source| source.path().clone())
            .or_else(|| self.nav.gone().cloned())
    }

    fn head_text(&self, path: &RelativePath) -> Option<&FileText> {
        let file = self.index.find_file(path)?;
        self.index.file(file).map(domain::SourceFile::text)
    }

    pub(crate) fn open_change(&mut self, path: &RelativePath) {
        let first = self
            .review
            .shown_change(path)
            .and_then(|change| change_starts(change).first().copied());
        match self.index.find_file(path) {
            Some(file) => self.open_line(file, first.unwrap_or(Line::new(0))),
            None => self.open_gone(path.clone()),
        }
    }

    pub(crate) fn open_place(&mut self, place: &CodePlace) {
        let line = self.review.place_line(place, self.head_text(&place.file));
        match self.index.find_file(&place.file) {
            Some(file) => self.open_line(file, line),
            None => self.show_gone(place.file.clone(), place.line, Aim::Line),
        }
    }

    fn next_unseen(&mut self) {
        let shown = self.shown_path();
        let next = self.review.opened().and_then(|opened| {
            opened
                .review
                .next_unseen(&opened.changes, shown.as_ref())
                .map(|change| change.path().clone())
        });
        match (next, self.review.open.is_some()) {
            (Some(path), _) => self.open_change(&path),
            (None, true) => self.say(ReviewStatus::AllSeen),
            (None, false) => self.say(ReviewStatus::NotOpened),
        }
    }

    fn next_file(&mut self, shown: Option<&RelativePath>) {
        let next = self.review.opened().and_then(|opened| {
            let position = shown.and_then(|path| {
                opened
                    .changes
                    .iter()
                    .position(|change| change.path() == path)
            });
            let next = position.map_or(0, |at| at + 1);
            opened.changes.get(next).map(|change| change.path().clone())
        });
        match next {
            Some(path) => self.open_change(&path),
            None => self.say(ReviewStatus::LastChange),
        }
    }

    fn step_hunk(&mut self, going: Going) {
        let forward = going == Going::Forward;
        if self.review.open.is_none() {
            self.say(ReviewStatus::NotOpened);
            return;
        }
        let shown = self.shown_path();
        let here = self.nav.lines().map(LineSelection::low);
        let starts = shown
            .as_ref()
            .and_then(|path| self.review.shown_change(path))
            .map(change_starts)
            .unwrap_or_default();
        let target = if forward {
            starts
                .iter()
                .copied()
                .find(|start| here.is_none_or(|here| *start > here))
        } else {
            starts
                .iter()
                .rev()
                .copied()
                .find(|start| here.is_some_and(|here| *start < here))
        };
        match (target, shown) {
            (Some(line), Some(path)) => match self.index.find_file(&path) {
                Some(file) => self.open_line(file, line),
                None => self.open_gone(path),
            },
            (None, shown) if forward => self.next_file(shown.as_ref()),
            (_, _) => {}
        }
    }

    fn show_editing(&mut self, on: &DraftOn) {
        let place = CodePlace {
            file: on.file.clone(),
            side: on.side,
            line: on.line,
        };
        let line = self.review.place_line(&place, self.head_text(&place.file));
        match self.index.find_file(&place.file) {
            Some(file) => self.show_editor(file, line),
            None => self.show_gone(place.file, place.line, Aim::Editor),
        }
    }

    fn draft_here(&self) -> Option<DraftOn> {
        let file = self.shown_path()?;
        if let Some(picked) = self
            .review
            .picked
            .as_ref()
            .filter(|picked| picked.file == file)
        {
            let lines = picked.lines();
            return Some(DraftOn {
                file,
                side: Side::Old,
                start: Some(lines.start()).filter(|start| *start < lines.end()),
                line: lines.end(),
                reply: None,
            });
        }
        let lines = self.nav.lines()?;
        Some(DraftOn {
            file,
            side: Side::New,
            start: Some(lines.low()).filter(|low| *low < lines.high()),
            line: lines.high(),
            reply: None,
        })
    }

    pub(crate) fn copied_lines(&self) -> Option<CopiedLines> {
        let file = self.shown_path()?;
        let head = self.head_text(&file);
        let key = SidesKey {
            commit: self.review.diffs.commit().cloned(),
            path: file.clone(),
        };
        let sides = match self.review.diffs.sides.get(&key) {
            Some(Loading::Got(sides)) => Some(sides),
            Some(Loading::Asking | Loading::Failed(_)) | None => None,
        };
        let picked = self
            .review
            .picked
            .as_ref()
            .filter(|picked| picked.file == file)
            .map(RemovedPick::lines);
        let lines: Vec<Label> = match (picked, sides) {
            (Some(span), Some(sides)) => line_labels(sides.old.text(), span),
            (Some(span), None) => self
                .review
                .rows(&file, head)
                .into_iter()
                .filter_map(|row| match row {
                    FileRow::Removed { old, text } if span.contains(old) => {
                        Some(Label::new(text.as_str()))
                    }
                    FileRow::Removed { .. } | FileRow::Head { .. } => None,
                })
                .collect(),
            (None, _) => {
                let selection = self.nav.lines()?;
                let span = Span::new(selection.low(), selection.high())?;
                let new = sides.map(|sides| sides.new.text()).or(head)?;
                line_labels(new, span)
            }
        };
        (!lines.is_empty()).then(|| CopiedLines {
            lines: Count::new(lines.len()),
            text: Label::new(lines.iter().fold(String::new(), |mut text, line| {
                text.push_str(line.as_str());
                text.push('\n');
                text
            })),
        })
    }

    pub(crate) fn starts_comment(&self, typed: &Typed) -> bool {
        !typed.as_str().trim().is_empty()
            && self.nav.tab() == Tab::Source
            && self.review.draft.is_none()
            && (self.nav.lines().is_some() || self.review.picked.is_some())
    }

    fn pick_removed(&mut self, file: RelativePath, line: Line, picking: Picking) {
        let kept = self
            .review
            .picked
            .take()
            .filter(|picked| picked.file == file);
        self.review.picked = Some(match (picking, kept) {
            (Picking::Extend | Picking::Drag, Some(picked)) => RemovedPick { to: line, ..picked },
            _ => RemovedPick {
                file,
                from: line,
                to: line,
            },
        });
        self.clear_lines();
    }
}

impl App {
    fn open_link(&mut self, link: WebLink) {
        match Opener::open(&Argument::new(link.as_str())) {
            Ok(()) => self.model.say(ReviewStatus::LinkOpened(link)),
            Err(error) => self
                .model
                .say(ReviewStatus::LinkFailed(Label::new(format!("{error:?}")))),
        }
    }

    fn show_linked_file(&mut self, linked: LinkedFile, link: WebLink) {
        let Some(file) = self.model.index.find_file(&linked.path) else {
            self.open_link(link);
            return;
        };
        let lines = linked
            .lines
            .map_or(LineSelection::one(Line::new(0)), |span| LineSelection {
                from: span.start(),
                to: span.end(),
            });
        self.model.open_lines(file, lines);
        let at_head = self
            .model
            .review
            .open
            .as_ref()
            .is_some_and(|open| linked.at.is_head(&open.pull.head, &open.opened.head));
        self.model.say(if at_head {
            ReviewStatus::LinkedFileShown(linked.path)
        } else {
            ReviewStatus::LinkedFileShownAtHead {
                path: linked.path,
                linked: linked.at,
            }
        });
    }

    pub(crate) fn review(&mut self, act: ReviewAct) {
        match act {
            ReviewAct::RefreshPulls => self.list_pulls(),
            ReviewAct::OpenPull(repo, number) => self.open_pull(&repo, number),
            ReviewAct::Pin => self.pin(),
            ReviewAct::Unpin(name) => self.unpin(&name),
            ReviewAct::ShowStanding(standing) => self.model.review.standing = standing,
            ReviewAct::ShowRequested(requested) => self.model.review.requested = requested,
            ReviewAct::OpenChange(path) => self.model.open_change(&path),
            ReviewAct::OpenPlace(place) => self.model.open_place(&place),
            ReviewAct::ToggleSeen(path) => self.toggle_seen(&path),
            ReviewAct::ToggleShownSeen => match self.model.shown_path() {
                Some(path) => self.toggle_seen(&path),
                None => self.model.say(ReviewStatus::NotAChange),
            },
            ReviewAct::NextUnseen => self.model.next_unseen(),
            ReviewAct::NextHunk => self.model.step_hunk(Going::Forward),
            ReviewAct::PreviousHunk => self.model.step_hunk(Going::Back),
            ReviewAct::ToggleWhole(path) => {
                let whole = &mut self.model.review.whole;
                if !whole.remove(&path) {
                    whole.insert(path);
                }
            }
            ReviewAct::Unfold(path, span) => {
                self.model
                    .review
                    .unfolded
                    .entry(path)
                    .or_default()
                    .push(span);
            }
            ReviewAct::Reveal(path, line) => self.model.review.reveal(&path, line),
            ReviewAct::PickRemoved(path, line, picking) => {
                self.model.pick_removed(path, line, picking);
            }
            ReviewAct::StartDraft(on) => self.start_draft(on),
            ReviewAct::StartDraftHere if self.model.review.diffs.commit().is_some() => {
                self.model.say(ReviewStatus::CommitComment);
            }
            ReviewAct::StartDraftHere => match self.model.draft_here() {
                Some(on) => self.start_draft(on),
                None => self.model.say(ReviewStatus::SelectALine),
            },
            ReviewAct::SaveDraft => self.save_draft(),
            ReviewAct::CancelDraft => {
                self.model.review.draft = None;
                self.model.fields.release(Which::Draft);
            }
            ReviewAct::RemoveDraft(id) => self.remove_draft(id),
            ReviewAct::RefreshTalk => self.ask_talk(),
            ReviewAct::AskSubmit(decision) => {
                self.model.review.submitting = Submitting::Asking(decision);
            }
            ReviewAct::CancelSubmit => self.model.review.submitting = Submitting::Idle,
            ReviewAct::Submit(decision) => self.submit(decision),
            ReviewAct::OpenLink(link) => self.open_link(link),
            ReviewAct::ShowLinkedFile(file, link) => self.show_linked_file(file, link),
            ReviewAct::ViewCommit(commit) => self.view_commit(commit),
            ReviewAct::NeedPicture(link) => self.need_picture(link),
            ReviewAct::PictureWake(at) => self.model.review.pictures.wake_at(at),
            ReviewAct::NeedSides(key) => self.need_sides(key),
            ReviewAct::ToggleLayout => {
                let diffs = &mut self.model.review.diffs;
                diffs.layout = match diffs.layout {
                    DiffLayout::Unified => DiffLayout::Split,
                    DiffLayout::Split => DiffLayout::Unified,
                };
            }
            ReviewAct::CopyLines => match self.model.copied_lines() {
                Some(copied) => {
                    self.clipboard = ClipboardRequest::Copy(copied.text);
                    self.model.say(ReviewStatus::LinesCopied(copied.lines));
                }
                None => self.model.say(ReviewStatus::NothingToCopy),
            },
            ReviewAct::CopyLink(link) => {
                self.clipboard = ClipboardRequest::Copy(Label::new(link.as_str()));
                self.model.say(ReviewStatus::LinkCopied(link));
            }
        }
    }

    pub(crate) fn list_pulls(&mut self) {
        if self.jobs.list.is_some() {
            self.jobs.relist = Relist::Again;
            return;
        }
        let desk = &self.model.review.desk;
        let repos: Vec<Repo> = desk.repos().cloned().collect();
        let pinned = desk.pinned().to_vec();
        let access = desk.access().clone();
        let sites = self.model.review.logins.sites(desk);
        if matches!(self.model.review.inbox, Inbox::Listed { .. }) && repos.is_empty() {
            self.model.review.inbox = Inbox::Listed {
                entries: Vec::new(),
                broken: Vec::new(),
                requests: Requests::default(),
            };
        }
        self.jobs.list = Some(Job::start(move || list(&repos, &pinned, &access, &sites)));
    }

    fn pin(&mut self) {
        if self.jobs.pin.is_some() {
            self.model.say(ReviewStatus::Busy);
            return;
        }
        let target = PinTarget::new(self.model.fields.get(Which::Pin).text().as_str());
        if target.as_str().is_empty() {
            return;
        }
        let mut desk = self.model.review.desk.clone();
        self.model.review.pinning = Pinning::Busy;
        self.jobs.pin = Some(Job::start(move || {
            desk.pin(&target)
                .map(|name| PinLanded { desk, name })
                .map_err(|error| Label::new(error.to_string()))
        }));
    }

    fn unpin(&mut self, name: &RepoName) {
        match self.model.review.desk.unpin(name) {
            Ok(name) => {
                self.model.say(ReviewStatus::Unpinned(name));
                self.list_pulls();
            }
            Err(error) => self
                .model
                .say(ReviewStatus::PinFailed(Label::new(error.to_string()))),
        }
    }

    fn open_pull(&mut self, name: &RepoName, number: PrNumber) {
        if self.jobs.open.is_some() || self.jobs.submit.is_some() {
            self.model.say(ReviewStatus::Busy);
            return;
        }
        let Inbox::Listed { entries, .. } = &self.model.review.inbox else {
            return;
        };
        let Some(pull) = entries
            .iter()
            .find(|entry| &entry.repo == name && entry.pull.number == number)
            .map(|entry| entry.pull.clone())
        else {
            return;
        };
        let Some(repo) = self
            .model
            .review
            .desk
            .repos()
            .find(|repo| repo.name() == name)
            .cloned()
        else {
            return;
        };
        self.model.review.opening = Some(number);
        self.model.say(ReviewStatus::Opening(number));
        self.jobs.open = Some(Job::start(move || open(repo, pull)));
    }

    fn ask_talk(&mut self) {
        let Some(open) = self.model.review.open.as_mut() else {
            return;
        };
        let repo = open.repo.clone();
        let pull = open.pull.clone();
        open.talk = Talk::Asking;
        self.jobs.talk = Some(Job::start(move || {
            repo.conversation(&pull)
                .map_err(|error| Label::new(error.to_string()))
        }));
        self.ask_rights();
    }

    fn ask_rights(&mut self) {
        let Some(open) = self.model.review.open.as_ref() else {
            return;
        };
        let repo = open.repo.clone();
        let pull = open.pull.clone();
        self.jobs.rights = Some(Job::start(move || {
            repo.rights(&pull)
                .map_err(|error| Label::new(error.to_string()))
        }));
    }

    fn poll_rights(&mut self) {
        let result = match landed(&mut self.jobs.rights) {
            Landing::Waiting => return,
            Landing::Landed(result) => result,
            Landing::Failed => Err(Label::new("the thread asking the forge failed")),
        };
        let Some(open) = self.model.review.open.as_mut() else {
            return;
        };
        open.rights = Some(result);
    }

    fn save_review(&mut self) {
        let review = &mut self.model.review;
        let Some(open) = &review.open else {
            return;
        };
        match open.repo.save(&open.opened) {
            Ok(()) => review.stamp = open.repo.store().stamp(open.opened.number),
            Err(error) => {
                self.model
                    .say(ReviewStatus::NotSaved(Label::new(error.to_string())));
            }
        }
    }

    fn toggle_seen(&mut self, path: &RelativePath) {
        let Some(opened) = self.model.review.opened_mut() else {
            self.model.say(ReviewStatus::NotOpened);
            return;
        };
        let Some(change) = opened.changes.iter().find(|change| change.path() == path) else {
            self.model.say(ReviewStatus::NotAChange);
            return;
        };
        let seen = opened.review.toggle(change, opened.head.clone());
        self.save_review();
        self.model.say(ReviewStatus::Marked {
            path: path.clone(),
            seen,
        });
    }

    fn start_draft(&mut self, on: DraftOn) {
        let inside = self
            .model
            .review
            .change(&on.file)
            .map(|change| on.reply.is_some() || change.commentable(on.side, on.lines()));
        match inside {
            None => self.model.say(ReviewStatus::NotAChange),
            Some(false) => self.model.say(ReviewStatus::NotInDiff),
            Some(true) => {
                self.model.show_editing(&on);
                self.model.review.draft = Some(on);
                self.model.fields.start_empty(Which::Draft);
            }
        }
    }

    fn save_draft(&mut self) {
        let Some(on) = self.model.review.draft.clone() else {
            return;
        };
        let Some(text) = DraftText::new(self.model.fields.get(Which::Draft).text().as_str()) else {
            self.model.say(ReviewStatus::EmptyDraft);
            return;
        };
        let Some(opened) = self.model.review.opened_mut() else {
            return;
        };
        let id = opened.review.add_draft(on, text);
        self.model.review.draft = None;
        self.model.fields.start_empty(Which::Draft);
        self.model.fields.release(Which::Draft);
        self.save_review();
        self.model.say(ReviewStatus::Drafted(id));
    }

    fn remove_draft(&mut self, id: DraftId) {
        let Some(opened) = self.model.review.opened_mut() else {
            return;
        };
        if opened.review.remove_draft(id).is_ok() {
            self.save_review();
            self.model.say(ReviewStatus::DraftRemoved(id));
        }
    }

    fn submit(&mut self, decision: Decision) {
        if self.jobs.busy() {
            self.model.say(ReviewStatus::Busy);
            return;
        }
        let Some(open) = &self.model.review.open else {
            self.model.say(ReviewStatus::NotOpened);
            return;
        };
        let repo = open.repo.clone();
        let opened = open.opened.clone();
        let body = DraftText::new(self.model.fields.get(Which::Summary).text().as_str());
        self.model.review.submitting = Submitting::Sending;
        self.model.say(ReviewStatus::Submitting);
        self.jobs.submit = Some(Job::start(move || {
            submit(&repo, opened, decision, body.as_ref())
        }));
    }

    pub(crate) fn poll_review(&mut self) {
        self.poll_list();
        self.poll_pin();
        self.poll_open();
        self.poll_talk();
        self.poll_rights();
        self.poll_submit();
        self.poll_diffs();
        self.poll_pictures();
        self.poll_review_disk();
    }

    fn poll_list(&mut self) {
        let listed = match landed(&mut self.jobs.list) {
            Landing::Waiting => return,
            Landing::Landed(listed) => listed,
            Landing::Failed => Listed {
                entries: Vec::new(),
                broken: vec![Broken {
                    place: Label::new("inbox"),
                    why: Label::new("the listing thread failed"),
                }],
                requests: Requests::default(),
                held: BTreeMap::new(),
            },
        };
        if self.jobs.relist == Relist::Again {
            self.jobs.relist = Relist::Settled;
            self.list_pulls();
        } else {
            self.model.review.logins.held.extend(listed.held);
            self.ask_rights();
        }
        let pulls = Count::new(listed.entries.len());
        match &self.model.status {
            Status::Login(LoginStatus::In(site)) => {
                self.model.status = Status::Login(LoginStatus::Listed(site.clone(), pulls));
            }
            Status::Login(_) => {}
            _ if self.model.review.open.is_none() && self.model.review.opening.is_none() => {
                self.model.say(ReviewStatus::Listed(pulls));
            }
            _ => {}
        }
        self.model.review.inbox = Inbox::Listed {
            entries: listed.entries,
            broken: listed.broken,
            requests: listed.requests,
        };
    }

    fn poll_pin(&mut self) {
        let result = match landed(&mut self.jobs.pin) {
            Landing::Waiting => return,
            Landing::Landed(result) => result,
            Landing::Failed => Err(Label::new("the pinning thread failed")),
        };
        self.model.review.pinning = Pinning::Idle;
        match result {
            Ok(PinLanded { desk, name }) => {
                self.model.review.desk = desk;
                self.model.fields.start_empty(Which::Pin);
                self.model.fields.release(Which::Pin);
                self.model.say(ReviewStatus::Pinned(name));
                self.list_pulls();
            }
            Err(why) => self.model.say(ReviewStatus::PinFailed(why)),
        }
    }

    fn poll_open(&mut self) {
        let result = match landed(&mut self.jobs.open) {
            Landing::Waiting => return,
            Landing::Landed(result) => result,
            Landing::Failed => Err(Label::new("the thread opening the pull request failed")),
        };
        match result {
            Ok(landed) => self.opened(landed),
            Err(why) => {
                self.model.review.opening = None;
                self.model.say(ReviewStatus::OpenFailed(why));
            }
        }
    }

    fn poll_talk(&mut self) {
        let result = match landed(&mut self.jobs.talk) {
            Landing::Waiting => return,
            Landing::Landed(result) => result,
            Landing::Failed => Err(Label::new("the thread reading it failed")),
        };
        let Some(open) = self.model.review.open.as_mut() else {
            return;
        };
        match result {
            Ok(conversation) => {
                open.talk = Talk::Got(conversation);
                self.model.review.pictures.forget_failed();
            }
            Err(why) => {
                open.talk = Talk::Failed(why.clone());
                self.model.say(ReviewStatus::TalkFailed(why));
            }
        }
    }

    fn poll_submit(&mut self) {
        let result = match landed(&mut self.jobs.submit) {
            Landing::Waiting => return,
            Landing::Landed(result) => result,
            Landing::Failed => Err(Label::new("the submitting thread failed")),
        };
        self.model.review.submitting = Submitting::Idle;
        match result {
            Ok(submitted) => {
                if let Some(open) = self.model.review.open.as_mut() {
                    open.opened = submitted.opened;
                }
                self.model.fields.start_empty(Which::Summary);
                self.model.fields.release(Which::Summary);
                self.model.say(ReviewStatus::Submitted {
                    decision: submitted.decision,
                    comments: submitted.comments,
                });
                self.ask_talk();
                self.list_pulls();
            }
            Err(why) => self.model.say(ReviewStatus::SubmitFailed(why)),
        }
    }

    fn opened(&mut self, landed: Landed) {
        let Landed {
            repo,
            pull,
            opened,
            indexed,
        } = landed;
        let number = opened.number;
        let files = Count::new(opened.changes.len());
        let tree = Root::new(&opened.tree);
        self.services = Services::new(&tree);
        self.model.work = WorkState::default();
        self.model.peek = None;
        self.model.hits.clear();
        self.model.forget_place();
        self.with_index_change(|app| app.model.index = indexed.index);
        self.services.watch(indexed.stamps);
        let review = &mut self.model.review;
        review.stamp = repo.store().stamp(number);
        review.opening = None;
        review.draft = None;
        review.whole.clear();
        review.unfolded.clear();
        review.diffs.forget();
        review.open = Some(OpenReview {
            web: repo.web(),
            repo,
            pull,
            opened,
            talk: Talk::Asking,
            rights: None,
        });
        self.model.say(ReviewStatus::Opened { number, files });
        self.model.panels.activate(View::Changes);
        self.model.set_tab(Tab::Review);
        self.ask_talk();
        self.start_backend();
        self.list_pulls();
    }

    pub(crate) fn dropped(&mut self, name: &RepoName, number: PrNumber) {
        let shown = self
            .model
            .review
            .open
            .as_ref()
            .is_some_and(|open| open.repo.name() == name && open.opened.number == number);
        if shown {
            self.model.peek = None;
            self.model.hits.clear();
            self.model.forget_place();
            self.with_index_change(|app| app.model.index = Index::default());
            self.services = Services::new(self.model.index.root());
            self.model.work = WorkState::default();
            let review = &mut self.model.review;
            review.stamp = None;
            review.draft = None;
            review.whole.clear();
            review.unfolded.clear();
            review.diffs.forget();
            review.open = None;
            self.model.set_tab(Tab::Inbox);
            self.index_working_copy();
        }
        self.list_pulls();
    }

    fn poll_review_disk(&mut self) {
        let review = &mut self.model.review;
        if review.last_poll.elapsed() < Duration::from_secs(1) {
            return;
        }
        review.last_poll = Instant::now();
        let Some(open) = review.open.as_mut() else {
            return;
        };
        let stamp = open.repo.store().stamp(open.opened.number);
        if stamp == review.stamp {
            return;
        }
        review.stamp = stamp;
        if let Ok(loaded) = open.repo.review(open.opened.number)
            && loaded != open.opened.review
        {
            open.opened.review = loaded;
            self.model.say(ReviewStatus::Reloaded);
        }
    }
}

#[cfg(test)]
mod tests {
    use domain::{ChangeKind, DiffLine, SourceLine};

    use super::*;

    fn line(kind: LineKind) -> DiffLine {
        DiffLine {
            kind,
            text: SourceLine::new("x"),
        }
    }

    #[test]
    fn one_hunk_with_two_separate_changes_gives_two_places_to_step_to() {
        let hunk = Hunk::new(
            Line::new(10),
            Line::new(10),
            vec![
                line(LineKind::Context),
                line(LineKind::Removed),
                line(LineKind::Added),
                line(LineKind::Added),
                line(LineKind::Context),
                line(LineKind::Context),
                line(LineKind::Removed),
                line(LineKind::Context),
            ],
        );
        let change = FileChange::new(
            RelativePath::new("a.rs"),
            None,
            ChangeKind::Modified,
            vec![hunk],
        );
        assert_eq!(change_starts(&change), vec![Line::new(11), Line::new(15)]);
    }

    fn span(start: u32, end: u32) -> Span {
        Span::new(Line::new(start), Line::new(end)).unwrap()
    }

    #[test]
    fn touching_and_overlapping_spans_merge_and_gaps_stay() {
        assert_eq!(
            merged(vec![
                span(10, 12),
                span(0, 3),
                span(4, 5),
                span(20, 21),
                span(11, 15)
            ]),
            vec![span(0, 5), span(10, 15), span(20, 21)]
        );
    }
}

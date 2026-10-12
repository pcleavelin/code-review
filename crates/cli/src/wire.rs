use std::fmt;
use std::iter;
use std::mem;
use std::path::Path;

use clap::{Args, CommandFactory, Parser};
use domain::{
    Block, ChangeKind, Conversation, Decision, Depth, Draft, FileChange, ForgeHost, Index, Line,
    LineKind, Location, Mark, PrNumber, Program, Progress, Review, Seen, Side, SourceFile,
    SourceLine, Span, Standing, Symbol, SymbolId, Thread, ThreadState, WebLink,
};
use session::{
    DeviceAuthorization, Entry, Forgotten, Opened, Pinned, PullAddress, RepoName, RequestedFrom,
};

use crate::failure::Failure;
use crate::output::{Output, write_line};

const PROGRAM: &str = "review";

#[derive(Parser, Debug)]
#[command(
    name = PROGRAM,
    disable_help_flag = true,
    override_usage = "review                             open the inbox window\n       review <command> [args]            text mode (the same commands work in the window's Console)\n\n<pr> is [repository#]number, e.g. hexyl#296 or 296 inside a clone or with one repository pinned"
)]
pub(crate) enum Command {
    #[command(about = "                                 the pinned repositories")]
    Pins,
    #[command(
        about = "<path or URL>                    pin a clone, or clone a repository and pin it"
    )]
    Pin(PinArguments),
    #[command(about = "<repository>                     unpin a repository; its reviews stay")]
    Unpin(RepoArguments),
    #[command(
        about = "<host or repository>             log in to the forge: a code to enter in the browser (GitHub, GitLab) or a pasted token (Forgejo), kept in the system keychain"
    )]
    Login(LoginArguments),
    #[command(
        about = "<host or repository>             forget the forge's token kept in the system keychain"
    )]
    Logout(LoginArguments),
    #[command(
        about = "[filter] [--requested]            every open pull request of the pinned repositories, newest first; --requested only those whose review is requested from you, teams included (needs a login)"
    )]
    Prs(FilterArguments),
    #[command(
        about = "<pr>                             fetch the pull request, check out its head and list its changed files"
    )]
    Open(PrArguments),
    #[command(about = "<pr>                             the description and every comment thread")]
    Discussion(PrArguments),
    #[command(
        about = "<pr>                             changed files, each seen, unseen, or changed since it was seen"
    )]
    Changes(PrArguments),
    #[command(
        about = "<pr> [file] [--since-seen]       the diff of one changed file, or of all of them, with line numbers; --since-seen shows a file changed since it was seen from the head it was seen at"
    )]
    Diff(DiffArguments),
    #[command(about = "<pr>                             the diff of the first file not seen yet")]
    Next(PrArguments),
    #[command(
        about = "<pr> <file>... | --all           mark changed files seen; a file turns unseen again when its diff changes"
    )]
    Seen(FilesArguments),
    #[command(about = "<pr> <file>... | --all           mark changed files unseen")]
    Unseen(FilesArguments),
    #[command(
        about = "<pr> <file> <line> <text> [--old] [--reply <thread>]  draft a comment on a line of the diff (1-based)"
    )]
    Comment(CommentArguments),
    #[command(about = "<pr>                             the draft comments not submitted yet")]
    Drafts(PrArguments),
    #[command(about = "<pr> <id>                        remove a draft comment")]
    Uncomment(UncommentArguments),
    #[command(
        about = "<pr> [summary] [--approve|--request-changes]  send the drafts to the forge as one review"
    )]
    Submit(SubmitArguments),
    #[command(
        about = "<pr>                             remove the pull request's tree and its base and head refs; its review stays"
    )]
    Drop(PrArguments),
    #[command(
        about = "<pr> [filter]                    the files of the pull request's tree whose path contains filter"
    )]
    Files(PrFilterArguments),
    #[command(
        about = "<pr> [filter]                    symbols of the tree: file:start-end kind name (calls, callers)"
    )]
    Symbols(PrFilterArguments),
    #[command(
        about = "<pr> <file> [start] [end]        numbered lines of a file of the tree (1-based, inclusive)"
    )]
    Show(ShowArguments),
    #[command(
        about = "<pr> <regex>                     every line of the tree that matches: file:line: text"
    )]
    Search(SearchArguments),
    #[command(
        about = "<pr> <symbol>                    who calls it (xrefs to it, from the language's server)"
    )]
    Callers(SymbolArguments),
    #[command(about = "<pr> <symbol>                    what it calls (xrefs from it)")]
    Callees(SymbolArguments),
    #[command(
        about = "<pr> <symbol>                    every reference to it, file:line: text (asks the language's server)"
    )]
    Refs(SymbolArguments),
    #[command(
        about = "<pr> <symbol> [depth]            what it calls, and what that calls, to depth (default 4)"
    )]
    Calltree(CalltreeArguments),
    #[command(
        about = "<pr> [limit]                     entry points: symbols nothing in the tree calls (at most limit, default 30)"
    )]
    Roots(RootsArguments),
}

#[derive(Args, Debug)]
pub(crate) struct PrFilterArguments {
    #[arg(value_parser = parse_pr)]
    pub(crate) pr: PullAddress,
    pub(crate) filter: Vec<String>,
}

#[derive(Args, Debug)]
pub(crate) struct ShowArguments {
    #[arg(value_parser = parse_pr)]
    pub(crate) pr: PullAddress,
    pub(crate) file: String,
    #[arg(value_parser = clap::value_parser!(u32).range(1..))]
    pub(crate) start: Option<u32>,
    #[arg(value_parser = clap::value_parser!(u32).range(1..))]
    pub(crate) end: Option<u32>,
}

#[derive(Args, Debug)]
pub(crate) struct SearchArguments {
    #[arg(value_parser = parse_pr)]
    pub(crate) pr: PullAddress,
    pub(crate) regex: String,
}

#[derive(Args, Debug)]
pub(crate) struct SymbolArguments {
    #[arg(value_parser = parse_pr)]
    pub(crate) pr: PullAddress,
    pub(crate) symbol: String,
}

#[derive(Args, Debug)]
pub(crate) struct CalltreeArguments {
    #[arg(value_parser = parse_pr)]
    pub(crate) pr: PullAddress,
    pub(crate) symbol: String,
    #[arg(default_value_t = 4, value_parser = clap::value_parser!(u32).range(1..))]
    pub(crate) depth: u32,
}

#[derive(Args, Debug)]
pub(crate) struct RootsArguments {
    #[arg(value_parser = parse_pr)]
    pub(crate) pr: PullAddress,
    #[arg(default_value_t = 30)]
    pub(crate) limit: usize,
}

fn parse_pr(text: &str) -> Result<PullAddress, String> {
    text.parse::<PullAddress>()
        .map_err(|()| format!("'{text}' is not a pull request: write [repository#]number"))
}

#[derive(Args, Debug)]
pub(crate) struct PinArguments {
    pub(crate) target: String,
}

#[derive(Args, Debug)]
pub(crate) struct LoginArguments {
    pub(crate) target: String,
}

#[derive(Args, Debug)]
pub(crate) struct RepoArguments {
    pub(crate) repository: String,
}

#[derive(Args, Debug)]
pub(crate) struct FilterArguments {
    pub(crate) filter: Vec<String>,
    #[arg(long)]
    pub(crate) requested: bool,
}

#[derive(Args, Debug)]
pub(crate) struct PrArguments {
    #[arg(value_parser = parse_pr)]
    pub(crate) pr: PullAddress,
}

#[derive(Args, Debug)]
pub(crate) struct DiffArguments {
    #[arg(value_parser = parse_pr)]
    pub(crate) pr: PullAddress,
    pub(crate) file: Option<String>,
    #[arg(long)]
    pub(crate) since_seen: bool,
}

#[derive(Args, Debug)]
pub(crate) struct FilesArguments {
    #[arg(value_parser = parse_pr)]
    pub(crate) pr: PullAddress,
    #[arg(required_unless_present = "all")]
    pub(crate) files: Vec<String>,
    #[arg(long, conflicts_with = "files")]
    pub(crate) all: bool,
}

#[derive(Args, Debug)]
pub(crate) struct CommentArguments {
    #[arg(value_parser = parse_pr)]
    pub(crate) pr: PullAddress,
    pub(crate) file: String,
    #[arg(value_parser = clap::value_parser!(u32).range(1..))]
    pub(crate) line: u32,
    pub(crate) text: String,
    #[arg(long)]
    pub(crate) old: bool,
    #[arg(long, value_name = "THREAD")]
    pub(crate) reply: Option<String>,
    #[arg(long, value_name = "LINE", value_parser = clap::value_parser!(u32).range(1..))]
    pub(crate) from: Option<u32>,
}

#[derive(Args, Debug)]
pub(crate) struct UncommentArguments {
    #[arg(value_parser = parse_pr)]
    pub(crate) pr: PullAddress,
    pub(crate) id: u32,
}

#[derive(Args, Debug)]
pub(crate) struct SubmitArguments {
    #[arg(value_parser = parse_pr)]
    pub(crate) pr: PullAddress,
    pub(crate) summary: Option<String>,
    #[arg(long, conflicts_with = "request_changes")]
    pub(crate) approve: bool,
    #[arg(long)]
    pub(crate) request_changes: bool,
}

pub(crate) fn parse<'a>(arguments: impl Iterator<Item = &'a str>) -> Result<Command, clap::Error> {
    Command::try_parse_from(iter::once(PROGRAM).chain(arguments))
}

pub(crate) fn help() -> Output {
    Output::of(Command::command().render_help().to_string())
}

pub(crate) fn render_error(error: &clap::Error) -> Output {
    Output::of(error.render().to_string())
}

pub(crate) fn words(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for character in line.chars() {
        match character {
            '"' => quoted = !quoted,
            space if space.is_whitespace() && !quoted => {
                if !current.is_empty() {
                    words.push(mem::take(&mut current));
                }
            }
            other => current.push(other),
        }
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

const fn seen_mark(seen: Seen) -> &'static str {
    match seen {
        Seen::Seen => "[x]",
        Seen::Unseen => "[ ]",
        Seen::Changed => "[~]",
    }
}

const fn kind_word(kind: ChangeKind) -> &'static str {
    match kind {
        ChangeKind::Added => "added",
        ChangeKind::Deleted => "deleted",
        ChangeKind::Modified => "modified",
        ChangeKind::Renamed => "renamed",
    }
}

const fn side_word(side: Side) -> &'static str {
    match side {
        Side::Old => "old",
        Side::New => "new",
    }
}

pub(crate) fn entry_text(entry: &Entry) -> String {
    let pull = &entry.pull;
    format!(
        "{} #{} {} {} {} {} {}",
        entry.repo,
        pull.number,
        pull.title,
        pull.author,
        pull.head,
        pull.base,
        standing_word(entry.standing)
    )
}

pub(crate) const fn standing_word(standing: Standing) -> &'static str {
    match standing {
        Standing::New => "new",
        Standing::Updated => "updated",
        Standing::Reviewing => "reviewing",
        Standing::Reviewed => "reviewed",
    }
}

fn local(entry: &Entry) -> String {
    let progress = entry
        .progress
        .map_or_else(String::new, |progress: Progress| {
            format!(", {progress} seen")
        });
    let drafts = match entry.drafts.len() {
        0 => String::new(),
        1 => ", 1 draft".to_owned(),
        many => format!(", {many} drafts"),
    };
    format!("{}{progress}{drafts}", standing_word(entry.standing))
}

pub(crate) fn inbox(
    output: &mut Output,
    entries: &[Entry],
    broken: &[(String, String)],
    from: RequestedFrom,
) {
    for (place, why) in broken {
        write_line!(output, "! {place}: {why}");
    }
    if entries.is_empty() {
        match from {
            RequestedFrom::Any => write_line!(output, "no open pull requests"),
            RequestedFrom::You => {
                write_line!(
                    output,
                    "no open pull request has a review requested from you"
                );
            }
        }
    }
    for entry in entries {
        let pull = &entry.pull;
        write_line!(
            output,
            "{}#{:<5} {}  ({}, {})  {} <- {}  [{}]",
            entry.repo,
            pull.number,
            pull.title,
            pull.author,
            pull.updated.day(),
            pull.base,
            pull.head,
            local(entry)
        );
    }
}

pub(crate) fn pins(output: &mut Output, here: Option<&RepoName>, pinned: &[Pinned]) {
    if let Some(here) = here {
        write_line!(output, "{here}  (the current directory, not pinned)");
    }
    if pinned.is_empty() {
        write_line!(
            output,
            "nothing pinned: 'review pin <path or URL>' pins a repository"
        );
    }
    for pin in pinned {
        match pin {
            Pinned::Ready(repo) => {
                write_line!(
                    output,
                    "{}  {}",
                    repo.name(),
                    repo.root().as_path().display()
                );
            }
            Pinned::Broken { root, why } => {
                write_line!(output, "! {}  {}", root.as_path().display(), why.as_str());
            }
        }
    }
}

pub(crate) fn pinned(output: &mut Output, name: &RepoName) {
    write_line!(output, "pinned {name}");
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Browser {
    Opened,
    NotOpened,
}

pub(crate) fn login_code(
    output: &mut Output,
    authorization: &DeviceAuthorization,
    browser: Browser,
) {
    write_line!(
        output,
        "to log in to {}, enter the code {} at {}",
        authorization.forge_host(),
        authorization.user_code(),
        authorization.verification()
    );
    let opening = match browser {
        Browser::Opened => "the browser is opening it",
        Browser::NotOpened => "open it in a browser",
    };
    write_line!(output, "{opening}; waiting for the approval...");
}

pub(crate) fn login_paste(output: &mut Output, site: &ForgeHost, page: Option<&WebLink>) {
    let made = page.map_or_else(String::new, |page| format!(" at {page}"));
    write_line!(
        output,
        "{} on {site} has no device login: make an access token{made} that may read and write repositories and issues, paste it here and press enter",
        site.kind()
    );
}

pub(crate) fn logged_in(output: &mut Output, site: &ForgeHost) {
    write_line!(
        output,
        "logged in to {site}; the token is in the system keychain"
    );
}

pub(crate) fn logged_out(output: &mut Output, site: &ForgeHost, forgotten: Forgotten) {
    match forgotten {
        Forgotten::Removed => write_line!(
            output,
            "logged out of {site}; its token is gone from the keychain"
        ),
        Forgotten::Absent => write_line!(output, "the keychain holds no token for {site}"),
    }
}

pub(crate) fn unpinned(output: &mut Output, name: &RepoName) {
    write_line!(
        output,
        "unpinned {name}; its reviews stay in its git directory"
    );
}

const fn decision_word(decision: Decision) -> &'static str {
    match decision {
        Decision::Comment => "commented",
        Decision::Approve => "approved",
        Decision::RequestChanges => "requested changes",
    }
}

const fn thread_word(state: ThreadState) -> &'static str {
    match state {
        ThreadState::Open => "",
        ThreadState::Resolved => " (resolved)",
        ThreadState::Outdated => " (outdated)",
    }
}

fn thread(output: &mut Output, thread: &Thread, indent: &str) {
    let place = thread.place.as_ref().map_or_else(String::new, |place| {
        format!(
            " on {}:{}{}",
            place.file,
            place.line.number(),
            match place.side {
                Side::Old => " (old)",
                Side::New => "",
            }
        )
    });
    write_line!(
        output,
        "{indent}thread {}{place}{}",
        thread.id,
        thread_word(thread.state)
    );
    for note in &thread.notes {
        write_line!(output, "{indent}  {} {}:", note.when, note.author);
        for line in note.body.as_str().lines() {
            write_line!(output, "{indent}    {line}");
        }
    }
}

pub(crate) fn discussion(output: &mut Output, conversation: &Conversation) {
    if conversation.description.is_empty() {
        write_line!(output, "(no description)");
    }
    for line in conversation.description.as_str().lines() {
        write_line!(output, "{line}");
    }
    for block in conversation.blocks() {
        write_line!(output, "");
        match block {
            Block::Alone(alone) => thread(output, alone, ""),
            Block::Submitted {
                submission,
                threads,
            } => {
                write_line!(
                    output,
                    "{} {} {} ({})",
                    submission.author,
                    decision_word(submission.decision),
                    submission.when,
                    submission.id
                );
                for line in submission.summary.as_str().lines() {
                    write_line!(output, "  {line}");
                }
                for each in threads {
                    thread(output, each, "  ");
                }
            }
        }
    }
}

pub(crate) fn opened(output: &mut Output, opened: &Opened) {
    write_line!(
        output,
        "#{} {}..{}  checked out at {}",
        opened.number,
        opened.base,
        opened.head,
        opened.tree.display()
    );
    changes(output, opened);
}

pub(crate) fn changes(output: &mut Output, opened: &Opened) {
    for change in &opened.changes {
        let from = change
            .old_path()
            .map_or_else(String::new, |old| format!(" (from {old})"));
        write_line!(
            output,
            "{} {:>5} {:>5}  {}{from}",
            seen_mark(opened.seen(change)),
            format!("+{}", change.added()),
            format!("-{}", change.removed()),
            change.path()
        );
    }
    write_line!(
        output,
        "{} seen; [x] seen, [~] changed since seen ('diff <pr> <file> --since-seen' shows what changed), [ ] unseen",
        opened.review.progress(&opened.changes)
    );
}

fn number(line: Option<domain::Line>) -> String {
    line.map_or_else(String::new, |line| line.number().to_string())
}

const fn marker(kind: LineKind) -> char {
    match kind {
        LineKind::Context => ' ',
        LineKind::Added => '+',
        LineKind::Removed => '-',
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OldSide {
    Base,
    SeenHead,
}

fn hunk_lines(output: &mut Output, change: &FileChange, review: &Review, old: OldSide) {
    for hunk in change.hunks() {
        write_line!(output, "  @@");
        for line in hunk.numbered() {
            write_line!(
                output,
                "{:>5} {:>5} {}{}",
                number(line.old),
                number(line.new),
                marker(line.kind),
                line.text
            );
            let drafted = review
                .drafts_at(change.path())
                .filter(|draft| match draft.side {
                    Side::New => line.new == Some(draft.line),
                    Side::Old => {
                        old == OldSide::Base
                            && line.kind == LineKind::Removed
                            && line.old == Some(draft.line)
                    }
                });
            for draft in drafted {
                write_line!(output, "            >> draft {}: {}", draft.id, draft.text);
            }
        }
    }
}

pub(crate) fn diff(output: &mut Output, change: &FileChange, seen: Seen, review: &Review) {
    write_line!(
        output,
        "{} {} ({}, +{} -{})",
        seen_mark(seen),
        change.path(),
        kind_word(change.kind()),
        change.added(),
        change.removed()
    );
    if change.hunks().is_empty() {
        write_line!(output, "      no text changes");
    }
    hunk_lines(output, change, review, OldSide::Base);
}

pub(crate) fn since_seen(
    output: &mut Output,
    change: &FileChange,
    mark: &Mark,
    since: Option<&FileChange>,
    review: &Review,
) {
    let Some(since) = since else {
        write_line!(
            output,
            "{} {} is as it was when seen at {}; its diff changed because the base moved",
            seen_mark(Seen::Changed),
            change.path(),
            mark.commit.short()
        );
        return;
    };
    write_line!(
        output,
        "{} {} since it was seen at {} (+{} -{}); the old side is the head it was seen at",
        seen_mark(Seen::Changed),
        change.path(),
        mark.commit.short(),
        since.added(),
        since.removed()
    );
    hunk_lines(output, since, review, OldSide::SeenHead);
}

pub(crate) fn all_seen(output: &mut Output, opened: &Opened) {
    write_line!(
        output,
        "every changed file is seen ({})",
        opened.review.progress(&opened.changes)
    );
}

pub(crate) fn marked(output: &mut Output, change: &FileChange, seen: Seen) {
    write_line!(output, "{} {}", seen_mark(seen), change.path());
}

pub(crate) fn draft(output: &mut Output, draft: &Draft) {
    let reply = draft
        .reply
        .as_ref()
        .map_or_else(String::new, |thread| format!(" answering {thread}"));
    write_line!(
        output,
        "draft {}  {}:{} ({}){reply}  {}",
        draft.id,
        draft.file,
        lines_word(draft.lines()),
        side_word(draft.side),
        draft.text
    );
}

fn lines_word(lines: Span) -> String {
    if lines.start() == lines.end() {
        lines.end().number().to_string()
    } else {
        format!("{}-{}", lines.start().number(), lines.end().number())
    }
}

pub(crate) fn drafts(output: &mut Output, review: &Review) {
    if review.drafts().is_empty() {
        write_line!(output, "no draft comments");
    }
    for each in review.drafts() {
        draft(output, each);
    }
}

pub(crate) fn removed(output: &mut Output, removed: &Draft) {
    write_line!(output, "removed draft {}", removed.id);
}

pub(crate) fn dropped(output: &mut Output, number: PrNumber, tree: &Path) {
    write_line!(
        output,
        "dropped #{number}: removed {} and its refs; its review stays, and 'open' fetches it again",
        tree.display()
    );
}

pub(crate) fn submitted(
    output: &mut Output,
    number: PrNumber,
    decision: Decision,
    comments: usize,
) {
    let what = match decision {
        Decision::Comment => "commented on",
        Decision::Approve => "approved",
        Decision::RequestChanges => "requested changes on",
    };
    write_line!(
        output,
        "{what} #{number} with {comments} inline comment{}",
        if comments == 1 { "" } else { "s" }
    );
}

impl fmt::Display for Failure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Session(error) => write!(formatter, "{error}"),
            Self::NotChanged { number, file } => write!(
                formatter,
                "{file} is not changed by #{number}; 'changes <pr>' lists the files it changes"
            ),
            Self::NotInDiff { file, side, line } => write!(
                formatter,
                "line {} of {file} ({} side) is not in the diff; a comment goes on a line 'diff' shows with a number on that side",
                line.number(),
                side_word(*side)
            ),
            Self::EmptyComment => formatter.write_str("the comment is empty"),
            Self::NoThread(thread) => write!(
                formatter,
                "'{thread}' is not a thread id; 'discussion <pr>' lists them"
            ),
            Self::Draft(error) => write!(formatter, "{error}"),
            Self::NoSuchFile(file) => write!(
                formatter,
                "{file} is not a file of the tree; 'files <pr> [filter]' lists them"
            ),
            Self::NoSuchSymbol(symbol) => write!(
                formatter,
                "no symbol {symbol} in the tree; 'symbols <pr> [filter]' lists them"
            ),
            Self::BadPattern(error) => write!(formatter, "{error}"),
            Self::NoToken(site) => write!(
                formatter,
                "nothing that looks like a token was pasted for {site}: paste the token alone on one line"
            ),
        }
    }
}

fn span_text(span: Span) -> String {
    format!("{}-{}", span.start().number(), span.end().number())
}

fn symbol_label(index: &Index, id: SymbolId) -> String {
    let (Some(file), Some(symbol)) = (index.file(id.file()), index.symbol(id)) else {
        return String::new();
    };
    format!(
        "{} {}:{}",
        symbol.name(),
        file.path(),
        span_text(symbol.span())
    )
}

pub(crate) fn file_row(output: &mut Output, file: &SourceFile) {
    write_line!(
        output,
        "{} ({} lines, {} symbols)",
        file.path(),
        file.text().count(),
        file.symbols().count()
    );
}

pub(crate) fn symbol_row(output: &mut Output, file: &SourceFile, symbol: &Symbol) {
    write_line!(
        output,
        "{}:{} {} {}{} ({} calls, {} callers)",
        file.path(),
        span_text(symbol.span()),
        symbol.kind(),
        if symbol.depth().value() > 0 { "  " } else { "" },
        symbol.name(),
        symbol.callees().len(),
        symbol.callers().len()
    );
}

pub(crate) fn numbered_lines(output: &mut Output, file: &SourceFile, span: Span) {
    for line in span.lines() {
        if let Some(text) = file.text().line(line) {
            write_line!(output, "{:5} {}", line.number(), text);
        }
    }
}

pub(crate) fn search_hit(output: &mut Output, file: &SourceFile, line: Line, text: &SourceLine) {
    write_line!(output, "{}:{}: {}", file.path(), line.number(), text);
}

pub(crate) fn symbol_line(output: &mut Output, index: &Index, id: SymbolId, depth: Depth) {
    write_line!(
        output,
        "{}{}",
        "  ".repeat(usize::try_from(depth.value()).unwrap_or(0)),
        symbol_label(index, id)
    );
}

pub(crate) fn root_row(output: &mut Output, index: &Index, id: SymbolId) {
    write_line!(
        output,
        "{} ({} calls)",
        symbol_label(index, id),
        index.symbol(id).map_or(0, |symbol| symbol.callees().len())
    );
}

pub(crate) fn reference_row(output: &mut Output, location: &Location, text: Option<&SourceLine>) {
    write_line!(
        output,
        "  {}:{}: {}",
        location.file,
        location.line.number(),
        text.map_or("", SourceLine::trimmed)
    );
}

pub(crate) fn server_missing(output: &mut Output, program: Option<Program>) {
    write_line!(
        output,
        "  (references need {})",
        program.map_or_else(
            || "a language server, and this language has none".to_owned(),
            |program| program.to_string()
        )
    );
}

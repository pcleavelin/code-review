use std::collections::HashMap;

use domain::{Decision, ForgeKind, Linked, NoteKind, Side, Span, ThreadState};
use serde_json::{Map, Value, json};
use strum::VariantArray;

pub(crate) const CURL: &str = "curl";
pub(crate) const GH: &str = "gh";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EnvVar {
    ReviewForge,
    GitHubToken,
    GhToken,
    GitLabToken,
    ForgejoToken,
    GiteaToken,
}

impl EnvVar {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::ReviewForge => "REVIEW_FORGE",
            Self::GitHubToken => "GITHUB_TOKEN",
            Self::GhToken => "GH_TOKEN",
            Self::GitLabToken => "GITLAB_TOKEN",
            Self::ForgejoToken => "FORGEJO_TOKEN",
            Self::GiteaToken => "GITEA_TOKEN",
        }
    }

    pub(crate) const fn tokens(kind: ForgeKind) -> &'static [Self] {
        match kind {
            ForgeKind::GitHub => &[Self::GitHubToken, Self::GhToken],
            ForgeKind::GitLab => &[Self::GitLabToken],
            ForgeKind::Forgejo => &[Self::ForgejoToken, Self::GiteaToken],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, VariantArray)]
enum Scheme {
    Https,
    Http,
    Ssh,
}

impl Scheme {
    const fn name(self) -> &'static str {
        match self {
            Self::Https => "https://",
            Self::Http => "http://",
            Self::Ssh => "ssh://",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HostMark {
    GitHub,
    Codeberg,
    GitLab,
    Forgejo,
    Gitea,
}

impl HostMark {
    const fn name(self) -> &'static str {
        match self {
            Self::GitHub => "github.com",
            Self::Codeberg => "codeberg.org",
            Self::GitLab => "gitlab",
            Self::Forgejo => "forgejo",
            Self::Gitea => "gitea",
        }
    }
}

const GIT_SUFFIX: &str = ".git";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WireRemote {
    pub(crate) host: String,
    pub(crate) path: String,
}

pub(crate) fn parse_remote(url: &str) -> Option<WireRemote> {
    let url = url.trim();
    let after_scheme = Scheme::VARIANTS
        .iter()
        .find_map(|scheme| url.strip_prefix(scheme.name()));
    let (authority, path) = match after_scheme {
        Some(rest) => rest.split_once('/')?,
        None => url
            .split_once(':')
            .filter(|(authority, _)| authority.chars().count() > 1)?,
    };
    let host_port = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    let host = host_port
        .split_once(':')
        .map_or(host_port, |(host, _)| host);
    let path = path.trim_end_matches('/');
    let path = path.strip_suffix(GIT_SUFFIX).unwrap_or(path);
    Some(WireRemote {
        host: host.to_owned(),
        path: path.to_owned(),
    })
}

pub(crate) fn detect(host: &str) -> Option<ForgeKind> {
    if host == HostMark::GitHub.name() {
        Some(ForgeKind::GitHub)
    } else if host.contains(HostMark::GitLab.name()) {
        Some(ForgeKind::GitLab)
    } else if host == HostMark::Codeberg.name()
        || host.contains(HostMark::Forgejo.name())
        || host.contains(HostMark::Gitea.name())
    {
        Some(ForgeKind::Forgejo)
    } else {
        None
    }
}

pub(crate) fn forge_named(text: &str) -> Option<ForgeKind> {
    let text = text.trim().to_ascii_lowercase();
    ForgeKind::VARIANTS
        .iter()
        .copied()
        .find(|kind| kind.name().as_str() == text)
}

pub(crate) fn head_ref(kind: ForgeKind, number: u64) -> String {
    match kind {
        ForgeKind::GitHub | ForgeKind::Forgejo => format!("refs/pull/{number}/head"),
        ForgeKind::GitLab => format!("refs/merge-requests/{number}/head"),
    }
}

pub(crate) fn gh_token_arguments(host: &str) -> Vec<String> {
    ["auth", "token", "--hostname", host]
        .iter()
        .map(|argument| (*argument).to_owned())
        .collect()
}

pub(crate) fn curl_arguments() -> Vec<String> {
    ["--silent", "--show-error", "--location", "--config", "-"]
        .iter()
        .map(|argument| (*argument).to_owned())
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Method {
    Get,
    Post,
}

impl Method {
    const fn name(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Request {
    pub(crate) method: Method,
    pub(crate) url: String,
    pub(crate) body: Option<Value>,
}

impl Request {
    fn get(url: String) -> Self {
        Self {
            method: Method::Get,
            url,
            body: None,
        }
    }

    fn post(url: String, body: Option<Value>) -> Self {
        Self {
            method: Method::Post,
            url,
            body,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Setting {
    Url,
    Header,
    Request,
    Data,
    WriteOut,
}

impl Setting {
    const fn name(self) -> &'static str {
        match self {
            Self::Url => "url",
            Self::Header => "header",
            Self::Request => "request",
            Self::Data => "data-binary",
            Self::WriteOut => "write-out",
        }
    }
}

const ACCEPT_JSON: &str = "Accept: application/json";
const SEND_JSON: &str = "Content-Type: application/json";
const STATUS_LAST: &str = "\n%{http_code}";

fn escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            other => escaped.push(other),
        }
    }
    escaped
}

fn setting(name: Setting, value: &str) -> String {
    format!("{} = \"{}\"\n", name.name(), escape(value))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WireResponse {
    pub(crate) status: u16,
    pub(crate) body: String,
}

pub(crate) fn parse_response(stdout: &str) -> Option<WireResponse> {
    let (body, status) = stdout.rsplit_once('\n')?;
    Some(WireResponse {
        status: status.trim().parse().ok()?,
        body: body.to_owned(),
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WirePull {
    pub(crate) number: u64,
    pub(crate) title: String,
    pub(crate) author: String,
    pub(crate) base: String,
    pub(crate) head: String,
    pub(crate) sha: String,
    pub(crate) description: String,
    pub(crate) updated: String,
}

struct PullFields {
    number: &'static str,
    title: &'static str,
    author: &'static str,
    base: &'static str,
    head: &'static str,
    sha: &'static str,
    description: &'static str,
}

const PULL_FIELDS: PullFields = PullFields {
    number: "/number",
    title: "/title",
    author: USER,
    base: "/base/ref",
    head: "/head/ref",
    sha: "/head/sha",
    description: BODY,
};

const MERGE_REQUEST_FIELDS: PullFields = PullFields {
    number: "/iid",
    title: "/title",
    author: AUTHOR,
    base: "/target_branch",
    head: "/source_branch",
    sha: "/sha",
    description: "/description",
};

const USER: &str = "/user/login";
const AUTHOR: &str = "/author/username";
const BODY: &str = "/body";
const CREATED: &str = "/created_at";
const SUBMITTED: &str = "/submitted_at";
const UPDATED: &str = "/updated_at";
const ID: &str = "/id";

fn text_at(value: &Value, pointer: &str) -> Option<String> {
    value.pointer(pointer)?.as_str().map(str::to_owned)
}

fn line_at(value: &Value, pointer: &str) -> Option<u32> {
    value
        .pointer(pointer)?
        .as_u64()
        .and_then(|line| u32::try_from(line).ok())
}

fn read_pull(value: &Value, fields: &PullFields) -> Option<WirePull> {
    Some(WirePull {
        number: value.pointer(fields.number)?.as_u64()?,
        title: text_at(value, fields.title)?,
        author: text_at(value, fields.author)?,
        base: text_at(value, fields.base)?,
        head: text_at(value, fields.head)?,
        sha: text_at(value, fields.sha)?,
        description: text_at(value, fields.description).unwrap_or_default(),
        updated: text_at(value, UPDATED).unwrap_or_default(),
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, VariantArray)]
enum WireSide {
    Left,
    Right,
}

impl WireSide {
    const fn name(self) -> &'static str {
        match self {
            Self::Left => "LEFT",
            Self::Right => "RIGHT",
        }
    }

    const fn of(side: Side) -> Self {
        match side {
            Side::Old => Self::Left,
            Side::New => Self::Right,
        }
    }

    fn read(text: &str) -> Option<Side> {
        Self::VARIANTS
            .iter()
            .find(|side| side.name() == text)
            .map(|side| match side {
                Self::Left => Side::Old,
                Self::Right => Side::New,
            })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, VariantArray)]
enum ReviewState {
    Approved,
    ChangesRequested,
    RequestChanges,
    Pending,
}

impl ReviewState {
    const fn name(self) -> &'static str {
        match self {
            Self::Approved => "APPROVED",
            Self::ChangesRequested => "CHANGES_REQUESTED",
            Self::RequestChanges => "REQUEST_CHANGES",
            Self::Pending => "PENDING",
        }
    }

    fn read(text: &str) -> Option<Self> {
        Self::VARIANTS
            .iter()
            .copied()
            .find(|state| state.name() == text)
    }

    const fn kind(state: Option<Self>) -> NoteKind {
        match state {
            Some(Self::Approved) => NoteKind::Approved,
            Some(Self::ChangesRequested | Self::RequestChanges) => NoteKind::ChangesRequested,
            Some(Self::Pending) | None => NoteKind::Comment,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ThreadPrefix {
    Issue,
    Review,
}

impl ThreadPrefix {
    const fn name(self) -> &'static str {
        match self {
            Self::Issue => "issue-",
            Self::Review => "review-",
        }
    }

    fn id(self, id: u64) -> String {
        format!("{}{id}", self.name())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WireNote {
    pub(crate) author: String,
    pub(crate) body: String,
    pub(crate) when: String,
    pub(crate) kind: NoteKind,
    pub(crate) link: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WirePlace {
    pub(crate) path: String,
    pub(crate) side: Side,
    pub(crate) line: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WireThread {
    pub(crate) id: String,
    pub(crate) place: Option<WirePlace>,
    pub(crate) state: ThreadState,
    pub(crate) notes: Vec<WireNote>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct WireReviews {
    pub(crate) threads: Vec<WireThread>,
    pub(crate) with_comments: Vec<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WireCodeComment {
    pub(crate) id: u64,
    pub(crate) place: WirePlace,
    pub(crate) resolved: bool,
    pub(crate) note: WireNote,
}

fn note(value: &Value, author: &str, when: &str, kind: NoteKind) -> WireNote {
    WireNote {
        author: text_at(value, author).unwrap_or_default(),
        body: text_at(value, BODY).unwrap_or_default(),
        when: text_at(value, when).unwrap_or_default(),
        kind,
        link: text_at(value, "/html_url").or_else(|| {
            value
                .pointer(ID)
                .and_then(Value::as_u64)
                .map(|id| format!("{NOTE_ANCHOR}{id}"))
        }),
    }
}

pub(crate) const NOTE_ANCHOR: &str = "#note_";

pub(crate) fn anchored(mut threads: Vec<WireThread>, pull_url: &str) -> Vec<WireThread> {
    for note in threads
        .iter_mut()
        .flat_map(|thread| thread.notes.iter_mut())
    {
        if let Some(link) = note.link.as_mut()
            && link.starts_with(NOTE_ANCHOR)
        {
            *link = format!("{pull_url}{link}");
        }
    }
    threads
}

fn url_path(path: &str) -> String {
    path.chars()
        .map(|character| match character {
            ' ' => "%20".to_owned(),
            '#' => "%23".to_owned(),
            '?' => "%3F".to_owned(),
            '%' => "%25".to_owned(),
            other => other.to_string(),
        })
        .collect()
}

pub(crate) fn web_url(kind: ForgeKind, host: &str, slug: &str, linked: &Linked) -> String {
    let repo = format!("https://{host}/{slug}");
    let lines = |lines: &Option<Span>| match (kind, lines) {
        (_, None) => String::new(),
        (_, Some(span)) if span.start() == span.end() => format!("#L{}", span.end().number()),
        (ForgeKind::GitLab, Some(span)) => {
            format!("#L{}-{}", span.start().number(), span.end().number())
        }
        (ForgeKind::GitHub | ForgeKind::Forgejo, Some(span)) => {
            format!("#L{}-L{}", span.start().number(), span.end().number())
        }
    };
    match (kind, linked) {
        (ForgeKind::GitHub, Linked::Pull(number)) => format!("{repo}/pull/{number}"),
        (ForgeKind::GitLab, Linked::Pull(number)) => format!("{repo}/-/merge_requests/{number}"),
        (ForgeKind::Forgejo, Linked::Pull(number)) => format!("{repo}/pulls/{number}"),
        (ForgeKind::GitLab, Linked::Issue(number)) => format!("{repo}/-/issues/{number}"),
        (ForgeKind::GitHub | ForgeKind::Forgejo, Linked::Issue(number)) => {
            format!("{repo}/issues/{number}")
        }
        (ForgeKind::GitLab, Linked::Commit(commit)) => {
            format!("{repo}/-/commit/{}", commit.as_str())
        }
        (ForgeKind::GitHub | ForgeKind::Forgejo, Linked::Commit(commit)) => {
            format!("{repo}/commit/{}", commit.as_str())
        }
        (
            kind,
            Linked::File {
                commit,
                path,
                lines: span,
            },
        ) => {
            let at = match kind {
                ForgeKind::GitHub => "blob",
                ForgeKind::GitLab => "-/blob",
                ForgeKind::Forgejo => "src/commit",
            };
            format!(
                "{repo}/{at}/{}/{}{}",
                commit.as_str(),
                url_path(path.as_str()),
                lines(span)
            )
        }
        (_, Linked::User(login)) => format!("https://{host}/{login}"),
    }
}

fn on_pull(id: String, note: WireNote) -> WireThread {
    WireThread {
        id,
        place: None,
        state: ThreadState::Open,
        notes: vec![note],
    }
}

fn items(text: &str) -> Option<Vec<Value>> {
    serde_json::from_str(text).ok()
}

pub(crate) fn parse_issue_comments(text: &str) -> Option<Vec<WireThread>> {
    items(text)?
        .iter()
        .map(|comment| {
            Some(on_pull(
                ThreadPrefix::Issue.id(comment.pointer(ID)?.as_u64()?),
                note(comment, USER, CREATED, NoteKind::Comment),
            ))
        })
        .collect()
}

pub(crate) fn parse_reviews(text: &str) -> Option<WireReviews> {
    let mut reviews = WireReviews::default();
    for review in items(text)? {
        let id = review.pointer(ID)?.as_u64()?;
        let state = text_at(&review, "/state").and_then(|state| ReviewState::read(&state));
        if state == Some(ReviewState::Pending) {
            continue;
        }
        if review.pointer("/comments_count").and_then(Value::as_u64) != Some(0) {
            reviews.with_comments.push(id);
        }
        let kind = ReviewState::kind(state);
        let note = note(&review, USER, SUBMITTED, kind);
        if kind != NoteKind::Comment || !note.body.trim().is_empty() {
            reviews
                .threads
                .push(on_pull(ThreadPrefix::Review.id(id), note));
        }
    }
    Some(reviews)
}

pub(crate) fn parse_github_code_threads(text: &str) -> Option<Vec<WireThread>> {
    let mut threads: Vec<WireThread> = Vec::new();
    let mut thread_of: HashMap<u64, usize> = HashMap::new();
    for comment in items(text)? {
        let id = comment.pointer(ID)?.as_u64()?;
        let parent = comment
            .pointer("/in_reply_to_id")
            .and_then(Value::as_u64)
            .and_then(|parent| thread_of.get(&parent).copied());
        let index = if let Some(index) = parent {
            index
        } else {
            let line = line_at(&comment, "/line");
            threads.push(WireThread {
                id: id.to_string(),
                place: Some(WirePlace {
                    path: text_at(&comment, "/path")?,
                    side: text_at(&comment, "/side")
                        .and_then(|side| WireSide::read(&side))
                        .unwrap_or(Side::New),
                    line: line
                        .or_else(|| line_at(&comment, "/original_line"))
                        .unwrap_or(1),
                }),
                state: if line.is_some() {
                    ThreadState::Open
                } else {
                    ThreadState::Outdated
                },
                notes: Vec::new(),
            });
            threads.len() - 1
        };
        threads
            .get_mut(index)?
            .notes
            .push(note(&comment, USER, CREATED, NoteKind::Comment));
        thread_of.insert(id, index);
    }
    Some(threads)
}

fn gitlab_place(position: &Value) -> WirePlace {
    match line_at(position, "/new_line") {
        Some(line) => WirePlace {
            path: text_at(position, "/new_path").unwrap_or_default(),
            side: Side::New,
            line,
        },
        None => WirePlace {
            path: text_at(position, "/old_path").unwrap_or_default(),
            side: Side::Old,
            line: line_at(position, "/old_line").unwrap_or(1),
        },
    }
}

pub(crate) fn parse_discussions(text: &str) -> Option<Vec<WireThread>> {
    let mut threads = Vec::new();
    for discussion in items(text)? {
        let notes: Vec<&Value> = discussion
            .pointer("/notes")?
            .as_array()?
            .iter()
            .filter(|note| note.pointer("/system").and_then(Value::as_bool) != Some(true))
            .collect();
        let Some(first) = notes.first() else {
            continue;
        };
        let resolved = first.pointer("/resolved").and_then(Value::as_bool) == Some(true);
        threads.push(WireThread {
            id: text_at(&discussion, ID)?,
            place: first
                .pointer("/position")
                .filter(|position| !position.is_null())
                .map(gitlab_place),
            state: if resolved {
                ThreadState::Resolved
            } else {
                ThreadState::Open
            },
            notes: notes
                .iter()
                .map(|note_value| note(note_value, AUTHOR, CREATED, NoteKind::Comment))
                .collect(),
        });
    }
    Some(threads)
}

pub(crate) fn parse_forgejo_comments(text: &str) -> Option<Vec<WireCodeComment>> {
    items(text)?
        .iter()
        .map(|comment| {
            let new = line_at(comment, "/position").unwrap_or(0);
            let old = line_at(comment, "/original_position").unwrap_or(0);
            let (side, line) = if new == 0 && old > 0 {
                (Side::Old, old)
            } else {
                (Side::New, new)
            };
            Some(WireCodeComment {
                id: comment.pointer(ID)?.as_u64()?,
                place: WirePlace {
                    path: text_at(comment, "/path")?,
                    side,
                    line,
                },
                resolved: comment
                    .pointer("/resolver")
                    .is_some_and(|resolver| !resolver.is_null()),
                note: note(comment, USER, CREATED, NoteKind::Comment),
            })
        })
        .collect()
}

pub(crate) fn forgejo_threads(mut comments: Vec<WireCodeComment>) -> Vec<WireThread> {
    comments.sort_by(|left, right| {
        left.note
            .when
            .cmp(&right.note.when)
            .then(left.id.cmp(&right.id))
    });
    let mut threads: Vec<WireThread> = Vec::new();
    for comment in comments {
        let state = if comment.resolved {
            ThreadState::Resolved
        } else {
            ThreadState::Open
        };
        if let Some(thread) = threads
            .iter_mut()
            .find(|thread| thread.place.as_ref() == Some(&comment.place))
        {
            thread.notes.push(comment.note);
            if comment.resolved {
                thread.state = state;
            }
        } else {
            threads.push(WireThread {
                id: comment.id.to_string(),
                place: Some(comment.place),
                state,
                notes: vec![comment.note],
            });
        }
    }
    threads
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WireDiffRefs {
    pub(crate) base: String,
    pub(crate) start: String,
    pub(crate) head: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WireComment {
    pub(crate) path: String,
    pub(crate) side: Side,
    pub(crate) start: Option<u32>,
    pub(crate) line: u32,
    pub(crate) body: String,
    pub(crate) reply: Option<String>,
}

impl WireComment {
    fn spanned_body(&self) -> String {
        match self.start {
            Some(start) => format!("(lines {start}-{})\n\n{}", self.line, self.body),
            None => self.body.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WireReview {
    pub(crate) commit: String,
    pub(crate) body: Option<String>,
    pub(crate) decision: Decision,
    pub(crate) comments: Vec<WireComment>,
}

fn with_body(mut value: Value, body: Option<&String>) -> Value {
    if let (Some(object), Some(body)) = (value.as_object_mut(), body) {
        object.insert("body".to_owned(), json!(body));
    }
    value
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Api {
    kind: ForgeKind,
    repo: String,
}

impl Api {
    pub(crate) fn new(kind: ForgeKind, host: &str, slug: &str) -> Self {
        let repo = match kind {
            ForgeKind::GitHub if host == HostMark::GitHub.name() => {
                format!("https://api.github.com/repos/{slug}")
            }
            ForgeKind::GitHub => format!("https://{host}/api/v3/repos/{slug}"),
            ForgeKind::GitLab => {
                format!(
                    "https://{host}/api/v4/projects/{}",
                    slug.replace('/', "%2F")
                )
            }
            ForgeKind::Forgejo => format!("https://{host}/api/v1/repos/{slug}"),
        };
        Self { kind, repo }
    }

    fn authorization(&self, token: &str) -> String {
        match self.kind {
            ForgeKind::GitHub => format!("Authorization: Bearer {token}"),
            ForgeKind::GitLab => format!("PRIVATE-TOKEN: {token}"),
            ForgeKind::Forgejo => format!("Authorization: token {token}"),
        }
    }

    pub(crate) fn config(&self, request: &Request, token: Option<&str>) -> String {
        let mut config = setting(Setting::Url, &request.url);
        config.push_str(&setting(Setting::Header, ACCEPT_JSON));
        if let Some(token) = token {
            config.push_str(&setting(Setting::Header, &self.authorization(token)));
        }
        if request.method != Method::Get {
            config.push_str(&setting(Setting::Request, request.method.name()));
        }
        if let Some(body) = &request.body {
            config.push_str(&setting(Setting::Header, SEND_JSON));
            config.push_str(&setting(Setting::Data, &body.to_string()));
        }
        config.push_str(&setting(Setting::WriteOut, STATUS_LAST));
        config
    }

    fn pulls_url(&self) -> String {
        match self.kind {
            ForgeKind::GitHub | ForgeKind::Forgejo => format!("{}/pulls", self.repo),
            ForgeKind::GitLab => format!("{}/merge_requests", self.repo),
        }
    }

    pub(crate) fn list(&self) -> Request {
        let query = match self.kind {
            ForgeKind::GitHub => "state=open&per_page=100",
            ForgeKind::GitLab => "state=opened&per_page=100",
            ForgeKind::Forgejo => "state=open&limit=50",
        };
        Request::get(format!("{}?{query}", self.pulls_url()))
    }

    pub(crate) fn pull(&self, number: u64) -> Request {
        Request::get(format!("{}/{number}", self.pulls_url()))
    }

    const fn fields(&self) -> &'static PullFields {
        match self.kind {
            ForgeKind::GitHub | ForgeKind::Forgejo => &PULL_FIELDS,
            ForgeKind::GitLab => &MERGE_REQUEST_FIELDS,
        }
    }

    pub(crate) fn parse_pulls(&self, text: &str) -> Option<Vec<WirePull>> {
        let value: Value = serde_json::from_str(text).ok()?;
        value
            .as_array()?
            .iter()
            .map(|pull| read_pull(pull, self.fields()))
            .collect()
    }

    pub(crate) fn parse_pull(&self, text: &str) -> Option<WirePull> {
        let value: Value = serde_json::from_str(text).ok()?;
        read_pull(&value, self.fields())
    }

    pub(crate) fn parse_diff_refs(text: &str) -> Option<WireDiffRefs> {
        let value: Value = serde_json::from_str(text).ok()?;
        Some(WireDiffRefs {
            base: text_at(&value, "/diff_refs/base_sha")?,
            start: text_at(&value, "/diff_refs/start_sha")?,
            head: text_at(&value, "/diff_refs/head_sha")?,
        })
    }

    const fn page(&self) -> &'static str {
        match self.kind {
            ForgeKind::GitHub | ForgeKind::GitLab => "per_page=100",
            ForgeKind::Forgejo => "limit=50",
        }
    }

    pub(crate) fn issue_comments(&self, number: u64) -> Request {
        Request::get(format!(
            "{}/issues/{number}/comments?{}",
            self.repo,
            self.page()
        ))
    }

    pub(crate) fn reviews(&self, number: u64) -> Request {
        Request::get(format!(
            "{}/{number}/reviews?{}",
            self.pulls_url(),
            self.page()
        ))
    }

    pub(crate) fn review_comments(&self, number: u64) -> Request {
        Request::get(format!(
            "{}/{number}/comments?{}",
            self.pulls_url(),
            self.page()
        ))
    }

    pub(crate) fn forgejo_review_comments(&self, number: u64, review: u64) -> Request {
        Request::get(format!(
            "{}/{number}/reviews/{review}/comments",
            self.pulls_url()
        ))
    }

    pub(crate) fn discussions(&self, number: u64) -> Request {
        Request::get(format!(
            "{}/{number}/discussions?{}",
            self.pulls_url(),
            self.page()
        ))
    }

    pub(crate) fn github_review(&self, number: u64, review: &WireReview) -> Option<Request> {
        let event = match review.decision {
            Decision::Comment => "COMMENT",
            Decision::Approve => "APPROVE",
            Decision::RequestChanges => "REQUEST_CHANGES",
        };
        let comments: Vec<Value> = review
            .comments
            .iter()
            .filter(|comment| comment.reply.is_none())
            .map(|comment| {
                let mut value = json!({
                    "path": comment.path,
                    "line": comment.line,
                    "side": WireSide::of(comment.side).name(),
                    "body": comment.body,
                });
                if let (Some(start), Some(object)) = (comment.start, value.as_object_mut()) {
                    object.insert("start_line".to_owned(), json!(start));
                    object.insert(
                        "start_side".to_owned(),
                        json!(WireSide::of(comment.side).name()),
                    );
                }
                value
            })
            .collect();
        if comments.is_empty() && review.body.is_none() && review.decision == Decision::Comment {
            return None;
        }
        let body = json!({"commit_id": review.commit, "event": event, "comments": comments});
        Some(Request::post(
            format!("{}/{number}/reviews", self.pulls_url()),
            Some(with_body(body, review.body.as_ref())),
        ))
    }

    pub(crate) fn github_replies(&self, number: u64, review: &WireReview) -> Vec<Request> {
        review
            .comments
            .iter()
            .filter_map(|comment| {
                let thread = comment.reply.as_ref()?;
                let url = if thread.parse::<u64>().is_ok() {
                    format!("{}/{number}/comments/{thread}/replies", self.pulls_url())
                } else {
                    format!("{}/issues/{number}/comments", self.repo)
                };
                Some(Request::post(url, Some(json!({"body": comment.body}))))
            })
            .collect()
    }

    pub(crate) fn forgejo_review(&self, number: u64, review: &WireReview) -> Request {
        let event = match review.decision {
            Decision::Comment => "COMMENT",
            Decision::Approve => "APPROVED",
            Decision::RequestChanges => "REQUEST_CHANGES",
        };
        let comments: Vec<Value> = review
            .comments
            .iter()
            .map(|comment| {
                let (new_position, old_position) = match comment.side {
                    Side::Old => (0, comment.line),
                    Side::New => (comment.line, 0),
                };
                json!({
                    "path": comment.path,
                    "body": comment.spanned_body(),
                    "new_position": new_position,
                    "old_position": old_position,
                })
            })
            .collect();
        let body = json!({"commit_id": review.commit, "event": event, "comments": comments});
        Request::post(
            format!("{}/{number}/reviews", self.pulls_url()),
            Some(with_body(body, review.body.as_ref())),
        )
    }

    pub(crate) fn gitlab_review(
        &self,
        number: u64,
        review: &WireReview,
        refs: &WireDiffRefs,
    ) -> Vec<Request> {
        let merge_request = format!("{}/{number}", self.pulls_url());
        let mut requests: Vec<Request> = review
            .comments
            .iter()
            .map(|comment| {
                if let Some(thread) = &comment.reply {
                    return Request::post(
                        format!("{merge_request}/discussions/{thread}/notes"),
                        Some(json!({"body": comment.body})),
                    );
                }
                let mut position = Map::new();
                position.insert("position_type".to_owned(), json!("text"));
                position.insert("base_sha".to_owned(), json!(refs.base));
                position.insert("start_sha".to_owned(), json!(refs.start));
                position.insert("head_sha".to_owned(), json!(refs.head));
                position.insert("old_path".to_owned(), json!(comment.path));
                position.insert("new_path".to_owned(), json!(comment.path));
                let line_key = match comment.side {
                    Side::Old => "old_line",
                    Side::New => "new_line",
                };
                position.insert(line_key.to_owned(), json!(comment.line));
                Request::post(
                    format!("{merge_request}/discussions"),
                    Some(json!({"body": comment.spanned_body(), "position": position})),
                )
            })
            .collect();
        if let Some(body) = &review.body {
            requests.push(Request::post(
                format!("{merge_request}/notes"),
                Some(json!({"body": body})),
            ));
        }
        if review.decision == Decision::Approve {
            requests.push(Request::post(format!("{merge_request}/approve"), None));
        }
        requests
    }
}

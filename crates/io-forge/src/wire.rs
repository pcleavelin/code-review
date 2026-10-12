use std::collections::HashMap;
use std::fmt::Write as _;
use std::str;

use domain::{
    CommitId, Decision, DraftId, ForgeKind, Line, Linked, LinkedFile, RefName, RelativePath,
    Revision, Side, Span, ThreadState,
};
use io_process::Captured;
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
const GITHUB_API: &str = "api.github.com";
const UPLOADS: &str = "uploads/";
const PARENT: &str = "..";
const UNSAFE_IN_PATH: [char; 2] = ['?', '#'];
const GITLAB_PROJECT: &str = "-/project/";
const ATTACHMENTS: &str = "attachments/";
const UPLOAD_LEADS: [&str; 2] = ["](", "src=\""];

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
    Delete,
}

impl Method {
    const fn name(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Delete => "DELETE",
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

    fn delete(url: String) -> Self {
        Self {
            method: Method::Delete,
            url,
            body: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Delivery {
    pub(crate) request: Request,
    pub(crate) drafts: Vec<DraftId>,
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
const ACCEPT_GITHUB_FULL: &str = "Accept: application/vnd.github.full+json";
const SEND_JSON: &str = "Content-Type: application/json";
const PAGING_THEN_STATUS: &str = "\n%header{link}\n%{http_code}";
const NEXT: &str = "rel=\"next\"";

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
    pub(crate) next_url: Option<String>,
}

fn next_page(paging: &str) -> Option<String> {
    paging.split('<').find_map(|entry| {
        let (url, parameters) = entry.split_once('>')?;
        parameters
            .split([',', ';'])
            .any(|parameter| parameter.trim() == NEXT)
            .then(|| url.trim().to_owned())
    })
}

pub(crate) fn parse_response(stdout: &str) -> Option<WireResponse> {
    let (rest, status) = stdout.rsplit_once('\n')?;
    let (body, paging) = rest.rsplit_once('\n')?;
    Some(WireResponse {
        status: status.trim().parse().ok()?,
        body: body.to_owned(),
        next_url: next_page(paging),
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WireUser {
    pub(crate) id: u64,
    pub(crate) login: String,
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
const BODY_HTML: &str = "/body_html";
const CREATED: &str = "/created_at";
const SUBMITTED: &str = "/submitted_at";
const UPDATED: &str = "/updated_at";
const ID: &str = "/id";
const REVIEW_ID: &str = "/pull_request_review_id";
const REPOSITORY_NAME: &str = "/repository/full_name";
const FOUND_ITEMS: &str = "items";

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

    const fn decision(state: Option<Self>) -> Decision {
        match state {
            Some(Self::Approved) => Decision::Approve,
            Some(Self::ChangesRequested | Self::RequestChanges) => Decision::RequestChanges,
            Some(Self::Pending) | None => Decision::Comment,
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
    pub(crate) submission: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WireSubmission {
    pub(crate) id: String,
    pub(crate) decision: Decision,
    pub(crate) note: WireNote,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct WireReviews {
    pub(crate) submissions: Vec<WireSubmission>,
    pub(crate) with_comments: Vec<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WireCodeComment {
    pub(crate) id: u64,
    pub(crate) place: WirePlace,
    pub(crate) resolved: bool,
    pub(crate) note: WireNote,
    pub(crate) submission: Option<u64>,
}

fn note(value: &Value, author: &str, when: &str) -> WireNote {
    WireNote {
        author: text_at(value, author).unwrap_or_default(),
        body: text_at(value, BODY).unwrap_or_default(),
        when: text_at(value, when).unwrap_or_default(),
        link: text_at(value, "/html_url").or_else(|| {
            value
                .pointer(ID)
                .and_then(Value::as_u64)
                .map(|id| format!("{NOTE_ANCHOR}{id}"))
        }),
    }
}

const GITHUB_SITE: &str = "https://github.com/";
const ASSETS: &str = "assets";
const USER_ATTACHMENTS: &str = "user-attachments";
const SIGNED_IMAGES: &str = "https://private-user-images.githubusercontent.com/";
const MARKDOWN_IMAGE: &str = "![";
const MARKDOWN_TARGET: &str = "](";
const HTML_IMAGE: &str = "<img";
const HTML_SOURCE: &str = " src=\"";
const ESCAPED_AMPERSAND: &str = "&amp;";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WireSignedPicture {
    pub(crate) written: String,
    pub(crate) signed: String,
}

fn html_source(tag_start: &str) -> Option<String> {
    let tag = tag_start.split('>').next()?;
    let (_, rest) = tag.split_once(HTML_SOURCE)?;
    let (source, _) = rest.split_once('"')?;
    Some(source.replace(ESCAPED_AMPERSAND, "&"))
}

fn markdown_target(image_start: &str) -> Option<String> {
    let (_, rest) = image_start.split_once(MARKDOWN_TARGET)?;
    let end = rest
        .find(|character: char| character == ')' || character.is_whitespace())
        .unwrap_or(rest.len());
    rest.get(..end).map(str::to_owned)
}

fn pictures_in(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = text;
    loop {
        let markdown = rest.find(MARKDOWN_IMAGE);
        let html = rest.find(HTML_IMAGE);
        let (at, source) = match (markdown, html) {
            (Some(markdown), Some(html)) if html < markdown => {
                (html, rest.get(html..).and_then(html_source))
            }
            (Some(markdown), _) => (markdown, rest.get(markdown..).and_then(markdown_target)),
            (None, Some(html)) => (html, rest.get(html..).and_then(html_source)),
            (None, None) => return found,
        };
        found.extend(source);
        rest = rest.get(at + 1..).unwrap_or_default();
    }
}

fn last_segment(link: &str) -> &str {
    let path = link.split(UNSAFE_IN_PATH).next().unwrap_or(link);
    path.rsplit('/').next().unwrap_or(path)
}

pub(crate) fn signed_in_item(item: &Value) -> Vec<WireSignedPicture> {
    let (Some(body), Some(html)) = (text_at(item, BODY), text_at(item, BODY_HTML)) else {
        return Vec::new();
    };
    let written: Vec<String> = pictures_in(&body)
        .into_iter()
        .filter(|link| {
            link.strip_prefix(GITHUB_SITE).is_some_and(|path| {
                let segments: Vec<&str> = path.split('/').collect();
                (segments.first() == Some(&USER_ATTACHMENTS) && segments.get(1) == Some(&ASSETS))
                    || segments.get(2) == Some(&ASSETS)
            })
        })
        .collect();
    let signed: Vec<String> = pictures_in(&html)
        .into_iter()
        .filter(|link| link.starts_with(SIGNED_IMAGES))
        .collect();
    if written.len() != signed.len() {
        return Vec::new();
    }
    written
        .into_iter()
        .zip(signed)
        .filter(|(written, signed)| signed.contains(last_segment(written)))
        .map(|(written, signed)| WireSignedPicture { written, signed })
        .collect()
}

pub(crate) fn signed_in_page(items: &[Value]) -> Vec<WireSignedPicture> {
    items.iter().flat_map(signed_in_item).collect()
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, VariantArray)]
enum FileRoute {
    GitHubBlob,
    GitLabBlob,
    ForgejoCommit,
    ForgejoBranch,
    ForgejoTag,
}

impl FileRoute {
    const fn spelled(self) -> &'static str {
        match self {
            Self::GitHubBlob => "blob/",
            Self::GitLabBlob => "-/blob/",
            Self::ForgejoCommit => "src/commit/",
            Self::ForgejoBranch => "src/branch/",
            Self::ForgejoTag => "src/tag/",
        }
    }

    const fn kind(self) -> ForgeKind {
        match self {
            Self::GitHubBlob => ForgeKind::GitHub,
            Self::GitLabBlob => ForgeKind::GitLab,
            Self::ForgejoCommit | Self::ForgejoBranch | Self::ForgejoTag => ForgeKind::Forgejo,
        }
    }

    const fn for_revision(kind: ForgeKind, at: &Revision) -> Self {
        match (kind, at) {
            (ForgeKind::GitHub, _) => Self::GitHubBlob,
            (ForgeKind::GitLab, _) => Self::GitLabBlob,
            (ForgeKind::Forgejo, Revision::Commit(_)) => Self::ForgejoCommit,
            (ForgeKind::Forgejo, Revision::Named(_)) => Self::ForgejoBranch,
        }
    }
}

pub(crate) fn linked_file(
    kind: ForgeKind,
    host: &str,
    slug: &str,
    url: &str,
    branches: &[&str],
) -> Option<LinkedFile> {
    let (address, fragment) = url
        .split_once('#')
        .map_or((url, None), |(address, fragment)| (address, Some(fragment)));
    let address = address
        .split_once('?')
        .map_or(address, |(address, _)| address);
    let rest = [Scheme::Https, Scheme::Http]
        .into_iter()
        .find_map(|scheme| address.strip_prefix(scheme.name()))?;
    let repo = format!("{host}/{slug}/");
    let rest = rest
        .get(..repo.len())
        .filter(|named| named.eq_ignore_ascii_case(&repo))
        .and_then(|_| rest.get(repo.len()..))?;
    let tail = FileRoute::VARIANTS
        .iter()
        .filter(|route| route.kind() == kind)
        .find_map(|route| rest.strip_prefix(route.spelled()))?;
    let (revision, path) = branches
        .iter()
        .filter_map(|branch| {
            tail.strip_prefix(branch)
                .and_then(|after| after.strip_prefix('/'))
                .map(|path| (*branch, path))
        })
        .max_by_key(|(branch, _)| branch.len())
        .or_else(|| tail.split_once('/'))?;
    let path = percent_decoded(path);
    if revision.is_empty() || path.is_empty() {
        return None;
    }
    Some(LinkedFile {
        at: CommitId::new(revision)
            .map_or_else(|| Revision::Named(RefName::new(revision)), Revision::Commit),
        path: RelativePath::new(&path),
        lines: fragment.and_then(line_span),
    })
}

fn revision_text(at: &Revision) -> &str {
    match at {
        Revision::Commit(commit) => commit.as_str(),
        Revision::Named(name) => name.as_str(),
    }
}

fn line_span(fragment: &str) -> Option<Span> {
    let (first, last) = fragment.split_once('-').unwrap_or((fragment, fragment));
    let line = |text: &str| -> Option<u32> {
        let digits: String = text.chars().take_while(char::is_ascii_digit).collect();
        digits.parse::<u32>().ok()?.checked_sub(1)
    };
    let first = line(first.strip_prefix('L')?)?;
    let last = line(last.strip_prefix('L').unwrap_or(last))?;
    Span::new(Line::new(first.min(last)), Line::new(first.max(last)))
}

fn percent_decoded(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while let Some(&byte) = bytes.get(at) {
        let escaped = (byte == b'%')
            .then(|| bytes.get(at + 1..at + 3))
            .flatten()
            .and_then(|hex| str::from_utf8(hex).ok())
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        if let Some(value) = escaped {
            decoded.push(value);
            at += 3;
        } else {
            decoded.push(byte);
            at += 1;
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
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
        (kind, Linked::File(file)) => format!(
            "{repo}/{}{}/{}{}",
            FileRoute::for_revision(kind, &file.at).spelled(),
            revision_text(&file.at),
            url_path(file.path.as_str()),
            lines(&file.lines)
        ),
        (_, Linked::User(login)) => format!("https://{host}/{login}"),
    }
}

fn on_pull(id: String, note: WireNote) -> WireThread {
    WireThread {
        id,
        place: None,
        state: ThreadState::Open,
        notes: vec![note],
        submission: None,
    }
}

pub(crate) fn parse_items(text: &str) -> Option<Vec<Value>> {
    match serde_json::from_str(text).ok()? {
        Value::Array(items) => Some(items),
        Value::Object(mut found) => match found.remove(FOUND_ITEMS)? {
            Value::Array(items) => Some(items),
            _ => None,
        },
        _ => None,
    }
}

pub(crate) fn parse_issue_comments(items: &[Value]) -> Option<Vec<WireThread>> {
    items
        .iter()
        .map(|comment| {
            Some(on_pull(
                ThreadPrefix::Issue.id(comment.pointer(ID)?.as_u64()?),
                note(comment, USER, CREATED),
            ))
        })
        .collect()
}

pub(crate) fn parse_reviews(items: &[Value]) -> Option<WireReviews> {
    let mut reviews = WireReviews::default();
    for review in items {
        let id = review.pointer(ID)?.as_u64()?;
        let state = text_at(review, "/state").and_then(|state| ReviewState::read(&state));
        if state == Some(ReviewState::Pending) {
            continue;
        }
        if review.pointer("/comments_count").and_then(Value::as_u64) != Some(0) {
            reviews.with_comments.push(id);
        }
        reviews.submissions.push(WireSubmission {
            id: ThreadPrefix::Review.id(id),
            decision: ReviewState::decision(state),
            note: note(review, USER, SUBMITTED),
        });
    }
    Some(reviews)
}

fn submission_of(comment: &Value) -> Option<String> {
    comment
        .pointer(REVIEW_ID)
        .and_then(Value::as_u64)
        .map(|review| ThreadPrefix::Review.id(review))
}

pub(crate) fn parse_github_code_threads(items: &[Value]) -> Option<Vec<WireThread>> {
    let mut threads: Vec<WireThread> = Vec::new();
    let mut thread_of: HashMap<u64, usize> = HashMap::new();
    for comment in items {
        let id = comment.pointer(ID)?.as_u64()?;
        let parent = comment
            .pointer("/in_reply_to_id")
            .and_then(Value::as_u64)
            .and_then(|parent| thread_of.get(&parent).copied());
        let index = if let Some(index) = parent {
            index
        } else {
            let line = line_at(comment, "/line");
            threads.push(WireThread {
                id: id.to_string(),
                place: Some(WirePlace {
                    path: text_at(comment, "/path")?,
                    side: text_at(comment, "/side")
                        .and_then(|side| WireSide::read(&side))
                        .unwrap_or(Side::New),
                    line: line
                        .or_else(|| line_at(comment, "/original_line"))
                        .unwrap_or(1),
                }),
                state: if line.is_some() {
                    ThreadState::Open
                } else {
                    ThreadState::Outdated
                },
                notes: Vec::new(),
                submission: submission_of(comment),
            });
            threads.len() - 1
        };
        threads
            .get_mut(index)?
            .notes
            .push(note(comment, USER, CREATED));
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

pub(crate) fn parse_discussions(items: &[Value], head_commit: &str) -> Option<Vec<WireThread>> {
    let mut threads = Vec::new();
    for discussion in items {
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
        let position = first
            .pointer("/position")
            .filter(|position| !position.is_null());
        let outdated = position
            .and_then(|position| text_at(position, "/head_sha"))
            .is_some_and(|head_sha| head_sha != head_commit);
        threads.push(WireThread {
            id: text_at(discussion, ID)?,
            place: position.map(gitlab_place),
            state: if resolved {
                ThreadState::Resolved
            } else if outdated {
                ThreadState::Outdated
            } else {
                ThreadState::Open
            },
            notes: notes
                .iter()
                .map(|note_value| note(note_value, AUTHOR, CREATED))
                .collect(),
            submission: None,
        });
    }
    Some(threads)
}

pub(crate) fn parse_forgejo_comments(items: &[Value]) -> Option<Vec<WireCodeComment>> {
    items
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
                note: note(comment, USER, CREATED),
                submission: comment.pointer(REVIEW_ID).and_then(Value::as_u64),
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
                submission: comment
                    .submission
                    .map(|review| ThreadPrefix::Review.id(review)),
            });
        }
    }
    threads
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct WireApprovals {
    pub(crate) can_approve: bool,
    pub(crate) has_approved: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WireDiffRefs {
    pub(crate) base: String,
    pub(crate) start: String,
    pub(crate) head: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WireComment {
    pub(crate) draft: DraftId,
    pub(crate) path: String,
    pub(crate) old_path: String,
    pub(crate) side: Side,
    pub(crate) start: Option<u32>,
    pub(crate) line: u32,
    pub(crate) counterpart: Option<u32>,
    pub(crate) body: String,
    pub(crate) reply: Option<String>,
}

impl WireComment {
    fn lines(&self) -> (Option<u32>, Option<u32>) {
        match self.side {
            Side::Old => (Some(self.line), self.counterpart),
            Side::New => (self.counterpart, Some(self.line)),
        }
    }

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
    origin: String,
    repo: String,
    site: String,
    slug: String,
}

impl Api {
    pub(crate) fn new(kind: ForgeKind, host: &str, slug: &str) -> Self {
        let github = kind == ForgeKind::GitHub && host == HostMark::GitHub.name();
        let origin = if github {
            format!("https://{GITHUB_API}/")
        } else {
            format!("https://{host}/")
        };
        let repo = match kind {
            ForgeKind::GitHub if github => format!("{origin}repos/{slug}"),
            ForgeKind::GitHub => format!("{origin}api/v3/repos/{slug}"),
            ForgeKind::GitLab => {
                format!("{origin}api/v4/projects/{}", slug.replace('/', "%2F"))
            }
            ForgeKind::Forgejo => format!("{origin}api/v1/repos/{slug}"),
        };
        Self {
            kind,
            origin,
            repo,
            site: format!("https://{host}/"),
            slug: slug.to_owned(),
        }
    }

    pub(crate) fn picture_behind_token(&self, link: &str) -> Option<String> {
        let on_site = link
            .strip_prefix(&self.site)
            .filter(|rest| !rest.contains(PARENT) && !rest.contains(UNSAFE_IN_PATH))?;
        let in_repo = on_site
            .strip_prefix(&self.slug)
            .and_then(|rest| rest.strip_prefix('/'));
        match self.kind {
            ForgeKind::GitHub => None,
            ForgeKind::GitLab => {
                if let Some(upload) = in_repo.and_then(|rest| rest.strip_prefix(UPLOADS)) {
                    return Some(format!("{}/{UPLOADS}{upload}", self.repo));
                }
                let (project, upload) = on_site
                    .strip_prefix(GITLAB_PROJECT)?
                    .split_once(&format!("/{UPLOADS}"))?;
                (!project.is_empty() && project.chars().all(|digit| digit.is_ascii_digit()))
                    .then(|| format!("{}api/v4/projects/{project}/{UPLOADS}{upload}", self.site))
            }
            ForgeKind::Forgejo => {
                let attachment = on_site
                    .strip_prefix(ATTACHMENTS)
                    .or_else(|| in_repo.and_then(|rest| rest.strip_prefix(ATTACHMENTS)))?;
                (!attachment.is_empty()).then(|| format!("{}{ATTACHMENTS}{attachment}", self.site))
            }
        }
    }

    pub(crate) fn absolute_uploads(&self, text: &str) -> String {
        let web = format!("{}{}/", self.site, self.slug);
        UPLOAD_LEADS.iter().fold(text.to_owned(), |text, lead| {
            text.replace(
                &format!("{lead}/{UPLOADS}"),
                &format!("{lead}{web}{UPLOADS}"),
            )
        })
    }

    pub(crate) fn next_request(&self, next_url: &str) -> Option<Request> {
        next_url
            .starts_with(&self.origin)
            .then(|| Request::get(next_url.to_owned()))
    }

    pub(crate) fn authorization(&self, token: &str) -> String {
        match self.kind {
            ForgeKind::GitHub | ForgeKind::GitLab => format!("Authorization: Bearer {token}"),
            ForgeKind::Forgejo => format!("Authorization: token {token}"),
        }
    }

    pub(crate) fn config(&self, request: &Request, token: Option<&str>) -> String {
        let mut config = setting(Setting::Url, &request.url);
        let accept = match self.kind {
            ForgeKind::GitHub => ACCEPT_GITHUB_FULL,
            ForgeKind::GitLab | ForgeKind::Forgejo => ACCEPT_JSON,
        };
        config.push_str(&setting(Setting::Header, accept));
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
        config.push_str(&setting(Setting::WriteOut, PAGING_THEN_STATUS));
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

    pub(crate) fn parse_pulls(&self, items: &[Value]) -> Option<Vec<WirePull>> {
        items
            .iter()
            .map(|pull| read_pull(pull, self.fields()))
            .collect()
    }

    pub(crate) fn parse_pull(&self, text: &str) -> Option<WirePull> {
        let value: Value = serde_json::from_str(text).ok()?;
        read_pull(&value, self.fields())
    }

    pub(crate) fn user(&self) -> Request {
        let repos = format!("repos/{}", self.slug);
        match self.repo.strip_suffix(&repos) {
            Some(api) => Request::get(format!("{api}user")),
            None => Request::get(format!("{}api/v4/user", self.origin)),
        }
    }

    pub(crate) fn parse_user(&self, text: &str) -> Option<WireUser> {
        let value: Value = serde_json::from_str(text).ok()?;
        let login = match self.kind {
            ForgeKind::GitHub | ForgeKind::Forgejo => "/login",
            ForgeKind::GitLab => "/username",
        };
        Some(WireUser {
            id: value.pointer(ID)?.as_u64()?,
            login: text_at(&value, login)?,
        })
    }

    pub(crate) fn gitlab_approvals(&self, number: u64) -> Request {
        Request::get(format!("{}/approvals", self.merge_request(number)))
    }

    pub(crate) fn parse_approvals(text: &str) -> Option<WireApprovals> {
        let value: Value = serde_json::from_str(text).ok()?;
        Some(WireApprovals {
            can_approve: value.pointer("/user_can_approve")?.as_bool()?,
            has_approved: value.pointer("/user_has_approved")?.as_bool()?,
        })
    }

    fn repos_root(&self) -> &str {
        self.repo
            .strip_suffix(&format!("repos/{}", self.slug))
            .unwrap_or(&self.origin)
    }

    pub(crate) fn github_requested(&self) -> Request {
        Request::get(format!(
            "{}search/issues?q=repo%3A{}+is%3Apr+is%3Aopen+review-requested%3A%40me&{}",
            self.repos_root(),
            self.slug,
            self.page_size()
        ))
    }

    pub(crate) fn gitlab_requested(&self, user: &WireUser) -> Request {
        Request::get(format!(
            "{}?state=opened&reviewer_id={}&{}",
            self.pulls_url(),
            user.id,
            self.page_size()
        ))
    }

    pub(crate) fn forgejo_requested(&self) -> Request {
        let owner = self.slug.split('/').next().unwrap_or_default();
        Request::get(format!(
            "{}repos/issues/search?type=pulls&state=open&review_requested=true&owner={owner}&{}",
            self.repos_root(),
            self.page_size()
        ))
    }

    pub(crate) fn parse_requested(&self, items: &[Value]) -> Option<Vec<u64>> {
        let number = self.fields().number;
        items
            .iter()
            .filter(|item| {
                self.kind != ForgeKind::Forgejo
                    || item
                        .pointer(REPOSITORY_NAME)
                        .and_then(Value::as_str)
                        .is_some_and(|name| name.eq_ignore_ascii_case(&self.slug))
            })
            .map(|item| item.pointer(number)?.as_u64())
            .collect()
    }

    pub(crate) fn parse_diff_refs(text: &str) -> Option<WireDiffRefs> {
        let value: Value = serde_json::from_str(text).ok()?;
        Some(WireDiffRefs {
            base: text_at(&value, "/diff_refs/base_sha")?,
            start: text_at(&value, "/diff_refs/start_sha")?,
            head: text_at(&value, "/diff_refs/head_sha")?,
        })
    }

    const fn page_size(&self) -> &'static str {
        match self.kind {
            ForgeKind::GitHub | ForgeKind::GitLab => "per_page=100",
            ForgeKind::Forgejo => "limit=50",
        }
    }

    pub(crate) fn issue_comments(&self, number: u64) -> Request {
        Request::get(format!(
            "{}/issues/{number}/comments?{}",
            self.repo,
            self.page_size()
        ))
    }

    pub(crate) fn reviews(&self, number: u64) -> Request {
        Request::get(format!(
            "{}/{number}/reviews?{}",
            self.pulls_url(),
            self.page_size()
        ))
    }

    pub(crate) fn review_comments(&self, number: u64) -> Request {
        Request::get(format!(
            "{}/{number}/comments?{}",
            self.pulls_url(),
            self.page_size()
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
            self.page_size()
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

    pub(crate) fn github_deliveries(&self, number: u64, review: &WireReview) -> Vec<Delivery> {
        let (replies, in_review): (Vec<&WireComment>, Vec<&WireComment>) = review
            .comments
            .iter()
            .partition(|comment| comment.reply.is_some());
        let review_delivery = self.github_review(number, review).map(|request| Delivery {
            request,
            drafts: in_review.iter().map(|comment| comment.draft).collect(),
        });
        let reply_deliveries = self
            .github_replies(number, review)
            .into_iter()
            .zip(replies)
            .map(|(request, comment)| Delivery {
                request,
                drafts: vec![comment.draft],
            });
        review_delivery
            .into_iter()
            .chain(reply_deliveries)
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

    fn merge_request(&self, number: u64) -> String {
        format!("{}/{number}", self.pulls_url())
    }

    pub(crate) fn gitlab_pending_notes(
        &self,
        number: u64,
        review: &WireReview,
        refs: &WireDiffRefs,
    ) -> Vec<Request> {
        let draft_notes = format!("{}/draft_notes", self.merge_request(number));
        let mut requests: Vec<Request> = review
            .comments
            .iter()
            .map(|comment| {
                if let Some(thread) = &comment.reply {
                    return Request::post(
                        draft_notes.clone(),
                        Some(json!({"note": comment.body, "in_reply_to_discussion_id": thread})),
                    );
                }
                let mut position = Map::new();
                position.insert("position_type".to_owned(), json!("text"));
                position.insert("base_sha".to_owned(), json!(refs.base));
                position.insert("start_sha".to_owned(), json!(refs.start));
                position.insert("head_sha".to_owned(), json!(refs.head));
                position.insert("old_path".to_owned(), json!(comment.old_path));
                position.insert("new_path".to_owned(), json!(comment.path));
                let (old_line, new_line) = comment.lines();
                if let Some(line) = old_line {
                    position.insert("old_line".to_owned(), json!(line));
                }
                if let Some(line) = new_line {
                    position.insert("new_line".to_owned(), json!(line));
                }
                Request::post(
                    draft_notes.clone(),
                    Some(json!({"note": comment.spanned_body(), "position": position})),
                )
            })
            .collect();
        if let Some(body) = &review.body {
            requests.push(Request::post(draft_notes, Some(json!({"note": body}))));
        }
        requests
    }

    pub(crate) fn gitlab_publish(&self, number: u64) -> Request {
        Request::post(
            format!("{}/draft_notes/bulk_publish", self.merge_request(number)),
            None,
        )
    }

    pub(crate) fn gitlab_discard(&self, number: u64, note: PendingNote) -> Request {
        Request::delete(format!(
            "{}/draft_notes/{}",
            self.merge_request(number),
            note.id()
        ))
    }

    pub(crate) fn gitlab_approve(&self, number: u64, review: &WireReview) -> Request {
        Request::post(
            format!("{}/approve", self.merge_request(number)),
            Some(json!({"sha": review.commit})),
        )
    }

    pub(crate) fn parse_pending_note(text: &str) -> Option<PendingNote> {
        let value: Value = serde_json::from_str(text).ok()?;
        value.pointer(ID)?.as_u64().map(PendingNote::new)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PendingNote(u64);

impl PendingNote {
    pub(crate) const fn new(id: u64) -> Self {
        Self(id)
    }

    const fn id(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct BuiltInApp {
    pub(crate) host: &'static str,
    pub(crate) client: &'static str,
}

pub(crate) const BUILT_IN_APPS: [BuiltInApp; 2] = [
    BuiltInApp {
        host: "github.com",
        client: "Iv23ctOWEMAaCnTk6twY",
    },
    BuiltInApp {
        host: "gitlab.com",
        client: "",
    },
];

pub(crate) const BUILT_IN_APPS_PLACE: &str = "BUILT_IN_APPS in crates/io-forge/src/wire.rs";

const DEFAULT_INTERVAL: u64 = 5;
pub(crate) const SLOW_DOWN_STEP: u64 = 5;
const DEVICE_GRANT: &str = "urn:ietf:params:oauth:grant-type:device_code";
const REFRESH_GRANT: &str = "refresh_token";
const ACCEPT_FORM_ANSWER: &str = "Accept: application/json";
const SEND_FORM: &str = "Content-Type: application/x-www-form-urlencoded";

#[derive(Clone, Copy, Debug, PartialEq, Eq, VariantArray)]
pub(crate) enum OAuthError {
    Pending,
    SlowDown,
    Expired,
    Denied,
}

impl OAuthError {
    const fn name(self) -> &'static str {
        match self {
            Self::Pending => "authorization_pending",
            Self::SlowDown => "slow_down",
            Self::Expired => "expired_token",
            Self::Denied => "access_denied",
        }
    }

    fn read(text: &str) -> Option<Self> {
        Self::VARIANTS
            .iter()
            .copied()
            .find(|error| error.name() == text)
    }
}

pub(crate) fn device_url(kind: ForgeKind, host: &str) -> Option<String> {
    match kind {
        ForgeKind::GitHub => Some(format!("https://{host}/login/device/code")),
        ForgeKind::GitLab => Some(format!("https://{host}/oauth/authorize_device")),
        ForgeKind::Forgejo => None,
    }
}

pub(crate) fn token_url(kind: ForgeKind, host: &str) -> Option<String> {
    match kind {
        ForgeKind::GitHub => Some(format!("https://{host}/login/oauth/access_token")),
        ForgeKind::GitLab => Some(format!("https://{host}/oauth/token")),
        ForgeKind::Forgejo => None,
    }
}

pub(crate) fn token_page(kind: ForgeKind, host: &str) -> String {
    match kind {
        ForgeKind::GitHub => format!("https://{host}/settings/tokens"),
        ForgeKind::GitLab => format!("https://{host}/-/user_settings/personal_access_tokens"),
        ForgeKind::Forgejo => format!("https://{host}/user/settings/applications"),
    }
}

const fn scope(kind: ForgeKind) -> Option<&'static str> {
    match kind {
        ForgeKind::GitLab => Some("api"),
        ForgeKind::GitHub | ForgeKind::Forgejo => None,
    }
}

fn form_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            escaped.push(char::from(byte));
        } else {
            write!(escaped, "%{byte:02X}").unwrap_or_default();
        }
    }
    escaped
}

fn form(fields: &[(&str, &str)]) -> String {
    fields
        .iter()
        .map(|(name, value)| format!("{name}={}", form_escape(value)))
        .collect::<Vec<String>>()
        .join("&")
}

pub(crate) fn device_form(kind: ForgeKind, client: &str) -> String {
    match scope(kind) {
        Some(scope) => form(&[("client_id", client), ("scope", scope)]),
        None => form(&[("client_id", client)]),
    }
}

pub(crate) fn poll_form(client: &str, device_code: &str) -> String {
    form(&[
        ("client_id", client),
        ("device_code", device_code),
        ("grant_type", DEVICE_GRANT),
    ])
}

pub(crate) fn refresh_form(client: &str, refresh: &str) -> String {
    form(&[
        ("client_id", client),
        ("grant_type", REFRESH_GRANT),
        ("refresh_token", refresh),
    ])
}

pub(crate) fn form_config(url: &str, body: &str) -> String {
    let mut config = setting(Setting::Url, url);
    config.push_str(&setting(Setting::Header, ACCEPT_FORM_ANSWER));
    config.push_str(&setting(Setting::Header, SEND_FORM));
    config.push_str(&setting(Setting::Data, body));
    config.push_str(&setting(Setting::WriteOut, PAGING_THEN_STATUS));
    config
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WireDevice {
    pub(crate) device_code: String,
    pub(crate) user_code: String,
    pub(crate) verification: String,
    pub(crate) verification_complete: Option<String>,
    pub(crate) expires_in: u64,
    pub(crate) interval: u64,
}

pub(crate) fn parse_device(text: &str) -> Option<WireDevice> {
    let value: Value = serde_json::from_str(text).ok()?;
    Some(WireDevice {
        device_code: text_at(&value, "/device_code")?,
        user_code: text_at(&value, "/user_code")?,
        verification: text_at(&value, "/verification_uri")?,
        verification_complete: text_at(&value, "/verification_uri_complete"),
        expires_in: value.pointer("/expires_in")?.as_u64()?,
        interval: value
            .pointer("/interval")
            .and_then(Value::as_u64)
            .unwrap_or(DEFAULT_INTERVAL),
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WireGrant {
    pub(crate) access: String,
    pub(crate) refresh: Option<String>,
    pub(crate) expires_in: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum WirePolled {
    Granted(WireGrant),
    Said(OAuthError, Option<u64>),
    Refused { error: String, description: String },
}

pub(crate) fn parse_polled(text: &str) -> Option<WirePolled> {
    let value: Value = serde_json::from_str(text).ok()?;
    if let Some(access) = text_at(&value, "/access_token") {
        return Some(WirePolled::Granted(WireGrant {
            access,
            refresh: text_at(&value, "/refresh_token"),
            expires_in: value.pointer("/expires_in").and_then(Value::as_u64),
        }));
    }
    let error = text_at(&value, "/error")?;
    let interval = value.pointer("/interval").and_then(Value::as_u64);
    Some(match OAuthError::read(&error) {
        Some(known) => WirePolled::Said(known, interval),
        None => WirePolled::Refused {
            description: text_at(&value, "/error_description").unwrap_or_default(),
            error,
        },
    })
}

fn form_request(url: Option<String>, body: &str) -> Option<Captured> {
    url.map(|url| Captured::of(form_config(&url, body).as_bytes()))
}

pub(crate) fn device_request(kind: ForgeKind, host: &str, client: &str) -> Option<Captured> {
    form_request(device_url(kind, host), &device_form(kind, client))
}

pub(crate) fn poll_request(
    kind: ForgeKind,
    host: &str,
    client: &str,
    device_code: &str,
) -> Option<Captured> {
    form_request(token_url(kind, host), &poll_form(client, device_code))
}

pub(crate) fn refresh_request(
    kind: ForgeKind,
    host: &str,
    client: &str,
    refresh: &str,
) -> Option<Captured> {
    form_request(token_url(kind, host), &refresh_form(client, refresh))
}

const INTEGRATION_REFUSED: &str = "Resource not accessible by integration";

pub(crate) fn integration_refused(body: &str) -> bool {
    body.contains(INTEGRATION_REFUSED)
}

mod convert;
mod login;
mod wire;

use std::collections::HashSet;
use std::env;
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

use domain::{
    BranchName, Conversation, Credential, Decision, Draft, DraftId, DraftText, FileChange, Forge,
    ForgeHost, ForgeKind, Host, Linked, LinkedFile, Login, PictureSource, PrNumber, Prose,
    PullRequest, RefName, RepoSlug, Rights, SignedPicture, Token, UnixTime, WebLink,
};
use io_process::{Argument, Captured, Process, ProcessError, Program};
use io_vcs::RemoteUrl;
use serde_json::Value;

use crate::wire::{
    Api, Delivery, EnvVar, PendingNote, Request, WireDiffRefs, WireResponse, WireReview, WireUser,
};

pub use crate::login::{
    Cancel, DeviceAuthorization, LoginMethod, MissingClient, authorize_device, built_in_client,
    login_method, refresh,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenSource {
    Environment,
    Keychain,
    GhCli,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OAuthErrorCode(String);

impl OAuthErrorCode {
    pub fn new(text: &str) -> Self {
        Self(text.to_owned())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for OAuthErrorCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

struct Sourced {
    token: Token,
    source: TokenSource,
}

impl Sourced {
    const fn new(token: Token, source: TokenSource) -> Self {
        Self { token, source }
    }
}

fn refusal(forge: &Forge, status: HttpStatus, body: ResponseBody) -> ForgeError {
    if status == HttpStatus::FORBIDDEN
        && forge.kind() == ForgeKind::GitHub
        && wire::integration_refused(body.as_str())
    {
        ForgeError::NotPermitted {
            slug: forge.slug().clone(),
            body,
        }
    } else {
        ForgeError::Http { status, body }
    }
}

fn now() -> UnixTime {
    UnixTime::new(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_secs()),
    )
}

fn curl(config: &Captured, host: &Host) -> Result<WireResponse, ForgeError> {
    let arguments: Vec<Argument> = wire::curl_arguments()
        .iter()
        .map(|argument| Argument::new(argument))
        .collect();
    let output = Process::run_with_input(
        &Program::new(wire::CURL),
        &arguments,
        &env::temp_dir(),
        config,
        wire::REQUEST_DEADLINE,
    )
    .map_err(|error| match error {
        ProcessError::TimedOut { .. } => ForgeError::TimedOut(host.clone()),
        other => ForgeError::Process(other),
    })?;
    if output.status.code() == Some(wire::CURL_EXIT_TIMED_OUT) {
        return Err(ForgeError::TimedOut(host.clone()));
    }
    if !output.success() {
        return Err(ForgeError::Unreachable(output.stderr));
    }
    let stdout = output.stdout.text();
    wire::parse_response(&stdout).ok_or_else(|| ForgeError::Malformed(ResponseBody::new(&stdout)))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CommentCount(usize);

impl CommentCount {
    pub const fn new(count: usize) -> Self {
        Self(count)
    }

    pub const fn get(self) -> usize {
        self.0
    }
}

impl fmt::Display for CommentCount {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HttpStatus(u16);

impl HttpStatus {
    pub const UNAUTHORIZED: Self = Self(401);
    pub const FORBIDDEN: Self = Self(403);

    pub const fn new(code: u16) -> Self {
        Self(code)
    }

    pub const fn get(self) -> u16 {
        self.0
    }

    pub const fn success(self) -> bool {
        self.0 >= 200 && self.0 < 300
    }
}

impl fmt::Display for HttpStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResponseBody(String);

impl ResponseBody {
    pub fn new(text: &str) -> Self {
        Self(text.to_owned())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ResponseBody {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0.trim())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Submitted {
    pub comments: CommentCount,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unsent {
    Drafts,
    Approval,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenFor {
    Submitting,
    ReviewRequests,
}

#[derive(Debug)]
pub enum ForgeError {
    UnknownHost(Host),
    UnreadableUrl(RemoteUrl),
    NoToken(ForgeHost, TokenFor),
    Process(ProcessError),
    Unreachable(Captured),
    TimedOut(Host),
    Http {
        status: HttpStatus,
        body: ResponseBody,
    },
    Malformed(ResponseBody),
    Unfinished {
        sent: Vec<DraftId>,
        unsent: Unsent,
        cause: Box<ForgeError>,
    },
    PendingLeft {
        pending: CommentCount,
        cause: Box<ForgeError>,
    },
    NotPermitted {
        slug: RepoSlug,
        body: ResponseBody,
    },
    NoDeviceLogin(ForgeHost),
    LoginDenied(ForgeHost),
    LoginExpired(ForgeHost),
    LoginCancelled(ForgeHost),
    LoginRefused {
        site: ForgeHost,
        code: OAuthErrorCode,
        description: ResponseBody,
    },
}

impl ForgeError {
    pub const fn refused_token(&self) -> bool {
        matches!(
            self,
            Self::Http {
                status: HttpStatus::UNAUTHORIZED,
                ..
            }
        )
    }

    pub fn sent(&self) -> &[DraftId] {
        match self {
            Self::Unfinished { sent, .. } => sent,
            Self::UnknownHost(_)
            | Self::UnreadableUrl(_)
            | Self::NoToken(..)
            | Self::Process(_)
            | Self::Unreachable(_)
            | Self::TimedOut(_)
            | Self::Http { .. }
            | Self::Malformed(_)
            | Self::PendingLeft { .. }
            | Self::NotPermitted { .. }
            | Self::NoDeviceLogin(_)
            | Self::LoginDenied(_)
            | Self::LoginExpired(_)
            | Self::LoginCancelled(_)
            | Self::LoginRefused { .. } => &[],
        }
    }
}

fn no_token(formatter: &mut fmt::Formatter<'_>, site: &ForgeHost, wanted: TokenFor) -> fmt::Result {
    match wanted {
        TokenFor::Submitting => write!(formatter, "submitting to {site} needs a login")?,
        TokenFor::ReviewRequests => {
            write!(formatter, "log in to see the review requests on {site}")?;
        }
    }
    let kind = site.kind();
    let names: Vec<&str> = EnvVar::tokens(kind)
        .iter()
        .map(|variable| variable.name())
        .collect();
    write!(
        formatter,
        ": `review login {site}`, or set {}",
        names.join(" or ")
    )?;
    if kind == ForgeKind::GitHub {
        write!(formatter, ", or `{} auth login`", wire::GH)?;
    }
    Ok(())
}

impl fmt::Display for ForgeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownHost(host) => write!(
                formatter,
                "cannot tell which forge {host} runs: set {}=github|gitlab|forgejo",
                EnvVar::ReviewForge.name()
            ),
            Self::UnreadableUrl(url) => write!(
                formatter,
                "cannot read a host and repository from the remote URL {}",
                url.as_str()
            ),
            Self::NoToken(site, wanted) => no_token(formatter, site, *wanted),
            Self::Process(error) => write!(formatter, "{error}"),
            Self::TimedOut(host) => write!(
                formatter,
                "{host} did not answer in time ({} gives up after {} connecting or {} on one request): the forge or the network is slow or down; refresh to try again",
                wire::CURL,
                wire::CONNECT_DEADLINE,
                wire::REQUEST_DEADLINE
            ),
            Self::Unreachable(stderr) => write!(
                formatter,
                "{} could not reach the forge: {}",
                wire::CURL,
                stderr.text().trim()
            ),
            Self::Http { status, body } => {
                write!(formatter, "the forge answered HTTP {status}: {body}")
            }
            Self::Malformed(body) => write!(
                formatter,
                "the forge answered with something that is not the expected JSON: {body}"
            ),
            Self::PendingLeft { pending, cause } => write!(
                formatter,
                "{cause}. Nothing was published, but {pending} pending notes could not be removed from the merge request and would go out with the next submit: delete them in the forge's web page first"
            ),
            Self::Unfinished {
                sent,
                unsent: Unsent::Drafts,
                cause,
            } => write!(
                formatter,
                "the forge took the review with {} of the drafts, then answered: {cause}. The drafts it took are no longer drafts; submit again, without a summary or decision, to send the others",
                sent.len()
            ),
            Self::Unfinished {
                unsent: Unsent::Approval,
                cause,
                ..
            } => write!(
                formatter,
                "the forge took every draft and the summary, then refused the approval: {cause}. Approve again, without a summary"
            ),
            Self::NotPermitted { slug, body } => write!(
                formatter,
                "the GitHub App this tool logs in through may not do that on {slug}: install the App on that repository, or grant it the permission it lacks, in its settings on GitHub; public reads keep working without it (the forge said: {body})"
            ),
            Self::NoDeviceLogin(site) => write!(
                formatter,
                "{} on {site} has no device login: paste a token instead",
                site.kind()
            ),
            Self::LoginDenied(site) => {
                write!(formatter, "the login to {site} was denied in the browser")
            }
            Self::LoginExpired(site) => write!(
                formatter,
                "the code for {site} expired before it was entered: log in again"
            ),
            Self::LoginCancelled(site) => write!(formatter, "the login to {site} was cancelled"),
            Self::LoginRefused {
                site,
                code,
                description,
            } => write!(
                formatter,
                "{site} refused the login ({code}): {description}"
            ),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForgeClient {
    forge: Forge,
    token: Option<Token>,
    source: Option<TokenSource>,
}

fn variable(name: EnvVar) -> Option<Token> {
    Token::new(&env::var(name.name()).ok()?)
}

fn gh_token(host: &Host) -> Option<Token> {
    let arguments: Vec<Argument> = wire::gh_token_arguments(host.as_str())
        .iter()
        .map(|argument| Argument::new(argument))
        .collect();
    let output = Process::run(
        &Program::new(wire::GH),
        &arguments,
        &env::temp_dir(),
        wire::GH_DEADLINE,
    )
    .ok()?;
    output
        .success()
        .then(|| Token::new(&output.stdout.text()))
        .flatten()
}

fn token(forge: &Forge, stored: impl FnOnce(&Forge) -> Option<Token>) -> Option<Sourced> {
    let kind = forge.kind();
    EnvVar::tokens(kind)
        .iter()
        .find_map(|name| variable(*name))
        .map(|token| Sourced::new(token, TokenSource::Environment))
        .or_else(|| stored(forge).map(|token| Sourced::new(token, TokenSource::Keychain)))
        .or_else(|| {
            (kind == ForgeKind::GitHub)
                .then(|| gh_token(forge.host()))
                .flatten()
                .map(|token| Sourced::new(token, TokenSource::GhCli))
        })
}

pub fn forge_host_of(host: &Host) -> Result<ForgeHost, ForgeError> {
    let chosen = env::var(EnvVar::ReviewForge.name())
        .ok()
        .and_then(|name| wire::forge_named(&name));
    chosen
        .or_else(|| wire::detect(host.as_str()))
        .map(|kind| ForgeHost::new(kind, host.clone()))
        .ok_or_else(|| ForgeError::UnknownHost(host.clone()))
}

fn every_page(
    api: &Api,
    first: &Request,
    mut request: impl FnMut(&Request) -> Result<WireResponse, ForgeError>,
) -> Result<Vec<Value>, ForgeError> {
    let mut items = Vec::new();
    let mut requested = HashSet::new();
    let mut upcoming = Some(first.clone());
    while let Some(page) = upcoming.take() {
        if !requested.insert(page.url.clone()) {
            break;
        }
        let response = request(&page)?;
        items.extend(
            wire::parse_items(&response.body)
                .ok_or_else(|| ForgeError::Malformed(ResponseBody::new(&response.body)))?,
        );
        upcoming = response
            .next_url
            .as_deref()
            .and_then(|url| api.next_request(url));
    }
    Ok(items)
}

pub fn forge_of(url: &RemoteUrl) -> Result<Forge, ForgeError> {
    let unreadable = || ForgeError::UnreadableUrl(url.clone());
    let remote = wire::parse_remote(url.as_str()).ok_or_else(unreadable)?;
    let host = Host::new(&remote.host).ok_or_else(unreadable)?;
    let slug = RepoSlug::new(&remote.path).ok_or_else(unreadable)?;
    let site = forge_host_of(&host)?;
    Ok(Forge::new(site.kind(), host, slug))
}

pub fn web_link(forge: &Forge, linked: &Linked) -> Option<WebLink> {
    WebLink::new(&wire::web_url(
        forge.kind(),
        forge.host().as_str(),
        forge.slug().as_str(),
        linked,
    ))
}

pub fn linked_file(forge: &Forge, link: &WebLink, branches: &[BranchName]) -> Option<LinkedFile> {
    let names: Vec<&str> = branches.iter().map(BranchName::as_str).collect();
    wire::linked_file(
        forge.kind(),
        forge.host().as_str(),
        forge.slug().as_str(),
        link.as_str(),
        &names,
    )
}

impl ForgeClient {
    pub fn connect(
        url: &RemoteUrl,
        stored: impl FnOnce(&Forge) -> Option<Token>,
    ) -> Result<Self, ForgeError> {
        let forge = forge_of(url)?;
        let sourced = token(&forge, stored);
        Ok(Self {
            forge,
            source: sourced.as_ref().map(|sourced| sourced.source),
            token: sourced.map(|sourced| sourced.token),
        })
    }

    pub fn forge(&self) -> &Forge {
        &self.forge
    }

    pub const fn token_source(&self) -> Option<TokenSource> {
        self.source
    }

    pub fn head_ref(&self, number: PrNumber) -> RefName {
        RefName::new(&wire::head_ref(self.forge.kind(), number.get()))
    }

    fn api(&self) -> Api {
        Api::new(
            self.forge.kind(),
            self.forge.host().as_str(),
            self.forge.slug().as_str(),
        )
    }

    fn call(&self, api: &Api, request: &Request) -> Result<ResponseBody, ForgeError> {
        self.exchange(api, request)
            .map(|response| ResponseBody::new(&response.body))
    }

    fn exchange(&self, api: &Api, request: &Request) -> Result<WireResponse, ForgeError> {
        let config = api.config(request, self.token.as_ref().map(Token::as_str));
        let response = curl(&Captured::of(config.as_bytes()), self.forge.host())?;
        let status = HttpStatus::new(response.status);
        if status.success() {
            Ok(response)
        } else {
            Err(refusal(
                &self.forge,
                status,
                ResponseBody::new(&response.body),
            ))
        }
    }

    fn read_pages<T>(
        &self,
        api: &Api,
        request: &Request,
        parse: impl FnOnce(&[Value]) -> Option<T>,
    ) -> Result<T, ForgeError> {
        let items = every_page(api, request, |page| self.exchange(api, page))?;
        match parse(&items) {
            Some(parsed) => Ok(parsed),
            None => Err(ForgeError::Malformed(ResponseBody::new(
                &Value::Array(items).to_string(),
            ))),
        }
    }

    fn read<T>(
        &self,
        api: &Api,
        request: &Request,
        parse: impl FnOnce(&ResponseBody) -> Option<T>,
    ) -> Result<T, ForgeError> {
        let body = self.call(api, request)?;
        parse(&body).ok_or(ForgeError::Malformed(body))
    }

    fn logged_in(&self, wanted: TokenFor) -> Result<(), ForgeError> {
        match self.token {
            Some(_) => Ok(()),
            None => Err(ForgeError::NoToken(self.forge.forge_host(), wanted)),
        }
    }

    fn user(&self, api: &Api) -> Result<WireUser, ForgeError> {
        self.read(api, &api.user(), |body| api.parse_user(body.as_str()))
    }

    pub fn open_pulls(&self) -> Result<Vec<PullRequest>, ForgeError> {
        let api = self.api();
        self.read_pages(&api, &api.list(), |items| {
            api.parse_pulls(items)
                .and_then(|pulls| pulls.iter().map(convert::pull).collect())
        })
    }

    pub fn requested(&self) -> Result<Vec<PrNumber>, ForgeError> {
        self.logged_in(TokenFor::ReviewRequests)?;
        let api = self.api();
        let request = match self.forge.kind() {
            ForgeKind::GitHub => api.github_requested(),
            ForgeKind::Forgejo => api.forgejo_requested(),
            ForgeKind::GitLab => api.gitlab_requested(&self.user(&api)?),
        };
        self.read_pages(&api, &request, |items| {
            api.parse_requested(items)?
                .into_iter()
                .map(PrNumber::new)
                .collect()
        })
    }

    pub fn pull(&self, number: PrNumber) -> Result<PullRequest, ForgeError> {
        let api = self.api();
        self.read(&api, &api.pull(number.get()), |body| {
            api.parse_pull(body.as_str())
                .and_then(|pull| convert::pull(&pull))
        })
    }

    pub fn conversation(&self, pull: &PullRequest) -> Result<Conversation, ForgeError> {
        let api = self.api();
        let number = pull.number.get();
        let mut signed = Vec::new();
        let mut submissions = Vec::new();
        let threads = match self.forge.kind() {
            ForgeKind::GitLab => self.read_pages(&api, &api.discussions(number), |items| {
                wire::parse_discussions(items, pull.head_commit.as_str())
            })?,
            ForgeKind::GitHub => {
                let (mut threads, issue_signed) =
                    self.read_pages(&api, &api.issue_comments(number), |items| {
                        Some((
                            wire::parse_issue_comments(items)?,
                            wire::signed_in_page(items),
                        ))
                    })?;
                signed.extend(issue_signed);
                let (reviews, review_signed) =
                    self.read_pages(&api, &api.reviews(number), |items| {
                        Some((wire::parse_reviews(items)?, wire::signed_in_page(items)))
                    })?;
                submissions = reviews.submissions;
                signed.extend(review_signed);
                let (code_threads, code_signed) =
                    self.read_pages(&api, &api.review_comments(number), |items| {
                        Some((
                            wire::parse_github_code_threads(items)?,
                            wire::signed_in_page(items),
                        ))
                    })?;
                threads.extend(code_threads);
                signed.extend(code_signed);
                signed.extend(
                    self.read(&api, &api.pull(number), |body| {
                        serde_json::from_str::<Value>(body.as_str())
                            .ok()
                            .map(|rendered| wire::signed_in_item(&rendered))
                    })
                    .unwrap_or_default(),
                );
                threads
            }
            ForgeKind::Forgejo => {
                let mut threads = self.read_pages(
                    &api,
                    &api.issue_comments(number),
                    wire::parse_issue_comments,
                )?;
                let reviews = self.read_pages(&api, &api.reviews(number), wire::parse_reviews)?;
                submissions = reviews.submissions;
                let mut comments = Vec::new();
                for review in reviews.with_comments {
                    comments.extend(self.read_pages(
                        &api,
                        &api.forgejo_review_comments(number, review),
                        wire::parse_forgejo_comments,
                    )?);
                }
                threads.extend(wire::forgejo_threads(comments));
                threads
            }
        };
        let pull_url = wire::web_url(
            self.forge.kind(),
            self.forge.host().as_str(),
            self.forge.slug().as_str(),
            &Linked::Pull(pull.number),
        );
        let mut threads = wire::anchored(threads, &pull_url);
        let mut description = pull.description.clone();
        if self.forge.kind() == ForgeKind::GitLab {
            for note in threads
                .iter_mut()
                .flat_map(|thread| thread.notes.iter_mut())
            {
                note.body = api.absolute_uploads(&note.body);
            }
            description = Prose::new(&api.absolute_uploads(description.as_str()));
        }
        convert::conversation(&description, &submissions, &threads, &signed)
            .ok_or_else(|| ForgeError::Malformed(ResponseBody::default()))
    }

    pub fn picture_source(&self, link: &WebLink, signed: &[SignedPicture]) -> PictureSource {
        let api = self.api();
        if let (Some(token), Some(from)) = (
            self.token.as_ref(),
            api.picture_behind_token(link.as_str())
                .and_then(|url| WebLink::new(&url)),
        ) {
            return PictureSource {
                from,
                credential: Some(Credential::new(&api.authorization(token.as_str()))),
            };
        }
        signed
            .iter()
            .find(|picture| &picture.written == link)
            .map_or_else(
                || PictureSource::anonymous(link),
                |picture| PictureSource::anonymous(&picture.signed),
            )
    }

    pub fn submit(
        &self,
        pull: &PullRequest,
        drafts: &[Draft],
        changes: &[FileChange],
        decision: Decision,
        body: Option<&DraftText>,
    ) -> Result<Submitted, ForgeError> {
        let kind = self.forge.kind();
        self.logged_in(TokenFor::Submitting)?;
        let api = self.api();
        let number = pull.number.get();
        let review = convert::review(pull, drafts, changes, decision, body);
        match kind {
            ForgeKind::GitHub => {
                send_in_turn(&api.github_deliveries(number, &review), |request| {
                    self.call(&api, request)
                })?;
            }
            ForgeKind::Forgejo => {
                self.call(&api, &api.forgejo_review(number, &review))?;
            }
            ForgeKind::GitLab => {
                let merge_request = self.call(&api, &api.pull(number))?;
                let refs = Api::parse_diff_refs(merge_request.as_str())
                    .ok_or(ForgeError::Malformed(merge_request))?;
                send_gitlab(&api, pull.number, &review, &refs, |request| {
                    self.call(&api, request)
                })?;
            }
        }
        Ok(Submitted {
            comments: CommentCount::new(drafts.len()),
        })
    }

    pub fn rights(&self, pull: &PullRequest) -> Result<Rights, ForgeError> {
        self.logged_in(TokenFor::Submitting)?;
        let api = self.api();
        match self.forge.kind() {
            ForgeKind::GitHub | ForgeKind::Forgejo => {
                let me = Login::new(&self.user(&api)?.login);
                Ok(convert::rights(pull, Some(&me), None))
            }
            ForgeKind::GitLab => {
                let approvals =
                    self.read(&api, &api.gitlab_approvals(pull.number.get()), |body| {
                        Api::parse_approvals(body.as_str())
                    })?;
                Ok(convert::rights(pull, None, Some(approvals)))
            }
        }
    }
}

fn send_in_turn(
    deliveries: &[Delivery],
    mut call: impl FnMut(&Request) -> Result<ResponseBody, ForgeError>,
) -> Result<(), ForgeError> {
    let mut sent: Vec<DraftId> = Vec::new();
    for (index, delivery) in deliveries.iter().enumerate() {
        if let Err(cause) = call(&delivery.request) {
            return Err(if index == 0 {
                cause
            } else {
                ForgeError::Unfinished {
                    sent,
                    unsent: Unsent::Drafts,
                    cause: Box::new(cause),
                }
            });
        }
        sent.extend(delivery.drafts.iter().copied());
    }
    Ok(())
}

fn send_gitlab(
    api: &Api,
    pull: PrNumber,
    review: &WireReview,
    refs: &WireDiffRefs,
    mut call: impl FnMut(&Request) -> Result<ResponseBody, ForgeError>,
) -> Result<(), ForgeError> {
    let number = pull.get();
    let notes = api.gitlab_pending_notes(number, review, refs);
    let mut pending: Vec<PendingNote> = Vec::new();
    let mut post_and_publish = || -> Result<(), ForgeError> {
        for note in &notes {
            let answer = call(note)?;
            pending.push(
                Api::parse_pending_note(answer.as_str()).ok_or(ForgeError::Malformed(answer))?,
            );
        }
        if !notes.is_empty() {
            call(&api.gitlab_publish(number))?;
        }
        Ok(())
    };
    if let Err(cause) = post_and_publish() {
        let left = pending
            .into_iter()
            .filter(|note| call(&api.gitlab_discard(number, *note)).is_err())
            .count();
        return Err(if left == 0 {
            cause
        } else {
            ForgeError::PendingLeft {
                pending: CommentCount::new(left),
                cause: Box::new(cause),
            }
        });
    }
    if review.decision == Decision::Approve
        && let Err(cause) = call(&api.gitlab_approve(number, review))
    {
        return Err(ForgeError::Unfinished {
            sent: review
                .comments
                .iter()
                .map(|comment| comment.draft)
                .collect(),
            unsent: Unsent::Approval,
            cause: Box::new(cause),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests;

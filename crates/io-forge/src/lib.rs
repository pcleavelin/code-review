mod convert;
mod wire;

use std::collections::HashSet;
use std::env;
use std::fmt;

use domain::{
    Conversation, Decision, Draft, DraftText, Forge, ForgeKind, Host, Linked, PrNumber,
    PullRequest, RefName, RepoSlug, WebLink,
};
use io_process::{Argument, Captured, Process, ProcessError, Program};
use io_vcs::RemoteUrl;
use serde_json::Value;

use crate::wire::{Api, EnvVar, Request, WireResponse};

#[derive(Clone, PartialEq, Eq)]
struct Token(String);

impl Token {
    fn new(text: &str) -> Option<Self> {
        let text = text.trim();
        (!text.is_empty()).then(|| Self(text.to_owned()))
    }

    fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Token {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Token(hidden)")
    }
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

#[derive(Debug)]
pub enum ForgeError {
    UnknownHost(Host),
    UnreadableUrl(RemoteUrl),
    NoToken(ForgeKind),
    Process(ProcessError),
    Unreachable(Captured),
    Http {
        status: HttpStatus,
        body: ResponseBody,
    },
    Malformed(ResponseBody),
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
            Self::NoToken(kind) => {
                let names: Vec<&str> = EnvVar::tokens(*kind)
                    .iter()
                    .map(|variable| variable.name())
                    .collect();
                write!(
                    formatter,
                    "submitting to {kind} needs a token: set {}",
                    names.join(" or ")
                )?;
                if *kind == ForgeKind::GitHub {
                    write!(formatter, ", or log in with `{} auth login`", wire::GH)?;
                }
                Ok(())
            }
            Self::Process(ProcessError::Missing(program)) => {
                write!(formatter, "{program} is not on PATH: install it")
            }
            Self::Process(ProcessError::Start { program, error }) => {
                write!(formatter, "cannot run {program}: {error}")
            }
            Self::Process(ProcessError::NoPipe(program)) => {
                write!(formatter, "cannot write to {program}")
            }
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
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForgeClient {
    forge: Forge,
    token: Option<Token>,
}

fn variable(name: EnvVar) -> Option<Token> {
    Token::new(&env::var(name.name()).ok()?)
}

fn gh_token(host: &Host) -> Option<Token> {
    let arguments: Vec<Argument> = wire::gh_token_arguments(host.as_str())
        .iter()
        .map(|argument| Argument::new(argument))
        .collect();
    let output = Process::run(&Program::new(wire::GH), &arguments, &env::temp_dir()).ok()?;
    output
        .success()
        .then(|| Token::new(&output.stdout.text()))
        .flatten()
}

fn token(kind: ForgeKind, host: &Host) -> Option<Token> {
    EnvVar::tokens(kind)
        .iter()
        .find_map(|name| variable(*name))
        .or_else(|| {
            (kind == ForgeKind::GitHub)
                .then(|| gh_token(host))
                .flatten()
        })
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
    let chosen = env::var(EnvVar::ReviewForge.name())
        .ok()
        .and_then(|name| wire::forge_named(&name));
    let kind = chosen
        .or_else(|| wire::detect(host.as_str()))
        .ok_or_else(|| ForgeError::UnknownHost(host.clone()))?;
    Ok(Forge::new(kind, host, slug))
}

pub fn web_link(forge: &Forge, linked: &Linked) -> Option<WebLink> {
    WebLink::new(&wire::web_url(
        forge.kind(),
        forge.host().as_str(),
        forge.slug().as_str(),
        linked,
    ))
}

impl ForgeClient {
    pub fn connect(url: &RemoteUrl) -> Result<Self, ForgeError> {
        let forge = forge_of(url)?;
        let token = token(forge.kind(), forge.host());
        Ok(Self { forge, token })
    }

    pub fn forge(&self) -> &Forge {
        &self.forge
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
        let arguments: Vec<Argument> = wire::curl_arguments()
            .iter()
            .map(|argument| Argument::new(argument))
            .collect();
        let output = Process::run_with_input(
            &Program::new(wire::CURL),
            &arguments,
            &env::temp_dir(),
            &Captured::of(config.as_bytes()),
        )
        .map_err(ForgeError::Process)?;
        if !output.success() {
            return Err(ForgeError::Unreachable(output.stderr));
        }
        let stdout = output.stdout.text();
        let response = wire::parse_response(&stdout)
            .ok_or_else(|| ForgeError::Malformed(ResponseBody::new(&stdout)))?;
        let status = HttpStatus::new(response.status);
        if status.success() {
            Ok(response)
        } else {
            Err(ForgeError::Http {
                status,
                body: ResponseBody::new(&response.body),
            })
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

    pub fn open_pulls(&self) -> Result<Vec<PullRequest>, ForgeError> {
        let api = self.api();
        self.read_pages(&api, &api.list(), |items| {
            api.parse_pulls(items)
                .and_then(|pulls| pulls.iter().map(convert::pull).collect())
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
        let threads = match self.forge.kind() {
            ForgeKind::GitLab => self.read_pages(&api, &api.discussions(number), |items| {
                wire::parse_discussions(items, pull.head_commit.as_str())
            })?,
            ForgeKind::GitHub => {
                let mut threads = self.read_pages(
                    &api,
                    &api.issue_comments(number),
                    wire::parse_issue_comments,
                )?;
                threads.extend(
                    self.read_pages(&api, &api.reviews(number), wire::parse_reviews)?
                        .threads,
                );
                threads.extend(self.read_pages(
                    &api,
                    &api.review_comments(number),
                    wire::parse_github_code_threads,
                )?);
                threads
            }
            ForgeKind::Forgejo => {
                let mut threads = self.read_pages(
                    &api,
                    &api.issue_comments(number),
                    wire::parse_issue_comments,
                )?;
                let reviews = self.read_pages(&api, &api.reviews(number), wire::parse_reviews)?;
                threads.extend(reviews.threads);
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
        let threads = wire::anchored(threads, &pull_url);
        convert::conversation(&pull.description, &threads)
            .ok_or_else(|| ForgeError::Malformed(ResponseBody::default()))
    }

    pub fn submit(
        &self,
        pull: &PullRequest,
        drafts: &[Draft],
        decision: Decision,
        body: Option<&DraftText>,
    ) -> Result<Submitted, ForgeError> {
        let kind = self.forge.kind();
        if self.token.is_none() {
            return Err(ForgeError::NoToken(kind));
        }
        let api = self.api();
        let number = pull.number.get();
        let review = convert::review(pull, drafts, decision, body);
        let requests = match kind {
            ForgeKind::GitHub => api
                .github_review(number, &review)
                .into_iter()
                .chain(api.github_replies(number, &review))
                .collect(),
            ForgeKind::Forgejo => vec![api.forgejo_review(number, &review)],
            ForgeKind::GitLab => {
                let merge_request = self.call(&api, &api.pull(number))?;
                let refs = Api::parse_diff_refs(merge_request.as_str())
                    .ok_or(ForgeError::Malformed(merge_request))?;
                api.gitlab_review(number, &review, &refs)
            }
        };
        for request in &requests {
            self.call(&api, request)?;
        }
        Ok(Submitted {
            comments: CommentCount::new(drafts.len()),
        })
    }
}

#[cfg(test)]
mod tests;

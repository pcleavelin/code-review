mod convert;
mod wire;

use std::fmt;

use domain::{Card, CardField, CardKey, Credential, JiraSite, PictureSource, Token, WebLink};
use io_process::{Argument, Captured, Process, ProcessError, Program};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HttpStatus(u16);

impl HttpStatus {
    const fn new(code: u16) -> Self {
        Self(code)
    }
}

impl fmt::Display for HttpStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

#[derive(Debug)]
pub enum JiraError {
    Process(ProcessError),
    Unreachable(JiraSite, Captured),
    LoggedOut(JiraSite),
    NoCard(CardKey),
    Http(JiraSite, HttpStatus),
    Malformed(JiraSite),
    NotJira(JiraSite),
    Refused(JiraSite),
}

impl fmt::Display for JiraError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Process(error) => write!(formatter, "curl did not run: {error}"),
            Self::Unreachable(site, stderr) => {
                write!(formatter, "{site} did not answer: {}", stderr.text().trim())
            }
            Self::LoggedOut(site) => write!(
                formatter,
                "{site} refused the login: `review login {site}` and paste email:api-token (Jira Cloud) or a personal access token"
            ),
            Self::NoCard(key) => write!(formatter, "no card {key} on Jira, or no right to read it"),
            Self::Http(site, status) => write!(formatter, "{site} answered {status}"),
            Self::Malformed(site) => write!(
                formatter,
                "{site} answered with something that is not a card"
            ),
            Self::Refused(site) => write!(
                formatter,
                "{site} refused the email and token: check both (a Jira Cloud API token is made at id.atlassian.com, a Data Center personal access token in your Jira profile)"
            ),
            Self::NotJira(site) => write!(
                formatter,
                "{site} does not answer like a Jira site: check the address (Jira Cloud sites end in .atlassian.net)"
            ),
        }
    }
}

const UNAUTHORIZED: HttpStatus = HttpStatus::new(401);
const FORBIDDEN: HttpStatus = HttpStatus::new(403);
const NOT_FOUND: HttpStatus = HttpStatus::new(404);
const OK: HttpStatus = HttpStatus::new(200);

fn ask(
    site: &JiraSite,
    token: &Token,
    path: &wire::ApiPath,
) -> Result<wire::WireResponse, JiraError> {
    let config = wire::config(site.host().as_str(), path.as_str(), token.as_str());
    let arguments: Vec<Argument> = wire::CURL_ARGUMENTS
        .iter()
        .map(|argument| Argument::new(argument))
        .collect();
    let output = Process::run_with_input(
        &Program::new(wire::CURL),
        &arguments,
        &io_process::temp_directory(),
        &Captured::of(config.as_bytes()),
        wire::REQUEST_DEADLINE,
    )
    .map_err(JiraError::Process)?;
    if !output.success() {
        return Err(JiraError::Unreachable(site.clone(), output.stderr));
    }
    wire::parse_response(&output.stdout.text()).ok_or_else(|| JiraError::Malformed(site.clone()))
}

pub fn card(site: &JiraSite, token: &Token, key: &CardKey) -> Result<Card, JiraError> {
    let response = ask(site, token, &wire::issue_path(key.as_str()))?;
    match response.status {
        OK => wire::parse_card(&response.body)
            .and_then(|found| convert::card(site, &found))
            .ok_or_else(|| JiraError::Malformed(site.clone())),
        UNAUTHORIZED | FORBIDDEN => Err(JiraError::LoggedOut(site.clone())),
        NOT_FOUND => Err(JiraError::NoCard(key.clone())),
        other => Err(JiraError::Http(site.clone(), other)),
    }
}

pub fn myself(site: &JiraSite, token: &Token) -> Result<CardField, JiraError> {
    let response = ask(site, token, &wire::myself_path())?;
    match response.status {
        OK => wire::parse_name(&response.body)
            .map(|name| CardField::new(&name))
            .ok_or_else(|| JiraError::NotJira(site.clone())),
        UNAUTHORIZED | FORBIDDEN => Err(JiraError::Refused(site.clone())),
        NOT_FOUND => Err(JiraError::NotJira(site.clone())),
        other => Err(JiraError::Http(site.clone(), other)),
    }
}

pub fn picture_source(token: &Token, link: &WebLink) -> PictureSource {
    PictureSource {
        from: link.clone(),
        credential: Some(Credential::new(&wire::authorization(token.as_str()))),
    }
}

#[cfg(test)]
mod tests;

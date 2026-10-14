mod convert;
mod exec;
mod failure;
mod output;
mod wire;

use domain::{JiraSite, Token};
use session::{Cancel, Desk, StartedLogin};

use crate::convert::Request;
use crate::exec::Run;

pub use crate::exec::Touched;
pub use crate::failure::{Failure, Given};
pub use crate::output::Output;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Argument(String);

impl Argument {
    pub fn new(text: &str) -> Self {
        Self(text.to_owned())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandLine(String);

impl CommandLine {
    pub fn new(line: &str) -> Self {
        Self(line.to_owned())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn arguments(&self) -> Vec<Argument> {
        wire::words(&self.0).into_iter().map(Argument).collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    Stdout,
    Stderr,
}

#[derive(Debug)]
pub struct ParseFailure(clap::Error);

impl ParseFailure {
    pub fn text(&self) -> Output {
        wire::render_error(&self.0)
    }

    pub fn channel(&self) -> Channel {
        if self.0.use_stderr() {
            Channel::Stderr
        } else {
            Channel::Stdout
        }
    }
}

#[derive(Debug)]
pub struct Invocation {
    request: Result<Request, Failure>,
}

impl Invocation {
    pub fn parse(arguments: &[Argument]) -> Result<Self, ParseFailure> {
        wire::parse(arguments.iter().map(Argument::as_str))
            .map(|command| Self {
                request: convert::request(command),
            })
            .map_err(ParseFailure)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PastedToken(String);

impl PastedToken {
    pub fn new(text: &str) -> Self {
        Self(text.to_owned())
    }
}

pub fn finish_login(
    desk: &Desk,
    started: &StartedLogin,
    pasted: Option<&PastedToken>,
    output: &mut Output,
) -> Result<(), Failure> {
    let site = match started {
        StartedLogin::Device(authorization) => {
            desk.access().finish(authorization, &Cancel::new())?
        }
        StartedLogin::Paste { site, .. } => {
            let token = pasted
                .and_then(|pasted| Token::new(&pasted.0))
                .ok_or_else(|| Failure::NoToken(site.clone()))?;
            desk.access().keep(site, token)?;
            site.clone()
        }
    };
    wire::logged_in(output, &site);
    Ok(())
}

pub fn finish_jira_login(
    desk: &Desk,
    site: &JiraSite,
    pasted: Option<&PastedToken>,
    output: &mut Output,
) -> Result<(), Failure> {
    let token = pasted
        .and_then(|pasted| Token::new(&pasted.0))
        .ok_or_else(|| Failure::NoJiraToken(site.clone()))?;
    let name = desk.access().login_jira(site, token)?;
    wire::jira_logged_in(output, site, &name);
    Ok(())
}

pub fn help() -> Output {
    wire::help()
}

pub fn exec(
    desk: &mut Desk,
    invocation: Invocation,
    output: &mut Output,
) -> Result<Touched, Failure> {
    let request = invocation.request?;
    Run { desk, output }.run(request)
}

#[cfg(test)]
mod tests;

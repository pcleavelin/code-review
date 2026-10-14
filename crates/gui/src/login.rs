use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use domain::{CardField, ForgeHost, JiraSite, Token, UserCode, WebLink};
use io_forge::ForgeError;
use io_process::{Argument, Opener};
use platform::ClipboardRequest;
use session::{
    Access, Cancel, Desk, DeviceAuthorization, Forgotten, Holds, Repo, SessionError, StartedLogin,
};
use ui::{Count, Label};

use crate::app::App;
use crate::field::Which;
use crate::model::Tab;
use crate::runtime::{Job, Landing, landed};
use crate::status::{Status, Tone};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Held {
    In,
    Out,
    TimedOut(Label),
}

pub(crate) enum Stage {
    Device {
        authorization: DeviceAuthorization,
        cancel: Cancel,
    },
    Paste {
        page: Option<WebLink>,
    },
}

pub(crate) struct Login {
    pub(crate) site: ForgeHost,
    pub(crate) stage: Stage,
}

impl Login {
    fn page(&self) -> Option<&WebLink> {
        match &self.stage {
            Stage::Device { authorization, .. } => Some(authorization.opens()),
            Stage::Paste { page } => page.as_ref(),
        }
    }
}

#[derive(Default)]
pub(crate) struct Logins {
    pub(crate) held: BTreeMap<ForgeHost, Held>,
    pub(crate) failed: BTreeMap<ForgeHost, Label>,
    asked: BTreeSet<ForgeHost>,
    pub(crate) login: Option<Login>,
    pub(crate) jira: BTreeMap<JiraSite, Held>,
    pub(crate) jira_form: Option<JiraForm>,
    pub(crate) jira_names: BTreeMap<JiraSite, CardField>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum JiraForm {
    Editing,
    Checking(JiraSite),
    Refused(Label),
}

pub(crate) struct JiraLanded {
    site: JiraSite,
    result: Result<CardField, Label>,
}

pub(crate) struct Holdings {
    pub(crate) forges: BTreeMap<ForgeHost, Held>,
    pub(crate) jira: BTreeMap<JiraSite, Held>,
}

#[derive(Clone, Copy, Debug)]
struct Literal(&'static str);

const JIRA_TOKENS: Literal = Literal("https://id.atlassian.com/manage-profile/security/api-tokens");

fn held_of(holds: Result<Holds, SessionError>) -> Held {
    match holds {
        Ok(Holds::Login) => Held::In,
        Err(error) if error.keychain_timed_out() => Held::TimedOut(Label::new(error.to_string())),
        Ok(Holds::Nothing) | Err(_) => Held::Out,
    }
}

impl Logins {
    pub(crate) fn sites(&self, desk: &Desk) -> BTreeSet<ForgeHost> {
        desk.repos()
            .filter_map(Repo::web)
            .map(|forge| forge.forge_host())
            .chain(self.asked.iter().cloned())
            .collect()
    }

    fn end(&mut self, site: &ForgeHost) {
        if self.login.as_ref().is_some_and(|login| &login.site == site) {
            self.login = None;
        }
    }
}

pub(crate) fn holding(access: &Access, sites: &BTreeSet<ForgeHost>) -> Holdings {
    Holdings {
        forges: sites
            .iter()
            .map(|site| (site.clone(), held_of(access.holds(site))))
            .collect(),
        jira: access
            .jira_sites()
            .into_iter()
            .map(|site| {
                let held = held_of(access.holds_jira(&site));
                (site, held)
            })
            .collect(),
    }
}

pub(crate) enum Ended {
    In(ForgeHost),
    Cancelled(ForgeHost),
    Failed(ForgeHost, Label),
}

fn wait(access: &Access, authorization: &DeviceAuthorization, cancel: &Cancel) -> Ended {
    match access.finish(authorization, cancel) {
        Ok(site) => Ended::In(site),
        Err(SessionError::Forge(ForgeError::LoginCancelled(site))) => Ended::Cancelled(site),
        Err(error) => Ended::Failed(
            authorization.forge_host().clone(),
            Label::new(error.to_string()),
        ),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Browser {
    Open,
    Opened,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum LoginAct {
    Start(ForgeHost),
    CopyCode,
    OpenPage,
    Cancel,
    Save,
    Logout(ForgeHost),
    StartJira(Option<JiraSite>),
    LogoutJira(JiraSite),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum LoginStatus {
    Waiting(ForgeHost),
    Paste(ForgeHost),
    CodeCopied(UserCode),
    PageOpened(WebLink),
    PageFailed(Label),
    NoToken,
    In(ForgeHost),
    Listed(ForgeHost, Count),
    Out(ForgeHost, Forgotten),
    Cancelled(ForgeHost),
    Failed(ForgeHost, Label),
    JiraForm,
    NoJiraSite,
    JiraChecking(JiraSite),
    JiraIn(JiraSite, CardField),
    JiraOut(JiraSite, Forgotten),
    JiraFailed(JiraSite, Label),
}

impl LoginStatus {
    pub(crate) const fn tone(&self) -> Tone {
        match self {
            Self::Waiting(_) | Self::Paste(_) | Self::JiraForm | Self::JiraChecking(_) => {
                Tone::Plain
            }
            Self::CodeCopied(_)
            | Self::PageOpened(_)
            | Self::In(_)
            | Self::Listed(..)
            | Self::Out(..)
            | Self::JiraIn(..)
            | Self::JiraOut(..) => Tone::Done,
            Self::NoToken | Self::NoJiraSite | Self::Cancelled(_) => Tone::Warning,
            Self::PageFailed(_) | Self::Failed(..) | Self::JiraFailed(..) => Tone::Problem,
        }
    }
}

impl fmt::Display for LoginStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Waiting(site) => write!(
                formatter,
                "logging in to {site}: approve the code in the browser"
            ),
            Self::Paste(site) => write!(
                formatter,
                "logging in to {site}: paste an access token in the Inbox and press enter"
            ),
            Self::CodeCopied(code) => write!(formatter, "copied the code {code}"),
            Self::PageOpened(page) => write!(formatter, "opened {page}"),
            Self::PageFailed(why) => {
                write!(formatter, "the browser did not start: {}", why.as_str())
            }
            Self::NoToken => formatter.write_str("paste the token first; cancel stops the login"),
            Self::In(site) => write!(
                formatter,
                "logged in to {site}; the token is in the system keychain"
            ),
            Self::Listed(site, pulls) => write!(
                formatter,
                "logged in to {site}; the token is in the system keychain; {pulls} open pull requests"
            ),
            Self::Out(site, Forgotten::Removed) => write!(
                formatter,
                "logged out of {site}; its token is gone from the keychain"
            ),
            Self::Out(site, Forgotten::Absent) => {
                write!(formatter, "the keychain holds no token for {site}")
            }
            Self::Cancelled(site) => write!(formatter, "the login to {site} was cancelled"),
            Self::Failed(site, why) => {
                write!(formatter, "not logged in to {site}: {}", why.as_str())
            }
            Self::JiraFailed(site, why) => {
                write!(
                    formatter,
                    "not logged in to Jira on {site}: {}",
                    why.as_str()
                )
            }
            Self::JiraForm => formatter.write_str(
                "logging in to Jira: its site, your email and an API token, then log in",
            ),
            Self::NoJiraSite => formatter
                .write_str("type the Jira site's address first, such as yourcompany.atlassian.net"),
            Self::JiraChecking(site) => {
                write!(formatter, "asking {site} who the token belongs to...")
            }
            Self::JiraIn(site, name) => write!(
                formatter,
                "logged in to Jira on {site} as {name}; the token is in the system keychain"
            ),
            Self::JiraOut(site, Forgotten::Removed) => write!(
                formatter,
                "logged out of Jira on {site}; its token is gone from the keychain"
            ),
            Self::JiraOut(site, Forgotten::Absent) => {
                write!(formatter, "the keychain holds no token for {site}")
            }
        }
    }
}

impl App {
    fn say_login(&mut self, status: LoginStatus) {
        self.model.status = Status::Login(status);
    }

    pub(crate) fn login(&mut self, act: LoginAct) {
        let in_form = self.model.review.logins.jira_form.is_some();
        match act {
            LoginAct::Start(site) => self.start_login(&site),
            LoginAct::CopyCode => self.copy_code(),
            LoginAct::OpenPage if in_form => self.open_page(JIRA_TOKENS),
            LoginAct::OpenPage => self.open_login_page(),
            LoginAct::Cancel if in_form => self.end_jira_form(),
            LoginAct::Cancel => self.cancel_login(),
            LoginAct::Save if in_form => self.login_jira(),
            LoginAct::Save => self.save_token(),
            LoginAct::Logout(site) => self.logout(&site),
            LoginAct::StartJira(site) => self.start_jira(site.as_ref()),
            LoginAct::LogoutJira(site) => self.logout_jira(&site),
        }
    }

    fn start_jira(&mut self, site: Option<&JiraSite>) {
        self.stop_login();
        self.model.review.logins.jira_form = Some(JiraForm::Editing);
        let fields = &mut self.model.fields;
        fields.start_empty(Which::Token);
        fields.start_empty(Which::JiraEmail);
        match site {
            Some(site) => {
                fields.start_with(Which::JiraSite, &Label::new(site.to_string()));
                fields.focus(Which::JiraEmail);
            }
            None => fields.start_empty(Which::JiraSite),
        }
        self.model.set_tab(Tab::Inbox);
        self.say_login(LoginStatus::JiraForm);
    }

    fn end_jira_form(&mut self) {
        self.model.review.logins.jira_form = None;
        for which in [Which::JiraSite, Which::JiraEmail, Which::Token] {
            self.model.fields.start_empty(which);
            self.model.fields.release(which);
        }
    }

    fn login_jira(&mut self) {
        if matches!(
            self.model.review.logins.jira_form,
            Some(JiraForm::Checking(_))
        ) {
            return;
        }
        let fields = &self.model.fields;
        let Some(site) = JiraSite::from_address(fields.get(Which::JiraSite).text().as_str()) else {
            self.say_login(LoginStatus::NoJiraSite);
            self.model.fields.focus(Which::JiraSite);
            return;
        };
        let email = fields
            .get(Which::JiraEmail)
            .text()
            .as_str()
            .trim()
            .to_owned();
        let secret = fields.get(Which::Token).text().as_str().trim().to_owned();
        let joined = if email.is_empty() {
            secret.clone()
        } else {
            format!("{email}:{secret}")
        };
        let Some(token) = Token::new(&joined).filter(|_| !secret.is_empty()) else {
            self.say_login(LoginStatus::NoToken);
            self.model.fields.focus(Which::Token);
            return;
        };
        self.model.review.logins.jira_form = Some(JiraForm::Checking(site.clone()));
        self.say_login(LoginStatus::JiraChecking(site.clone()));
        let access = self.model.review.desk.access().clone();
        self.jobs.jira = Some(Job::start(move || JiraLanded {
            result: access
                .login_jira(&site, token)
                .map_err(|error| Label::new(error.to_string())),
            site,
        }));
    }

    pub(crate) fn poll_jira(&mut self) {
        let landed_login = match landed(&mut self.jobs.jira) {
            Landing::Waiting => return,
            Landing::Landed(landed_login) => landed_login,
            Landing::Failed => {
                self.model.review.logins.jira_form = Some(JiraForm::Refused(Label::new(
                    "the thread asking Jira stopped; try again",
                )));
                return;
            }
        };
        let JiraLanded { site, result } = landed_login;
        match result {
            Ok(name) => {
                self.end_jira_form();
                let logins = &mut self.model.review.logins;
                logins.jira.insert(site.clone(), Held::In);
                logins.jira_names.insert(site.clone(), name.clone());
                self.say_login(LoginStatus::JiraIn(site, name));
                self.ask_card();
            }
            Err(why) => {
                self.model.review.logins.jira_form = Some(JiraForm::Refused(why.clone()));
                self.say_login(LoginStatus::JiraFailed(site, why));
            }
        }
    }

    fn logout_jira(&mut self, site: &JiraSite) {
        match self.model.review.desk.access().forget_jira(site) {
            Ok(forgotten) => {
                self.model
                    .review
                    .logins
                    .jira
                    .insert(site.clone(), Held::Out);
                self.say_login(LoginStatus::JiraOut(site.clone(), forgotten));
                self.ask_card();
            }
            Err(error) => self.say_login(LoginStatus::JiraFailed(
                site.clone(),
                Label::new(error.to_string()),
            )),
        }
    }

    fn open_page(&mut self, page: Literal) {
        let Some(page) = WebLink::new(page.0) else {
            return;
        };
        match Opener::open(&Argument::new(page.as_str())) {
            Ok(()) => self.say_login(LoginStatus::PageOpened(page)),
            Err(error) => self.say_login(LoginStatus::PageFailed(Label::new(format!("{error:?}")))),
        }
    }

    fn start_login(&mut self, site: &ForgeHost) {
        let started = self.model.review.desk.access().start(site);
        match started {
            Ok(started) => self.carry_login(started, Browser::Open),
            Err(error) => self.login_failed(site.clone(), Label::new(error.to_string())),
        }
    }

    pub(crate) fn carry_login(&mut self, started: StartedLogin, browser: Browser) {
        self.stop_login();
        self.model.review.logins.jira_form = None;
        let access = self.model.review.desk.access().clone();
        let logins = &mut self.model.review.logins;
        let site = match started {
            StartedLogin::Device(authorization) => {
                let site = authorization.forge_host().clone();
                let cancel = Cancel::new();
                let (waited, stopper) = (authorization.clone(), cancel.clone());
                self.jobs.login = Some(Job::start(move || wait(&access, &waited, &stopper)));
                logins.login = Some(Login {
                    site: site.clone(),
                    stage: Stage::Device {
                        authorization,
                        cancel,
                    },
                });
                site
            }
            StartedLogin::Paste { site, page } => {
                logins.login = Some(Login {
                    site: site.clone(),
                    stage: Stage::Paste { page },
                });
                self.model.fields.start_empty(Which::Token);
                self.model.fields.focus(Which::Token);
                site
            }
        };
        logins.failed.remove(&site);
        logins.asked.insert(site.clone());
        let device = matches!(
            logins.login.as_ref().map(|login| &login.stage),
            Some(Stage::Device { .. })
        );
        self.model.set_tab(Tab::Inbox);
        if device {
            self.say_login(LoginStatus::Waiting(site));
            if browser == Browser::Open {
                self.open_login_page();
            }
        } else {
            self.say_login(LoginStatus::Paste(site));
        }
    }

    fn stop_login(&mut self) {
        if let Some(login) = self.model.review.logins.login.take() {
            match login.stage {
                Stage::Device { cancel, .. } => cancel.cancel(),
                Stage::Paste { .. } => {
                    self.model.fields.start_empty(Which::Token);
                    self.model.fields.release(Which::Token);
                }
            }
        }
        self.jobs.login = None;
    }

    fn copy_code(&mut self) {
        let Some(Login {
            stage: Stage::Device { authorization, .. },
            ..
        }) = &self.model.review.logins.login
        else {
            return;
        };
        let code = authorization.user_code().clone();
        self.clipboard = ClipboardRequest::Copy(Label::new(code.as_str()));
        self.say_login(LoginStatus::CodeCopied(code));
    }

    fn open_login_page(&mut self) {
        let Some(page) = self
            .model
            .review
            .logins
            .login
            .as_ref()
            .and_then(Login::page)
            .cloned()
        else {
            return;
        };
        match Opener::open(&Argument::new(page.as_str())) {
            Ok(()) => {
                if !matches!(self.model.status, Status::Login(LoginStatus::Waiting(_))) {
                    self.say_login(LoginStatus::PageOpened(page));
                }
            }
            Err(error) => self.say_login(LoginStatus::PageFailed(Label::new(format!("{error:?}")))),
        }
    }

    fn cancel_login(&mut self) {
        let Some(site) = self
            .model
            .review
            .logins
            .login
            .as_ref()
            .map(|login| login.site.clone())
        else {
            return;
        };
        self.stop_login();
        self.say_login(LoginStatus::Cancelled(site));
    }

    fn save_token(&mut self) {
        let Some(Login {
            site,
            stage: Stage::Paste { .. },
        }) = &self.model.review.logins.login
        else {
            return;
        };
        let site = site.clone();
        let Some(token) = Token::new(self.model.fields.get(Which::Token).text().as_str()) else {
            self.say_login(LoginStatus::NoToken);
            return;
        };
        let kept = self.model.review.desk.access().keep(&site, token);
        match kept {
            Ok(()) => self.logged_in(site),
            Err(error) => {
                let why = Label::new(error.to_string());
                self.model
                    .review
                    .logins
                    .failed
                    .insert(site.clone(), why.clone());
                self.say_login(LoginStatus::Failed(site, why));
            }
        }
    }

    fn logout(&mut self, site: &ForgeHost) {
        let forgotten = self.model.review.desk.access().forget(site);
        match forgotten {
            Ok(forgotten) => {
                let logins = &mut self.model.review.logins;
                logins.held.insert(site.clone(), Held::Out);
                logins.failed.remove(site);
                self.say_login(LoginStatus::Out(site.clone(), forgotten));
                self.list_pulls();
            }
            Err(error) => self.say_login(LoginStatus::Failed(
                site.clone(),
                Label::new(error.to_string()),
            )),
        }
    }

    fn logged_in(&mut self, site: ForgeHost) {
        let logins = &mut self.model.review.logins;
        logins.end(&site);
        logins.held.insert(site.clone(), Held::In);
        logins.failed.remove(&site);
        self.model.fields.start_empty(Which::Token);
        self.model.fields.release(Which::Token);
        self.say_login(LoginStatus::In(site));
        self.list_pulls();
    }

    fn login_failed(&mut self, site: ForgeHost, why: Label) {
        let logins = &mut self.model.review.logins;
        logins.end(&site);
        logins.failed.insert(site.clone(), why.clone());
        logins.asked.insert(site.clone());
        self.say_login(LoginStatus::Failed(site, why));
    }

    pub(crate) fn poll_login(&mut self) {
        let ended = match landed(&mut self.jobs.login) {
            Landing::Waiting => return,
            Landing::Landed(ended) => ended,
            Landing::Failed => {
                let Some(login) = &self.model.review.logins.login else {
                    return;
                };
                Ended::Failed(login.site.clone(), Label::new("the login thread failed"))
            }
        };
        match ended {
            Ended::In(site) => self.logged_in(site),
            Ended::Cancelled(site) => {
                self.model.review.logins.end(&site);
                self.say_login(LoginStatus::Cancelled(site));
            }
            Ended::Failed(site, why) => self.login_failed(site, why),
        }
    }
}

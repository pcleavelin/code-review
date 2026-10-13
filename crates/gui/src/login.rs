use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use domain::{ForgeHost, Token, UserCode, WebLink};
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

pub(crate) fn holding(access: &Access, sites: &BTreeSet<ForgeHost>) -> BTreeMap<ForgeHost, Held> {
    sites
        .iter()
        .map(|site| {
            let held = match access.holds(site) {
                Ok(Holds::Login) => Held::In,
                Err(error) if error.keychain_timed_out() => {
                    Held::TimedOut(Label::new(error.to_string()))
                }
                Ok(Holds::Nothing) | Err(_) => Held::Out,
            };
            (site.clone(), held)
        })
        .collect()
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
}

impl LoginStatus {
    pub(crate) const fn tone(&self) -> Tone {
        match self {
            Self::Waiting(_) | Self::Paste(_) => Tone::Plain,
            Self::CodeCopied(_)
            | Self::PageOpened(_)
            | Self::In(_)
            | Self::Listed(..)
            | Self::Out(..) => Tone::Done,
            Self::NoToken | Self::Cancelled(_) => Tone::Warning,
            Self::PageFailed(_) | Self::Failed(..) => Tone::Problem,
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
        }
    }
}

impl App {
    fn say_login(&mut self, status: LoginStatus) {
        self.model.status = Status::Login(status);
    }

    pub(crate) fn login(&mut self, act: LoginAct) {
        match act {
            LoginAct::Start(site) => self.start_login(&site),
            LoginAct::CopyCode => self.copy_code(),
            LoginAct::OpenPage => self.open_login_page(),
            LoginAct::Cancel => self.cancel_login(),
            LoginAct::Save => self.save_token(),
            LoginAct::Logout(site) => self.logout(&site),
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

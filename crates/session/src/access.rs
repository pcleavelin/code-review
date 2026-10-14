use std::sync::{Mutex, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use domain::{
    Card, CardField, CardKey, ClientId, Forge, ForgeHost, Grant, Host, JiraSite, PictureSource,
    PullRequest, Token, UnixTime, WebLink,
};
use io_config::{ForgesStore, Reach};
use io_forge::{Cancel, DeviceAuthorization, LoginMethod};
use io_keychain::{Forgotten, Keychain};

use crate::SessionError;
use crate::desk::LoginTarget;

static RENEWING: Mutex<()> = Mutex::new(());

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Holds {
    Login,
    Nothing,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StartedLogin {
    Device(DeviceAuthorization),
    Paste {
        site: ForgeHost,
        page: Option<WebLink>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Access {
    keychain: Option<Keychain>,
    forges: Option<ForgesStore>,
}

fn now() -> UnixTime {
    UnixTime::new(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_secs()),
    )
}

impl Access {
    pub fn find(reach: Reach) -> Self {
        Self {
            keychain: match reach {
                Reach::User => Some(Keychain::user()),
                Reach::OverrideOnly => Keychain::override_only(),
            },
            forges: ForgesStore::find(reach),
        }
    }

    fn keychain(&self) -> Result<&Keychain, SessionError> {
        self.keychain.as_ref().ok_or(SessionError::NoKeychain)
    }

    fn client(&self, site: &ForgeHost) -> Result<ClientId, SessionError> {
        let configured = self
            .forges
            .as_ref()
            .map(ForgesStore::load)
            .unwrap_or_default()
            .into_iter()
            .rev()
            .find(|app| &app.host == site.host())
            .map(|app| app.client);
        match configured {
            Some(client) => Ok(client),
            None => io_forge::built_in_client(site.host()).map_err(|gap| SessionError::NoClient {
                host: site.host().clone(),
                gap,
                file: self.forges.as_ref().map(|store| store.path().to_path_buf()),
            }),
        }
    }

    fn renewed(&self, site: &ForgeHost, grant: &Grant) -> Result<Grant, SessionError> {
        let Some(renewal) = &grant.renewal else {
            return Ok(grant.clone());
        };
        let renewed = io_forge::refresh(site, renewal).map_err(|cause| SessionError::Renewal {
            site: site.clone(),
            cause,
        })?;
        self.keychain()?
            .save(site.host(), &renewed)
            .map_err(SessionError::Keychain)?;
        Ok(renewed)
    }

    pub fn token(&self, forge: &Forge) -> Result<Option<Token>, SessionError> {
        let Some(keychain) = self.keychain.as_ref() else {
            return Ok(None);
        };
        let site = forge.forge_host();
        let Some(grant) = keychain.load(site.host()).map_err(SessionError::Keychain)? else {
            return Ok(None);
        };
        if !grant.expired(now()) {
            return Ok(Some(grant.token));
        }
        let _renewing = RENEWING.lock().unwrap_or_else(PoisonError::into_inner);
        let current = keychain
            .load(site.host())
            .map_err(SessionError::Keychain)?
            .unwrap_or(grant);
        if !current.expired(now()) {
            return Ok(Some(current.token));
        }
        Ok(Some(self.renewed(&site, &current)?.token))
    }

    pub fn renew(&self, site: &ForgeHost) -> Result<Option<Token>, SessionError> {
        let keychain = self.keychain()?;
        let _renewing = RENEWING.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(grant) = keychain.load(site.host()).map_err(SessionError::Keychain)? else {
            return Ok(None);
        };
        if grant.renewal.is_none() {
            return Ok(None);
        }
        Ok(Some(self.renewed(site, &grant)?.token))
    }

    pub fn holds(&self, site: &ForgeHost) -> Result<Holds, SessionError> {
        self.holds_for(site.host())
    }

    pub fn holds_jira(&self, site: &JiraSite) -> Result<Holds, SessionError> {
        self.holds_for(site.host())
    }

    fn holds_for(&self, host: &Host) -> Result<Holds, SessionError> {
        let Some(keychain) = self.keychain.as_ref() else {
            return Ok(Holds::Nothing);
        };
        keychain
            .load(host)
            .map(|grant| grant.map_or(Holds::Nothing, |_| Holds::Login))
            .map_err(SessionError::Keychain)
    }

    pub fn start(&self, site: &ForgeHost) -> Result<StartedLogin, SessionError> {
        self.keychain()?;
        match io_forge::login_method(site) {
            LoginMethod::Paste(page) => Ok(StartedLogin::Paste {
                site: site.clone(),
                page,
            }),
            LoginMethod::Device => {
                let client = self.client(site)?;
                Ok(StartedLogin::Device(io_forge::authorize_device(
                    site, &client,
                )?))
            }
        }
    }

    pub fn finish(
        &self,
        authorization: &DeviceAuthorization,
        cancel: &Cancel,
    ) -> Result<ForgeHost, SessionError> {
        let grant = authorization.wait(cancel)?;
        let site = authorization.forge_host().clone();
        self.keychain()?
            .save(site.host(), &grant)
            .map_err(SessionError::Keychain)?;
        Ok(site)
    }

    pub fn jira_sites(&self) -> Vec<JiraSite> {
        self.forges
            .as_ref()
            .map(ForgesStore::jira_sites)
            .unwrap_or_default()
    }

    pub fn jira_site(&self, target: &LoginTarget) -> Option<JiraSite> {
        let asked = JiraSite::from_address(target.as_str())?;
        self.jira_sites()
            .into_iter()
            .find(|site| site == &asked)
            .or_else(|| asked.is_cloud().then_some(asked))
    }

    pub fn login_jira(&self, site: &JiraSite, token: Token) -> Result<CardField, SessionError> {
        let keychain = self.keychain()?;
        let forges = self.forges.as_ref().ok_or(SessionError::NoForgesFile)?;
        let name = io_jira::myself(site, &token)?;
        forges
            .add_jira(site)
            .map_err(|error| SessionError::Forges(forges.path().to_path_buf(), error))?;
        keychain
            .save(site.host(), &Grant::lasting(token))
            .map_err(SessionError::Keychain)?;
        Ok(name)
    }

    pub fn forget_jira(&self, site: &JiraSite) -> Result<Forgotten, SessionError> {
        self.keychain()?
            .forget(site.host())
            .map_err(SessionError::Keychain)
    }

    pub fn picture_source(&self, link: &WebLink) -> Option<PictureSource> {
        let site = self
            .jira_sites()
            .into_iter()
            .find(|site| site.serves(link))?;
        let grant = self.keychain.as_ref()?.load(site.host()).ok()??;
        Some(io_jira::picture_source(&grant.token, link))
    }

    pub fn card(&self, pull: &PullRequest) -> Result<Option<Card>, SessionError> {
        let Some(key) = CardKey::of_pull(pull) else {
            return Ok(None);
        };
        let Some(site) = self.jira_sites().into_iter().next() else {
            return Ok(None);
        };
        let grant = self
            .keychain()?
            .load(site.host())
            .map_err(SessionError::Keychain)?
            .ok_or_else(|| SessionError::JiraLoggedOut(site.clone()))?;
        Ok(Some(io_jira::card(&site, &grant.token, &key)?))
    }

    pub fn keep(&self, site: &ForgeHost, token: Token) -> Result<(), SessionError> {
        self.keychain()?
            .save(site.host(), &Grant::lasting(token))
            .map_err(SessionError::Keychain)
    }

    pub fn forget(&self, site: &ForgeHost) -> Result<Forgotten, SessionError> {
        self.keychain()?
            .forget(site.host())
            .map_err(SessionError::Keychain)
    }
}

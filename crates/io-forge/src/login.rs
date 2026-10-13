use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use domain::{ClientId, ForgeHost, Grant, Host, Renewal, UserCode, WebLink};

use crate::wire::{self, OAuthError, WireGrant, WirePolled};
use io_process::Captured;

use crate::{ForgeError, HttpStatus, OAuthErrorCode, ResponseBody, convert, curl, now};

const PAUSE_SLICE: Duration = Duration::from_millis(100);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MissingClient {
    BuiltInEmpty,
    NotBuiltIn,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoginMethod {
    Device,
    Paste(Option<WebLink>),
}

#[derive(Clone, Debug, Default)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    pub fn cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

#[derive(Clone, PartialEq, Eq)]
struct DeviceCode(String);

impl fmt::Debug for DeviceCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("DeviceCode(hidden)")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceAuthorization {
    site: ForgeHost,
    client: ClientId,
    device_code: DeviceCode,
    user_code: UserCode,
    verification: WebLink,
    opens: WebLink,
    interval: Duration,
    lifetime: Duration,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Paused {
    Done,
    Stopped,
}

pub fn built_in_client(host: &Host) -> Result<ClientId, MissingClient> {
    let app = wire::BUILT_IN_APPS
        .iter()
        .find(|app| app.host == host.as_str());
    match app {
        Some(app) => ClientId::new(app.client).ok_or(MissingClient::BuiltInEmpty),
        None => Err(MissingClient::NotBuiltIn),
    }
}

impl fmt::Display for MissingClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BuiltInEmpty => write!(
                formatter,
                "its built-in client ID is not filled in yet ({})",
                wire::BUILT_IN_APPS_PLACE
            ),
            Self::NotBuiltIn => formatter.write_str("no client ID is built in for it"),
        }
    }
}

pub fn login_method(site: &ForgeHost) -> LoginMethod {
    match wire::device_url(site.kind(), site.host().as_str()) {
        Some(_) => LoginMethod::Device,
        None => LoginMethod::Paste(WebLink::new(&wire::token_page(
            site.kind(),
            site.host().as_str(),
        ))),
    }
}

struct Answer {
    status: HttpStatus,
    body: ResponseBody,
}

fn post(site: &ForgeHost, request: Option<Captured>) -> Result<Answer, ForgeError> {
    let request = request.ok_or_else(|| ForgeError::NoDeviceLogin(site.clone()))?;
    let response = curl(&request, site.host())?;
    Ok(Answer {
        status: HttpStatus::new(response.status),
        body: ResponseBody::new(&response.body),
    })
}

pub fn authorize_device(
    site: &ForgeHost,
    client: &ClientId,
) -> Result<DeviceAuthorization, ForgeError> {
    let Answer { status, body } = post(
        site,
        wire::device_request(site.kind(), site.host().as_str(), client.as_str()),
    )?;
    if !status.success() {
        return Err(ForgeError::Http { status, body });
    }
    let device =
        wire::parse_device(body.as_str()).ok_or_else(|| ForgeError::Malformed(body.clone()))?;
    let malformed = || ForgeError::Malformed(body.clone());
    let verification = WebLink::new(&device.verification).ok_or_else(malformed)?;
    let opens = match &device.verification_complete {
        Some(complete) => WebLink::new(complete).ok_or_else(malformed)?,
        None => verification.clone(),
    };
    Ok(DeviceAuthorization {
        site: site.clone(),
        client: client.clone(),
        device_code: DeviceCode(device.device_code),
        user_code: UserCode::new(&device.user_code).ok_or_else(malformed)?,
        verification,
        opens,
        interval: Duration::from_secs(device.interval),
        lifetime: Duration::from_secs(device.expires_in),
    })
}

fn polled(status: HttpStatus, body: &ResponseBody) -> Result<WirePolled, ForgeError> {
    match wire::parse_polled(body.as_str()) {
        Some(polled) => Ok(polled),
        None if status.success() => Err(ForgeError::Malformed(body.clone())),
        None => Err(ForgeError::Http {
            status,
            body: body.clone(),
        }),
    }
}

fn wait_for(
    site: &ForgeHost,
    mut interval: Duration,
    lifetime: Duration,
    mut pause: impl FnMut(Duration) -> Paused,
    mut poll: impl FnMut() -> Result<WirePolled, ForgeError>,
) -> Result<WireGrant, ForgeError> {
    let mut waited = Duration::ZERO;
    loop {
        if waited >= lifetime {
            return Err(ForgeError::LoginExpired(site.clone()));
        }
        if pause(interval) == Paused::Stopped {
            return Err(ForgeError::LoginCancelled(site.clone()));
        }
        waited += interval;
        match poll()? {
            WirePolled::Granted(grant) => return Ok(grant),
            WirePolled::Said(OAuthError::Pending, _) => {}
            WirePolled::Said(OAuthError::SlowDown, given) => {
                let stepped = interval + Duration::from_secs(wire::SLOW_DOWN_STEP);
                interval =
                    given.map_or(stepped, |seconds| stepped.max(Duration::from_secs(seconds)));
            }
            WirePolled::Said(OAuthError::Expired, _) => {
                return Err(ForgeError::LoginExpired(site.clone()));
            }
            WirePolled::Said(OAuthError::Denied, _) => {
                return Err(ForgeError::LoginDenied(site.clone()));
            }
            WirePolled::Refused { error, description } => {
                return Err(ForgeError::LoginRefused {
                    site: site.clone(),
                    code: OAuthErrorCode::new(&error),
                    description: ResponseBody::new(&description),
                });
            }
        }
    }
}

fn pause_unless_cancelled(length: Duration, cancel: &Cancel) -> Paused {
    let mut left = length;
    loop {
        if cancel.cancelled() {
            return Paused::Stopped;
        }
        if left.is_zero() {
            return Paused::Done;
        }
        let slice = left.min(PAUSE_SLICE);
        thread::sleep(slice);
        left = left.saturating_sub(slice);
    }
}

impl DeviceAuthorization {
    pub const fn forge_host(&self) -> &ForgeHost {
        &self.site
    }

    pub const fn user_code(&self) -> &UserCode {
        &self.user_code
    }

    pub const fn verification(&self) -> &WebLink {
        &self.verification
    }

    pub const fn opens(&self) -> &WebLink {
        &self.opens
    }

    pub fn wait(&self, cancel: &Cancel) -> Result<Grant, ForgeError> {
        let site = &self.site;
        let granted = wait_for(
            &self.site,
            self.interval,
            self.lifetime,
            |length| pause_unless_cancelled(length, cancel),
            || {
                let answer = post(
                    site,
                    wire::poll_request(
                        site.kind(),
                        site.host().as_str(),
                        self.client.as_str(),
                        &self.device_code.0,
                    ),
                )?;
                polled(answer.status, &answer.body)
            },
        )?;
        convert::grant(&granted, &self.client, now())
            .ok_or_else(|| ForgeError::Malformed(ResponseBody::default()))
    }
}

pub fn refresh(site: &ForgeHost, renewal: &Renewal) -> Result<Grant, ForgeError> {
    let Answer { status, body } = post(
        site,
        wire::refresh_request(
            site.kind(),
            site.host().as_str(),
            renewal.client.as_str(),
            renewal.refresh.as_str(),
        ),
    )?;
    match polled(status, &body)? {
        WirePolled::Granted(granted) => convert::grant(&granted, &renewal.client, now())
            .ok_or_else(|| ForgeError::Malformed(body.clone())),
        WirePolled::Said(OAuthError::Denied, _) => Err(ForgeError::LoginDenied(site.clone())),
        WirePolled::Said(_, _) => Err(ForgeError::LoginExpired(site.clone())),
        WirePolled::Refused { error, description } => Err(ForgeError::LoginRefused {
            site: site.clone(),
            code: OAuthErrorCode::new(&error),
            description: ResponseBody::new(&description),
        }),
    }
}

#[cfg(test)]
mod tests;

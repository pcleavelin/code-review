use std::time::Duration;

use domain::{ClientId, ForgeHost, ForgeKind, Host, UnixTime, WebLink};

use super::{
    Cancel, LoginMethod, MissingClient, Paused, built_in_client, login_method, polled, wait_for,
};
use crate::wire::{self, OAuthError, WireGrant, WirePolled};
use crate::{ForgeError, HttpStatus, ResponseBody, convert};

const GITHUB_DEVICE: &str = r#"{"device_code":"3584d83530557fdd1f46af8289938c8ef79f9dc5","user_code":"WDJB-MJHT","verification_uri":"https://github.com/login/device","expires_in":900,"interval":5}"#;
const GITLAB_DEVICE: &str = r#"{"device_code":"GmRhmhcxhwAzkoEqiMEg_DnyEysNkuNhszIySk9eS","user_code":"0A44L90H","verification_uri":"https://gitlab.com/oauth/device","verification_uri_complete":"https://gitlab.com/oauth/device?user_code=0A44L90H","expires_in":300,"interval":5}"#;
const PENDING: &str = r#"{"error":"authorization_pending","error_description":"The authorization request is still pending.","error_uri":"https://docs.github.com/developers/apps/authorizing-oauth-apps#error-codes-for-the-device-flow"}"#;
const SLOW_DOWN: &str = r#"{"error":"slow_down","error_description":"Too many requests have been made in the same timeframe.","interval":10}"#;
const DENIED: &str =
    r#"{"error":"access_denied","error_description":"The authorization request was denied."}"#;
const EXPIRED: &str =
    r#"{"error":"expired_token","error_description":"The device_code has expired."}"#;
const DISABLED: &str = r#"{"error":"device_flow_disabled","error_description":"Device Flow must be explicitly enabled for this App"}"#;
const GITLAB_GRANT: &str = r#"{"access_token":"de6780bc506a0446309bd9362820ba8aed28aa506c71eedbe1c5c4f9dd350e54","token_type":"Bearer","expires_in":7200,"refresh_token":"8257e65c97202ed1726cf9571600918f3bffb2544b26e00a61df9897668c33a1","scope":"api","created_at":1607635748}"#;
const OAUTH_APP_GRANT: &str = r#"{"access_token":"gho_16C7e42F292c6912E7710c838347Ae178B4a","token_type":"bearer","scope":"repo"}"#;

fn site(kind: ForgeKind, host: &str) -> ForgeHost {
    ForgeHost::new(kind, Host::new(host).unwrap())
}

fn said(text: &str) -> WirePolled {
    wire::parse_polled(text).unwrap()
}

#[test]
fn github_and_gitlab_device_answers_give_the_code_the_page_and_the_timing() {
    let github = wire::parse_device(GITHUB_DEVICE).unwrap();
    assert_eq!(github.user_code, "WDJB-MJHT");
    assert_eq!(github.verification, "https://github.com/login/device");
    assert_eq!(github.verification_complete, None);
    assert_eq!((github.expires_in, github.interval), (900, 5));
    let gitlab = wire::parse_device(GITLAB_DEVICE).unwrap();
    assert_eq!(
        gitlab.verification_complete.as_deref(),
        Some("https://gitlab.com/oauth/device?user_code=0A44L90H")
    );
    let no_interval = wire::parse_device(
        r#"{"device_code":"d","user_code":"U","verification_uri":"https://x/d","expires_in":60}"#,
    )
    .unwrap();
    assert_eq!(no_interval.interval, 5);
    assert_eq!(wire::parse_device(PENDING), None);
}

#[test]
fn each_forge_asks_for_the_device_code_and_the_token_at_its_own_paths() {
    assert_eq!(
        wire::device_url(ForgeKind::GitHub, "github.com").as_deref(),
        Some("https://github.com/login/device/code")
    );
    assert_eq!(
        wire::token_url(ForgeKind::GitHub, "github.com").as_deref(),
        Some("https://github.com/login/oauth/access_token")
    );
    assert_eq!(
        wire::device_url(ForgeKind::GitLab, "gitlab.example.org").as_deref(),
        Some("https://gitlab.example.org/oauth/authorize_device")
    );
    assert_eq!(
        wire::token_url(ForgeKind::GitLab, "gitlab.example.org").as_deref(),
        Some("https://gitlab.example.org/oauth/token")
    );
    assert_eq!(wire::device_url(ForgeKind::Forgejo, "codeberg.org"), None);
}

#[test]
fn forgejo_logs_in_by_pasting_a_token_made_on_its_applications_page() {
    assert_eq!(
        login_method(&site(ForgeKind::Forgejo, "codeberg.org")),
        LoginMethod::Paste(WebLink::new(
            "https://codeberg.org/user/settings/applications"
        ))
    );
    assert_eq!(
        login_method(&site(ForgeKind::GitLab, "gitlab.com")),
        LoginMethod::Device
    );
}

#[test]
fn the_forms_ask_for_the_scope_poll_with_the_device_grant_and_refresh_with_the_refresh_token() {
    assert_eq!(
        wire::device_form(ForgeKind::GitHub, "Iv23client"),
        "client_id=Iv23client"
    );
    assert_eq!(
        wire::device_form(ForgeKind::GitLab, "abc"),
        "client_id=abc&scope=api"
    );
    assert_eq!(
        wire::poll_form("abc", "dev code/+="),
        "client_id=abc&device_code=dev%20code%2F%2B%3D&grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Adevice_code"
    );
    assert_eq!(
        wire::refresh_form("abc", "8257e65c"),
        "client_id=abc&grant_type=refresh_token&refresh_token=8257e65c"
    );
}

#[test]
fn the_device_code_and_refresh_token_reach_curl_only_through_stdin() {
    let config = wire::form_config(
        "https://github.com/login/oauth/access_token",
        &wire::poll_form("abc", "devicesecret"),
    );
    assert_eq!(
        config,
        "url = \"https://github.com/login/oauth/access_token\"\n\
         header = \"Accept: application/json\"\n\
         header = \"Content-Type: application/x-www-form-urlencoded\"\n\
         data-binary = \"client_id=abc&device_code=devicesecret&grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Adevice_code\"\n\
         write-out = \"\\n%header{link}\\n%{http_code}\"\n"
    );
    assert!(
        wire::curl_arguments()
            .iter()
            .all(|argument| !argument.contains("secret"))
    );
}

#[test]
fn every_answer_of_the_token_endpoint_is_read_for_what_it_says() {
    assert_eq!(said(PENDING), WirePolled::Said(OAuthError::Pending, None));
    assert_eq!(
        said(SLOW_DOWN),
        WirePolled::Said(OAuthError::SlowDown, Some(10))
    );
    assert_eq!(said(DENIED), WirePolled::Said(OAuthError::Denied, None));
    assert_eq!(said(EXPIRED), WirePolled::Said(OAuthError::Expired, None));
    assert_eq!(
        said(DISABLED),
        WirePolled::Refused {
            error: "device_flow_disabled".to_owned(),
            description: "Device Flow must be explicitly enabled for this App".to_owned(),
        }
    );
    assert_eq!(
        said(GITLAB_GRANT),
        WirePolled::Granted(WireGrant {
            access: "de6780bc506a0446309bd9362820ba8aed28aa506c71eedbe1c5c4f9dd350e54".to_owned(),
            refresh: Some(
                "8257e65c97202ed1726cf9571600918f3bffb2544b26e00a61df9897668c33a1".to_owned()
            ),
            expires_in: Some(7200),
        })
    );
}

#[test]
fn gitlab_says_pending_with_http_400_and_that_is_still_pending() {
    let body = ResponseBody::new(PENDING);
    assert_eq!(
        polled(HttpStatus::new(400), &body).unwrap(),
        WirePolled::Said(OAuthError::Pending, None)
    );
    match polled(
        HttpStatus::new(502),
        &ResponseBody::new("<html>bad gateway</html>"),
    ) {
        Err(ForgeError::Http { status, .. }) => assert_eq!(status.get(), 502),
        other => panic!("{other:?}"),
    }
}

#[test]
fn an_expiring_grant_keeps_its_refresh_token_and_expiry_and_a_lasting_one_has_neither() {
    let client = ClientId::new("2f1e0d9c").unwrap();
    let WirePolled::Granted(expiring) = said(GITLAB_GRANT) else {
        panic!("not granted");
    };
    let grant = convert::grant(&expiring, &client, UnixTime::new(1000)).unwrap();
    let renewal = grant.renewal.unwrap();
    assert_eq!(renewal.expires, UnixTime::new(8_200));
    assert_eq!(renewal.client, client);
    assert!(renewal.refresh.as_str().starts_with("8257e65c"));
    let WirePolled::Granted(lasting) = said(OAUTH_APP_GRANT) else {
        panic!("not granted");
    };
    let lasting_grant = convert::grant(&lasting, &client, UnixTime::new(1000)).unwrap();
    assert_eq!(lasting_grant.renewal, None);
    assert_eq!(
        lasting_grant.token.as_str(),
        "gho_16C7e42F292c6912E7710c838347Ae178B4a"
    );
}

struct Recorded {
    pauses: Vec<u64>,
    polls: usize,
}

fn run_wait(
    answers: &[&str],
    lifetime: u64,
    stop_after: Option<usize>,
) -> (Result<WireGrant, ForgeError>, Recorded) {
    let mut recorded = Recorded {
        pauses: Vec::new(),
        polls: 0,
    };
    let mut answers = answers.iter();
    let mut pauses = Vec::new();
    let result = wait_for(
        &site(ForgeKind::GitHub, "github.com"),
        Duration::from_secs(5),
        Duration::from_secs(lifetime),
        |length| {
            pauses.push(length.as_secs());
            if stop_after.is_some_and(|after| pauses.len() > after) {
                Paused::Stopped
            } else {
                Paused::Done
            }
        },
        || {
            recorded.polls += 1;
            Ok(said(answers.next().unwrap()))
        },
    );
    recorded.pauses = pauses;
    (result, recorded)
}

#[test]
fn polling_waits_the_interval_before_each_poll_and_slows_down_when_told() {
    let (result, recorded) = run_wait(&[PENDING, SLOW_DOWN, PENDING, GITLAB_GRANT], 900, None);
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(recorded.pauses, [5, 5, 10, 10]);
    assert_eq!(recorded.polls, 4);
    let (_, unspoken) = run_wait(&[r#"{"error":"slow_down"}"#, OAUTH_APP_GRANT], 900, None);
    assert_eq!(unspoken.pauses, [5, 10]);
}

#[test]
fn polling_ends_on_denial_expiry_a_refusal_or_a_cancel() {
    let site = site(ForgeKind::GitHub, "github.com");
    let (denied, _) = run_wait(&[PENDING, DENIED], 900, None);
    assert!(matches!(denied, Err(ForgeError::LoginDenied(ref at)) if *at == site));
    let (expired, _) = run_wait(&[EXPIRED], 900, None);
    assert!(matches!(expired, Err(ForgeError::LoginExpired(_))));
    let (refused, _) = run_wait(&[DISABLED], 900, None);
    assert!(
        matches!(refused, Err(ForgeError::LoginRefused { ref code, .. }) if code.as_str() == "device_flow_disabled")
    );
    let (stopped, recorded) = run_wait(&[PENDING, PENDING, PENDING], 900, Some(2));
    assert!(matches!(stopped, Err(ForgeError::LoginCancelled(_))));
    assert_eq!(recorded.polls, 2);
    let (timed_out, timing) = run_wait(&[PENDING, PENDING, PENDING, PENDING], 10, None);
    assert!(matches!(timed_out, Err(ForgeError::LoginExpired(_))));
    assert_eq!(timing.polls, 2);
}

#[test]
fn a_cancel_ends_a_pause_before_its_time() {
    let cancel = Cancel::new();
    cancel.cancel();
    assert_eq!(
        super::pause_unless_cancelled(Duration::from_secs(3600), &cancel),
        Paused::Stopped
    );
    assert_eq!(
        super::pause_unless_cancelled(Duration::ZERO, &Cancel::new()),
        Paused::Done
    );
}

#[test]
fn a_host_with_a_built_in_slot_is_never_told_it_has_none_and_any_other_host_is() {
    assert_ne!(
        built_in_client(&Host::new("GitHub.com").unwrap()),
        Err(MissingClient::NotBuiltIn)
    );
    assert_eq!(
        built_in_client(&Host::new("gitlab.example.org").unwrap()),
        Err(MissingClient::NotBuiltIn)
    );
}

const GITHUB_APP_GRANT: &str = r#"{"access_token":"ghu_16C7e42F292c6912E7710c838347Ae178B4a","expires_in":28800,"refresh_token":"ghr_1B4a2e77838347a7E420ce178F2E7c6912E169246c34E1ccbF66C46812d16D5B1A9Dc86A1498","refresh_token_expires_in":15897600,"scope":"","token_type":"bearer"}"#;
const GITHUB_APP_LASTING_GRANT: &str = r#"{"access_token":"ghu_16C7e42F292c6912E7710c838347Ae178B4a","scope":"","token_type":"bearer"}"#;

#[test]
fn a_github_app_token_expires_in_eight_hours_with_its_refresh_token_unless_it_comes_alone() {
    let client = ClientId::new("Iv23client").unwrap();
    let WirePolled::Granted(expiring) = said(GITHUB_APP_GRANT) else {
        panic!("not granted");
    };
    let renewal = convert::grant(&expiring, &client, UnixTime::new(1000))
        .unwrap()
        .renewal
        .unwrap();
    assert_eq!(renewal.expires, UnixTime::new(29_800));
    assert!(renewal.refresh.as_str().starts_with("ghr_1B4a"));
    let WirePolled::Granted(lasting) = said(GITHUB_APP_LASTING_GRANT) else {
        panic!("not granted");
    };
    assert_eq!(
        convert::grant(&lasting, &client, UnixTime::new(1000))
            .unwrap()
            .renewal,
        None
    );
}

#[test]
fn github_renews_at_its_token_endpoint_with_the_refresh_token_and_no_secret() {
    let request = wire::refresh_request(ForgeKind::GitHub, "github.com", "Iv23client", "ghr_x")
        .unwrap()
        .text();
    assert_eq!(
        request,
        "url = \"https://github.com/login/oauth/access_token\"\n\
         header = \"Accept: application/json\"\n\
         header = \"Content-Type: application/x-www-form-urlencoded\"\n\
         data-binary = \"client_id=Iv23client&grant_type=refresh_token&refresh_token=ghr_x\"\n\
         write-out = \"\\n%header{link}\\n%{http_code}\"\n"
    );
}

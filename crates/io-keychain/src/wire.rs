use std::path::Path;

use io_process::{Argument, Captured};

pub(crate) const KEYCHAIN_VARIABLE: &str = "REVIEW_KEYCHAIN";
pub(crate) const SERVICE: &str = "code-review";
pub(crate) const SECURITY: &str = "security";
pub(crate) const SECRET_TOOL: &str = "secret-tool";

const SECURITY_ABSENT: i32 = 44;
const SECRET_TOOL_ABSENT: i32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tool {
    Security,
    SecretTool,
}

impl Tool {
    pub(crate) const fn here() -> Option<Self> {
        if cfg!(target_os = "macos") {
            Some(Self::Security)
        } else if cfg!(windows) {
            None
        } else {
            Some(Self::SecretTool)
        }
    }

    pub(crate) const fn program(self) -> &'static str {
        match self {
            Self::Security => SECURITY,
            Self::SecretTool => SECRET_TOOL,
        }
    }

    pub(crate) const fn absent(self, code: Option<i32>) -> bool {
        matches!(
            (self, code),
            (Self::Security, Some(SECURITY_ABSENT)) | (Self::SecretTool, Some(SECRET_TOOL_ABSENT))
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WireRenewal {
    pub(crate) refresh: String,
    pub(crate) expires: u64,
    pub(crate) client: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WireSecret {
    pub(crate) token: String,
    pub(crate) renewal: Option<WireRenewal>,
}

pub(crate) fn print_secret(secret: &WireSecret) -> String {
    match &secret.renewal {
        None => secret.token.clone(),
        Some(renewal) => format!(
            "{} {} {} {}",
            secret.token, renewal.refresh, renewal.expires, renewal.client
        ),
    }
}

pub(crate) fn parse_secret(text: &str) -> Option<WireSecret> {
    let words: Vec<&str> = text.split_whitespace().collect();
    match words.as_slice() {
        [token] => Some(WireSecret {
            token: (*token).to_owned(),
            renewal: None,
        }),
        [token, refresh, expires, client] => Some(WireSecret {
            token: (*token).to_owned(),
            renewal: Some(WireRenewal {
                refresh: (*refresh).to_owned(),
                expires: expires.parse().ok()?,
                client: (*client).to_owned(),
            }),
        }),
        _ => None,
    }
}

fn owned(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| (*word).to_owned()).collect()
}

fn path_text(keychain: Option<&Path>) -> Option<String> {
    keychain.map(|path| path.to_string_lossy().into_owned())
}

fn with_keychain(mut arguments: Vec<String>, keychain: Option<&str>) -> Vec<String> {
    arguments.extend(keychain.map(str::to_owned));
    arguments
}

fn argued(words: &[String]) -> Vec<Argument> {
    words.iter().map(|word| Argument::new(word)).collect()
}

fn quoted(text: &str) -> String {
    let mut out = String::from("\"");
    for character in text.chars() {
        if matches!(character, '"' | '\\') {
            out.push('\\');
        }
        out.push(character);
    }
    out.push('"');
    out
}

fn label(account: &str) -> String {
    format!("{SERVICE} {account}")
}

fn secret_tool_attributes(account: &str, keychain: Option<&str>) -> Vec<String> {
    let mut attributes = owned(&["service", SERVICE, "account", account]);
    if let Some(keychain) = keychain {
        attributes.extend(owned(&["keychain", keychain]));
    }
    attributes
}

pub(crate) fn find_arguments(tool: Tool, account: &str, keychain: Option<&Path>) -> Vec<Argument> {
    let keychain = path_text(keychain);
    let keychain = keychain.as_deref();
    argued(&match tool {
        Tool::Security => with_keychain(
            owned(&["find-generic-password", "-s", SERVICE, "-a", account, "-w"]),
            keychain,
        ),
        Tool::SecretTool => {
            let mut arguments = owned(&["lookup"]);
            arguments.extend(secret_tool_attributes(account, keychain));
            arguments
        }
    })
}

pub(crate) fn delete_arguments(
    tool: Tool,
    account: &str,
    keychain: Option<&Path>,
) -> Vec<Argument> {
    let keychain = path_text(keychain);
    let keychain = keychain.as_deref();
    argued(&match tool {
        Tool::Security => with_keychain(
            owned(&["delete-generic-password", "-s", SERVICE, "-a", account]),
            keychain,
        ),
        Tool::SecretTool => {
            let mut arguments = owned(&["clear"]);
            arguments.extend(secret_tool_attributes(account, keychain));
            arguments
        }
    })
}

pub(crate) struct WireStore {
    pub(crate) arguments: Vec<Argument>,
    pub(crate) input: Captured,
}

pub(crate) fn store(tool: Tool, account: &str, secret: &str, keychain: Option<&Path>) -> WireStore {
    let keychain = path_text(keychain);
    let keychain = keychain.as_deref();
    match tool {
        Tool::Security => {
            let mut command = format!(
                "add-generic-password -U -s {} -a {} -l {} -w {}",
                quoted(SERVICE),
                quoted(account),
                quoted(&label(account)),
                quoted(secret)
            );
            if let Some(keychain) = keychain {
                command.push(' ');
                command.push_str(&quoted(keychain));
            }
            command.push('\n');
            WireStore {
                arguments: argued(&owned(&["-i"])),
                input: Captured::of(command.as_bytes()),
            }
        }
        Tool::SecretTool => {
            let mut arguments = owned(&["store"]);
            arguments.push(format!("--label={}", label(account)));
            arguments.extend(secret_tool_attributes(account, keychain));
            WireStore {
                arguments: argued(&arguments),
                input: Captured::of(secret.as_bytes()),
            }
        }
    }
}

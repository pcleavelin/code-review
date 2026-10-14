mod wiki;

use domain::{CardFamily, CardStage};
use io_process::Deadline;
use serde_json::Value;

use crate::HttpStatus;

pub(crate) const CURL: &str = "curl";
pub(crate) const REQUEST_DEADLINE: Deadline = Deadline::seconds(30);
pub(crate) const CURL_ARGUMENTS: [&str; 4] = ["--silent", "--show-error", "--config", "-"];
const FIELDS: &str = "summary,status,issuetype,assignee,description,attachment";
const WRITE_OUT: &str = "\\n%{http_code}";
const BASIC: char = ':';
const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const SEXTET: u32 = 0b11_1111;
const PAD: char = '=';
const STAGES: [CardStage; 3] = [CardStage::ToDo, CardStage::InProgress, CardStage::Done];
const FAMILIES: [CardFamily; 4] = [
    CardFamily::Bug,
    CardFamily::Story,
    CardFamily::Task,
    CardFamily::Epic,
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WireAttachment {
    pub(crate) filename: String,
    pub(crate) content: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WireCard {
    pub(crate) key: String,
    pub(crate) summary: String,
    pub(crate) status: String,
    pub(crate) stage: CardStage,
    pub(crate) kind: String,
    pub(crate) family: CardFamily,
    pub(crate) assignee: Option<String>,
    pub(crate) description: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WireResponse {
    pub(crate) status: HttpStatus,
    pub(crate) body: String,
}

const fn stage_key(stage: CardStage) -> &'static str {
    match stage {
        CardStage::ToDo => "new",
        CardStage::InProgress => "indeterminate",
        CardStage::Done => "done",
    }
}

const fn family_names(family: CardFamily) -> &'static [&'static str] {
    match family {
        CardFamily::Bug => &["bug", "defect"],
        CardFamily::Story => &["story", "user story"],
        CardFamily::Task => &["task", "sub-task", "subtask", "sub task"],
        CardFamily::Epic => &["epic"],
        CardFamily::Other => &[],
    }
}

fn escaped(value: &str) -> String {
    value
        .chars()
        .filter(|character| !character.is_control())
        .flat_map(|character| match character {
            '"' | '\\' => vec!['\\', character],
            other => vec![other],
        })
        .collect()
}

fn base64(text: &str) -> String {
    let mut encoded = String::new();
    for chunk in text.as_bytes().chunks(3) {
        let group = chunk
            .iter()
            .chain([0, 0].iter())
            .take(3)
            .fold(0_u32, |group, byte| (group << 8) | u32::from(*byte));
        for place in 0..4_u32 {
            let shown = usize::try_from(place).is_ok_and(|place| place <= chunk.len());
            let sextet = (group >> (18 - 6 * place)) & SEXTET;
            let letter = usize::try_from(sextet)
                .ok()
                .and_then(|index| BASE64.get(index))
                .map_or(PAD, |letter| char::from(*letter));
            encoded.push(if shown { letter } else { PAD });
        }
    }
    encoded
}

pub(crate) fn authorization(token: &str) -> String {
    if token.contains(BASIC) {
        format!("Authorization: Basic {}", base64(token))
    } else {
        format!("Authorization: Bearer {token}")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ApiPath(String);

impl ApiPath {
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

pub(crate) fn myself_path() -> ApiPath {
    ApiPath("myself".to_owned())
}

pub(crate) fn issue_path(key: &str) -> ApiPath {
    ApiPath(format!("issue/{key}?fields={FIELDS}"))
}

pub(crate) fn config(host: &str, path: &str, token: &str) -> String {
    let url = format!("https://{host}/rest/api/2/{path}");
    format!(
        "url = \"{}\"\nheader = \"Accept: application/json\"\nheader = \"{}\"\nwrite-out = \"{WRITE_OUT}\"\n",
        escaped(&url),
        escaped(&authorization(token))
    )
}

pub(crate) fn parse_response(stdout: &str) -> Option<WireResponse> {
    let (body, status) = stdout.rsplit_once('\n')?;
    Some(WireResponse {
        status: HttpStatus::new(status.trim().parse().ok()?),
        body: body.to_owned(),
    })
}

fn name_of(fields: &Value, field: &str, inner: &str) -> Option<String> {
    fields.get(field)?.get(inner)?.as_str().map(str::to_owned)
}

fn stage_of(fields: &Value) -> CardStage {
    let key = fields
        .get("status")
        .and_then(|status| status.get("statusCategory"))
        .and_then(|category| category.get("key"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    STAGES
        .into_iter()
        .find(|stage| stage_key(*stage) == key)
        .unwrap_or(CardStage::ToDo)
}

fn family_of(kind: &str, fields: &Value) -> CardFamily {
    let lower = kind.to_lowercase();
    let subtask = fields
        .get("issuetype")
        .and_then(|issue| issue.get("subtask"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    FAMILIES
        .into_iter()
        .find(|family| family_names(*family).contains(&lower.as_str()))
        .unwrap_or(if subtask {
            CardFamily::Task
        } else {
            CardFamily::Other
        })
}

fn attachments_of(fields: &Value) -> Vec<WireAttachment> {
    fields
        .get("attachment")
        .and_then(Value::as_array)
        .map(|attachments| {
            attachments
                .iter()
                .filter_map(|attachment| {
                    Some(WireAttachment {
                        filename: attachment.get("filename")?.as_str()?.to_owned(),
                        content: attachment.get("content")?.as_str()?.to_owned(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn parse_name(body: &str) -> Option<String> {
    let value: Value = serde_json::from_str(body).ok()?;
    value.get("displayName")?.as_str().map(str::to_owned)
}

pub(crate) fn parse_card(body: &str) -> Option<WireCard> {
    let value: Value = serde_json::from_str(body).ok()?;
    let fields = value.get("fields")?;
    let kind = name_of(fields, "issuetype", "name")?;
    let description = fields
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or_default();
    Some(WireCard {
        key: value.get("key")?.as_str()?.to_owned(),
        summary: fields.get("summary")?.as_str()?.to_owned(),
        status: name_of(fields, "status", "name")?,
        stage: stage_of(fields),
        family: family_of(&kind, fields),
        kind,
        assignee: name_of(fields, "assignee", "displayName"),
        description: wiki::markdown(description, &attachments_of(fields)),
    })
}

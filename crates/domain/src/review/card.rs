use std::fmt;

use super::{Host, Prose, PullRequest, Spelling, WebLink};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CardKey(String);

impl CardKey {
    pub fn new(text: &str) -> Option<Self> {
        let (project, number) = text.split_once('-')?;
        let mut letters = project.chars();
        let project_ok = letters
            .next()
            .is_some_and(|first| first.is_ascii_uppercase())
            && letters.all(|rest| rest.is_ascii_uppercase() || rest.is_ascii_digit());
        let number_ok = !number.is_empty() && number.chars().all(|digit| digit.is_ascii_digit());
        (project_ok && number_ok).then(|| Self(text.to_owned()))
    }

    pub fn find_in(text: &str) -> Option<Self> {
        let characters: Vec<char> = text.chars().collect();
        (0..characters.len()).find_map(|start| {
            let starts_word = start == 0
                || characters
                    .get(start - 1)
                    .is_some_and(|before| !before.is_ascii_alphanumeric());
            if !starts_word {
                return None;
            }
            let project = characters
                .iter()
                .skip(start)
                .take_while(|character| {
                    character.is_ascii_uppercase() || character.is_ascii_digit()
                })
                .count();
            let dash = start + project;
            if project == 0 || characters.get(dash) != Some(&'-') {
                return None;
            }
            let digits = characters
                .iter()
                .skip(dash + 1)
                .take_while(|character| character.is_ascii_digit())
                .count();
            let ends_word = characters
                .get(dash + 1 + digits)
                .is_none_or(|after| !after.is_ascii_alphanumeric());
            (digits > 0 && ends_word)
                .then(|| characters.get(start..dash + 1 + digits))
                .flatten()
                .and_then(|key| Self::new(&key.iter().collect::<String>()))
        })
    }

    pub fn of_pull(pull: &PullRequest) -> Option<Self> {
        Self::find_in(pull.title.as_str()).or_else(|| Self::find_in(pull.head.as_str()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CardKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SiteMark {
    Secure,
    Plain,
    Cloud,
}

impl SiteMark {
    const fn spelling(self) -> Spelling {
        Spelling::new(match self {
            Self::Secure => "https://",
            Self::Plain => "http://",
            Self::Cloud => ".atlassian.net",
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct JiraSite(Host);

impl JiraSite {
    pub const fn new(host: Host) -> Self {
        Self(host)
    }

    pub fn from_address(text: &str) -> Option<Self> {
        let text = text.trim();
        let bare = [SiteMark::Secure, SiteMark::Plain]
            .into_iter()
            .find_map(|mark| text.strip_prefix(mark.spelling().as_str()))
            .unwrap_or(text);
        let host = bare.split(['/', '?', '#']).next()?;
        Host::new(host)
            .filter(|host| host.as_str().contains('.'))
            .map(Self)
    }

    pub fn is_cloud(&self) -> bool {
        self.0
            .as_str()
            .ends_with(SiteMark::Cloud.spelling().as_str())
    }

    pub const fn host(&self) -> &Host {
        &self.0
    }

    pub fn card_link(&self, key: &CardKey) -> Option<WebLink> {
        WebLink::new(&format!("https://{}/browse/{key}", self.0.as_str()))
    }

    pub fn serves(&self, link: &WebLink) -> bool {
        let root = format!("https://{}/", self.0.as_str());
        link.as_str()
            .get(..root.len())
            .is_some_and(|start| start.eq_ignore_ascii_case(&root))
    }
}

impl fmt::Display for JiraSite {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0.as_str())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CardField(String);

impl CardField {
    pub fn new(text: &str) -> Self {
        Self(text.trim().to_owned())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CardField {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CardStage {
    ToDo,
    InProgress,
    Done,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CardFamily {
    Bug,
    Story,
    Task,
    Epic,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Card {
    pub key: CardKey,
    pub summary: CardField,
    pub status: CardField,
    pub stage: CardStage,
    pub kind: CardField,
    pub family: CardFamily,
    pub assignee: Option<CardField>,
    pub description: Prose,
    pub link: Option<WebLink>,
}

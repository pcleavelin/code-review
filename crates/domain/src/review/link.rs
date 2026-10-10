use std::fmt;

use strum::VariantArray;

use crate::review::{CommitId, Login, PrNumber, Spelling};
use crate::text::{RelativePath, Span};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WebLink(String);

#[derive(Clone, Copy, Debug, PartialEq, Eq, VariantArray)]
enum Scheme {
    Https,
    Http,
}

impl Scheme {
    const fn name(self) -> Spelling {
        Spelling::new(match self {
            Self::Https => "https://",
            Self::Http => "http://",
        })
    }
}

impl WebLink {
    pub fn new(text: &str) -> Option<Self> {
        let text = text.trim();
        let web = Scheme::VARIANTS
            .iter()
            .any(|scheme| text.starts_with(scheme.name().as_str()));
        (web && !text.contains(char::is_whitespace)).then(|| Self(text.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for WebLink {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Linked {
    Pull(PrNumber),
    Issue(PrNumber),
    Commit(CommitId),
    File {
        commit: CommitId,
        path: RelativePath,
        lines: Option<Span>,
    },
    User(Login),
}

use std::fmt;

use domain::{DraftError, Line, PrNumber, RelativePath, Side, SymbolName};
use session::SessionError;

#[derive(Debug)]
pub enum Failure {
    Session(SessionError),
    NotChanged {
        number: PrNumber,
        file: RelativePath,
    },
    NotInDiff {
        file: RelativePath,
        side: Side,
        line: Line,
    },
    EmptyComment,
    NoThread(Given),
    Draft(DraftError),
    NoSuchFile(RelativePath),
    NoSuchSymbol(SymbolName),
    BadPattern(regex::Error),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Given(String);

impl Given {
    pub(crate) fn new(text: &str) -> Self {
        Self(text.to_owned())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Given {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl From<SessionError> for Failure {
    fn from(error: SessionError) -> Self {
        Self::Session(error)
    }
}

impl From<DraftError> for Failure {
    fn from(error: DraftError) -> Self {
        Self::Draft(error)
    }
}

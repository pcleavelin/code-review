use std::fmt;

use crate::review::{Decision, Draft, DraftText};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    OwnPullRequest,
    NotApprover,
    ApprovedAlready,
    NothingToSend,
}

impl fmt::Display for Refusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OwnPullRequest => formatter.write_str(
                "you opened this pull request, and the forge takes no approval or request for changes from its author",
            ),
            Self::NotApprover => {
                formatter.write_str("the forge does not count you among the approvers of this pull request")
            }
            Self::ApprovedAlready => formatter.write_str("you approved this pull request already"),
            Self::NothingToSend => {
                formatter.write_str("there are no draft comments and no summary to send")
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Content {
    Something,
    Nothing,
}

impl Content {
    pub const fn of(drafts: &[Draft], summary: Option<&DraftText>) -> Self {
        if drafts.is_empty() && summary.is_none() {
            Self::Nothing
        } else {
            Self::Something
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Rights {
    pub comment: Option<Refusal>,
    pub approve: Option<Refusal>,
    pub request_changes: Option<Refusal>,
}

impl Rights {
    pub fn refusal(&self, decision: Decision, content: Content) -> Option<Refusal> {
        let forge = match decision {
            Decision::Comment => &self.comment,
            Decision::Approve => &self.approve,
            Decision::RequestChanges => &self.request_changes,
        };
        forge.clone().or_else(|| {
            (decision != Decision::Approve && content == Content::Nothing)
                .then_some(Refusal::NothingToSend)
        })
    }
}

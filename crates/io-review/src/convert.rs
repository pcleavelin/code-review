use std::collections::BTreeMap;

use domain::{
    CommitId, Draft, DraftId, DraftText, Line, Mark, RelativePath, Review, Side, TextHash, ThreadId,
};

use crate::wire::{self, WireDraft, WireFault, WireMark, WireReview, WireSide};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseReason(String);

impl ParseReason {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<WireFault> for ParseReason {
    fn from(fault: WireFault) -> Self {
        Self(match fault {
            WireFault::Version(found) => format!("expected '{}', found '{found}'", wire::VERSION),
            WireFault::Record(found) => format!("unknown record '{found}'"),
            WireFault::Field(line) => format!("unreadable line '{line}'"),
        })
    }
}

const fn side(wire: WireSide) -> Side {
    match wire {
        WireSide::Old => Side::Old,
        WireSide::New => Side::New,
    }
}

const fn wire_side(side: Side) -> WireSide {
    match side {
        Side::Old => WireSide::Old,
        Side::New => WireSide::New,
    }
}

pub(crate) fn review_from_wire(wire: WireReview) -> Result<Review, ParseReason> {
    let mut marks = BTreeMap::new();
    for mark in wire.marks {
        let commit = CommitId::new(&mark.commit)
            .ok_or_else(|| ParseReason(format!("'{}' is not a commit id", mark.commit)))?;
        marks.insert(
            RelativePath::new(&mark.path),
            Mark {
                hash: TextHash::new(mark.hash),
                commit,
            },
        );
    }
    let drafts = wire
        .drafts
        .into_iter()
        .map(|draft| {
            Ok(Draft {
                id: DraftId::new(draft.id),
                file: RelativePath::new(&draft.path),
                side: side(draft.side),
                start: draft.start.map(Line::new),
                line: Line::new(draft.line),
                text: DraftText::new(&draft.text)
                    .ok_or_else(|| ParseReason(format!("draft {} has no text", draft.id)))?,
                reply: draft.reply.as_deref().and_then(ThreadId::new),
            })
        })
        .collect::<Result<Vec<_>, ParseReason>>()?;
    let looked = match wire.looked {
        Some(commit) => Some(
            CommitId::new(&commit)
                .ok_or_else(|| ParseReason(format!("'{commit}' is not a commit id")))?,
        ),
        None => None,
    };
    Ok(Review::new(marks, drafts, looked))
}

pub(crate) fn review_to_wire(review: &Review) -> WireReview {
    WireReview {
        marks: review
            .marks()
            .iter()
            .map(|(path, mark)| WireMark {
                hash: mark.hash.value(),
                commit: mark.commit.as_str().to_owned(),
                path: path.as_str().to_owned(),
            })
            .collect(),
        drafts: review
            .drafts()
            .iter()
            .map(|draft| WireDraft {
                id: draft.id.get(),
                side: wire_side(draft.side),
                line: draft.line.value(),
                path: draft.file.as_str().to_owned(),
                text: draft.text.as_str().to_owned(),
                reply: draft.reply.as_ref().map(|reply| reply.as_str().to_owned()),
                start: draft.start.map(Line::value),
            })
            .collect(),
        looked: review.looked().map(|commit| commit.as_str().to_owned()),
    }
}

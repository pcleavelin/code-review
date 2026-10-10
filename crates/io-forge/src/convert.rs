use domain::{
    BranchName, CodePlace, CommitId, Conversation, Decision, Draft, DraftText, Line, Login, Moment,
    Note, PrNumber, PrTitle, Prose, PullRequest, RelativePath, Thread, ThreadId, WebLink,
};

use crate::wire::{WireComment, WireNote, WirePlace, WirePull, WireReview, WireThread};

pub(crate) fn pull(wire: &WirePull) -> Option<PullRequest> {
    Some(PullRequest {
        number: PrNumber::new(wire.number)?,
        title: PrTitle::new(&wire.title),
        author: Login::new(&wire.author),
        base: BranchName::new(&wire.base),
        head: BranchName::new(&wire.head),
        head_commit: CommitId::new(&wire.sha)?,
        description: Prose::new(&wire.description),
        updated: Moment::new(&wire.updated),
    })
}

fn comment(draft: &Draft) -> WireComment {
    WireComment {
        path: draft.file.as_str().to_owned(),
        side: draft.side,
        start: draft.start.map(Line::number),
        line: draft.line.number(),
        body: draft.text.as_str().to_owned(),
        reply: draft
            .reply
            .as_ref()
            .map(|thread| thread.as_str().to_owned()),
    }
}

pub(crate) fn review(
    pull: &PullRequest,
    drafts: &[Draft],
    decision: Decision,
    body: Option<&DraftText>,
) -> WireReview {
    WireReview {
        commit: pull.head_commit.as_str().to_owned(),
        body: body.map(|text| text.as_str().to_owned()),
        decision,
        comments: drafts.iter().map(comment).collect(),
    }
}

fn note(wire: &WireNote) -> Note {
    Note {
        author: Login::new(&wire.author),
        body: Prose::new(&wire.body),
        when: Moment::new(&wire.when),
        kind: wire.kind,
        link: wire.link.as_deref().and_then(WebLink::new),
    }
}

fn place(wire: &WirePlace) -> CodePlace {
    CodePlace {
        file: RelativePath::new(&wire.path),
        side: wire.side,
        line: Line::new(wire.line.saturating_sub(1)),
    }
}

fn thread(wire: &WireThread) -> Option<Thread> {
    Some(Thread {
        id: ThreadId::new(&wire.id)?,
        place: wire.place.as_ref().map(place),
        state: wire.state,
        notes: wire.notes.iter().map(note).collect(),
    })
}

pub(crate) fn conversation(description: &Prose, threads: &[WireThread]) -> Option<Conversation> {
    let mut threads: Vec<Thread> = threads.iter().map(thread).collect::<Option<_>>()?;
    threads.sort_by(|left, right| left.started().cmp(&right.started()));
    Some(Conversation {
        description: description.clone(),
        threads,
    })
}

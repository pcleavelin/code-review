use domain::{
    BranchName, ClientId, CodePlace, CommitId, Conversation, Decision, Draft, DraftText,
    FileChange, Grant, Lifetime, Line, Login, Moment, Note, PrNumber, PrTitle, Prose, PullRequest,
    RefreshToken, Refusal, RelativePath, Renewal, Rights, SignedPicture, Submission, SubmissionId,
    Thread, ThreadId, Token, UnixTime, WebLink,
};

use crate::wire::{
    WireApprovals, WireComment, WireGrant, WireNote, WirePlace, WirePull, WireReview,
    WireSignedPicture, WireSubmission, WireThread,
};

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

fn comment(draft: &Draft, changes: &[FileChange]) -> WireComment {
    let change = changes.iter().find(|change| change.path() == &draft.file);
    WireComment {
        draft: draft.id,
        path: draft.file.as_str().to_owned(),
        old_path: change
            .and_then(FileChange::old_path)
            .unwrap_or(&draft.file)
            .as_str()
            .to_owned(),
        side: draft.side,
        start: draft.start.map(Line::number),
        line: draft.line.number(),
        counterpart: change
            .and_then(|change| change.counterpart(draft.side, draft.line))
            .map(Line::number),
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
    changes: &[FileChange],
    decision: Decision,
    body: Option<&DraftText>,
) -> WireReview {
    WireReview {
        commit: pull.head_commit.as_str().to_owned(),
        body: body.map(|text| text.as_str().to_owned()),
        decision,
        comments: drafts.iter().map(|draft| comment(draft, changes)).collect(),
    }
}

fn note(wire: &WireNote) -> Note {
    Note {
        author: Login::new(&wire.author),
        body: Prose::new(&wire.body),
        when: Moment::new(&wire.when),
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
        submission: wire.submission.as_deref().and_then(SubmissionId::new),
    })
}

fn submission(wire: &WireSubmission) -> Option<Submission> {
    let said = note(&wire.note);
    Some(Submission {
        id: SubmissionId::new(&wire.id)?,
        author: said.author,
        decision: wire.decision,
        summary: said.body,
        when: said.when,
        link: said.link,
    })
}

fn signed_picture(wire: &WireSignedPicture) -> Option<SignedPicture> {
    Some(SignedPicture {
        written: WebLink::new(&wire.written)?,
        signed: WebLink::new(&wire.signed)?,
    })
}

pub(crate) fn conversation(
    description: &Prose,
    submissions: &[WireSubmission],
    threads: &[WireThread],
    signed: &[WireSignedPicture],
) -> Option<Conversation> {
    let mut threads: Vec<Thread> = threads.iter().map(thread).collect::<Option<_>>()?;
    threads.sort_by(|left, right| left.started().cmp(&right.started()));
    Some(Conversation {
        description: description.clone(),
        submissions: submissions.iter().map(submission).collect::<Option<_>>()?,
        threads,
        signed_pictures: signed.iter().filter_map(signed_picture).collect(),
    })
}

pub(crate) fn grant(wire: &WireGrant, client: &ClientId, now: UnixTime) -> Option<Grant> {
    let token = Token::new(&wire.access)?;
    let renewal = match (&wire.refresh, wire.expires_in) {
        (Some(refresh), Some(expires_in)) => Some(Renewal {
            refresh: RefreshToken::new(refresh)?,
            expires: now.later_by(Lifetime::seconds(expires_in)),
            client: client.clone(),
        }),
        _ => None,
    };
    Some(Grant { token, renewal })
}

pub(crate) fn rights(
    pull: &PullRequest,
    me: Option<&Login>,
    approvals: Option<WireApprovals>,
) -> Rights {
    let own = me == Some(&pull.author);
    let approve = match approvals {
        Some(WireApprovals {
            has_approved: true, ..
        }) => Some(Refusal::ApprovedAlready),
        Some(WireApprovals {
            can_approve: false, ..
        }) => Some(Refusal::NotApprover),
        _ if own => Some(Refusal::OwnPullRequest),
        _ => None,
    };
    Rights {
        comment: None,
        approve,
        request_changes: own.then_some(Refusal::OwnPullRequest),
    }
}

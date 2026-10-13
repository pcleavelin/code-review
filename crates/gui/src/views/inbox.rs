use domain::Standing;
use session::{Entry, Pinned, RequestedFrom};
use ui::{Count, Label, Run};

use crate::action::Action;
use crate::field::Which;
use crate::ids;
use crate::login::{Held, Login, LoginAct, Stage};
use crate::model::Model;
use crate::review::{Broken, Cure, Inbox, Pinning, Requests, ReviewAct};
use crate::text::Needle;
use crate::theme::{
    ACCENT, FORGE_STATE, GREEN, INBOX_FIELD, ORANGE, PIN_FIELD, RED, SELECTED, TEXT, TOKEN_FIELD,
    WEAK,
};
use crate::widgets::{Chosen, Container, Frame, Padding, Scroller, TreeRow};

const STANDINGS: [Standing; 4] = [
    Standing::New,
    Standing::Updated,
    Standing::Reviewing,
    Standing::Reviewed,
];

pub(super) fn standing_word(standing: Standing) -> Label {
    Label::new(match standing {
        Standing::New => "new",
        Standing::Updated => "updated",
        Standing::Reviewing => "reviewing",
        Standing::Reviewed => "reviewed",
    })
}

pub(super) const fn standing_color(standing: Standing) -> ui::Color {
    match standing {
        Standing::New => ACCENT,
        Standing::Updated => ORANGE,
        Standing::Reviewing => TEXT,
        Standing::Reviewed => GREEN,
    }
}

fn haystack(entry: &Entry) -> Label {
    let pull = &entry.pull;
    Label::new(format!(
        "{} #{} {} {} {} {} {}",
        entry.repo,
        pull.number,
        pull.title,
        pull.author,
        pull.head,
        pull.base,
        standing_word(entry.standing).as_str()
    ))
}

fn kept(model: &Model, entry: &Entry) -> bool {
    let text = haystack(entry);
    model
        .fields
        .get(Which::Inbox)
        .text()
        .as_str()
        .split_whitespace()
        .all(|word| Needle::new(word).found_in(text.as_str()))
}

fn toolbar(model: &Model, frame: &mut Frame<'_>, entries: &[Entry], requests: &Requests) {
    frame.start(Container::ToolbarSmall);
    frame.field(
        &model.fields,
        Which::Inbox,
        &Label::new("filter: words of the title, repository, author, branch, #number"),
        INBOX_FIELD,
    );
    let chosen = model.review.standing;
    let all = ids::INBOX_STATE.with(&Label::new("all"));
    if frame
        .button(
            format!("all {}", entries.len()),
            all,
            Chosen::of(&chosen, &None),
        )
        .clicked()
    {
        frame.push(Action::Review(ReviewAct::ShowStanding(None)));
    }
    frame.attach_tip(all);
    for standing in STANDINGS {
        let word = standing_word(standing);
        let count = entries
            .iter()
            .filter(|entry| entry.standing == standing)
            .count();
        let target = ids::INBOX_STATE.with(&word);
        if frame
            .button(
                format!("{} {count}", word.as_str()),
                target,
                Chosen::of(&chosen, &Some(standing)),
            )
            .clicked()
        {
            frame.push(Action::Review(ReviewAct::ShowStanding(Some(standing))));
        }
        frame.attach_tip(target);
    }
    requested(model, frame, entries, requests);
    frame.grow();
    let refresh = ids::PULLS_REFRESH.target();
    if frame.small_button("refresh", refresh).clicked() {
        frame.push(Action::Review(ReviewAct::RefreshPulls));
    }
    frame.attach_tip(refresh);
    frame.finish();
}

fn requested(model: &Model, frame: &mut Frame<'_>, entries: &[Entry], requests: &Requests) {
    let chosen = model.review.requested;
    let target = ids::INBOX_REQUESTED.target();
    if frame
        .button(
            requests.chip(entries),
            target,
            Chosen::of(&chosen, &RequestedFrom::You),
        )
        .clicked()
    {
        let next = match chosen {
            RequestedFrom::Any => RequestedFrom::You,
            RequestedFrom::You => RequestedFrom::Any,
        };
        frame.push(Action::Review(ReviewAct::ShowRequested(next)));
    }
    frame.attach_tip(target);
}

fn pins(model: &Model, frame: &mut Frame<'_>) {
    frame.start(Container::ToolbarSmall);
    frame.label("pinned", WEAK);
    let desk = &model.review.desk;
    if let Some(here) = desk.here() {
        frame.label_runs(vec![
            Run::new(here.name().as_str(), TEXT),
            Run::new(" (here)", WEAK),
        ]);
    }
    for (position, pin) in desk.pinned().iter().enumerate() {
        let (name, color) = match pin {
            Pinned::Ready(repo) => (Label::new(repo.name().as_str()), TEXT),
            Pinned::Broken { root, .. } => (Label::new(root.as_path().display().to_string()), RED),
        };
        frame.label(name.as_str(), color);
        let unpin = ids::UNPIN.nth(Count::new(position));
        if frame.small_button("x", unpin).clicked() {
            frame.push(Action::Review(ReviewAct::Unpin(session::RepoName::new(
                name.as_str(),
            ))));
        }
        frame.attach_tip(unpin);
    }
    frame.grow();
    frame.field(
        &model.fields,
        Which::Pin,
        &Label::new("a clone's path, or a repository URL"),
        PIN_FIELD,
    );
    let pin = ids::PIN_ADD.target();
    let word = match model.review.pinning {
        Pinning::Idle => "pin",
        Pinning::Busy => "pinning...",
    };
    if frame.small_button(word, pin).clicked() {
        frame.push(Action::Review(ReviewAct::Pin));
    }
    frame.attach_tip(pin);
    frame.finish();
}

fn forges(model: &Model, frame: &mut Frame<'_>) {
    let logins = &model.review.logins;
    if let Some(login) = &logins.login {
        strip(model, frame, login);
        return;
    }
    let sites = logins.sites(&model.review.desk);
    if sites.is_empty() {
        return;
    }
    frame.start(Container::ToolbarSmall);
    frame.label("forges", WEAK);
    for site in sites {
        let host = Label::new(site.to_string());
        let held = logins.held.get(&site);
        let (state, color) = match (logins.failed.contains_key(&site), held) {
            (true, _) => ("login failed", RED),
            (false, Some(Held::In)) => ("logged in", GREEN),
            (false, Some(Held::Out)) => ("not logged in", WEAK),
            (false, Some(Held::TimedOut(_))) => ("not answering", ORANGE),
            (false, None) => ("checking...", WEAK),
        };
        frame.label_runs(vec![
            Run::new(host.as_str(), TEXT),
            Run::new(
                format!(
                    " {state:<width$}",
                    width = usize::try_from(FORGE_STATE.get()).unwrap_or(0)
                ),
                color,
            ),
        ]);
        if held == Some(&Held::In) {
            let logout = ids::LOGOUT.with(&host);
            if frame.small_button("log out", logout).clicked() {
                frame.push(Action::Login(LoginAct::Logout(site.clone())));
            }
            frame.attach_tip(logout);
        } else {
            let login = ids::LOGIN.with(&host);
            if frame.small_button("log in", login).clicked() {
                frame.push(Action::Login(LoginAct::Start(site.clone())));
            }
            frame.attach_tip(login);
        }
    }
    frame.finish();
}

fn strip(model: &Model, frame: &mut Frame<'_>, login: &Login) {
    frame.start(Container::ToolbarSmall);
    let host = login.site.to_string();
    match &login.stage {
        Stage::Device { authorization, .. } => {
            frame.label_runs(vec![
                Run::new(format!("{host}: enter "), WEAK),
                Run::new(authorization.user_code().as_str(), ACCENT),
                Run::new(" at ", WEAK),
                Run::new(authorization.verification().as_str(), TEXT),
            ]);
            let copy = ids::LOGIN_COPY.target();
            if frame.small_button("copy code", copy).clicked() {
                frame.push(Action::Login(LoginAct::CopyCode));
            }
            frame.attach_tip(copy);
            open_page(frame);
            frame.label("waiting for approval", ORANGE);
        }
        Stage::Paste { page } => {
            let made = page
                .as_ref()
                .map_or_else(|| "on the forge".to_owned(), |page| format!("at {page}"));
            frame.label_runs(vec![
                Run::new(host, TEXT),
                Run::new(format!(": paste a token made {made}"), WEAK),
            ]);
            if page.is_some() {
                open_page(frame);
            }
            frame.field(
                &model.fields,
                Which::Token,
                &Label::new("paste the token here"),
                TOKEN_FIELD,
            );
            let save = ids::LOGIN_SAVE.target();
            if frame.small_button("save", save).clicked() {
                frame.push(Action::Login(LoginAct::Save));
            }
            frame.attach_tip(save);
        }
    }
    frame.grow();
    let cancel = ids::LOGIN_CANCEL.target();
    if frame.small_button("cancel", cancel).clicked() {
        frame.push(Action::Login(LoginAct::Cancel));
    }
    frame.attach_tip(cancel);
    frame.finish();
}

fn open_page(frame: &mut Frame<'_>) {
    let open = ids::LOGIN_OPEN.target();
    if frame.small_button("open page", open).clicked() {
        frame.push(Action::Login(LoginAct::OpenPage));
    }
    frame.attach_tip(open);
}

fn row(model: &Model, frame: &mut Frame<'_>, position: Count, entry: &Entry) {
    let pull = &entry.pull;
    let open = model
        .review
        .open
        .as_ref()
        .is_some_and(|open| open.repo.name() == &entry.repo && open.pull.number == pull.number);
    let mut badges = Vec::new();
    if model.review.opening == Some(pull.number) {
        badges.push(Run::new("opening", ORANGE));
    }
    if let Some(progress) = entry.progress {
        badges.push(Run::new(
            format!("{progress} seen"),
            if progress.done() { GREEN } else { WEAK },
        ));
    }
    if !entry.drafts.is_empty() {
        badges.push(Run::new(format!("{} drafts", entry.drafts.len()), ORANGE));
    }
    badges.push(Run::new(pull.updated.day(), WEAK));
    let runs = vec![
        Run::new(
            format!("{:<9} ", standing_word(entry.standing).as_str()),
            standing_color(entry.standing),
        ),
        Run::new(format!("{}#{} ", entry.repo, pull.number), ACCENT),
        Run::new(pull.title.as_str(), TEXT),
        Run::new(
            format!("  {}  {} <- {}", pull.author, pull.base, pull.head),
            WEAK,
        ),
    ];
    let tree = TreeRow {
        level: Count::ZERO,
        runs,
        badges,
        marked: open.then_some(SELECTED),
        action: None,
    };
    let clicks = frame.tree_row(tree, ids::PULL_ROW.nth(position));
    if clicks.row.clicked() {
        frame.push(Action::Review(ReviewAct::OpenPull(
            entry.repo.clone(),
            pull.number,
        )));
    }
}

pub(super) fn inbox(model: &Model, frame: &mut Frame<'_>) {
    let none = Requests::default();
    let (entries, broken, requests): (&[Entry], &[Broken], &Requests) = match &model.review.inbox {
        Inbox::Listed {
            entries,
            broken,
            requests,
        } => (entries, broken, requests),
        Inbox::Asking => (&[], &[], &none),
    };
    let only_requested = model.review.requested == RequestedFrom::You;
    toolbar(model, frame, entries, requests);
    pins(model, frame);
    forges(model, frame);
    let id = ids::inbox();
    frame.scroll_column(id, model.scrolls.get(id), Scroller::Plain, None);
    for (site, held) in &model.review.logins.held {
        if let Held::TimedOut(why) = held {
            frame.note(format!("{site}: {}", why.as_str()), ORANGE, Padding::Step);
        }
    }
    for (repo, waiting) in &model.review.waiting {
        frame.note(format!("{repo}: {waiting}..."), WEAK, Padding::Step);
    }
    if matches!(model.review.inbox, Inbox::Asking) && model.review.waiting.is_empty() {
        frame.note(
            "asking every forge for its open pull requests...",
            WEAK,
            Padding::Step,
        );
    }
    for gone in broken {
        frame.note(
            format!("{}: {}", gone.place.as_str(), gone.why.as_str()),
            RED,
            Padding::Step,
        );
    }
    if only_requested {
        for unanswered in &requests.unanswered {
            let color = match unanswered.cure {
                Cure::LogIn => ORANGE,
                Cure::Unknown => RED,
            };
            frame.note(
                format!(
                    "{}: {}",
                    unanswered.broken.place.as_str(),
                    unanswered.broken.why.as_str()
                ),
                color,
                Padding::Step,
            );
        }
    }
    if model.review.desk.repos().next().is_none() {
        frame.note(
            "nothing pinned yet: type a clone's path or a repository's URL in the field above and press enter. Starting the app inside a clone shows that one too.",
            WEAK,
            Padding::Step,
        );
    }
    let shown: Vec<(usize, &Entry)> = entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| {
            model
                .review
                .standing
                .is_none_or(|standing| entry.standing == standing)
                && (!only_requested || requests.of(entry))
                && kept(model, entry)
        })
        .collect();
    if shown.is_empty() && !entries.is_empty() {
        frame.note("nothing matches the filter", WEAK, Padding::Step);
    }
    for (position, entry) in shown {
        row(model, frame, Count::new(position), entry);
    }
    frame.finish();
}

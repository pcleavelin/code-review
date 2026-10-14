use std::cmp::Ordering;

use domain::{Progress, Standing};
use platform::Cursor;
use session::{Entry, Pinned, RequestedFrom};
use strum::VariantArray;
use ui::{Button, Count, Label, Px, Run};

use crate::action::Action;
use crate::columns::{GridAct, InboxColumn, Order, Resizing, Sorting};
use crate::field::Which;
use crate::ids;
use crate::login::{Held, JiraForm, Login, LoginAct, Stage};
use crate::model::Model;
use crate::review::{Broken, Cure, Inbox, Pinning, Requests, ReviewAct};
use crate::text::Needle;
use crate::theme::{
    ACCENT, COLUMN_EDGE, Cells, FORGE_STATE, FORM_LABEL, GREEN, INBOX_FIELD, JIRA_FIELD, ORANGE,
    PIN_FIELD, RED, SELECTED, TEXT, TOKEN_FIELD, WEAK,
};
use crate::widgets::{Chosen, CommentLook, Container, Frame, Padding, Scroller};

const STANDINGS: [Standing; 4] = [
    Standing::New,
    Standing::Updated,
    Standing::Reviewing,
    Standing::Reviewed,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LoginFailed {
    Yes,
    No,
}

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
    frame.start(Container::ToolbarSmall);
    frame.label("forges", WEAK);
    jira_sites(model, frame);
    for site in sites {
        let host = Label::new(site.to_string());
        let held = logins.held.get(&site);
        let failed = if logins.failed.contains_key(&site) {
            LoginFailed::Yes
        } else {
            LoginFailed::No
        };
        site_state(frame, &host, held, failed);
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
    if let Some(form) = &logins.jira_form {
        jira_form(model, frame, form);
    }
}

fn jira_sites(model: &Model, frame: &mut Frame<'_>) {
    let logins = &model.review.logins;
    if logins.jira.is_empty() {
        frame.label_runs(vec![
            Run::new("Jira", TEXT),
            Run::new(" not logged in", WEAK),
        ]);
        if logins.jira_form.is_none() {
            let login = ids::JIRA_LOGIN.target();
            if frame.small_button("log in", login).clicked() {
                frame.push(Action::Login(LoginAct::StartJira(None)));
            }
            frame.attach_tip(login);
        }
        return;
    }
    for (site, held) in &logins.jira {
        let host = Label::new(site.to_string());
        let (state, color) = match (held, logins.jira_names.get(site)) {
            (Held::In, Some(name)) => (format!(" logged in as {name}"), GREEN),
            (Held::In, None) => (" logged in".to_owned(), GREEN),
            (Held::Out, _) => (" not logged in".to_owned(), WEAK),
            (Held::TimedOut(_), _) => (" keychain not answering".to_owned(), ORANGE),
        };
        frame.label_runs(vec![
            Run::new(format!("Jira {site}"), TEXT),
            Run::new(state, color),
        ]);
        if held == &Held::In {
            let logout = ids::LOGOUT.with(&host);
            if frame.small_button("log out", logout).clicked() {
                frame.push(Action::Login(LoginAct::LogoutJira(site.clone())));
            }
            frame.attach_tip(logout);
        } else if logins.jira_form.is_none() {
            let login = ids::LOGIN.with(&host);
            if frame.small_button("log in", login).clicked() {
                frame.push(Action::Login(LoginAct::StartJira(Some(site.clone()))));
            }
            frame.attach_tip(login);
        }
    }
}

fn form_row(model: &Model, frame: &mut Frame<'_>, which: Which, words: [Label; 2]) {
    let [name, hint] = words;
    frame.start(Container::FitRow);
    let width = FORM_LABEL.of(frame.cell_width());
    frame.cell(vec![Run::new(name, WEAK)], width);
    frame.field(&model.fields, which, &hint, JIRA_FIELD);
}

fn jira_form(model: &Model, frame: &mut Frame<'_>, form: &JiraForm) {
    frame.start(Container::Comment {
        look: CommentLook::Thread,
        id: ids::jira_form(),
    });
    frame.title("Log in to Jira");
    form_row(
        model,
        frame,
        Which::JiraSite,
        [
            Label::new("site"),
            Label::new("yourcompany.atlassian.net, or any link to the site"),
        ],
    );
    frame.finish();
    form_row(
        model,
        frame,
        Which::JiraEmail,
        [
            Label::new("email"),
            Label::new("you@company.com; empty for a Data Center personal access token"),
        ],
    );
    frame.finish();
    form_row(
        model,
        frame,
        Which::Token,
        [Label::new("API token"), Label::new("paste the token")],
    );
    let tokens = ids::JIRA_TOKENS.target();
    if frame.small_button("create a token", tokens).clicked() {
        frame.push(Action::Login(LoginAct::OpenPage));
    }
    frame.attach_tip(tokens);
    frame.finish();
    frame.start(Container::ToolbarSmall);
    let save = ids::LOGIN_SAVE.target();
    let checking = matches!(form, JiraForm::Checking(_));
    if frame
        .small_button(if checking { "checking..." } else { "log in" }, save)
        .clicked()
    {
        frame.push(Action::Login(LoginAct::Save));
    }
    frame.attach_tip(save);
    let cancel = ids::LOGIN_CANCEL.target();
    if frame.small_button("cancel", cancel).clicked() {
        frame.push(Action::Login(LoginAct::Cancel));
    }
    frame.attach_tip(cancel);
    frame.finish();
    match form {
        JiraForm::Editing => frame.note(
            "the token is checked with Jira, then kept in the system keychain",
            WEAK,
            Padding::Step,
        ),
        JiraForm::Checking(site) => frame.note(format!("asking {site}..."), ORANGE, Padding::Step),
        JiraForm::Refused(why) => frame.note(why.as_str(), RED, Padding::Step),
    }
    frame.finish();
}

fn site_state(frame: &mut Frame<'_>, name: &Label, held: Option<&Held>, failed: LoginFailed) {
    let (state, color) = match (failed, held) {
        (LoginFailed::Yes, _) => ("login failed", RED),
        (LoginFailed::No, Some(Held::In)) => ("logged in", GREEN),
        (LoginFailed::No, Some(Held::Out)) => ("not logged in", WEAK),
        (LoginFailed::No, Some(Held::TimedOut(_))) => ("not answering", ORANGE),
        (LoginFailed::No, None) => ("checking...", WEAK),
    };
    frame.label_runs(vec![
        Run::new(name.as_str(), TEXT),
        Run::new(
            format!(
                " {state:<width$}",
                width = usize::try_from(FORGE_STATE.get()).unwrap_or(0)
            ),
            color,
        ),
    ]);
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

fn cell_runs(model: &Model, column: InboxColumn, entry: &Entry) -> Vec<Run> {
    let pull = &entry.pull;
    match column {
        InboxColumn::Standing if model.review.opening == Some(pull.number) => {
            vec![Run::new("opening", ORANGE)]
        }
        InboxColumn::Standing => vec![Run::new(
            standing_word(entry.standing),
            standing_color(entry.standing),
        )],
        InboxColumn::Pull => vec![Run::new(format!("{}#{}", entry.repo, pull.number), ACCENT)],
        InboxColumn::Title => vec![Run::new(pull.title.as_str(), TEXT)],
        InboxColumn::Author => vec![Run::new(pull.author.as_str(), TEXT)],
        InboxColumn::Branch => vec![Run::new(format!("{} <- {}", pull.base, pull.head), WEAK)],
        InboxColumn::Seen => {
            let mut runs = Vec::new();
            if let Some(progress) = entry.progress {
                runs.push(Run::new(
                    format!("{progress} seen"),
                    if progress.done() { GREEN } else { WEAK },
                ));
            }
            if !entry.drafts.is_empty() {
                runs.push(Run::new(format!("  {} drafts", entry.drafts.len()), ORANGE));
            }
            runs
        }
        InboxColumn::Updated => vec![Run::new(pull.updated.to_string(), WEAK)],
    }
}

fn cell_text(model: &Model, column: InboxColumn, entry: &Entry) -> Label {
    Label::new(
        cell_runs(model, column, entry)
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>(),
    )
}

fn standing_rank(standing: Standing) -> Count {
    Count::new(
        STANDINGS
            .iter()
            .position(|known| *known == standing)
            .unwrap_or(STANDINGS.len()),
    )
}

fn ordered(model: &Model, column: InboxColumn, one: &Entry, other: &Entry) -> Ordering {
    match column {
        InboxColumn::Standing => standing_rank(one.standing).cmp(&standing_rank(other.standing)),
        InboxColumn::Pull => (&one.repo, one.pull.number).cmp(&(&other.repo, other.pull.number)),
        InboxColumn::Seen => (one.progress.map(Progress::seen), one.drafts.len())
            .cmp(&(other.progress.map(Progress::seen), other.drafts.len())),
        InboxColumn::Updated => one
            .pull
            .updated
            .to_string()
            .cmp(&other.pull.updated.to_string()),
        InboxColumn::Title | InboxColumn::Author | InboxColumn::Branch => {
            cell_text(model, column, one)
                .as_str()
                .to_lowercase()
                .cmp(&cell_text(model, column, other).as_str().to_lowercase())
        }
    }
}

fn width(model: &Model, frame: &Frame<'_>, column: InboxColumn) -> Px {
    frame.cell_width() * Count::new(usize::from(model.inbox_grid.cells(column).get()))
}

fn header(model: &Model, frame: &mut Frame<'_>) {
    frame.start(Container::FitRow);
    let resizing = model.inbox_grid.resizing;
    let mouse = frame.ui.pointer().mouse.horizontal;
    for column in InboxColumn::VARIANTS.iter().copied() {
        let name = column.name();
        let arrow = match model.inbox_grid.sorting {
            Some(Sorting {
                column: sorted,
                order,
            }) if sorted == column => match order {
                Order::Ascending => " \u{25b2}",
                Order::Descending => " \u{25bc}",
            },
            Some(_) | None => "",
        };
        let target = ids::COLUMN_HEADER.with(&name);
        let runs = vec![Run::new(format!("{}{arrow}", name.as_str()), WEAK)];
        if frame
            .header_cell(runs, width(model, frame, column), target)
            .clicked()
        {
            frame.push(Action::Grid(GridAct::Sort(column)));
        }
        frame.attach_tip(target);
        let edge = ids::COLUMN_EDGE.with(&name);
        let held = frame.column_edge(COLUMN_EDGE, Some(edge));
        if held.down() || held.hovered() {
            frame.cursor = Cursor::ColumnResize;
        }
        match resizing {
            Some(Resizing {
                column: grabbed, ..
            }) if grabbed == column => {
                if frame.ui.pointer().down.contains(Button::Left) {
                    frame.push(Action::Grid(GridAct::Drag(mouse)));
                    frame.cursor = Cursor::ColumnResize;
                } else {
                    frame.push(Action::Grid(GridAct::Drop));
                }
            }
            None if held.down() => frame.push(Action::Grid(GridAct::Grab(column, mouse))),
            Some(_) | None => {}
        }
        frame.attach_tip(edge);
    }
    frame.finish();
    frame.start(Container::FitRow);
    for column in InboxColumn::VARIANTS.iter().copied() {
        let cells = Cells::of_count(usize::from(model.inbox_grid.cells(column).get()));
        frame.field_named(
            &model.fields,
            Which::Column(column),
            Which::Column(column).id(),
            &Label::new("filter"),
            cells,
        );
        frame.column_edge(COLUMN_EDGE, None);
    }
    frame.finish();
}

fn row(model: &Model, frame: &mut Frame<'_>, position: Count, entry: &Entry) {
    let pull = &entry.pull;
    let open = model
        .review
        .open
        .as_ref()
        .is_some_and(|open| open.repo.name() == &entry.repo && open.pull.number == pull.number);
    let row = frame.open_grid_row(ids::PULL_ROW.nth(position), open.then_some(SELECTED));
    for column in InboxColumn::VARIANTS.iter().copied() {
        frame.cell(cell_runs(model, column, entry), width(model, frame, column));
        frame.column_edge(COLUMN_EDGE, None);
    }
    frame.finish();
    if row.clicked() {
        frame.push(Action::Review(ReviewAct::OpenPull(
            entry.repo.clone(),
            pull.number,
        )));
    }
}

fn filtered(model: &Model, entry: &Entry) -> bool {
    InboxColumn::VARIANTS.iter().copied().all(|column| {
        let filter = model.fields.get(Which::Column(column)).text();
        let cell = cell_text(model, column, entry);
        filter
            .as_str()
            .split_whitespace()
            .all(|word| Needle::new(word).found_in(cell.as_str()))
    })
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
    let mut shown: Vec<(usize, &Entry)> = entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| {
            model
                .review
                .standing
                .is_none_or(|standing| entry.standing == standing)
                && (!only_requested || requests.of(entry))
                && kept(model, entry)
                && filtered(model, entry)
        })
        .collect();
    if let Some(sorting) = model.inbox_grid.sorting {
        shown.sort_by(|(_, one), (_, other)| {
            let order = ordered(model, sorting.column, one, other);
            match sorting.order {
                Order::Ascending => order,
                Order::Descending => order.reverse(),
            }
        });
    }
    let across = ids::inbox().with("grid");
    frame.open_across(model, across);
    if !entries.is_empty() {
        header(model, frame);
    }
    if shown.is_empty() && !entries.is_empty() {
        frame.note("nothing matches the filter", WEAK, Padding::Step);
    }
    for (position, entry) in shown {
        row(model, frame, Count::new(position), entry);
    }
    frame.finish();
    frame.finish();
}

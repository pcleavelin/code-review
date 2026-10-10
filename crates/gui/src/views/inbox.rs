use domain::Standing;
use session::{Entry, Pinned};
use ui::{Count, Label, Run};

use crate::action::Action;
use crate::field::Which;
use crate::ids;
use crate::model::Model;
use crate::review::{Broken, Inbox, Pinning, ReviewAct};
use crate::text::Needle;
use crate::theme::{ACCENT, GREEN, INBOX_FIELD, ORANGE, PIN_FIELD, RED, SELECTED, TEXT, WEAK};
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

fn toolbar(model: &Model, frame: &mut Frame<'_>, entries: &[Entry]) {
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
    frame.grow();
    let refresh = ids::PULLS_REFRESH.target();
    if frame.small_button("refresh", refresh).clicked() {
        frame.push(Action::Review(ReviewAct::RefreshPulls));
    }
    frame.attach_tip(refresh);
    frame.finish();
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
    let (entries, broken): (&[Entry], &[Broken]) = match &model.review.inbox {
        Inbox::Listed { entries, broken } => (entries, broken),
        Inbox::Asking => (&[], &[]),
    };
    toolbar(model, frame, entries);
    pins(model, frame);
    let id = ids::inbox();
    frame.scroll_column(id, model.scrolls.get(id), Scroller::Plain, None);
    if matches!(model.review.inbox, Inbox::Asking) {
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

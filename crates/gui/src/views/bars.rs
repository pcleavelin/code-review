use ui::{Icon, Run};

use crate::action::Action;
use crate::ids;
use crate::model::Model;
use crate::settings::SettingsAct;
use crate::status::Tone;
use crate::text::Clipped;
use crate::theme::{ACCENT, GREEN, ORANGE, RED, TEXT, TREE_PLACE, WEAK};
use crate::widgets::{Chosen, Container, Enabled, Frame};
use crate::work::ServerState;

pub(super) fn top_bar(model: &Model, frame: &mut Frame<'_>) {
    frame.start(Container::TopBar);
    let enabled = |can: bool| {
        if can {
            Enabled::Enabled
        } else {
            Enabled::Disabled
        }
    };
    if frame
        .nav_button(
            Icon::Back,
            ids::BACK.target(),
            enabled(model.nav.can_go_back()),
        )
        .clicked()
    {
        frame.push(Action::Back);
    }
    frame.attach_tip(ids::BACK.target());
    if frame
        .nav_button(
            Icon::Forward,
            ids::FORWARD.target(),
            enabled(model.nav.can_go_forward()),
        )
        .clicked()
    {
        frame.push(Action::Forward);
    }
    frame.attach_tip(ids::FORWARD.target());
    if let Some(open) = &model.review.open {
        let (pull, opened) = (&open.pull, &open.opened);
        let progress = opened.review.progress(&opened.changes);
        let drafts = opened.review.drafts().len();
        frame.label_runs(vec![
            Run::new(format!("{}#{} ", open.repo.name(), pull.number), ACCENT),
            Run::new(pull.title.as_str(), TEXT),
            Run::new(format!("  {} <- {}", pull.base, pull.head), WEAK),
            Run::new(
                format!("  {progress} seen"),
                if progress.done() { GREEN } else { ORANGE },
            ),
            Run::new(format!("  {drafts} drafts"), WEAK),
        ]);
    } else if let Some(number) = model.review.opening {
        frame.label(format!("opening #{number}..."), ORANGE);
    } else {
        frame.label("no pull request open: pick one in the Inbox", WEAK);
    }
    frame.grow();
    let open = if model.settings_menu.is_some() {
        Chosen::Chosen
    } else {
        Chosen::Plain
    };
    if frame
        .button(Icon::Settings, ids::SETTINGS_OPEN.target(), open)
        .clicked()
    {
        frame.push(Action::Settings(SettingsAct::Toggle));
    }
    frame.attach_tip(ids::SETTINGS_OPEN.target());
    frame.finish();
}

pub(super) fn status_bar(model: &Model, frame: &mut Frame<'_>) {
    frame.start(Container::StatusBar);
    let status = frame
        .overlay
        .status
        .clone()
        .unwrap_or_else(|| model.status.clone());
    let color = match status.tone() {
        Tone::Plain => TEXT,
        Tone::Done => GREEN,
        Tone::Warning => ORANGE,
        Tone::Problem => RED,
    };
    frame.caption(vec![Run::new(status.line(), color)]);
    let progress = model.work.progress();
    if !progress.as_str().is_empty() {
        frame.label(progress.clone(), WEAK);
    }
    for server in model.work.server_states(&model.index) {
        let light = match server.state {
            ServerState::NotStarted => WEAK,
            ServerState::Starting | ServerState::Indexing => ORANGE,
            ServerState::Ready => GREEN,
            ServerState::Missing | ServerState::Failed => RED,
        };
        frame.label_runs(vec![
            Run::new(Icon::Dot, light),
            Run::new(format!(" {}", server.language.program()), WEAK),
        ]);
    }
    if let Some(opened) = model.review.opened() {
        let place = opened.tree.display().to_string().replace('\\', "/");
        let room = usize::try_from(TREE_PLACE.get()).unwrap_or(0);
        frame.label(format!("tree: {}", Clipped::left(&place, room)), WEAK);
    }
    frame.finish();
}

use ui::{Canvas, Point, Px, Rect};

use crate::action::Action;
use crate::diffs::Loading;
use crate::ids;
use crate::model::Model;
use crate::review::ReviewAct;
use crate::theme::{BORDER, PIXEL, RED, TEXT, WEAK};
use crate::viewer::{Step, Viewer, ViewerAct, Zoom};
use crate::widgets::{Container, Frame, Padding};

fn controls(frame: &mut Frame<'_>, zoom: Option<Zoom>) {
    frame.start(Container::Toolbar);
    for (word, control, act) in [
        (
            "\u{2212}",
            ids::VIEWER_OUT,
            ViewerAct::Step(Step::Out, None),
        ),
        ("fit", ids::VIEWER_FIT, ViewerAct::Fit),
        ("100%", ids::VIEWER_ACTUAL, ViewerAct::Actual),
        ("+", ids::VIEWER_IN, ViewerAct::Step(Step::In, None)),
    ] {
        let target = control.target();
        if frame.small_button(word, target).clicked() {
            frame.push(Action::Viewer(act));
        }
        frame.attach_tip(target);
    }
    if let Some(zoom) = zoom {
        frame.label(format!("{}%", zoom.percent()), WEAK);
    }
    frame.grow();
    for (word, control, act) in [
        ("open in browser", ids::VIEWER_BROWSER, ViewerAct::Browser),
        ("close", ids::VIEWER_CLOSE, ViewerAct::Close),
    ] {
        let target = control.target();
        if frame.small_button(word, target).clicked() {
            frame.push(Action::Viewer(act));
        }
        frame.attach_tip(target);
    }
    frame.finish();
}

fn stage_input(frame: &mut Frame<'_>, viewer: &Viewer) {
    let stage = frame.ui.interaction(ids::viewer_stage());
    let pointer = frame.ui.pointer();
    if stage.down() {
        frame.push(Action::Viewer(ViewerAct::Drag(pointer.mouse)));
    } else if viewer.dragging() {
        frame.push(Action::Viewer(ViewerAct::Release));
    }
    if !stage.hovered() {
        return;
    }
    if pointer.pinch.get() != 0.0 {
        frame.push(Action::Viewer(ViewerAct::Pinch(
            pointer.pinch,
            pointer.mouse,
        )));
    }
    let wheel = Point::new(
        pointer.wheel.horizontal.truncate(),
        pointer.wheel.vertical.truncate(),
    );
    if wheel == Point::new(Px::ZERO, Px::ZERO) {
        return;
    }
    if pointer.mods.primary() {
        let step = if wheel.vertical > Px::ZERO {
            Step::In
        } else {
            Step::Out
        };
        frame.push(Action::Viewer(ViewerAct::Step(step, Some(pointer.mouse))));
    } else {
        frame.push(Action::Viewer(ViewerAct::Pan(wheel)));
    }
}

pub(super) fn viewer(model: &Model, frame: &mut Frame<'_>) {
    let Some(viewer) = &model.viewer else {
        return;
    };
    let window = frame.ui.size();
    frame.start(Container::Viewer { size: window });
    let stage = frame
        .ui
        .placement(ids::viewer_stage())
        .map(|placed| placed.rect);
    match model.review.pictures.get(&viewer.link) {
        Some(Loading::Got(animation)) => {
            let size = animation.size();
            let zoom = stage.map(|stage| viewer.zoom(size, stage.extent()));
            controls(frame, zoom);
            if let Some(shown) = animation.at(model.now) {
                if let Some(next) = shown.next {
                    frame.push(Action::Review(ReviewAct::PictureWake(next)));
                }
                let picture = shown.picture;
                let placed_viewer = viewer.clone();
                let draw = move |canvas: &mut Canvas<'_>, rect: Rect| {
                    let area = placed_viewer.placed(size, rect);
                    canvas.picture(area, &picture);
                    canvas.outline(area, PIXEL, BORDER);
                };
                frame.stage(Box::new(draw), ids::viewer_stage());
            }
        }
        Some(Loading::Failed(why)) => {
            controls(frame, None);
            frame.note(why.as_str(), RED, Padding::Step);
        }
        Some(Loading::Asking) | None => {
            controls(frame, None);
            frame.note("loading the picture...", TEXT, Padding::Step);
        }
    }
    stage_input(frame, viewer);
    frame.finish();
}

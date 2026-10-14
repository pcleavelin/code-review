use domain::WebLink;
use ui::{Extent, Pinch, Point, Px, Rect};

use super::{Fitting, Viewer, Zoom};

fn px(value: i32) -> Px {
    Px::new(value)
}

fn viewer() -> Viewer {
    Viewer::new(WebLink::new("https://example.com/shot.png").unwrap())
}

#[test]
fn a_picture_fits_the_stage_but_is_never_blown_up_to_fit() {
    let stage = Extent::new(px(800), px(600));
    assert_eq!(
        Zoom::fit(Extent::new(px(1600), px(600)), stage).percent(),
        50
    );
    assert_eq!(
        Zoom::fit(Extent::new(px(400), px(1200)), stage).percent(),
        50
    );
    assert_eq!(
        Zoom::fit(Extent::new(px(100), px(100)), stage),
        Zoom::ACTUAL
    );
}

#[test]
fn a_fitted_picture_is_centred_on_the_stage() {
    let stage = Rect::new(px(100), px(50), px(800), px(600));
    let placed = viewer().placed(Extent::new(px(1600), px(600)), stage);
    assert_eq!(placed, Rect::new(px(100), px(200), px(800), px(300)));
}

#[test]
fn zooming_keeps_the_point_under_the_pointer_in_place() {
    let stage = Rect::new(px(0), px(0), px(800), px(600));
    let picture = Extent::new(px(800), px(600));
    let mut shown = viewer();
    let pointer = Point::new(px(600), px(450));
    shown.zoom_to(Zoom(2000), pointer, picture, stage);
    let placed = shown.placed(picture, stage);
    assert_eq!(placed.width, px(1600));
    assert_eq!(placed.left, px(-600));
    assert_eq!(placed.top, px(-450));
    assert_eq!(shown.fitting, Fitting::Zoomed(Zoom(2000)));
}

#[test]
fn a_step_or_a_pinch_moves_the_zoom_within_its_bounds() {
    assert_eq!(Zoom::ACTUAL.times(Zoom::STEP_UP), Zoom(1250));
    assert_eq!(Zoom::ACTUAL.times(Zoom::STEP_DOWN), Zoom(800));
    assert_eq!(Zoom(15_000).times(Zoom::STEP_UP), Zoom::MOST);
    assert_eq!(Zoom(21).times(Zoom::STEP_DOWN), Zoom::LEAST);
    assert_eq!(Zoom::ACTUAL.pinched(Pinch::new(0.5)), Zoom(1500));
    assert_eq!(Zoom::ACTUAL.pinched(Pinch::new(-0.25)), Zoom(750));
}

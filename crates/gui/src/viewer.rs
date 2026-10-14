use domain::WebLink;
use ui::{Coordinate, Extent, Pinch, Point, Px, Rect};

use crate::app::App;
use crate::diffs::Loading;
use crate::ids;
use crate::review::ReviewAct;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Zoom(i64);

impl Zoom {
    pub(crate) const ACTUAL: Self = Self(1000);
    const LEAST: Self = Self(20);
    const MOST: Self = Self(16_000);
    const WHOLE: i64 = 1000;
    const STEP_UP: i64 = 1250;
    const STEP_DOWN: i64 = 800;

    pub(crate) fn of(self, length: Px) -> Px {
        Px::of_wide(length.wide() * self.0 / Self::WHOLE)
    }

    pub(crate) fn fit(picture: Extent, stage: Extent) -> Self {
        let across = |room: Px, length: Px| room.wide() * Self::WHOLE / length.wide().max(1);
        Self(
            across(stage.width, picture.width)
                .min(across(stage.height, picture.height))
                .min(Self::ACTUAL.0),
        )
        .bounded()
    }

    const fn bounded(self) -> Self {
        Self(if self.0 < Self::LEAST.0 {
            Self::LEAST.0
        } else if self.0 > Self::MOST.0 {
            Self::MOST.0
        } else {
            self.0
        })
    }

    fn times(self, permille: i64) -> Self {
        Self(self.0 * permille / Self::WHOLE).bounded()
    }

    fn pinched(self, pinch: Pinch) -> Self {
        let change = Coordinate::new(pinch.get() * 1000.0).truncate().wide();
        self.times((Self::WHOLE + change).max(Self::LEAST.0))
    }

    pub(crate) const fn percent(self) -> i64 {
        self.0 / 10
    }

    fn rescale(self, length: Px, before: Self) -> Px {
        Px::of_wide(length.wide() * self.0 / before.0.max(1))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Fitting {
    Fit,
    Zoomed(Zoom),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Viewer {
    pub(crate) link: WebLink,
    pub(crate) fitting: Fitting,
    pub(crate) pan: Point,
    grab: Option<Point>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Viewed {
    picture: Extent,
    stage: Rect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Step {
    In,
    Out,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ViewerAct {
    Open(WebLink),
    Close,
    Step(Step, Option<Point>),
    Pinch(Pinch, Point),
    Fit,
    Actual,
    Pan(Point),
    Drag(Point),
    Release,
    Browser,
}

fn center(stage: Rect) -> Point {
    Point::new(stage.left + stage.width / 2, stage.top + stage.height / 2)
}

impl Viewer {
    pub(crate) const fn new(link: WebLink) -> Self {
        Self {
            link,
            fitting: Fitting::Fit,
            pan: Point::new(Px::ZERO, Px::ZERO),
            grab: None,
        }
    }

    pub(crate) const fn dragging(&self) -> bool {
        self.grab.is_some()
    }

    pub(crate) fn zoom(&self, picture: Extent, stage: Extent) -> Zoom {
        match self.fitting {
            Fitting::Fit => Zoom::fit(picture, stage),
            Fitting::Zoomed(zoom) => zoom,
        }
    }

    pub(crate) fn placed(&self, picture: Extent, stage: Rect) -> Rect {
        let zoom = self.zoom(picture, stage.extent());
        let width = zoom.of(picture.width);
        let height = zoom.of(picture.height);
        let middle = center(stage) + self.pan;
        Rect::new(
            middle.horizontal - width / 2,
            middle.vertical - height / 2,
            width,
            height,
        )
    }

    fn zoom_to(&mut self, next: Zoom, about: Point, picture: Extent, stage: Rect) {
        let before = self.zoom(picture, stage.extent());
        let offset = about - center(stage);
        let kept = offset - self.pan;
        self.pan = offset
            - Point::new(
                next.rescale(kept.horizontal, before),
                next.rescale(kept.vertical, before),
            );
        self.fitting = Fitting::Zoomed(next);
    }
}

impl App {
    fn viewed(&self) -> Option<Viewed> {
        let viewer = self.model.viewer.as_ref()?;
        let picture = match self.model.review.pictures.get(&viewer.link)? {
            Loading::Got(animation) => animation.size(),
            Loading::Asking | Loading::Failed(_) => return None,
        };
        let stage = self.ui.placement(ids::viewer_stage())?.rect;
        Some(Viewed { picture, stage })
    }

    pub(crate) fn viewer(&mut self, act: ViewerAct) {
        if let ViewerAct::Open(link) = act {
            self.model.viewer = Some(Viewer::new(link));
            return;
        }
        let measured = self.viewed();
        let Some(viewer) = self.model.viewer.as_mut() else {
            return;
        };
        match act {
            ViewerAct::Open(_) => {}
            ViewerAct::Close => self.model.viewer = None,
            ViewerAct::Fit => {
                viewer.fitting = Fitting::Fit;
                viewer.pan = Point::new(Px::ZERO, Px::ZERO);
            }
            ViewerAct::Actual => {
                if let Some(Viewed { picture, stage }) = measured {
                    viewer.zoom_to(Zoom::ACTUAL, center(stage) + viewer.pan, picture, stage);
                }
            }
            ViewerAct::Step(step, about) => {
                if let Some(Viewed { picture, stage }) = measured {
                    let now = viewer.zoom(picture, stage.extent());
                    let next = match step {
                        Step::In => now.times(Zoom::STEP_UP),
                        Step::Out => now.times(Zoom::STEP_DOWN),
                    };
                    let about = about.unwrap_or_else(|| center(stage));
                    viewer.zoom_to(next, about, picture, stage);
                }
            }
            ViewerAct::Pinch(pinch, about) => {
                if let Some(Viewed { picture, stage }) = measured {
                    let next = viewer.zoom(picture, stage.extent()).pinched(pinch);
                    viewer.zoom_to(next, about, picture, stage);
                }
            }
            ViewerAct::Pan(by) => viewer.pan = viewer.pan + by,
            ViewerAct::Drag(at) => {
                if let Some(grab) = viewer.grab {
                    viewer.pan = viewer.pan + (at - grab);
                }
                viewer.grab = Some(at);
            }
            ViewerAct::Release => viewer.grab = None,
            ViewerAct::Browser => {
                let link = viewer.link.clone();
                self.review(ReviewAct::OpenLink(link));
            }
        }
    }
}

#[cfg(test)]
mod tests;

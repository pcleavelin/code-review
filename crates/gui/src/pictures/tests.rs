use std::rc::Rc;
use std::time::Duration;

use ui::{Extent, Picture, PictureBytes, PictureKey, Px};

use super::{Animation, AnimationFrame};

fn frame(key: u64, millis: u64) -> AnimationFrame {
    AnimationFrame {
        picture: Rc::new(Picture {
            key: PictureKey::new(key),
            size: Extent::new(Px::new(1), Px::new(1)),
            rgba: PictureBytes::new(vec![0; 4]),
        }),
        shown_for: Duration::from_millis(millis),
    }
}

fn animation(frames: Vec<AnimationFrame>) -> Animation {
    Animation {
        frames,
        started_at: Duration::from_millis(1000),
        size: Extent::new(Px::new(1), Px::new(1)),
    }
}

fn shown_at(animation: &Animation, millis: u64) -> Option<(u64, Option<Duration>)> {
    animation
        .at(Duration::from_millis(millis))
        .map(|shown| (shown.picture.key.get(), shown.next))
}

#[test]
fn an_animation_shows_each_frame_for_its_time_from_when_it_started_and_loops() {
    let playing = animation(vec![frame(1, 300), frame(2, 50)]);
    let at = |millis: u64| Some(Duration::from_millis(millis));
    assert_eq!(shown_at(&playing, 1000), Some((1, at(1300))));
    assert_eq!(shown_at(&playing, 1299), Some((1, at(1300))));
    assert_eq!(shown_at(&playing, 1300), Some((2, at(1350))));
    assert_eq!(shown_at(&playing, 1360), Some((1, at(1650))));
}

#[test]
fn a_still_picture_never_asks_for_another_frame() {
    let still = animation(vec![frame(7, 0)]);
    assert_eq!(shown_at(&still, 5000), Some((7, None)));
}

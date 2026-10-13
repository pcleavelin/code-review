#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::string_slice,
    clippy::disallowed_methods,
    clippy::print_stderr,
    reason = "a test harness fails by panicking, builds fixtures on disk, runs the binary and prints each scale step's time"
)]

mod common;

use std::sync::Mutex;

static ONE_WINDOW: Mutex<()> = Mutex::new(());

macro_rules! tests {
    ($kind:ident: $($s:ident),* $(,)?) => {$(
        #[test]
        fn $s() {
            let _one = ONE_WINDOW.lock().unwrap_or_else(|error| error.into_inner());
            let name = concat!(stringify!($kind), "-", stringify!($s));
            let scenario = common::gui::$s();
            let output = common::gui::play(&common::binary(), name, &scenario);
            common::assert_clean(name, &output);
            common::gui::assert_shown(name, &scenario, &output);
        }
    )*};
}

gui_scenarios!(tests);

macro_rules! at_scale {
    ($kind:ident: $($s:ident),* $(,)?) => {$(
        #[test]
        fn $s() {
            let _one = ONE_WINDOW.lock().unwrap_or_else(|error| error.into_inner());
            let name = concat!(stringify!($kind), "-", stringify!($s));
            let scale = common::gui::$s();
            let output = common::gui::play_at_scale(&common::binary(), name, &scale);
            common::assert_clean(name, &output);
            common::gui::assert_shown(name, &scale.scenario, &output);
            common::gui::assert_frames(name, &scale, &output);
        }
    )*};
}

gui_scale_scenarios!(at_scale);

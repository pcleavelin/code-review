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

use common::gui::Scenario;
use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

const TRANSCRIPT_OPENED: &str = "0/3 seen";

const TRANSCRIPT: &[&[&str]] = &[
    &["prs"],
    &["open", "1"],
    &["discussion", "1"],
    &["changes", "1"],
    &["diff", "1"],
    &["next", "1"],
    &["seen", "1", "src/lib.rs"],
    &["comment", "1", "src/lib.rs", "10", "why square?"],
    &["comment", "1", "old.txt", "3", "keep this", "--old"],
    &["drafts", "1"],
    &["uncomment", "1", "1"],
    &["changes", "1"],
    &["submit", "1"],
];

fn base() -> PathBuf {
    PathBuf::from(env::var_os("REVIEW_BASE_BIN").expect("REVIEW_BASE_BIN=<old review binary>"))
}

fn kept_directory(name: &str) -> PathBuf {
    let dir = common::scratch("parity").join(name);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn same(what: &str, old: &str, new: &str, fails: &mut Vec<String>) {
    if old != new {
        let dir = kept_directory(what);
        fs::write(dir.join("old.txt"), old).unwrap();
        fs::write(dir.join("new.txt"), new).unwrap();
        fails.push(format!(
            "{what}: differs at line {} (see {})",
            common::first_difference(old, new) + 1,
            dir.display()
        ));
    }
}

fn screenshot_files(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut shots: Vec<(String, Vec<u8>)> = fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| {
                    (
                        entry.file_name().to_string_lossy().into_owned(),
                        fs::read(entry.path()).unwrap(),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    shots.sort();
    shots
}

struct Played {
    text: String,
    shots: Vec<(String, Vec<u8>)>,
}

fn play_gui(binary: &Path, name: &str, scenario: &Scenario, keep: &Path) -> Played {
    let err = common::gui::play(binary, name, scenario);
    let shots = screenshot_files(&common::gui::screenshots(name));
    fs::remove_dir_all(keep).ok();
    fs::create_dir_all(keep).unwrap();
    for (shot_name, bytes) in &shots {
        fs::write(keep.join(shot_name), bytes).unwrap();
    }
    Played {
        text: common::steady(&err),
        shots,
    }
}

fn compare_gui(name: &str, scenario: &Scenario, old: &Path, new: &Path, fails: &mut Vec<String>) {
    let played_old = play_gui(old, name, scenario, &kept_directory(name).join("old"));
    let played_new = play_gui(new, name, scenario, &kept_directory(name).join("new"));
    same(name, &played_old.text, &played_new.text, fails);
    for (build, played) in [("old", &played_old), ("new", &played_new)] {
        fails.extend(
            played
                .text
                .lines()
                .filter(|line| line.starts_with("script:") || line.contains("panicked at"))
                .map(|line| format!("{name}: the {build} build's script failed: {line}")),
        );
    }
    if played_old.shots.len() != played_new.shots.len()
        || played_old.shots.is_empty() && scenario.script.contains("shot ")
    {
        fails.push(format!(
            "{name}: {} screenshots old, {} new",
            played_old.shots.len(),
            played_new.shots.len()
        ));
    }
    for ((shot, old_bytes), (_, new_bytes)) in played_old.shots.iter().zip(&played_new.shots) {
        if old_bytes != new_bytes {
            fails.push(format!(
                "{name}: {shot} differs (both kept under {})",
                kept_directory(name).display()
            ));
        }
    }
}

fn transcript(binary: &Path) -> String {
    let fixture = common::fixture("parity-cli");
    TRANSCRIPT
        .iter()
        .fold(String::new(), |mut record, arguments| {
            writeln!(record, "$ review {}", arguments.join(" ")).ok();
            record.push_str(&common::review(binary, &fixture, arguments));
            record
        })
}

struct NamedScenario {
    name: &'static str,
    build: fn() -> Scenario,
}

macro_rules! gui_list {
    ($kind:ident: $($s:ident),* $(,)?) => {
        [$(NamedScenario {
            name: concat!(stringify!($kind), "-", stringify!($s)),
            build: common::$kind::$s,
        }),*]
    };
}

#[test]
#[ignore = "needs REVIEW_BASE_BIN; cargo xtask parity runs it"]
fn old_and_new_agree() {
    let (old, new) = (base(), common::binary());
    let mut fails = Vec::new();
    let only = env::var("REVIEW_PARITY_ONLY").unwrap_or_default();
    if "parity-cli".contains(only.as_str()) {
        let (old_text, new_text) = (transcript(&old), transcript(&new));
        if !new_text.contains(TRANSCRIPT_OPENED) {
            fails.push(format!(
                "parity-cli: the new build did not open the fixture's pull request:\n{new_text}"
            ));
        }
        same("parity-cli", &old_text, &new_text, &mut fails);
    }
    let gui = gui_scenarios!(gui_list);
    for scenario in gui.iter().filter(|scenario| scenario.name.contains(&only)) {
        compare_gui(scenario.name, &(scenario.build)(), &old, &new, &mut fails);
    }
    assert!(
        fails.is_empty(),
        "{} differences:\n{}",
        fails.len(),
        fails.join("\n")
    );
}

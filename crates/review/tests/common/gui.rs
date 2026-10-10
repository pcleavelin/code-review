use super::{fixture, play_script};
use std::path::{Path, PathBuf};

#[macro_export]
macro_rules! gui_scenarios {
    ($m:ident) => {
        $m! { gui: open, deleted }
    };
}

pub(crate) struct Scenario {
    pub(crate) script: &'static str,
    pub(crate) shows: &'static [&'static str],
}

const OPEN: &str = "idle\nclick-id pull/0\nidle\nwait 2\n";

pub(crate) fn open() -> Scenario {
    Scenario {
        script: "idle
wait 2
dump
shot {shots}/inbox.png
OPEN
dump
shot {shots}/review.png
click-id change/2
idle
wait 2
dump
shot {shots}/changed.png
quit
",
        shows: &[
            "DUMP entry owner/repo#1 New drafts=0 \"Square the inputs\"",
            "DUMP change old.txt Deleted +0 -400 Unseen",
            "DUMP change src/lib.rs Modified +1 -1 Unseen",
            "DUMP change notes.md Modified +1 -1 Unseen",
            "DUMP tab=Source focus=None file=Some(\"src/lib.rs\")",
        ],
    }
}

pub(crate) fn screenshots(name: &str) -> PathBuf {
    super::scratch(name).join("shots")
}

pub(crate) fn play(binary: &Path, name: &str, scenario: &Scenario) -> String {
    let script = scenario.script.replace("OPEN\n", OPEN);
    let mut lines: Vec<String> = script.lines().map(str::to_owned).collect();
    for line_index in 0..lines.len() {
        while let Some(open) = lines[line_index].find("<<") {
            let close = open + lines[line_index][open..].find(">>").expect("unclosed <<");
            let (prefix, offset) = match lines[line_index][open + 2..close].split_once('|') {
                Some((raw_prefix, raw_offset)) => {
                    (raw_prefix.to_owned(), Some(raw_offset.to_owned()))
                }
                None => (lines[line_index][open + 2..close].to_owned(), None),
            };
            let probe = lines[..line_index].join("\n") + "\ndump\nquit\n";
            let err = play_script(binary, &fixture(name), name, &probe);
            let (left, top, width, height) = last_rectangle(&err, &prefix).unwrap_or_else(|| {
                panic!(
                    "{name}: no rectangle for '{prefix}' before line {}\n{err}",
                    line_index + 1
                )
            });
            let (point_x, point_y) = match offset {
                Some(spec) => {
                    let (dx, dy) = spec.split_once(',').expect("dx,dy");
                    (
                        left + dx.trim().parse::<i32>().unwrap(),
                        top + dy.trim().parse::<i32>().unwrap(),
                    )
                }
                None => (left + width / 2, top + height / 2),
            };
            lines[line_index].replace_range(open..close + 2, &format!("{point_x} {point_y}"));
        }
    }
    play_script(binary, &fixture(name), name, &(lines.join("\n") + "\n"))
}

pub(crate) fn assert_shown(name: &str, scenario: &Scenario, output: &str) {
    let missing: Vec<&&str> = scenario
        .shows
        .iter()
        .filter(|wanted| !output.lines().any(|line| line.contains(**wanted)))
        .collect();
    assert!(
        missing.is_empty(),
        "{name}: the window never showed {missing:?} (screenshots in {})\n--- output in full ---\n{output}",
        screenshots(name).display()
    );
}

fn last_rectangle(stderr: &str, prefix: &str) -> Option<(i32, i32, i32, i32)> {
    let line = stderr.lines().rfind(|line| line.starts_with(prefix))?;
    let rect_text = line.split("Rect { ").nth(1)?;
    let field = |key: &str| -> Option<i32> {
        rect_text
            .split(&format!("{key}: "))
            .nth(1)?
            .split([',', ' ', '}'])
            .next()?
            .parse()
            .ok()
    };
    Some((field("x")?, field("y")?, field("w")?, field("h")?))
}

pub(crate) fn deleted() -> Scenario {
    Scenario {
        script: "OPEN
click-id change/1
idle
wait 2
rect draft-removed@1
rect draft-removed@300
dump
shot {shots}/top.png
click-id draft-removed@2
wait 1
text near the top
wait 2
click-id draft-add
wait 2
scroll source 100000
wait 3
rect draft-removed@1
rect draft-removed@400
click-id draft-removed@398
wait 1
click-id draft-removed@400 shift
wait 1
text keep these
wait 2
shot {shots}/editing.png
click-id draft-add
wait 2
dump
shot {shots}/bottom.png
scroll source 0
wait 3
rect draft-box/1
rect draft-box/2
shot {shots}/drafted.png
click-id tab@Review
wait 2
click-id review-draft/1
wait 3
rect draft-box/2
shot {shots}/jumped.png
quit
",
        shows: &[
            "DUMP tab=Source focus=None file=None gone=Some(\"old.txt\")",
            "DUMP rect draft-removed@1 = Some(",
            "DUMP rect draft-removed@300 = None",
            "DUMP rect draft-removed@1 = None",
            "DUMP rect draft-removed@400 = Some(",
            "DUMP draft 1 old.txt:2-2 Old reply=None \"near the top\"",
            "DUMP draft 2 old.txt:398-400 Old reply=None \"keep these\"",
            "DUMP rect draft-box/1 = Some(",
            "DUMP rect draft-box/2 = None",
            "DUMP rect draft-box/2 = Some(",
        ],
    }
}

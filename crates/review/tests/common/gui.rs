use super::{fixture, play_script};
use std::path::{Path, PathBuf};

#[macro_export]
macro_rules! gui_scenarios {
    ($m:ident) => {
        $m! { gui: open, deleted, split, copy, markdown, linked_file, working_copy, dropped, login, submissions, confirm_submit, submit_logged_out, requested, all_changes, stacked }
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
            "DUMP entry owner/repo#1 New drafts=0 requested=false \"Square the inputs\"",
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
rect draft-removed@old.txt:1
rect draft-removed@old.txt:300
dump
shot {shots}/top.png
click-id draft-removed@old.txt:2
wait 1
text near the top
wait 2
click-id draft-add
wait 2
mouse 800 400
wheel -5800
wait 3
rect draft-removed@old.txt:1
rect draft-removed@old.txt:400
click-id draft-removed@old.txt:398
wait 1
click-id draft-removed@old.txt:400 shift
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
            "DUMP rect draft-removed@old.txt:1 = Some(",
            "DUMP rect draft-removed@old.txt:300 = None",
            "DUMP rect draft-removed@old.txt:1 = None",
            "DUMP rect draft-removed@old.txt:400 = Some(",
            "DUMP draft 1 old.txt:2-2 Old reply=None \"near the top\"",
            "DUMP draft 2 old.txt:398-400 Old reply=None \"keep these\"",
            "DUMP rect draft-box/1 = Some(",
            "DUMP rect draft-box/2 = None",
            "DUMP rect draft-box/2 = Some(",
        ],
    }
}

pub(crate) fn split() -> Scenario {
    Scenario {
        script: "OPEN
click-id change/2
idle
wait 2
click-id layout
idle
wait 3
rect table@src/lib.rs:6
mouse <<DUMP rect table@src/lib.rs:6|569,56>>
wait 60
idle
wait 3
dump
shot {shots}/hover.png
hover-id table@src/lib.rs:6
swipe -400 0
wait 3
dump
shot {shots}/sideways.png
swipe 4000 0
wait 3
rect table@src/lib.rs:6
click <<DUMP rect table@src/lib.rs:6|569,56>> ctrl
wait 3
idle
dump
click-id tab@Commits
wait 2
click-id commit/0
idle
wait 3
click-id change/1
idle
wait 3
rect table@src/lib.rs:6
click <<DUMP rect table@src/lib.rs:6|569,56>> alt
wait 3
idle
dump
shot {shots}/commit.png
quit
",
        shows: &[
            "DUMP tip=Some(\"area\")",
            "DUMP across table off=",
            "DUMP tab=Source focus=Some(\"area\") file=Some(\"src/shapes.rs\")",
            "peek=Some(Sym(SymRef",
        ],
    }
}

pub(crate) fn copy() -> Scenario {
    Scenario {
        script: "OPEN
click-id change/2
idle
wait 3
click-id copy-lines
wait 2
click-id cmd
key v ctrl
wait 2
dump
shot {shots}/head.png
key u ctrl
click-id draft-removed@src/lib.rs:10
wait 2
key c ctrl
wait 2
click-id cmd
key v ctrl
wait 2
dump
key u ctrl
text kept
key a ctrl
key c ctrl
key u ctrl
key v ctrl
wait 2
dump
click-id layout
idle
wait 3
rect table@src/lib.rs:6
drag <<DUMP rect table@src/lib.rs:6|500,24>> <<DUMP rect table@src/lib.rs:6|500,56>>
wait 2
click-id copy-lines
wait 2
dump
shot {shots}/split.png
quit
",
        shows: &[
            "status=\"copied 1 line\"",
            "text=\"    add(area(value), add(value, value)) + add(area(1), area(2))",
            "text=\"    add(value, value) \"",
            "DUMP field Command caret=4 first=0 rows=1 text=\"kept\"",
            "status=\"copied 3 lines\"",
        ],
    }
}

pub(crate) fn markdown() -> Scenario {
    Scenario {
        script: "OPEN
hover-id md-link@description/0/5
wait 2
rect md-link@description/0/5
dump
shot {shots}/description.png
quit
",
        shows: &["DUMP rect md-link@description/0/5 = Some("],
    }
}

pub(crate) fn working_copy() -> Scenario {
    Scenario {
        script: "idle
wait 2
dump
OPEN
dump
quit
",
        shows: &["/local files=4", "/code-review/trees/1 files=3"],
    }
}

pub(crate) fn dropped() -> Scenario {
    Scenario {
        script: "OPEN
click-id cmd
text drop 1
key enter
wait 3
idle
dump
quit
",
        shows: &["open=None", "/local files=4"],
    }
}

pub(crate) fn login() -> Scenario {
    Scenario {
        script: "idle
wait 2
dump
shot {shots}/forges.png
click-id login@forgejo.test
wait 2
dump
text fjtoken123
dump
shot {shots}/paste.png
key enter
idle
wait 2
dump
shot {shots}/pasted.png
click-id cmd
text login gitlab.test
key enter
wait 3
dump
shot {shots}/code.png
pause 5000
idle
wait 2
dump
shot {shots}/logged-in.png
click-id logout@forgejo.test
idle
wait 2
dump
quit
",
        shows: &[
            "DUMP login sites=[forgejo.test=out] stage=none",
            "DUMP login sites=[forgejo.test=out] stage=forgejo.test paste page=Some(https://forgejo.test/user/settings/applications)",
            "field Token caret=10 first=0 rows=1 text=<10 hidden>",
            "DUMP login sites=[forgejo.test=in] stage=none",
            "stage=gitlab.test device code=ABCD-1234 page=https://gitlab.test/oauth/device",
            "DUMP login sites=[gitlab.test=in forgejo.test=in] stage=none",
            "DUMP login sites=[gitlab.test=in forgejo.test=out] stage=none",
        ],
    }
}

pub(crate) fn submissions() -> Scenario {
    Scenario {
        script: "OPEN
dump
shot {shots}/submissions.png
quit
",
        shows: &[
            "DUMP block alone issue-501",
            "DUMP block submitted review-20 dave RequestChanges [300 301]",
            "DUMP block submitted review-22 erin Approve []",
        ],
    }
}

pub(crate) fn confirm_submit() -> Scenario {
    Scenario {
        script: "idle
wait 2
click-id login@forgejo.test
wait 2
text fjtoken123
key enter
idle
wait 2
OPEN
dump
shot {shots}/rights.png
hover-id submit-changes
wait 80
dump
shot {shots}/refused.png
click-id field@summary
text Looks right.
wait 2
click-id submit-approve
wait 2
dump
shot {shots}/confirm.png
click-id submit-cancel
wait 2
absent submit-confirm
dump
click-id submit-approve
wait 2
click-id submit-confirm
idle
wait 2
dump
shot {shots}/sent.png
quit
",
        shows: &[
            "DUMP rights Some(Ok(Rights { comment: None, approve: None, request_changes: None }))",
            "tip=Some(\"there are no draft comments and no summary to send\")",
            "submitting=Asking(Approve)",
            "status=\"approved with 0 inline comments\"",
        ],
    }
}

pub(crate) fn submit_logged_out() -> Scenario {
    Scenario {
        script: "OPEN
hover-id submit-approve
wait 80
dump
shot {shots}/logged-out.png
quit
",
        shows: &[
            "DUMP rights Some(Err(Label(\"submitting to forgejo.test needs a login: `review login forgejo.test`, or set FORGEJO_TOKEN or GITEA_TOKEN\")))",
            "tip=Some(\"submitting to forgejo.test needs a login: `review login forgejo.test`, or set FORGEJO_TOKEN or GITEA_TOKEN\")",
        ],
    }
}

pub(crate) fn linked_file() -> Scenario {
    Scenario {
        script: "OPEN
hover-id md-source@description/0/7
wait 60
shot {shots}/tip.png
click-id md-source@description/0/7
wait 3
idle
wait 2
dump
rect file-header@src/lib.rs
shot {shots}/head.png
click-id link-open@src/lib.rs
wait 2
dump
click-id tab@Review
wait 2
click-id md-source@description/1/8
wait 3
idle
wait 2
dump
shot {shots}/main.png
click-id tab@Review
wait 2
click-id md-link@description/2/9
wait 2
dump
quit
",
        shows: &[
            "file=Some(\"src/lib.rs\") gone=None sel=Some((4, 6))",
            "DUMP rect file-header@src/lib.rs = Some(Rect { x: 361, y: 96,",
            "status=\"src/lib.rs from the link\"",
            "status=\"opened https://forgejo.test/owner/repo/src/commit/",
            "/src/lib.rs#L5-L7\"",
            "file=Some(\"src/shapes.rs\") gone=None sel=Some((1, 1))",
            "status=\"src/shapes.rs from the link, shown at the head of the pull request: the link points at main\"",
            "status=\"opened https://forgejo.test/owner/repo/src/branch/feature/gone.rs\"",
        ],
    }
}

pub(crate) fn requested() -> Scenario {
    Scenario {
        script: "idle
wait 2
dump
shot {shots}/out.png
click-id inbox-requested
wait 2
rect pull/0
dump
shot {shots}/out-on.png
click-id login@forgejo.test
wait 2
text fjtoken123
key enter
idle
wait 2
rect pull/0
rect pull/1
dump
shot {shots}/in-on.png
click-id inbox-requested
wait 2
rect pull/1
dump
shot {shots}/in-off.png
rect inbox-requested
mouse <<DUMP rect inbox-requested>>
wait 2
click <<DUMP rect inbox-requested>>
wait 1
click <<DUMP rect inbox-requested>>
wait 1
click <<DUMP rect inbox-requested>>
wait 2
rect inbox-requested
absent pull/1
rect pull/0
dump
quit
",
        shows: &[
            "DUMP requests chip=\"requested: log in\" answered=0 pulls=[] unanswered=[owner/repo:LogIn:\"log in to see the review requests on forgejo.test",
            "requested=You",
            "DUMP rect pull/0 = None",
            "DUMP requests chip=\"requested 1\" answered=1 pulls=[owner/repo#1] unanswered=[]",
            "DUMP entry owner/repo#1 New drafts=0 requested=true \"Square the inputs\"",
            "DUMP entry owner/repo#2 New drafts=0 requested=false \"Rename the notes\"",
            "DUMP rect pull/0 = Some(",
            "DUMP rect pull/1 = None",
            "DUMP rect pull/1 = Some(",
        ],
    }
}

pub(crate) fn all_changes() -> Scenario {
    Scenario {
        script: "OPEN
click-id tab@Commits
wait 2
click-id commit/0
idle
wait 3
click-id change/0
idle
wait 3
dump
click-id all-changes@changes
wait 3
hover-id next-unseen
dump
shot {shots}/from-changes.png
click-id tab@Commits
wait 2
click-id commit/0
idle
wait 3
click-id change/0
idle
wait 3
click-id tab@Commits
wait 2
click-id all-changes
wait 3
hover-id next-unseen
dump
click-id tab@Commits
wait 2
click-id commit/0
idle
wait 3
click-id change/0
idle
wait 3
click-id all-changes@source
wait 3
hover-id next-unseen
dump
quit
",
        shows: &[
            "DUMP diffs layout=Unified commit=Some(",
            "DUMP diffs layout=Unified commit=None",
        ],
    }
}

pub(crate) fn stacked() -> Scenario {
    Scenario {
        script: "OPEN
click-id change/2
idle
wait 3
rect file-header@src/lib.rs
rect source
dump
shot {shots}/last.png
mouse 800 400
wheel 300
wait 3
dump
shot {shots}/scrolled.png
click-id change/0
idle
wait 3
rect file-header@notes.md
rect file-header@old.txt
click-id collapse@notes.md
wait 3
rect file-header@old.txt
rect draft-removed@old.txt:1
click-id collapse@old.txt
wait 3
absent draft-removed@old.txt:1
rect file-header@src/lib.rs
click-id seen@notes.md
wait 2
dump
shot {shots}/collapsed.png
quit
",
        shows: &[
            "DUMP rect file-header@src/lib.rs = Some(Rect { x: 361, y: 96,",
            "DUMP tab=Source focus=None file=Some(\"src/lib.rs\")",
            "DUMP tab=Source focus=None file=None gone=Some(\"old.txt\")",
            "DUMP rect file-header@notes.md = Some(Rect { x: 361, y: 96,",
            "DUMP rect draft-removed@old.txt:1 = Some(",
            "DUMP change notes.md Modified +1 -1 Seen",
            "DUMP change notes.md Modified +1 -1 Unseen generated",
            "DUMP rect file-header@old.txt = Some(Rect { x: 361, y: 126,",
            "DUMP rect file-header@old.txt = Some(Rect { x: 361, y: 274,",
        ],
    }
}

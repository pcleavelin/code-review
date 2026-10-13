use std::fmt::Write as _;
use std::ops::Range;

pub(crate) const MODIFIED: usize = 1500;
pub(crate) const DELETED: usize = 300;
pub(crate) const RENAMED: usize = 300;
pub(crate) const ADDED: usize = 700;
pub(crate) const GENERATED: usize = 200;
pub(crate) const TABLES: usize = 3;
pub(crate) const CHANGED: usize = MODIFIED + DELETED + RENAMED + ADDED + GENERATED + TABLES;

const UNTOUCHED: usize = 500;
const LINES: usize = 60;
const LONG_EVERY: usize = 100;
const LONG_LINES: usize = 800;
const LONG_HUNK: Range<usize> = 100..700;
const SHORT_HUNKS: [usize; 2] = [3, 40];
const TABLE_LINES: usize = 30_000;
const TABLE_HUNK_EVERY: usize = 1000;
const DEEPEST: usize = 12;
const STAMP: &str = "test <test@example.com> 1767225600 +0000";

pub(crate) fn first_modified() -> String {
    deep("modified", 0)
}

pub(crate) fn first_modified_lines() -> usize {
    LONG_HUNK.len() + SHORT_HUNKS.len()
}

fn deep(kind: &str, number: usize) -> String {
    let mut path = format!("src/{kind}/area_{}/", number % 17);
    for level in 0..number % DEEPEST {
        write!(path, "level_{level}/").ok();
    }
    write!(path, "file_{number}.rs").ok();
    path
}

fn code(kind: &str, number: usize, changed: impl Fn(usize) -> bool) -> String {
    let count = if number.is_multiple_of(LONG_EVERY) {
        LONG_LINES
    } else {
        LINES
    };
    (1..=count).fold(String::new(), |mut text, line| {
        let tail = if changed(line) { " + 1" } else { "" };
        writeln!(
            text,
            "pub fn {kind}_{number}_{line}() -> u64 {{ {line}{tail} }}"
        )
        .ok();
        text
    })
}

fn base_code(kind: &str, number: usize) -> String {
    code(kind, number, |_| false)
}

fn modified_code(number: usize) -> String {
    let long = number.is_multiple_of(LONG_EVERY);
    code("modified", number, |line| {
        SHORT_HUNKS.contains(&line) || (long && LONG_HUNK.contains(&line))
    })
}

fn added_code(number: usize) -> String {
    (1..=LINES).fold(String::new(), |mut text, line| {
        writeln!(text, "pub fn added_{number}_{line}() -> u64 {{ {line} }}").ok();
        text
    })
}

fn generated_json(number: usize) -> String {
    let fields = (1..=LINES).fold(String::new(), |mut text, line| {
        writeln!(text, "  \"field_{number}_{line}\": {line},").ok();
        text
    });
    format!("{{\n{fields}  \"end\": 0\n}}\n")
}

fn table(number: usize, changed: bool) -> String {
    (1..=TABLE_LINES).fold(String::new(), |mut text, line| {
        let mark = if changed && line.is_multiple_of(TABLE_HUNK_EVERY) {
            " changed"
        } else {
            ""
        };
        writeln!(
            text,
            "row {line} of table {number}: value {}{mark}",
            line * 7 % 1000
        )
        .ok();
        text
    })
}

fn file(stream: &mut String, path: &str, text: &str) {
    write!(
        stream,
        "M 100644 inline {path}\ndata {}\n{text}\n",
        text.len()
    )
    .ok();
}

fn commit(stream: &mut String, branch: &str, mark: usize, message: &str) {
    write!(
        stream,
        "commit refs/heads/{branch}\nmark :{mark}\nauthor {STAMP}\ncommitter {STAMP}\ndata {}\n{message}\n",
        message.len()
    )
    .ok();
}

pub(crate) fn history() -> String {
    let mut stream = String::new();
    commit(&mut stream, "main", 1, "base");
    file(
        &mut stream,
        ".gitattributes",
        "generated/** linguist-generated\n",
    );
    for (kind, count) in [
        ("modified", MODIFIED),
        ("deleted", DELETED),
        ("renamed", RENAMED),
        ("untouched", UNTOUCHED),
    ] {
        for number in 0..count {
            file(&mut stream, &deep(kind, number), &base_code(kind, number));
        }
    }
    for number in 0..TABLES {
        file(
            &mut stream,
            &format!("assets/table_{number}.txt"),
            &table(number, false),
        );
    }
    commit(&mut stream, "feature", 2, "Rework every module");
    stream.push_str("from :1\n");
    for number in 0..MODIFIED {
        file(
            &mut stream,
            &deep("modified", number),
            &modified_code(number),
        );
    }
    for number in 0..DELETED {
        writeln!(stream, "D {}", deep("deleted", number)).ok();
    }
    for number in 0..RENAMED {
        writeln!(
            stream,
            "R {} {}",
            deep("renamed", number),
            deep("moved", number)
        )
        .ok();
    }
    for number in 0..ADDED {
        file(&mut stream, &deep("added", number), &added_code(number));
    }
    for number in 0..GENERATED {
        file(
            &mut stream,
            &format!("generated/schema_{number}.json"),
            &generated_json(number),
        );
    }
    for number in 0..TABLES {
        file(
            &mut stream,
            &format!("assets/table_{number}.txt"),
            &table(number, true),
        );
    }
    stream
}

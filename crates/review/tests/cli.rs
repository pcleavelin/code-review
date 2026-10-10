#![expect(
    clippy::unwrap_used,
    clippy::disallowed_methods,
    reason = "a test harness fails by panicking, builds fixtures on disk and runs the binary"
)]

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{self, Command};

macro_rules! scenarios {
    ($($scenario:ident),* $(,)?) => {$(
        #[test]
        fn $scenario() {
            scenario::$scenario(&Fixture::new(stringify!($scenario)));
        }
    )*};
}

scenarios!(offline_review, refusals, pinning);

struct Fixture {
    forge: PathBuf,
    local: PathBuf,
}

struct Ran {
    stdout: String,
    stderr: String,
    code: Option<i32>,
}

fn git(directory: &Path, arguments: &[&str]) {
    let output = Command::new("git")
        .args(["-c", "user.name=test", "-c", "user.email=test@example.com"])
        .args(arguments)
        .current_dir(directory)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {arguments:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn put(directory: &Path, name: &str, text: &str) {
    fs::write(directory.join(name), text).unwrap();
}

impl Fixture {
    fn new(name: &str) -> Self {
        let base = env::temp_dir().join(format!("review-cli-{name}-{}", process::id()));
        drop(fs::remove_dir_all(&base));
        let forge = base.join("forge");
        let local = base.join("local");
        fs::create_dir_all(&forge).unwrap();
        git(&forge, &["init", "-q", "-b", "main"]);
        put(&forge, "a.txt", "one\ntwo\nthree\n");
        put(&forge, "gone.txt", "bye\n");
        git(&forge, &["add", "-A"]);
        git(&forge, &["commit", "-q", "-m", "base"]);
        git(&forge, &["checkout", "-q", "-b", "feature"]);
        put(&forge, "a.txt", "one\nTWO\nthree\n");
        put(&forge, "new.txt", "hi\n");
        git(&forge, &["rm", "-q", "gone.txt"]);
        git(&forge, &["add", "-A"]);
        git(&forge, &["commit", "-q", "-m", "change"]);
        git(&forge, &["checkout", "-q", "main"]);
        git(
            &base,
            &[
                "clone",
                "-q",
                forge.to_str().unwrap(),
                local.to_str().unwrap(),
            ],
        );
        git(
            &local,
            &[
                "fetch",
                "-q",
                "origin",
                "+refs/heads/feature:refs/code-review/1/head",
                "+refs/heads/main:refs/code-review/1/base",
            ],
        );
        Self { forge, local }
    }

    fn run(&self, arguments: &[&str]) -> Ran {
        self.run_in(&self.local, arguments)
    }

    fn run_in(&self, directory: &Path, arguments: &[&str]) -> Ran {
        let base = self.forge.parent().unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_review"))
            .args(arguments)
            .current_dir(directory)
            .env("REVIEW_PINS", base.join("pins"))
            .env("REVIEW_CLONES", base.join("clones"))
            .output()
            .unwrap();
        Ran {
            stdout: String::from_utf8_lossy(&output.stdout).replace('\r', ""),
            stderr: String::from_utf8_lossy(&output.stderr).replace('\r', ""),
            code: output.status.code(),
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(base) = self.forge.parent() {
            drop(fs::remove_dir_all(base));
        }
    }
}

mod scenario {
    use super::Fixture;

    pub(super) fn offline_review(fixture: &Fixture) {
        let changes = fixture.run(&["changes", "1"]);
        assert_eq!(changes.code, Some(0), "{}", changes.stderr);
        for path in ["a.txt", "gone.txt", "new.txt"] {
            assert!(changes.stdout.contains(path), "{}", changes.stdout);
        }
        assert!(changes.stdout.contains("0/3 seen"), "{}", changes.stdout);

        let seen = fixture.run(&["seen", "1", "a.txt"]);
        assert_eq!(seen.stdout, "[x] a.txt\n");
        assert!(fixture.run(&["changes", "1"]).stdout.contains("1/3 seen"));

        let diff = fixture.run(&["diff", "1", "a.txt"]);
        assert!(diff.stdout.contains("    2       -two"), "{}", diff.stdout);
        assert!(diff.stdout.contains("          2 +TWO"), "{}", diff.stdout);

        let new_side = fixture.run(&["comment", "1", "a.txt", "2", "why the capitals?"]);
        assert_eq!(
            new_side.stdout,
            "draft 1  a.txt:2 (new)  why the capitals?\n"
        );
        let old_side = fixture.run(&["comment", "1", "a.txt", "2", "was fine", "--old"]);
        assert!(old_side.stdout.starts_with("draft 2  a.txt:2 (old)"));
        let drafted = fixture.run(&["diff", "1", "a.txt"]);
        assert!(drafted.stdout.contains(">> draft 1: why the capitals?"));
        assert!(drafted.stdout.contains(">> draft 2: was fine"));

        let reply = fixture.run(&["comment", "1", "a.txt", "2", "seconded", "--reply", "4711"]);
        assert!(
            reply.stdout.contains("answering 4711  seconded"),
            "{}",
            reply.stdout
        );
        fixture.run(&["uncomment", "1", "3"]);

        let removed = fixture.run(&["uncomment", "1", "1"]);
        assert_eq!(removed.stdout, "removed draft 1\n");
        let drafts = fixture.run(&["drafts", "1"]);
        assert_eq!(drafts.stdout, "draft 2  a.txt:2 (old)  was fine\n");

        let next = fixture.run(&["next", "1"]);
        assert!(
            next.stdout.starts_with("[ ] gone.txt (deleted"),
            "{}",
            next.stdout
        );
        fixture.run(&["seen", "1", "--all"]);
        let done = fixture.run(&["next", "1"]);
        assert!(done.stdout.starts_with("every changed file is seen"));

        let unseen = fixture.run(&["unseen", "1", "new.txt"]);
        assert_eq!(unseen.stdout, "[ ] new.txt\n");
    }

    pub(super) fn refusals(fixture: &Fixture) {
        let outside = fixture.run(&["comment", "1", "a.txt", "9", "nowhere"]);
        assert_eq!(outside.code, Some(2));
        assert!(
            outside.stderr.contains("is not in the diff"),
            "{}",
            outside.stderr
        );
        let unchanged = fixture.run(&["seen", "1", "missing.txt"]);
        assert!(unchanged.stderr.contains("is not changed by #1"));
        let unopened = fixture.run(&["changes", "7"]);
        assert!(unopened.stderr.contains("has not been fetched yet"));
        let blank = fixture.run(&["comment", "1", "a.txt", "2", "  "]);
        assert!(blank.stderr.contains("the comment is empty"));
        let unknown = fixture.run(&["uncomment", "1", "42"]);
        assert!(unknown.stderr.contains("no draft comment 42"));
        let help = fixture.run(&["help"]);
        assert!(help.stdout.contains("[repository#]number"));
        assert!(help.stdout.contains("submit"));
        let both = fixture.run(&["submit", "1", "--approve", "--request-changes"]);
        assert_eq!(both.code, Some(2));
        let nothing = fixture.run(&["submit", "1"]);
        assert!(nothing.stderr.contains("no draft comments and no summary"));
        let listed = fixture.run(&["prs"]);
        assert!(
            listed.stdout.contains("cannot read a host and repository"),
            "{}",
            listed.stdout
        );
        for asked in [&["open", "1"][..], &["discussion", "1"]] {
            let local = fixture.run(asked);
            assert_eq!(local.code, Some(2));
            assert!(
                local.stderr.contains("cannot read a host and repository"),
                "{}",
                local.stderr
            );
        }
        let threadless = fixture.run(&["comment", "1", "a.txt", "2", "yes", "--reply", "a b"]);
        assert!(threadless.stderr.contains("is not a thread id"));
        let typo = fixture.run(&["bogus"]);
        assert_eq!(typo.code, Some(2));
    }

    pub(super) fn pinning(fixture: &Fixture) {
        let base = fixture.forge.parent().unwrap().to_path_buf();
        let nowhere = fixture.run_in(&base, &["changes", "1"]);
        assert!(
            nowhere.stderr.contains("pin one with"),
            "{}",
            nowhere.stderr
        );
        let unpinned = fixture.run(&["pins"]);
        assert!(
            unpinned
                .stdout
                .contains("(the current directory, not pinned)")
        );
        let pinned = fixture.run(&["pin", "."]);
        assert!(pinned.stdout.starts_with("pinned "), "{}", pinned.stderr);
        let again = fixture.run(&["pin", "."]);
        assert!(again.stderr.contains("is pinned already"));
        let elsewhere = fixture.run_in(&base, &["changes", "forge#1"]);
        assert!(
            elsewhere.stdout.contains("0/3 seen"),
            "{}",
            elsewhere.stderr
        );
        let pinned_only = fixture.run_in(&base, &["changes", "1"]);
        assert!(
            pinned_only.stdout.contains("0/3 seen"),
            "{}",
            pinned_only.stderr
        );
        let wrong = fixture.run_in(&base, &["changes", "other#1"]);
        assert!(
            wrong
                .stderr
                .contains("no pinned repository is called other")
        );
        let listed = fixture.run_in(&base, &["pins"]);
        assert!(listed.stdout.contains("local"), "{}", listed.stdout);
        let gone = fixture.run_in(&base, &["unpin", "forge"]);
        assert!(gone.stdout.starts_with("unpinned"), "{}", gone.stderr);
        let empty = fixture.run_in(&base, &["pins"]);
        assert!(empty.stdout.contains("nothing pinned"));
    }
}

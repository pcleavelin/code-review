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

use std::env;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{self, Command, Output};
use std::thread;
use std::time::{Duration, Instant};

use common::{COMMAND_DEADLINE, Deadline};

macro_rules! scenarios {
    ($($scenario:ident),* $(,)?) => {$(
        #[test]
        fn $scenario() {
            scenario::$scenario(&Fixture::new(stringify!($scenario)));
        }
    )*};
}

macro_rules! at_scale {
    ($($scenario:ident),* $(,)?) => {$(
        #[test]
        fn $scenario() {
            let fixture = common::large_fixture(stringify!($scenario));
            scale::$scenario(&fixture);
            fixture.remove();
        }
    )*};
}

at_scale!(scale_inbox, scale_review);

scenarios!(
    jira_card,
    offline_review,
    refusals,
    pinning,
    navigation,
    since_seen,
    login,
    stalls,
    discussion,
    submit_refused
);

struct Fixture {
    name: String,
    forge: PathBuf,
    local: PathBuf,
}

struct Ran {
    stdout: String,
    stderr: String,
    code: Option<i32>,
}

fn run_git(directory: &Path, arguments: &[&str]) -> Output {
    let mut command = Command::new("git");
    command
        .args(["-c", "user.name=test", "-c", "user.email=test@example.com"])
        .args(arguments)
        .current_dir(directory);
    common::run_within(
        &mut command,
        b"",
        &Deadline {
            scenario: &directory.display().to_string(),
            step: &format!("git {}", arguments.join(" ")),
            within: COMMAND_DEADLINE,
        },
    )
}

fn git(directory: &Path, arguments: &[&str]) {
    let output = run_git(directory, arguments);
    assert!(
        output.status.success(),
        "git {arguments:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn git_output(directory: &Path, arguments: &[&str]) -> String {
    let output = run_git(directory, arguments);
    assert!(output.status.success(), "git {arguments:?}");
    String::from_utf8_lossy(&output.stdout).into_owned()
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
        put(
            &forge,
            "tools.py",
            "def helper(x):\n    return x + 1\n\n\ndef run():\n    return helper(2)\n\n\ndef main():\n    run()\n",
        );
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
        Self {
            name: name.to_owned(),
            forge,
            local,
        }
    }

    fn bounded(&self, command: &mut Command, arguments: &[&str], input: &[u8]) -> Output {
        common::run_within(
            command,
            input,
            &Deadline {
                scenario: &self.name,
                step: &format!("review {}", arguments.join(" ")),
                within: COMMAND_DEADLINE,
            },
        )
    }

    fn run(&self, arguments: &[&str]) -> Ran {
        self.run_in(&self.local, arguments)
    }

    fn run_in(&self, directory: &Path, arguments: &[&str]) -> Ran {
        self.run_with(directory, arguments, None)
    }

    fn run_without_servers(&self, arguments: &[&str]) -> Ran {
        let paths = env::var_os("PATH").unwrap_or_default();
        let git = env::split_paths(&paths)
            .find(|directory| {
                directory.join("git").is_file() || directory.join("git.exe").is_file()
            })
            .unwrap();
        self.run_with(&self.local, arguments, Some(git))
    }

    fn run_with(&self, directory: &Path, arguments: &[&str], path: Option<PathBuf>) -> Ran {
        let base = self.forge.parent().unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_review"));
        command
            .args(arguments)
            .current_dir(directory)
            .env("REVIEW_PINS", base.join("pins"))
            .env("REVIEW_CLONES", base.join("clones"));
        if let Some(path) = path {
            command.env("PATH", path);
        }
        let output = self.bounded(&mut command, arguments, b"");
        Ran {
            stdout: String::from_utf8_lossy(&output.stdout).replace('\r', ""),
            stderr: String::from_utf8_lossy(&output.stderr).replace('\r', ""),
            code: output.status.code(),
        }
    }
}

const FAKE_CURL: &str = r#"#!/bin/sh
PATH=/usr/bin:/bin
here=$(dirname "$0")
api="$here/api"
printf '%s\n' "$*" >> "$here/curl.argv"
config=$(cat)
url=$(printf '%s\n' "$config" | sed -n 's/^url = "\(.*\)"$/\1/p')
host=${url#https://}; host=${host%%/*}
if [ -f "$api/stall-$host" ]; then echo "curl: (28) Operation timed out after 30001 milliseconds with 0 bytes received" >&2; exit 28; fi
status=200
case "$url" in
*/login/device/code|*/oauth/authorize_device) body=$(cat "$api/device.json");;
*/login/oauth/access_token|*/oauth/token)
  case "$config" in
  *grant_type=refresh_token*) body=$(cat "$api/refresh.json");;
  *) count=$(( $(cat "$api/polls" 2>/dev/null || echo 0) + 1 )); echo "$count" > "$api/polls"
     body=$(cat "$api/poll-$count.json"); status=$(cat "$api/poll-$count.status");;
  esac;;
*/api/v1/*)
  rest=${url#https://*/api/v1/}; rest=${rest%%\?*}
  file="$api/$(printf '%s' "$rest" | tr '/' '-').json"
  if [ -f "$file" ]; then body=$(cat "$file"); else body='{"message":"not found"}'; status=404; fi;;
*/rest/api/2/myself)
  case "$config" in
  *"Authorization: Basic $(printf %s "$(cat "$api/jira-login" 2>/dev/null)" | base64)\""*) body='{"displayName":"Ann Smith"}';;
  *) body=''; status=401;;
  esac;;
*/rest/api/2/issue/*)
  key=${url#*/rest/api/2/issue/}; key=${key%%\?*}
  case "$config" in
  *"Authorization: Basic $(printf %s "$(cat "$api/jira-login" 2>/dev/null)" | base64)\""*)
    if [ -f "$api/jira-$key.json" ]; then body=$(cat "$api/jira-$key.json"); else body='{"errorMessages":["Issue does not exist"]}'; status=404; fi;;
  *) body=''; status=401;;
  esac;;
*/api/v4/user|*/merge_requests*)
  case "$config" in
  *"Authorization: Bearer $(cat "$api/token")\""*)
    case "$url" in
    */api/v4/user) body='{"id":77,"username":"ann"}';;
    *reviewer_id=77*) body=$(cat "$api/requested.json" 2>/dev/null || echo '[]');;
    *) body=$(cat "$api/merge_requests.json" 2>/dev/null || echo '[]');;
    esac;;
  *) body='{"message":"401 Unauthorized"}'; status=401;;
  esac;;
*) body='{"message":"not found"}'; status=404;;
esac
printf '%s\n\n%s' "$body" "$status"
"#;

const FAKE_SECURITY: &str = r#"#!/bin/sh
PATH=/usr/bin:/bin
here=$(dirname "$0")
store="$here/keychain"
mkdir -p "$store"
printf '%s\n' "$*" >> "$here/keychain.argv"
[ -f "$here/keychain.stall" ] && exec sleep 3600
case "$1" in
-i) line=$(cat)
    account=$(printf '%s' "$line" | sed -n 's/.* -a "\([^"]*\)".*/\1/p')
    secret=$(printf '%s' "$line" | sed -n 's/.* -w "\([^"]*\)".*/\1/p')
    printf '%s' "$secret" > "$store/$account";;
find-generic-password) [ -f "$store/$5" ] || { echo "security: The specified item could not be found in the keychain." >&2; exit 44; }
    cat "$store/$5"; echo;;
delete-generic-password) [ -f "$store/$5" ] || exit 44; rm "$store/$5";;
*) exit 1;;
esac
"#;

const FAKE_SECRET_TOOL: &str = r#"#!/bin/sh
PATH=/usr/bin:/bin
here=$(dirname "$0")
store="$here/keychain"
mkdir -p "$store"
printf '%s\n' "$*" >> "$here/keychain.argv"
case "$1" in
store) cat > "$store/$6";;
lookup) [ -f "$store/$5" ] || exit 1; cat "$store/$5";;
clear) rm -f "$store/$5";;
*) exit 1;;
esac
"#;

const FAKE_OPENER: &str = r#"#!/bin/sh
PATH=/usr/bin:/bin
printf '%s\n' "$1" >> "$(dirname "$0")/opened"
"#;

fn executable(file: &Path, text: &str) {
    fs::write(file, text).unwrap();
    #[cfg(unix)]
    fs::set_permissions(file, fs::Permissions::from_mode(0o755)).unwrap();
}

struct FakeForge {
    tools: PathBuf,
    forges: PathBuf,
}

impl FakeForge {
    fn new(fixture: &Fixture) -> Self {
        let base = fixture.forge.parent().unwrap();
        let tools = base.join("tools");
        fs::create_dir_all(tools.join("api")).unwrap();
        executable(&tools.join("curl"), FAKE_CURL);
        executable(&tools.join("security"), FAKE_SECURITY);
        executable(&tools.join("secret-tool"), FAKE_SECRET_TOOL);
        executable(&tools.join("open"), FAKE_OPENER);
        executable(&tools.join("xdg-open"), FAKE_OPENER);
        Self {
            forges: base.join("forges"),
            tools,
        }
    }

    fn answer(&self, name: &str, text: &str) {
        fs::write(self.tools.join("api").join(name), text).unwrap();
    }

    fn polls(&self, answers: &[(&str, u16)]) {
        drop(fs::remove_file(self.tools.join("api").join("polls")));
        for (number, (body, status)) in answers.iter().enumerate() {
            self.answer(&format!("poll-{}.json", number + 1), body);
            self.answer(&format!("poll-{}.status", number + 1), &status.to_string());
        }
    }

    fn read(&self, name: &str) -> String {
        fs::read_to_string(self.tools.join(name)).unwrap_or_default()
    }

    fn read_when_written(&self, name: &str) -> String {
        let start = Instant::now();
        while !self.tools.join(name).exists() && start.elapsed() < Duration::from_secs(10) {
            thread::sleep(Duration::from_millis(20));
        }
        self.read(name)
    }

    fn store(&self, account: &str, secret: &str) {
        fs::create_dir_all(self.tools.join("keychain")).unwrap();
        fs::write(self.tools.join("keychain").join(account), secret).unwrap();
    }

    fn run(
        &self,
        fixture: &Fixture,
        arguments: &[&str],
        variables: &[(&str, &str)],
        input: &str,
    ) -> Ran {
        let base = fixture.forge.parent().unwrap();
        let paths = env::var_os("PATH").unwrap_or_default();
        let git = env::split_paths(&paths)
            .find(|directory| directory.join("git").is_file())
            .unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_review"));
        command
            .args(arguments)
            .current_dir(&fixture.local)
            .env("REVIEW_PINS", base.join("pins"))
            .env("REVIEW_CLONES", base.join("clones"))
            .env("REVIEW_FORGES", &self.forges)
            .env_remove("REVIEW_KEYCHAIN")
            .env_remove("REVIEW_FORGE")
            .env_remove("GITLAB_TOKEN")
            .env_remove("FORGEJO_TOKEN")
            .env_remove("GITEA_TOKEN")
            .env("PATH", env::join_paths([self.tools.clone(), git]).unwrap())
            .envs(variables.iter().copied());
        let output = fixture.bounded(&mut command, arguments, input.as_bytes());
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
    use std::fs;

    use super::{FakeForge, Fixture, Ran, git, git_output};

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

        let edited = fixture.run(&["edit", "1", "2", "was fine as two"]);
        assert_eq!(edited.stdout, "draft 2  a.txt:2 (old)  was fine as two\n");
        let blank = fixture.run(&["edit", "1", "2", "  "]);
        assert_eq!(blank.code, Some(2), "{}", blank.stderr);
        let missing = fixture.run(&["edit", "1", "9", "nothing here"]);
        assert_eq!(missing.code, Some(2), "{}", missing.stderr);
        assert!(missing.stderr.contains('9'), "{}", missing.stderr);
        let edit_again = fixture.run(&["edit", "1", "2", "was fine"]);
        assert_eq!(edit_again.code, Some(0), "{}", edit_again.stderr);

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

        let tree = fixture.local.join(".git/code-review/trees/1");
        git(
            &fixture.local,
            &[
                "worktree",
                "add",
                "-q",
                "--detach",
                tree.to_str().unwrap(),
                "refs/code-review/1/head",
            ],
        );
        fs::write(tree.join("built.o"), "untracked").unwrap();
        let dropped = fixture.run(&["drop", "1"]);
        assert_eq!(dropped.code, Some(0), "{}", dropped.stderr);
        assert!(
            dropped.stdout.starts_with("dropped #1: removed "),
            "{}",
            dropped.stdout
        );
        assert!(!tree.exists());
        assert!(!fixture.local.join(".git/worktrees/1").exists());
        let gone = fixture.run(&["changes", "1"]);
        assert!(gone.stderr.contains("has not been fetched yet"));
        let kept = fixture.run(&["drafts", "1"]);
        assert_eq!(kept.stdout, "draft 2  a.txt:2 (old)  was fine\n");
        let again = fixture.run(&["drop", "1"]);
        assert_eq!(again.code, Some(2));
        assert!(
            again
                .stderr
                .contains("pull request 1 has no tree and no base or head ref here"),
            "{}",
            again.stderr
        );
        git(
            &fixture.local,
            &[
                "fetch",
                "-q",
                "origin",
                "+refs/heads/feature:refs/code-review/1/head",
                "+refs/heads/main:refs/code-review/1/base",
            ],
        );
        let back = fixture.run(&["changes", "1"]);
        assert!(back.stdout.contains("2/3 seen"), "{}", back.stdout);
    }

    pub(super) fn since_seen(fixture: &Fixture) {
        fixture.run(&["seen", "1", "a.txt", "new.txt"]);
        let forge = &fixture.forge;
        git(forge, &["checkout", "-q", "feature"]);
        fs::write(forge.join("a.txt"), "one\nTWO\nTHREE\n").unwrap();
        git(
            forge,
            &["commit", "-q", "--amend", "-am", "change, amended"],
        );
        git(forge, &["checkout", "-q", "main"]);
        let refetch = [
            "fetch",
            "-q",
            "origin",
            "+refs/heads/feature:refs/code-review/1/head",
        ];
        git(&fixture.local, &refetch);
        let changes = fixture.run(&["changes", "1"]);
        assert!(
            changes.stdout.contains("[~]    +2    -2  a.txt"),
            "{}",
            changes.stdout
        );

        let since = fixture.run(&["diff", "1", "a.txt", "--since-seen"]);
        assert_eq!(since.code, Some(0), "{}", since.stderr);
        assert!(
            since.stdout.starts_with("[~] a.txt since it was seen at "),
            "{}",
            since.stdout
        );
        assert!(
            since.stdout.contains("    3       -three\n"),
            "{}",
            since.stdout
        );
        assert!(
            since.stdout.contains("          3 +THREE\n"),
            "{}",
            since.stdout
        );
        assert!(!since.stdout.contains("two"), "{}", since.stdout);
        let whole = fixture.run(&["diff", "1", "a.txt"]);
        assert!(
            whole.stdout.contains("    2       -two\n"),
            "{}",
            whole.stdout
        );
        let kept = fixture.run(&["diff", "1", "new.txt", "--since-seen"]);
        assert_eq!(kept.stdout, "[x] new.txt\n");
        let never = fixture.run(&["diff", "1", "gone.txt", "--since-seen"]);
        assert!(
            never.stdout.starts_with("[ ] gone.txt (deleted"),
            "{}",
            never.stdout
        );

        git(
            &fixture.local,
            &["reflog", "expire", "--expire=now", "--all"],
        );
        git(&fixture.local, &["gc", "-q", "--prune=now"]);
        let gone = fixture.run(&["diff", "1", "a.txt", "--since-seen"]);
        assert_eq!(gone.code, Some(2));
        assert!(
            gone.stderr.contains("a.txt was seen at ")
                && gone.stderr.contains("which is no longer in the clone"),
            "{}",
            gone.stderr
        );
    }

    pub(super) fn navigation(fixture: &Fixture) {
        let unopened = fixture.run_without_servers(&["files", "7"]);
        assert_eq!(unopened.code, Some(2));
        assert!(unopened.stderr.contains("has not been fetched yet"));
        let files = fixture.run_without_servers(&["files", "1", "TOOLS"]);
        assert_eq!(files.code, Some(0), "{}", files.stderr);
        assert_eq!(files.stdout, "tools.py (10 lines, 3 symbols)\n");
        assert!(
            fixture
                .local
                .join(".git/code-review/trees/1/tools.py")
                .exists()
        );

        let symbols = fixture.run_without_servers(&["symbols", "1", "tools"]);
        assert_eq!(
            symbols.stdout,
            "tools.py:1-2 function_definition helper (0 calls, 1 callers)\n\
             tools.py:5-6 function_definition run (1 calls, 1 callers)\n\
             tools.py:9-10 function_definition main (1 calls, 0 callers)\n"
        );
        let named = fixture.run_without_servers(&["symbols", "1", "help"]);
        assert_eq!(
            named.stdout,
            "tools.py:1-2 function_definition helper (0 calls, 1 callers)\n"
        );
        let shown = fixture.run_without_servers(&["show", "1", "tools.py", "5", "6"]);
        assert_eq!(
            shown.stdout,
            "    5 def run():\n    6     return helper(2)\n"
        );
        let hits = fixture.run_without_servers(&["search", "1", "helper\\("]);
        assert_eq!(
            hits.stdout,
            "tools.py:1: def helper(x):\ntools.py:6:     return helper(2)\n"
        );

        let callers = fixture.run_without_servers(&["callers", "1", "helper"]);
        assert_eq!(callers.stdout, "helper tools.py:1-2\n  run tools.py:5-6\n");
        let called = fixture.run_without_servers(&["callees", "1", "run"]);
        assert_eq!(called.stdout, "run tools.py:5-6\n  helper tools.py:1-2\n");
        let tree = fixture.run_without_servers(&["calltree", "1", "main", "2"]);
        assert_eq!(
            tree.stdout,
            "main tools.py:9-10\n  run tools.py:5-6\n    helper tools.py:1-2\n"
        );
        let shallow = fixture.run_without_servers(&["calltree", "1", "main", "1"]);
        assert_eq!(shallow.stdout, "main tools.py:9-10\n  run tools.py:5-6\n");
        let roots = fixture.run_without_servers(&["roots", "1"]);
        assert_eq!(roots.stdout, "main tools.py:9-10 (1 calls)\n");
        let none = fixture.run_without_servers(&["roots", "1", "0"]);
        assert_eq!(none.stdout, "");
        let refs = fixture.run_without_servers(&["refs", "1", "helper"]);
        assert_eq!(
            refs.stdout,
            "helper tools.py:1-2\n  (references need pyright-langserver)\n"
        );

        let missing = fixture.run_without_servers(&["show", "1", "nope.py"]);
        assert_eq!(missing.code, Some(2));
        assert!(
            missing.stderr.contains("nope.py is not a file of the tree"),
            "{}",
            missing.stderr
        );
        let nobody = fixture.run_without_servers(&["callers", "1", "nothing"]);
        assert_eq!(nobody.code, Some(2));
        assert!(nobody.stderr.contains("no symbol nothing in the tree"));
        let broken = fixture.run_without_servers(&["search", "1", "("]);
        assert_eq!(broken.code, Some(2));
        assert!(
            broken.stderr.contains("regex parse error"),
            "{}",
            broken.stderr
        );
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
        assert_eq!(unopened.code, Some(2));
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

    const DEVICE: &str = r#"{"device_code":"devsecret","user_code":"ABCD-1234","verification_uri":"https://gitlab.test/oauth/device","verification_uri_complete":"https://gitlab.test/oauth/device?user_code=ABCD-1234","expires_in":300,"interval":0}"#;
    const PENDING: &str = r#"{"error":"authorization_pending","error_description":"pending"}"#;
    const GRANTED: &str = r#"{"access_token":"glaccess1","token_type":"Bearer","expires_in":7200,"refresh_token":"glrefresh1","scope":"api","created_at":1}"#;

    pub(super) fn login(fixture: &Fixture) {
        let forge = FakeForge::new(fixture);
        git(
            &fixture.local,
            &[
                "remote",
                "set-url",
                "origin",
                "https://gitlab.test/owner/repo.git",
            ],
        );
        unregistered_logins(fixture, &forge);
        device_login(fixture, &forge);
        review_requests(fixture, &forge);
        renewal(fixture, &forge);
        logout_and_refusals(fixture, &forge);
        pasted_login(fixture, &forge);
    }

    fn unregistered_logins(fixture: &Fixture, forge: &FakeForge) {
        let run = |arguments: &[&str]| forge.run(fixture, arguments, &[], "");
        let unregistered = run(&["login", "gitlab.test"]);
        assert_eq!(unregistered.code, Some(2));
        assert!(
            unregistered.stderr.contains(
                "no client ID is built in for it: add the line `client gitlab.test <client id>` to "
            ),
            "{}",
            unregistered.stderr
        );
        let not_filled = run(&["login", "gitlab.com"]);
        assert!(
            not_filled
                .stderr
                .contains("its built-in client ID is not filled in yet (BUILT_IN_APPS in crates/io-forge/src/wire.rs)"),
            "{}",
            not_filled.stderr
        );
        let nowhere = run(&["login", "nowhere"]);
        assert!(
            nowhere
                .stderr
                .contains("nowhere is neither a pinned repository nor a forge's host"),
            "{}",
            nowhere.stderr
        );
    }

    fn device_login(fixture: &Fixture, forge: &FakeForge) {
        let run = |arguments: &[&str]| forge.run(fixture, arguments, &[], "");
        fs::write(
            &forge.forges,
            "review forges 1\nclient gitlab.test cid123\n",
        )
        .unwrap();
        forge.answer("device.json", DEVICE);
        forge.answer("token", "glaccess1");
        forge.polls(&[(PENDING, 400), (GRANTED, 200)]);
        let logged = run(&["login", "repo"]);
        assert_eq!(logged.code, Some(0), "{}", logged.stderr);
        assert_eq!(
            logged.stdout,
            "to log in to gitlab.test, enter the code ABCD-1234 at https://gitlab.test/oauth/device\n\
             the browser is opening it; waiting for the approval...\n\
             logged in to gitlab.test; the token is in the system keychain\n"
        );
        assert_eq!(
            forge.read_when_written("opened"),
            "https://gitlab.test/oauth/device?user_code=ABCD-1234\n"
        );
        let stored = forge.read("keychain/gitlab.test");
        assert!(
            stored.starts_with("glaccess1 glrefresh1 ") && stored.ends_with(" cid123"),
            "{stored}"
        );
        let arguments = forge.read("keychain.argv") + &forge.read("curl.argv");
        for secret in ["glaccess1", "glrefresh1", "devsecret"] {
            assert!(
                !arguments.contains(secret),
                "{secret} on a command line: {arguments}"
            );
        }
    }

    const MERGE_REQUESTS: &str = r#"[
        {"id": 901, "iid": 12, "title": "Teach the parser", "author": {"username": "bob"},
         "target_branch": "main", "source_branch": "parser",
         "sha": "0123456789abcdef0123456789abcdef01234567", "updated_at": "2026-02-02T00:00:00Z"},
        {"id": 902, "iid": 13, "title": "Bump the version", "author": {"username": "cat"},
         "target_branch": "main", "source_branch": "bump",
         "sha": "89abcdef0123456789abcdef0123456789abcdef", "updated_at": "2026-02-01T00:00:00Z"}
    ]"#;
    const REQUESTED: &str = r#"[{"id": 901, "iid": 12, "title": "Teach the parser"}]"#;

    fn review_requests(fixture: &Fixture, forge: &FakeForge) {
        let run = |arguments: &[&str]| forge.run(fixture, arguments, &[], "");
        forge.answer("merge_requests.json", MERGE_REQUESTS);
        forge.answer("requested.json", REQUESTED);
        let every = run(&["prs"]);
        assert!(every.stdout.contains("owner/repo#12"), "{}", every.stdout);
        assert!(every.stdout.contains("owner/repo#13"), "{}", every.stdout);
        let requested = run(&["prs", "--requested"]);
        assert_eq!(requested.code, Some(0), "{}", requested.stderr);
        assert_eq!(
            requested.stdout,
            "owner/repo#12 Teach the parser  (bob, 2026-02-02)  main <- parser  [new]\n"
        );
        let narrowed = run(&["prs", "bump", "--requested"]);
        assert_eq!(
            narrowed.stdout,
            "no open pull request has a review requested from you\n"
        );
        forge.answer("requested.json", "[]");
        let none = run(&["prs", "--requested"]);
        assert_eq!(
            none.stdout,
            "no open pull request has a review requested from you\n"
        );
        let api = forge.tools.join("api");
        fs::remove_file(api.join("merge_requests.json")).unwrap();
        fs::remove_file(api.join("requested.json")).unwrap();
    }

    fn renewal(fixture: &Fixture, forge: &FakeForge) {
        let run = |arguments: &[&str]| forge.run(fixture, arguments, &[], "");
        let listed = run(&["prs"]);
        assert_eq!(
            listed.stdout, "no open pull requests\n",
            "{}",
            listed.stderr
        );
        let overridden = forge.run(fixture, &["prs"], &[("GITLAB_TOKEN", "glvariable")], "");
        assert!(
            overridden.stdout.contains("the forge answered HTTP 401"),
            "{}",
            overridden.stdout
        );

        forge.answer("token", "glaccess2");
        forge.answer(
            "refresh.json",
            r#"{"access_token":"glaccess2","token_type":"Bearer","expires_in":7200,"refresh_token":"glrefresh2"}"#,
        );
        let renewed = run(&["prs"]);
        assert_eq!(
            renewed.stdout, "no open pull requests\n",
            "{}",
            renewed.stderr
        );
        assert!(
            forge
                .read("keychain/gitlab.test")
                .starts_with("glaccess2 glrefresh2 ")
        );

        forge.store("gitlab.test", "glaccess2 glrefresh2 1 cid123");
        forge.answer("token", "glaccess3");
        forge.answer(
            "refresh.json",
            r#"{"access_token":"glaccess3","token_type":"Bearer","expires_in":7200,"refresh_token":"glrefresh3"}"#,
        );
        let expired = run(&["prs"]);
        assert_eq!(
            expired.stdout, "no open pull requests\n",
            "{}",
            expired.stderr
        );
        assert!(
            forge
                .read("keychain/gitlab.test")
                .starts_with("glaccess3 glrefresh3 ")
        );

        forge.answer("token", "glaccess4");
        forge.answer(
            "refresh.json",
            r#"{"error":"invalid_grant","error_description":"The provided authorization grant is invalid"}"#,
        );
        let unrenewable = run(&["prs"]);
        assert!(
            unrenewable
                .stdout
                .contains("the login to gitlab.test expired and could not be renewed"),
            "{}",
            unrenewable.stdout
        );
    }

    fn logout_and_refusals(fixture: &Fixture, forge: &FakeForge) {
        let run = |arguments: &[&str]| forge.run(fixture, arguments, &[], "");
        let out = run(&["logout", "repo"]);
        assert_eq!(
            out.stdout,
            "logged out of gitlab.test; its token is gone from the keychain\n"
        );
        let again = run(&["logout", "gitlab.test"]);
        assert_eq!(again.code, Some(0));
        assert_eq!(
            again.stdout,
            "the keychain holds no token for gitlab.test\n"
        );
        let anonymous = run(&["prs"]);
        assert!(
            anonymous.stdout.contains("the forge answered HTTP 401"),
            "{}",
            anonymous.stdout
        );
        let unrequested = run(&["prs", "--requested"]);
        assert_eq!(unrequested.code, Some(2));
        assert_eq!(unrequested.stdout, "");
        assert!(
            unrequested.stderr.contains(
                "log in to see the review requests on gitlab.test: `review login gitlab.test`, or set GITLAB_TOKEN"
            ),
            "{}",
            unrequested.stderr
        );

        forge.polls(&[(PENDING, 400), (r#"{"error":"access_denied"}"#, 400)]);
        let denied = run(&["login", "gitlab.test"]);
        assert_eq!(denied.code, Some(2));
        assert!(
            denied
                .stderr
                .contains("the login to gitlab.test was denied in the browser")
        );
        forge.polls(&[(r#"{"error":"expired_token"}"#, 400)]);
        let late = run(&["login", "gitlab.test"]);
        assert!(
            late.stderr
                .contains("the code for gitlab.test expired before it was entered")
        );
        assert_eq!(forge.read("keychain/gitlab.test"), "");
    }

    fn pasted_login(fixture: &Fixture, forge: &FakeForge) {
        let pasted = forge.run(fixture, &["login", "forgejo.test"], &[], "  fjtoken \n");
        assert_eq!(pasted.code, Some(0), "{}", pasted.stderr);
        assert!(
            pasted.stdout.starts_with(
                "forgejo on forgejo.test has no device login: make an access token at https://forgejo.test/user/settings/applications"
            ),
            "{}",
            pasted.stdout
        );
        assert!(
            pasted
                .stdout
                .ends_with("logged in to forgejo.test; the token is in the system keychain\n")
        );
        assert_eq!(forge.read("keychain/forgejo.test"), "fjtoken");
        assert!(!forge.read("keychain.argv").contains("fjtoken"));
        let blank = forge.run(fixture, &["login", "forgejo.test"], &[], "\n");
        assert_eq!(blank.code, Some(2));
        assert!(
            blank
                .stderr
                .contains("nothing that looks like a token was pasted for forgejo.test"),
            "{}",
            blank.stderr
        );
    }

    const PULL: &str = r#"{"number":1,"title":"Shout two","user":{"login":"ann"},"base":{"ref":"main"},"head":{"ref":"feature","sha":"HEAD"},"body":"Says two louder.","updated_at":"2026-01-02T12:00:00Z"}"#;
    const ISSUE_COMMENTS: &str = r#"[{"id":501,"user":{"login":"carol"},"body":"Can this land before the release?","created_at":"2026-01-02T09:00:00Z"}]"#;
    const REVIEWS: &str = r#"[
        {"id":20,"user":{"login":"dave"},"body":"Two things before this goes in.","state":"REQUEST_CHANGES","comments_count":1,"submitted_at":"2026-01-02T10:00:00Z"},
        {"id":21,"user":{"login":"ann"},"body":"","state":"COMMENT","comments_count":1,"submitted_at":"2026-01-02T11:00:00Z"},
        {"id":22,"user":{"login":"erin"},"body":"","state":"APPROVED","comments_count":0,"submitted_at":"2026-01-02T12:00:00Z"}
    ]"#;
    const REVIEW_20: &str = r#"[{"id":300,"body":"Why upper case?","user":{"login":"dave"},"path":"a.txt","position":2,"original_position":0,"resolver":null,"pull_request_review_id":20,"created_at":"2026-01-02T09:58:00Z"}]"#;
    const REVIEW_21: &str = r#"[{"id":310,"body":"It is louder.","user":{"login":"ann"},"path":"a.txt","position":2,"original_position":0,"resolver":null,"pull_request_review_id":21,"created_at":"2026-01-02T11:00:00Z"}]"#;

    pub(super) fn stalls(fixture: &Fixture) {
        let forge = FakeForge::new(fixture);
        git(
            &fixture.local,
            &[
                "remote",
                "set-url",
                "origin",
                "https://forgejo.test/owner/repo.git",
            ],
        );
        forge.answer("repos-owner-repo-pulls.json", "[]");
        forge.store("forgejo.test", "fjtoken");
        fs::write(forge.tools.join("keychain.stall"), "").unwrap();
        let unanswered = forge.run(fixture, &["prs"], &[], "");
        assert_eq!(unanswered.code, Some(0), "{}", unanswered.stderr);
        assert_eq!(unanswered.stdout, "no open pull requests\n");
        let unrequested = forge.run(fixture, &["prs", "--requested"], &[], "");
        for said in [
            "! owner/repo: the keychain did not answer within 5 s: ",
            " may be waiting on a dialog that asks to unlock the keychain or to allow access to the code-review item; answer it (or unlock the keychain), then refresh the Inbox or run the command again\n",
        ] {
            assert!(
                unrequested.stdout.contains(said),
                "{}{}",
                unrequested.stdout,
                unrequested.stderr
            );
        }
        fs::remove_file(forge.tools.join("keychain.stall")).unwrap();

        let base = fixture.forge.parent().unwrap();
        let other = base.join("other");
        git(
            base,
            &[
                "clone",
                "-q",
                fixture.forge.to_str().unwrap(),
                other.to_str().unwrap(),
            ],
        );
        git(
            &other,
            &[
                "remote",
                "set-url",
                "origin",
                "https://gitlab.test/owner/other.git",
            ],
        );
        let pinned = forge.run(fixture, &["pin", other.to_str().unwrap()], &[], "");
        assert_eq!(pinned.code, Some(0), "{}", pinned.stderr);
        forge.answer("token", "gltoken");
        forge.answer("merge_requests.json", MERGE_REQUESTS);
        fs::write(forge.tools.join("api").join("stall-forgejo.test"), "").unwrap();
        let stalled = forge.run(fixture, &["prs"], &[("GITLAB_TOKEN", "gltoken")], "");
        assert_eq!(stalled.code, Some(0), "{}", stalled.stderr);
        assert!(
            stalled.stdout.starts_with(
                "! owner/repo: forgejo.test did not answer in time (curl gives up after 10 s connecting or 30 s on one request)"
            ),
            "{}",
            stalled.stdout
        );
        assert!(
            stalled.stdout.contains("owner/other#12"),
            "{}",
            stalled.stdout
        );
        assert!(
            forge
                .read("curl.argv")
                .contains("--connect-timeout 10 --max-time 30"),
            "{}",
            forge.read("curl.argv")
        );
    }

    fn forgejo(fixture: &Fixture) -> FakeForge {
        let forge = FakeForge::new(fixture);
        git(
            &fixture.local,
            &[
                "remote",
                "set-url",
                "origin",
                "https://forgejo.test/owner/repo.git",
            ],
        );
        let head = git_output(&fixture.local, &["rev-parse", "refs/code-review/1/head"]);
        forge.answer(
            "repos-owner-repo-pulls-1.json",
            &PULL.replace("HEAD", head.trim()),
        );
        forge.answer("repos-owner-repo-issues-1-comments.json", ISSUE_COMMENTS);
        forge.answer("repos-owner-repo-pulls-1-reviews.json", REVIEWS);
        forge.answer(
            "repos-owner-repo-pulls-1-reviews-20-comments.json",
            REVIEW_20,
        );
        forge.answer(
            "repos-owner-repo-pulls-1-reviews-21-comments.json",
            REVIEW_21,
        );
        forge
    }

    pub(super) fn jira_card(fixture: &Fixture) {
        let forge = forgejo(fixture);
        let head = git_output(&fixture.local, &["rev-parse", "refs/code-review/1/head"]);
        forge.answer(
            "repos-owner-repo-pulls-1.json",
            &PULL
                .replace("HEAD", head.trim())
                .replace("\"feature\"", "\"PROJ-7-shout\""),
        );
        let run = |arguments: &[&str], input: &str| forge.run(fixture, arguments, &[], input);
        let unset = run(&["card", "1"], "");
        assert_eq!(unset.code, Some(0), "{}", unset.stderr);
        assert!(
            unset
                .stdout
                .contains("no Jira site: add a line `jira <host>`"),
            "{}",
            unset.stdout
        );
        fs::write(&forge.forges, "review forges 1\njira jira.test\n").unwrap();
        let logged_out = run(&["card", "1"], "");
        assert_eq!(logged_out.code, Some(2), "{}", logged_out.stdout);
        assert!(
            logged_out
                .stderr
                .contains("not logged in to jira.test: `review login jira.test`"),
            "{}",
            logged_out.stderr
        );
        forge.answer("jira-login", "ann@acme.test:jtoken");
        forge.answer(
            "jira-PROJ-7.json",
            r#"{"key":"PROJ-7","fields":{"summary":"Shout the second line","issuetype":{"name":"Story"},"assignee":{"displayName":"Ann Smith"},"status":{"name":"In Review"},"description":"*Two* goes up.\r\nNothing else."}}"#,
        );
        let blank = run(&["login", "jira.test"], "\n");
        assert_eq!(blank.code, Some(2), "{}", blank.stdout);
        let login = run(&["login", "jira.test"], "ann@acme.test:jtoken\n");
        assert_eq!(login.code, Some(0), "{}", login.stderr);
        assert!(
            login
                .stdout
                .contains("logged in to Jira on jira.test as Ann Smith"),
            "{}",
            login.stdout
        );
        let card = run(&["card", "1"], "");
        assert_eq!(card.code, Some(0), "{}", card.stderr);
        assert_eq!(
            card.stdout,
            "PROJ-7  Shout the second line\nStory  In Review  Ann Smith\nhttps://jira.test/browse/PROJ-7\n\n  **Two** goes up.  \n  Nothing else.\n"
        );
        forge.store("jira.test", "ann@acme.test:wrong");
        let refused = run(&["card", "1"], "");
        assert_eq!(refused.code, Some(2), "{}", refused.stdout);
        assert!(
            refused.stderr.contains("jira.test refused the login"),
            "{}",
            refused.stderr
        );
        let logout = run(&["logout", "jira.test"], "");
        assert!(
            logout.stdout.contains("logged out of jira.test"),
            "{}",
            logout.stdout
        );
        let cloud = run(&["login", "acme.atlassian.net"], "ann@acme.test:jtoken\n");
        assert_eq!(cloud.code, Some(0), "{}", cloud.stderr);
        assert!(
            fs::read_to_string(&forge.forges)
                .unwrap()
                .ends_with("jira jira.test\njira acme.atlassian.net\n"),
            "{}",
            cloud.stdout
        );
        let wrong = run(&["login", "jira.test"], "ann@acme.test:wrong\n");
        assert!(
            wrong
                .stderr
                .contains("jira.test refused the email and token"),
            "{}",
            wrong.stderr
        );
    }

    pub(super) fn discussion(fixture: &Fixture) {
        let forge = forgejo(fixture);
        let shown = forge.run(fixture, &["discussion", "1"], &[], "");
        assert_eq!(shown.code, Some(0), "{}", shown.stderr);
        assert_eq!(
            shown.stdout,
            "Says two louder.

thread issue-501
  2026-01-02 09:00 carol:
    Can this land before the release?

dave requested changes 2026-01-02 10:00 (review-20)
  Two things before this goes in.
  thread 300 on a.txt:2
    2026-01-02 09:58 dave:
      Why upper case?
    2026-01-02 11:00 ann:
      It is louder.

erin approved 2026-01-02 12:00 (review-22)
"
        );
    }

    pub(super) fn submit_refused(fixture: &Fixture) {
        let forge = forgejo(fixture);
        let token = [("FORGEJO_TOKEN", "fjtoken")];
        let refused = |ran: &Ran| {
            assert_eq!(ran.code, Some(2), "{}", ran.stdout);
            ran.stderr.clone()
        };
        let logged_out = forge.run(fixture, &["submit", "1", "--approve"], &[], "");
        assert!(
            refused(&logged_out).contains(
                "submitting to forgejo.test needs a login: `review login forgejo.test`, or set FORGEJO_TOKEN or GITEA_TOKEN"
            ),
            "{}",
            logged_out.stderr
        );
        let empty = forge.run(fixture, &["submit", "1", "--request-changes"], &token, "");
        assert!(
            refused(&empty).contains("there are no draft comments and no summary to send"),
            "{}",
            empty.stderr
        );
        forge.answer("user.json", r#"{"id":1,"login":"ann"}"#);
        for decision in ["--approve", "--request-changes"] {
            let own = forge.run(fixture, &["submit", "1", "Fine.", decision], &token, "");
            assert!(
                refused(&own).contains("you opened this pull request"),
                "{decision}: {}",
                own.stderr
            );
        }
        forge.answer("user.json", r#"{"id":2,"login":"zed"}"#);
        let approved = forge.run(fixture, &["submit", "1", "--approve"], &token, "");
        assert_eq!(approved.code, Some(0), "{}", approved.stderr);
        assert_eq!(approved.stdout, "approved #1 with 0 inline comments\n");
    }
}

mod scale {
    use std::time::{Duration, Instant};

    use super::common::{self, Fixture, large};

    const OPEN_DEADLINE: Duration = Duration::from_secs(10);
    const STEP_DEADLINE: Duration = Duration::from_secs(4);
    const UNOPENED_INBOX_DEADLINE: Duration = Duration::from_secs(6);

    fn step(fixture: &Fixture, arguments: &[&str], within: Duration) -> String {
        let start = Instant::now();
        let out = common::review_within(&common::binary(), fixture, arguments, within);
        eprintln!(
            "SCALE review {} took {} ms (bound {} ms)",
            arguments.join(" "),
            start.elapsed().as_millis(),
            within.as_millis()
        );
        assert!(!out.contains("[exit "), "review {arguments:?}\n{out}");
        out
    }

    fn marked(out: &str, mark: &str) -> usize {
        out.lines().filter(|line| line.starts_with(mark)).count()
    }

    pub(super) fn scale_inbox(fixture: &Fixture) {
        common::git(
            &fixture.local,
            &[
                "fetch",
                "-q",
                "origin",
                "+refs/pull/1/head:refs/code-review/1/head",
                "+refs/heads/main:refs/code-review/1/base",
            ],
        );
        let listed = step(fixture, &["prs"], UNOPENED_INBOX_DEADLINE);
        let progress = format!("0/{} seen]", large::CHANGED);
        assert!(
            listed
                .lines()
                .any(|line| line.starts_with("owner/repo#1 ") && line.contains(&progress)),
            "{listed}"
        );
    }

    pub(super) fn scale_review(fixture: &Fixture) {
        let opened = step(fixture, &["open", "1"], OPEN_DEADLINE);
        assert_eq!(marked(&opened, "[ ] "), large::CHANGED, "{opened}");
        let listed = step(fixture, &["prs"], STEP_DEADLINE);
        let progress = format!("[reviewing, 0/{} seen]", large::CHANGED);
        assert!(listed.contains(&progress), "{listed}");
        let changes = step(fixture, &["changes", "1"], STEP_DEADLINE);
        assert_eq!(marked(&changes, "[ ] "), large::CHANGED, "{changes}");
        assert_eq!(
            changes
                .lines()
                .filter(|line| line.contains(" (from "))
                .count(),
            large::RENAMED,
            "{changes}"
        );
        let first = large::first_modified();
        let one = step(fixture, &["diff", "1", &first], STEP_DEADLINE);
        let lines = large::first_modified_lines();
        assert!(
            one.starts_with(&format!("[ ] {first} (modified, +{lines} -{lines})")),
            "{}",
            one.lines().next().unwrap_or_default()
        );
        let every = step(fixture, &["diff", "1"], STEP_DEADLINE);
        assert_eq!(marked(&every, "[ ] "), large::CHANGED);
        let hunks = every
            .lines()
            .filter(|line| line.trim_start().starts_with("@@"))
            .count();
        assert!(hunks >= large::CHANGED - large::RENAMED, "{hunks} hunks");
        let next = step(fixture, &["next", "1"], STEP_DEADLINE);
        let unseen = changes.lines().next().unwrap_or_default();
        let path = unseen.split_whitespace().last().unwrap_or_default();
        assert!(
            next.starts_with(&format!("[ ] {path} ")),
            "{unseen}\n{}",
            next.lines().next().unwrap_or_default()
        );
        let seen = step(fixture, &["seen", "1", "--all"], STEP_DEADLINE);
        assert_eq!(marked(&seen, "[x] "), large::CHANGED, "{seen}");
        let after = step(fixture, &["changes", "1"], STEP_DEADLINE);
        assert_eq!(marked(&after, "[x] "), large::CHANGED, "{after}");
        let done = step(fixture, &["next", "1"], STEP_DEADLINE);
        assert_eq!(
            done.trim_end(),
            format!(
                "every changed file is seen ({}/{})",
                large::CHANGED,
                large::CHANGED
            )
        );
    }
}

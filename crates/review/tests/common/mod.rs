#![expect(
    dead_code,
    reason = "each test binary uses its own part of the shared harness"
)]

pub(crate) mod gui;
pub(crate) mod large;

use std::env;
use std::fmt::Write as _;
use std::fs;
use std::io::Write as _;
#[cfg(unix)]
use std::os::unix::fs::{PermissionsExt, symlink};
#[cfg(unix)]
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{self, Command, Output, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

pub(crate) const HOST: &str = "forgejo.test";
pub(crate) const SLUG: &str = "owner/repo";

pub(crate) const BASE: &[(&str, &str)] = &[
    (
        "src/lib.rs",
        "pub mod shapes;

use shapes::area;

pub fn add(left: i32, right: i32) -> i32 {
    left + right
}

pub fn twice(value: i32) -> i32 {
    add(value, value)
}
",
    ),
    (
        "src/shapes.rs",
        "pub fn area(side: i32) -> i32 {
    side * side
}
",
    ),
    ("notes.md", "# notes\n\nWhat the shapes are for.\n"),
    (".gitattributes", "notes.md linguist-generated\n"),
];

pub(crate) const FIRST: &[(&str, &str)] = &[(
    "src/lib.rs",
    "pub mod shapes;

use shapes::area;

pub fn add(left: i32, right: i32) -> i32 {
    left + right
}

pub fn twice(value: i32) -> i32 {
    add(area(value), add(value, value)) + add(area(1), area(2)) + add(area(3), area(4)) + add(area(5), area(6)) + add(area(7), area(8))
}
",
)];

pub(crate) const SECOND: &[(&str, &str)] = &[(
    "notes.md",
    "# notes\n\nWhat the shapes are for, and how big they get.\n",
)];

pub(crate) const GONE: &str = "old.txt";
pub(crate) const GONE_LINES: usize = 400;

pub(crate) const DESCRIPTION: &str = "Squares the inputs before adding them.

| input | squared | notes |
|---|---:|---|
| 3 | 9 | the square grows faster than the sum once the input passes two, which is the point of this change; measured on the shapes benchmark, see https://example.com/bench |

The sum is in https://forgejo.test/owner/repo/src/branch/feature/src/lib.rs#L5-L7, the side in https://forgejo.test/owner/repo/src/branch/main/src/shapes.rs#L2, and https://forgejo.test/owner/repo/src/branch/feature/gone.rs is not in the tree.";

const ISSUE_COMMENTS: &str = r#"[
    {"id": 501, "user": {"login": "carol"}, "body": "Can this land before the release?",
     "created_at": "2026-01-02T09:00:00Z"}
]"#;

const REVIEWS: &str = r#"[
    {"id": 20, "user": {"login": "dave"}, "body": "Two things before this goes in.",
     "state": "REQUEST_CHANGES", "comments_count": 2, "submitted_at": "2026-01-02T10:00:00Z",
     "html_url": "https://forge.test/owner/repo/pull/1#pullrequestreview-20"},
    {"id": 21, "user": {"login": "ann"}, "body": "", "state": "COMMENT", "comments_count": 1,
     "submitted_at": "2026-01-02T11:00:00Z"},
    {"id": 22, "user": {"login": "erin"}, "body": "", "state": "APPROVED", "comments_count": 0,
     "submitted_at": "2026-01-02T12:00:00Z"}
]"#;

const REVIEW_20_COMMENTS: &str = r#"[
    {"id": 300, "body": "Why square here?", "user": {"login": "dave"}, "path": "src/lib.rs",
     "position": 10, "original_position": 0, "resolver": null, "pull_request_review_id": 20,
     "created_at": "2026-01-02T09:58:00Z"},
    {"id": 301, "body": "Say how big in numbers.", "user": {"login": "dave"}, "path": "notes.md",
     "position": 3, "original_position": 0, "resolver": null, "pull_request_review_id": 20,
     "created_at": "2026-01-02T09:59:00Z"}
]"#;

const REVIEW_21_COMMENTS: &str = r#"[
    {"id": 310, "body": "It makes the sum grow faster.", "user": {"login": "ann"},
     "path": "src/lib.rs", "position": 10, "original_position": 0, "resolver": null,
     "pull_request_review_id": 21, "created_at": "2026-01-02T11:00:00Z"}
]"#;

const REQUESTED: &str = r#"[{"number":1,"title":"Square the inputs","repository":{"full_name":"owner/repo"}},{"number":2,"title":"Elsewhere","repository":{"full_name":"owner/other"}}]"#;

pub(crate) const FORGES: &str = "review forges 1\nclient gitlab.test cid123\njira jira.test\n";
const DEVICE: &str = r#"{"device_code":"devsecret","user_code":"ABCD-1234","verification_uri":"https://gitlab.test/oauth/device","expires_in":300,"interval":1}"#;
const PENDING: &str = r#"{"error":"authorization_pending"}"#;
const GRANTED: &str = r#"{"access_token":"glaccess1","token_type":"Bearer","expires_in":7200,"refresh_token":"glrefresh1"}"#;

pub(crate) fn binary() -> PathBuf {
    env::var_os("REVIEW_BIN").map_or_else(
        || PathBuf::from(env!("CARGO_BIN_EXE_review")),
        PathBuf::from,
    )
}

pub(crate) fn scratch(name: &str) -> PathBuf {
    env::temp_dir()
        .join("review-tests")
        .join(process::id().to_string())
        .join(name)
}

fn find_tool(tool: &str) -> PathBuf {
    let path = env::var_os("PATH").unwrap_or_default();
    env::split_paths(&path)
        .map(|dir| dir.join(tool))
        .find(|file| file.is_file())
        .unwrap_or_else(|| panic!("{tool} is not on PATH"))
}

pub(crate) const COMMAND_DEADLINE: Duration = Duration::from_mins(1);
pub(crate) const WINDOW_DEADLINE: Duration = Duration::from_mins(2);
const KILLED_OUTPUT_WAIT: Duration = Duration::from_secs(10);

pub(crate) struct Deadline<'a> {
    pub(crate) scenario: &'a str,
    pub(crate) step: &'a str,
    pub(crate) within: Duration,
}

pub(crate) fn run_within(command: &mut Command, input: &[u8], bound: &Deadline<'_>) -> Output {
    #[cfg(unix)]
    command.process_group(0);
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|error| {
            panic!(
                "{}: `{}` did not start: {error}",
                bound.scenario, bound.step
            )
        });
    let group = child.id();
    let mut stdin = child.stdin.take().expect("piped stdin");
    let fed = input.to_vec();
    thread::spawn(move || {
        stdin.write_all(&fed).ok();
    });
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        sender.send(child.wait_with_output()).ok();
    });
    if let Ok(output) = receiver.recv_timeout(bound.within) {
        return output.expect("wait for the run");
    }
    let running = processes_in(group);
    kill(group, &running);
    let before = receiver
        .recv_timeout(KILLED_OUTPUT_WAIT)
        .ok()
        .and_then(Result::ok)
        .map(|output| {
            format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
        })
        .unwrap_or_default();
    let listing = running.iter().fold(String::new(), |mut listing, process| {
        writeln!(listing, "  {}", process.row).ok();
        listing
    });
    panic!(
        "{}: `{}` did not finish within {:?}; killed its process group {group}, which was running:\n      PID     ELAPSED COMMAND\n{listing}--- its output before the kill ---\n{before}",
        bound.scenario, bound.step, bound.within
    );
}

struct Process {
    pid: u32,
    row: String,
}

fn processes_in(group: u32) -> Vec<Process> {
    let Ok(listed) = Command::new("ps")
        .args(["-A", "-o", "pid=,ppid=,pgid=,etime=,command="])
        .output()
    else {
        return Vec::new();
    };
    let table: Vec<(u32, u32, u32, String)> = String::from_utf8_lossy(&listed.stdout)
        .lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            let pid = words.next()?.parse().ok()?;
            let parent = words.next()?.parse().ok()?;
            let leader = words.next()?.parse().ok()?;
            let elapsed = words.next()?;
            let command = words.collect::<Vec<_>>().join(" ");
            Some((
                pid,
                parent,
                leader,
                format!("{pid:>7} {elapsed:>11} {command}"),
            ))
        })
        .collect();
    let mut under = vec![group];
    loop {
        let more: Vec<u32> = table
            .iter()
            .filter(|(pid, parent, leader, _)| {
                !under.contains(pid) && (under.contains(parent) || *leader == group)
            })
            .map(|(pid, ..)| *pid)
            .collect();
        if more.is_empty() {
            break;
        }
        under.extend(more);
    }
    table
        .into_iter()
        .filter(|(pid, ..)| under.contains(pid))
        .map(|(pid, _, _, row)| Process { pid, row })
        .collect()
}

fn kill(group: u32, running: &[Process]) {
    Command::new("kill")
        .args(["-s", "KILL", "--", &format!("-{group}")])
        .args(running.iter().map(|process| process.pid.to_string()))
        .output()
        .ok();
}

fn git_fed(root: &Path, arguments: &[&str], input: &[u8]) -> String {
    let mut command = Command::new("git");
    command
        .args([
            "-c",
            "user.name=test",
            "-c",
            "user.email=test@example.com",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(arguments)
        .current_dir(root)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_COMMITTER_DATE", "2026-01-01T00:00:00Z")
        .env("GIT_AUTHOR_DATE", "2026-01-01T00:00:00Z");
    let step = format!("git {}", arguments.join(" "));
    let out = run_within(
        &mut command,
        input,
        &Deadline {
            scenario: &root.display().to_string(),
            step: &step,
            within: COMMAND_DEADLINE,
        },
    );
    assert!(
        out.status.success(),
        "git {arguments:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

pub(crate) fn git(root: &Path, arguments: &[&str]) -> String {
    git_fed(root, arguments, b"")
}

fn put(root: &Path, files: &[(&str, &str)]) {
    for (path, text) in files {
        let file = root.join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, text).unwrap();
    }
}

fn executable(file: &Path, text: &str) {
    fs::write(file, text).unwrap();
    #[cfg(unix)]
    fs::set_permissions(file, fs::Permissions::from_mode(0o755)).unwrap();
}

fn gone_text() -> String {
    (1..=GONE_LINES).fold(String::new(), |mut text, number| {
        writeln!(text, "removed line {number}").ok();
        text
    })
}

fn pull_json(head: &str) -> String {
    format!(
        r#"{{"number":1,"title":"Square the inputs","user":{{"login":"ann"}},"base":{{"ref":"main"}},"head":{{"ref":"feature","sha":"{head}"}},"body":{},"updated_at":"2026-01-01T00:00:00Z"}}"#,
        json_text(DESCRIPTION)
    )
}

fn other_pull_json(head: &str) -> String {
    format!(
        r#"{{"number":2,"title":"Rename the notes","user":{{"login":"bob"}},"base":{{"ref":"main"}},"head":{{"ref":"notes","sha":"{head}"}},"body":"","updated_at":"2025-12-01T00:00:00Z"}}"#
    )
}

fn json_text(text: &str) -> String {
    let mut quoted = String::from("\"");
    for character in text.chars() {
        match character {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            '\n' => quoted.push_str("\\n"),
            other => quoted.push(other),
        }
    }
    quoted.push('"');
    quoted
}

fn fake_curl(api: &Path) -> String {
    format!(
        r#"#!/bin/sh
PATH=/usr/bin:/bin
config=$(cat)
url=$(printf '%s\n' "$config" | sed -n 's/^url = "\(.*\)"$/\1/p')
case "$url" in
*/login/device/code|*/oauth/authorize_device) cat "{api}/device.json"; status=200;;
*/api/v1/user) cat "{api}/user.json"; status=200;;
*/login/oauth/access_token|*/oauth/token)
  count=$(( $(cat "{api}/polls" 2>/dev/null || echo 0) + 1 )); echo "$count" > "{api}/polls"
  [ -f "{api}/poll-$count.json" ] || count=granted
  cat "{api}/poll-$count.json"; status=$(cat "{api}/poll-$count.status");;
*/api/v1/repos/issues/search\?*) cat "{api}/requested.json"; status=200;;
*/rest/api/2/attachment/content/*)
  case "$config" in *"Authorization: Basic "*) cat "{api}/jira-picture.png"; printf '\n'; exit 0;; esac
  exit 22;;
*/rest/api/2/issue/*)
  key=${{url#*/rest/api/2/issue/}}; key=${{key%%\?*}}
  case "$config" in
  *"Authorization: Basic $(printf %s "$(cat "{api}/jira-login" 2>/dev/null)" | base64)\""*)
    if [ -f "{api}/jira-$key.json" ]; then cat "{api}/jira-$key.json"; status=200; else printf '{{"errorMessages":[]}}'; status=404; fi;;
  *) status=401;;
  esac;;
*)
  rest=${{url#https://{HOST}/api/v1/repos/{SLUG}/}}
  rest=${{rest%%\?*}}
  file="{api}/$(printf '%s' "$rest" | tr '/' '-').json"
  if [ -f "$file" ]; then cat "$file"; status=200; else printf '{{"message":"not found"}}'; status=404; fi;;
esac
case "$config" in *write-out*) printf '\n\n%s' "$status";; esac
"#,
        api = api.display()
    )
}

fn fake_keychain(store: &Path) -> String {
    format!(
        r#"#!/bin/sh
PATH=/usr/bin:/bin
store="{store}"
mkdir -p "$store"
[ -f "{store}.stall" ] && exec sleep 3600
case "$1" in
-i) line=$(cat)
    account=$(printf '%s' "$line" | sed -n 's/.* -a "\([^"]*\)".*/\1/p')
    secret=$(printf '%s' "$line" | sed -n 's/.* -w "\([^"]*\)".*/\1/p')
    printf '%s' "$secret" > "$store/$account";;
find-generic-password) [ -f "$store/$5" ] || exit 44; cat "$store/$5"; echo;;
lookup) [ -f "$store/$5" ] || exit 1; cat "$store/$5";;
delete-generic-password|clear) [ -f "$store/$5" ] || exit 44; rm "$store/$5";;
store) cat > "$store/$6";;
*) exit 1;;
esac
"#,
        store = store.display()
    )
}

fn fake_opener(log: &Path) -> String {
    format!("#!/bin/sh\nprintf '%s\\n' \"$1\" >> '{}'\n", log.display())
}

fn fake_ssh(git: &Path, forge: &Path) -> String {
    format!(
        "#!/bin/sh\nexec '{}' upload-pack '{}'\n",
        git.display(),
        forge.display()
    )
}

pub(crate) struct Fixture {
    pub(crate) local: PathBuf,
    pub(crate) tools: PathBuf,
    pub(crate) home: PathBuf,
}

impl Fixture {
    pub(crate) fn remove(self) {
        fs::remove_dir_all(&self.home).ok();
    }

    fn scenario(&self) -> String {
        self.home
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    }
}

fn fresh(name: &str) -> PathBuf {
    let home = scratch(name);
    fs::remove_dir_all(&home).ok();
    for dir in ["forge", "tools", "api"] {
        fs::create_dir_all(home.join(dir)).unwrap();
    }
    git(&home.join("forge"), &["init", "-q"]);
    home
}

pub(crate) fn fixture(name: &str) -> Fixture {
    let home = fresh(name);
    let forge = home.join("forge");
    put(&forge, BASE);
    put(&forge, &[(GONE, &gone_text())]);
    git(&forge, &["add", "-A"]);
    git(&forge, &["commit", "-q", "-m", "base"]);
    git(&forge, &["checkout", "-q", "-b", "feature"]);
    put(&forge, FIRST);
    git(&forge, &["rm", "-q", GONE]);
    git(&forge, &["add", "-A"]);
    git(&forge, &["commit", "-q", "-m", "Square the inputs"]);
    put(&forge, SECOND);
    git(&forge, &["add", "-A"]);
    git(&forge, &["commit", "-q", "-m", "Say how big"]);
    git(&forge, &["update-ref", "refs/pull/1/head", "feature"]);
    git(&forge, &["checkout", "-q", "main"]);
    serve(home)
}

pub(crate) fn large_fixture(name: &str) -> Fixture {
    let home = fresh(name);
    let forge = home.join("forge");
    git_fed(
        &forge,
        &["fast-import", "--quiet"],
        large::history().as_bytes(),
    );
    git(&forge, &["update-ref", "refs/pull/1/head", "feature"]);
    serve(home)
}

fn serve(home: PathBuf) -> Fixture {
    let forge = home.join("forge");
    let local = home.join("local");
    let tools = home.join("tools");
    let api = home.join("api");
    git(
        &home,
        &[
            "clone",
            "-q",
            forge.to_str().unwrap(),
            local.to_str().unwrap(),
        ],
    );
    let real_git = find_tool("git");
    let ssh = tools.join("ssh");
    git(
        &local,
        &[
            "remote",
            "set-url",
            "origin",
            &format!("ssh://git@{HOST}/{SLUG}.git"),
        ],
    );
    git(
        &local,
        &["config", "core.sshCommand", ssh.to_str().unwrap()],
    );
    executable(&ssh, &fake_ssh(&real_git, &forge));
    executable(&tools.join("curl"), &fake_curl(&api));
    for keychain in ["security", "secret-tool"] {
        executable(
            &tools.join(keychain),
            &fake_keychain(&home.join("keychain")),
        );
    }
    for opener in ["open", "xdg-open"] {
        executable(&tools.join(opener), &fake_opener(&home.join("opened")));
    }
    fs::write(home.join("keychain.file"), "").unwrap();
    fs::write(home.join("forges"), FORGES).unwrap();
    #[cfg(unix)]
    symlink(&real_git, tools.join("git")).unwrap();
    let head = git(&forge, &["rev-parse", "feature"]).trim().to_owned();
    let pull = pull_json(&head);
    put(
        &api,
        &[
            (
                "pulls.json",
                &format!("[{pull},{}]", other_pull_json(&head)),
            ),
            ("requested.json", REQUESTED),
            ("pulls-1.json", &pull),
            ("issues-1-comments.json", ISSUE_COMMENTS),
            ("pulls-1-reviews.json", REVIEWS),
            ("pulls-1-reviews-20-comments.json", REVIEW_20_COMMENTS),
            ("pulls-1-reviews-21-comments.json", REVIEW_21_COMMENTS),
            ("device.json", DEVICE),
            ("user.json", r#"{"id": 9, "login": "zed"}"#),
            ("poll-1.json", PENDING),
            ("poll-1.status", "400"),
            ("poll-2.json", PENDING),
            ("poll-2.status", "400"),
            ("poll-granted.json", GRANTED),
            ("poll-granted.status", "200"),
        ],
    );
    Fixture { local, tools, home }
}

fn rooted(text: &str, home: &Path) -> String {
    text.replace("\r\n", "\n")
        .replace(&home.display().to_string(), "<home>")
}

fn command(binary: &Path, fixture: &Fixture) -> Command {
    let mut command = Command::new(binary);
    command
        .current_dir(&fixture.local)
        .env("PATH", &fixture.tools)
        .env("REVIEW_PINS", fixture.home.join("pins"))
        .env("REVIEW_CLONES", fixture.home.join("clones"))
        .env("REVIEW_KEYCHAIN", fixture.home.join("keychain.file"))
        .env("REVIEW_FORGES", fixture.home.join("forges"))
        .env_remove("GITLAB_TOKEN")
        .env_remove("FORGEJO_TOKEN")
        .env_remove("GITEA_TOKEN");
    command
}

pub(crate) fn review(binary: &Path, fixture: &Fixture, arguments: &[&str]) -> String {
    review_within(binary, fixture, arguments, COMMAND_DEADLINE)
}

pub(crate) fn review_within(
    binary: &Path,
    fixture: &Fixture,
    arguments: &[&str],
    within: Duration,
) -> String {
    let out = run_within(
        command(binary, fixture).args(arguments),
        b"",
        &Deadline {
            scenario: &fixture.scenario(),
            step: &format!("review {}", arguments.join(" ")),
            within,
        },
    );
    let mut text = rooted(&String::from_utf8_lossy(&out.stdout), &fixture.home);
    for line in rooted(&String::from_utf8_lossy(&out.stderr), &fixture.home).lines() {
        writeln!(text, "! {line}").ok();
    }
    if !out.status.success() {
        writeln!(text, "[exit {}]", out.status.code().unwrap_or(-1)).ok();
    }
    text
}

pub(crate) fn play_script(
    binary: &Path,
    fixture: &Fixture,
    bound: &Deadline<'_>,
    script: &str,
) -> String {
    let shots = fixture.home.join("shots");
    fs::create_dir_all(&shots).unwrap();
    let file = fixture.home.join("script.txt");
    fs::write(
        &file,
        script.replace("{shots}", &shots.display().to_string().replace('\\', "/")),
    )
    .unwrap();
    let out = run_within(
        command(binary, fixture).env("REVIEW_SCRIPT", &file),
        b"",
        bound,
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "{}: the window exited with {}\n{stderr}",
        bound.scenario,
        out.status
    );
    rooted(&stderr, &fixture.home)
}

pub(crate) fn assert_clean(name: &str, output: &str) {
    let crashed: Vec<&str> = output
        .lines()
        .filter(|line| line.contains("panicked at"))
        .collect();
    assert!(
        crashed.is_empty(),
        "{name}: review crashed (fixture in {}):\n{}\n--- output in full ---\n{output}",
        scratch(name).display(),
        crashed.join("\n")
    );
    let failed: Vec<&str> = output
        .lines()
        .filter(|line| line.starts_with("script:"))
        .collect();
    assert!(
        failed.is_empty(),
        "{name}: the script failed (fixture in {}):\n{}\n--- output in full ---\n{output}",
        scratch(name).display(),
        failed.join("\n")
    );
}

pub(crate) fn first_difference(left: &str, right: &str) -> usize {
    left.lines()
        .zip(right.lines())
        .position(|(from_left, from_right)| from_left != from_right)
        .unwrap_or(left.lines().count().min(right.lines().count()))
}

pub(crate) fn steady(stderr: &str) -> String {
    stderr
        .lines()
        .filter(|line| !line.starts_with("DUMP frames") && !line.starts_with("screenshot:"))
        .fold(String::new(), |mut kept, line| {
            writeln!(kept, "{line}").ok();
            kept
        })
}

use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process;
use std::sync::atomic::{AtomicU32, Ordering};

use domain::{BranchName, ChangeKind, PrNumber, RefName, Root};
use io_process::{Argument, Process, Program};

use crate::{Remote, RemoteUrl, Vcs, VcsError, VcsKind};

mod wire;

static SCRATCH_COUNT: AtomicU32 = AtomicU32::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Self {
        let count = SCRATCH_COUNT.fetch_add(1, Ordering::Relaxed);
        let path = env::temp_dir().join(format!("io-vcs-{label}-{}-{count}", process::id()));
        drop(fs::remove_dir_all(&path));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn put(&self, name: &str, text: &str) {
        let path = self.0.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)
            .unwrap()
            .write_all(text.as_bytes())
            .unwrap();
    }

    fn run(&self, program: &str, arguments: &[&str]) {
        let arguments: Vec<Argument> = arguments
            .iter()
            .map(|argument| Argument::new(argument))
            .collect();
        let output = Process::run(&Program::new(program), &arguments, &self.0).unwrap();
        assert!(
            output.success(),
            "{program} {arguments:?}: {}",
            output.stderr.text()
        );
    }

    fn root(&self) -> Root {
        Root::new(&self.0)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.0));
    }
}

fn available(program: &str) -> bool {
    Program::new(program).find_on_path().is_some()
}

const IDENTITY: [&str; 4] = ["-c", "user.name=test", "-c", "user.email=test@example.com"];

fn commit_all(scratch: &Scratch, message: &str) {
    scratch.run("git", &["add", "-A"]);
    scratch.run(
        "git",
        &[IDENTITY.as_slice(), &["commit", "-q", "-m", message]].concat(),
    );
}

#[test]
fn a_directory_outside_any_repository_has_no_vcs() {
    let scratch = Scratch::new("none");
    assert!(matches!(
        Vcs::detect(&scratch.root()),
        Err(VcsError::NoRepository)
    ));
}

#[test]
fn a_missing_root_keeps_the_reason() {
    let root = Root::new(Path::new("/review-no-such-root"));
    assert!(matches!(
        Vcs::detect(&root),
        Err(VcsError::UnreadableRoot(_))
    ));
}

#[test]
fn a_pull_request_is_fetched_diffed_against_its_merge_base_and_checked_out() {
    if !available("git") {
        return;
    }
    let forge = Scratch::new("forge");
    forge.run("git", &["init", "-q", "-b", "main"]);
    forge.put("src/lib.rs", "a\nb\nc\n");
    forge.put("gone.txt", "bye\n");
    commit_all(&forge, "base");
    forge.run("git", &["checkout", "-q", "-b", "feature"]);
    forge.put("src/lib.rs", "a\nx\nc\n");
    forge.put("new.txt", "hi\n");
    forge.run("git", &["rm", "-q", "gone.txt"]);
    commit_all(&forge, "change");
    forge.run("git", &["checkout", "-q", "main"]);
    forge.put("later.txt", "base moved on\n");
    commit_all(&forge, "unrelated");

    let local = Scratch::new("local");
    local.run("git", &["init", "-q"]);
    local.run(
        "git",
        &["remote", "add", "origin", forge.0.to_str().unwrap()],
    );
    let vcs = Vcs::detect(&local.root()).unwrap();
    let origin = Remote::new("origin");
    assert!(vcs.remote_url(&origin).unwrap().as_str().contains("forge"));
    let fetched = vcs
        .fetch(
            &origin,
            PrNumber::new(7).unwrap(),
            &RefName::new("refs/heads/feature"),
            &BranchName::new("main"),
        )
        .unwrap();
    assert_eq!(vcs.fetched(PrNumber::new(7).unwrap()).unwrap(), fetched);
    assert!(vcs.fetched(PrNumber::new(8).unwrap()).is_err());
    let changes = vcs.diff(&fetched.base, &fetched.head).unwrap();
    let kinds: Vec<(String, ChangeKind)> = changes
        .iter()
        .map(|change| (change.path().as_str().to_owned(), change.kind()))
        .collect();
    assert_eq!(
        kinds,
        vec![
            ("gone.txt".to_owned(), ChangeKind::Deleted),
            ("new.txt".to_owned(), ChangeKind::Added),
            ("src/lib.rs".to_owned(), ChangeKind::Modified),
        ]
    );

    let cloned = local.0.join("clones/forge");
    let root = Vcs::clone_from(&RemoteUrl::new(forge.0.to_str().unwrap()), &cloned).unwrap();
    let again = Vcs::clone_from(&RemoteUrl::new("no such place"), &cloned).unwrap();
    assert_eq!(root, again);
    let other = Vcs::detect(&root).unwrap();
    assert!(
        other
            .remote_url(&origin)
            .unwrap()
            .as_str()
            .contains("forge")
    );
    assert!(!cloned.join("src").exists());

    let tree = local.0.join("tree");
    vcs.check_out(&tree, &fetched.head).unwrap();
    assert_eq!(
        fs::read_to_string(tree.join("src/lib.rs"))
            .unwrap()
            .replace('\r', ""),
        "a\nx\nc\n"
    );
    vcs.check_out(&tree, &fetched.base).unwrap();
    assert!(tree.join("later.txt").exists());
}

#[test]
fn a_subdirectory_finds_the_repository_above_it() {
    let scratch = Scratch::new("above");
    fs::create_dir_all(scratch.0.join(".git")).unwrap();
    fs::create_dir_all(scratch.0.join("inner/.jj")).unwrap();
    fs::create_dir_all(scratch.0.join("inner/deeper")).unwrap();
    let deeper = Root::new(&scratch.0.join("inner/deeper"));
    assert_eq!(Vcs::detect(&deeper).unwrap().kind(), VcsKind::Jj);
    let other = Root::new(&scratch.0.join("inner"));
    fs::remove_dir(scratch.0.join("inner/.jj")).unwrap();
    assert_eq!(Vcs::detect(&other).unwrap().kind(), VcsKind::Git);
}

#[test]
fn every_jj_workspace_shares_the_main_repo_store() {
    let scratch = Scratch::new("jj-shared");
    fs::create_dir_all(scratch.0.join("main/.jj/repo")).unwrap();
    scratch.put("second/.jj/repo", "../../main/.jj/repo");
    let main = Vcs::detect(&Root::new(&scratch.0.join("main"))).unwrap();
    let second = Vcs::detect(&Root::new(&scratch.0.join("second"))).unwrap();
    let store = scratch.0.join("main/.jj/repo").canonicalize().unwrap();
    assert_eq!(
        main.shared_directory().unwrap().canonicalize().unwrap(),
        store
    );
    assert_eq!(
        second.shared_directory().unwrap().canonicalize().unwrap(),
        store
    );
}

#[test]
fn every_git_worktree_shares_the_common_git_directory() {
    let scratch = Scratch::new("git-shared");
    scratch.put("main/.git/worktrees/side/commondir", "../..\n");
    let pointer = scratch.0.join("main/.git/worktrees/side");
    scratch.put("side/.git", &format!("gitdir: {}\n", pointer.display()));
    let main = Vcs::detect(&Root::new(&scratch.0.join("main"))).unwrap();
    let side = Vcs::detect(&Root::new(&scratch.0.join("side"))).unwrap();
    let common = scratch.0.join("main/.git").canonicalize().unwrap();
    assert_eq!(
        main.shared_directory().unwrap().canonicalize().unwrap(),
        common
    );
    assert_eq!(
        side.shared_directory().unwrap().canonicalize().unwrap(),
        common
    );
    assert!(
        !main
            .shared_directory()
            .unwrap()
            .to_string_lossy()
            .starts_with(r"\\?\")
    );
}

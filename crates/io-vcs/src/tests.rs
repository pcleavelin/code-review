use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process;
use std::sync::atomic::{AtomicU32, Ordering};

use domain::{
    BranchName, ChangeKind, CommitId, Line, PrNumber, Provenance, RefName, RelativePath, Root,
};
use io_process::{Argument, Process, Program};

use crate::wire::GIT_READ_DEADLINE;
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
        let output = Process::run(
            &Program::new(program),
            &arguments,
            &self.0,
            GIT_READ_DEADLINE,
        )
        .unwrap();
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

fn registered_worktrees(scratch: &Scratch) -> Vec<String> {
    fs::read_dir(scratch.0.join(".git/worktrees"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect()
}

#[test]
fn a_dropped_pull_request_loses_its_tree_its_worktree_entry_and_its_refs() {
    if !available("git") {
        return;
    }
    let forge = Scratch::new("drop-forge");
    forge.run("git", &["init", "-q", "-b", "main"]);
    forge.put("a.txt", "one\n");
    commit_all(&forge, "base");
    forge.run("git", &["checkout", "-q", "-b", "feature"]);
    forge.put("a.txt", "two\n");
    commit_all(&forge, "change");
    let local = Scratch::new("drop-local");
    local.run("git", &["init", "-q"]);
    local.run(
        "git",
        &["remote", "add", "origin", forge.0.to_str().unwrap()],
    );
    let vcs = Vcs::detect(&local.root()).unwrap();
    let fetch = |number: u64| {
        vcs.fetch(
            &Remote::new("origin"),
            PrNumber::new(number).unwrap(),
            &RefName::new("refs/heads/feature"),
            &BranchName::new("main"),
        )
        .unwrap()
    };
    let (seven, eight) = (PrNumber::new(7).unwrap(), PrNumber::new(8).unwrap());
    let kept = local.0.join("trees/8");
    vcs.check_out(&kept, &fetch(8).head).unwrap();
    let tree = local.0.join("trees/7");
    vcs.check_out(&tree, &fetch(7).head).unwrap();
    local.put("trees/7/target/debug/built", "untracked output");
    local.put("trees/7/a.txt", "edited in the tree\n");

    vcs.drop_fetched(seven, &tree).unwrap();
    assert!(!tree.exists());
    assert!(vcs.fetched(seven).is_err());
    assert!(vcs.fetched(eight).is_ok());
    assert!(kept.join("a.txt").exists());
    assert_eq!(registered_worktrees(&local), ["8"]);

    vcs.check_out(&tree, &fetch(7).head).unwrap();
    assert_eq!(fs::read_to_string(tree.join("a.txt")).unwrap(), "two\n");
    fs::remove_dir_all(&kept).unwrap();
    vcs.drop_fetched(eight, &kept).unwrap();
    assert!(vcs.fetched(eight).is_err());
    assert_eq!(registered_worktrees(&local), ["7"]);
}

#[test]
fn a_pull_requests_commits_are_listed_oldest_first_and_each_diffs_against_its_parent() {
    if !available("git") {
        return;
    }
    let repo = Scratch::new("commits");
    repo.run("git", &["init", "-q", "-b", "main"]);
    repo.put(
        "a.txt", "one
",
    );
    repo.put(
        "b.txt", "two
",
    );
    commit_all(&repo, "base");
    repo.run("git", &["checkout", "-q", "-b", "feature"]);
    repo.put(
        "a.txt",
        "one changed
",
    );
    commit_all(&repo, "first: touch a");
    repo.put(
        "b.txt",
        "two changed
",
    );
    commit_all(&repo, "second: touch b");
    let vcs = Vcs::detect(&repo.root()).unwrap();
    let base = vcs.resolve(&RefName::new("main")).unwrap();
    let head = vcs.resolve(&RefName::new("feature")).unwrap();
    let commits = vcs.commits(&base, &head).unwrap();
    let subjects: Vec<&str> = commits
        .iter()
        .map(|commit| commit.subject.as_str())
        .collect();
    assert_eq!(subjects, ["first: touch a", "second: touch b"]);
    assert_eq!(commits[1].id, head);
    let touched: Vec<String> = vcs
        .commit_changes(&commits[1].id)
        .unwrap()
        .iter()
        .map(|change| change.path().as_str().to_owned())
        .collect();
    assert_eq!(touched, ["b.txt"]);
    let before = vcs.read_at(&base, &RelativePath::new("a.txt")).unwrap();
    assert_eq!(
        before.line(Line::new(0)).map(domain::SourceLine::as_str),
        Some("one")
    );
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

#[test]
fn a_file_change_between_two_commits_is_that_file_only_and_none_when_it_is_the_same() {
    if !available("git") {
        return;
    }
    let repo = Scratch::new("file-change");
    repo.run("git", &["init", "-q", "-b", "main"]);
    repo.put("a.txt", "one\ntwo\n");
    repo.put("b.txt", "same\n");
    commit_all(&repo, "first");
    repo.put("a.txt", "one\nTWO\n");
    repo.put("c.txt", "new\n");
    commit_all(&repo, "second");
    let vcs = Vcs::detect(&repo.root()).unwrap();
    let resolve = |name: &str| vcs.resolve(&RefName::new(name)).unwrap();
    let (first, second) = (resolve("HEAD~1"), resolve("HEAD"));

    let changed = vcs
        .file_change(&first, &second, &RelativePath::new("a.txt"))
        .unwrap()
        .unwrap();
    assert_eq!(changed.path().as_str(), "a.txt");
    assert_eq!((changed.added().value(), changed.removed().value()), (1, 1));
    assert_eq!(
        vcs.file_change(&first, &second, &RelativePath::new("b.txt"))
            .unwrap(),
        None
    );
    assert!(vcs.has_commit(&first));
    assert!(!vcs.has_commit(&CommitId::new("0123456789abcdef0123456789abcdef01234567").unwrap()));
}

#[test]
fn attributes_at_the_commit_mark_files_generated_or_handwritten_whatever_the_working_copy_says() {
    if !available("git") {
        return;
    }
    let repo = Scratch::new("attributes");
    repo.run("git", &["init", "-q", "-b", "main"]);
    repo.put(
        ".gitattributes",
        "gen/** linguist-generated\nCargo.lock -linguist-generated\n*.bin binary\nmr.rs gitlab-generated\n",
    );
    for file in ["gen/a.rs", "Cargo.lock", "logo.bin", "mr.rs", "src/main.rs"] {
        repo.put(file, "x\n");
    }
    commit_all(&repo, "marked");
    repo.put(".gitattributes", "src/** linguist-generated\n");
    let vcs = Vcs::detect(&repo.root()).unwrap();
    let head = Process::run(
        &Program::new("git"),
        &[Argument::new("rev-parse"), Argument::new("HEAD")],
        &repo.0,
        GIT_READ_DEADLINE,
    )
    .unwrap();
    let head = CommitId::new(head.stdout.text().trim()).unwrap();
    let paths: Vec<RelativePath> = ["gen/a.rs", "Cargo.lock", "logo.bin", "mr.rs", "src/main.rs"]
        .into_iter()
        .map(RelativePath::new)
        .collect();
    let marked = vcs.provenances(&head, &paths).unwrap();
    let said = |path: &str| marked.get(&RelativePath::new(path)).copied();
    assert_eq!(said("gen/a.rs"), Some(Provenance::Generated));
    assert_eq!(said("Cargo.lock"), Some(Provenance::Handwritten));
    assert_eq!(said("logo.bin"), Some(Provenance::Undeclared));
    assert_eq!(said("mr.rs"), Some(Provenance::Generated));
    assert_eq!(said("src/main.rs"), Some(Provenance::Undeclared));
}

#[test]
fn attributes_answer_for_more_paths_than_a_pipe_holds() {
    if !available("git") {
        return;
    }
    let repo = Scratch::new("many-attributes");
    repo.run("git", &["init", "-q", "-b", "main"]);
    repo.put(".gitattributes", "gen/** linguist-generated\n");
    repo.put("README", "x\n");
    commit_all(&repo, "marked");
    let vcs = Vcs::detect(&repo.root()).unwrap();
    let head = Process::run(
        &Program::new("git"),
        &[Argument::new("rev-parse"), Argument::new("HEAD")],
        &repo.0,
        GIT_READ_DEADLINE,
    )
    .unwrap();
    let head = CommitId::new(head.stdout.text().trim()).unwrap();
    let paths: Vec<RelativePath> = (0..10_000)
        .map(|number| {
            let folder = if number % 2 == 0 { "gen" } else { "src" };
            RelativePath::new(&format!("{folder}/module-{number:05}.rs"))
        })
        .collect();
    let marked = vcs.provenances(&head, &paths).unwrap();
    let generated = marked
        .values()
        .filter(|provenance| **provenance == Provenance::Generated)
        .count();
    assert_eq!(marked.len(), paths.len());
    assert_eq!(generated, paths.len() / 2);
}

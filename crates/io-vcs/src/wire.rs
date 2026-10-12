pub(crate) struct CommandLine {
    pub(crate) program: &'static str,
    pub(crate) arguments: Vec<String>,
}

pub(crate) const GIT: &str = "git";
pub(crate) const JJ_MARKER: &str = ".jj";
pub(crate) const GIT_MARKER: &str = ".git";
pub(crate) const JJ_REPO: &str = "repo";
pub(crate) const GIT_DIR_PREFIX: &str = "gitdir:";
pub(crate) const GIT_COMMON_DIR: &str = "commondir";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Header {
    Diff,
    NewSide,
    Hunk,
    HunkNew,
    HunkEnd,
    NewFile,
    DeletedFile,
    RenameFrom,
    RenameTo,
    Old,
    New,
    OldPrefix,
    NewPrefix,
}

impl Header {
    const fn name(self) -> &'static str {
        match self {
            Self::Diff => "diff --git a/",
            Self::NewSide => " b/",
            Self::Hunk => "@@ -",
            Self::HunkNew => " +",
            Self::HunkEnd => " @@",
            Self::NewFile => "new file mode",
            Self::DeletedFile => "deleted file mode",
            Self::RenameFrom => "rename from ",
            Self::RenameTo => "rename to ",
            Self::Old => "--- ",
            Self::New => "+++ ",
            Self::OldPrefix => "a/",
            Self::NewPrefix => "b/",
        }
    }
}

const LOCAL_REFS: &str = "refs/code-review";
const BRANCH_REFS: &str = "refs/heads";
const NO_FILE: &str = "/dev/null";

fn git(arguments: &[&str]) -> CommandLine {
    CommandLine {
        program: GIT,
        arguments: arguments
            .iter()
            .map(|argument| (*argument).to_owned())
            .collect(),
    }
}

pub(crate) fn clone(url: &str, into: &str) -> CommandLine {
    git(&[
        "clone",
        "--quiet",
        "--filter=blob:none",
        "--no-checkout",
        url,
        into,
    ])
}

pub(crate) fn remote_url(remote: &str) -> CommandLine {
    git(&["remote", "get-url", remote])
}

pub(crate) fn local_ref(number: u64, end: &str) -> String {
    format!("{LOCAL_REFS}/{number}/{end}")
}

pub(crate) const HEAD_END: &str = "head";
pub(crate) const BASE_END: &str = "base";

pub(crate) fn fetch(remote: &str, number: u64, head: &str, base: &str) -> CommandLine {
    let head = format!("+{head}:{}", local_ref(number, HEAD_END));
    let base = format!("+{BRANCH_REFS}/{base}:{}", local_ref(number, BASE_END));
    git(&["fetch", "--no-tags", "--quiet", remote, &head, &base])
}

pub(crate) fn resolve(reference: &str) -> CommandLine {
    git(&[
        "rev-parse",
        "--verify",
        "--quiet",
        &format!("{reference}^{{commit}}"),
    ])
}

pub(crate) fn diff(base: &str, head: &str) -> CommandLine {
    git(&[
        "-c",
        "core.quotepath=off",
        "diff",
        "--no-color",
        "--no-ext-diff",
        "--find-renames",
        "--src-prefix=a/",
        "--dst-prefix=b/",
        "--unified=3",
        &format!("{base}...{head}"),
    ])
}

pub(crate) fn file_diff(from: &str, to: &str, path: &str) -> CommandLine {
    git(&[
        "-c",
        "core.quotepath=off",
        "diff",
        "--no-color",
        "--no-ext-diff",
        "--src-prefix=a/",
        "--dst-prefix=b/",
        "--unified=3",
        from,
        to,
        "--",
        path,
    ])
}

pub(crate) const FIELD: char = '\u{1f}';

pub(crate) fn log(base: &str, head: &str) -> CommandLine {
    git(&[
        "log",
        "--no-color",
        "--reverse",
        "--format=%H%x1f%an%x1f%aI%x1f%s",
        &format!("{base}..{head}"),
    ])
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WireCommit {
    pub(crate) id: String,
    pub(crate) author: String,
    pub(crate) when: String,
    pub(crate) subject: String,
}

pub(crate) fn parse_log(text: &str) -> Vec<WireCommit> {
    text.lines()
        .filter_map(|line| {
            let mut fields = line.splitn(4, FIELD);
            Some(WireCommit {
                id: fields.next()?.to_owned(),
                author: fields.next()?.to_owned(),
                when: fields.next()?.to_owned(),
                subject: fields.next().unwrap_or_default().to_owned(),
            })
        })
        .collect()
}

pub(crate) fn commit_diff(commit: &str) -> CommandLine {
    git(&[
        "-c",
        "core.quotepath=off",
        "diff",
        "--no-color",
        "--no-ext-diff",
        "--find-renames",
        "--src-prefix=a/",
        "--dst-prefix=b/",
        "--unified=3",
        &format!("{commit}^"),
        commit,
    ])
}

pub(crate) fn show(commit: &str, path: &str) -> CommandLine {
    git(&["show", &format!("{commit}:{path}")])
}

pub(crate) fn add_worktree(tree: &str, commit: &str) -> CommandLine {
    git(&["worktree", "add", "--detach", "--force", tree, commit])
}

pub(crate) fn remove_worktree(tree: &str) -> CommandLine {
    git(&["worktree", "remove", "--force", "--force", tree])
}

pub(crate) fn prune_worktrees() -> CommandLine {
    git(&["worktree", "prune"])
}

pub(crate) fn delete_ref(reference: &str) -> CommandLine {
    git(&["update-ref", "-d", reference])
}

pub(crate) fn check_out(commit: &str) -> CommandLine {
    git(&["checkout", "--detach", "--force", "--quiet", commit])
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WireKind {
    Added,
    Deleted,
    Modified,
    Renamed,
}

pub(crate) struct WireHunk {
    pub(crate) old_start: u32,
    pub(crate) new_start: u32,
    pub(crate) lines: Vec<(char, String)>,
}

pub(crate) struct WireFile {
    pub(crate) path: String,
    pub(crate) old_path: Option<String>,
    pub(crate) kind: WireKind,
    pub(crate) hunks: Vec<WireHunk>,
}

fn start_of(range: &str) -> Option<u32> {
    let (start, count) = range.split_once(',').unwrap_or((range, "1"));
    let start: u32 = start.parse().ok()?;
    let count: u32 = count.parse().ok()?;
    Some(if count == 0 {
        start
    } else {
        start.saturating_sub(1)
    })
}

fn hunk_header(line: &str) -> Option<WireHunk> {
    let ranges = line.strip_prefix(Header::Hunk.name())?;
    let (old, rest) = ranges.split_once(Header::HunkNew.name())?;
    let (new, _) = rest.split_once(Header::HunkEnd.name())?;
    Some(WireHunk {
        old_start: start_of(old)?,
        new_start: start_of(new)?,
        lines: Vec::new(),
    })
}

fn side_path(value: &str, prefix: Header) -> Option<String> {
    (value != NO_FILE).then(|| {
        value
            .strip_prefix(prefix.name())
            .unwrap_or(value)
            .to_owned()
    })
}

fn header_paths(line: &str) -> Option<(String, String)> {
    let paths = line.strip_prefix(Header::Diff.name())?;
    let (old, new) = paths.split_once(Header::NewSide.name())?;
    Some((old.to_owned(), new.to_owned()))
}

pub(crate) fn parse_diff(text: &str) -> Vec<WireFile> {
    let mut files: Vec<WireFile> = Vec::new();
    let mut in_hunk = false;
    for line in text.lines() {
        if let Some((old, new)) = header_paths(line) {
            in_hunk = false;
            files.push(WireFile {
                path: new,
                old_path: Some(old),
                kind: WireKind::Modified,
                hunks: Vec::new(),
            });
            continue;
        }
        let Some(file) = files.last_mut() else {
            continue;
        };
        if in_hunk {
            let mut characters = line.chars();
            match characters.next() {
                Some(marker @ (' ' | '+' | '-')) => {
                    if let Some(hunk) = file.hunks.last_mut() {
                        hunk.lines.push((marker, characters.as_str().to_owned()));
                    }
                    continue;
                }
                Some('\\') => continue,
                None => {
                    if let Some(hunk) = file.hunks.last_mut() {
                        hunk.lines.push((' ', String::new()));
                    }
                    continue;
                }
                Some(_) => in_hunk = false,
            }
        }
        if let Some(hunk) = hunk_header(line) {
            file.hunks.push(hunk);
            in_hunk = true;
        } else if line.starts_with(Header::NewFile.name()) {
            file.kind = WireKind::Added;
        } else if line.starts_with(Header::DeletedFile.name()) {
            file.kind = WireKind::Deleted;
        } else if let Some(from) = line.strip_prefix(Header::RenameFrom.name()) {
            file.kind = WireKind::Renamed;
            file.old_path = Some(from.to_owned());
        } else if let Some(to) = line.strip_prefix(Header::RenameTo.name()) {
            to.clone_into(&mut file.path);
        } else if let Some(old) = line.strip_prefix(Header::Old.name()) {
            file.old_path = side_path(old, Header::OldPrefix);
        } else if let Some(new) = line.strip_prefix(Header::New.name())
            && let Some(path) = side_path(new, Header::NewPrefix)
        {
            file.path = path;
        }
    }
    for file in &mut files {
        match file.kind {
            WireKind::Added => file.old_path = None,
            WireKind::Modified if file.old_path.as_deref() == Some(file.path.as_str()) => {
                file.old_path = None;
            }
            WireKind::Deleted => {
                if let Some(old) = file.old_path.take() {
                    file.path = old;
                }
            }
            WireKind::Modified | WireKind::Renamed => {}
        }
    }
    files
}

pub(crate) const LINGUIST_GENERATED: &str = "linguist-generated";
pub(crate) const GITLAB_GENERATED: &str = "gitlab-generated";
pub(crate) const SET: &str = "set";
pub(crate) const TRUE: &str = "true";
pub(crate) const UNSET: &str = "unset";
pub(crate) const FALSE: &str = "false";

pub(crate) fn check_attributes(commit: &str) -> CommandLine {
    git(&[
        "check-attr",
        "-z",
        "--stdin",
        &format!("--source={commit}"),
        LINGUIST_GENERATED,
        GITLAB_GENERATED,
    ])
}

pub(crate) fn attribute_paths(paths: &[&str]) -> Vec<u8> {
    paths.iter().fold(Vec::new(), |mut input, path| {
        input.extend_from_slice(path.as_bytes());
        input.push(0);
        input
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WireAttribute {
    pub(crate) path: String,
    pub(crate) name: String,
    pub(crate) value: String,
}

pub(crate) fn parse_attributes(text: &str) -> Vec<WireAttribute> {
    let mut fields = text.split('\0');
    let mut attributes = Vec::new();
    while let (Some(path), Some(name), Some(value)) = (fields.next(), fields.next(), fields.next())
    {
        attributes.push(WireAttribute {
            path: path.to_owned(),
            name: name.to_owned(),
            value: value.to_owned(),
        });
    }
    attributes
}

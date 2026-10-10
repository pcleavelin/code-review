use domain::{
    BranchName, ChangeKind, CodePlace, CommitId, Conversation, Decision, DiffLine, Draft, DraftId,
    DraftText, FileChange, Forge, ForgeKind, Host, Hunk, Line, LineKind, Linked, Login, Moment,
    NoteKind, PrNumber, PrTitle, Prose, PullRequest, RelativePath, RepoSlug, Side, SourceLine,
    Span, ThreadId, ThreadState,
};
use serde_json::{Value, json};

use crate::convert;
use crate::wire::{
    self, Api, Method, PendingNote, Request, WireDiffRefs, WireRemote, WireResponse, WireThread,
    detect, forge_named, parse_items, parse_remote, parse_response,
};
use crate::{
    ForgeClient, ForgeError, HttpStatus, ResponseBody, Token, Unsent, every_page, send_gitlab,
    send_in_turn,
};

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

fn remote(host: &str, path: &str) -> WireRemote {
    WireRemote {
        host: host.to_owned(),
        path: path.to_owned(),
    }
}

#[test]
fn every_remote_url_form_yields_host_and_slug_without_port_user_or_git_suffix() {
    let cases = [
        (
            "https://github.com/owner/repo.git",
            Some(remote("github.com", "owner/repo")),
        ),
        (
            "https://github.com/owner/repo",
            Some(remote("github.com", "owner/repo")),
        ),
        (
            "http://git.example.com:8080/owner/repo/",
            Some(remote("git.example.com", "owner/repo")),
        ),
        (
            "https://user:secret@gitlab.com/group/sub/repo.git",
            Some(remote("gitlab.com", "group/sub/repo")),
        ),
        (
            "ssh://git@codeberg.org:2222/owner/repo.git",
            Some(remote("codeberg.org", "owner/repo")),
        ),
        (
            "ssh://git@gitlab.com/group/sub/repo",
            Some(remote("gitlab.com", "group/sub/repo")),
        ),
        (
            "git@github.com:owner/repo.git",
            Some(remote("github.com", "owner/repo")),
        ),
        (
            "  git@gitlab.example.com:group/sub/repo.git\n",
            Some(remote("gitlab.example.com", "group/sub/repo")),
        ),
        ("not a url", None),
        ("C:/Users/me/forge", None),
        ("D:\\work\\forge", None),
    ];
    for (url, expected) in cases {
        assert_eq!(parse_remote(url), expected, "{url}");
    }
}

#[test]
fn a_repository_named_like_the_suffix_keeps_its_name() {
    assert_eq!(
        parse_remote("https://github.com/owner/dotgit.github.git"),
        Some(remote("github.com", "owner/dotgit.github"))
    );
}

#[test]
fn the_host_decides_the_forge_and_lookalike_hosts_are_not_github() {
    let cases = [
        ("github.com", Some(ForgeKind::GitHub)),
        ("notgithub.com", None),
        ("github.example.com", None),
        ("gitlab.com", Some(ForgeKind::GitLab)),
        ("gitlab.example.org", Some(ForgeKind::GitLab)),
        ("codeberg.org", Some(ForgeKind::Forgejo)),
        ("forgejo.example.org", Some(ForgeKind::Forgejo)),
        ("gitea.example.org", Some(ForgeKind::Forgejo)),
        ("git.example.org", None),
    ];
    for (host, expected) in cases {
        assert_eq!(detect(host), expected, "{host}");
    }
}

#[test]
fn the_override_reads_every_forge_spelling_case_blind_and_rejects_others() {
    assert_eq!(forge_named("github"), Some(ForgeKind::GitHub));
    assert_eq!(forge_named(" GitLab\n"), Some(ForgeKind::GitLab));
    assert_eq!(forge_named("forgejo"), Some(ForgeKind::Forgejo));
    assert_eq!(forge_named("gitea"), None);
}

#[test]
fn each_forge_lists_from_its_own_api_root() {
    let cases = [
        (
            Api::new(ForgeKind::GitHub, "github.com", "owner/repo"),
            "https://api.github.com/repos/owner/repo/pulls?state=open&per_page=100",
        ),
        (
            Api::new(ForgeKind::GitHub, "ghe.example.com", "owner/repo"),
            "https://ghe.example.com/api/v3/repos/owner/repo/pulls?state=open&per_page=100",
        ),
        (
            Api::new(ForgeKind::GitLab, "gitlab.com", "group/sub/repo"),
            "https://gitlab.com/api/v4/projects/group%2Fsub%2Frepo/merge_requests?state=opened&per_page=100",
        ),
        (
            Api::new(ForgeKind::Forgejo, "codeberg.org", "owner/repo"),
            "https://codeberg.org/api/v1/repos/owner/repo/pulls?state=open&limit=50",
        ),
    ];
    for (api, expected) in cases {
        let request = api.list();
        assert_eq!(request.method, Method::Get);
        assert_eq!(request.url, expected);
        assert_eq!(request.body, None);
    }
}

#[test]
fn a_single_pull_is_read_from_the_pull_or_merge_request_path() {
    assert_eq!(
        Api::new(ForgeKind::GitHub, "github.com", "owner/repo")
            .pull(7)
            .url,
        "https://api.github.com/repos/owner/repo/pulls/7"
    );
    assert_eq!(
        Api::new(ForgeKind::GitLab, "gitlab.com", "group/repo")
            .pull(7)
            .url,
        "https://gitlab.com/api/v4/projects/group%2Frepo/merge_requests/7"
    );
}

#[test]
fn head_refs_follow_each_forges_naming() {
    assert_eq!(wire::head_ref(ForgeKind::GitHub, 4), "refs/pull/4/head");
    assert_eq!(wire::head_ref(ForgeKind::Forgejo, 4), "refs/pull/4/head");
    assert_eq!(
        wire::head_ref(ForgeKind::GitLab, 4),
        "refs/merge-requests/4/head"
    );
}

#[test]
fn a_get_config_carries_url_accept_auth_and_status_but_no_method_or_data() {
    let api = Api::new(ForgeKind::GitHub, "github.com", "owner/repo");
    let config = api.config(&api.pull(3), Some("tok"));
    assert_eq!(
        config,
        "url = \"https://api.github.com/repos/owner/repo/pulls/3\"\n\
         header = \"Accept: application/json\"\n\
         header = \"Authorization: Bearer tok\"\n\
         write-out = \"\\n%header{link}\\n%{http_code}\"\n"
    );
}

#[test]
fn each_forge_sends_the_token_in_its_own_header() {
    let header = |kind| {
        let api = Api::new(kind, "example.org", "owner/repo");
        api.config(&api.list(), Some("tok"))
    };
    assert!(header(ForgeKind::GitHub).contains("header = \"Authorization: Bearer tok\"\n"));
    assert!(header(ForgeKind::GitLab).contains("header = \"PRIVATE-TOKEN: tok\"\n"));
    assert!(header(ForgeKind::Forgejo).contains("header = \"Authorization: token tok\"\n"));
}

#[test]
fn a_read_without_a_token_sends_no_authorization() {
    let api = Api::new(ForgeKind::GitHub, "github.com", "owner/repo");
    let config = api.config(&api.list(), None);
    assert!(!config.contains("Authorization"), "{config}");
}

#[test]
fn the_token_reaches_curl_only_through_stdin() {
    let secret = "ghp_secret";
    let api = Api::new(ForgeKind::GitHub, "github.com", "owner/repo");
    let config = api.config(&api.list(), Some(secret));
    assert!(config.contains(secret));
    let arguments = wire::curl_arguments();
    assert_eq!(
        arguments,
        ["--silent", "--show-error", "--location", "--config", "-"]
    );
    assert!(arguments.iter().all(|argument| !argument.contains(secret)));
}

#[test]
fn quotes_backslashes_and_newlines_cannot_break_out_of_a_config_string() {
    let api = Api::new(ForgeKind::GitHub, "github.com", "owner/repo");
    let request = Request {
        method: Method::Post,
        url: "https://x/a\"b\\c".to_owned(),
        body: Some(json!({"body": "line \"one\"\nC:\\path"})),
    };
    let config = api.config(&request, Some("t\"ok\nurl = \"https://evil\""));
    let lines: Vec<&str> = config.lines().collect();
    assert_eq!(
        lines,
        vec![
            "url = \"https://x/a\\\"b\\\\c\"",
            "header = \"Accept: application/json\"",
            "header = \"Authorization: Bearer t\\\"ok\\nurl = \\\"https://evil\\\"\"",
            "request = \"POST\"",
            "header = \"Content-Type: application/json\"",
            "data-binary = \"{\\\"body\\\":\\\"line \\\\\\\"one\\\\\\\"\\\\nC:\\\\\\\\path\\\"}\"",
            "write-out = \"\\n%header{link}\\n%{http_code}\"",
        ]
    );
}

#[test]
fn the_status_is_the_last_line_the_link_header_before_it_and_the_body_keeps_its_newlines() {
    assert_eq!(
        parse_response("{\"a\":\n1}\n\n<https://x.org/p?page=2>; rel=\"next\"\n404"),
        Some(WireResponse {
            status: 404,
            body: "{\"a\":\n1}\n".to_owned(),
            next_url: Some("https://x.org/p?page=2".to_owned()),
        })
    );
    assert_eq!(
        parse_response("[]\n\n200"),
        Some(WireResponse {
            status: 200,
            body: "[]".to_owned(),
            next_url: None,
        })
    );
    assert_eq!(parse_response("no status"), None);
}

const GITHUB_PAGING: &str = "<https://api.github.com/repositories/53631945/pulls?state=open&per_page=30&page=2>; rel=\"next\", <https://api.github.com/repositories/53631945/pulls?state=open&per_page=30&page=3>; rel=\"last\"";
const GITLAB_PAGING: &str = "<https://gitlab.com/api/v4/projects/g%2Fr/merge_requests?page=1&per_page=2&state=opened>; rel=\"prev\", <https://gitlab.com/api/v4/projects/g%2Fr/merge_requests?page=3&per_page=2&state=opened>; rel=\"next\", <https://gitlab.com/api/v4/projects/g%2Fr/merge_requests?page=1&per_page=2&state=opened>; rel=\"first\"";
const FORGEJO_PAGING: &str = "<https://codeberg.org/api/v1/repos/o/r/pulls?limit=2&page=2&state=open>; rel=\"next\",<https://codeberg.org/api/v1/repos/o/r/pulls?limit=2&page=72&state=open>; rel=\"last\"";
const LAST_PAGE_PAGING: &str = "<https://codeberg.org/api/v1/repos/o/r/pulls?limit=2&page=1&state=open>; rel=\"first\",<https://codeberg.org/api/v1/repos/o/r/pulls?limit=2&page=71&state=open>; rel=\"prev\"";

fn next_of(paging: &str) -> Option<String> {
    parse_response(&format!("[]\n{paging}\n200")).and_then(|response| response.next_url)
}

#[test]
fn the_next_page_is_the_link_marked_next_in_every_forges_spelling() {
    assert_eq!(
        next_of(GITHUB_PAGING).as_deref(),
        Some("https://api.github.com/repositories/53631945/pulls?state=open&per_page=30&page=2")
    );
    assert_eq!(
        next_of(GITLAB_PAGING).as_deref(),
        Some(
            "https://gitlab.com/api/v4/projects/g%2Fr/merge_requests?page=3&per_page=2&state=opened"
        )
    );
    assert_eq!(
        next_of(FORGEJO_PAGING).as_deref(),
        Some("https://codeberg.org/api/v1/repos/o/r/pulls?limit=2&page=2&state=open")
    );
    assert_eq!(next_of(LAST_PAGE_PAGING), None);
}

fn page(body: &str, next: Option<&str>) -> WireResponse {
    WireResponse {
        status: 200,
        body: body.to_owned(),
        next_url: next.map(str::to_owned),
    }
}

fn fetched_pages(api: &Api, pages: &[(&str, WireResponse)]) -> (Vec<Value>, Vec<String>) {
    let mut asked = Vec::new();
    let items = every_page(api, &api.list(), |request| {
        asked.push(request.url.clone());
        Ok(pages
            .iter()
            .find(|(url, _)| *url == request.url)
            .map(|(_, response)| response.clone())
            .unwrap())
    })
    .unwrap();
    (items, asked)
}

#[test]
fn a_list_follows_next_links_until_the_last_page_and_keeps_every_item_in_order() {
    let api = Api::new(ForgeKind::GitHub, "github.com", "owner/repo");
    let first = "https://api.github.com/repos/owner/repo/pulls?state=open&per_page=100";
    let second = "https://api.github.com/repositories/1/pulls?state=open&per_page=100&page=2";
    let third = "https://api.github.com/repositories/1/pulls?state=open&per_page=100&page=3";
    let (items, asked) = fetched_pages(
        &api,
        &[
            (first, page("[1, 2]", Some(second))),
            (second, page("[3]", Some(third))),
            (third, page("[4]", None)),
        ],
    );
    assert_eq!(items, vec![json!(1), json!(2), json!(3), json!(4)]);
    assert_eq!(asked, vec![first, second, third]);
}

#[test]
fn a_next_link_to_another_host_is_not_followed_so_the_token_stays_with_the_forge() {
    let api = Api::new(ForgeKind::Forgejo, "codeberg.org", "owner/repo");
    let first = "https://codeberg.org/api/v1/repos/owner/repo/pulls?state=open&limit=50";
    let (items, asked) = fetched_pages(
        &api,
        &[(
            first,
            page(
                "[1]",
                Some("https://codeberg.org.evil.example/steal?page=2"),
            ),
        )],
    );
    assert_eq!(items, vec![json!(1)]);
    assert_eq!(asked, vec![first]);
}

#[test]
fn a_next_link_back_to_a_page_already_read_ends_the_list() {
    let api = Api::new(ForgeKind::GitLab, "gitlab.com", "group/repo");
    let first =
        "https://gitlab.com/api/v4/projects/group%2Frepo/merge_requests?state=opened&per_page=100";
    let second = "https://gitlab.com/api/v4/projects/group%2Frepo/merge_requests?page=2";
    let (items, asked) = fetched_pages(
        &api,
        &[
            (first, page("[1]", Some(second))),
            (second, page("[2]", Some(first))),
        ],
    );
    assert_eq!(items, vec![json!(1), json!(2)]);
    assert_eq!(asked, vec![first, second]);
}

const GITHUB_PULLS: &str = r#"[{
    "number": 12, "title": "Fix it",
    "body": "Fixes the crash.\r\n",
    "updated_at": "2026-05-02T10:00:00Z",
    "user": {"login": "octo"},
    "base": {"ref": "main", "sha": "ffffffffffffffffffffffffffffffffffffffff"},
    "head": {"ref": "fix", "sha": "0123456789abcdef0123456789abcdef01234567"}
}]"#;

const GITLAB_MERGE_REQUESTS: &str = r#"[{
    "id": 999, "iid": 12, "title": "Fix it",
    "description": "Fixes the crash.",
    "updated_at": "2026-05-02T10:00:00Z",
    "body": "not the description",
    "author": {"username": "octo"},
    "target_branch": "main", "source_branch": "fix",
    "sha": "0123456789abcdef0123456789abcdef01234567",
    "diff_refs": {
        "base_sha": "1111111111111111111111111111111111111111",
        "start_sha": "2222222222222222222222222222222222222222",
        "head_sha": "0123456789abcdef0123456789abcdef01234567"
    }
}]"#;

fn expected_pull() -> PullRequest {
    PullRequest {
        number: PrNumber::new(12).unwrap(),
        title: PrTitle::new("Fix it"),
        author: Login::new("octo"),
        base: BranchName::new("main"),
        head: BranchName::new("fix"),
        head_commit: CommitId::new(SHA).unwrap(),
        description: Prose::new("Fixes the crash."),
        updated: Moment::new("2026-05-02T10:00:00Z"),
    }
}

fn listed(kind: ForgeKind, text: &str) -> Vec<PullRequest> {
    Api::new(kind, "example.org", "owner/repo")
        .parse_pulls(&parse_items(text).unwrap())
        .unwrap()
        .into_iter()
        .map(|pull| convert::pull(&pull).unwrap())
        .collect()
}

#[test]
fn github_and_forgejo_lists_read_number_author_branches_and_head_sha() {
    assert_eq!(
        listed(ForgeKind::GitHub, GITHUB_PULLS),
        vec![expected_pull()]
    );
    assert_eq!(
        listed(ForgeKind::Forgejo, GITHUB_PULLS),
        vec![expected_pull()]
    );
}

#[test]
fn gitlab_lists_read_the_iid_not_the_global_id() {
    assert_eq!(
        listed(ForgeKind::GitLab, GITLAB_MERGE_REQUESTS),
        vec![expected_pull()]
    );
}

#[test]
fn a_single_pull_parses_like_a_listed_one() {
    let single = GITLAB_MERGE_REQUESTS
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']');
    let pull = Api::new(ForgeKind::GitLab, "gitlab.com", "owner/repo")
        .parse_pull(single)
        .and_then(|pull| convert::pull(&pull));
    assert_eq!(pull, Some(expected_pull()));
}

#[test]
fn a_list_in_the_other_forges_shape_or_with_a_bad_sha_is_malformed() {
    let api = Api::new(ForgeKind::GitLab, "gitlab.com", "owner/repo");
    assert_eq!(api.parse_pulls(&parse_items(GITHUB_PULLS).unwrap()), None);
    assert_eq!(parse_items("{\"message\": \"404\"}"), None);
    let bad_sha = GITHUB_PULLS.replace(SHA, "not-hex");
    let pulls = Api::new(ForgeKind::GitHub, "github.com", "owner/repo")
        .parse_pulls(&parse_items(&bad_sha).unwrap())
        .unwrap();
    assert_eq!(pulls.iter().map(convert::pull).next(), Some(None));
}

#[test]
fn gitlab_diff_refs_are_read_from_the_merge_request() {
    let single = GITLAB_MERGE_REQUESTS
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']');
    assert_eq!(
        Api::parse_diff_refs(single),
        Some(WireDiffRefs {
            base: "1".repeat(40),
            start: "2".repeat(40),
            head: SHA.to_owned(),
        })
    );
}

fn diff_line(kind: LineKind, text: &str) -> DiffLine {
    DiffLine {
        kind,
        text: SourceLine::new(text),
    }
}

fn changes() -> Vec<FileChange> {
    vec![
        FileChange::new(
            RelativePath::new("src/lib.rs"),
            None,
            ChangeKind::Modified,
            vec![Hunk::new(
                Line::new(5),
                Line::new(8),
                vec![
                    diff_line(LineKind::Context, "a"),
                    diff_line(LineKind::Context, "b"),
                    diff_line(LineKind::Added, "c"),
                ],
            )],
        ),
        FileChange::new(
            RelativePath::new("src/old.rs"),
            Some(RelativePath::new("src/older.rs")),
            ChangeKind::Renamed,
            vec![Hunk::new(
                Line::new(0),
                Line::new(0),
                vec![
                    diff_line(LineKind::Removed, "x"),
                    diff_line(LineKind::Context, "y"),
                ],
            )],
        ),
    ]
}

fn drafts() -> Vec<Draft> {
    vec![
        Draft {
            id: DraftId::new(1),
            file: RelativePath::new("src/lib.rs"),
            side: Side::New,
            line: Line::new(9),
            text: DraftText::new("rename this").unwrap(),
            reply: None,
            start: None,
        },
        Draft {
            id: DraftId::new(2),
            file: RelativePath::new("src/old.rs"),
            side: Side::Old,
            line: Line::new(0),
            text: DraftText::new("why removed?").unwrap(),
            reply: None,
            start: None,
        },
    ]
}

fn review_of(drafts: &[Draft], decision: Decision, body: Option<&str>) -> wire::WireReview {
    let body = body.map(|text| DraftText::new(text).unwrap());
    convert::review(
        &expected_pull(),
        drafts,
        &changes(),
        decision,
        body.as_ref(),
    )
}

fn review(decision: Decision, body: Option<&str>) -> wire::WireReview {
    review_of(&drafts(), decision, body)
}

#[test]
fn a_github_review_sends_one_based_lines_on_left_or_right_and_the_head_commit() {
    let api = Api::new(ForgeKind::GitHub, "github.com", "owner/repo");
    let request = api
        .github_review(12, &review(Decision::RequestChanges, Some("see notes")))
        .unwrap();
    assert_eq!(request.method, Method::Post);
    assert_eq!(
        request.url,
        "https://api.github.com/repos/owner/repo/pulls/12/reviews"
    );
    assert_eq!(
        request.body,
        Some(json!({
            "commit_id": SHA,
            "body": "see notes",
            "event": "REQUEST_CHANGES",
            "comments": [
                {"path": "src/lib.rs", "line": 10, "side": "RIGHT", "body": "rename this"},
                {"path": "src/old.rs", "line": 1, "side": "LEFT", "body": "why removed?"},
            ],
        }))
    );
}

#[test]
fn a_github_review_without_a_body_omits_it_rather_than_sending_null() {
    let api = Api::new(ForgeKind::GitHub, "github.com", "owner/repo");
    let request = api
        .github_review(12, &review(Decision::Approve, None))
        .unwrap();
    let body = request.body.unwrap();
    assert_eq!(body.get("body"), None);
    assert_eq!(body.get("event"), Some(&json!("APPROVE")));
}

#[test]
fn a_forgejo_review_says_approved_and_puts_the_line_on_its_side_only() {
    let api = Api::new(ForgeKind::Forgejo, "codeberg.org", "owner/repo");
    let request = api.forgejo_review(12, &review(Decision::Approve, Some("lgtm")));
    assert_eq!(
        request.url,
        "https://codeberg.org/api/v1/repos/owner/repo/pulls/12/reviews"
    );
    assert_eq!(
        request.body,
        Some(json!({
            "commit_id": SHA,
            "body": "lgtm",
            "event": "APPROVED",
            "comments": [
                {"path": "src/lib.rs", "body": "rename this", "new_position": 10, "old_position": 0},
                {"path": "src/old.rs", "body": "why removed?", "new_position": 0, "old_position": 1},
            ],
        }))
    );
}

fn refs() -> WireDiffRefs {
    WireDiffRefs {
        base: "b".repeat(40),
        start: "s".repeat(40),
        head: SHA.to_owned(),
    }
}

const MERGE_REQUEST: &str = "https://gitlab.com/api/v4/projects/group%2Frepo/merge_requests/12";

fn gitlab_position(old_path: &str, new_path: &str, lines: &Value) -> Value {
    let mut position = json!({
        "position_type": "text",
        "base_sha": "b".repeat(40),
        "start_sha": "s".repeat(40),
        "head_sha": SHA,
        "old_path": old_path,
        "new_path": new_path,
    });
    if let (Some(object), Some(lines)) = (position.as_object_mut(), lines.as_object()) {
        object.extend(lines.clone());
    }
    position
}

#[test]
fn a_gitlab_review_drafts_a_positioned_note_per_draft_and_one_for_the_summary() {
    let api = Api::new(ForgeKind::GitLab, "gitlab.com", "group/repo");
    let requests = api.gitlab_pending_notes(12, &review(Decision::Approve, Some("lgtm")), &refs());
    let draft_notes = format!("{MERGE_REQUEST}/draft_notes");
    assert_eq!(
        requests,
        vec![
            Request {
                method: Method::Post,
                url: draft_notes.clone(),
                body: Some(json!({"note": "rename this", "position": gitlab_position(
                    "src/lib.rs",
                    "src/lib.rs",
                    &json!({"old_line": 7, "new_line": 10}),
                )})),
            },
            Request {
                method: Method::Post,
                url: draft_notes.clone(),
                body: Some(json!({"note": "why removed?", "position": gitlab_position(
                    "src/older.rs",
                    "src/old.rs",
                    &json!({"old_line": 1}),
                )})),
            },
            Request {
                method: Method::Post,
                url: draft_notes,
                body: Some(json!({"note": "lgtm"})),
            },
        ]
    );
}

#[test]
fn a_gitlab_draft_on_an_added_line_sends_only_the_new_line() {
    let api = Api::new(ForgeKind::GitLab, "gitlab.com", "group/repo");
    let mut added = drafts();
    added.truncate(1);
    if let Some(draft) = added.first_mut() {
        draft.line = Line::new(10);
    }
    let requests =
        api.gitlab_pending_notes(12, &review_of(&added, Decision::Comment, None), &refs());
    assert_eq!(
        requests.first().and_then(|request| request.body.clone()),
        Some(json!({"note": "rename this", "position": gitlab_position(
            "src/lib.rs",
            "src/lib.rs",
            &json!({"new_line": 11}),
        )}))
    );
}

#[test]
fn gitlab_publishes_the_pending_notes_at_once_and_approves_the_reviewed_head() {
    let api = Api::new(ForgeKind::GitLab, "gitlab.com", "group/repo");
    let publish = api.gitlab_publish(12);
    assert_eq!(
        publish.url,
        format!("{MERGE_REQUEST}/draft_notes/bulk_publish")
    );
    let config = api.config(&publish, Some("tok"));
    assert!(config.contains("request = \"POST\"\n"), "{config}");
    assert!(!config.contains("data-binary"), "{config}");
    let approve = api.gitlab_approve(12, &review(Decision::Approve, None));
    assert_eq!(approve.url, format!("{MERGE_REQUEST}/approve"));
    assert_eq!(approve.body, Some(json!({"sha": SHA})));
    let discard = api.gitlab_discard(12, PendingNote::new(77));
    assert_eq!(discard.url, format!("{MERGE_REQUEST}/draft_notes/77"));
    assert!(
        api.config(&discard, Some("tok"))
            .contains("request = \"DELETE\"\n")
    );
}

fn answering<'asked>(
    asked: &'asked mut Vec<Request>,
    fails: impl Fn(&Request) -> bool + 'asked,
) -> impl FnMut(&Request) -> Result<ResponseBody, ForgeError> + 'asked {
    move |request| {
        asked.push(request.clone());
        if fails(request) {
            return Err(ForgeError::Http {
                status: HttpStatus::new(500),
                body: ResponseBody::new("boom"),
            });
        }
        Ok(ResponseBody::new(&json!({"id": asked.len()}).to_string()))
    }
}

#[test]
fn a_gitlab_review_that_fails_before_publishing_discards_its_drafted_notes_and_sends_nothing() {
    let api = Api::new(ForgeKind::GitLab, "gitlab.com", "group/repo");
    let mut asked = Vec::new();
    let error = send_gitlab(
        &api,
        PrNumber::new(12).unwrap(),
        &review(Decision::Approve, Some("lgtm")),
        &refs(),
        answering(&mut asked, |request| {
            request.body.as_ref().and_then(|body| body.get("note")) == Some(&json!("lgtm"))
        }),
    )
    .unwrap_err();
    assert!(error.sent().is_empty(), "{error}");
    let tail: Vec<(Method, String)> = asked
        .iter()
        .skip(3)
        .map(|request| (request.method, request.url.clone()))
        .collect();
    assert_eq!(
        tail,
        vec![
            (Method::Delete, format!("{MERGE_REQUEST}/draft_notes/1")),
            (Method::Delete, format!("{MERGE_REQUEST}/draft_notes/2")),
        ]
    );
}

#[test]
fn a_gitlab_approval_refused_after_publishing_says_every_draft_was_sent() {
    let api = Api::new(ForgeKind::GitLab, "gitlab.com", "group/repo");
    let mut asked = Vec::new();
    let error = send_gitlab(
        &api,
        PrNumber::new(12).unwrap(),
        &review(Decision::Approve, None),
        &refs(),
        answering(&mut asked, |request| request.url.ends_with("/approve")),
    )
    .unwrap_err();
    assert_eq!(error.sent(), &[DraftId::new(1), DraftId::new(2)]);
    assert!(matches!(
        error,
        ForgeError::Unfinished {
            unsent: Unsent::Approval,
            ..
        }
    ));
    let endpoints: Vec<&str> = asked
        .iter()
        .map(|request| request.url.rsplit('/').next().unwrap())
        .collect();
    assert_eq!(
        endpoints,
        vec!["draft_notes", "draft_notes", "bulk_publish", "approve"]
    );
}

#[test]
fn pending_notes_that_cannot_be_removed_are_named_in_the_error() {
    let api = Api::new(ForgeKind::GitLab, "gitlab.com", "group/repo");
    let mut asked = Vec::new();
    let error = send_gitlab(
        &api,
        PrNumber::new(12).unwrap(),
        &review(Decision::Comment, None),
        &refs(),
        answering(&mut asked, |request| {
            request.url.ends_with("/bulk_publish") || request.method == Method::Delete
        }),
    )
    .unwrap_err();
    assert!(
        matches!(&error, ForgeError::PendingLeft { pending, .. } if pending.get() == 2),
        "{error}"
    );
    assert!(error.sent().is_empty());
}

#[test]
fn a_gitlab_request_for_changes_publishes_but_never_approves() {
    let api = Api::new(ForgeKind::GitLab, "gitlab.com", "group/repo");
    let mut asked = Vec::new();
    send_gitlab(
        &api,
        PrNumber::new(12).unwrap(),
        &review(Decision::RequestChanges, Some("no")),
        &refs(),
        answering(&mut asked, |_| false),
    )
    .unwrap();
    let endpoints: Vec<&str> = asked
        .iter()
        .map(|request| request.url.rsplit('/').next().unwrap())
        .collect();
    assert_eq!(
        endpoints,
        vec!["draft_notes", "draft_notes", "draft_notes", "bulk_publish"]
    );
}

#[test]
fn a_github_reply_that_fails_after_the_review_names_the_drafts_already_sent() {
    let api = Api::new(ForgeKind::GitHub, "github.com", "owner/repo");
    let review = review_of(&drafts_with_replies(), Decision::Comment, None);
    let deliveries = api.github_deliveries(12, &review);
    let mut asked = Vec::new();
    let error = send_in_turn(
        &deliveries,
        answering(&mut asked, |request| request.url.contains("/issues/")),
    )
    .unwrap_err();
    assert_eq!(
        error.sent(),
        &[DraftId::new(1), DraftId::new(2), DraftId::new(3)]
    );
    assert!(matches!(
        error,
        ForgeError::Unfinished {
            unsent: Unsent::Drafts,
            ..
        }
    ));
    let mut asked_once = Vec::new();
    let first = send_in_turn(
        &deliveries,
        answering(&mut asked_once, |request| request.url.ends_with("/reviews")),
    )
    .unwrap_err();
    assert!(matches!(first, ForgeError::Http { .. }), "{first}");
    assert_eq!(asked_once.len(), 1);
}

#[test]
fn a_github_reply_that_fails_after_a_summary_only_review_says_the_review_went_through() {
    let api = Api::new(ForgeKind::GitHub, "github.com", "owner/repo");
    let replies: Vec<Draft> = drafts_with_replies().split_off(2);
    let review = review_of(&replies, Decision::Comment, Some("ok"));
    let mut asked = Vec::new();
    let error = send_in_turn(
        &api.github_deliveries(12, &review),
        answering(&mut asked, |request| request.url.ends_with("/replies")),
    )
    .unwrap_err();
    assert!(
        matches!(
            error,
            ForgeError::Unfinished {
                unsent: Unsent::Drafts,
                ..
            }
        ),
        "{error}"
    );
    assert!(error.sent().is_empty());
}

fn client(token: Option<&str>) -> ForgeClient {
    ForgeClient {
        forge: Forge::new(
            ForgeKind::GitHub,
            Host::new("github.com").unwrap(),
            RepoSlug::new("owner/repo").unwrap(),
        ),
        token: token.and_then(Token::new),
    }
}

#[test]
fn the_client_never_prints_its_token() {
    let printed = format!("{:?}", client(Some("ghp_secret")));
    assert!(!printed.contains("ghp_secret"), "{printed}");
}

#[test]
fn submitting_without_a_token_names_the_variables_to_set_before_any_request() {
    let error = client(None)
        .submit(
            &expected_pull(),
            &drafts(),
            &changes(),
            Decision::Comment,
            None,
        )
        .unwrap_err();
    assert!(matches!(error, ForgeError::NoToken(ForgeKind::GitHub)));
    let message = error.to_string();
    assert!(message.contains("GITHUB_TOKEN"), "{message}");
    assert!(message.contains("GH_TOKEN"), "{message}");
}

#[test]
fn an_unknown_host_says_how_to_choose_the_forge() {
    let message = ForgeError::UnknownHost(Host::new("git.example.org").unwrap()).to_string();
    assert!(
        message.contains("REVIEW_FORGE=github|gitlab|forgejo"),
        "{message}"
    );
}

#[test]
fn the_client_names_head_refs_for_its_forge() {
    assert_eq!(
        client(None).head_ref(PrNumber::new(5).unwrap()).as_str(),
        "refs/pull/5/head"
    );
}

#[test]
fn a_pull_without_a_body_has_an_empty_description() {
    let text = GITHUB_PULLS.replace("\"Fixes the crash.\\r\\n\"", "null");
    let pulls = listed(ForgeKind::GitHub, &text);
    assert!(pulls[0].description.is_empty(), "{:?}", pulls[0]);
    assert_eq!(pulls[0].updated, expected_pull().updated);
}

#[test]
fn each_conversation_endpoint_is_read_from_its_forges_path_with_its_forges_page_size() {
    let github = Api::new(ForgeKind::GitHub, "github.com", "owner/repo");
    let forgejo = Api::new(ForgeKind::Forgejo, "codeberg.org", "owner/repo");
    let gitlab = Api::new(ForgeKind::GitLab, "gitlab.com", "group/repo");
    let urls = [
        (
            github.issue_comments(12),
            "https://api.github.com/repos/owner/repo/issues/12/comments?per_page=100",
        ),
        (
            github.reviews(12),
            "https://api.github.com/repos/owner/repo/pulls/12/reviews?per_page=100",
        ),
        (
            github.review_comments(12),
            "https://api.github.com/repos/owner/repo/pulls/12/comments?per_page=100",
        ),
        (
            forgejo.issue_comments(12),
            "https://codeberg.org/api/v1/repos/owner/repo/issues/12/comments?limit=50",
        ),
        (
            forgejo.reviews(12),
            "https://codeberg.org/api/v1/repos/owner/repo/pulls/12/reviews?limit=50",
        ),
        (
            forgejo.forgejo_review_comments(12, 20),
            "https://codeberg.org/api/v1/repos/owner/repo/pulls/12/reviews/20/comments",
        ),
        (
            gitlab.discussions(12),
            "https://gitlab.com/api/v4/projects/group%2Frepo/merge_requests/12/discussions?per_page=100",
        ),
    ];
    for (request, expected) in urls {
        assert_eq!(request.method, Method::Get);
        assert_eq!(request.url, expected);
    }
}

const GITHUB_ISSUE_COMMENTS: &str = r#"[
    {"id": 501, "user": {"login": "carol"}, "body": "Can we ship this today?",
     "created_at": "2026-05-01T09:00:00Z", "updated_at": "2026-05-01T09:00:00Z",
     "author_association": "MEMBER"}
]"#;

const GITHUB_REVIEWS: &str = r#"[
    {"id": 80, "user": {"login": "dave"}, "body": "", "state": "COMMENTED",
     "submitted_at": "2026-05-01T10:00:00Z", "commit_id": "0123456789abcdef0123456789abcdef01234567"},
    {"id": 81, "user": {"login": "erin"}, "body": "", "state": "APPROVED",
     "submitted_at": "2026-05-01T12:00:00Z"},
    {"id": 82, "user": {"login": "dave"}, "body": "Please split this.", "state": "CHANGES_REQUESTED",
     "submitted_at": "2026-05-01T08:00:00Z"},
    {"id": 83, "user": {"login": "octo"}, "body": "not sent yet", "state": "PENDING"}
]"#;

const GITHUB_REVIEW_COMMENTS: &str = r#"[
    {"id": 1001, "pull_request_review_id": 80, "user": {"login": "dave"}, "body": "Off by one?",
     "path": "src/lib.rs", "line": 10, "original_line": 8, "side": "RIGHT",
     "diff_hunk": "@@ -1,3 +1,3 @@", "created_at": "2026-05-01T10:00:00Z"},
    {"id": 1002, "pull_request_review_id": 84, "in_reply_to_id": 1001, "user": {"login": "octo"},
     "body": "Fixed.", "path": "src/lib.rs", "line": 10, "original_line": 8, "side": "RIGHT",
     "created_at": "2026-05-01T11:00:00Z"},
    {"id": 1003, "pull_request_review_id": 85, "in_reply_to_id": 1001, "user": {"login": "dave"},
     "body": "Thanks", "path": "src/lib.rs", "line": 10, "original_line": 8, "side": "RIGHT",
     "created_at": "2026-05-01T11:30:00Z"},
    {"id": 1004, "pull_request_review_id": 86, "user": {"login": "erin"}, "body": "Gone now",
     "path": "src/old.rs", "line": null, "original_line": 4, "side": "LEFT",
     "created_at": "2026-05-01T07:00:00Z"}
]"#;

fn conversation(threads: &[WireThread]) -> Conversation {
    convert::conversation(&expected_pull().description, threads).unwrap()
}

fn github_conversation() -> Conversation {
    let mut threads =
        wire::parse_issue_comments(&parse_items(GITHUB_ISSUE_COMMENTS).unwrap()).unwrap();
    threads.extend(
        wire::parse_reviews(&parse_items(GITHUB_REVIEWS).unwrap())
            .unwrap()
            .threads,
    );
    threads.extend(
        wire::parse_github_code_threads(&parse_items(GITHUB_REVIEW_COMMENTS).unwrap()).unwrap(),
    );
    conversation(&threads)
}

fn ids(conversation: &Conversation) -> Vec<&str> {
    conversation
        .threads
        .iter()
        .map(|thread| thread.id.as_str())
        .collect()
}

fn code_place(file: &str, side: Side, line: u32) -> CodePlace {
    CodePlace {
        file: RelativePath::new(file),
        side,
        line: Line::new(line),
    }
}

fn bodies(conversation: &Conversation, id: &str) -> Vec<(String, String)> {
    conversation
        .thread(&ThreadId::new(id).unwrap())
        .unwrap()
        .notes
        .iter()
        .map(|note| {
            (
                note.author.as_str().to_owned(),
                note.body.as_str().to_owned(),
            )
        })
        .collect()
}

#[test]
fn github_threads_come_in_order_of_their_first_note_from_all_three_endpoints() {
    let conversation = github_conversation();
    assert_eq!(conversation.description, Prose::new("Fixes the crash."));
    assert_eq!(
        ids(&conversation),
        vec!["1004", "review-82", "issue-501", "1001", "review-81"]
    );
}

#[test]
fn github_replies_join_their_root_comment_and_the_root_places_the_thread() {
    let conversation = github_conversation();
    let thread = conversation
        .thread(&ThreadId::new("1001").unwrap())
        .unwrap();
    assert_eq!(thread.place, Some(code_place("src/lib.rs", Side::New, 9)));
    assert_eq!(thread.state, ThreadState::Open);
    assert_eq!(
        bodies(&conversation, "1001"),
        vec![
            ("dave".to_owned(), "Off by one?".to_owned()),
            ("octo".to_owned(), "Fixed.".to_owned()),
            ("dave".to_owned(), "Thanks".to_owned()),
        ]
    );
    assert_eq!(thread.started(), Some(&Moment::new("2026-05-01T10:00:00Z")));
}

#[test]
fn a_github_comment_whose_line_is_gone_is_outdated_at_its_original_line() {
    let conversation = github_conversation();
    let thread = conversation
        .thread(&ThreadId::new("1004").unwrap())
        .unwrap();
    assert_eq!(thread.state, ThreadState::Outdated);
    assert_eq!(thread.place, Some(code_place("src/old.rs", Side::Old, 3)));
}

#[test]
fn reviews_keep_verdicts_and_bodies_but_drop_pending_and_empty_comments() {
    let conversation = github_conversation();
    let kind = |id: &str| {
        conversation
            .thread(&ThreadId::new(id).unwrap())
            .map(|thread| (thread.place.clone(), thread.notes[0].kind))
    };
    assert_eq!(kind("review-81"), Some((None, NoteKind::Approved)));
    assert_eq!(kind("review-82"), Some((None, NoteKind::ChangesRequested)));
    assert_eq!(kind("issue-501"), Some((None, NoteKind::Comment)));
    assert_eq!(kind("review-80"), None);
    assert_eq!(kind("review-83"), None);
}

const GITLAB_DISCUSSIONS: &str = r#"[
    {"id": "6a9c1750b37d513a43987b574953fceb50b03ce7", "individual_note": true, "notes": [
        {"id": 1, "type": null, "body": "added 1 commit", "author": {"username": "octo"},
         "created_at": "2026-05-01T06:00:00.000Z", "system": true, "noteable_type": "MergeRequest"}
    ]},
    {"id": "aaaaaaaabbbbbbbbccccccccddddddddeeeeeeee", "individual_note": true, "notes": [
        {"id": 2, "type": null, "body": "Looks good overall", "author": {"username": "carol"},
         "created_at": "2026-05-01T09:00:00.000Z", "system": false, "resolvable": false}
    ]},
    {"id": "bbbbbbbbccccccccddddddddeeeeeeeeffffffff", "individual_note": false, "notes": [
        {"id": 3, "type": "DiffNote", "body": "Off by one?", "author": {"username": "dave"},
         "created_at": "2026-05-01T10:00:00.000Z", "system": false, "resolvable": true, "resolved": true,
         "position": {"base_sha": "1111111111111111111111111111111111111111",
                      "start_sha": "2222222222222222222222222222222222222222",
                      "head_sha": "0123456789abcdef0123456789abcdef01234567",
                      "position_type": "text", "old_path": "src/lib.rs", "new_path": "src/lib.rs",
                      "old_line": null, "new_line": 10}},
        {"id": 4, "type": "DiffNote", "body": "Fixed.", "author": {"username": "octo"},
         "created_at": "2026-05-01T11:00:00.000Z", "system": false, "resolvable": true, "resolved": true,
         "position": {"position_type": "text", "old_path": "src/lib.rs", "new_path": "src/lib.rs",
                      "old_line": null, "new_line": 10}},
        {"id": 5, "type": null, "body": "resolved all threads", "author": {"username": "octo"},
         "created_at": "2026-05-01T11:01:00.000Z", "system": true}
    ]},
    {"id": "ccccccccddddddddeeeeeeeeffffffff00000000", "individual_note": false, "notes": [
        {"id": 6, "type": "DiffNote", "body": "Why removed?", "author": {"username": "erin"},
         "created_at": "2026-05-01T07:00:00.000Z", "system": false, "resolvable": true, "resolved": false,
         "position": {"position_type": "text", "old_path": "src/gone.rs", "new_path": "src/gone.rs",
                      "old_line": 4, "new_line": null}}
    ]}
]"#;

#[test]
fn gitlab_discussions_drop_system_notes_and_place_positioned_ones_on_their_side() {
    let conversation = conversation(
        &wire::parse_discussions(&parse_items(GITLAB_DISCUSSIONS).unwrap(), SHA).unwrap(),
    );
    assert_eq!(
        ids(&conversation),
        vec![
            "ccccccccddddddddeeeeeeeeffffffff00000000",
            "aaaaaaaabbbbbbbbccccccccddddddddeeeeeeee",
            "bbbbbbbbccccccccddddddddeeeeeeeeffffffff",
        ]
    );
    let [removed, general, resolved] = conversation.threads.as_slice() else {
        panic!("{conversation:?}");
    };
    assert_eq!(removed.place, Some(code_place("src/gone.rs", Side::Old, 3)));
    assert_eq!(removed.state, ThreadState::Open);
    assert_eq!(general.place, None);
    assert_eq!(resolved.place, Some(code_place("src/lib.rs", Side::New, 9)));
    assert_eq!(resolved.state, ThreadState::Resolved);
    assert_eq!(
        bodies(&conversation, "bbbbbbbbccccccccddddddddeeeeeeeeffffffff"),
        vec![
            ("dave".to_owned(), "Off by one?".to_owned()),
            ("octo".to_owned(), "Fixed.".to_owned()),
        ]
    );
}

fn gitlab_diff_note(id: &str, head_sha: &str, resolved: bool) -> Value {
    json!({"id": id, "individual_note": false, "notes": [
        {"id": 7, "type": "DiffNote", "body": "Typo", "author": {"username": "erin"},
         "created_at": "2026-05-01T07:00:00.000Z", "system": false, "resolvable": true,
         "resolved": resolved,
         "position": {"base_sha": "1".repeat(40), "start_sha": "2".repeat(40),
                      "head_sha": head_sha, "position_type": "text",
                      "old_path": "src/lib.rs", "new_path": "src/lib.rs",
                      "old_line": null, "new_line": 4}}
    ]})
}

#[test]
fn a_gitlab_thread_written_on_an_earlier_head_is_outdated_at_its_line_unless_resolved() {
    let earlier = "f".repeat(40);
    let threads = wire::parse_discussions(
        &[
            gitlab_diff_note("current", SHA, false),
            gitlab_diff_note("earlier", &earlier, false),
            gitlab_diff_note("settled", &earlier, true),
        ],
        SHA,
    )
    .unwrap();
    let states: Vec<(&str, ThreadState)> = threads
        .iter()
        .map(|thread| (thread.id.as_str(), thread.state))
        .collect();
    assert_eq!(
        states,
        vec![
            ("current", ThreadState::Open),
            ("earlier", ThreadState::Outdated),
            ("settled", ThreadState::Resolved),
        ]
    );
    let placed = conversation(&threads);
    assert_eq!(
        placed
            .thread(&ThreadId::new("earlier").unwrap())
            .and_then(|thread| thread.place.clone()),
        Some(code_place("src/lib.rs", Side::New, 3))
    );
}

const FORGEJO_REVIEWS: &str = r#"[
    {"id": 20, "user": {"login": "dave"}, "body": "", "state": "COMMENT", "comments_count": 3,
     "submitted_at": "2026-05-01T10:00:00+02:00", "commit_id": "0123456789abcdef0123456789abcdef01234567"},
    {"id": 21, "user": {"login": "octo"}, "body": "", "state": "COMMENT", "comments_count": 1,
     "submitted_at": "2026-05-01T11:00:00+02:00"},
    {"id": 22, "user": {"login": "erin"}, "body": "", "state": "APPROVED", "comments_count": 0,
     "submitted_at": "2026-05-01T12:00:00+02:00"},
    {"id": 23, "user": {"login": "dave"}, "body": "Needs tests.", "state": "REQUEST_CHANGES",
     "comments_count": 0, "submitted_at": "2026-05-01T09:00:00+02:00"},
    {"id": 24, "user": {"login": "octo"}, "body": "", "state": "PENDING", "comments_count": 2,
     "submitted_at": "2026-05-01T13:00:00+02:00"}
]"#;

const FORGEJO_REVIEW_20: &str = r#"[
    {"id": 300, "body": "Off by one?", "user": {"login": "dave"}, "path": "src/lib.rs",
     "position": 10, "original_position": 0, "resolver": null, "pull_request_review_id": 20,
     "diff_hunk": "@@ -1,3 +1,3 @@", "created_at": "2026-05-01T10:00:00+02:00"},
    {"id": 301, "body": "Why removed?", "user": {"login": "dave"}, "path": "src/gone.rs",
     "position": 0, "original_position": 4, "resolver": {"login": "octo"},
     "pull_request_review_id": 20, "created_at": "2026-05-01T10:00:00+02:00"},
    {"id": 302, "body": "And rename it.", "user": {"login": "dave"}, "path": "src/lib.rs",
     "position": 10, "original_position": 0, "resolver": null, "pull_request_review_id": 20,
     "created_at": "2026-05-01T10:00:00+02:00"}
]"#;

const FORGEJO_REVIEW_21: &str = r#"[
    {"id": 310, "body": "Fixed.", "user": {"login": "octo"}, "path": "src/lib.rs",
     "position": 10, "original_position": 0, "resolver": null, "pull_request_review_id": 21,
     "created_at": "2026-05-01T11:00:00+02:00"}
]"#;

#[test]
fn forgejo_reads_comments_only_of_sent_reviews_that_have_some() {
    let reviews = wire::parse_reviews(&parse_items(FORGEJO_REVIEWS).unwrap()).unwrap();
    assert_eq!(reviews.with_comments, vec![20, 21]);
    let conversation = conversation(&reviews.threads);
    assert_eq!(ids(&conversation), vec!["review-23", "review-22"]);
    assert_eq!(
        conversation.threads[0].notes[0].kind,
        NoteKind::ChangesRequested
    );
    assert_eq!(conversation.threads[1].notes[0].kind, NoteKind::Approved);
}

#[test]
fn forgejo_comments_on_one_line_from_every_review_make_one_thread() {
    let mut comments =
        wire::parse_forgejo_comments(&parse_items(FORGEJO_REVIEW_21).unwrap()).unwrap();
    comments
        .extend(wire::parse_forgejo_comments(&parse_items(FORGEJO_REVIEW_20).unwrap()).unwrap());
    let conversation = conversation(&wire::forgejo_threads(comments));
    assert_eq!(ids(&conversation), vec!["300", "301"]);
    let [line, removed] = conversation.threads.as_slice() else {
        panic!("{conversation:?}");
    };
    assert_eq!(line.place, Some(code_place("src/lib.rs", Side::New, 9)));
    assert_eq!(line.state, ThreadState::Open);
    assert_eq!(
        bodies(&conversation, "300"),
        vec![
            ("dave".to_owned(), "Off by one?".to_owned()),
            ("dave".to_owned(), "And rename it.".to_owned()),
            ("octo".to_owned(), "Fixed.".to_owned()),
        ]
    );
    assert_eq!(removed.place, Some(code_place("src/gone.rs", Side::Old, 3)));
    assert_eq!(removed.state, ThreadState::Resolved);
}

fn drafts_with_replies() -> Vec<Draft> {
    let mut drafts = drafts();
    let reply = |id: u32, text: &str, thread: &str| Draft {
        id: DraftId::new(id),
        file: RelativePath::new("src/lib.rs"),
        side: Side::New,
        line: Line::new(9),
        text: DraftText::new(text).unwrap(),
        reply: ThreadId::new(thread),
        start: None,
    };
    drafts.push(reply(3, "done", "1001"));
    drafts.push(reply(4, "will do", "issue-501"));
    drafts
}

#[test]
fn a_github_reply_answers_its_thread_and_stays_out_of_the_review() {
    let api = Api::new(ForgeKind::GitHub, "github.com", "owner/repo");
    let review = review_of(&drafts_with_replies(), Decision::Comment, None);
    let sent = api.github_review(12, &review).unwrap().body.unwrap();
    assert_eq!(sent["comments"].as_array().unwrap().len(), 2, "{sent}");
    assert_eq!(
        api.github_replies(12, &review),
        vec![
            Request {
                method: Method::Post,
                url: "https://api.github.com/repos/owner/repo/pulls/12/comments/1001/replies"
                    .to_owned(),
                body: Some(json!({"body": "done"})),
            },
            Request {
                method: Method::Post,
                url: "https://api.github.com/repos/owner/repo/issues/12/comments".to_owned(),
                body: Some(json!({"body": "will do"})),
            },
        ]
    );
}

#[test]
fn a_github_submit_of_only_replies_posts_no_empty_review_unless_it_decides() {
    let api = Api::new(ForgeKind::GitHub, "github.com", "owner/repo");
    let replies: Vec<Draft> = drafts_with_replies().split_off(2);
    assert_eq!(
        api.github_review(12, &review_of(&replies, Decision::Comment, None)),
        None
    );
    assert!(
        api.github_review(12, &review_of(&replies, Decision::Comment, Some("ok")))
            .is_some()
    );
    let approval = api
        .github_review(12, &review_of(&replies, Decision::Approve, None))
        .unwrap();
    assert_eq!(approval.body.unwrap()["comments"], json!([]));
}

#[test]
fn a_gitlab_reply_is_a_draft_note_on_its_discussion() {
    let api = Api::new(ForgeKind::GitLab, "gitlab.com", "group/repo");
    let requests = api.gitlab_pending_notes(
        12,
        &review_of(&drafts_with_replies(), Decision::Comment, None),
        &refs(),
    );
    let replies: Vec<Option<Value>> = requests
        .iter()
        .skip(2)
        .map(|request| request.body.clone())
        .collect();
    assert_eq!(
        replies,
        vec![
            Some(json!({"note": "done", "in_reply_to_discussion_id": "1001"})),
            Some(json!({"note": "will do", "in_reply_to_discussion_id": "issue-501"})),
        ]
    );
}

#[test]
fn a_forgejo_reply_is_a_review_comment_on_its_own_line() {
    let api = Api::new(ForgeKind::Forgejo, "codeberg.org", "owner/repo");
    let request = api.forgejo_review(
        12,
        &review_of(&drafts_with_replies(), Decision::Comment, None),
    );
    let comments = request.body.unwrap()["comments"].clone();
    assert_eq!(
        comments[2],
        json!({"path": "src/lib.rs", "body": "done", "new_position": 10, "old_position": 0})
    );
    assert_eq!(comments.as_array().unwrap().len(), 4);
}

#[test]
fn each_forge_spells_its_own_web_links() {
    let commit = CommitId::new("0123456789abcdef").unwrap();
    let file = |lines: Option<Span>| Linked::File {
        commit: commit.clone(),
        path: RelativePath::new("src/a b.rs"),
        lines,
    };
    let range = Span::new(Line::new(9), Line::new(11));
    let url = |kind: ForgeKind, linked: &Linked| wire::web_url(kind, "h.org", "o/r", linked);
    let number = PrNumber::new(7).unwrap();
    assert_eq!(
        url(ForgeKind::GitHub, &Linked::Pull(number)),
        "https://h.org/o/r/pull/7"
    );
    assert_eq!(
        url(ForgeKind::GitLab, &Linked::Pull(number)),
        "https://h.org/o/r/-/merge_requests/7"
    );
    assert_eq!(
        url(ForgeKind::Forgejo, &Linked::Pull(number)),
        "https://h.org/o/r/pulls/7"
    );
    assert_eq!(
        url(ForgeKind::GitHub, &file(range)),
        "https://h.org/o/r/blob/0123456789abcdef/src/a%20b.rs#L10-L12"
    );
    assert_eq!(
        url(ForgeKind::GitLab, &file(range)),
        "https://h.org/o/r/-/blob/0123456789abcdef/src/a%20b.rs#L10-12"
    );
    assert_eq!(
        url(ForgeKind::Forgejo, &file(Some(Span::line(Line::new(0))))),
        "https://h.org/o/r/src/commit/0123456789abcdef/src/a%20b.rs#L1"
    );
    assert_eq!(
        url(ForgeKind::GitLab, &Linked::Issue(number)),
        "https://h.org/o/r/-/issues/7"
    );
    assert_eq!(
        url(ForgeKind::GitLab, &Linked::Commit(commit.clone())),
        "https://h.org/o/r/-/commit/0123456789abcdef"
    );
    assert_eq!(
        url(ForgeKind::GitHub, &Linked::User(Login::new("ann"))),
        "https://h.org/ann"
    );
}

#[test]
fn a_gitlab_note_links_to_its_anchor_on_the_merge_request() {
    let threads = wire::anchored(
        wire::parse_discussions(
            &[
                json!({"id": "d1", "notes": [{"id": 42, "body": "hi", "author": {"username": "ann"},
                "created_at": "2026-01-01T00:00:00Z", "system": false}]}),
            ],
            SHA,
        )
        .unwrap(),
        "https://g.org/o/r/-/merge_requests/3",
    );
    assert_eq!(
        threads[0].notes[0].link.as_deref(),
        Some("https://g.org/o/r/-/merge_requests/3#note_42")
    );
}

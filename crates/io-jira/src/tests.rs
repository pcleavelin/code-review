use domain::{CardFamily, CardStage, Host, JiraSite, WebLink};

use super::{HttpStatus, convert, wire};

const ANSWER: &str = r#"{"expand":"renderedFields","id":"10002","key":"PROJ-3014","fields":{"summary":"Speed up the parser","issuetype":{"name":"Story","subtask":false},"assignee":{"displayName":"Ann Smith","active":true},"status":{"name":"In Progress","statusCategory":{"key":"indeterminate"}},"description":"Lines move to the *new* system.\r\n\r\nh2. Done when\r\n* every line moves"}}"#;

const UNASSIGNED: &str = r#"{"key":"PROJ-7","fields":{"summary":"Fix it","issuetype":{"name":"Bug"},"assignee":null,"status":{"name":"To Do"},"description":null}}"#;

fn site() -> JiraSite {
    JiraSite::new(Host::new("acme.atlassian.net").unwrap())
}

fn described(wiki: &str, attachments: &str) -> String {
    let answer = format!(
        r#"{{"key":"PROJ-1","fields":{{"summary":"s","issuetype":{{"name":"Sub-task","subtask":true}},"status":{{"name":"Shipped","statusCategory":{{"key":"done"}}}},"description":{},"attachment":{attachments}}}}}"#,
        serde_json::Value::String(wiki.to_owned())
    );
    wire::parse_card(&answer).unwrap().description
}

#[test]
fn a_card_reads_its_summary_status_type_assignee_description_and_link() {
    let card = convert::card(&site(), &wire::parse_card(ANSWER).unwrap()).unwrap();
    assert_eq!(card.key.as_str(), "PROJ-3014");
    assert_eq!(card.summary.as_str(), "Speed up the parser");
    assert_eq!(card.status.as_str(), "In Progress");
    assert_eq!(card.stage, CardStage::InProgress);
    assert_eq!(card.kind.as_str(), "Story");
    assert_eq!(card.family, CardFamily::Story);
    assert_eq!(
        card.assignee.map(|who| who.to_string()).as_deref(),
        Some("Ann Smith")
    );
    assert_eq!(
        card.description.as_str(),
        "Lines move to the **new** system.\n\n## Done when\n\n- every line moves"
    );
    assert_eq!(
        card.link.map(|link| link.as_str().to_owned()).as_deref(),
        Some("https://acme.atlassian.net/browse/PROJ-3014")
    );
}

#[test]
fn an_unassigned_card_without_a_description_or_category_still_reads() {
    let card = convert::card(&site(), &wire::parse_card(UNASSIGNED).unwrap()).unwrap();
    assert!(card.assignee.is_none());
    assert!(card.description.as_str().is_empty());
    assert_eq!(card.stage, CardStage::ToDo);
    assert_eq!(card.family, CardFamily::Bug);
    assert!(wire::parse_card(r#"{"key":"PROJ-7","fields":{}}"#).is_none());
}

#[test]
fn a_subtask_is_a_task_and_a_done_category_is_done() {
    let answer = r#"{"key":"PROJ-1","fields":{"summary":"s","issuetype":{"name":"Chore","subtask":true},"status":{"name":"Shipped","statusCategory":{"key":"done"}}}}"#;
    let card = wire::parse_card(answer).unwrap();
    assert_eq!(card.family, CardFamily::Task);
    assert_eq!(card.stage, CardStage::Done);
    let other = answer.replace("true", "false");
    assert_eq!(wire::parse_card(&other).unwrap().family, CardFamily::Other);
}

#[test]
fn wiki_headings_lists_and_marks_become_markdown() {
    let wiki = "h3. Overview\n\n*Repro:*\n\n# Open a {{local}} file\n## Pick _one_ line\n# Hide -it- now -- or later\n\n* The sum updates\n** nested";
    assert_eq!(
        described(wiki, "[]"),
        "### Overview\n\n**Repro:**\n\n1. Open a `local` file\n   1. Pick *one* line\n1. Hide ~~it~~ now \u{2013} or later\n\n- The sum updates\n  - nested"
    );
}

#[test]
fn wiki_links_name_their_text_and_drop_the_smart_link_word() {
    let wiki = "From [PROJ-3105|https://acme.atlassian.net/browse/PROJ-3105], see [https://x.test/a|https://x.test/a|smart-link] and [https://y.test/b c] for [~accountid:5570] or [~ann]; [not a link]";
    assert_eq!(
        described(wiki, "[]"),
        "From [PROJ-3105](<https://acme.atlassian.net/browse/PROJ-3105>), see [https://x.test/a](<https://x.test/a>) and [https://y.test/b c](<https://y.test/b%20c>) for **someone** or **ann**; \\[not a link\\]"
    );
}

#[test]
fn a_wiki_code_block_keeps_its_text_and_language_even_on_the_tag_lines() {
    let wiki = "Error:\n{code:json}{\n  \"a\": \"*b*_c\"\n}{code}\nafter\n{noformat}\nplain ```\n{noformat}";
    assert_eq!(
        described(wiki, "[]"),
        "Error:\n\n```json\n{\n  \"a\": \"*b*_c\"\n}\n```\n\nafter\n\n````\nplain ```\n````"
    );
}

#[test]
fn an_attached_picture_points_at_its_content_and_an_unknown_one_is_named() {
    let attachments = r#"[{"filename":"shot.png","content":"https://acme.atlassian.net/rest/api/2/attachment/content/7"}]"#;
    assert_eq!(
        described(
            "!shot.png|width=703,alt=\"shot.png\"!\n\n!gone.png!\n\nHi!there!",
            attachments
        ),
        "![shot.png](<https://acme.atlassian.net/rest/api/2/attachment/content/7>)\n\n*gone.png*\n\nHi\\!there\\!"
    );
}

#[test]
fn wiki_lines_break_tables_quote_and_markdown_marks_stay_text() {
    let wiki = "one\ntwo\n1. not a list\n\n||Name||Link||\n|a_b|[x|https://x.test]|\n\n{quote}said *this*{quote}\nbq. short\n----\n<b>#3</b> `tick`";
    assert_eq!(
        described(wiki, "[]"),
        "one  \ntwo  \n1\\. not a list\n\n| Name | Link |\n| --- | --- |\n| a\\_b | [x](<https://x.test>) |\n\n> said **this**\n\n> short\n***\n\n\\<b\\>\\#3\\</b\\> \\`tick\\`"
    );
}

#[test]
fn an_email_and_token_log_in_as_basic_and_a_lone_token_as_a_bearer() {
    assert_eq!(
        wire::authorization("ann@acme.com:abc\"def"),
        "Authorization: Basic YW5uQGFjbWUuY29tOmFiYyJkZWY="
    );
    assert_eq!(wire::authorization("a:"), "Authorization: Basic YTo=");
    assert_eq!(wire::authorization("ab:"), "Authorization: Basic YWI6");
    let basic = wire::config("acme.atlassian.net", "PROJ-1", "ann@acme.com:abc\"def");
    assert!(basic.contains("url = \"https://acme.atlassian.net/rest/api/2/issue/PROJ-1?fields="));
    assert!(basic.contains("header = \"Authorization: Basic YW5uQGFjbWUuY29tOmFiYyJkZWY=\"\n"));
    let bearer = wire::config("jira.example.org", "PROJ-1", "pat123");
    assert!(bearer.contains("header = \"Authorization: Bearer pat123\"\n"));
}

#[test]
fn a_picture_on_the_site_is_fetched_with_the_login() {
    let on_site =
        WebLink::new("https://ACME.atlassian.net/rest/api/2/attachment/content/7").unwrap();
    let elsewhere = WebLink::new("https://acme.atlassian.net.evil.test/x.png").unwrap();
    assert!(site().serves(&on_site));
    assert!(!site().serves(&elsewhere));
}

#[test]
fn the_status_is_read_from_the_last_line_curl_writes() {
    let response = wire::parse_response("{\"a\":\n1}\n404").unwrap();
    assert_eq!(response.status, HttpStatus::new(404));
    assert_eq!(response.body, "{\"a\":\n1}");
    assert!(wire::parse_response("no status").is_none());
}

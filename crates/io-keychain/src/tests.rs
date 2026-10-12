use std::path::Path;

use domain::{ClientId, Grant, RefreshToken, Renewal, Token, UnixTime};

use io_process::Argument;

use crate::convert;
use crate::wire::{self, Tool};

const TOOLS: [Tool; 2] = [Tool::Security, Tool::SecretTool];

fn words(arguments: &[Argument]) -> Vec<String> {
    arguments
        .iter()
        .map(|argument| argument.as_str().to_owned())
        .collect()
}

fn renewable() -> Grant {
    Grant {
        token: Token::new("de6780bc").unwrap(),
        renewal: Some(Renewal {
            refresh: RefreshToken::new("8257e65c").unwrap(),
            expires: UnixTime::new(1_790_000_000),
            client: ClientId::new("2f1e0d9c").unwrap(),
        }),
    }
}

#[test]
fn the_secret_reaches_the_keychain_program_only_through_stdin() {
    let secret = "glsecret glrefreshsecret 1790000000 2f1e0d9c";
    for tool in TOOLS {
        for keychain in [None, Some(Path::new("/tmp/scratch.keychain"))] {
            let store = wire::store(tool, "github.com", secret, keychain);
            assert!(
                store.input.text().contains(secret),
                "{tool:?}: {}",
                store.input.text()
            );
            for arguments in [
                words(&store.arguments),
                words(&wire::find_arguments(tool, "github.com", keychain)),
                words(&wire::delete_arguments(tool, "github.com", keychain)),
            ] {
                assert!(
                    arguments
                        .iter()
                        .all(|argument| !argument.contains("secret")),
                    "{tool:?}: {arguments:?}"
                );
            }
        }
    }
}

#[test]
fn security_reads_one_quoted_add_command_on_stdin_with_the_keychain_last() {
    let store = wire::store(
        Tool::Security,
        "github.com",
        "tok",
        Some(Path::new("/tmp/a \"b\"/k.keychain")),
    );
    assert_eq!(words(&store.arguments), ["-i"]);
    assert_eq!(
        store.input.text(),
        "add-generic-password -U -s \"code-review\" -a \"github.com\" -l \"code-review github.com\" -w \"tok\" \"/tmp/a \\\"b\\\"/k.keychain\"\n"
    );
}

#[test]
fn secret_tool_stores_under_service_and_account_and_the_scratch_keychain_as_an_attribute() {
    let store = wire::store(
        Tool::SecretTool,
        "gitlab.com",
        "tok",
        Some(Path::new("scratch")),
    );
    assert_eq!(
        words(&store.arguments),
        [
            "store",
            "--label=code-review gitlab.com",
            "service",
            "code-review",
            "account",
            "gitlab.com",
            "keychain",
            "scratch",
        ]
    );
    assert_eq!(store.input.text(), "tok");
    assert_eq!(
        words(&wire::find_arguments(Tool::SecretTool, "gitlab.com", None)),
        ["lookup", "service", "code-review", "account", "gitlab.com"]
    );
}

#[test]
fn security_finds_and_deletes_by_service_and_account_in_the_named_keychain() {
    assert_eq!(
        words(&wire::find_arguments(
            Tool::Security,
            "github.com",
            Some(Path::new("/k"))
        )),
        [
            "find-generic-password",
            "-s",
            "code-review",
            "-a",
            "github.com",
            "-w",
            "/k"
        ]
    );
    assert_eq!(
        words(&wire::delete_arguments(Tool::Security, "github.com", None)),
        [
            "delete-generic-password",
            "-s",
            "code-review",
            "-a",
            "github.com"
        ]
    );
}

#[test]
fn a_grant_is_stored_as_its_words_and_read_back_whole() {
    let text = wire::print_secret(&convert::wire(&renewable()));
    assert_eq!(text, "de6780bc 8257e65c 1790000000 2f1e0d9c");
    let read = wire::parse_secret(&format!("{text}\n")).and_then(|secret| convert::grant(&secret));
    assert_eq!(read, Some(renewable()));
    let lasting = Grant::lasting(Token::new("glpat-abc").unwrap());
    let lasting_text = wire::print_secret(&convert::wire(&lasting));
    assert_eq!(lasting_text, "glpat-abc");
    assert_eq!(
        wire::parse_secret(&lasting_text).and_then(|secret| convert::grant(&secret)),
        Some(lasting)
    );
}

#[test]
fn a_stored_secret_of_another_shape_is_not_read_as_a_grant() {
    assert_eq!(wire::parse_secret("two words"), None);
    assert_eq!(wire::parse_secret("a b notanumber c"), None);
    assert_eq!(wire::parse_secret(""), None);
}

#[test]
fn each_program_says_absent_with_its_own_exit_code() {
    assert!(Tool::Security.absent(Some(44)));
    assert!(!Tool::Security.absent(Some(1)));
    assert!(Tool::SecretTool.absent(Some(1)));
    assert!(!Tool::SecretTool.absent(Some(44)));
    assert!(!Tool::Security.absent(None));
}

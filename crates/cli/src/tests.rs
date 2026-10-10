use crate::wire::words;
use crate::{Argument, Invocation};

fn parse(line: &str) -> Result<Invocation, crate::ParseFailure> {
    let arguments: Vec<Argument> = words(line).iter().map(|word| Argument::new(word)).collect();
    Invocation::parse(&arguments)
}

#[test]
fn a_pull_request_takes_a_repository_and_a_forge_prefix() {
    assert!(parse("changes #12").is_ok());
    assert!(parse("changes hexyl!12").is_ok());
    assert!(parse("changes sharkdp/hexyl#12").is_ok());
    assert!(parse("changes twelve").is_err());
}

#[test]
fn quoted_words_keep_their_spaces() {
    assert_eq!(
        words("comment 3 src/a.rs 4 \"why not a map?\""),
        vec!["comment", "3", "src/a.rs", "4", "why not a map?"]
    );
}

#[test]
fn approving_and_requesting_changes_cannot_be_combined() {
    assert!(parse("submit 3 --approve --request-changes").is_err());
    assert!(parse("submit 3 --approve").is_ok());
}

#[test]
fn line_zero_is_refused_because_lines_are_one_based() {
    assert!(parse("comment 3 a.rs 0 hi").is_err());
    assert!(parse("comment 3 a.rs 1 hi").is_ok());
}

#[test]
fn seen_takes_files_or_all_but_not_both() {
    assert!(parse("seen 3").is_err());
    assert!(parse("seen 3 --all").is_ok());
    assert!(parse("seen 3 a.rs --all").is_err());
}

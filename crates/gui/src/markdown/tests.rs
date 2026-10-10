use super::*;

fn blocks(text: &str) -> Vec<Block> {
    parse(&Prose::new(text))
}

fn texts(pieces: &[Piece]) -> Vec<(&str, Tone, Option<&Goal>)> {
    pieces
        .iter()
        .map(|piece| (piece.text.as_str(), piece.tone, piece.goal.as_ref()))
        .collect()
}

#[test]
fn emphasis_code_and_links_become_toned_pieces() {
    let parsed = blocks("A **bold** `call()` and [docs](https://h.org/d).");
    let [Block::Text { lead, pieces }] = parsed.as_slice() else {
        panic!("{parsed:?}");
    };
    assert_eq!(lead, &Lead::Paragraph);
    let docs = Goal::Web(WebLink::new("https://h.org/d").unwrap());
    assert_eq!(
        texts(pieces),
        vec![
            ("A ", Tone::Plain, None),
            ("bold", Tone::Strong, None),
            (" ", Tone::Plain, None),
            ("call()", Tone::Code, None),
            (" and ", Tone::Plain, None),
            ("docs", Tone::Plain, Some(&docs)),
            (".", Tone::Plain, None),
        ]
    );
}

#[test]
fn references_to_pulls_people_commits_and_urls_link_like_on_the_forge() {
    let parsed = blocks("Fixes #12 with @ann, see 1a2b3c4d5e and https://h.org/x.");
    let [Block::Text { pieces, .. }] = parsed.as_slice() else {
        panic!("{parsed:?}");
    };
    let goals: Vec<(&str, &Goal)> = pieces
        .iter()
        .filter_map(|piece| piece.goal.as_ref().map(|goal| (piece.text.as_str(), goal)))
        .collect();
    assert_eq!(
        goals,
        vec![
            (
                "#12",
                &Goal::Forge(Linked::Issue(PrNumber::new(12).unwrap()))
            ),
            ("@ann", &Goal::Forge(Linked::User(Login::new("ann")))),
            (
                "1a2b3c4d5e",
                &Goal::Forge(Linked::Commit(CommitId::new("1a2b3c4d5e").unwrap()))
            ),
            (
                "https://h.org/x",
                &Goal::Web(WebLink::new("https://h.org/x").unwrap())
            ),
        ]
    );
}

#[test]
fn a_word_of_digits_or_a_bare_hash_is_not_a_commit() {
    let parsed = blocks("12345678 and deadbeef stay text, a#1 too");
    let [Block::Text { pieces, .. }] = parsed.as_slice() else {
        panic!("{parsed:?}");
    };
    assert!(
        pieces.iter().all(|piece| piece.goal.is_none()),
        "{pieces:?}"
    );
}

#[test]
fn list_items_carry_their_marker_and_depth() {
    let parsed = blocks("1. one\n2. two\n   - inner\n");
    let leads: Vec<&Lead> = parsed
        .iter()
        .filter_map(|block| match block {
            Block::Text { lead, .. } => Some(lead),
            _ => None,
        })
        .collect();
    assert_eq!(
        leads,
        vec![
            &Lead::Item {
                depth: Count::ZERO,
                marker: Label::new("1.")
            },
            &Lead::Item {
                depth: Count::ZERO,
                marker: Label::new("2.")
            },
            &Lead::Item {
                depth: Count::new(1),
                marker: Label::new("\u{2022}")
            },
        ]
    );
}

#[test]
fn pictures_come_from_markdown_and_from_img_tags() {
    let parsed = blocks(
        "![shot](https://h.org/a.png)\n\n<img width=\"300\" alt=\"after\" src=\"https://h.org/b.png\">",
    );
    let pictures: Vec<(&str, &str)> = parsed
        .iter()
        .filter_map(|block| match block {
            Block::Picture { link, alt } => Some((link.as_str(), alt.as_str())),
            _ => None,
        })
        .collect();
    assert_eq!(
        pictures,
        vec![
            ("https://h.org/a.png", "shot"),
            ("https://h.org/b.png", "after")
        ]
    );
}

#[test]
fn a_fenced_block_keeps_its_language_and_text() {
    let parsed = blocks("```rust\nfn main() {}\n```\n");
    assert_eq!(
        parsed,
        vec![Block::Code {
            language: Label::new("rust"),
            text: Label::new("fn main() {}"),
        }]
    );
}

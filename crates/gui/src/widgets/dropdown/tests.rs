use domain::PullStatus;
use strum::VariantArray;

use super::*;

fn text(runs: &[Run]) -> String {
    runs.iter().map(|run| run.text.as_str()).collect()
}

fn hiding(hidden: &[PullStatus]) -> Checks<PullStatus> {
    let mut checks = Checks::default();
    for status in hidden {
        checks.toggle(*status);
    }
    checks
}

#[test]
fn the_trigger_says_all_until_a_value_is_hidden_then_names_the_shown_ones() {
    let room = Cells::new(20);
    assert_eq!(text(&trigger_runs(&hiding(&[]), room)), "all");
    assert_eq!(
        text(&trigger_runs(
            &hiding(&[PullStatus::Reviewing, PullStatus::Reviewed]),
            room
        )),
        "new, updated"
    );
    assert_eq!(
        text(&trigger_runs(&hiding(PullStatus::VARIANTS), room)),
        "none"
    );
}

#[test]
fn the_trigger_counts_the_shown_values_when_their_words_do_not_fit() {
    let checks = hiding(&[PullStatus::Reviewed]);
    assert_eq!(
        text(&trigger_runs(&checks, Cells::new(26))),
        "new, updated, reviewing"
    );
    assert_eq!(text(&trigger_runs(&checks, Cells::new(22))), "3 of 4");
}

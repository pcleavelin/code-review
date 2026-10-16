use domain::PullStatus;
use strum::VariantArray;

use super::*;

#[test]
fn every_value_is_checked_until_toggled_and_a_second_toggle_checks_it_again() {
    let mut checks = Checks::<PullStatus>::default();
    assert!(checks.every());
    checks.toggle(PullStatus::Updated);
    assert!(!checks.shows(PullStatus::Updated));
    assert!(checks.shows(PullStatus::New));
    assert_eq!(
        checks.shown(),
        [PullStatus::New, PullStatus::Reviewing, PullStatus::Reviewed]
    );
    checks.toggle(PullStatus::Updated);
    assert!(checks.every());
    assert_eq!(checks.shown(), PullStatus::VARIANTS);
}

#[test]
fn a_toggle_opens_a_closed_dropdown_and_closes_an_open_one() {
    let toggle = DropdownAct::Toggle(Dropdown::InboxStatus);
    assert_eq!(toggle.open_after(None), Some(Dropdown::InboxStatus));
    assert_eq!(toggle.open_after(Some(Dropdown::InboxStatus)), None);
    assert_eq!(
        DropdownAct::Close.open_after(Some(Dropdown::InboxStatus)),
        None
    );
}

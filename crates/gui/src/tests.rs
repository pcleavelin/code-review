use std::collections::BTreeSet;

use crate::ids::CONTROLS;

#[test]
fn no_two_controls_share_an_element_name() {
    let mut names = BTreeSet::new();
    for control in CONTROLS {
        assert!(
            names.insert(control.element().as_str()),
            "{} names two controls",
            control.element().as_str()
        );
    }
}

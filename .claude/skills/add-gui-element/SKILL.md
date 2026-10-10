---
name: add-gui-element
description: Add or change a view, panel, button, row, field or gesture in the review GUI (crates/gui). Use as a step of add-feature, or for a change to how the GUI looks.
---

# Add a GUI element

1. `crates/gui/src/ids.rs`: the element's control, `Control::new(Element::new(..),
   Words::new(..))` with `.keyed(keys::X)` when a chord also triggers it, named as scripts
   will address it (`name`, `name/<n>` for rows, `name@<key>`), and listed in `CONTROLS`.
   The words are its tooltip. A chord is a const in `keys.rs`; a palette entry a row of
   `OFFERED` in `palette.rs`.
2. `crates/gui/src/theme.rs`: any new colour, size, padding or gap as a named constant.
3. `crates/gui/src/widgets.rs`: build it from an existing helper; if none fits, add the helper
   there.
4. What it does: an `Action` variant and its arm in `apply`, or a `ReviewAct` handled in
   `review.rs` for anything about a review; selection changes go through `nav.rs`; background
   work (the forge, git, indexing) through a `Job` from `runtime.rs`, never on the frame;
   key chords through `keys.rs`.
5. What a script sees: extend the component's `Dump` so its state shows in `dump`.
6. Drive it: a `REVIEW_SCRIPT` run (CLAUDE.md, "Testing the GUI") that uses it, aimed by id,
   with `dump` after it.
7. Look at it: `shot` in that script, and read the PNG. Never claim a visual result without
   the screenshot. The owner reads code in this window all day, so legibility beats theme: the
   one readable monospace font, never a pixel font, and restrained decoration. A glyph the
   font lacks draws as a box: use an `Icon` (Codicons) instead.
8. `finish`.

---
name: add-gui-element
description: Add or change a view, panel, button, row, field or gesture in the review GUI (crates/gui). Use as a step of add-feature, or for a change to how the GUI looks.
---

# Add a GUI element

Two rules hold for every element, and both are checked:

- **Nothing moves out from under the pointer.** A button once clicked keeps its place and its
  size until the pointer leaves it or the wheel turns, whatever its click changes; what has
  to move moves after. Resting the pointer on a button holds nothing, so the wheel still
  scrolls under it. Rapid clicks on one spot hit the same button every time, and a button that
  vanishes under the pointer does not hand the next click to whatever slid into its place.
  `crates/ui` does this for every element laid out `.steady()` (the button widgets in
  `widgets.rs` are): it keeps the element's size, slides its row, and scrolls its scroll
  column to hold it in place. So a clickable element is built from a steady widget, and its
  id stays the same across the change its click makes (a fold button is keyed by the edge
  that does not move, not by a line its click reveals). A virtualised view lays out a screen
  more above and below while `ui.holding()`, so the held element still exists to be held.
  Checked by the `ui` tests and by a script that clicks one coordinate repeatedly (step 6).
- **What an action does appears where the user is looking.** A control whose result is drawn
  in another view (a reply button in the Review tab opens an editor in the Source view)
  switches to that view and scrolls the result into sight, in the one function every caller
  of the action goes through (`Model::show_editing` for drafts). The app reports
  `script: the <field> field has the keyboard but is not on screen` when a focused field
  stays off screen, which fails the run. Drive every control from each place it appears, not
  only from the view where it was written.

1. `crates/gui/src/ids.rs`: the element's control, `Control::new(Element::new(..),
   Words::new(..))` with `.keyed(keys::X)` when a chord also triggers it, named as scripts
   will address it (`name`, `name/<n>` for rows, `name@<key>`), and listed in `CONTROLS`.
   The words are its tooltip. A chord is a const in `keys.rs`; a palette entry a row of
   `OFFERED` in `palette.rs`.
2. `crates/gui/src/theme.rs`: any new colour, size, padding or gap as a named constant.
3. `crates/gui/src/widgets.rs`: build it from an existing helper; if none fits, add the helper
   there, `.steady()` when it is clicked.
4. What it does: an `Action` variant and its arm in `apply`, or a `ReviewAct` handled in
   `review.rs` for anything about a review; selection changes go through `nav.rs`; background
   work (the forge, git, indexing) through a `Job` from `runtime.rs`, never on the frame;
   key chords through `keys.rs`. If the result shows in another view, the action shows it
   (rule two above).
5. What a script sees: extend the component's `Dump` so its state shows in `dump`.
6. Drive it: a `REVIEW_SCRIPT` run (CLAUDE.md, "Testing the GUI") that uses it, aimed by id,
   with `dump` after it, from every view the control appears in. For a button whose click
   changes the layout, `mouse` onto it and `click x y` the same coordinate twice: the second
   click must land on the same button (`rect <id>` before and after shows it held).
7. Look at it: `shot` in that script, and read the PNG. Never claim a visual result without
   the screenshot. The owner reads code in this window all day, so legibility beats theme: the
   one readable monospace font, never a pixel font, and restrained decoration. A glyph the
   font lacks draws as a box: use an `Icon` (Codicons) instead.
8. `finish`.

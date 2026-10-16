use ui::{Button, Coordinate, Glyph, Input, Interaction, Key, Label, Mods, Pointer, Press, Vector};

use crate::field::{Edit, EnterMods, Motion, Unit, Which};
use crate::review::ReviewAct;
use crate::viewer::{Step, ViewerAct};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Chord {
    key: Key,
    mods: Mods,
}

impl Chord {
    const fn primary(key: Key) -> Self {
        Self {
            key,
            mods: Mods::PRIMARY,
        }
    }

    const fn alt(key: Key) -> Self {
        Self {
            key,
            mods: Mods::ALT,
        }
    }

    fn pressed(self, input: &Input) -> bool {
        input.pressed_key(self.key, self.mods)
    }

    pub(crate) fn label(self) -> Label {
        let modifier = [
            (self.mods.command(), COMMAND_NAME),
            (self.mods.ctrl(), CTRL_NAME),
            (self.mods.alt(), ALT_NAME),
        ]
        .into_iter()
        .find(|(held, _)| *held)
        .map_or_else(String::new, |(_, name)| format!("{}+", name.as_str()));
        let key = match self.key {
            Key::Left => "left".to_owned(),
            Key::Right => "right".to_owned(),
            Key::Up => "up".to_owned(),
            Key::Down => "down".to_owned(),
            Key::Character(glyph) => glyph.get().to_string(),
            Key::Enter => "enter".to_owned(),
            Key::Escape => "escape".to_owned(),
            Key::Backspace => "backspace".to_owned(),
            Key::Remove => "delete".to_owned(),
            Key::Home => "home".to_owned(),
            Key::End => "end".to_owned(),
        };
        Label::new(format!("{modifier}{key}"))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct KeyName(&'static str);

impl KeyName {
    pub(crate) const fn as_str(self) -> &'static str {
        self.0
    }
}

const COMMAND_NAME: KeyName = KeyName("cmd");
const CTRL_NAME: KeyName = KeyName("ctrl");
const ALT_NAME: KeyName = KeyName("alt");
pub(crate) const PRIMARY_NAME: KeyName = if Mods::PRIMARY.command() {
    COMMAND_NAME
} else {
    CTRL_NAME
};

pub(crate) const GO_BACK: Chord = Chord::alt(Key::Left);
pub(crate) const GO_FORWARD: Chord = Chord::alt(Key::Right);
const GO_BACK_TOO: Chord = Chord::primary(Key::Left);
const GO_FORWARD_TOO: Chord = Chord::primary(Key::Right);
pub(crate) const PALETTE: Chord = Chord::primary(Key::Character(Glyph::new('p')));
pub(crate) const SETTINGS: Chord = Chord::primary(Key::Character(Glyph::new(',')));
pub(crate) const TOGGLE_SEEN: Chord = Chord::alt(Key::Character(Glyph::new('s')));
pub(crate) const NEXT_UNSEEN: Chord = Chord::alt(Key::Character(Glyph::new('n')));
pub(crate) const NEXT_HUNK: Chord = Chord::alt(Key::Down);
pub(crate) const PREVIOUS_HUNK: Chord = Chord::alt(Key::Up);
pub(crate) const COPY_LINES: Chord = Chord::primary(Key::Character(Glyph::new('c')));
pub(crate) const COMMENT: Chord = Chord::alt(Key::Character(Glyph::new('c')));

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Going {
    Back,
    Forward,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Walk {
    Down,
    Up,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CodeGesture {
    Peek,
    Jump,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LineGesture {
    Press,
    Extend,
    Drag,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Extend {
    Replace,
    Extend,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Wheeling {
    Across,
    Along,
}

pub(crate) fn review_keys(input: &Input, typing: Option<Which>) -> Vec<ReviewAct> {
    [
        (TOGGLE_SEEN, ReviewAct::ToggleShownSeen),
        (NEXT_UNSEEN, ReviewAct::NextUnseen),
        (NEXT_HUNK, ReviewAct::NextHunk),
        (PREVIOUS_HUNK, ReviewAct::PreviousHunk),
        (COMMENT, ReviewAct::StartDraftHere),
        (COPY_LINES, ReviewAct::CopyLines),
    ]
    .into_iter()
    .filter(|(chord, act)| {
        chord.pressed(input) && (typing.is_none() || *act != ReviewAct::CopyLines)
    })
    .map(|(_, act)| act)
    .collect()
}

pub(crate) fn going(input: &Input, going: Going, focused: Option<Which>) -> bool {
    let (chords, button) = match going {
        Going::Back => ([GO_BACK, GO_BACK_TOO], Button::Back),
        Going::Forward => ([GO_FORWARD, GO_FORWARD_TOO], Button::Forward),
    };
    (focused.is_none() && chords.iter().any(|chord| chord.pressed(input)))
        || input.pointer.pressed.contains(button)
}

pub(crate) const fn enter_mods(mods: Mods) -> EnterMods {
    if mods.primary() {
        EnterMods::Primary
    } else if mods.shift() {
        EnterMods::Shift
    } else {
        EnterMods::Plain
    }
}

fn clipboard_edit(press: Press) -> Option<Edit> {
    if !press.mods.same_chord(Mods::PRIMARY) {
        return None;
    }
    match press.key {
        Key::Character(glyph) => match glyph.get() {
            'c' => Some(Edit::Copy),
            'x' => Some(Edit::Cut),
            'v' => Some(Edit::Paste),
            _ => None,
        },
        _ => None,
    }
}

fn edit_of(press: Press) -> Option<Edit> {
    let mods = press.mods;
    let unit = if mods.ctrl() || mods.alt() {
        Unit::Word
    } else {
        Unit::Character
    };
    let extend = if mods.shift() {
        Extend::Extend
    } else {
        Extend::Replace
    };
    Some(match press.key {
        Key::Enter => Edit::Enter(enter_mods(mods)),
        Key::Backspace => Edit::Backspace(unit),
        Key::Remove => Edit::Remove(unit),
        Key::Left if mods.command() => Edit::Move(Motion::RowStart, extend),
        Key::Right if mods.command() => Edit::Move(Motion::RowEnd, extend),
        Key::Left => Edit::Move(Motion::Left(unit), extend),
        Key::Right => Edit::Move(Motion::Right(unit), extend),
        Key::Home if mods.primary() => Edit::Move(Motion::Start, extend),
        Key::Home => Edit::Move(Motion::RowStart, extend),
        Key::End if mods.primary() => Edit::Move(Motion::End, extend),
        Key::End => Edit::Move(Motion::RowEnd, extend),
        Key::Up if mods.command() => Edit::Move(Motion::Start, extend),
        Key::Up => Edit::Move(Motion::Up, extend),
        Key::Down if mods.command() => Edit::Move(Motion::End, extend),
        Key::Down => Edit::Move(Motion::Down, extend),
        Key::Escape => Edit::Escape,
        Key::Character(glyph) if glyph.get() == 'a' && mods.primary() => Edit::SelectAll,
        Key::Character(glyph) if glyph.get() == 'u' && mods.primary() => Edit::ClearLine,
        Key::Character(_) => return clipboard_edit(press),
    })
}

pub(crate) fn edits(input: &Input) -> Vec<Edit> {
    input
        .keys
        .iter()
        .filter_map(|press| edit_of(*press))
        .collect()
}

pub(crate) fn code_gesture(interaction: Interaction, pointer: Pointer) -> Option<CodeGesture> {
    let mods = pointer.mods;
    if interaction.clicked() && mods.alt() {
        Some(CodeGesture::Peek)
    } else if (interaction.clicked() && mods.primary()) || interaction.double_clicked() {
        Some(CodeGesture::Jump)
    } else {
        None
    }
}

pub(crate) fn selects_line(interaction: Interaction, pointer: Pointer) -> Option<LineGesture> {
    let mods = pointer.mods;
    if interaction.drag().is_some() {
        Some(LineGesture::Drag)
    } else if interaction.clicked() && !mods.chorded() && !interaction.double_clicked() {
        Some(if mods.shift() {
            LineGesture::Extend
        } else {
            LineGesture::Press
        })
    } else {
        None
    }
}

pub(crate) const fn wheeling(pointer: Pointer) -> Wheeling {
    if pointer.mods.shift() {
        Wheeling::Across
    } else {
        Wheeling::Along
    }
}

pub(crate) fn turn_wheel(pointer: &mut Pointer) {
    if wheeling(*pointer) == Wheeling::Across {
        let wheel = pointer.wheel;
        pointer.wheel = Vector::new(
            Coordinate::new(wheel.horizontal.get() + wheel.vertical.get()),
            Coordinate::ZERO,
        );
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PaletteKey {
    Move(Walk),
    Run,
    Close,
}

pub(crate) fn palette_toggled(input: &Input) -> bool {
    PALETTE.pressed(input)
}

pub(crate) fn settings_toggled(input: &Input) -> bool {
    SETTINGS.pressed(input)
}

pub(crate) fn escaped(input: &Input) -> bool {
    input
        .keys
        .iter()
        .any(|press| press.key == Key::Escape && !press.mods.chorded())
}

pub(crate) fn palette_keys(input: &Input) -> Vec<PaletteKey> {
    input
        .keys
        .iter()
        .filter(|press| !press.mods.chorded())
        .filter_map(|press| match press.key {
            Key::Up => Some(PaletteKey::Move(Walk::Up)),
            Key::Down => Some(PaletteKey::Move(Walk::Down)),
            Key::Enter => Some(PaletteKey::Run),
            Key::Escape => Some(PaletteKey::Close),
            _ => None,
        })
        .collect()
}

pub(crate) fn palette_edits(input: &Input) -> Vec<Edit> {
    edits(input)
        .into_iter()
        .filter(|edit| {
            !matches!(
                edit,
                Edit::Enter(_) | Edit::Escape | Edit::Move(Motion::Up | Motion::Down, _)
            )
        })
        .collect()
}

pub(crate) fn viewer_keys(input: &Input) -> Vec<ViewerAct> {
    input
        .keys
        .iter()
        .filter(|press| !press.mods.chorded())
        .filter_map(|press| match press.key {
            Key::Escape => Some(ViewerAct::Close),
            Key::Character(glyph) => match glyph.get() {
                '+' | '=' => Some(ViewerAct::Step(Step::In, None)),
                '-' => Some(ViewerAct::Step(Step::Out, None)),
                '0' => Some(ViewerAct::Fit),
                '1' => Some(ViewerAct::Actual),
                _ => None,
            },
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pressed(chord: Chord) -> Input {
        let mut input = Input::default();
        input.keys.push(Press {
            key: chord.key,
            mods: chord.mods,
        });
        input
    }

    #[test]
    fn primary_c_copies_lines_only_while_no_field_has_the_keyboard() {
        let input = pressed(COPY_LINES);
        assert_eq!(review_keys(&input, None), vec![ReviewAct::CopyLines]);
        assert_eq!(review_keys(&input, Some(Which::Command)), Vec::new());
        assert_eq!(
            review_keys(&pressed(COMMENT), Some(Which::Command)),
            vec![ReviewAct::StartDraftHere]
        );
    }

    fn typed(glyph: char, mods: Mods) -> Input {
        let mut input = Input::default();
        input.keys.push(Press {
            key: Key::Character(Glyph::new(glyph)),
            mods,
        });
        input
    }

    #[test]
    fn the_platform_primary_modifier_copies_cuts_pastes_and_selects_all_and_the_other_does_not() {
        let other = if Mods::PRIMARY.command() {
            Mods::CTRL
        } else {
            Mods::COMMAND
        };
        for (glyph, edit) in [
            ('c', Edit::Copy),
            ('x', Edit::Cut),
            ('v', Edit::Paste),
            ('a', Edit::SelectAll),
        ] {
            assert_eq!(edits(&typed(glyph, Mods::PRIMARY)), vec![edit]);
            assert_eq!(edits(&typed(glyph, other)), Vec::new());
        }
        assert_eq!(
            edits(&typed('v', Mods::PRIMARY.with(Mods::ALT))),
            Vec::new()
        );
    }

    #[test]
    fn command_arrows_move_to_the_ends_of_the_row_and_of_the_text() {
        let moved = |key| {
            let mut input = Input::default();
            input.keys.push(Press {
                key,
                mods: Mods::COMMAND.with(Mods::SHIFT),
            });
            edits(&input)
        };
        assert_eq!(
            moved(Key::Left),
            vec![Edit::Move(Motion::RowStart, Extend::Extend)]
        );
        assert_eq!(
            moved(Key::Right),
            vec![Edit::Move(Motion::RowEnd, Extend::Extend)]
        );
        assert_eq!(
            moved(Key::Up),
            vec![Edit::Move(Motion::Start, Extend::Extend)]
        );
        assert_eq!(
            moved(Key::Down),
            vec![Edit::Move(Motion::End, Extend::Extend)]
        );
    }
}

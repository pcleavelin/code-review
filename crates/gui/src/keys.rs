use ui::{Button, Coordinate, Glyph, Input, Interaction, Key, Label, Mods, Pointer, Press, Vector};

use crate::field::{Edit, EnterMods, Motion, Unit, Which};
use crate::review::ReviewAct;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Chord {
    key: Key,
    mods: Mods,
}

impl Chord {
    const fn ctrl(key: Key) -> Self {
        Self {
            key,
            mods: Mods::CTRL,
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
        let modifier = if self.mods.ctrl() {
            "ctrl+"
        } else if self.mods.alt() {
            "alt+"
        } else {
            ""
        };
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

pub(crate) const GO_BACK: Chord = Chord::alt(Key::Left);
pub(crate) const GO_FORWARD: Chord = Chord::alt(Key::Right);
const GO_BACK_TOO: Chord = Chord::ctrl(Key::Left);
const GO_FORWARD_TOO: Chord = Chord::ctrl(Key::Right);
pub(crate) const PALETTE: Chord = Chord::ctrl(Key::Character(Glyph::new('p')));
pub(crate) const SETTINGS: Chord = Chord::ctrl(Key::Character(Glyph::new(',')));
pub(crate) const TOGGLE_SEEN: Chord = Chord::alt(Key::Character(Glyph::new('s')));
pub(crate) const NEXT_UNSEEN: Chord = Chord::alt(Key::Character(Glyph::new('n')));
pub(crate) const NEXT_HUNK: Chord = Chord::alt(Key::Down);
pub(crate) const PREVIOUS_HUNK: Chord = Chord::alt(Key::Up);
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

pub(crate) fn review_keys(input: &Input) -> Vec<ReviewAct> {
    [
        (TOGGLE_SEEN, ReviewAct::ToggleShownSeen),
        (NEXT_UNSEEN, ReviewAct::NextUnseen),
        (NEXT_HUNK, ReviewAct::NextHunk),
        (PREVIOUS_HUNK, ReviewAct::PreviousHunk),
        (COMMENT, ReviewAct::StartDraftHere),
    ]
    .into_iter()
    .filter(|(chord, _)| chord.pressed(input))
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
    if mods.ctrl() {
        EnterMods::Control
    } else if mods.shift() {
        EnterMods::Shift
    } else {
        EnterMods::Plain
    }
}

fn clipboard_edit(press: Press) -> Option<Edit> {
    if !press.mods.same_ctrl_alt(Mods::CTRL) {
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
        Key::Left => Edit::Move(Motion::Left(unit), extend),
        Key::Right => Edit::Move(Motion::Right(unit), extend),
        Key::Home if mods.ctrl() => Edit::Move(Motion::Start, extend),
        Key::Home => Edit::Move(Motion::RowStart, extend),
        Key::End if mods.ctrl() => Edit::Move(Motion::End, extend),
        Key::End => Edit::Move(Motion::RowEnd, extend),
        Key::Up => Edit::Move(Motion::Up, extend),
        Key::Down => Edit::Move(Motion::Down, extend),
        Key::Escape => Edit::Escape,
        Key::Character(glyph) if glyph.get() == 'a' && mods.ctrl() => Edit::SelectAll,
        Key::Character(glyph) if glyph.get() == 'u' && mods.ctrl() => Edit::ClearLine,
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
    } else if (interaction.clicked() && mods.ctrl()) || interaction.double_clicked() {
        Some(CodeGesture::Jump)
    } else {
        None
    }
}

pub(crate) fn selects_line(interaction: Interaction, pointer: Pointer) -> Option<LineGesture> {
    let mods = pointer.mods;
    if interaction.drag().is_some() {
        Some(LineGesture::Drag)
    } else if interaction.clicked() && !mods.ctrl() && !mods.alt() && !interaction.double_clicked()
    {
        Some(LineGesture::Press)
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
        .any(|press| press.key == Key::Escape && !press.mods.ctrl() && !press.mods.alt())
}

pub(crate) fn palette_keys(input: &Input) -> Vec<PaletteKey> {
    input
        .keys
        .iter()
        .filter(|press| !press.mods.ctrl() && !press.mods.alt())
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

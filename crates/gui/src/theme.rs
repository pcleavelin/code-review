use std::ops::{Add, AddAssign, Div, Mul, Sub};

use domain::{BaseFontSize, HighlightClass, Theme};
use ui::{Color, Count, FontSize, Px, Repaint, Scale};

use crate::panels::Ratio;

pub(crate) const BACKGROUND: Color = Color::rgba(24, 24, 26, 255);
pub(crate) const PANEL: Color = Color::rgba(32, 32, 35, 255);
pub(crate) const FIELD: Color = Color::rgba(18, 18, 20, 255);
pub(crate) const BORDER: Color = Color::rgba(60, 60, 66, 255);
pub(crate) const TEXT: Color = Color::rgba(220, 220, 220, 255);
pub(crate) const WEAK: Color = Color::rgba(140, 140, 148, 255);
pub(crate) const ACCENT: Color = Color::rgba(90, 150, 240, 255);
pub(crate) const GREEN: Color = Color::rgba(90, 200, 120, 255);
pub(crate) const RED: Color = Color::rgba(240, 110, 110, 255);
pub(crate) const ORANGE: Color = Color::rgba(220, 160, 80, 255);
pub(crate) const HOVER: Color = Color::rgba(50, 50, 56, 255);
pub(crate) const SELECTED: Color = Color::rgba(45, 65, 100, 255);
pub(crate) const DANGER_HOVER: Color = Color::rgba(110, 40, 40, 255);
pub(crate) const NOTE: Color = Color::rgba(228, 210, 178, 255);
pub(crate) const VIOLET: Color = Color::rgba(170, 130, 240, 255);

pub(crate) const DISABLED: Color = WEAK.with_alpha(90);
pub(crate) const LINES_SELECTED: Color = SELECTED.with_alpha(160);
pub(crate) const SLICE: Color = SELECTED.with_alpha(120);
pub(crate) const FAINT: Color = WEAK.with_alpha(140);
pub(crate) const PENDING: Color = WEAK.with_alpha(120);
pub(crate) const DROP_BAND: Color = ACCENT.with_alpha(70);
pub(crate) const SIBLINGS_FILL: Color = Color::rgba(30, 30, 34, 255);
pub(crate) const SIBLINGS_BORDER: Color = Color::rgba(52, 52, 58, 255);
pub(crate) const TAB_STRIP: Color = Color::rgba(22, 22, 24, 255);
pub(crate) const TREE_GUIDE: Color = Color::rgba(72, 72, 82, 255);
pub(crate) const BADGE: Color = Color::rgba(48, 48, 55, 255);
pub(crate) const COMMENT_SAVED_FILL: Color = ORANGE.with_alpha(26);
pub(crate) const ADDED_FILL: Color = GREEN.with_alpha(34);
pub(crate) const REMOVED_FILL: Color = RED.with_alpha(40);
pub(crate) const DRAFTED_BAR: Color = ORANGE;
pub(crate) const COMMENT_DRAFT_FILL: Color = ACCENT.with_alpha(26);

const KEYWORD: Color = Color::rgba(197, 134, 192, 255);
const STRING: Color = Color::rgba(206, 145, 120, 255);
const COMMENT: Color = Color::rgba(106, 153, 85, 255);
const FUNCTION: Color = Color::rgba(220, 220, 170, 255);
const TYPE: Color = Color::rgba(78, 201, 176, 255);
const CONSTANT: Color = Color::rgba(181, 206, 168, 255);
const PROPERTY: Color = Color::rgba(156, 220, 254, 255);
pub(crate) const HOVER_CODE: Color = PROPERTY;

pub(crate) const fn highlight(class: HighlightClass) -> Color {
    match class {
        HighlightClass::Keyword => KEYWORD,
        HighlightClass::String => STRING,
        HighlightClass::Comment => COMMENT,
        HighlightClass::Function => FUNCTION,
        HighlightClass::Type => TYPE,
        HighlightClass::Constant => CONSTANT,
        HighlightClass::Property => PROPERTY,
        HighlightClass::Plain => TEXT,
    }
}

const SCROLL_TRACK: Color = Color::rgba(0, 0, 0, 255);

#[derive(Clone, Copy, Debug)]
struct ThemePair {
    dark: Color,
    light: Color,
}

const fn theme_pair(dark: Color, light: Color) -> ThemePair {
    ThemePair { dark, light }
}

const LIGHT_PALETTE: [ThemePair; 26] = [
    theme_pair(VIOLET, Color::rgba(110, 60, 200, 255)),
    theme_pair(BACKGROUND, Color::rgba(250, 250, 250, 255)),
    theme_pair(PANEL, Color::rgba(243, 243, 243, 255)),
    theme_pair(FIELD, Color::rgba(255, 255, 255, 255)),
    theme_pair(BORDER, Color::rgba(200, 200, 206, 255)),
    theme_pair(TEXT, Color::rgba(32, 32, 34, 255)),
    theme_pair(WEAK, Color::rgba(108, 108, 116, 255)),
    theme_pair(ACCENT, Color::rgba(0, 95, 200, 255)),
    theme_pair(GREEN, Color::rgba(20, 130, 60, 255)),
    theme_pair(RED, Color::rgba(200, 40, 40, 255)),
    theme_pair(ORANGE, Color::rgba(175, 100, 0, 255)),
    theme_pair(HOVER, Color::rgba(226, 226, 232, 255)),
    theme_pair(SELECTED, Color::rgba(196, 218, 250, 255)),
    theme_pair(DANGER_HOVER, Color::rgba(250, 205, 205, 255)),
    theme_pair(NOTE, Color::rgba(92, 64, 28, 255)),
    theme_pair(SIBLINGS_FILL, Color::rgba(238, 238, 242, 255)),
    theme_pair(SIBLINGS_BORDER, Color::rgba(214, 214, 222, 255)),
    theme_pair(TAB_STRIP, Color::rgba(232, 232, 236, 255)),
    theme_pair(KEYWORD, Color::rgba(175, 0, 219, 255)),
    theme_pair(STRING, Color::rgba(163, 21, 21, 255)),
    theme_pair(COMMENT, Color::rgba(0, 128, 0, 255)),
    theme_pair(FUNCTION, Color::rgba(121, 94, 38, 255)),
    theme_pair(TYPE, Color::rgba(38, 127, 153, 255)),
    theme_pair(CONSTANT, Color::rgba(9, 134, 88, 255)),
    theme_pair(PROPERTY, Color::rgba(0, 16, 128, 255)),
    theme_pair(SCROLL_TRACK, Color::rgba(0, 0, 0, 255)),
];

const fn same_hue(left: Color, right: Color) -> bool {
    left.red() == right.red() && left.green() == right.green() && left.blue() == right.blue()
}

const fn inverted(color: Color) -> Color {
    Color::rgba(!color.red(), !color.green(), !color.blue(), color.alpha())
}

fn light(color: Color) -> Color {
    LIGHT_PALETTE
        .iter()
        .find(|pair| same_hue(pair.dark, color))
        .map_or_else(
            || inverted(color),
            |pair| pair.light.with_alpha(color.alpha()),
        )
}

pub(crate) const fn repaint(theme: Theme) -> Repaint {
    match theme {
        Theme::Dark => Repaint::NONE,
        Theme::Light => Repaint::new(light),
    }
}

const BASE_FONT: FontSize = FontSize::new(14);
pub(crate) const START_CELL: ui::Extent = ui::Extent::new(Px::new(8), Px::new(16));
const SMALLEST_FONT: FontSize = FontSize::new(8);

pub(crate) const fn start_font() -> FontSize {
    BASE_FONT
}

pub(crate) fn scaled_font_size(base: BaseFontSize, scale: Scale) -> FontSize {
    FontSize::scaled(u32::from(base.get()), scale, SMALLEST_FONT.get())
}

pub(crate) fn title_font(font: FontSize) -> FontSize {
    FontSize::new(font.get() + font.get() / 3)
}

pub(crate) fn heading_font(font: FontSize, level: ui::Count) -> FontSize {
    let base = font.get();
    FontSize::new(match level.get() {
        1 => base + base * 3 / 5,
        2 => base + base / 3,
        3 => base + base / 6,
        _ => base,
    })
}

pub(crate) const LABEL_PADDING: Px = Px::new(2);
pub(crate) const BUTTON_PADDING: Px = Px::new(4);
pub(crate) const TAB_PADDING: Px = Px::new(6);
pub(crate) const SMALL_BUTTON_PADDING: Px = Px::new(2);
pub(crate) const SMALL_BUTTON_EXTRA: Px = Px::new(4);
pub(crate) const NAV_BUTTON_EXTRA: Px = Px::new(8);
pub(crate) const ROW_PADDING: Px = Px::new(2);
pub(crate) const BADGE_PADDING: Px = Px::new(1);
pub(crate) const FIELD_PADDING: Px = Px::new(3);
pub(crate) const FIELD_CARET_ROOM: Px = Px::new(6);
pub(crate) const NOTE_ROWS_LEAST: ui::Count = ui::Count::new(3);
pub(crate) const NOTE_ROWS_MOST: ui::Count = ui::Count::new(12);
pub(crate) const BAR_PADDING: Px = Px::new(4);
pub(crate) const PANEL_PADDING: Px = Px::new(4);
pub(crate) const TOOLTIP_PADDING: Px = Px::new(6);
pub(crate) const COMMENT_PADDING: Px = Px::new(6);
pub(crate) const TIGHT_GAP: Px = Px::new(2);
pub(crate) const SMALL_GAP: Px = Px::new(4);
pub(crate) const GAP: Px = Px::new(6);
pub(crate) const WIDE_GAP: Px = Px::new(8);
pub(crate) const STATUS_GAP: Px = Px::new(12);
pub(crate) const STEP_SPACER: Px = Px::new(6);
pub(crate) const INDENT_EXTRA: Px = Px::new(4);
pub(crate) const PIXEL: Px = Px::new(1);
pub(crate) const SELECTED_BAR: Px = PIXEL;
pub(crate) const COLUMN_EDGE: Px = Px::new(4);
pub(crate) const ROW_EXTRA: Px = Px::new(4);
pub(crate) const PANEL_TEXT_ROOM: Px = Px::new(8);
pub(crate) const PEEK_EXTRA: Px = Px::new(8);
pub(crate) const PEEK_LEAST: Px = Px::new(100);
pub(crate) const TOOLTIP_OFFSET: Px = Px::new(16);
pub(crate) const TOOLTIP_FRAME: Px = Px::new(12);
pub(crate) const TOOLTIP_ROW_GAP: Px = Px::new(2);
pub(crate) const TOOLTIP_MARGIN: Px = Px::new(40);
pub(crate) const ELEMENT_TIP_GAP: Px = Px::new(4);
pub(crate) const SPLIT_LEAST: Px = Px::new(5);
pub(crate) const GRAB_REACH: Px = Px::new(8);
pub(crate) const SOURCE_GUESS: Px = Px::new(600);
pub(crate) const FILE_HEADER_EXTRA: Px = Px::new(14);
pub(crate) const COMMENT_FRAME: Px = Px::new(16);
pub(crate) const DRAFT_ROWS: Count = Count::new(3);
pub(crate) const DRAFT_FRAME: Px = Px::new(24);

pub(crate) const EDGE_SCROLL_BAND: Ratio = Ratio::permille(250);
pub(crate) const EDGE_SCROLL_MOST: Count = Count::new(2);

pub(crate) const NAV_BUTTON: Cells = Cells::new(3);
pub(crate) const CLOSE_BUTTON: Cells = Cells::new(3);
pub(crate) const SEARCH_FIELD: Cells = Cells::new(24);
pub(crate) const SUMMARY_FIELD: Cells = Cells::new(72);
pub(crate) const INBOX_FIELD: Cells = Cells::new(32);
pub(crate) const PIN_FIELD: Cells = Cells::new(36);
pub(crate) const TOKEN_FIELD: Cells = Cells::new(36);
pub(crate) const FORGE_STATE: Cells = Cells::new(14);
pub(crate) const THREAD_FILL: Color = ACCENT.with_alpha(18);
pub(crate) const CARD_FILL: Color = VIOLET.with_alpha(20);
pub(crate) const CARD_EDGE: Color = VIOLET.with_alpha(110);
pub(crate) const CARD_STRIPE: Px = Px::new(4);

pub(crate) const fn pill_fill(color: Color) -> Color {
    color.with_alpha(44)
}

pub(crate) const fn pill_edge(color: Color) -> Color {
    color.with_alpha(130)
}
pub(crate) const RESOLVED_FILL: Color = WEAK.with_alpha(18);
pub(crate) const THREAD_BAR: Color = ACCENT;
pub(crate) const FOLD_FILL: Color = WEAK.with_alpha(22);
pub(crate) const STRONG_TEXT: Color = Color::rgba(250, 250, 252, 255);
pub(crate) const EMPHASIS_TEXT: Color = Color::rgba(200, 190, 230, 255);
pub(crate) const CODE_TEXT: Color = Color::rgba(230, 180, 120, 255);
pub(crate) const MARKDOWN_GUESS: Count = Count::new(80);
pub(crate) const PICTURE_TALLEST: Px = Px::new(420);
pub(crate) const BLOCK_GAP: Px = Px::new(6);
pub(crate) const TREE_PLACE: Cells = Cells::new(40);
pub(crate) const FILTER_FIELD: Cells = Cells::new(16);
pub(crate) const TREE_LEVEL_WIDTH: Cells = Cells::new(2);
pub(crate) const COMMENT_FIELD_GUESS: Count = Count::new(60);
pub(crate) const COMMENT_BUTTONS_ROOM: Count = Count::new(22);
pub(crate) const COMMENT_LEAST_FIELD: Count = Count::new(16);
pub(crate) const SYMBOLS_GUESS: Cells = Cells::new(60);
pub(crate) const SYMBOLS_LEAST: Cells = Cells::new(30);
pub(crate) const SYMBOL_NAME: Cells = Cells::new(40);
pub(crate) const SYMBOL_NAME_LEAST: Cells = Cells::new(10);
pub(crate) const SYMBOL_FIXED: Cells = Cells::new(20);
pub(crate) const PEEK_TITLE: Cells = Cells::new(24);
pub(crate) const PEEK_PLACE_ROOM: Cells = Cells::new(14);
pub(crate) const TOOLTIP_LEAST: Cells = Cells::new(20);
pub(crate) const TOOLTIP_SYMBOL_WIDTH: Cells = Cells::new(70);
pub(crate) const TOOLTIP_TEXT_WIDTH: Cells = Cells::new(84);
pub(crate) const TOOLTIP_RULE: Cells = Cells::new(20);
pub(crate) const ELEMENT_TIP_WIDTH: Cells = Cells::new(56);
pub(crate) const PANEL_LEAST_ACROSS: Cells = Cells::new(12);
pub(crate) const PANEL_LEAST_DOWN: Cells = Cells::new(4);
pub(crate) const PICKER_WIDTH: Cells = Cells::new(28);
pub(crate) const PICKER_FIELD: Cells = Cells::new(24);
pub(crate) const PALETTE_FIELD: Cells = Cells::new(96);
pub(crate) const PALETTE_TAG: Cells = Cells::new(8);
pub(crate) const SETTINGS_WIDTH: Cells = Cells::new(60);
pub(crate) const SETTINGS_HEADING: Cells = Cells::new(14);
pub(crate) const SETTINGS_FONT_ROWS: Count = Count::new(10);

pub(crate) const RATIO_WHOLE: Ratio = Ratio::permille(1000);
pub(crate) const HALF: Ratio = Ratio::permille(500);
pub(crate) const EDGE_ZONE: Ratio = Ratio::permille(250);
pub(crate) const DEFAULT_LEFT: Ratio = Ratio::permille(220);
pub(crate) const DEFAULT_RIGHT: Ratio = Ratio::permille(740);
pub(crate) const DEFAULT_BOTTOM: Ratio = Ratio::permille(820);
pub(crate) const CONSOLE_ROWS: Cells = Cells::new(4);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct Cells(i32);

impl Cells {
    pub(crate) const fn new(count: i32) -> Self {
        Self(count)
    }

    pub(crate) const fn get(self) -> i32 {
        self.0
    }

    pub(crate) fn of(self, cell: Px) -> Px {
        cell * self.0
    }

    pub(crate) fn of_count(count: usize) -> Self {
        Self(i32::try_from(count).unwrap_or(0))
    }
}

impl Add for Cells {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self(self.0 + other.0)
    }
}

impl Sub for Cells {
    type Output = Self;

    fn sub(self, other: Self) -> Self {
        Self(self.0 - other.0)
    }
}

impl AddAssign for Cells {
    fn add_assign(&mut self, other: Self) {
        self.0 += other.0;
    }
}

impl Div<i32> for Cells {
    type Output = Self;

    fn div(self, divisor: i32) -> Self {
        Self(self.0 / divisor)
    }
}

impl Mul<i32> for Cells {
    type Output = Self;

    fn mul(self, factor: i32) -> Self {
        Self(self.0 * factor)
    }
}

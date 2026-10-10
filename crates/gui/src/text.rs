use std::fmt;

use std::iter;
use ui::Count;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Clipped(String);

impl Clipped {
    pub(crate) fn right(text: &str, columns: usize) -> Self {
        if text.chars().count() <= columns {
            return Self(text.to_owned());
        }
        Self(
            text.chars()
                .take(columns.saturating_sub(1))
                .chain(iter::once('\u{2026}'))
                .collect(),
        )
    }

    pub(crate) fn left(text: &str, columns: usize) -> Self {
        let count = text.chars().count();
        if count <= columns {
            return Self(text.to_owned());
        }
        Self(
            iter::once('\u{2026}')
                .chain(text.chars().skip(count + 1 - columns.max(1)))
                .collect(),
        )
    }
}

impl fmt::Display for Clipped {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Needle(String);

impl Needle {
    pub(crate) fn new(text: &str) -> Self {
        Self(text.to_owned())
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    pub(crate) fn found_in(&self, text: &str) -> bool {
        let needle = self.0.as_bytes();
        needle.is_empty()
            || text
                .as_bytes()
                .windows(needle.len())
                .any(|window| window.eq_ignore_ascii_case(needle))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Noun {
    Hit,
    Symbol,
    File,
    Draft,
    Comment,
    Thread,
    Line,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Counted {
    count: Count,
    noun: Noun,
}

impl Counted {
    pub(crate) const fn new(count: Count, noun: Noun) -> Self {
        Self { count, noun }
    }
}

impl fmt::Display for Counted {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let one = self.count.get() == 1;
        let word = match self.noun {
            Noun::Hit if one => "hit",
            Noun::Hit => "hits",
            Noun::Symbol if one => "symbol",
            Noun::Symbol => "symbols",
            Noun::File if one => "file",
            Noun::File => "files",
            Noun::Draft if one => "draft",
            Noun::Draft => "drafts",
            Noun::Comment if one => "comment",
            Noun::Comment => "comments",
            Noun::Thread if one => "thread",
            Noun::Thread => "threads",
            Noun::Line if one => "line",
            Noun::Line => "lines",
        };
        write!(formatter, "{} {word}", self.count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clipping_keeps_short_text_and_marks_cut_text() {
        assert_eq!(Clipped::right("short", 8).to_string(), "short");
        assert_eq!(
            Clipped::right("a long name", 6).to_string(),
            "a lon\u{2026}"
        );
        assert_eq!(
            Clipped::left("src/main.rs:12", 8).to_string(),
            "\u{2026}n.rs:12"
        );
        assert_eq!(Clipped::left("abc", 0).to_string(), "\u{2026}");
    }

    #[test]
    fn a_needle_matches_ignoring_ascii_case() {
        let needle = Needle::new("count");
        assert!(needle.found_in("IndexCounts"));
        assert!(!needle.found_in("Coun"));
        assert!(Needle::new("").found_in("anything"));
    }

    #[test]
    fn a_count_of_one_takes_the_singular() {
        assert_eq!(
            Counted::new(Count::new(1), Noun::Draft).to_string(),
            "1 draft"
        );
        assert_eq!(Counted::new(Count::ZERO, Noun::File).to_string(), "0 files");
        assert_eq!(Counted::new(Count::new(3), Noun::Hit).to_string(), "3 hits");
    }
}

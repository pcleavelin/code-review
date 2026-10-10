use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use domain::{
    Column, FileId, Language, Line, Location, SourceLine, SymbolId, SymbolQuery, TextHash,
};
use io_lsp::{Character, DocumentPosition, HoverReply, HoverText};
use ui::{Count, Grid, Label};

use crate::model::Model;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct Probe {
    pub(crate) position: DocumentPosition,
    pub(crate) hash: TextHash,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Intent {
    Jump,
    Peek,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Hovered {
    Asked,
    Answered(Option<HoverText>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Flight {
    Idle,
    Waiting(Probe),
}

#[derive(Clone, Debug)]
struct Wanted {
    probe: Probe,
    since: Duration,
}

#[derive(Clone, Debug)]
pub(crate) struct WantedDefinition {
    pub(crate) probe: Probe,
    pub(crate) intent: Intent,
}

#[derive(Clone, Debug)]
pub(crate) struct Queries {
    hovers: BTreeMap<Probe, Hovered>,
    want: Option<Wanted>,
    flight: Flight,
    asked: Count,
    want_definition: Option<WantedDefinition>,
    references_asked: BTreeSet<Probe>,
    closed: Option<WordSpot>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct WordSpot {
    file: FileId,
    line: Line,
    start: Column,
}

impl Default for Queries {
    fn default() -> Self {
        Self {
            hovers: BTreeMap::new(),
            want: None,
            flight: Flight::Idle,
            asked: Count::ZERO,
            want_definition: None,
            references_asked: BTreeSet::new(),
            closed: None,
        }
    }
}

pub(crate) enum HoverStep {
    Wait,
    Ask,
}

impl Queries {
    const DELAY: Duration = Duration::from_millis(150);
    const MOST_HOVERS: Count = Count::new(500);

    pub(crate) const fn asked(&self) -> Count {
        self.asked
    }

    pub(crate) fn asked_references(&self, probe: &Probe) -> bool {
        self.references_asked.contains(probe)
    }

    pub(crate) fn hover_step(&mut self, probe: &Probe, now: Duration) -> HoverStep {
        let since = match &self.want {
            Some(wanted) if wanted.probe == *probe => wanted.since,
            _ => {
                self.want = Some(Wanted {
                    probe: probe.clone(),
                    since: now,
                });
                now
            }
        };
        if now.saturating_sub(since) >= Self::DELAY && !self.in_flight(probe) {
            if self.hovers.len() > Self::MOST_HOVERS.get() {
                self.hovers.clear();
            }
            self.hovers.insert(probe.clone(), Hovered::Asked);
            HoverStep::Ask
        } else {
            HoverStep::Wait
        }
    }

    pub(crate) fn asks_now(&self, probe: &Probe, now: Duration) -> bool {
        let since = match &self.want {
            Some(wanted) if wanted.probe == *probe => wanted.since,
            _ => now,
        };
        now.saturating_sub(since) >= Self::DELAY && !self.in_flight(probe)
    }

    pub(crate) const fn waiting(&self) -> Option<&Probe> {
        match &self.flight {
            Flight::Idle => None,
            Flight::Waiting(probe) => Some(probe),
        }
    }

    fn in_flight(&self, probe: &Probe) -> bool {
        match &self.flight {
            Flight::Idle => false,
            Flight::Waiting(waiting) => waiting == probe,
        }
    }

    pub(crate) fn hover_sent(&mut self, probe: &Probe) {
        self.asked += Count::new(1);
        self.flight = Flight::Waiting(probe.clone());
    }

    pub(crate) fn definition_sent(&mut self, wanted: WantedDefinition) {
        self.asked += Count::new(1);
        self.want_definition = Some(wanted);
    }

    pub(crate) fn references_sent(&mut self, probe: Probe) {
        self.asked += Count::new(1);
        self.references_asked.insert(probe);
    }

    fn answered(&mut self) {
        self.asked = Count::new(self.asked.get().saturating_sub(1));
    }

    pub(crate) fn hover_answered(&mut self, probe: Probe, reply: HoverReply) {
        self.answered();
        if self.flight == Flight::Waiting(probe.clone()) {
            self.flight = Flight::Idle;
        }
        match reply {
            HoverReply::Given(text) => {
                self.hovers.insert(probe, Hovered::Answered(text));
            }
            HoverReply::Cancelled => {
                self.hovers.remove(&probe);
            }
        }
    }

    pub(crate) fn references_answered(&mut self) {
        self.answered();
    }

    pub(crate) fn definition_answered(&mut self, probe: &Probe) -> Option<Intent> {
        self.answered();
        let wanted = self
            .want_definition
            .take_if(|wanted| wanted.probe == *probe)?;
        Some(wanted.intent)
    }

    pub(crate) const fn reopen_hover(&mut self) {
        self.closed = None;
    }

    pub(crate) fn forget_hovers(&mut self) {
        self.hovers.clear();
    }

    pub(crate) fn server_gone(&mut self) {
        self.hovers.clear();
        self.flight = Flight::Idle;
        self.asked = Count::ZERO;
        self.want_definition = None;
    }
}

#[derive(Clone, Debug)]
pub(crate) enum Tip {
    Symbol(SymbolId),
    Text(HoverText),
}

#[derive(Clone, Debug)]
pub(crate) enum Peek {
    Symbol(SymbolId),
    Line {
        file: FileId,
        line: Line,
    },
    Outside {
        file: PathBuf,
        line: Line,
        first: Line,
        grid: Rc<Grid>,
    },
}

pub(crate) struct Word {
    pub(crate) start: Column,
    pub(crate) text: Label,
}

pub(crate) enum Probing {
    Server { language: Language, probe: Probe },
    Resolver,
    Nothing,
}

pub(crate) enum Hovering {
    Tip(Tip),
    Ask { language: Language, probe: Probe },
    Reopen,
    Nothing,
}

fn character_at(line: Option<&SourceLine>, column: Column) -> Character {
    let count: usize = line
        .map_or("", SourceLine::as_str)
        .chars()
        .take(usize::try_from(column.value()).unwrap_or(0))
        .map(char::len_utf16)
        .sum();
    Character::new(u32::try_from(count).unwrap_or(0))
}

impl Model {
    pub(crate) fn word_at(&self, file: FileId, line: Line, column: Column) -> Option<Word> {
        let text = self.index.file(file)?.text().line(line)?.as_str();
        let characters: Vec<char> = text.chars().collect();
        let at = usize::try_from(column.value()).ok()?;
        let is_word = |character: &char| character.is_alphanumeric() || *character == '_';
        if !characters.get(at).is_some_and(is_word) {
            return None;
        }
        let start = (0..at)
            .rev()
            .take_while(|position| characters.get(*position).is_some_and(is_word))
            .last()
            .unwrap_or(at);
        let end = (at..characters.len())
            .take_while(|position| characters.get(*position).is_some_and(is_word))
            .last()
            .unwrap_or(at);
        let word: String = characters
            .iter()
            .skip(start)
            .take(end + 1 - start)
            .collect();
        Some(Word {
            start: Column::new(u32::try_from(start).unwrap_or(0)),
            text: Label::new(word),
        })
    }

    pub(crate) fn probing(&self, file: FileId, line: Line, column: Column) -> Probing {
        let Some(source) = self.index.file(file) else {
            return Probing::Nothing;
        };
        let language = source.language();
        match language.filter(|language| !self.work.no_server(*language)) {
            Some(language) => {
                let text = source.text().line(line);
                Probing::Server {
                    language,
                    probe: Probe {
                        position: DocumentPosition {
                            file: source.path().clone(),
                            line,
                            character: character_at(text, column),
                        },
                        hash: source.hash(),
                    },
                }
            }
            None if language.is_some() => Probing::Resolver,
            None => Probing::Nothing,
        }
    }

    pub(crate) fn close_hover(&mut self, file: FileId, line: Line, column: Column) {
        if let Some(word) = self.word_at(file, line, column) {
            self.queries.closed = Some(WordSpot {
                file,
                line,
                start: word.start,
            });
        }
    }

    pub(crate) fn hovering(&self, file: FileId, line: Line, column: Column) -> Hovering {
        let Some(word) = self.word_at(file, line, column) else {
            return Hovering::Nothing;
        };
        let spot = WordSpot {
            file,
            line,
            start: word.start,
        };
        match self.queries.closed {
            Some(closed) if closed == spot => return Hovering::Nothing,
            Some(_) => return Hovering::Reopen,
            None => {}
        }
        match self.probing(file, line, word.start) {
            Probing::Server { language, probe } => match self.queries.hovers.get(&probe) {
                Some(Hovered::Answered(text)) => text
                    .clone()
                    .map_or(Hovering::Nothing, |text| Hovering::Tip(Tip::Text(text))),
                Some(Hovered::Asked) => Hovering::Nothing,
                None => Hovering::Ask { language, probe },
            },
            Probing::Resolver => self
                .symbol_at(file, line, column)
                .map_or(Hovering::Nothing, |symbol| {
                    Hovering::Tip(Tip::Symbol(symbol))
                }),
            Probing::Nothing => Hovering::Nothing,
        }
    }

    pub(crate) fn symbol_at(&self, file: FileId, line: Line, column: Column) -> Option<SymbolId> {
        let word = self.word_at(file, line, column)?;
        let candidates = self
            .index
            .find_symbols(&SymbolQuery::from(word.text.as_str()));
        let here = Location {
            file: self.index.file(file)?.path().clone(),
            line,
        };
        candidates
            .iter()
            .copied()
            .find(|candidate| {
                self.index
                    .symbol(*candidate)
                    .is_some_and(|symbol| symbol.references().contains(&here))
            })
            .or_else(|| {
                candidates
                    .iter()
                    .copied()
                    .find(|candidate| candidate.file() == file)
            })
            .or_else(|| candidates.first().copied())
    }
}

#[cfg(test)]
mod tests {
    use domain::RelativePath;

    use super::*;

    fn probe(line: u32) -> Probe {
        Probe {
            position: DocumentPosition {
                file: RelativePath::new("src/lib.rs"),
                line: Line::new(line),
                character: Character::new(4),
            },
            hash: TextHash::new(7),
        }
    }

    fn after(millis: u64) -> Duration {
        Duration::from_millis(millis)
    }

    #[test]
    fn a_word_the_pointer_moves_to_is_asked_while_the_last_one_still_waits() {
        let (left, now) = (probe(1), probe(2));
        let mut queries = Queries::default();
        assert!(matches!(
            queries.hover_step(&left, after(0)),
            HoverStep::Wait
        ));
        assert!(matches!(
            queries.hover_step(&left, after(200)),
            HoverStep::Ask
        ));
        queries.hover_sent(&left);
        assert!(matches!(
            queries.hover_step(&now, after(250)),
            HoverStep::Wait
        ));
        assert!(queries.asks_now(&now, after(450)));
        assert!(matches!(
            queries.hover_step(&now, after(450)),
            HoverStep::Ask
        ));
        queries.hover_sent(&now);
        assert!(!queries.asks_now(&now, after(900)));
    }

    #[test]
    fn a_cancelled_hover_is_forgotten_and_an_answered_one_kept() {
        let (left, now) = (probe(1), probe(2));
        let mut queries = Queries::default();
        queries.hover_step(&left, after(0));
        queries.hover_step(&left, after(200));
        queries.hover_sent(&left);
        queries.hover_step(&now, after(250));
        queries.hover_step(&now, after(450));
        queries.hover_sent(&now);
        queries.hover_answered(left.clone(), HoverReply::Cancelled);
        assert!(!queries.hovers.contains_key(&left));
        assert!(!queries.asks_now(&now, after(1000)));
        queries.hover_answered(now.clone(), HoverReply::Given(None));
        assert_eq!(queries.hovers.get(&now), Some(&Hovered::Answered(None)));
        assert_eq!(queries.asked(), Count::ZERO);
        assert!(queries.asks_now(&now, after(1000)));
    }
}

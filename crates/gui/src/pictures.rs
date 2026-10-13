use std::collections::BTreeMap;
use std::mem;
use std::rc::Rc;
use std::time::Duration;

use domain::{PictureSource, SignedPicture, WebLink};
use ui::{Count, Extent, Label, Picture, PictureBytes, PictureKey, Px};

use crate::app::{App, picture_cache};
use crate::diffs::Loading;
use crate::runtime::{Job, Landing, landed};

const AT_ONCE: Count = Count::new(3);

pub(crate) struct Fetched {
    link: WebLink,
    result: Result<domain::Picture, Label>,
}

pub(crate) struct AnimationFrame {
    picture: Rc<Picture>,
    shown_for: Duration,
}

pub(crate) struct Animation {
    frames: Vec<AnimationFrame>,
    started_at: Duration,
    size: Extent,
}

pub(crate) struct Shown {
    pub(crate) picture: Rc<Picture>,
    pub(crate) next: Option<Duration>,
}

impl Animation {
    pub(crate) const fn size(&self) -> Extent {
        self.size
    }

    pub(crate) fn at(&self, now: Duration) -> Option<Shown> {
        let cycle: Duration = self.frames.iter().map(|frame| frame.shown_for).sum();
        let first = self.frames.first().map(|frame| Rc::clone(&frame.picture))?;
        if cycle.is_zero() {
            return Some(Shown {
                picture: first,
                next: None,
            });
        }
        let since = now.saturating_sub(self.started_at);
        let into =
            Duration::from_nanos(u64::try_from(since.as_nanos() % cycle.as_nanos()).unwrap_or(0));
        let mut ends = Duration::ZERO;
        for frame in &self.frames {
            ends += frame.shown_for;
            if into < ends {
                return Some(Shown {
                    picture: Rc::clone(&frame.picture),
                    next: Some(now + ends.saturating_sub(into)),
                });
            }
        }
        Some(Shown {
            picture: first,
            next: Some(now),
        })
    }
}

#[derive(Default)]
pub(crate) struct Pictures {
    loaded: BTreeMap<WebLink, Loading<Animation>>,
    last: PictureKey,
    pub(crate) wake: Option<Duration>,
}

impl Pictures {
    pub(crate) const fn loaded(&self) -> &BTreeMap<WebLink, Loading<Animation>> {
        &self.loaded
    }

    pub(crate) fn forget_failed(&mut self) {
        self.loaded
            .retain(|_, loading| !matches!(loading, Loading::Failed(_)));
    }

    pub(crate) fn get(&self, link: &WebLink) -> Option<&Loading<Animation>> {
        self.loaded.get(link)
    }

    pub(crate) fn wake_at(&mut self, at: Duration) {
        self.wake = Some(self.wake.map_or(at, |wake| wake.min(at)));
    }

    fn land(&mut self, fetched: Fetched, now: Duration) {
        let loaded = match fetched.result {
            Ok(decoded) => {
                let size = Extent::new(
                    Px::new(i32::try_from(decoded.width.get()).unwrap_or(0)),
                    Px::new(i32::try_from(decoded.height.get()).unwrap_or(0)),
                );
                let frames = decoded
                    .frames
                    .into_iter()
                    .map(|frame| {
                        self.last = PictureKey::new(self.last.get().wrapping_add(1));
                        let picture = Rc::new(Picture {
                            key: self.last,
                            size,
                            rgba: PictureBytes::new(frame.rgba.into_bytes()),
                        });
                        AnimationFrame {
                            picture,
                            shown_for: frame.shown_for,
                        }
                    })
                    .collect();
                Loading::Got(Animation {
                    frames,
                    started_at: now,
                    size,
                })
            }
            Err(why) => Loading::Failed(why),
        };
        self.loaded.insert(fetched.link, loaded);
    }
}

#[derive(Default)]
pub(crate) struct PictureJobs {
    running: Vec<Job<Fetched>>,
    waiting: Vec<WebLink>,
}

impl PictureJobs {
    pub(crate) fn busy(&self) -> bool {
        !self.running.is_empty() || !self.waiting.is_empty()
    }
}

impl App {
    pub(crate) fn need_picture(&mut self, link: WebLink) {
        if self.model.review.pictures.loaded.contains_key(&link) {
            return;
        }
        self.model
            .review
            .pictures
            .loaded
            .insert(link.clone(), Loading::Asking);
        self.jobs.pictures.waiting.push(link);
        self.start_pictures();
    }

    fn start_pictures(&mut self) {
        let open = self.model.review.open.as_ref();
        let repo = open.map(|open| open.repo.clone());
        let signed: Vec<SignedPicture> = open
            .and_then(|open| open.conversation())
            .map(|conversation| conversation.signed_pictures.clone())
            .unwrap_or_default();
        let access = self.model.review.desk.access().clone();
        let jobs = &mut self.jobs.pictures;
        while jobs.running.len() < AT_ONCE.get() && !jobs.waiting.is_empty() {
            let link = jobs.waiting.remove(0);
            let cache = picture_cache();
            let repo = repo.clone();
            let signed = signed.clone();
            let access = access.clone();
            jobs.running.push(Job::start(move || {
                let source = access.picture_source(&link).unwrap_or_else(|| {
                    repo.and_then(|repo| repo.forge().ok()).map_or_else(
                        || PictureSource::anonymous(&link),
                        |forge| forge.picture_source(&link, &signed),
                    )
                });
                let result = io_picture::load(&link, &source, &cache)
                    .map_err(|error| Label::new(error.to_string()));
                Fetched { link, result }
            }));
        }
    }

    pub(crate) fn poll_pictures(&mut self) {
        let mut still = Vec::new();
        for job in mem::take(&mut self.jobs.pictures.running) {
            let mut slot = Some(job);
            match landed(&mut slot) {
                Landing::Waiting => still.extend(slot),
                Landing::Landed(fetched) => {
                    self.model.review.pictures.land(fetched, self.model.now);
                }
                Landing::Failed => {}
            }
        }
        self.jobs.pictures.running = still;
        self.start_pictures();
    }
}

#[cfg(test)]
mod tests;

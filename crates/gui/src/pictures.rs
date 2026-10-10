use std::collections::BTreeMap;
use std::mem;
use std::rc::Rc;

use domain::WebLink;
use ui::{Count, Extent, Label, Picture, PictureBytes, PictureKey, Px};

use crate::app::{App, picture_cache};
use crate::diffs::Loading;
use crate::runtime::{Job, Landing, landed};

const AT_ONCE: Count = Count::new(3);

pub(crate) struct Fetched {
    link: WebLink,
    result: Result<domain::Picture, Label>,
}

#[derive(Default)]
pub(crate) struct Pictures {
    loaded: BTreeMap<WebLink, Loading<Rc<Picture>>>,
    last: PictureKey,
}

impl Pictures {
    pub(crate) fn get(&self, link: &WebLink) -> Option<&Loading<Rc<Picture>>> {
        self.loaded.get(link)
    }

    fn land(&mut self, fetched: Fetched) {
        let loaded = match fetched.result {
            Ok(picture) => {
                self.last = PictureKey::new(self.last.get().wrapping_add(1));
                let size = Extent::new(
                    Px::new(i32::try_from(picture.width.get()).unwrap_or(0)),
                    Px::new(i32::try_from(picture.height.get()).unwrap_or(0)),
                );
                Loading::Got(Rc::new(Picture {
                    key: self.last,
                    size,
                    rgba: PictureBytes::new(picture.rgba.into_bytes()),
                }))
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
        let jobs = &mut self.jobs.pictures;
        while jobs.running.len() < AT_ONCE.get() && !jobs.waiting.is_empty() {
            let link = jobs.waiting.remove(0);
            let cache = picture_cache();
            jobs.running.push(Job::start(move || {
                let result =
                    io_picture::load(&link, &cache).map_err(|error| Label::new(error.to_string()));
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
                Landing::Landed(fetched) => self.model.review.pictures.land(fetched),
                Landing::Failed => {}
            }
        }
        self.jobs.pictures.running = still;
        self.start_pictures();
    }
}

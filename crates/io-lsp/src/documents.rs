use std::collections::{BTreeMap, VecDeque};

use domain::RelativePath;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct OpenLimit(usize);

impl OpenLimit {
    pub(crate) const fn new(count: usize) -> Self {
        Self(count)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum DocumentChange {
    Close(RelativePath),
    Open,
}

#[derive(Debug)]
pub(crate) struct OpenDocuments<Version> {
    limit: OpenLimit,
    least_recent_first: VecDeque<RelativePath>,
    versions: BTreeMap<RelativePath, Version>,
}

impl<Version: Copy + PartialEq> OpenDocuments<Version> {
    pub(crate) const fn new(limit: OpenLimit) -> Self {
        Self {
            limit,
            least_recent_first: VecDeque::new(),
            versions: BTreeMap::new(),
        }
    }

    pub(crate) fn touch(&mut self, file: &RelativePath, version: Version) -> Vec<DocumentChange> {
        let held = self.versions.insert(file.clone(), version);
        self.least_recent_first.retain(|open| open != file);
        let mut changes = Vec::new();
        if held.is_none()
            && self.least_recent_first.len() >= self.limit.0
            && let Some(oldest) = self.least_recent_first.pop_front()
        {
            self.versions.remove(&oldest);
            changes.push(DocumentChange::Close(oldest));
        }
        self.least_recent_first.push_back(file.clone());
        match held {
            Some(opened) if opened == version => {}
            Some(_) => {
                changes.push(DocumentChange::Close(file.clone()));
                changes.push(DocumentChange::Open);
            }
            None => changes.push(DocumentChange::Open),
        }
        changes
    }
}

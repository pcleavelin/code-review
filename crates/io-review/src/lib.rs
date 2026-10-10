mod convert;
mod wire;

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use domain::{PrNumber, Review, Root};
use io_vcs::{Vcs, VcsError};

use crate::convert::{review_from_wire, review_to_wire};

pub use crate::convert::ParseReason;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Saved;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ReviewStamp(SystemTime);

#[derive(Debug)]
pub enum ReviewError {
    NoRepository(VcsError),
    Unreadable {
        file: PathBuf,
        error: io::Error,
    },
    Unparsable {
        file: PathBuf,
        reason: ParseReason,
    },
    CreateDirectory {
        directory: PathBuf,
        error: io::Error,
    },
    Write {
        file: PathBuf,
        error: io::Error,
    },
}

impl fmt::Display for ReviewError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoRepository(error) => write!(
                formatter,
                "reviews are kept in the repository's git directory, and none was found: {error:?}"
            ),
            Self::Unreadable { file, error } => {
                write!(formatter, "cannot read {}: {error}", file.display())
            }
            Self::Unparsable { file, reason } => write!(
                formatter,
                "cannot read {}: {}; delete it to start the review over",
                file.display(),
                reason.as_str()
            ),
            Self::CreateDirectory { directory, error } => {
                write!(formatter, "cannot create {}: {error}", directory.display())
            }
            Self::Write { file, error } => {
                write!(formatter, "cannot write {}: {error}", file.display())
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewStore {
    directory: PathBuf,
}

impl ReviewStore {
    pub fn new(root: &Root) -> Result<Self, ReviewError> {
        let shared = Vcs::detect(root)
            .and_then(|vcs| vcs.shared_directory())
            .map_err(ReviewError::NoRepository)?;
        Ok(Self::at(shared.join(wire::SHARED_DIRECTORY)))
    }

    pub fn at(directory: PathBuf) -> Self {
        Self { directory }
    }

    pub fn tree(&self, number: PrNumber) -> PathBuf {
        self.directory.join(wire::TREES).join(number.to_string())
    }

    fn file(&self, number: PrNumber) -> PathBuf {
        self.directory
            .join(wire::REVIEWS)
            .join(format!("{number}.{}", wire::EXTENSION))
    }

    pub fn load(&self, number: PrNumber) -> Result<Review, ReviewError> {
        let file = self.file(number);
        let text = match fs::read_to_string(&file) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(Review::default());
            }
            Err(error) => return Err(ReviewError::Unreadable { file, error }),
        };
        let unparsable = |at: &Path, reason: ParseReason| ReviewError::Unparsable {
            file: at.to_path_buf(),
            reason,
        };
        let wire = wire::parse(&text).map_err(|fault| unparsable(&file, fault.into()))?;
        review_from_wire(wire).map_err(|reason| unparsable(&file, reason))
    }

    pub fn save(&self, number: PrNumber, review: &Review) -> Result<Saved, ReviewError> {
        let file = self.file(number);
        let directory = self.directory.join(wire::REVIEWS);
        fs::create_dir_all(&directory)
            .map_err(|error| ReviewError::CreateDirectory { directory, error })?;
        io_store::write(&file, wire::render(&review_to_wire(review)))
            .map_err(|error| ReviewError::Write { file, error })?;
        Ok(Saved)
    }

    pub fn stamp(&self, number: PrNumber) -> Option<ReviewStamp> {
        fs::metadata(self.file(number))
            .and_then(|metadata| metadata.modified())
            .ok()
            .map(ReviewStamp)
    }
}

#[cfg(test)]
mod tests;

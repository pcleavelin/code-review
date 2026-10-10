use std::fmt;
use std::time::Duration;

use crate::review::WebLink;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PixelCount(u32);

impl PixelCount {
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u32 {
        self.0
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct Rgba(Vec<u8>);

impl Rgba {
    pub const fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.0
    }
}

impl fmt::Debug for Rgba {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} bytes", self.0.len())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PictureFrame {
    pub rgba: Rgba,
    pub shown_for: Duration,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picture {
    pub width: PixelCount,
    pub height: PixelCount,
    pub frames: Vec<PictureFrame>,
}

#[derive(Clone, PartialEq, Eq)]
pub struct Credential(String);

impl Credential {
    pub fn new(header: &str) -> Self {
        Self(header.to_owned())
    }

    pub fn as_header(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Credential {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Credential(hidden)")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PictureSource {
    pub from: WebLink,
    pub credential: Option<Credential>,
}

impl PictureSource {
    pub fn anonymous(link: &WebLink) -> Self {
        Self {
            from: link.clone(),
            credential: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SignedPicture {
    pub written: WebLink,
    pub signed: WebLink,
}

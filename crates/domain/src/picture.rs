use std::fmt;

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
pub struct Picture {
    pub width: PixelCount,
    pub height: PixelCount,
    pub rgba: Rgba,
}

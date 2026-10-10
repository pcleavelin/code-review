use std::fmt;
use std::fs;
use std::path::PathBuf;

use domain::{Picture, PixelCount, Rgba, TextHash, WebLink};
use image::imageops::FilterType;
use io_process::{Argument, Captured, Process, ProcessError, Program};

const LARGEST: PixelCount = PixelCount::new(2048);

#[derive(Clone, Copy, Debug)]
struct Literal(&'static str);

const CURL: Literal = Literal("curl");

const CURL_ARGUMENTS: [Literal; 10] = [
    Literal("--silent"),
    Literal("--show-error"),
    Literal("--location"),
    Literal("--fail"),
    Literal("--max-time"),
    Literal("30"),
    Literal("--max-filesize"),
    Literal("25000000"),
    Literal("--config"),
    Literal("-"),
];

#[derive(Debug)]
pub enum PictureError {
    Process(ProcessError),
    Fetch(Captured),
    Decode(image::ImageError),
}

impl fmt::Display for PictureError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Process(error) => write!(formatter, "curl did not run: {error:?}"),
            Self::Fetch(stderr) => write!(formatter, "not fetched: {}", stderr.text().trim()),
            Self::Decode(error) => write!(formatter, "not a picture this tool reads: {error}"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PictureCache(PathBuf);

impl PictureCache {
    pub const fn new(folder: PathBuf) -> Self {
        Self(folder)
    }

    fn place(&self, link: &WebLink) -> PathBuf {
        let hash = TextHash::of(link.as_str().as_bytes());
        self.0.join(format!("{:016x}", hash.value()))
    }
}

struct Config(String);

impl Config {
    fn of(link: &WebLink) -> Self {
        let url: String = link
            .as_str()
            .chars()
            .filter(|character| !character.is_control())
            .flat_map(|character| match character {
                '"' | '\\' => vec!['\\', character],
                other => vec![other],
            })
            .collect();
        Self(format!("url = \"{url}\"\n"))
    }
}

fn fetch(link: &WebLink, cache: &PictureCache) -> Result<Captured, PictureError> {
    let arguments: Vec<Argument> = CURL_ARGUMENTS
        .iter()
        .map(|argument| Argument::new(argument.0))
        .collect();
    let output = Process::run_with_input(
        &Program::new(CURL.0),
        &arguments,
        &cache.0,
        &Captured::of(Config::of(link).0.as_bytes()),
    )
    .map_err(PictureError::Process)?;
    if output.success() {
        Ok(output.stdout)
    } else {
        Err(PictureError::Fetch(output.stderr))
    }
}

fn decode(bytes: &Captured) -> Result<Picture, PictureError> {
    let mut image = image::load_from_memory(bytes.as_bytes()).map_err(PictureError::Decode)?;
    if image.width() > LARGEST.get() || image.height() > LARGEST.get() {
        image = image.resize(LARGEST.get(), LARGEST.get(), FilterType::Triangle);
    }
    let rgba = image.to_rgba8();
    Ok(Picture {
        width: PixelCount::new(rgba.width()),
        height: PixelCount::new(rgba.height()),
        rgba: Rgba::new(rgba.into_raw()),
    })
}

pub fn load(link: &WebLink, cache: &PictureCache) -> Result<Picture, PictureError> {
    let place = cache.place(link);
    if let Ok(bytes) = fs::read(&place) {
        return decode(&Captured::of(&bytes));
    }
    drop(fs::create_dir_all(&cache.0));
    let bytes = fetch(link, cache)?;
    drop(io_store::write(&place, bytes.as_bytes()));
    decode(&bytes)
}

#[cfg(test)]
mod tests;

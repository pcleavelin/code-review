use std::fmt;
use std::fs;
use std::io::Cursor;
use std::path::PathBuf;
use std::str;
use std::time::Duration;

use domain::{
    Credential, Picture, PictureFrame, PictureSource, PixelCount, Rgba, TextHash, WebLink,
};
use image::codecs::gif::GifDecoder;
use image::codecs::png::PngDecoder;
use image::codecs::webp::WebPDecoder;
use image::imageops::FilterType;
use image::{AnimationDecoder, Delay, DynamicImage, Frames, ImageFormat, RgbaImage};
use io_process::{Argument, Captured, Deadline, Process, ProcessError, Program};

const LARGEST: PixelCount = PixelCount::new(2048);
const MOST_PIXELS: PixelCount = PixelCount::new(16 * 1024 * 1024);
const QUICKEST: Duration = Duration::from_millis(20);
const BROWSER_DEFAULT: Duration = Duration::from_millis(100);

#[derive(Clone, Copy, Debug)]
struct Literal(&'static str);

const CURL: Literal = Literal("curl");
const MAX_TIME: Literal = Literal("--max-time");
const FETCH_DEADLINE: Deadline = Deadline::seconds(30);

const CURL_ARGUMENTS: [Literal; 8] = [
    Literal("--silent"),
    Literal("--show-error"),
    Literal("--location"),
    Literal("--fail"),
    Literal("--max-filesize"),
    Literal("25000000"),
    Literal("--config"),
    Literal("-"),
];

const CURL_ARGUMENTS_NO_REDIRECT: [Literal; 7] = [
    Literal("--silent"),
    Literal("--show-error"),
    Literal("--fail"),
    Literal("--max-filesize"),
    Literal("25000000"),
    Literal("--config"),
    Literal("-"),
];

const WRITE_OUT_REDIRECT: Literal = Literal("\\n%{redirect_url}");

#[derive(Debug)]
pub enum PictureError {
    Process(ProcessError),
    Fetch(Captured),
    Decode(image::ImageError),
}

impl fmt::Display for PictureError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Process(error) => write!(formatter, "curl did not run: {error}"),
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

struct Escaped(String);

impl Escaped {
    fn of(value: &str) -> Self {
        Self(
            value
                .chars()
                .filter(|character| !character.is_control())
                .flat_map(|character| match character {
                    '"' | '\\' => vec!['\\', character],
                    other => vec![other],
                })
                .collect(),
        )
    }
}

impl Config {
    fn of(link: &WebLink) -> Self {
        Self(format!("url = \"{}\"\n", Escaped::of(link.as_str()).0))
    }

    fn authorized(link: &WebLink, credential: &Credential) -> Self {
        Self(format!(
            "url = \"{}\"\nheader = \"{}\"\nwrite-out = \"{}\"\n",
            Escaped::of(link.as_str()).0,
            Escaped::of(credential.as_header()).0,
            WRITE_OUT_REDIRECT.0
        ))
    }
}

fn run_curl(
    arguments: &[Literal],
    config: &Config,
    cache: &PictureCache,
) -> Result<Captured, PictureError> {
    let arguments: Vec<Argument> = [
        Argument::new(MAX_TIME.0),
        Argument::new(&FETCH_DEADLINE.whole_seconds().to_string()),
    ]
    .into_iter()
    .chain(arguments.iter().map(|argument| Argument::new(argument.0)))
    .collect();
    let output = Process::run_with_input(
        &Program::new(CURL.0),
        &arguments,
        &cache.0,
        &Captured::of(config.0.as_bytes()),
        FETCH_DEADLINE,
    )
    .map_err(PictureError::Process)?;
    if output.success() {
        Ok(output.stdout)
    } else {
        Err(PictureError::Fetch(output.stderr))
    }
}

fn body_of(answer: &Captured) -> Captured {
    let bytes = answer.as_bytes();
    Captured::of(
        bytes
            .iter()
            .rposition(|byte| *byte == b'\n')
            .and_then(|at| bytes.get(..at))
            .unwrap_or(bytes),
    )
}

fn redirect_of(answer: &Captured) -> Option<WebLink> {
    let bytes = answer.as_bytes();
    bytes
        .iter()
        .rposition(|byte| *byte == b'\n')
        .and_then(|at| bytes.get(at + 1..))
        .and_then(|tail| str::from_utf8(tail).ok())
        .and_then(WebLink::new)
}

fn fetch(source: &PictureSource, cache: &PictureCache) -> Result<Captured, PictureError> {
    let Some(credential) = &source.credential else {
        return run_curl(&CURL_ARGUMENTS, &Config::of(&source.from), cache);
    };
    let answer = run_curl(
        &CURL_ARGUMENTS_NO_REDIRECT,
        &Config::authorized(&source.from, credential),
        cache,
    )?;
    match redirect_of(&answer) {
        Some(next) => run_curl(&CURL_ARGUMENTS, &Config::of(&next), cache),
        None => Ok(body_of(&answer)),
    }
}

fn shrunk(image: &DynamicImage) -> RgbaImage {
    if image.width() > LARGEST.get() || image.height() > LARGEST.get() {
        image
            .resize(LARGEST.get(), LARGEST.get(), FilterType::Triangle)
            .to_rgba8()
    } else {
        image.to_rgba8()
    }
}

fn frame_time(delay: Delay) -> Duration {
    let (numerator, denominator) = delay.numer_denom_ms();
    let shown = Duration::from_millis(u64::from(numerator) / u64::from(denominator.max(1)));
    if shown < QUICKEST {
        BROWSER_DEFAULT
    } else {
        shown
    }
}

fn animation(
    bytes: &Captured,
    format: ImageFormat,
) -> Result<Option<Frames<'_>>, image::ImageError> {
    let cursor = Cursor::new(bytes.as_bytes());
    Ok(match format {
        ImageFormat::Gif => Some(GifDecoder::new(cursor)?.into_frames()),
        ImageFormat::WebP => {
            let decoder = WebPDecoder::new(cursor)?;
            decoder.has_animation().then(|| decoder.into_frames())
        }
        ImageFormat::Png => {
            let decoder = PngDecoder::new(cursor)?;
            if decoder.is_apng()? {
                Some(decoder.apng()?.into_frames())
            } else {
                None
            }
        }
        _ => None,
    })
}

fn sized_like(first: &RgbaImage) -> Picture {
    Picture {
        width: PixelCount::new(first.width()),
        height: PixelCount::new(first.height()),
        frames: Vec::new(),
    }
}

fn frame(rgba: RgbaImage, shown_for: Duration) -> PictureFrame {
    PictureFrame {
        rgba: Rgba::new(rgba.into_raw()),
        shown_for,
    }
}

fn still(image: &DynamicImage) -> Picture {
    let rgba = shrunk(image);
    let mut still = sized_like(&rgba);
    still.frames.push(frame(rgba, Duration::ZERO));
    still
}

fn animated(animation: Frames<'_>) -> Option<Picture> {
    let mut playing: Option<Picture> = None;
    let mut pixels = 0_u64;
    for decoded in animation {
        let Ok(decoded) = decoded else {
            break;
        };
        let shown_for = frame_time(decoded.delay());
        let rgba = shrunk(&DynamicImage::ImageRgba8(decoded.into_buffer()));
        pixels = pixels.saturating_add(u64::from(rgba.width()) * u64::from(rgba.height()));
        let filling = playing.get_or_insert_with(|| sized_like(&rgba));
        if pixels > u64::from(MOST_PIXELS.get()) && !filling.frames.is_empty() {
            break;
        }
        filling.frames.push(frame(rgba, shown_for));
    }
    playing.filter(|picture| picture.frames.len() > 1)
}

fn decode(bytes: &Captured) -> Result<Picture, PictureError> {
    let format = image::guess_format(bytes.as_bytes()).map_err(PictureError::Decode)?;
    if let Some(frames) = animation(bytes, format).ok().flatten()
        && let Some(picture) = animated(frames)
    {
        return Ok(picture);
    }
    let image = image::load_from_memory(bytes.as_bytes()).map_err(PictureError::Decode)?;
    Ok(still(&image))
}

pub fn load(
    link: &WebLink,
    source: &PictureSource,
    cache: &PictureCache,
) -> Result<Picture, PictureError> {
    let place = cache.place(link);
    if let Ok(bytes) = fs::read(&place) {
        return decode(&Captured::of(&bytes));
    }
    drop(fs::create_dir_all(&cache.0));
    let bytes = fetch(source, cache).or_else(|refused| {
        if source.credential.is_some() {
            fetch(&PictureSource::anonymous(link), cache)
        } else {
            Err(refused)
        }
    })?;
    drop(io_store::write(&place, bytes.as_bytes()));
    decode(&bytes)
}

#[cfg(test)]
mod tests;

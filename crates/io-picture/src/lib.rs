use std::fmt;
use std::fs;
use std::io::Cursor;
use std::path::PathBuf;
use std::time::Duration;

use domain::{Picture, PictureFrame, PixelCount, Rgba, TextHash, WebLink};
use image::codecs::gif::GifDecoder;
use image::codecs::png::PngDecoder;
use image::codecs::webp::WebPDecoder;
use image::imageops::FilterType;
use image::{AnimationDecoder, Delay, DynamicImage, Frames, ImageFormat, RgbaImage};
use io_process::{Argument, Captured, Process, ProcessError, Program};

const LARGEST: PixelCount = PixelCount::new(2048);
const MOST_PIXELS: PixelCount = PixelCount::new(16 * 1024 * 1024);
const QUICKEST: Duration = Duration::from_millis(20);
const BROWSER_DEFAULT: Duration = Duration::from_millis(100);

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

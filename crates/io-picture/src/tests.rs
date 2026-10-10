use std::io::Cursor;

use image::codecs::gif::GifEncoder;
use image::{Delay, Frame, ImageFormat, Rgba as Pixel, RgbaImage};

use super::*;

fn png(width: u32, height: u32) -> Captured {
    let image = RgbaImage::from_pixel(width, height, Pixel([10, 20, 30, 255]));
    let mut bytes = Cursor::new(Vec::new());
    image.write_to(&mut bytes, ImageFormat::Png).unwrap();
    Captured::of(bytes.get_ref())
}

fn gif(frames: &[([u8; 4], u32)]) -> Captured {
    let mut bytes = Vec::new();
    {
        let mut encoder = GifEncoder::new(&mut bytes);
        for (color, millis) in frames {
            encoder
                .encode_frame(Frame::from_parts(
                    RgbaImage::from_pixel(4, 3, Pixel(*color)),
                    0,
                    0,
                    Delay::from_numer_denom_ms(*millis, 1),
                ))
                .unwrap();
        }
    }
    Captured::of(&bytes)
}

fn first_pixel(frame: &PictureFrame) -> Vec<u8> {
    frame
        .rgba
        .clone()
        .into_bytes()
        .into_iter()
        .take(4)
        .collect()
}

#[test]
fn a_png_decodes_to_one_still_frame_of_its_size() {
    let picture = decode(&png(3, 2)).unwrap();
    assert_eq!((picture.width.get(), picture.height.get()), (3, 2));
    let firsts: Vec<(Vec<u8>, Duration)> = picture
        .frames
        .iter()
        .map(|frame| (first_pixel(frame), frame.shown_for))
        .collect();
    assert_eq!(firsts, vec![(vec![10, 20, 30, 255], Duration::ZERO)]);
}

#[test]
fn an_animated_gif_keeps_every_frame_with_how_long_it_shows() {
    let picture = decode(&gif(&[([255, 0, 0, 255], 300), ([0, 0, 255, 255], 50)])).unwrap();
    assert_eq!((picture.width.get(), picture.height.get()), (4, 3));
    let frames: Vec<(Vec<u8>, Duration)> = picture
        .frames
        .iter()
        .map(|frame| (first_pixel(frame), frame.shown_for))
        .collect();
    assert_eq!(
        frames,
        vec![
            (vec![255, 0, 0, 255], Duration::from_millis(300)),
            (vec![0, 0, 255, 255], Duration::from_millis(50)),
        ]
    );
}

#[test]
fn a_frame_without_a_delay_shows_as_long_as_a_browser_shows_it() {
    let picture = decode(&gif(&[([255, 0, 0, 255], 0), ([0, 0, 255, 255], 10)])).unwrap();
    let shown: Vec<Duration> = picture.frames.iter().map(|frame| frame.shown_for).collect();
    assert_eq!(
        shown,
        vec![Duration::from_millis(100), Duration::from_millis(100)]
    );
}

#[test]
fn a_gif_of_one_frame_is_a_still_picture() {
    let picture = decode(&gif(&[([0, 255, 0, 255], 500)])).unwrap();
    let shown: Vec<Duration> = picture.frames.iter().map(|frame| frame.shown_for).collect();
    assert_eq!(shown, vec![Duration::ZERO]);
}

#[test]
fn a_huge_picture_is_shrunk_to_fit_and_keeps_its_shape() {
    let picture = decode(&png(4096, 1024)).unwrap();
    assert_eq!((picture.width.get(), picture.height.get()), (2048, 512));
}

#[test]
fn bytes_that_are_no_picture_fail_to_decode() {
    assert!(matches!(
        decode(&Captured::of(b"<html>not found</html>")),
        Err(PictureError::Decode(_))
    ));
}

#[test]
fn a_quote_in_the_url_cannot_end_the_config_string() {
    let link = WebLink::new(r#"https://h.org/a"b\c.png"#).unwrap();
    assert_eq!(
        Config::of(&link).0,
        "url = \"https://h.org/a\\\"b\\\\c.png\"\n"
    );
}

#[test]
fn a_frame_of_twenty_milliseconds_keeps_its_own_time() {
    let picture = decode(&gif(&[([255, 0, 0, 255], 20), ([0, 0, 255, 255], 19)])).unwrap();
    let shown: Vec<Duration> = picture.frames.iter().map(|frame| frame.shown_for).collect();
    assert_eq!(
        shown,
        vec![Duration::from_millis(20), Duration::from_millis(100)]
    );
}

#[test]
fn a_cut_off_animation_keeps_the_frames_before_the_cut() {
    let whole = gif(&[
        ([255, 0, 0, 255], 100),
        ([0, 255, 0, 255], 100),
        ([0, 0, 255, 255], 100),
    ]);
    let cut: Vec<u8> = whole
        .as_bytes()
        .iter()
        .take(whole.as_bytes().len() - 8)
        .copied()
        .collect();
    let picture = decode(&Captured::of(&cut)).unwrap();
    let colors: Vec<Vec<u8>> = picture.frames.iter().map(first_pixel).collect();
    assert_eq!(colors, vec![vec![255, 0, 0, 255], vec![0, 255, 0, 255]]);
}

use std::io::Cursor;

use image::{ImageFormat, Rgba as Pixel, RgbaImage};

use super::*;

fn png(width: u32, height: u32) -> Captured {
    let image = RgbaImage::from_pixel(width, height, Pixel([10, 20, 30, 255]));
    let mut bytes = Cursor::new(Vec::new());
    image.write_to(&mut bytes, ImageFormat::Png).unwrap();
    Captured::of(bytes.get_ref())
}

#[test]
fn a_png_decodes_to_rgba_of_its_size() {
    let picture = decode(&png(3, 2)).unwrap();
    assert_eq!((picture.width.get(), picture.height.get()), (3, 2));
    assert_eq!(&picture.rgba.into_bytes()[..4], &[10, 20, 30, 255]);
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

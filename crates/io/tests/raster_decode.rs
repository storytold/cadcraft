//! Raster image files decoded to pixels (IMAGEATTACH): round trip through PNG, and hostile files
//! (truncated, random, absurd sizes) refused without a panic.

use cadcraft_doc::Raster;
use cadcraft_io::raster::{MAX_IMAGE_PIXELS, decode, encode_png, probe};

/// A 3 x 2 image with a different colour per pixel, one of them half transparent.
fn sample() -> Raster {
    let px: [[u8; 4]; 6] = [[255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255], [255, 255, 0, 255], [0, 255, 255, 128], [10, 20, 30, 255]];
    Raster::new(3, 2, px.concat()).unwrap()
}

#[test]
fn png_round_trips_pixel_for_pixel() {
    let r = sample();
    let png = encode_png(&r).unwrap();
    assert_eq!(probe(&png).unwrap(), (3, 2));
    let back = decode(&png).unwrap();
    assert_eq!(back, r);
    assert!(!back.opaque());
    assert_eq!(back.pixel(1, 1), Some([0, 255, 255, 128]));
    assert_eq!(back.pixel(3, 0), None);
}

#[test]
fn hostile_image_does_not_panic() {
    let png = encode_png(&sample()).unwrap();
    // Empty, random, not an image, and every truncation of a real file.
    for bytes in [&b""[..], &[0u8; 64], b"0\nSECTION\n2\nHEADER\n", &[0xff; 4096]] {
        assert!(decode(bytes).is_err());
        assert!(probe(bytes).is_err());
    }
    for n in 0..png.len() {
        let _ = probe(&png[..n]);
        let _ = decode(&png[..n]);
    }
    assert!(decode(&png[..png.len() / 2]).is_err());
    // Every byte flipped in turn: an error or an image, never a panic.
    for i in 0..png.len() {
        let mut b = png.clone();
        b[i] ^= 0xff;
        let _ = decode(&b);
    }
}

#[test]
fn an_absurd_declared_size_is_refused_before_decoding() {
    // A PNG declaring 60000 x 60000 pixels (3.6 gigapixels) with no pixel data.
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = b"IHDR".to_vec();
    ihdr.extend(60000u32.to_be_bytes());
    ihdr.extend(60000u32.to_be_bytes());
    ihdr.extend([8, 6, 0, 0, 0]);
    for chunk in [&ihdr[..], b"IDAT", b"IEND"] {
        png.extend(u32::try_from(chunk.len() - 4).unwrap().to_be_bytes());
        png.extend(chunk);
        png.extend(crc32(chunk).to_be_bytes());
    }
    assert!(u64::from(60000u32) * 60000 > MAX_IMAGE_PIXELS);
    let e = decode(&png).unwrap_err().to_string();
    assert!(e.contains("megapixels"), "{e}");
}

fn crc32(data: &[u8]) -> u32 {
    let mut c = !0u32;
    for b in data {
        c ^= u32::from(*b);
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
        }
    }
    !c
}

#[test]
fn raster_new_checks_the_buffer() {
    assert!(Raster::new(0, 4, Vec::new()).is_none());
    assert!(Raster::new(2, 2, vec![0; 15]).is_none());
    assert!(Raster::new(u32::MAX, u32::MAX, vec![0; 16]).is_none());
    assert!(Raster::new(2, 2, vec![255; 16]).unwrap().opaque());
}

/// `r` written in `format` by the image library.
fn encoded(r: &Raster, format: image::ImageFormat, color: image::ExtendedColorType, bytes: &[u8]) -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    image::write_buffer_with_format(&mut out, bytes, r.width(), r.height(), color, format).unwrap();
    out.into_inner()
}

#[test]
fn bmp_tiff_and_jpeg_files_are_read() {
    // Opaque colours in 8 x 8 blocks (JPEG works in blocks of that size).
    let (w, h) = (16u32, 8u32);
    let rgb: Vec<u8> = (0..h).flat_map(|_| (0..w).flat_map(|x| if x < 8 { [200u8, 30, 30] } else { [30, 30, 200] })).collect();
    let rgba: Vec<u8> = rgb.chunks(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect();
    let r = Raster::new(w, h, rgba).unwrap();

    for format in [image::ImageFormat::Bmp, image::ImageFormat::Tiff] {
        let file = encoded(&r, format, image::ExtendedColorType::Rgb8, &rgb);
        assert_eq!(probe(&file).unwrap(), (w, h), "{format:?}");
        let back = decode(&file).unwrap();
        assert_eq!(back, r, "{format:?} keeps every pixel");
        assert!(back.opaque());
    }

    // JPEG loses a little: the colours are close, the size exact.
    let file = encoded(&r, image::ImageFormat::Jpeg, image::ExtendedColorType::Rgb8, &rgb);
    let back = decode(&file).unwrap();
    assert_eq!((back.width(), back.height()), (w, h));
    for (x, want) in [(2, [200u8, 30, 30]), (13, [30, 30, 200])] {
        let got = back.pixel(x, 4).unwrap();
        assert!(got.iter().zip(want).all(|(a, b)| a.abs_diff(b) <= 12) && got[3] == 255, "{got:?} at {x}");
    }

    // Grey and grey-with-alpha files become RGBA.
    let grey = Raster::new(2, 1, vec![0, 0, 0, 255, 255, 255, 255, 255]).unwrap();
    assert_eq!(decode(&encoded(&grey, image::ImageFormat::Png, image::ExtendedColorType::L8, &[0, 255])).unwrap(), grey);
    let grey_alpha = Raster::new(2, 1, vec![9, 9, 9, 0, 255, 255, 255, 128]).unwrap();
    assert_eq!(decode(&encoded(&grey_alpha, image::ImageFormat::Png, image::ExtendedColorType::La8, &[9, 0, 255, 128])).unwrap(), grey_alpha);
}

#[test]
fn damaged_files_of_every_format_do_not_panic() {
    let (w, h) = (16u32, 8u32);
    let rgb = vec![120u8; (w * h * 3) as usize];
    let r = Raster::new(w, h, vec![255; (w * h * 4) as usize]).unwrap();
    for format in [image::ImageFormat::Bmp, image::ImageFormat::Tiff, image::ImageFormat::Jpeg] {
        let file = encoded(&r, format, image::ExtendedColorType::Rgb8, &rgb);
        for n in 0..file.len() {
            let _ = probe(&file[..n]);
            let _ = decode(&file[..n]);
        }
        for i in 0..file.len() {
            let mut b = file.clone();
            b[i] ^= 0xff;
            let _ = decode(&b);
            b[i] = 0;
            let _ = decode(&b);
        }
    }
}

#[test]
fn the_pixels_never_reach_the_drawing_file() {
    use cadcraft_doc::{Common, Drawing, EntityKind, Image, Space};
    use cadcraft_geom::{Vec2, Vec3};
    let mut d = Drawing::new_metric();
    let i = Image {
        insert: Vec3::new(3.0, 4.0, 0.0),
        u: Vec3::new(0.5, 0.0, 0.0),
        v: Vec3::new(0.0, 0.5, 0.0),
        size: Vec2::new(3.0, 2.0),
        path: "maps/site.png".into(),
        name: "site".into(),
        ..Image::default()
    };
    d.add(&Space::Model, Common::default(), EntityKind::Image(i.clone())).unwrap();
    d.rasters.insert("maps/site.png", std::sync::Arc::new(sample()));
    let text = cadcraft_io::write_dxf(&d);
    assert!(text.contains("maps/site.png"));
    let back = cadcraft_io::read_dxf(text.as_bytes()).unwrap();
    assert!(back.rasters.is_empty(), "a drawing just read has no pixels");
    let got: Vec<&Image> = back.model.iter().filter_map(|e| if let EntityKind::Image(i) = &e.kind { Some(i) } else { None }).collect();
    // An unclipped image is written with the boundary of the whole image, switched off.
    let i = Image { clip: vec![Vec2::new(-0.5, -0.5), Vec2::new(2.5, 1.5)], ..i };
    assert_eq!(got, [&i]);
    // Loaded pixels don't make two drawings differ.
    let mut bare = d.clone();
    bare.rasters.remove("maps/site.png");
    assert!(bare.rasters.is_empty() && d.rasters.len() == 1);
    assert_eq!(bare, d);
}

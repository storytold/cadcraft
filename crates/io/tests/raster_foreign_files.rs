//! Raster image files CADCraft did not write itself (#956): files made by another program, and
//! files laid out by hand from the formats' public descriptions, so reading is not only checked
//! against the library that also does the writing.
//!
//! The four embedded files hold our own synthetic picture (16 x 8 pixels: the left half
//! (200, 30, 30), the right half (30, 30, 200)), converted from a hand-built PNG with the macOS
//! command `sips` (`-s format jpeg|tiff|bmp|png`, colour profile removed). They contain nothing
//! but those pixels and the encoder's headers.

use cadcraft_doc::Raster;
use cadcraft_io::raster::{decode, probe};

/// JPEG, baseline, quality 90, with JFIF and Exif headers.
const SIPS_JPEG: &str = "ffd8ffe000104a46494600010100004800480000ffe1004c4578696600004d4d002a00000008000187690004000000010000001a000000000003a001 \
     000300000001ffff0000a00200040000000100000010a0030004000000010000000800000000ffed003850686f746f73686f7020332e30003842494d \
     04040000000000003842494d0425000000000010d41d8cd98f00b204e9800998ecf8427effc00011080008001003012200021101031101ffc4001f00 \
     00010501010101010100000000000000000102030405060708090a0bffc400b5100002010303020403050504040000017d0102030004110512213141 \
     0613516107227114328191a1082342b1c11552d1f02433627282090a161718191a25262728292a3435363738393a434445464748494a535455565758 \
     595a636465666768696a737475767778797a838485868788898a92939495969798999aa2a3a4a5a6a7a8a9aab2b3b4b5b6b7b8b9bac2c3c4c5c6c7c8 \
     c9cad2d3d4d5d6d7d8d9dae1e2e3e4e5e6e7e8e9eaf1f2f3f4f5f6f7f8f9faffc4001f01000301010101010101010100000000000001020304050607 \
     08090a0bffc400b51100020102040403040705040400010277000102031104052131061241510761711322328108144291a1b1c109233352f0156272 \
     d10a162434e125f11718191a262728292a35363738393a434445464748494a535455565758595a636465666768696a737475767778797a8283848586 \
     8788898a92939495969798999aa2a3a4a5a6a7a8a9aab2b3b4b5b6b7b8b9bac2c3c4c5c6c7c8c9cad2d3d4d5d6d7d8d9dae2e3e4e5e6e7e8e9eaf2f3 \
     f4f5f6f7f8f9faffdb004300020202020202030202030503030305060505050506080606060606080a0808080808080a0a0a0a0a0a0a0a0c0c0c0c0c \
     0c0e0e0e0e0e0f0f0f0f0f0f0f0f0f0fffdb00430102030304040407040407100b090b10101010101010101010101010101010101010101010101010 \
     10101010101010101010101010101010101010101010101010ffdd00040001ffda000c03010002110311003f00f90ebe7fafa02be7fafdc7e8f3ff00 \
     331ffb87ff00b79fd1de3aff00cc07fdbfff00b69fffd9";
/// TIFF, big-endian, LZW compressed.
const SIPS_TIFF_LZW: &str = "4d4d002a000000680002a00200040000000100000010a0030004000000010000000800000000803203c1e004160d0784424ac00554261d0f814121f1 \
     38342e1b148a44631148b46e271a8f43a3b2184c82490791c9e0d2695002532d964aa5f2a98c9e672780800000100100000300000001001000000101 \
     0003000000010008000001020003000000030000012e010300030000000100050000010600030000000100020000010a000300000001000100000111 \
     00040000000100000026011200030000000100010000011500030000000100030000011600030000000100080000011700040000000100000041011c \
     00030000000100010000012800030000000100020000013d000300000001000200000153000300000003000001348769000400000001000000080000 \
     0000000800080008000100010001";
/// TIFF, big-endian, not compressed.
const SIPS_TIFF_PLAIN: &str = "4d4d002a000001a60002a00200040000000100000010a0030004000000010000000800000000c81e1ec81e1ec81e1ec81e1ec81e1ec81e1ec81e1ec8 \
     1e1e1e1ec81e1ec81e1ec81e1ec81e1ec81e1ec81e1ec81e1ec8c81e1ec81e1ec81e1ec81e1ec81e1ec81e1ec81e1ec81e1e1e1ec81e1ec81e1ec81e \
     1ec81e1ec81e1ec81e1ec81e1ec8c81e1ec81e1ec81e1ec81e1ec81e1ec81e1ec81e1ec81e1e1e1ec81e1ec81e1ec81e1ec81e1ec81e1ec81e1ec81e \
     1ec8c81e1ec81e1ec81e1ec81e1ec81e1ec81e1ec81e1ec81e1e1e1ec81e1ec81e1ec81e1ec81e1ec81e1ec81e1ec81e1ec8c81e1ec81e1ec81e1ec8 \
     1e1ec81e1ec81e1ec81e1ec81e1e1e1ec81e1ec81e1ec81e1ec81e1ec81e1ec81e1ec81e1ec8c81e1ec81e1ec81e1ec81e1ec81e1ec81e1ec81e1ec8 \
     1e1e1e1ec81e1ec81e1ec81e1ec81e1ec81e1ec81e1ec81e1ec8c81e1ec81e1ec81e1ec81e1ec81e1ec81e1ec81e1ec81e1e1e1ec81e1ec81e1ec81e \
     1ec81e1ec81e1ec81e1ec81e1ec8c81e1ec81e1ec81e1ec81e1ec81e1ec81e1ec81e1ec81e1e1e1ec81e1ec81e1ec81e1ec81e1ec81e1ec81e1ec81e \
     1ec8000f0100000300000001001000000101000300000001000800000102000300000003000002600103000300000001000100000106000300000001 \
     00020000010a000300000001000100000111000400000001000000260112000300000001000100000115000300000001000300000116000300000001 \
     00080000011700040000000100000180011c000300000001000100000128000300000001000200000153000300000003000002668769000400000001 \
     0000000800000000000800080008000100010001";
/// BMP, 24 bits, rows stored top-down (negative height).
const SIPS_BMP: &str = "424db601000000000000360000002800000010000000f8ffffff010018000000000080010000000000000000000000000000000000001e1ec81e1ec8 \
     1e1ec81e1ec81e1ec81e1ec81e1ec81e1ec8c81e1ec81e1ec81e1ec81e1ec81e1ec81e1ec81e1ec81e1e1e1ec81e1ec81e1ec81e1ec81e1ec81e1ec8 \
     1e1ec81e1ec8c81e1ec81e1ec81e1ec81e1ec81e1ec81e1ec81e1ec81e1e1e1ec81e1ec81e1ec81e1ec81e1ec81e1ec81e1ec81e1ec8c81e1ec81e1e \
     c81e1ec81e1ec81e1ec81e1ec81e1ec81e1e1e1ec81e1ec81e1ec81e1ec81e1ec81e1ec81e1ec81e1ec8c81e1ec81e1ec81e1ec81e1ec81e1ec81e1e \
     c81e1ec81e1e1e1ec81e1ec81e1ec81e1ec81e1ec81e1ec81e1ec81e1ec8c81e1ec81e1ec81e1ec81e1ec81e1ec81e1ec81e1ec81e1e1e1ec81e1ec8 \
     1e1ec81e1ec81e1ec81e1ec81e1ec81e1ec8c81e1ec81e1ec81e1ec81e1ec81e1ec81e1ec81e1ec81e1e1e1ec81e1ec81e1ec81e1ec81e1ec81e1ec8 \
     1e1ec81e1ec8c81e1ec81e1ec81e1ec81e1ec81e1ec81e1ec81e1ec81e1e1e1ec81e1ec81e1ec81e1ec81e1ec81e1ec81e1ec81e1ec8c81e1ec81e1e \
     c81e1ec81e1ec81e1ec81e1ec81e1ec81e1e";
/// PNG as that program writes it (its own chunks and compression).
const SIPS_PNG: &str = "89504e470d0a1a0a0000000d49484452000000100000000808020000007f14e8c00000000467414d410000b18f0bfc6105000000206348524d00007a \
     26000080840000fa00000080e8000075300000ea6000003a98000017709cba513c00000038655849664d4d002a000000080001876900040000000100 \
     00001a000000000002a00200040000000100000010a0030004000000010000000800000000470d8f8b000001c869545874584d4c3a636f6d2e61646f \
     62652e786d7000000000003c783a786d706d65746120786d6c6e733a783d2261646f62653a6e733a6d6574612f2220783a786d70746b3d22584d5020 \
     436f726520362e302e30223e0a2020203c7264663a52444620786d6c6e733a7264663d22687474703a2f2f7777772e77332e6f72672f313939392f30 \
     322f32322d7264662d73796e7461782d6e7323223e0a2020202020203c7264663a4465736372697074696f6e207264663a61626f75743d22220a2020 \
     20202020202020202020786d6c6e733a657869663d22687474703a2f2f6e732e61646f62652e636f6d2f657869662f312e302f223e0a202020202020 \
     2020203c657869663a436f6c6f7253706163653e313c2f657869663a436f6c6f7253706163653e0a2020202020202020203c657869663a506978656c \
     5844696d656e73696f6e3e31363c2f657869663a506978656c5844696d656e73696f6e3e0a2020202020202020203c657869663a506978656c594469 \
     6d656e73696f6e3e383c2f657869663a506978656c5944696d656e73696f6e3e0a2020202020203c2f7264663a4465736372697074696f6e3e0a2020 \
     203c2f7264663a5244463e0a3c2f783a786d706d6574613e0ae6ba92b400000021494441541819633c2127c7800d8431acc226ccc08455148fe0a806 \
     3c8103970200086302148ee3f7130000000049454e44ae426082";

fn bytes(hex: &str) -> Vec<u8> {
    let digits: Vec<u8> = hex.bytes().filter(u8::is_ascii_hexdigit).collect();
    digits.chunks(2).map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap()).collect()
}

const LEFT: [u8; 3] = [200, 30, 30];
const RIGHT: [u8; 3] = [30, 30, 200];

/// The picture is the two-colour 16 x 8 one, each colour within `tolerance`.
#[track_caller]
fn is_the_picture(r: &Raster, tolerance: u8, what: &str) {
    assert_eq!((r.width(), r.height()), (16, 8), "{what}");
    assert!(r.opaque(), "{what}");
    for y in 0..8 {
        for x in 0..16 {
            // A lossy file blends the columns around the colour change (JPEG stores colour at
            // half resolution).
            if tolerance > 3 && (6..=9).contains(&x) {
                continue;
            }
            let want = if x < 8 { LEFT } else { RIGHT };
            let got = r.pixel(x, y).unwrap();
            assert!(got.iter().zip(want).all(|(a, b)| a.abs_diff(b) <= tolerance), "{what}: {got:?} at {x},{y}, wanted {want:?}");
        }
    }
}

#[test]
fn files_written_by_another_program_are_read() {
    for (hex, tolerance, what) in [
        (SIPS_PNG, 3, "PNG"),
        (SIPS_BMP, 3, "top-down BMP"),
        (SIPS_TIFF_LZW, 3, "LZW TIFF"),
        (SIPS_TIFF_PLAIN, 3, "plain TIFF"),
        (SIPS_JPEG, 14, "JPEG"),
    ] {
        let file = bytes(hex);
        assert_eq!(probe(&file).unwrap(), (16, 8), "{what}");
        is_the_picture(&decode(&file).unwrap(), tolerance, what);
        // Cut short or damaged, they are refused or read: never a panic.
        for n in 0..file.len() {
            let _ = decode(&file[..n]);
        }
        for i in 0..file.len() {
            let mut b = file.clone();
            b[i] ^= 0x55;
            let _ = decode(&b);
        }
    }
}

/// An uncompressed RGB TIFF of one strip, laid out by hand: header, pixels, then the directory.
fn tiff_by_hand(big_endian: bool, w: u32, h: u32, rgb: &[u8]) -> Vec<u8> {
    let u16b = |v: u16| if big_endian { v.to_be_bytes() } else { v.to_le_bytes() };
    let u32b = |v: u32| if big_endian { v.to_be_bytes() } else { v.to_le_bytes() };
    let mut f: Vec<u8> = if big_endian { b"MM".to_vec() } else { b"II".to_vec() };
    f.extend(u16b(42));
    let pixels_at = 8u32;
    let bits_at = pixels_at + rgb.len() as u32;
    let dir_at = bits_at + 6;
    f.extend(u32b(dir_at));
    f.extend(rgb);
    for _ in 0..3 {
        f.extend(u16b(8));
    }
    // (tag, type: 3 short / 4 long, count, value)
    let entries: [(u16, u16, u32, u32); 10] = [
        (256, 4, 1, w),
        (257, 4, 1, h),
        (258, 3, 3, bits_at),
        (259, 3, 1, 1),
        (262, 3, 1, 2),
        (273, 4, 1, pixels_at),
        (277, 3, 1, 3),
        (278, 4, 1, h),
        (279, 4, 1, rgb.len() as u32),
        (284, 3, 1, 1),
    ];
    f.extend(u16b(entries.len() as u16));
    for (tag, kind, count, value) in entries {
        f.extend(u16b(tag));
        f.extend(u16b(kind));
        f.extend(u32b(count));
        // A single short sits in the first two bytes of the value field.
        if kind == 3 && count == 1 {
            f.extend(u16b(value as u16));
            f.extend([0, 0]);
        } else {
            f.extend(u32b(value));
        }
    }
    f.extend(u32b(0));
    f
}

/// A 24-bit BMP laid out by hand: rows bottom-up, blue-green-red, each padded to four bytes.
fn bmp_by_hand(w: u32, h: u32, rgb: &[u8]) -> Vec<u8> {
    let row = (w as usize * 3).div_ceil(4) * 4;
    let size = 54 + row * h as usize;
    let mut f = b"BM".to_vec();
    f.extend((size as u32).to_le_bytes());
    f.extend([0; 4]);
    f.extend(54u32.to_le_bytes());
    f.extend(40u32.to_le_bytes());
    f.extend((w as i32).to_le_bytes());
    f.extend((h as i32).to_le_bytes());
    f.extend(1u16.to_le_bytes());
    f.extend(24u16.to_le_bytes());
    f.extend([0; 24]);
    for y in (0..h as usize).rev() {
        let line = &rgb[y * w as usize * 3..(y + 1) * w as usize * 3];
        for p in line.chunks(3) {
            f.extend([p[2], p[1], p[0]]);
        }
        f.extend(std::iter::repeat_n(0, row - w as usize * 3));
    }
    f
}

#[test]
fn files_laid_out_by_hand_are_read_pixel_for_pixel() {
    // 3 x 2: an odd width, so BMP rows are padded; every pixel different.
    let rgb: Vec<u8> = (0..18u8).map(|i| i * 13 + 5).collect();
    let rgba: Vec<u8> = rgb.chunks(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect();
    let want = Raster::new(3, 2, rgba).unwrap();
    for (file, what) in [
        (tiff_by_hand(false, 3, 2, &rgb), "little-endian TIFF"),
        (tiff_by_hand(true, 3, 2, &rgb), "big-endian TIFF"),
        (bmp_by_hand(3, 2, &rgb), "bottom-up BMP"),
    ] {
        assert_eq!(probe(&file).unwrap(), (3, 2), "{what}");
        assert_eq!(decode(&file).unwrap(), want, "{what}");
    }
}

#[test]
fn hand_made_files_that_lie_about_their_size_are_refused() {
    let rgb = [9u8; 18];
    // More pixels declared than the file holds.
    for file in [tiff_by_hand(false, 300, 200, &rgb), tiff_by_hand(true, 3, 2000, &rgb), bmp_by_hand(3, 2, &rgb)[..60].to_vec()] {
        assert!(decode(&file).is_err());
    }
    // Sizes past the limit, before any pixel is read.
    for file in [tiff_by_hand(false, 60_000, 60_000, &rgb), tiff_by_hand(true, u32::MAX, u32::MAX, &rgb)] {
        assert!(decode(&file).is_err());
    }
    let mut bmp = bmp_by_hand(3, 2, &rgb);
    bmp[18..22].copy_from_slice(&60_000i32.to_le_bytes());
    bmp[22..26].copy_from_slice(&60_000i32.to_le_bytes());
    assert!(decode(&bmp).is_err());
    // A zero-sized image.
    assert!(decode(&tiff_by_hand(false, 0, 0, &[])).is_err());
}

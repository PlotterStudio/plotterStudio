//! Ground truth for material textures: reject anything over 4096 x 4096, build
//! mip chains, and keep GPU-ready copies keyed by the BLAKE3 of the source image.

use std::io::Cursor;

use image::{ImageFormat, Rgba, RgbaImage};

use super::*;

fn png(img: &RgbaImage) -> Vec<u8> {
    let mut out = Cursor::new(Vec::new());
    img.write_to(&mut out, ImageFormat::Png).unwrap();
    out.into_inner()
}

fn solid(w: u32, h: u32, px: [u8; 4]) -> RgbaImage {
    RgbaImage::from_pixel(w, h, Rgba(px))
}

#[test]
fn a_small_png_decodes() {
    let img = decode(&png(&solid(8, 4, [10, 20, 30, 255]))).unwrap();
    assert_eq!((img.width(), img.height()), (8, 4));
    assert_eq!(img.get_pixel(3, 2).0, [10, 20, 30, 255]);
}

#[test]
fn the_size_limit_is_4096_in_each_direction() {
    assert!(decode(&png(&solid(4096, 1, [0, 0, 0, 255]))).is_ok());
    assert_eq!(
        decode(&png(&solid(4097, 1, [0, 0, 0, 255]))).unwrap_err(),
        TextureError::TooLarge {
            width: 4097,
            height: 1
        }
    );
    assert_eq!(
        decode(&png(&solid(1, 4097, [0, 0, 0, 255]))).unwrap_err(),
        TextureError::TooLarge {
            width: 1,
            height: 4097
        }
    );
}

#[test]
fn garbage_is_not_an_image() {
    assert!(matches!(
        decode(b"not an image").unwrap_err(),
        TextureError::Unreadable(_)
    ));
    assert!(matches!(
        decode(&[]).unwrap_err(),
        TextureError::Unreadable(_)
    ));
}

#[test]
fn a_mip_chain_halves_down_to_one_pixel() {
    let chain = mip_chain(&solid(8, 4, [0, 0, 0, 255]));
    let sizes: Vec<(u32, u32)> = chain.iter().map(|m| (m.width(), m.height())).collect();
    assert_eq!(sizes, [(8, 4), (4, 2), (2, 1), (1, 1)]);
    assert_eq!(mip_level_count(8, 4), 4);
    assert_eq!(mip_level_count(1, 1), 1);
    assert_eq!(mip_level_count(4096, 4096), 13);
    assert_eq!(mip_level_count(5, 3), 3);
}

#[test]
fn each_mip_averages_two_by_two_blocks() {
    let mut img = RgbaImage::new(2, 2);
    img.put_pixel(0, 0, Rgba([0, 0, 0, 255]));
    img.put_pixel(1, 0, Rgba([100, 40, 8, 255]));
    img.put_pixel(0, 1, Rgba([200, 80, 16, 255]));
    img.put_pixel(1, 1, Rgba([100, 40, 8, 255]));
    let chain = mip_chain(&img);
    assert_eq!(chain.len(), 2);
    // (0 + 100 + 200 + 100) / 4 = 100, (0 + 40 + 80 + 40) / 4 = 40, (0 + 8 + 16 + 8) / 4 = 8.
    assert_eq!(chain[1].get_pixel(0, 0).0, [100, 40, 8, 255]);
}

#[test]
fn a_solid_colour_survives_every_level() {
    for level in mip_chain(&solid(16, 16, [12, 34, 56, 255])) {
        assert!(level.pixels().all(|p| p.0 == [12, 34, 56, 255]));
    }
}

#[test]
fn the_cache_key_is_the_hash_of_the_source_bytes() {
    let bytes = png(&solid(2, 2, [1, 2, 3, 255]));
    assert_eq!(cache_key(&bytes), hash_bytes(&bytes));
    assert_ne!(cache_key(&bytes), cache_key(b"other"));
}

#[test]
fn the_cache_stores_every_mip_and_reads_them_back() {
    let dir = tempfile::tempdir().unwrap();
    let source = png(&solid(8, 4, [9, 9, 9, 255]));
    let key = write_cache(dir.path(), &source).unwrap();
    assert_eq!(key, cache_key(&source));
    let cached = read_cache(dir.path(), &key).unwrap();
    assert_eq!(cached.levels.len(), 4);
    assert_eq!((cached.width, cached.height), (8, 4));
    assert_eq!(cached.levels[0].len(), 8 * 4 * 4);
    assert_eq!(cached.levels[3], vec![9, 9, 9, 255]);
}

#[test]
fn writing_the_same_texture_twice_is_harmless() {
    let dir = tempfile::tempdir().unwrap();
    let source = png(&solid(4, 4, [5, 5, 5, 255]));
    let a = write_cache(dir.path(), &source).unwrap();
    let b = write_cache(dir.path(), &source).unwrap();
    assert_eq!(a, b);
}

#[test]
fn a_missing_cache_entry_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    assert!(read_cache(dir.path(), &cache_key(b"nothing")).is_err());
}

#[test]
fn oversized_textures_are_never_cached() {
    let dir = tempfile::tempdir().unwrap();
    let err = write_cache(dir.path(), &png(&solid(5000, 1, [0, 0, 0, 255]))).unwrap_err();
    assert!(matches!(err, TextureError::TooLarge { .. }), "{err:?}");
}

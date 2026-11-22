//! Material textures: size limits, mip chains and the GPU cache.

use std::fs;
use std::io::Cursor;
use std::path::Path;

use image::{ImageReader, RgbaImage};
use ps_core::{ContentHash, hash_bytes};

/// Largest accepted width or height in pixels.
pub const MAX_DIMENSION: u32 = 4096;

/// Why a texture was refused or could not be cached.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TextureError {
    /// The image is larger than 4096 × 4096.
    #[error("texture is {width} x {height}, larger than {MAX_DIMENSION} x {MAX_DIMENSION}")]
    TooLarge {
        /// Width in pixels.
        width: u32,
        /// Height in pixels.
        height: u32,
    },
    /// The bytes are not a readable PNG, JPEG or WebP image.
    #[error("texture cannot be read: {0}")]
    Unreadable(String),
    /// The cache could not be read or written.
    #[error("texture cache error: {0}")]
    Cache(String),
}

/// Decodes an image, refusing anything over the size limit before the pixels are allocated.
///
/// # Errors
/// [`TextureError`] for unreadable or oversized images.
pub fn decode(bytes: &[u8]) -> Result<RgbaImage, TextureError> {
    let unreadable = |e: &dyn std::fmt::Display| TextureError::Unreadable(e.to_string());
    let reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| unreadable(&e))?;
    let (width, height) = reader.into_dimensions().map_err(|e| unreadable(&e))?;
    if width > MAX_DIMENSION || height > MAX_DIMENSION {
        return Err(TextureError::TooLarge { width, height });
    }
    let img = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| unreadable(&e))?
        .decode()
        .map_err(|e| unreadable(&e))?;
    Ok(img.to_rgba8())
}

/// The number of levels in a full mip chain: `floor(log2(max(w, h))) + 1`.
#[must_use]
pub fn mip_level_count(width: u32, height: u32) -> u32 {
    32 - width.max(height).max(1).leading_zeros()
}

/// Builds the full mip chain by averaging 2 × 2 blocks (edge pixels repeat for odd sizes).
#[must_use]
pub fn mip_chain(base: &RgbaImage) -> Vec<RgbaImage> {
    let mut chain = vec![base.clone()];
    while let Some(last) = chain.last() {
        if last.width() == 1 && last.height() == 1 {
            break;
        }
        let (w, h) = ((last.width() / 2).max(1), (last.height() / 2).max(1));
        let mut next = RgbaImage::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let (x0, y0) = (x * 2, y * 2);
                let (x1, y1) = (
                    (x0 + 1).min(last.width() - 1),
                    (y0 + 1).min(last.height() - 1),
                );
                let px = [
                    last.get_pixel(x0, y0),
                    last.get_pixel(x1, y0),
                    last.get_pixel(x0, y1),
                    last.get_pixel(x1, y1),
                ];
                let mut out = [0_u8; 4];
                for (c, slot) in out.iter_mut().enumerate() {
                    let sum: u32 = px.iter().map(|p| u32::from(p.0[c])).sum();
                    // Round to nearest.
                    *slot = u8::try_from((sum + 2) / 4).unwrap_or(u8::MAX);
                }
                next.put_pixel(x, y, image::Rgba(out));
            }
        }
        chain.push(next);
    }
    chain
}

/// The cache key of a source image: the BLAKE3 hash of its bytes.
#[must_use]
pub fn cache_key(source: &[u8]) -> ContentHash {
    hash_bytes(source)
}

/// A texture read back from the cache.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedTexture {
    /// Width of the base level.
    pub width: u32,
    /// Height of the base level.
    pub height: u32,
    /// Raw RGBA bytes of every level, base first.
    pub levels: Vec<Vec<u8>>,
}

fn folder(cache_root: &Path, key: &ContentHash) -> std::path::PathBuf {
    cache_root.join("textures").join(key.to_string())
}

fn cache_err(e: impl std::fmt::Display) -> TextureError {
    TextureError::Cache(e.to_string())
}

/// Decodes a texture and stores its mip chain under `<cache_root>/textures/<key>/`.
///
/// # Errors
/// [`TextureError`] if the image is refused or the cache cannot be written.
pub fn write_cache(cache_root: &Path, source: &[u8]) -> Result<ContentHash, TextureError> {
    let img = decode(source)?;
    let key = cache_key(source);
    let dir = folder(cache_root, &key);
    if dir.join("meta.json").exists() {
        return Ok(key);
    }
    fs::create_dir_all(&dir).map_err(cache_err)?;
    let chain = mip_chain(&img);
    for (i, level) in chain.iter().enumerate() {
        fs::write(dir.join(format!("mip{i}.rgba")), level.as_raw()).map_err(cache_err)?;
    }
    let meta = format!(
        "{{\"width\":{},\"height\":{},\"levels\":{}}}",
        img.width(),
        img.height(),
        chain.len()
    );
    // The meta file goes last: its presence means the entry is complete.
    fs::write(dir.join("meta.json"), meta).map_err(cache_err)?;
    Ok(key)
}

/// Reads a cached texture.
///
/// # Errors
/// [`TextureError::Cache`] when the entry is missing or damaged.
pub fn read_cache(cache_root: &Path, key: &ContentHash) -> Result<CachedTexture, TextureError> {
    let dir = folder(cache_root, key);
    let meta: serde_json::Value =
        serde_json::from_slice(&fs::read(dir.join("meta.json")).map_err(cache_err)?)
            .map_err(cache_err)?;
    let field = |name: &str| {
        meta.get(name)
            .and_then(serde_json::Value::as_u64)
            .and_then(|v| u32::try_from(v).ok())
            .ok_or_else(|| TextureError::Cache(format!("bad {name} in cache metadata")))
    };
    let (width, height, count) = (field("width")?, field("height")?, field("levels")?);
    let levels = (0..count)
        .map(|i| fs::read(dir.join(format!("mip{i}.rgba"))).map_err(cache_err))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(CachedTexture {
        width,
        height,
        levels,
    })
}

#[cfg(test)]
mod tests;

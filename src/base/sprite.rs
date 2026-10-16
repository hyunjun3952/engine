use super::Rect;
use crate::base::in_file;
use std::sync::{
    RwLock, RwLockReadGuard,
    atomic::{AtomicU64, Ordering},
};

pub(crate) struct Image {
    pub(crate) w: usize,
    pub(crate) h: usize,
    pub(crate) px: Vec<u32>,
}

impl Image {
    /// Whether a source rectangle is at least a pixel big and lies wholly inside the picture. A sheet swapped for a smaller one
    /// by hot reload leaves frames and tiles pointing outside it; a rectangle that is not a number holds nothing.
    pub(crate) fn holds(&self, r: &Rect) -> bool {
        r.w >= 1.0 && r.h >= 1.0 && r.x >= 0.0 && r.y >= 0.0 && r.x + r.w <= self.w as f32 && r.y + r.h <= self.h as f32
    }
}

/// An image, shared as `Arc<Sprite>`. Pixels are 0xAARRGGBB; hot reload swaps them inside the same sprite.
///
/// ```
/// use engine::{Rect, Sprite};
///
/// let sprite = Sprite::new(2, 1, vec![0xffff0000, 0xff00ff00]);
/// assert_eq!(sprite.size(), (2, 1));
/// assert_eq!(sprite.bounds(), Rect::new(0.0, 0.0, 2.0, 1.0));
/// ```
pub struct Sprite {
    image: RwLock<Image>,
    version: AtomicU64,
}

impl Sprite {
    /// Reads a PNG.
    pub fn load(path: &str) -> Result<Self, String> {
        Ok(Sprite { image: RwLock::new(decode(path)?), version: AtomicU64::new(0) })
    }

    /// A sprite made in code from `w * h` pixels, row by row, as 0xAARRGGBB.
    pub fn new(w: usize, h: usize, px: Vec<u32>) -> Self {
        assert_eq!(px.len(), w * h, "a {w}x{h} sprite needs {} pixels", w * h);
        Sprite { image: RwLock::new(Image { w, h, px }), version: AtomicU64::new(0) }
    }

    /// Width and height in pixels.
    pub fn size(&self) -> (usize, usize) {
        let img = self.image();
        (img.w, img.h)
    }

    /// The whole image as a rect at the origin.
    pub fn bounds(&self) -> Rect {
        let (w, h) = self.size();
        Rect::new(0.0, 0.0, w as f32, h as f32)
    }

    #[cfg(test)]
    pub(crate) fn test_tiles() -> std::sync::Arc<Sprite> {
        std::sync::Arc::new(Sprite::load("assets/images/tiles.png").unwrap())
    }

    #[cfg(test)]
    pub(crate) fn test_ball() -> std::sync::Arc<Sprite> {
        std::sync::Arc::new(Sprite::load("assets/images/ball.png").unwrap())
    }

    pub(crate) fn image(&self) -> RwLockReadGuard<'_, Image> {
        self.image.read().unwrap()
    }

    #[cfg(any(feature = "gpu", test))]
    pub(crate) fn version(&self) -> u64 {
        self.version.load(Ordering::Acquire)
    }

    /// Swaps the pixels with `px` (the same number of them) and marks the sprite changed: a picture that is remade
    /// every frame without a new texture each time. `px` gets the old pixels back to be written over.
    pub(crate) fn swap_pixels(&self, px: &mut Vec<u32>) {
        let mut image = self.image.write().unwrap();
        assert_eq!(image.px.len(), px.len(), "a swapped picture keeps its size");
        std::mem::swap(&mut image.px, px);
        drop(image);
        self.version.fetch_add(1, Ordering::AcqRel);
    }

    pub(crate) fn reload(&self, path: &str) -> Result<(), String> {
        let image = decode(path)?;
        *self.image.write().unwrap() = image;
        self.version.fetch_add(1, Ordering::AcqRel);
        Ok(())
    }
}

fn decode(path: &str) -> Result<Image, String> {
    let img = image::open(path).map_err(in_file(path))?.to_rgba8();
    let px = img.pixels().map(|p| u32::from_be_bytes([p[3], p[0], p[1], p[2]])).collect();
    Ok(Image { w: img.width() as usize, h: img.height() as usize, px })
}

//! The CPU rasteriser, PNG files and the contact sheet.
//!
//! The paint path is `mui::vello::software::Renderer` under one scale
//! transform, so a 2x render is a 2x rasterisation, not a resampled 1x.

use std::path::Path;

use mui::scene::ResolvedScene;
use mui::vello::kurbo::Affine;
use mui::vello::software::Renderer;

use crate::Error;

/// A rasteriser reused across frames: [`software::Renderer`] at a scale,
/// giving straight RGBA.
///
/// [`software::Renderer`]: mui::vello::software::Renderer
pub struct Raster {
    pub w: u16,
    pub h: u16,
    renderer: Option<Renderer>,
}

impl Raster {
    pub fn new(w: u16, h: u16) -> Self {
        Self {
            w,
            h,
            renderer: None,
        }
    }

    /// Straight RGBA, `w * h * 4` bytes, of `scene` at `scale`.
    pub fn draw(&mut self, scene: &ResolvedScene, scale: f64) -> Result<Vec<u8>, Error> {
        let renderer = match &mut self.renderer {
            Some(r) => r,
            None => self
                .renderer
                .insert(Renderer::new((self.w.into(), self.h.into())).map_err(Error::new)?),
        };
        renderer.render(scene, Affine::scale(scale)).map_err(|e| {
            Error::new(format!(
                "paint: {e} (the CPU renderer cannot draw native GPU materials)"
            ))
        })?;
        Ok(renderer.rgba8())
    }
}

/// Write `rgba` (`w * h * 4`) as a PNG. The encoder writes no timestamp or
/// other run-dependent chunk, so the same pixels are the same bytes.
pub fn write_png(path: &Path, w: u32, h: u32, rgba: &[u8]) -> Result<(), Error> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    let file = std::fs::File::create(path)?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()?.write_image_data(rgba)?;
    Ok(())
}

/// Frames laid out left to right, top to bottom, `cols` to a row, with a
/// 2-pixel gutter in transparent black. Returns the sheet's RGBA and size.
pub fn contact_sheet(frames: &[Vec<u8>], w: usize, h: usize, cols: usize) -> (Vec<u8>, u32, u32) {
    const GUTTER: usize = 2;
    let cols = cols.clamp(1, frames.len().max(1));
    let rows = frames.len().div_ceil(cols).max(1);
    let (sw, sh) = (
        cols * w + (cols - 1) * GUTTER,
        rows * h + (rows - 1) * GUTTER,
    );
    let mut sheet = vec![0u8; sw * sh * 4];
    for (i, f) in frames.iter().enumerate() {
        let (x0, y0) = ((i % cols) * (w + GUTTER), (i / cols) * (h + GUTTER));
        for y in 0..h {
            let dst = ((y0 + y) * sw + x0) * 4;
            sheet[dst..dst + w * 4].copy_from_slice(&f[y * w * 4..(y + 1) * w * 4]);
        }
    }
    (sheet, sw as u32, sh as u32)
}

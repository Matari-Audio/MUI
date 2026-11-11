//! The CPU side of a frame: the walk over the paint list, the instances
//! it writes, and the layers they are drawn into.

use super::build::{Drawn, Instance, SLOTS, Walk};
use super::layer::{Bind, Kind, Layers};
use super::{Error, Reason, build, text};
use crate::Cache;
use crate::effects::WeldTextures;
use crate::kurbo::Affine;
use mui_scene::{Layer, Painted, ResolvedScene};

/// Where a frame's welds come from: the scene's placements, and the pool
/// that holds their textures.
#[derive(Clone, Copy)]
pub(super) struct Welds<'a> {
    pub scene: &'a ResolvedScene,
    pub pool: &'a WeldTextures,
}

/// The CPU side of a frame: the walk, the instances it writes and the glyph
/// atlas, all kept, so a steady frame allocates nothing.
pub(super) struct Instances {
    pub list: Vec<Instance>,
    pub bytes: Vec<u8>,
    pub walk: Walk,
    pub atlas: text::Atlas,
    /// Fonts, and each image's bind group, keyed on the app's buffer.
    pub cache: Cache,
    /// Each entry's device box, as of the last frame that built it.
    pub boxes: Vec<[f32; 4]>,
    /// Which target each instance is drawn into, and what it binds.
    pub layers: Layers,
}

/// Which entries a build writes, given an entry's index and its box from
/// the last frame that built it.
pub(super) type Keep<'a> = &'a dyn Fn(usize, [f32; 4]) -> bool;

impl Instances {
    pub fn new(side: u16, pages: usize) -> Self {
        Self {
            list: Vec::new(),
            bytes: Vec::new(),
            walk: Walk::default(),
            atlas: text::Atlas::new(side, pages),
            cache: Cache::default(),
            boxes: Vec::new(),
            layers: Layers::default(),
        }
    }

    /// Start a frame of builds.
    pub fn begin(&mut self, entries: usize) {
        self.cache.tick();
        self.walk.paths.begin();
        self.atlas.begin();
        self.atlas.flushed = false;
        self.boxes.resize(entries, build::EMPTY);
    }

    /// The instances of the entries of `paint` that `keep` names, into
    /// `self.list`, each kept entry's box updated; their count. Every entry
    /// is walked, so an unsupported one is refused whether kept or not. A
    /// frame the glyph atlas or the path texture cannot hold empties it and
    /// tries once more.
    pub fn build(
        &mut self,
        paint: &[Painted],
        transform: Affine,
        keep: Keep,
        welds: Option<Welds>,
    ) -> Result<usize, Error> {
        match self.list(paint, transform, keep, welds) {
            Err(Error::Unsupported {
                reason: Reason::GlyphBudget,
                ..
            }) if !self.atlas.flushed => {
                self.atlas.flush();
                self.build(paint, transform, keep, welds)
            }
            Err(Error::Unsupported {
                reason: Reason::PathBudget,
                ..
            }) if !self.walk.paths.flushed => {
                self.walk.paths.flush();
                self.build(paint, transform, keep, welds)
            }
            done => done,
        }
    }

    fn list(
        &mut self,
        paint: &[Painted],
        transform: Affine,
        keep: Keep,
        welds: Option<Welds>,
    ) -> Result<usize, Error> {
        self.list.clear();
        self.layers.begin(self.walk.size);
        self.walk.begin();
        let mut one: Instance = [[0.; 4]; SLOTS];
        for (index, p) in paint.iter().enumerate() {
            let refuse = |reason| Error::Unsupported { index, reason };
            let drawn = self.walk.entry(p, transform, &mut one).map_err(refuse)?;
            let start = self.list.len();
            match drawn {
                Drawn::Open { mix, opacity } => {
                    self.layers
                        .open(index, start as u32, mix, opacity)
                        .map_err(refuse)?;
                    continue;
                }
                Drawn::Close => {
                    let size = self.walk.size;
                    let (blend, b) = self
                        .layers
                        .close(index, &mut self.list, size)
                        .map_err(refuse)?;
                    self.boxes[blend] = b;
                    self.boxes[index] = b;
                    continue;
                }
                _ => {}
            }
            if self.layers.hidden() {
                self.boxes[index] = build::EMPTY;
                continue;
            }
            if !keep(index, self.boxes[index]) {
                continue;
            }
            let at = start as u32;
            match drawn {
                Drawn::Nothing | Drawn::Open { .. } | Drawn::Close => {}
                Drawn::One if p.layer == Layer::Mask => {
                    if !self.layers.mask() {
                        return Err(refuse(Reason::Layer));
                    }
                    self.layers.push(at, Bind::None, true);
                    self.list.push(one);
                    super::ramp::spans(&mut self.list, start, &p.paint);
                }
                Drawn::One => {
                    self.layers.push(at, Bind::None, false);
                    self.list.push(one);
                    super::ramp::spans(&mut self.list, start, &p.paint);
                }
                Drawn::Image(image) => {
                    self.layers.push(at, Bind::Image(image.clone()), false);
                    self.list.push(one);
                }
                Drawn::Host(key) => {
                    self.layers.push(at, Bind::Host(key), false);
                    self.list.push(one);
                }
                Drawn::Backdrop {
                    region,
                    scale,
                    sigma,
                } => {
                    let kind = Kind::Backdrop { scale, sigma };
                    let id = self
                        .layers
                        .backdrop(index, at, region, kind)
                        .map_err(refuse)?;
                    self.layers.charge_backdrop(id).map_err(refuse)?;
                    self.list.push(one);
                }
                Drawn::External => {
                    let w = welds.ok_or(refuse(Reason::External))?;
                    let e = w
                        .scene
                        .external_weld(&p.key)
                        .ok_or(refuse(Reason::External))?;
                    // No texture: a weld off the target, never drawn.
                    if let Some((id, texture, used)) = w.pool.texture(&p.key) {
                        let b = e.bounds();
                        let [uw, uh] = used.map(|v| v as f32);
                        one[2] = [b.x0, b.y0, b.x1, b.y1].map(|v| v as f32);
                        one[3] = [build::code(0., build::PAINT_IMAGE), 0., 0., 0.];
                        one[4] = [1., uw, uh, 0.];
                        let (w, h) = (b.width() as f32, b.height() as f32);
                        one[9] = [b.x0 as f32, b.y0 as f32, w / uw, h / uh];
                        self.layers.push(at, Bind::Weld(id, texture.clone()), false);
                        self.list.push(one);
                    }
                }
                Drawn::Text(text) => {
                    self.layers.push(at, Bind::None, false);
                    let xf = crate::placed(transform, p);
                    self.atlas
                        .draw(&mut self.cache, text, xf, &one, &mut self.list)
                        .map_err(|f| {
                            refuse(match f {
                                text::Failure::Colour => Reason::Text,
                                text::Failure::Full => Reason::GlyphBudget,
                            })
                        })?;
                    super::ramp::spans(&mut self.list, start, &p.paint);
                }
            }
            self.boxes[index] = self.list[start..]
                .iter()
                .map(build::device_box)
                .fold(build::EMPTY, build::union);
        }
        self.layers
            .finish()
            .map_err(|(index, reason)| Error::Unsupported { index, reason })?;
        Ok(self.list.len())
    }

    /// `self.list` as the bytes of the instance buffer.
    pub fn encode(&mut self) {
        let floats = self.list.as_flattened().as_flattened();
        self.bytes.resize(floats.len() * 4, 0);
        for (out, v) in self.bytes.as_chunks_mut::<4>().0.iter_mut().zip(floats) {
            *out = v.to_ne_bytes();
        }
    }
}

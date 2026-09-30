//! MUI's paint walk onto `vello_gpu` (Vello's sparse-strips GPU renderer,
//! ex `vello_hybrid`): paths are tiled on the CPU, strips are drawn with
//! plain vertex/fragment shaders, so it runs where classic Vello's compute
//! shaders cannot (WebGL2) as well as on WebGPU and native.
//!
//! `vello_gpu` lives on Vello git main with its own `vello_common`, while
//! mui-vello speaks the crates.io 0.2 one; kurbo and peniko are shared, so a
//! brush converts by matching and images by id.
use std::sync::{Arc, Weak};

use mui_vello::kurbo::{Affine, BezPath, Rect, Stroke};
use mui_vello::peniko::{self, Blob, Brush, FontData};
use mui_vello::{Canvas, PaintType};
use vello_common::paint as mui_paint;
use vello_gpu::{Image as GpuImage, ImageId, ImageSource as GpuSource};
use vello_gpu::{Renderer, Resources, Scene};

/// One `vello_gpu` renderer and the scene it re-records every frame.
pub struct Sparse {
    pub scene: Scene,
    renderer: Renderer,
    resources: Resources,
    depth: wgpu::TextureView,
    size: [u16; 2],
    /// Uploaded images, by the buffer MUI's `Image` points at.
    images: Vec<(Weak<[u8]>, ImageId)>,
    /// `FontData` per MUI font id: glyph caches key on the blob.
    fonts: Vec<(u64, FontData)>,
}

fn size16(size: [u32; 2]) -> Result<[u16; 2], String> {
    size.map(u16::try_from)
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .map(|v| [v[0].max(1), v[1].max(1)])
        .map_err(|_| format!("{}x{} is too large for vello_gpu", size[0], size[1]))
}

impl Sparse {
    pub fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        size: [u32; 2],
    ) -> Result<Self, String> {
        let [width, height] = size16(size)?;
        let (renderer, resources) = Renderer::new(
            device,
            &vello_gpu::RenderTargetConfig {
                format,
                width,
                height,
            },
        );
        Ok(Self {
            scene: Scene::new(width, height),
            renderer,
            resources,
            depth: depth(device, [width, height]),
            size: [width, height],
            images: Vec::new(),
            fonts: Vec::new(),
        })
    }

    pub fn resize(&mut self, device: &wgpu::Device, size: [u32; 2]) -> Result<(), String> {
        let s = size16(size)?;
        if s != self.size {
            self.size = s;
            self.scene = Scene::new(s[0], s[1]);
            self.depth = depth(device, s);
        }
        Ok(())
    }

    /// Upload any image of `images` not yet on the GPU, and forget the ones
    /// whose buffers are gone.
    pub fn upload<'a>(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        images: impl Iterator<Item = &'a mui_scene::Image>,
    ) {
        self.images.retain(|(k, _)| k.strong_count() > 0);
        let mut enc = None;
        for img in images {
            if self.find(&img.rgba).is_some() {
                continue;
            }
            let Some(px) = pixmap(img) else { continue };
            let enc = enc.get_or_insert_with(|| {
                device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default())
            });
            let id = self
                .renderer
                .upload_image(&mut self.resources, device, queue, enc, &px);
            self.images.push((Arc::downgrade(&img.rgba), id));
        }
        if let Some(enc) = enc {
            queue.submit([enc.finish()]);
        }
    }

    fn find(&self, rgba: &Arc<[u8]>) -> Option<ImageId> {
        self.images
            .iter()
            .find(|(k, _)| std::ptr::addr_eq(k.as_ptr(), Arc::as_ptr(rgba)))
            .map(|(_, id)| *id)
    }

    /// A canvas recording into the (already reset) scene.
    pub fn canvas(&mut self) -> SparseCanvas<'_> {
        SparseCanvas {
            scene: &mut self.scene,
            resources: &mut self.resources,
            images: &self.images,
            fonts: &mut self.fonts,
        }
    }

    /// Draw the recorded scene into `target`, clearing it first.
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &wgpu::TextureView,
    ) -> Result<(), String> {
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        self.renderer
            .render(
                &self.scene,
                &mut self.resources,
                device,
                queue,
                &mut enc,
                &vello_gpu::RenderSize {
                    width: self.size[0],
                    height: self.size[1],
                },
                target,
                Some(&self.depth),
                &vello_gpu::TextureBindings::new(),
                vello_gpu::TargetInit::Clear(vello_gpu::ClearSettings::default()),
            )
            .map_err(|e| e.to_string())?;
        queue.submit([enc.finish()]);
        Ok(())
    }
}

fn depth(device: &wgpu::Device, [width, height]: [u16; 2]) -> wgpu::TextureView {
    Renderer::create_depth_texture_view(device, &vello_gpu::RenderSize { width, height })
}

/// The pixmap `vello_gpu` uploads (premultiplied on the way in), or `None`
/// for an image whose buffer does not match its size.
fn pixmap(img: &mui_scene::Image) -> Option<vello_gpu::Pixmap> {
    let (w, h) = (
        u16::try_from(img.width).ok()?,
        u16::try_from(img.height).ok()?,
    );
    (img.rgba.len() == usize::from(w) * usize::from(h) * 4).then(|| {
        vello_gpu::Pixmap::from_parts(
            img.rgba.to_vec(),
            w,
            h,
            vello_gpu::PixelMetadata::new(peniko::ImageAlphaType::Alpha, true),
        )
    })
}

/// [`Canvas`] over a `vello_gpu` scene.
pub struct SparseCanvas<'a> {
    scene: &'a mut Scene,
    resources: &'a mut Resources,
    images: &'a [(Weak<[u8]>, ImageId)],
    fonts: &'a mut Vec<(u64, FontData)>,
}

impl Canvas for SparseCanvas<'_> {
    fn set_transform(&mut self, t: Affine) {
        self.scene.set_transform(t);
    }
    fn set_paint(&mut self, p: PaintType) {
        let p: vello_gpu::PaintType = match p {
            Brush::Solid(c) => Brush::Solid(c),
            Brush::Gradient(g) => Brush::Gradient(g),
            Brush::Image(i) => match i.image {
                mui_paint::ImageSource::OpaqueId {
                    id,
                    may_have_transparency,
                } => Brush::Image(GpuImage {
                    image: GpuSource::OpaqueId {
                        id: ImageId::new(id.as_u32()),
                        may_have_transparency,
                    },
                    sampler: i.sampler,
                }),
                // `image` below only hands out ids.
                mui_paint::ImageSource::Pixmap(_) => {
                    Brush::Solid(peniko::color::palette::css::TRANSPARENT)
                }
            },
        };
        self.scene.set_paint(p);
    }
    fn set_paint_transform(&mut self, t: Affine) {
        self.scene.set_paint_transform(t);
    }
    fn reset_paint_transform(&mut self) {
        self.scene.reset_paint_transform();
    }
    fn image(&mut self, img: &mui_scene::Image) -> Option<PaintType> {
        let id = self
            .images
            .iter()
            .find(|(k, _)| std::ptr::addr_eq(k.as_ptr(), Arc::as_ptr(&img.rgba)))?
            .1;
        Some(
            mui_paint::Image {
                image: mui_paint::ImageSource::OpaqueId {
                    id: mui_paint::ImageId::new(id.as_u32()),
                    may_have_transparency: true,
                },
                sampler: peniko::ImageSampler::default(),
            }
            .into(),
        )
    }
    fn set_stroke(&mut self, s: Stroke) {
        self.scene.set_stroke(s);
    }
    fn fill_path(&mut self, p: &BezPath) {
        self.scene.fill_path(p);
    }
    fn stroke_path(&mut self, p: &BezPath) {
        self.scene.stroke_path(p);
    }
    fn fill_blurred_rounded_rect(&mut self, r: &Rect, radius: f32, std_dev: f32, invert: bool) {
        self.scene
            .fill_blurred_rounded_rect(r, radius, std_dev, invert);
    }
    fn push_clip(&mut self, p: &BezPath) {
        self.scene.push_clip_path(p);
    }
    fn pop_clip(&mut self) {
        self.scene.pop_clip_path();
    }
    fn push_layer(&mut self, blend: peniko::BlendMode, opacity: f32) {
        self.scene
            .push_layer(None, Some(blend), Some(opacity), None, None);
    }
    fn pop_layer(&mut self) {
        self.scene.pop_layer();
    }
    fn glyphs(&mut self, text: &mui_scene::Text) {
        let mut start = 0;
        while start < text.glyphs.len() {
            let index = text.glyphs[start].font;
            let end = text.glyphs[start + 1..]
                .iter()
                .position(|g| g.font != index)
                .map_or(text.glyphs.len(), |o| start + 1 + o);
            let Some(face) = text.fonts.get(index) else {
                start = end;
                continue;
            };
            let key = face.id();
            let font = if let Some((_, f)) = self.fonts.iter().find(|(k, _)| *k == key) {
                f.clone()
            } else {
                // ponytail: fonts are never evicted; a project has a handful.
                let f = FontData::new(Blob::new(Arc::new(face.clone())), 0);
                self.fonts.push((key, f.clone()));
                f
            };
            let coords = text.font_coords.get(index).map_or(&[][..], |c| &c[..]);
            let (ox, oy) = (text.origin.x as f32, text.origin.y as f32);
            self.scene
                .glyph_run(self.resources, &font)
                .font_size(text.size)
                .hint(text.hint)
                .normalized_coords(coords)
                .fill_glyphs(text.glyphs[start..end].iter().map(|g| glifo::Glyph {
                    id: g.id,
                    x: ox + g.x,
                    y: oy + g.y,
                }));
            start = end;
        }
    }
}

//! MUI's paint walk onto `vello_gpu` (Vello's sparse-strips GPU renderer,
//! ex `vello_hybrid`): paths are tiled on the CPU, strips are drawn with
//! plain vertex/fragment shaders, so it runs where classic Vello's compute
//! shaders cannot (WebGL2) as well as on WebGPU and native.
//!
//! `vello_gpu` lives on Vello git main with its own `vello_common`, while
//! mui-vello speaks the crates.io 0.2 one; kurbo and peniko are shared, so a
//! brush converts by matching and images by id.
//!
//! MUI's backdrop blur splits the frame (see [`SparseCanvas::backdrop`]):
//! what is recorded so far renders into a texture, the recording restarts
//! on that texture, and the blur reads it (and whatever the caller says lies
//! beneath the whole paint) through a `vello_gpu` filter layer.
use std::sync::{Arc, Weak};

use mui_vello::kurbo::{Affine, BezPath, Rect, Stroke};
use mui_vello::peniko::{self, Blob, Brush, FontData};
use mui_vello::{Canvas, PaintType};
use vello_common::paint as mui_paint;
use vello_gpu::filter_effects::{EdgeMode, Filter, FilterPrimitive};
use vello_gpu::{Image as GpuImage, ImageId, ImageSource as GpuSource};
use vello_gpu::{Renderer, Resources, Scene, TextureBindings, TextureId};

/// The external textures a scene reads: what lies beneath the paint, and the
/// two a backdrop split renders into in turn.
const BENEATH: TextureId = TextureId(1);
const BELOW: [TextureId; 2] = [TextureId(2), TextureId(3)];

/// One `vello_gpu` renderer and the scene it re-records every frame.
pub struct Sparse {
    pub scene: Scene,
    renderer: Renderer,
    resources: Resources,
    depth: wgpu::TextureView,
    size: [u16; 2],
    format: wgpu::TextureFormat,
    /// What backdrop splits render into, made by the first; and the
    /// external textures bound for this frame.
    below: Option<[wgpu::TextureView; 2]>,
    bindings: TextureBindings,
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
            format,
            below: None,
            bindings: TextureBindings::new(),
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
            self.below = None;
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

    /// A canvas recording into the (already reset) scene; `beneath`, if
    /// any, is what the target will be composited over, which a backdrop
    /// blur sees too.
    pub fn canvas<'a>(
        &'a mut self,
        device: &'a wgpu::Device,
        queue: &'a wgpu::Queue,
        beneath: Option<&wgpu::TextureView>,
    ) -> SparseCanvas<'a> {
        match beneath {
            Some(v) => self.bindings.insert(BENEATH, v.clone()),
            None => drop(self.bindings.remove(BENEATH)),
        }
        SparseCanvas {
            device,
            queue,
            beneath: beneath.is_some(),
            s: self,
            open: Vec::new(),
            transform: Affine::IDENTITY,
            next: 0,
            failed: None,
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
                &self.bindings,
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
    s: &'a mut Sparse,
    device: &'a wgpu::Device,
    queue: &'a wgpu::Queue,
    beneath: bool,
    /// The clips and layers open now, with the transform each was pushed
    /// under: a split closes them to render and opens them again after.
    open: Vec<(Open, Affine)>,
    transform: Affine,
    /// Which of [`BELOW`] the next split renders into.
    next: usize,
    /// A split that could not render; the paint goes on, unblurred.
    pub failed: Option<String>,
}

#[derive(Clone)]
enum Open {
    Clip(BezPath),
    Layer(
        Option<BezPath>,
        Option<peniko::BlendMode>,
        Option<f32>,
        Option<Filter>,
    ),
}

/// The one filter MUI asks for, as mui-vello's: edges repeated, so a
/// backdrop is not darkened at the frame's edge.
fn blur(std_dev: f32) -> Filter {
    Filter::from_primitive(FilterPrimitive::GaussianBlur {
        std_deviation: std_dev,
        edge_mode: EdgeMode::Duplicate,
    })
}

/// External texture `id`, `size` texels, as a paint placed 1:1 on the
/// device's pixels.
fn external(id: TextureId, [w, h]: [u16; 2]) -> vello_gpu::PaintType {
    Brush::Image(GpuImage {
        image: GpuSource::ExternalTexture {
            id,
            source_region: vello_gpu::RectU16::new(0, 0, w, h),
            may_have_transparency: true,
        },
        sampler: peniko::ImageSampler {
            quality: peniko::ImageQuality::Low,
            ..Default::default()
        },
    })
}

impl SparseCanvas<'_> {
    fn reopen(&mut self, o: &Open, t: Affine) {
        let scene = &mut self.s.scene;
        scene.set_transform(t);
        match o.clone() {
            Open::Clip(p) => scene.push_clip_path(&p),
            Open::Layer(clip, blend, opacity, filter) => {
                scene.push_layer(clip.as_ref(), blend, opacity, None, filter);
            }
        }
    }

    fn push(&mut self, o: Open) {
        self.reopen(&o, self.transform);
        self.s.scene.set_transform(self.transform);
        self.open.push((o, self.transform));
    }

    fn pop(&mut self) {
        match self.open.pop() {
            Some((Open::Clip(_), _)) => self.s.scene.pop_clip_path(),
            Some((Open::Layer(..), _)) => self.s.scene.pop_layer(),
            // Unbalanced, as Vello takes it: a pop of nothing is a layer's.
            None => self.s.scene.pop_layer(),
        }
    }
}

impl Canvas for SparseCanvas<'_> {
    fn set_transform(&mut self, t: Affine) {
        self.transform = t;
        self.s.scene.set_transform(t);
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
        self.s.scene.set_paint(p);
    }
    fn set_paint_transform(&mut self, t: Affine) {
        self.s.scene.set_paint_transform(t);
    }
    fn reset_paint_transform(&mut self) {
        self.s.scene.reset_paint_transform();
    }
    fn image(&mut self, img: &mui_scene::Image) -> Option<PaintType> {
        let id = self
            .s
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
        self.s.scene.set_stroke(s);
    }
    fn fill_path(&mut self, p: &BezPath) {
        self.s.scene.fill_path(p);
    }
    fn stroke_path(&mut self, p: &BezPath) {
        self.s.scene.stroke_path(p);
    }
    fn fill_blurred_rounded_rect(&mut self, r: &Rect, radius: f32, std_dev: f32, invert: bool) {
        self.s
            .scene
            .fill_blurred_rounded_rect(r, radius, std_dev, invert);
    }
    fn push_clip(&mut self, p: &BezPath) {
        self.push(Open::Clip(p.clone()));
    }
    fn pop_clip(&mut self) {
        self.pop();
    }
    fn push_layer(&mut self, blend: peniko::BlendMode, opacity: f32) {
        self.push(Open::Layer(None, Some(blend), Some(opacity), None));
    }
    fn pop_layer(&mut self) {
        self.pop();
    }
    fn push_blur(&mut self, clip: &BezPath, std_dev: f32) -> bool {
        self.push(Open::Layer(Some(clip.clone()), None, None, None));
        self.push(Open::Layer(None, None, None, Some(blur(std_dev))));
        true
    }
    /// The split: close what is open, render the recording into one of
    /// [`BELOW`], restart the recording on that picture, open again what
    /// was open, then draw `beneath` and the picture through a blur layer
    /// clipped to the node. An open layer's opacity has already applied
    /// to what it drew before the split, and applies again, apart, to the
    /// rest: the same unless the two overlap inside it.
    fn backdrop(&mut self, clip: &BezPath, std_dev: f32, reach: Rect) -> bool {
        let [w, h] = self.s.size;
        if self.s.below.is_none() {
            let make = || {
                self.device
                    .create_texture(&wgpu::TextureDescriptor {
                        label: Some("mui-cut backdrop"),
                        size: wgpu::Extent3d {
                            width: u32::from(w),
                            height: u32::from(h),
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: self.s.format,
                        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                            | wgpu::TextureUsages::TEXTURE_BINDING,
                        view_formats: &[],
                    })
                    .create_view(&wgpu::TextureViewDescriptor::default())
            };
            self.s.below = Some([make(), make()]);
        }
        let k = self.next;
        self.next ^= 1;
        let view = self.s.below.as_ref().expect("made above")[k].clone();
        for (o, _) in self.open.iter().rev() {
            match o {
                Open::Clip(_) => self.s.scene.pop_clip_path(),
                Open::Layer(..) => self.s.scene.pop_layer(),
            }
        }
        if let Err(e) = self.s.render(self.device, self.queue, &view) {
            self.failed = Some(e);
        }
        self.s.bindings.insert(BELOW[k], view);
        let full = Rect::new(0., 0., f64::from(w), f64::from(h));
        let scene = &mut self.s.scene;
        scene.reset();
        scene.set_transform(Affine::IDENTITY);
        scene.reset_paint_transform();
        scene.set_paint(external(BELOW[k], [w, h]));
        scene.fill_rect(&full);
        for (o, t) in self.open.clone() {
            self.reopen(&o, t);
        }
        let scene = &mut self.s.scene;
        scene.set_transform(self.transform);
        scene.push_clip_layer(clip);
        scene.push_filter_layer(blur(std_dev));
        scene.set_transform(Affine::IDENTITY);
        let reach = reach.intersect(full).expand();
        if self.beneath {
            scene.set_paint(external(BENEATH, [w, h]));
            scene.fill_rect(&reach);
        }
        scene.set_paint(external(BELOW[k], [w, h]));
        scene.fill_rect(&reach);
        scene.pop_layer();
        scene.pop_layer();
        scene.set_transform(self.transform);
        true
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
            let font = if let Some((_, f)) = self.s.fonts.iter().find(|(k, _)| *k == key) {
                f.clone()
            } else {
                // ponytail: fonts are never evicted; a project has a handful.
                let f = FontData::new(Blob::new(Arc::new(face.clone())), 0);
                self.s.fonts.push((key, f.clone()));
                f
            };
            let coords = text.font_coords.get(index).map_or(&[][..], |c| &c[..]);
            let (ox, oy) = (text.origin.x as f32, text.origin.y as f32);
            self.s
                .scene
                .glyph_run(&mut self.s.resources, &font)
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

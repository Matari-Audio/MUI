use base64::Engine;
type Raster = Option<([usize; 4], String)>;
struct CachedFragment {
    key: (u16, u16, f64),
    paint: Vec<mui_scene::Painted>,
    raster: Raster,
}
pub fn capture_frame(
    scene: &mui_scene::ResolvedScene,
    width: u16,
    height: u16,
    scale: f64,
    roots: &[String],
) -> Result<serde_json::Value, String> {
    capture_cached(scene, width, height, scale, roots, &mut Vec::new())
}
fn raster(
    scene: &mui_scene::ResolvedScene,
    width: u16,
    height: u16,
    scale: f64,
) -> Result<Raster, String> {
    let mut ctx = RenderContext::new(width, height);
    let mut res = Resources::default();
    mui_vello::paint(
        &mut mui_vello::Cpu {
            ctx: &mut ctx,
            resources: &mut res,
            cache: &mut mui_vello::Cache::default(),
        },
        scene,
        mui_vello::kurbo::Affine::scale(scale),
    )
    .map_err(|e| e.to_string())?;
    ctx.flush();
    let mut pix = Pixmap::new(width, height);
    ctx.render(&mut pix, &mut res);
    let pixels = pix.take_unpremultiplied();
    let (mut x0, mut y0, mut x1, mut y1) = (usize::from(width), usize::from(height), 0, 0);
    for (i, _) in pixels.iter().enumerate().filter(|(_, p)| p.a != 0) {
        let (x, y) = (i % usize::from(width), i / usize::from(width));
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x + 1);
        y1 = y1.max(y + 1);
    }
    if x1 == 0 {
        return Ok(None);
    }
    let rgba: Vec<u8> = (y0..y1)
        .flat_map(|y| {
            pixels[y * usize::from(width) + x0..y * usize::from(width) + x1]
                .iter()
                .flat_map(|p| [p.r, p.g, p.b, p.a])
        })
        .collect();
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, (x1 - x0) as u32, (y1 - y0) as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .and_then(|mut w| w.write_image_data(&rgba))
        .map_err(|e| e.to_string())?;
    Ok(Some((
        [x0, y0, x1, y1],
        base64::engine::general_purpose::STANDARD.encode(bytes),
    )))
}
use mui_vello::vello_cpu::{Pixmap, RenderContext, Resources};
fn capture_cached(
    scene: &mui_scene::ResolvedScene,
    width: u16,
    height: u16,
    scale: f64,
    roots: &[String],
    cache: &mut Vec<CachedFragment>,
) -> Result<serde_json::Value, String> {
    if width == 0 || height == 0 || !scale.is_finite() || scale <= 0. {
        return Err("capture dimensions and scale must be positive and finite".into());
    }
    let mut images = serde_json::Map::new();
    let refs: Vec<&str> = roots.iter().map(String::as_str).collect();
    let surfaces: Vec<_> = scene.surfaces().map(|s| serde_json::json!({"id":s.key.as_ref(),"parent":s.parent.as_deref(),"frame":[s.frame.x,s.frame.y,s.frame.size.width,s.frame.size.height]})).collect();
    let mut layers = Vec::new();
    let mut fragment_count = 0;
    for (index, fragment) in scene
        .capture_layers(&refs)
        .map_err(|e| e.to_string())?
        .into_iter()
        .enumerate()
    {
        fragment_count = index + 1;
        let id = fragment.part.as_ref();
        let isolated = fragment.scene;
        let key = (width, height, scale);
        let cached = cache
            .get(index)
            .filter(|c| c.key == key && c.paint == isolated.paint);
        let data = if let Some(c) = cached {
            c.raster.clone()
        } else {
            raster(&isolated, width, height, scale)?
        };
        let entry = CachedFragment {
            key,
            paint: isolated.paint,
            raster: data.clone(),
        };
        if index < cache.len() {
            cache[index] = entry;
        } else {
            cache.push(entry);
        }
        let Some(([x0, y0, x1, y1], data)) = data else {
            continue;
        };
        let name = format!("layer-{index:02}.png");
        images.insert(name.clone(), serde_json::Value::String(data));
        let origin = id.and_then(|id| scene.surface(id)).map_or(
            [
                f64::from(width) / scale / 2.,
                f64::from(height) / scale / 2.,
            ],
            |s| {
                [
                    s.frame.x + s.frame.size.width / 2.,
                    s.frame.y + s.frame.size.height / 2.,
                ]
            },
        );
        layers.push(serde_json::json!({"id":format!("fragment-{index}"),"group":id.map_or("background",String::as_str),"origin":origin,"src":name,"rect":[x0 as f64/scale,y0 as f64/scale,(x1-x0) as f64/scale,(y1-y0) as f64/scale]}));
    }
    cache.truncate(fragment_count);
    let manifest = serde_json::json!({"version":1,"width":f64::from(width)/scale,"height":f64::from(height)/scale,"scale":scale,"layers":layers,"groups":roots,"surfaces":surfaces});
    Ok(serde_json::json!({"scene":manifest,"images":images}))
}

/// Offline capture uses exactly the same pixels as the live in-memory stream.
pub fn capture(
    scene: &mui_scene::ResolvedScene,
    width: u16,
    height: u16,
    scale: f64,
    directory: &std::path::Path,
    roots: &[String],
) -> Result<(), String> {
    use base64::Engine;
    let frame = capture_frame(scene, width, height, scale, roots)?;
    std::fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    for (name, data) in frame["images"].as_object().unwrap() {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data.as_str().unwrap())
            .map_err(|e| e.to_string())?;
        std::fs::write(directory.join(name), bytes).map_err(|e| e.to_string())?;
    }
    std::fs::write(
        directory.join("scene.json"),
        serde_json::to_vec_pretty(&frame["scene"]).unwrap(),
    )
    .map_err(|e| e.to_string())
}

/// Content-addressed, changed-texture transport. Only the current frame's
/// textures are retained; a static surface is not sent again on every tick.
#[derive(Default)]
pub struct CaptureStream {
    previous: std::collections::HashSet<String>,
    cache: Vec<CachedFragment>,
}
impl CaptureStream {
    pub fn frame(
        &mut self,
        scene: &mui_scene::ResolvedScene,
        width: u16,
        height: u16,
        scale: f64,
        roots: &[String],
    ) -> Result<serde_json::Value, String> {
        use std::hash::{DefaultHasher, Hash, Hasher};
        let mut frame = capture_cached(scene, width, height, scale, roots, &mut self.cache)?;
        let source = frame["images"].take();
        let mut images = serde_json::Map::new();
        let mut current = std::collections::HashSet::new();
        for layer in frame["scene"]["layers"].as_array_mut().unwrap() {
            let data = source[layer["src"].as_str().unwrap()].as_str().unwrap();
            // ponytail: 64-bit std hash, not a digest; a collision in one
            // session would reuse a stale texture. Names never leave the session.
            let mut hash = DefaultHasher::new();
            data.hash(&mut hash);
            let name = format!("{:016x}-{}.png", hash.finish(), data.len());
            if !self.previous.contains(&name) {
                images.insert(name.clone(), serde_json::Value::String(data.into()));
            }
            current.insert(name.clone());
            layer["src"] = serde_json::Value::String(name);
        }
        self.previous = current;
        frame["images"] = serde_json::Value::Object(images);
        Ok(frame)
    }
}

/// Discover a useful non-overlapping partition without plugin-specific names.
/// Every native surface remains in the manifest for deeper selection. Large
/// container surfaces are traversed until a panel-sized named surface is found.
pub fn discover_parts(scene: &mui_scene::ResolvedScene, width: f64, height: f64) -> Vec<String> {
    let surfaces: Vec<_> = scene
        .surfaces()
        .filter(|s| !s.key.starts_with('/'))
        .collect();
    let candidates: Vec<_> = surfaces
        .iter()
        .filter(|s| {
            s.parent.is_some()
                && s.frame.size.width >= 50.
                && s.frame.size.height >= 35.
                && s.frame.size.width * s.frame.size.height <= width * height * 0.3
        })
        .collect();
    candidates
        .iter()
        .filter(|s| {
            let mut parent = s.parent.as_deref();
            while let Some(id) = parent {
                if candidates.iter().any(|p| p.key.as_ref() == id) {
                    return false;
                }
                parent = surfaces
                    .iter()
                    .find(|p| p.key.as_ref() == id)
                    .and_then(|p| p.parent.as_deref());
            }
            true
        })
        .map(|s| s.key.to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mui_scene::prelude::*;
    #[test]
    fn automatic_parts_and_pixel_cache_follow_real_scene_changes() {
        let make = |width| {
            resolve_scene(&SceneSpec::new(
                column([leaf(width, 60.).fill(Ink).id("panel")])
                    .size(300., 200.)
                    .id("root"),
            ))
            .unwrap()
        };
        let a = make(100.);
        let roots = discover_parts(&a, 300., 200.);
        assert_eq!(roots, vec!["panel"]);
        let mut stream = CaptureStream::default();
        let first = stream.frame(&a, 300, 200, 1., &roots).unwrap();
        assert!(!first["images"].as_object().unwrap().is_empty());
        let second = stream.frame(&a, 300, 200, 1., &roots).unwrap();
        assert!(second["images"].as_object().unwrap().is_empty());
        assert_eq!(first["scene"], second["scene"]);
        let changed = stream.frame(&make(140.), 300, 200, 1., &roots).unwrap();
        assert!(!changed["images"].as_object().unwrap().is_empty());
        assert_ne!(first["scene"]["layers"], changed["scene"]["layers"]);
    }
}

use super::*;
use mui_geometry::Path;
use mui_scene::{Image, Text};

/// `Image`'s fields are public, so a caller can skip `Image::rgba` and
/// hand over a buffer that does not match the size. Vello's `Pixmap`
/// asserts on that; the paint falls back instead.
#[test]
fn an_image_whose_buffer_does_not_match_its_size_falls_back() {
    let image = Image {
        width: 4,
        height: 4,
        rgba: Arc::from(&[0u8, 0, 0, 255][..]),
        texture: None,
    };
    assert!(premultiply(&image).is_none());
}

/// A font unused for `FONT_FRAMES` frames lets go of its `FontData`; one
/// drawn every frame keeps it, blob id and all.
#[test]
fn the_font_cache_preserves_collection_face_indices_and_identity() {
    // Two faces may share one sfnt and its tables inside a TTC. Rebase
    // Hack's table offsets from the standalone font into that container.
    let font = epaint_default_fonts::HACK_REGULAR;
    let mut collection = Vec::from(&b"ttcf\0\x01\0\0\0\0\0\x02\0\0\0\x14\0\0\0\x14"[..]);
    collection.extend_from_slice(font);
    let tables = u16::from_be_bytes(font[4..6].try_into().unwrap());
    for table in 0..usize::from(tables) {
        let offset = 20 + 12 + table * 16 + 8;
        let value = u32::from_be_bytes(collection[offset..offset + 4].try_into().unwrap());
        collection[offset..offset + 4].copy_from_slice(&(value + 20).to_be_bytes());
    }
    let bytes: Arc<[u8]> = collection.into();
    let first = mui_scene::Font::from_index(Arc::clone(&bytes), 0).unwrap();
    let second = mui_scene::Font::from_index(bytes, 1).unwrap();
    let mut cache = Cache::default();
    let (a, _) = cache.font(&first);
    let (b, _) = cache.font(&second);
    assert_eq!((a.index, b.index), (0, 1));
    assert_ne!(first.id(), second.id());
    assert_ne!(a.data.id(), b.data.id());
    assert_eq!(cache.fonts.len(), 2);
    assert_eq!(cache.font(&second).0.index, 1);
    assert_eq!(cache.font(&second).0.data.id(), b.data.id());
}

#[test]
fn an_unused_font_ages_out() {
    let font = |bytes| mui_scene::Font::new(bytes).unwrap();
    let (kept, dropped) = (
        font(epaint_default_fonts::HACK_REGULAR),
        font(epaint_default_fonts::UBUNTU_LIGHT),
    );
    let mut cache = Cache::default();
    cache.tick();
    let (first, warm) = cache.font(&kept);
    assert!(!warm && !cache.font(&kept).1, "warm in its first frame");
    cache.font(&dropped);
    for _ in 0..FONT_FRAMES {
        cache.tick();
        assert!(cache.font(&kept).1, "cold after its first frame");
    }
    assert_eq!(cache.fonts.len(), 2, "aged out early");
    cache.tick();
    assert_eq!(cache.fonts.len(), 1, "the idle font is still held");
    assert_eq!(cache.font(&kept).0.data.id(), first.data.id());
}

/// A gradient-filled label still gets a box to fit the gradient to,
/// although its path is empty.
#[test]
fn a_text_layer_takes_its_brush_box_from_the_run() {
    let mut p = Painted {
        key: "t".into(),
        layer: Layer::Text,
        path: Path::default().into(),
        paint: Paint::Solid(mui_scene::Color::oklch(0.5, 0., 0.)),
        rect: None,
        offset: mui_geometry::Point::default(),
        transform: Affine::IDENTITY,
        width: 0.,
        cap: crate::kurbo::Cap::Round,
        join: crate::kurbo::Join::Round,
        blur: 0.,
        text: Some(Text {
            fonts: Arc::from([mui_scene::Font::new(epaint_default_fonts::HACK_REGULAR).unwrap()]),
            size: 16.,
            origin: mui_geometry::Point::new(10., 30.),
            glyphs: Arc::from(
                &[
                    mui_scene::TextGlyph {
                        id: 1,
                        x: 0.,
                        y: 0.,
                        font: 0,
                    },
                    mui_scene::TextGlyph {
                        id: 2,
                        x: 12.,
                        y: 0.,
                        font: 0,
                    },
                ][..],
            ),
            axes: mui_scene::Axes::default(),
            hint: true,
            font_coords: Arc::from(&[][..]),
        }),
    };
    let b = paint_box(&p, &BezPath::new());
    assert!(b.width() > 0. && b.height() > 0., "{b:?}");
    assert_eq!((b.x0, b.y1), (10., 30.));
    p.text = None;
    assert_eq!(paint_box(&p, &BezPath::new()), Rect::ZERO);
}

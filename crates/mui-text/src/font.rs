use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use harfrust::{ShapePlan, ShaperData};
use skrifa::{FontRef, MetadataProvider as _};

use crate::Error;

/// One variable-font axis position, e.g. `("FILL", 1.0)` or `("wght", 500.0)`.
/// Unknown tags are ignored by the face; out-of-range values are clamped to the
/// axis bounds, so a caller cannot produce an outline the font does not define.
pub type Axis<'a> = (&'a str, f32);

/// How heavy a run is drawn, as the `wght` axis position every variable font
/// names the same way: 400 regular, 700 bold.
///
/// A newtype rather than an enum because the axis is continuous -- a display
/// face that looks right at 520 should be able to say so -- and the four
/// constants cover what a UI usually asks for.
///
/// ponytail: a static face has no `wght` axis, so it draws at its one
/// weight; nothing here synthesises a bold by smearing outlines. Ship a
/// variable face, or a second blob for the bold, if the difference matters.
///
/// ```
/// use mui_text::Weight;
/// assert_eq!(Weight::BOLD.axis(), ("wght", 700.0));
/// assert_eq!(Weight::default(), Weight::REGULAR);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Weight(u16);
impl Weight {
    pub const REGULAR: Self = Self(400);
    pub const MEDIUM: Self = Self(500);
    pub const SEMIBOLD: Self = Self(600);
    pub const BOLD: Self = Self(700);

    /// Any position on the axis. The face clamps it to the range it declares.
    ///
    /// ```
    /// use mui_text::Weight;
    /// assert_eq!(Weight::new(520).value(), 520);
    /// ```
    pub const fn new(wght: u16) -> Self {
        Self(wght)
    }
    /// The number, for a caller that stores or shows it.
    pub const fn value(self) -> u16 {
        self.0
    }
    /// This weight as the axis setting [`text_run`] takes.
    pub fn axis(self) -> Axis<'static> {
        ("wght", f32::from(self.0))
    }
}
impl Default for Weight {
    fn default() -> Self {
        Self::REGULAR
    }
}

/// An owned axis setting list: what an element stores and a cache keys on.
///
/// Tags are kept sorted and values as `f32` bits, so the same settings in any
/// order compare and hash equal. Setting a tag again replaces it. A tag that
/// is not four ASCII bytes, or a value that is not finite, is ignored: no
/// font can declare the one and no outline exists at the other.
///
/// Always set the same tags every frame of an animation: an omitted axis
/// sits at its default, so `FILL` present then absent is a jump to 0.
///
/// ```
/// use mui_text::{Axes, Weight};
/// let a = Axes::new().with("wght", 700.).with("FILL", 1.);
/// let b = Axes::from(Weight::BOLD).with("FILL", 1.);
/// assert_eq!(a, b);
/// assert_eq!(a.get("FILL"), Some(1.));
/// assert_eq!(a.to_vec(), [("FILL", 1.), ("wght", 700.)]);
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Axes(Vec<([u8; 4], u32)>);
impl Axes {
    pub const fn new() -> Self {
        Self(Vec::new())
    }
    pub fn with(mut self, tag: &str, value: f32) -> Self {
        self.set(tag, value);
        self
    }
    pub fn set(&mut self, tag: &str, value: f32) {
        let Ok(tag) = <[u8; 4]>::try_from(tag.as_bytes()) else {
            return;
        };
        if !tag.is_ascii() || !value.is_finite() {
            return;
        }
        match self.0.binary_search_by_key(&tag, |a| a.0) {
            Ok(i) => self.0[i].1 = value.to_bits(),
            Err(i) => self.0.insert(i, (tag, value.to_bits())),
        }
    }
    pub fn get(&self, tag: &str) -> Option<f32> {
        let tag = <[u8; 4]>::try_from(tag.as_bytes()).ok()?;
        self.0
            .binary_search_by_key(&tag, |a| a.0)
            .ok()
            .map(|i| f32::from_bits(self.0[i].1))
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn iter(&self) -> impl Iterator<Item = Axis<'_>> + '_ {
        // ASCII was checked on the way in, so this never fails.
        self.0
            .iter()
            .map(|(t, v)| (std::str::from_utf8(t).unwrap_or(""), f32::from_bits(*v)))
    }
    /// The borrowed form every measuring function here takes.
    pub fn to_vec(&self) -> Vec<Axis<'_>> {
        self.iter().collect()
    }
}
impl From<Weight> for Axes {
    fn from(w: Weight) -> Self {
        Self::new().with("wght", f32::from(w.value()))
    }
}

/// A font face: its bytes, an identity, and what shaping it costs to set up,
/// built on first use and shared by every clone.
///
/// Equality and hashing are by identity, never bytes: every [`Font::new`] is a
/// new face. A cache keyed on one cannot serve glyphs from a buffer that was
/// freed and reallocated at the same address -- and a `Font` built afresh
/// every frame misses every such cache, so build one per face and clone it,
/// which is an `Arc` bump.
///
/// ```
/// use mui_text::Font;
/// let hack = Font::new(epaint_default_fonts::HACK_REGULAR).unwrap();
/// assert_eq!(hack, hack.clone());
/// assert_ne!(hack, Font::new(epaint_default_fonts::HACK_REGULAR).unwrap());
/// assert!(Font::new(&b"not a font"[..]).is_err());
/// ```
#[derive(Clone)]
pub struct Font(pub(crate) Arc<FontData>);

pub(crate) struct FontData {
    pub(crate) id: u64,
    pub(crate) bytes: Arc<[u8]>,
    /// harfrust's lookup accelerators for the face: the expensive part of
    /// setting a shaper up, and the same at every size and axis position.
    pub(crate) shaper: OnceLock<ShaperData>,
    /// Compiled feature maps, one per (script, direction, feature-variation)
    /// the face has shaped. Reusing one took Inter caret_x from 18.3 to 15.9 us.
    /// LRU, capped at `shape::PLAN_CAP`.
    pub(crate) plans: Mutex<Vec<Arc<ShapePlan>>>,
}

impl Font {
    /// Parse `bytes` as one font face. Bytes no parser accepts are an error
    /// here, once, rather than on every run shaped with them.
    pub fn new(bytes: impl Into<Arc<[u8]>>) -> Result<Self, Error> {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        let bytes = bytes.into();
        FontRef::new(&bytes).map_err(Error::Font)?;
        Ok(Self(Arc::new(FontData {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            bytes,
            shaper: OnceLock::new(),
            plans: Mutex::new(Vec::new()),
        })))
    }
    /// Unique to one [`Font::new`] for the life of the process and shared by
    /// its clones: the key for any cache of what this face draws.
    pub fn id(&self) -> u64 {
        self.0.id
    }
    pub(crate) fn font_ref(&self) -> Result<FontRef<'_>, Error> {
        // Validated in `new`; the table directory is all this re-reads.
        FontRef::new(&self.0.bytes).map_err(Error::Font)
    }
}
impl AsRef<[u8]> for Font {
    fn as_ref(&self) -> &[u8] {
        &self.0.bytes
    }
}
impl PartialEq for Font {
    fn eq(&self, other: &Self) -> bool {
        self.0.id == other.0.id
    }
}
impl Eq for Font {}
impl Hash for Font {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.id.hash(state);
    }
}
impl std::fmt::Debug for Font {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Font")
            .field("id", &self.0.id)
            .field("len", &self.0.bytes.len())
            .finish()
    }
}

/// One variation axis the face actually declares, with the range it accepts.
/// A UI needs this to offer a slider that cannot leave the design space.
#[derive(Debug, Clone, PartialEq)]
pub struct AxisInfo {
    pub tag: String,
    pub min: f32,
    pub default: f32,
    pub max: f32,
    /// fvar `HIDDEN_AXIS`: the font asks UIs not to expose this axis directly
    /// (an `opsz` it sets itself, an internal `XTRA`). Still settable.
    pub hidden: bool,
}

/// The variation axes of a face, in the font's own order. Empty for a static
/// font -- which is the honest answer, not an error.
pub fn axes(font: &Font) -> Vec<AxisInfo> {
    let Ok(font) = font.font_ref() else {
        return Vec::new();
    };
    font.axes()
        .iter()
        .map(|a| AxisInfo {
            tag: a.tag().to_string(),
            min: a.min_value(),
            default: a.default_value(),
            max: a.max_value(),
            hidden: a.is_hidden(),
        })
        .collect()
}

/// The face's normalized coordinates for an axis setting, one per axis it
/// declares, in the font's own order.
///
/// A renderer that draws cached glyph outlines instead of the path
/// [`text_run`] hands back needs these, or it paints the default instance
/// while layout measured the varied one. The numbers are F2Dot14 bits --
/// what every glyph cache keys its variations on. `size_px` matters only to
/// a face with an `opsz` axis, which follows the em size unless set.
///
/// ```
/// # let font = mui_text::Font::new(epaint_default_fonts::HACK_REGULAR).unwrap();
/// // A static face declares no axes, so there is nothing to vary.
/// assert!(mui_text::normalized_coords(&font, 16., &[mui_text::Weight::BOLD.axis()])
///     .unwrap()
///     .is_empty());
/// ```
pub fn normalized_coords(font: &Font, size_px: f64, axes: &[Axis<'_>]) -> Result<Vec<i16>, Error> {
    let font = font.font_ref()?;
    Ok(location(&font, size_px, axes)
        .coords()
        .iter()
        .map(|c| c.to_bits())
        .collect())
}

/// The one place an axis setting becomes a position in the design space, so
/// outlines, metrics, shaping and a renderer's cache key all agree.
///
/// `opsz` follows the em size unless the caller sets it -- CSS
/// `font-optical-sizing: auto`, in CSS-pixel semantics; the face clamps it
/// to the range it declares, so nothing is asked for that does not exist.
///
/// Coordinates are then rounded to 1/128 of a half-axis. A spring never
/// lands on the same float twice, so without this every frame of a `FILL`
/// tween is a new glyph-cache instance; with it a hover in and out reuses
/// at most 128 per axis, and the extremes and the default round to
/// themselves. Nobody can see 1/128 of an axis.
pub(crate) fn location(
    font: &FontRef<'_>,
    size_px: f64,
    axes: &[Axis<'_>],
) -> skrifa::instance::Location {
    let all = font.axes();
    let auto_opsz =
        all.get_by_tag(skrifa::Tag::new(b"opsz")).is_some() && !axes.iter().any(|a| a.0 == "opsz");
    let mut location = if auto_opsz {
        all.location(axes.iter().copied().chain([("opsz", size_px as f32)]))
    } else {
        all.location(axes.iter().copied())
    };
    for c in location.coords_mut() {
        *c = skrifa::instance::NormalizedCoord::from_bits(
            ((i32::from(c.to_bits()) + 64).div_euclid(128) * 128) as i16,
        );
    }
    location
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_fonts::{hack, symbols};

    #[test]
    fn opsz_follows_the_size_unless_set_and_coords_are_quantised() {
        let at24 = normalized_coords(&symbols(), 24., &[]).unwrap();
        let at48 = normalized_coords(&symbols(), 48., &[]).unwrap();
        let at480 = normalized_coords(&symbols(), 480., &[]).unwrap();
        // Axis order is FILL, GRAD, opsz, wght; opsz default is 24, max 48.
        assert_eq!(at24[2], 0);
        assert_eq!(at48[2], 16384);
        assert_eq!(at480, at48, "clamped to the font's range, not reset");
        let pinned = normalized_coords(&symbols(), 480., &[("opsz", 24.)]).unwrap();
        assert_eq!(pinned[2], 0, "an explicit opsz wins");
        let a = normalized_coords(&symbols(), 24., &[("FILL", 0.5001)]).unwrap();
        let b = normalized_coords(&symbols(), 24., &[("FILL", 0.5019)]).unwrap();
        assert_eq!(a, b, "a spring's neighbouring floats share a cache key");
        assert_eq!(a[0] % 128, 0);
        assert_eq!(
            normalized_coords(&symbols(), 24., &[("FILL", 1.)]).unwrap()[0],
            16384
        );
        assert_eq!(
            normalized_coords(&symbols(), 24., &[("wght", 100.)]).unwrap()[3],
            -16384
        );
    }

    #[test]
    fn a_static_font_declares_no_axes() {
        assert!(axes(&super::tests::hack()).is_empty());
    }

    #[test]
    fn a_bad_font_keeps_its_skrifa_cause() {
        let Err(e) = Font::new(&b"not a font"[..]) else {
            panic!("bad bytes must not parse");
        };
        assert!(matches!(e, Error::Font(_)));
        assert!(std::error::Error::source(&e).is_some());
    }
}

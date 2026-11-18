//! [`Id`]: a node's name, composed without touching the heap.
//!
//! A synth editor names one node per slot per field -- one shell formatted
//! 374 of them every frame with `format!`, which is 374 allocations to build
//! strings that were identical to last frame's. [`Id`] is the same key space,
//! a `/`-joined path, held in the value itself until it outgrows the buffer.

use arrayvec::ArrayString;
use std::borrow::Borrow;
use std::sync::Arc;

/// How many bytes an id keeps inline. `osc/12/gain` is eleven; the buffer is
/// sized so a three-segment name never reaches the heap and `Id` stays 48
/// bytes, the length and the tag included.
const INLINE: usize = 40;

#[derive(Clone, Debug)]
enum Repr {
    /// `ArrayString` holds the UTF-8 invariant, so reading the name back is
    /// free: a layout reads thousands per frame.
    Inline(ArrayString<INLINE>),
    /// ponytail: one allocation past `INLINE` bytes, shared on clone. Longer
    /// names are a naming problem, not a hot path; intern if one appears.
    Long(Arc<str>),
}

/// A node's name: a gesture target, a spring's key, a lookup into the
/// resolved scene. Segments join with `/`, and composing one allocates
/// nothing until the whole name passes 40 bytes.
///
/// `&str` and `String` convert into one, so every `.id(..)` that took a
/// string still does.
///
/// ```
/// use mui_layout::Id;
/// let osc = Id::of("osc");
/// assert_eq!(&*osc.slot(3).field("gain"), "osc/3/gain");
/// assert_eq!(osc.slot(3), Id::of("osc").slot(3));
/// assert_ne!(osc.slot(3), osc.slot(4));
/// ```
#[derive(Clone, Debug)]
pub struct Id(Repr);

impl Id {
    /// Name a root: `Id::of("osc")`. Names starting with `/` belong to the
    /// runtime (tree paths, `/tip`); a debug build refuses them here. The
    /// `From` conversions don't: a string can be a runtime id read back
    /// from the scene (`ui.get(&surface.key)`), and it must reach its node.
    ///
    /// ```
    /// use mui_layout::Id;
    /// assert_eq!(&*Id::of("osc"), "osc");
    /// ```
    pub fn of(name: &str) -> Self {
        debug_assert!(
            !name.starts_with('/'),
            "{name:?}: ids starting with `/` are reserved for the runtime"
        );
        Self::runtime(name)
    }

    /// A name the runtime owns, which may start with `/`: the tooltip's
    /// `/tip`. Application code names nodes with [`Id::of`].
    pub fn runtime(name: &str) -> Self {
        match ArrayString::from(name) {
            Ok(inline) => Self(Repr::Inline(inline)),
            Err(_) => Self(Repr::Long(Arc::from(name))),
        }
    }

    /// Whether `key` is an application's id rather than a tree path (`/0/2`,
    /// or `""` for the root) or a name the runtime owns (`/tip`). The one
    /// copy of the rule.
    ///
    /// ```
    /// use mui_layout::Id;
    /// assert!(Id::is_named("osc/3"));
    /// assert!(!Id::is_named("/0/2") && !Id::is_named(""));
    /// ```
    pub fn is_named(key: &str) -> bool {
        !(key.is_empty() || key.starts_with('/'))
    }

    /// The whole name, `/`-joined.
    ///
    /// ```
    /// use mui_layout::Id;
    /// assert_eq!(Id::of("rack").slot(0).as_str(), "rack/0");
    /// ```
    pub fn as_str(&self) -> &str {
        match &self.0 {
            Repr::Inline(s) => s,
            Repr::Long(s) => s,
        }
    }

    /// One more segment, by index: the *n*th card in a rack.
    ///
    /// ```
    /// use mui_layout::Id;
    /// let rack = Id::of("rack");
    /// assert_eq!(&*rack.slot(12), "rack/12");
    /// ```
    pub fn slot(&self, n: usize) -> Self {
        Self::runtime_slot(self, n)
    }

    /// The runtime key of unnamed child `n` of the node keyed `parent`:
    /// `parent/n`, built in place (`/0/2` under an unnamed root).
    ///
    /// ```
    /// use mui_layout::Id;
    /// assert_eq!(&*Id::runtime_slot("/0", 12), "/0/12");
    /// ```
    pub fn runtime_slot(parent: &str, n: usize) -> Self {
        // usize::MAX is 20 digits.
        let mut digits = [0u8; 20];
        let mut i = digits.len();
        let mut n = n;
        loop {
            i -= 1;
            digits[i] = b'0' + (n % 10) as u8;
            n /= 10;
            if n == 0 {
                break;
            }
        }
        Self::joined(
            parent,
            std::str::from_utf8(&digits[i..]).unwrap_or_default(),
        )
    }

    /// One more segment from a permanent, non-reused model entity ID.
    /// Unlike a display index, this value must remain unchanged on reorder,
    /// reparent or rename. The caller owns allocation/generation of entity IDs.
    /// Keep display names in `.named(..)`, not in this identity path.
    ///
    /// ```
    /// use mui_layout::Id;
    /// let gain = Id::of("osc").entity(u64::MAX).field("gain");
    /// assert_eq!(gain.as_str(), "osc/18446744073709551615/gain");
    /// ```
    pub fn entity(&self, mut id: u64) -> Self {
        let mut digits = [0u8; 20];
        let mut at = digits.len();
        loop {
            at -= 1;
            digits[at] = b'0' + (id % 10) as u8;
            id /= 10;
            if id == 0 {
                break;
            }
        }
        // These bytes are generated ASCII digits, not user-provided UTF-8.
        self.join(std::str::from_utf8(&digits[at..]).expect("ASCII entity id"))
    }

    /// One more segment, by name: the field on that card.
    ///
    /// ```
    /// use mui_layout::Id;
    /// assert_eq!(&*Id::of("osc").slot(3).field("gain"), "osc/3/gain");
    /// ```
    pub fn field(&self, name: &str) -> Self {
        self.join(name)
    }

    fn join(&self, part: &str) -> Self {
        Self::joined(self, part)
    }

    fn joined(head: &str, part: &str) -> Self {
        if head.len() + 1 + part.len() <= INLINE {
            let mut inline = ArrayString::new();
            inline.push_str(head);
            inline.push('/');
            inline.push_str(part);
            Self(Repr::Inline(inline))
        } else {
            Self(Repr::Long(Arc::from(format!("{head}/{part}"))))
        }
    }
}

impl std::ops::Deref for Id {
    type Target = str;
    fn deref(&self) -> &str {
        self.as_str()
    }
}
impl AsRef<str> for Id {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}
impl Borrow<str> for Id {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}
impl std::fmt::Display for Id {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}
impl PartialEq for Id {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}
impl Eq for Id {}
impl PartialOrd for Id {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Id {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // UTF-8 orders bytewise as `str` does.
        self.as_str().cmp(other.as_str())
    }
}
impl std::hash::Hash for Id {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.as_str().hash(state);
    }
}
impl From<&str> for Id {
    fn from(s: &str) -> Self {
        Self::runtime(s)
    }
}
impl From<&String> for Id {
    fn from(s: &String) -> Self {
        Self::runtime(s)
    }
}
impl From<String> for Id {
    fn from(s: String) -> Self {
        Self::runtime(&s)
    }
}
impl From<&Id> for Id {
    fn from(id: &Id) -> Self {
        id.clone()
    }
}
impl From<Id> for String {
    fn from(id: Id) -> Self {
        id.as_str().to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Equal names are one key and different ones are not, however they were
    /// composed -- a composed id and a literal have to agree, or last
    /// frame's spring belongs to nobody.
    #[test]
    fn composed_ids_compare_by_their_whole_name() {
        assert_eq!(Id::of("osc").slot(3).field("gain"), Id::from("osc/3/gain"));
        assert_ne!(Id::of("osc").slot(3), Id::of("osc").slot(30));
        assert_ne!(Id::of("osc").field("3"), Id::of("osc3"));
        assert_eq!(Id::of("osc").slot(0).slot(10).as_str(), "osc/0/10");
    }

    /// `/` names are the runtime's: `Id::of` refuses one, `Id::runtime`
    /// makes one.
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "reserved for the runtime")]
    fn a_leading_slash_is_the_runtimes() {
        assert_eq!(Id::runtime("/tip").as_str(), "/tip");
        let _ = Id::of("/tip");
    }

    #[test]
    fn a_runtime_id_read_back_as_a_string_converts() {
        let key = Id::runtime_slot("/0", 0).as_str().to_owned();
        assert_eq!(Id::from(&key), Id::runtime(&key));
        assert_eq!(Id::from(key.as_str()), Id::runtime(&key));
    }

    /// The whole point: the hot path -- one id per slot per field, every
    /// frame -- never reaches the heap.
    #[test]
    fn a_slot_field_id_stays_inline() {
        let id = Id::of("modulators").slot(374).field("depth");
        assert!(matches!(id.0, Repr::Inline(_)), "{id} allocated");
        assert_eq!(size_of::<Id>(), 48, "every node carries one");
        // And the escape hatch still produces the right name.
        let long = Id::of(&"x".repeat(INLINE + 1));
        assert!(matches!(long.0, Repr::Long(_)));
        assert_eq!(long.as_str().len(), INLINE + 1);
    }
}

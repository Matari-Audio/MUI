//! Explicit, eager font discovery. This module never changes the default font
//! chain and does no I/O during selection or shaping.

use std::{ops::Range, sync::Arc};

use skrifa::MetadataProvider as _;
use unicode_segmentation::UnicodeSegmentation;

use crate::{Axes, Error, Font, Weight, shape::covers_grapheme};

pub use fontdb::{Family, Stretch as FontStretch, Style as FontStyle};

/// CSS-style face selection, followed by whole-grapheme fallback.
///
/// `families` is an ordered preference list. A language hint prefers common
/// locale-specific CJK families before the remaining installed faces; it is
/// a fallback hint, not an operating-system locale change or a shaper setting.
#[derive(Clone, Copy, Debug)]
pub struct FontQuery<'a> {
    pub families: &'a [Family<'a>],
    pub weight: Weight,
    pub style: FontStyle,
    pub stretch: FontStretch,
    /// BCP-47 language, for example `ja`, `zh-Hant` or `ko`.
    pub language: Option<&'a str>,
}

impl Default for FontQuery<'_> {
    fn default() -> Self {
        Self {
            families: &[],
            weight: Weight::REGULAR,
            style: FontStyle::Normal,
            stretch: FontStretch::Normal,
            language: None,
        }
    }
}

/// Prepared fonts for [`crate::shape_run`], [`crate::text_run`] or a Scene/Ui's
/// primary/fallback fonts. Glyph font indices refer to this exact ordered list.
#[derive(Clone, Debug)]
pub struct FontSelection {
    pub fonts: Vec<Font>,
    /// Requested variable-font weight/style/stretch coordinates. Pass `axes.to_vec()`
    /// to shaping along with the font chain; static faces ignore absent axes.
    pub axes: Axes,
    /// Grapheme byte ranges that no supplied/discovered face covers. Shaping
    /// retains the primary face's .notdef glyph for these, rather than dropping
    /// text. Default-ignorable controls alone are not reported as missing.
    pub missing: Vec<Range<usize>>,
    /// Include this in a cache of selections. Adding/removing fonts can change
    /// a fallback result even when its primary font stays the same.
    pub revision: u64,
}

/// A fully resident collection. Construct/discover/update it on a startup or
/// worker thread, then pass prepared fonts to rendering. Selection uses memory
/// only, but can allocate and inspect many faces: it is not an audio-thread API.
///
/// `new()` is empty and deterministic. `discover_system()` is opt-in and scans
/// fontdb's macOS/Windows font directories or Linux fontconfig configuration.
/// Fonts registered only through an OS API outside those paths may be absent;
/// add their bytes explicitly with [`Self::add`].
#[derive(Default)]
pub struct FontDatabase {
    db: fontdb::Database,
    fonts: Vec<(fontdb::ID, Font)>,
    revision: u64,
    unavailable_faces: usize,
}

impl FontDatabase {
    pub fn new() -> Self {
        Self::default()
    }

    /// Discover and read fonts now, including each face of TTC/OTC collections.
    /// The returned database's selection methods never access the filesystem.
    /// Faces that cannot be read by MUI after discovery are skipped and counted
    /// by `unavailable_faces()`. fontdb logs files it rejects during discovery.
    pub fn discover_system() -> Self {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        let mut collection = Self {
            db,
            ..Self::default()
        };
        collection.load_pending();
        collection
    }

    /// Register supplied bytes (including every face of a collection). Returns
    /// the number of readable, named faces added. Invalid bytes return an error;
    /// valid unnamed fonts should instead be passed directly as [`Font`]s.
    /// Existing Font identities stay stable; selection caches use `revision()`
    /// to invalidate any previously prepared fallback choices after this call.
    pub fn add(&mut self, bytes: impl Into<Arc<[u8]>>) -> Result<usize, Error> {
        let bytes = bytes.into();
        Font::from_index(bytes.clone(), 0)?;
        let before = self.fonts.len();
        self.db
            .load_font_source(fontdb::Source::Binary(Arc::new(bytes)));
        self.load_pending();
        Ok(self.fonts.len() - before)
    }

    /// Remove all supplied/discovered faces and advance the collection revision.
    /// Already prepared Font handles remain valid and keep their own cache keys.
    pub fn clear(&mut self) {
        self.db = fontdb::Database::new();
        self.fonts.clear();
        self.unavailable_faces = 0;
        self.revision += 1;
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn len(&self) -> usize {
        self.fonts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.fonts.is_empty()
    }

    pub fn unavailable_faces(&self) -> usize {
        self.unavailable_faces
    }

    /// Unique, sorted family names, including localized font family names.
    pub fn families(&self) -> Vec<&str> {
        let mut names: Vec<_> = self
            .db
            .faces()
            .flat_map(|face| face.families.iter().map(|family| family.0.as_str()))
            .collect();
        names.sort_unstable();
        names.dedup();
        names
    }

    /// The first available family, using fontdb's CSS weight/style/stretch match.
    /// Returns None for missing families; it does not silently choose a system
    /// primary or discover fonts.
    pub fn select(&self, query: &FontQuery<'_>) -> Option<Font> {
        let id = self.db.query(&fontdb::Query {
            families: query.families,
            weight: fontdb::Weight(query.weight.value()),
            style: query.style,
            stretch: query.stretch,
        })?;
        self.fonts
            .iter()
            .find(|(candidate, _)| *candidate == id)
            .map(|(_, font)| font.clone())
    }

    /// Select a primary family and prepare fallback for `text`. Missing primary
    /// families are an error; use `prepare_with` to retain supplied primary fonts.
    pub fn prepare(&self, query: &FontQuery<'_>, text: &str) -> Result<FontSelection, Error> {
        let primary = self
            .select(query)
            .ok_or(Error::InvalidOptions("font family"))?;
        self.prepare_with(&[primary], query, text)
    }

    /// Retain the supplied primary/fallback chain, append discovered faces only
    /// where needed, and report unsupported graphemes. The original chain wins
    /// whenever it covers the complete grapheme; bases and marks never split.
    /// Re-run after text, query or collection revision changes. Normal shaping
    /// functions need no knowledge of the database.
    pub fn prepare_with(
        &self,
        supplied: &[Font],
        query: &FontQuery<'_>,
        text: &str,
    ) -> Result<FontSelection, Error> {
        if supplied.is_empty() {
            return Err(Error::InvalidOptions("fonts"));
        }
        let mut candidates = Vec::new();
        // Explicit families precede locale preferences and automatic fallback.
        for family in query.families {
            let local = FontQuery {
                families: std::slice::from_ref(family),
                ..*query
            };
            if let Some(font) = self.select(&local) {
                candidates.push(font);
            }
        }
        for name in language_families(query.language) {
            let families = [Family::Name(name)];
            if let Some(font) = self.select(&FontQuery {
                families: &families,
                ..*query
            }) {
                candidates.push(font);
            }
        }
        // ponytail: linear fallback scan, bounded by installed faces. Cache
        // prepared chains by text/query/revision if repeated selection matters.
        let mut remaining: Vec<_> = self.fonts.iter().collect();
        remaining.sort_by_key(|(id, _)| {
            let info = self.db.face(*id).expect("resident face metadata");
            (
                info.style != query.style,
                info.stretch != query.stretch,
                info.weight.0.abs_diff(query.weight.value()),
                &info.post_script_name,
                info.index,
            )
        });
        candidates.extend(remaining.into_iter().map(|(_, font)| font.clone()));
        let mut fonts = supplied.to_vec();
        let mut missing = Vec::new();
        for (start, grapheme) in text.grapheme_indices(true) {
            if fonts.iter().any(|font| covers(font, grapheme)) {
                continue;
            }
            if let Some(font) = candidates.iter().find(|font| covers(font, grapheme)) {
                fonts.push(font.clone());
            } else {
                missing.push(start..start + grapheme.len());
            }
        }
        let mut axes = Axes::from(query.weight);
        axes.set(
            "wdth",
            [50., 62.5, 75., 87.5, 100., 112.5, 125., 150., 200.]
                [usize::from(query.stretch.to_number()) - 1],
        );
        axes.set(
            "ital",
            if query.style == FontStyle::Italic {
                1.
            } else {
                0.
            },
        );
        axes.set(
            "slnt",
            if query.style == FontStyle::Oblique {
                -14.
            } else {
                0.
            },
        );
        Ok(FontSelection {
            fonts,
            axes,
            missing,
            revision: self.revision,
        })
    }

    fn load_pending(&mut self) {
        let mut sources: Vec<(fontdb::Source, Arc<[u8]>)> = Vec::new();
        let faces: Vec<_> = self.db.faces().cloned().collect();
        let mut unavailable = Vec::new();
        for face in faces {
            if self.fonts.iter().any(|(id, _)| *id == face.id) {
                continue;
            }
            // Each collection file is read/copied once, then shared by its
            // constituent Font faces. No persistent mmap of mutable files.
            let bytes = sources
                .iter()
                .find(|(source, _)| same_source(source, &face.source))
                .map(|(_, bytes)| bytes.clone())
                .or_else(|| {
                    self.db.with_face_data(face.id, |bytes, _| {
                        let bytes: Arc<[u8]> = Arc::from(bytes);
                        sources.push((face.source.clone(), bytes.clone()));
                        bytes
                    })
                });
            match bytes.and_then(|bytes| Font::from_index(bytes, face.index).ok()) {
                Some(font) => self.fonts.push((face.id, font)),
                None => unavailable.push(face.id),
            }
        }
        self.unavailable_faces += unavailable.len();
        for id in unavailable {
            self.db.remove_face(id);
        }
        self.revision += 1;
    }
}

fn same_source(a: &fontdb::Source, b: &fontdb::Source) -> bool {
    if let (fontdb::Source::Binary(a), fontdb::Source::Binary(b)) = (a, b) {
        return Arc::ptr_eq(a, b);
    }
    if let (fontdb::Source::File(a), fontdb::Source::File(b)) = (a, b) {
        return a == b;
    }
    // Another crate can enable fontdb's mmap variant via feature unification.
    false
}

fn covers(font: &Font, grapheme: &str) -> bool {
    font.font_ref()
        .is_ok_and(|face| covers_grapheme(&face.charmap(), grapheme))
}

fn language_families(language: Option<&str>) -> &'static [&'static str] {
    let Some(language) = language else {
        return &[];
    };
    let primary = language.split('-').next().unwrap_or("");
    if primary.eq_ignore_ascii_case("ja") {
        &[
            "Noto Sans CJK JP",
            "Noto Sans JP",
            "Yu Gothic",
            "Hiragino Sans",
        ]
    } else if primary.eq_ignore_ascii_case("ko") {
        &[
            "Noto Sans CJK KR",
            "Noto Sans KR",
            "Malgun Gothic",
            "Apple SD Gothic Neo",
        ]
    } else if primary.eq_ignore_ascii_case("zh") {
        let traditional = language.split('-').any(|part| {
            ["Hant", "TW", "HK", "MO"]
                .iter()
                .any(|hint| part.eq_ignore_ascii_case(hint))
        });
        if traditional {
            &[
                "Noto Sans CJK TC",
                "Noto Sans TC",
                "Microsoft JhengHei",
                "PingFang TC",
            ]
        } else {
            &[
                "Noto Sans CJK SC",
                "Noto Sans SC",
                "Microsoft YaHei",
                "PingFang SC",
            ]
        }
    } else {
        &[]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{shape_run, test_fonts::hack};

    #[test]
    fn supplied_fonts_stay_primary_and_added_faces_rekey_selection() {
        let mut db = FontDatabase::new();
        let primary = hack();
        let query = FontQuery::default();
        let before = db
            .prepare_with(std::slice::from_ref(&primary), &query, "A😀")
            .unwrap();
        assert_eq!(before.fonts.as_slice(), std::slice::from_ref(&primary));
        assert_eq!(before.missing.len(), 1);
        assert_eq!(before.missing[0], 1..5);
        assert_eq!(db.add(epaint_default_fonts::NOTO_EMOJI_REGULAR).unwrap(), 1);
        let after = db
            .prepare_with(std::slice::from_ref(&primary), &query, "A😀")
            .unwrap();
        assert!(after.revision > before.revision);
        assert_eq!(after.fonts[0], primary);
        assert!(after.missing.is_empty());
        let run = shape_run(&after.fonts, "A😀", 24., &[]).unwrap();
        assert_eq!(run.glyphs.last().unwrap().font, 1);
        let fallback_id = after.fonts[1].id();
        db.clear();
        db.add(epaint_default_fonts::NOTO_EMOJI_REGULAR).unwrap();
        let reloaded = db.prepare_with(&[primary], &query, "A😀").unwrap();
        assert_ne!(reloaded.fonts[1].id(), fallback_id);
        assert!(reloaded.revision > after.revision);
    }

    #[test]
    fn family_queries_and_whole_graphemes_are_deterministic() {
        let mut db = FontDatabase::new();
        assert!(db.is_empty());
        db.add(epaint_default_fonts::HACK_REGULAR).unwrap();
        db.add(epaint_default_fonts::NOTO_EMOJI_REGULAR).unwrap();
        let families = [Family::Name("does not exist"), Family::Name("Hack")];
        let query = FontQuery {
            families: &families,
            ..FontQuery::default()
        };
        let primary = db.select(&query).expect("second family selected");
        assert_eq!(primary, db.select(&query).unwrap());
        assert!(db.prepare(&query, "A\n\r\n\tB").unwrap().missing.is_empty());
        let sequence = "😀\u{200d}😀\u{fe0f}";
        let selection = db.prepare(&query, sequence).unwrap();
        assert!(selection.missing.is_empty());
        let run = shape_run(&selection.fonts, sequence, 24., &[]).unwrap();
        assert!(run.glyphs.iter().all(|glyph| glyph.font == 1));
        let absent = "a\u{10ffff}\u{301}";
        let selection = db.prepare(&query, absent).unwrap();
        assert_eq!(selection.missing.len(), 1);
        assert_eq!(selection.missing[0], 1..absent.len());
        assert!(db.prepare(&FontQuery::default(), "A").is_err());
        assert!(db.add(&b"not a font"[..]).is_err());
    }

    #[test]
    fn collection_and_synthetic_style_language_matching() {
        let mut db = FontDatabase::new();
        assert_eq!(db.add(crate::test_fonts::collection()).unwrap(), 2);
        let id = db
            .fonts
            .iter()
            .find(|(_, font)| font.index() == 1)
            .unwrap()
            .0;
        let inter_family = db.db.face(id).unwrap().families[0].0.clone();
        let families = [Family::Name(&inter_family)];
        let selected = db
            .select(&FontQuery {
                families: &families,
                ..FontQuery::default()
            })
            .unwrap();
        assert_eq!(selected.index(), 1);

        // Deterministic registry: use valid supplied glyph bytes with synthetic
        // metadata, so matching tests don't depend on a host's installed fonts.
        let mut raw = fontdb::Database::new();
        raw.load_font_data(epaint_default_fonts::NOTO_EMOJI_REGULAR.to_vec());
        let template = raw.faces().next().unwrap().clone();
        let mut normal = template.clone();
        normal.families[0].0 = "Test".into();
        normal.style = FontStyle::Normal;
        normal.weight = fontdb::Weight::NORMAL;
        let normal_id = db.db.push_face_info(normal.clone());
        let mut bold = normal;
        bold.style = FontStyle::Italic;
        bold.weight = fontdb::Weight::BOLD;
        let bold_id = db.db.push_face_info(bold);
        let mut japanese = template.clone();
        japanese.families[0].0 = "Noto Sans CJK JP".into();
        let jp_id = db.db.push_face_info(japanese);
        let mut chinese = template;
        chinese.families[0].0 = "Noto Sans CJK TC".into();
        let tc_id = db.db.push_face_info(chinese);
        db.load_pending();
        let lookup = |id| {
            db.fonts
                .iter()
                .find(|(key, _)| *key == id)
                .unwrap()
                .1
                .clone()
        };
        let families = [Family::Name("Test")];
        let query = FontQuery {
            families: &families,
            ..FontQuery::default()
        };
        assert_eq!(db.select(&query).unwrap(), lookup(normal_id));
        assert_eq!(
            db.select(&FontQuery {
                weight: Weight::BOLD,
                style: FontStyle::Italic,
                ..query
            })
            .unwrap(),
            lookup(bold_id)
        );
        let primary = hack();
        let query = FontQuery {
            language: Some("ja"),
            ..FontQuery::default()
        };
        assert_eq!(
            db.prepare_with(std::slice::from_ref(&primary), &query, "😀")
                .unwrap()
                .fonts[1],
            lookup(jp_id)
        );
        let query = FontQuery {
            language: Some("zh-Hant"),
            ..query
        };
        assert_eq!(
            db.prepare_with(&[primary], &query, "😀").unwrap().fonts[1],
            lookup(tc_id)
        );
    }

    #[test]
    fn prepared_weight_reaches_variable_font_shaping() {
        let mut db = FontDatabase::new();
        db.add(ttf_inter::REGULAR).unwrap();
        let families = [Family::Name(db.families()[0])];
        let query = FontQuery {
            families: &families,
            ..FontQuery::default()
        };
        let normal = db.prepare(&query, "AV").unwrap();
        let bold = db
            .prepare(
                &FontQuery {
                    weight: Weight::BOLD,
                    ..query
                },
                "AV",
            )
            .unwrap();
        assert_eq!(bold.axes.get("wght"), Some(700.));
        let condensed = db
            .prepare(
                &FontQuery {
                    stretch: FontStretch::Condensed,
                    ..query
                },
                "AV",
            )
            .unwrap();
        assert_eq!(condensed.axes.get("wdth"), Some(75.));
        let normal =
            crate::text_run(&normal.fonts, "AV", 24., &normal.axes.to_vec(), 0.05).unwrap();
        let bold = crate::text_run(&bold.fonts, "AV", 24., &bold.axes.to_vec(), 0.05).unwrap();
        assert_ne!(normal.path, bold.path);
    }

    #[test]
    fn locale_preferences_distinguish_cjk_forms() {
        assert_eq!(language_families(Some("JA-jp"))[0], "Noto Sans CJK JP");
        assert_eq!(language_families(Some("zh-hant"))[0], "Noto Sans CJK TC");
        assert_eq!(language_families(Some("zh-HK"))[0], "Noto Sans CJK TC");
        assert_eq!(language_families(Some("zh-CN"))[0], "Noto Sans CJK SC");
        assert_eq!(language_families(Some("ko"))[0], "Noto Sans CJK KR");
        assert!(language_families(None).is_empty());
    }

    #[test]
    #[ignore = "explicit native discovery smoke test; requires installed fonts"]
    fn native_discovery_is_ready_for_memory_only_selection() {
        let db = FontDatabase::discover_system();
        assert!(!db.is_empty(), "no installed readable fonts");
        let families = db.families();
        let names = [Family::Name(families[0])];
        let selected = db
            .prepare(
                &FontQuery {
                    families: &names,
                    ..FontQuery::default()
                },
                "Hello",
            )
            .unwrap();
        assert!(
            shape_run(&selected.fonts, "Hello", 16., &[])
                .unwrap()
                .advance
                > 0.
        );
    }
}

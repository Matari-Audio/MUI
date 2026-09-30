//! The project's sources: the files and plugins imported into it, which
//! the editor's Sources panel lists (a plugin as a folder of its parts)
//! and layers are made from.
use serde::{Deserialize, Serialize};

use crate::{Kind, Project, Source};

/// One imported file or plugin.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Media {
    /// Unique among the sources.
    pub id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(flatten)]
    pub kind: MediaKind,
}

/// What a source is; a file's `path` is relative to the project.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum MediaKind {
    /// A MUI plugin editor (a mui-motion-bridge adapter), as a plugin
    /// layer's `source` names it.
    Plugin {
        source: Source,
    },
    /// A PNG.
    Image {
        path: String,
    },
    Svg {
        path: String,
    },
    /// A Lottie JSON file.
    Lottie {
        path: String,
    },
    /// A glTF binary (`.glb`), for 3D scenes.
    Model {
        path: String,
    },
    /// A TrueType or OpenType font (`.ttf`, `.otf`) text layers pick by
    /// its id.
    Font {
        path: String,
    },
}

impl Media {
    /// The file it is, if it is one.
    pub fn path(&self) -> Option<&str> {
        (!matches!(self.kind, MediaKind::Plugin { .. })).then(|| self.kind.path())
    }

    /// A plugin's fresh capture state ([`crate::plugin::home`]): the
    /// manifest `CACHE/<state>.json` lists its parts.
    pub fn state(&self) -> Option<String> {
        match &self.kind {
            MediaKind::Plugin { source } => Some(crate::plugin::home(source)),
            _ => None,
        }
    }

    /// What a layer of `kind` draws, as a source (`None` for kinds that
    /// draw no file or plugin).
    pub fn of(kind: &Kind) -> Option<MediaKind> {
        Some(match kind {
            Kind::Plugin { source, .. } => MediaKind::Plugin {
                source: source.clone(),
            },
            Kind::Image { path } => MediaKind::Image { path: path.clone() },
            Kind::Svg { path } => MediaKind::Svg { path: path.clone() },
            Kind::Lottie { path, .. } => MediaKind::Lottie { path: path.clone() },
            Kind::Model { path } => MediaKind::Model { path: path.clone() },
            Kind::Text { font, .. } if !font.is_empty() => MediaKind::Font { path: font.clone() },
            _ => return None,
        })
    }
}

/// Ids unique and not empty, files named, plugin sources well formed.
pub(crate) fn check(list: &[Media]) -> Result<(), String> {
    for (i, m) in list.iter().enumerate() {
        let at = format!("sources[{i}]");
        if m.id.is_empty() {
            return Err(format!("{at}.id: a source needs an id"));
        }
        if list[..i].iter().any(|o| o.id == m.id) {
            return Err(format!("{at}.id: duplicate source id `{}`", m.id));
        }
        match &m.kind {
            MediaKind::Plugin { source } => source
                .check()
                .map_err(|e| format!("{at}.source: source `{}`: {e}", m.id))?,
            _ if m.path().is_some_and(str::is_empty) => {
                return Err(format!("{at}.path: source `{}` names no file", m.id));
            }
            _ => {}
        }
    }
    Ok(())
}

impl Project {
    /// `sources`, then every file and plugin a layer uses that they do not
    /// list, once each, named after its file (or its example or binary):
    /// the Sources panel's rows.
    pub fn all_sources(&self) -> Vec<Media> {
        let mut out = self.sources.clone();
        for l in self.scenes.iter().flat_map(|s| &s.layers) {
            let Some(kind) = Media::of(&l.kind) else {
                continue;
            };
            // A text layer's font names a source by id, or a file.
            let named = |m: &Media| matches!(kind, MediaKind::Font { .. }) && m.id == kind.path();
            if out.iter().any(|m| m.kind == kind || named(m)) {
                continue;
            }
            let stem = match &kind {
                MediaKind::Plugin { source } => [&source.example, &source.bin, &source.plugin]
                    .into_iter()
                    .find(|s| !s.is_empty())
                    .map_or("plugin", |s| s.as_str()),
                other => other.path(),
            };
            let stem = stem.rsplit(['/', '\\']).next().unwrap_or(stem).to_owned();
            let mut id = stem.clone();
            let mut n = 2;
            while out.iter().any(|m| m.id == id) {
                id = format!("{stem} {n}");
                n += 1;
            }
            out.push(Media {
                id,
                name: String::new(),
                kind,
            });
        }
        out
    }
}

impl MediaKind {
    fn path(&self) -> &str {
        match self {
            MediaKind::Plugin { .. } => "",
            MediaKind::Image { path }
            | MediaKind::Svg { path }
            | MediaKind::Lottie { path }
            | MediaKind::Model { path }
            | MediaKind::Font { path } => path,
        }
    }
}

impl Project {
    /// The file a text layer's `font` names: a font source's path by its
    /// id, else `font` itself (a path, or empty for Inter).
    pub fn font<'a>(&'a self, font: &'a str) -> &'a str {
        self.sources
            .iter()
            .find_map(|m| match &m.kind {
                MediaKind::Font { path } if m.id == font => Some(path.as_str()),
                _ => None,
            })
            .unwrap_or(font)
    }

    /// The file layer `l` draws: [`Layer::asset`](crate::Layer::asset), or
    /// a text layer's font file.
    pub fn asset_of<'a>(&'a self, l: &'a crate::Layer) -> Option<&'a str> {
        match &l.kind {
            Kind::Text { font, .. } => Some(self.font(font)).filter(|f| !f.is_empty()),
            _ => l.asset(),
        }
    }
}

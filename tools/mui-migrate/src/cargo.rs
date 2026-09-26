//! Cargo.toml: find mui crate aliases, and repoint path deps of moved crates.

use crate::rules::{CARGO_MOVES, MUI_CRATES};
use std::path::{Component, Path, PathBuf};
use toml_edit::{DocumentMut, Item, TableLike, Value};

/// Dependency keys that name a mui crate under another name
/// (`mui2 = { package = "mui", .. }` -> `mui2`), as Rust idents.
pub fn aliases(manifest: &str) -> Vec<String> {
    let Ok(doc) = manifest.parse::<DocumentMut>() else { return Vec::new() };
    let mut out = Vec::new();
    deps(doc.as_item(), false, &mut |key, entry| {
        if let Some(pkg) = entry.get("package").and_then(Item::as_str)
            && MUI_CRATES.contains(&pkg)
        {
            out.push(key.replace('-', "_"));
        }
    });
    out
}

/// Rewrite moved path deps. Returns the new text and the number of edits.
pub fn rewrite(manifest: &str, dir: &Path) -> Result<(String, usize), String> {
    let mut doc = manifest.parse::<DocumentMut>().map_err(|e| e.to_string())?;
    let mut n = 0;
    deps_mut(doc.as_item_mut(), false, &mut |entry| {
        let Some(item) = entry.get_mut("path") else { return };
        let Some(Value::String(old)) = item.as_value() else { return };
        let old = old.value().clone();
        if let Some(new) = moved(&old, dir) {
            let decor = item.as_value().unwrap().decor().clone();
            let mut v = Value::from(new);
            *v.decor_mut() = decor;
            *item = Item::Value(v);
            n += 1;
        }
    });
    Ok((if n == 0 { manifest.to_string() } else { doc.to_string() }, n))
}

fn is_deps(key: &str) -> bool {
    key.ends_with("dependencies")
}

fn deps(item: &Item, in_deps: bool, f: &mut impl FnMut(&str, &dyn TableLike)) {
    let Some(t) = item.as_table_like() else { return };
    for (k, v) in t.iter() {
        if in_deps {
            if let Some(entry) = v.as_table_like() {
                f(k, entry);
            }
        } else {
            deps(v, is_deps(k), f);
        }
    }
}

fn deps_mut(item: &mut Item, in_deps: bool, f: &mut impl FnMut(&mut dyn TableLike)) {
    let Some(t) = item.as_table_like_mut() else { return };
    for (k, v) in t.iter_mut() {
        if in_deps {
            if let Some(entry) = v.as_table_like_mut() {
                f(entry);
            }
        } else {
            let d = is_deps(k.get());
            deps_mut(v, d, f);
        }
    }
}

/// New path string for a dep path that points into a moved crate.
fn moved(path: &str, dir: &Path) -> Option<String> {
    let rel = Path::new(path).is_relative();
    let abs = normalize(&dir.join(path));
    for (old, new) in CARGO_MOVES {
        let old = Path::new(old);
        if abs.ends_with(old) {
            let base = abs.components().take(abs.components().count() - old.components().count()).collect::<PathBuf>();
            let target = base.join(new);
            let out = if rel { relative(&normalize(dir), &target) } else { target };
            return Some(out.to_string_lossy().replace('\\', "/"));
        }
    }
    None
}

fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir if matches!(out.components().next_back(), Some(Component::Normal(_))) => {
                out.pop();
            }
            c => out.push(c),
        }
    }
    out
}

fn relative(from: &Path, to: &Path) -> PathBuf {
    let f: Vec<_> = from.components().collect();
    let t: Vec<_> = to.components().collect();
    let common = f.iter().zip(&t).take_while(|(a, b)| a == b).count();
    let mut out = PathBuf::new();
    for _ in common..f.len() {
        out.push("..");
    }
    for c in &t[common..] {
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moves_relative_and_nested_paths() {
        let src = "[dependencies]\n# keep me\nmui-stage = { path = \"../mui-stage\", optional = true } # trailing\nmui = { path = \"../mui\" }\n\n[target.'cfg(unix)'.dev-dependencies.mui-reel]\npath = \"../../crates/mui-reel\"\n\n[lib]\npath = \"src/lib.rs\"\n";
        let (out, n) = rewrite(src, Path::new("/r/crates/mui-preview")).unwrap();
        assert_eq!(n, 2);
        assert_eq!(
            out,
            "[dependencies]\n# keep me\nmui-stage = { path = \"../../media/mui-stage\", optional = true } # trailing\nmui = { path = \"../mui\" }\n\n[target.'cfg(unix)'.dev-dependencies.mui-reel]\npath = \"../../media/mui-reel\"\n\n[lib]\npath = \"src/lib.rs\"\n"
        );
        let (again, n2) = rewrite(&out, Path::new("/r/crates/mui-preview")).unwrap();
        assert_eq!((again.as_str(), n2), (out.as_str(), 0));
    }

    #[test]
    fn finds_package_aliases() {
        let src = "[dependencies]\nmui2 = { package = \"mui\", path = \"x\" }\nmui2-vello = { package = \"mui-vello\", path = \"y\" }\nother = { package = \"mui-gpui-plugin-probe\", path = \"z\" }\n";
        assert_eq!(aliases(src), ["mui2", "mui2_vello"]);
    }
}

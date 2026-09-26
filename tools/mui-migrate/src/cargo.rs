//! Cargo.toml: find mui crate aliases, and repoint path deps of moved crates.

use crate::rules::{CARGO_MOVES, MUI_CRATES};
use std::path::{Component, Path, PathBuf};
use toml_edit::{DocumentMut, Item, TableLike, Value};

/// A manifest's mui dependencies, as Rust idents: `v2` are the keys that
/// name a mui crate under another name (`mui2 = { package = "mui", .. }` ->
/// `mui2`), `v1` the keys of mui crates whose path points into a v1 checkout
/// (no `crates/mui-scene` next to them, or under `.build-inputs/mui/`), which
/// this tool must leave alone. With `root`, a path dep is v2 only inside it.
pub fn mui_deps(manifest: &str, dir: &Path, root: Option<&Path>) -> (Vec<String>, Vec<String>) {
    let Ok(doc) = manifest.parse::<DocumentMut>() else { return Default::default() };
    let (mut v2, mut v1) = (Vec::new(), Vec::new());
    deps(doc.as_item(), false, &mut |key, entry| {
        let pkg = entry.get("package").and_then(Item::as_str).unwrap_or(key);
        if !MUI_CRATES.contains(&pkg) {
            return;
        }
        let ident = key.replace('-', "_");
        if let Some(path) = entry.get("path").and_then(Item::as_str)
            && is_v1(&normalize(&dir.join(path)), root)
        {
            v1.push(ident);
        } else if key != pkg {
            v2.push(ident);
        }
    });
    (v2, v1)
}

/// Whether the crate at `dir` belongs to a v1 mui checkout.
fn is_v1(dir: &Path, root: Option<&Path>) -> bool {
    if let Some(root) = root {
        return !dir.starts_with(normalize(root));
    }
    // The workspace root is the nearest ancestor with a `crates/` directory.
    match dir.ancestors().find(|a| a.join("crates").is_dir()) {
        Some(ws) => !ws.join("crates/mui-scene").is_dir(),
        None => dir.components().collect::<Vec<_>>().windows(2).any(|w| w[0].as_os_str() == ".build-inputs" && w[1].as_os_str() == "mui"),
    }
}

/// `[package] name`, if the manifest has one.
pub fn package(manifest: &str) -> Option<String> {
    let doc = manifest.parse::<DocumentMut>().ok()?;
    Some(doc.get("package")?.get("name")?.as_str()?.to_string())
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
        assert_eq!(mui_deps(src, Path::new("/nowhere"), None), (vec!["mui2".into(), "mui2_vello".into()], vec![]));
    }

    #[test]
    fn v1_path_deps_are_not_mui() {
        // KURV: `mui-truce` from the v1 checkout, `mui2` from the v2 one.
        let tmp = std::env::temp_dir().join(format!("mui-migrate-v1-{}", std::process::id()));
        std::fs::create_dir_all(tmp.join(".build-inputs/mui/crates/mui-truce")).unwrap();
        std::fs::create_dir_all(tmp.join(".build-inputs/mui2/crates/mui-scene")).unwrap();
        std::fs::create_dir_all(tmp.join(".build-inputs/mui2/crates/mui")).unwrap();
        let src = "[dependencies]\nmui-truce = { path = \".build-inputs/mui/crates/mui-truce\" }\nmui2 = { package = \"mui\", path = \".build-inputs/mui2/crates/mui\" }\n";
        assert_eq!(mui_deps(src, &tmp, None), (vec!["mui2".into()], vec!["mui_truce".into()]));
        // Not checked out: the `.build-inputs/mui/` path alone says v1.
        let missing = "[dependencies]\nmui-truce = { path = \"x/.build-inputs/mui/crates/mui-truce\" }\nmui = { path = \"y/crates/mui\" }\n";
        assert_eq!(mui_deps(missing, Path::new("/nowhere"), None), (vec![], vec!["mui_truce".into()]));
        // `--mui-root` decides alone.
        let root = tmp.join(".build-inputs/mui");
        assert_eq!(mui_deps(src, &tmp, Some(&root)), (vec![], vec!["mui2".into()]));
        std::fs::remove_dir_all(&tmp).unwrap();
    }
}

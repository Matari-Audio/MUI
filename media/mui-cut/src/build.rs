//! Plugins onboarded with no code in them: [`detect`] reads a plugin crate
//! (its framework, its MUI crates, its editor), [`adapter`] builds a
//! generated adapter around it that hosts its real editor headless
//! (`mui::host::headless`, `mui_motion_bridge::run_headless`), against the
//! MUI tree mui-cut itself was built from.
//!
//! The adapter is its own small Cargo workspace under the cache
//! ([`cache`]`/adapters/<package>`): it depends on the plugin by path,
//! copies the plugin's `Cargo.lock` (so everything but MUI stays at the
//! plugin's versions) and its non-MUI `[patch]`es, and builds with
//! `--config patch.<MUI>.<crate>.path=…` for every MUI crate the plugin
//! uses. The plugin's checkout, `Cargo.lock` included, is only read.
//!
//! Frameworks: moose and truce plugins (`moose::plugin!` / `truce::plugin!`
//! make `crate::Plugin`), nice-plug (the type `nice_export_clap!` names,
//! public at the crate root), and plain MUI crates, which export
//! `pub fn mui_editor() -> (mui::Ui, (u32, u32), impl mui::host::View + Send + 'static)`.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{Value, json};

use crate::Result;

/// MUI's git URL, as plugins depend on it.
pub const MUI_GIT: &str = "https://github.com/Matari-Audio/MUI";

/// How a plugin makes its editor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Framework {
    Moose,
    Truce,
    NicePlug,
    /// No plugin framework: MUI alone.
    Mui,
}

impl Framework {
    /// The crate a plugin depends on to be one; `None` for plain MUI.
    fn krate(self) -> Option<&'static str> {
        match self {
            Framework::Moose => Some("moose"),
            Framework::Truce => Some("truce"),
            Framework::NicePlug => Some("nice-plug"),
            Framework::Mui => None,
        }
    }
    pub fn name(self) -> &'static str {
        self.krate().unwrap_or("mui")
    }
}

/// The framework of a package from its dependencies (`cargo metadata`'s
/// `dependencies`): the first plugin framework, or plain MUI.
pub fn framework(deps: &[Value]) -> Option<Framework> {
    let has = |n: &str| deps.iter().any(|d| d["name"] == n);
    [Framework::Moose, Framework::Truce, Framework::NicePlug]
        .into_iter()
        .find(|f| f.krate().is_some_and(has))
        .or_else(|| has("mui").then_some(Framework::Mui))
}

/// What [`detect`] found in a plugin crate.
#[derive(Clone, Debug)]
pub struct Plugin {
    /// The package's directory.
    pub dir: PathBuf,
    pub package: String,
    pub framework: Framework,
    /// The framework dependency, as `cargo metadata` lists it.
    framework_dep: Option<Value>,
    /// The type the adapter opens: `Plugin` (moose, truce) or nice-plug's.
    pub entry: String,
    /// The MUI crates it uses, from its dependencies and its lock.
    pub mui: Vec<String>,
    /// MUI dependencies pinned to a `rev` (see [`pinned`]).
    pub pinned: Vec<String>,
    /// Where its editor is made: `src/editor.rs:232`.
    pub editor: Option<String>,
    /// What it lacks for the generic adapter, each an exact instruction.
    pub missing: Vec<String>,
    workspace_root: PathBuf,
}

impl Plugin {
    /// The report `mui-cut add` prints and `plugin_add` returns.
    pub fn report(&self) -> Value {
        json!({
            "package": self.package,
            "dir": self.dir,
            "framework": self.framework.name(),
            "entry": self.entry,
            "editor": self.editor,
            "mui": self.mui,
            "pinned": self.pinned,
            "missing": self.missing,
        })
    }
}

/// Read the plugin crate at `dir` (a package, or a workspace with one
/// plugin package in it).
pub fn detect(dir: &Path) -> Result<Plugin> {
    let dir = dir
        .canonicalize()
        .map_err(|e| format!("{}: {e}", dir.display()))?;
    let manifest = dir.join("Cargo.toml");
    let out = cargo(&dir)
        .args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--manifest-path",
        ])
        .arg(&manifest)
        .output()
        .map_err(|e| format!("cargo: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "cargo metadata {}: {}",
            manifest.display(),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let meta: Value = serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())?;
    let packages = meta["packages"].as_array().cloned().unwrap_or_default();
    let deps = |p: &Value| p["dependencies"].as_array().cloned().unwrap_or_default();
    // The package at `dir`, or the one plugin among the workspace's.
    let pkg = match packages
        .iter()
        .find(|p| Path::new(p["manifest_path"].as_str().unwrap_or("")) == manifest)
    {
        Some(p) => p.clone(),
        None => {
            let plugins: Vec<&Value> = packages
                .iter()
                .filter(|p| framework(&deps(p)).is_some())
                .collect();
            match plugins[..] {
                [one] => one.clone(),
                [] => return Err(format!("{}: no package uses MUI", dir.display())),
                _ => {
                    let names: Vec<&str> =
                        plugins.iter().filter_map(|p| p["name"].as_str()).collect();
                    return Err(format!(
                        "{}: several plugins ({}); add one's folder",
                        dir.display(),
                        names.join(", ")
                    ));
                }
            }
        }
    };
    let pkg_deps = deps(&pkg);
    let package = pkg["name"].as_str().unwrap_or_default().to_owned();
    let fw = framework(&pkg_deps).ok_or_else(|| format!("`{package}` does not use MUI"))?;
    let pkg_dir = Path::new(pkg["manifest_path"].as_str().unwrap_or(""))
        .parent()
        .unwrap_or(&dir)
        .to_path_buf();
    let workspace_root = PathBuf::from(meta["workspace_root"].as_str().unwrap_or(""));
    let mut missing = Vec::new();
    let lib = pkg["targets"].as_array().into_iter().flatten().find(|t| {
        t["kind"]
            .as_array()
            .is_some_and(|k| k.iter().any(|k| k == "lib" || k == "rlib"))
    });
    if lib.is_none() {
        missing.push(format!(
            "`{package}` has no linkable library: add `\"rlib\"` to `[lib] crate-type` in its Cargo.toml"
        ));
    }
    let src = sources(&pkg_dir.join("src"));
    let (entry, editor) = entry(fw, &src, &mut missing);
    let lock = std::fs::read_to_string(workspace_root.join("Cargo.lock")).unwrap_or_default();
    let mut mui: Vec<String> = pkg_deps
        .iter()
        .filter(|d| is_mui(d["source"].as_str().unwrap_or("")))
        .filter_map(|d| d["name"].as_str().map(str::to_owned))
        .chain(locked_mui(&lock))
        .collect();
    mui.sort();
    mui.dedup();
    let pinned = std::fs::read_to_string(pkg_dir.join("Cargo.toml"))
        .map(|t| pinned(&t))
        .unwrap_or_default();
    Ok(Plugin {
        dir: pkg_dir,
        package,
        framework: fw,
        framework_dep: fw
            .krate()
            .and_then(|k| pkg_deps.iter().find(|d| d["name"] == k).cloned()),
        entry,
        mui,
        pinned,
        editor,
        missing,
        workspace_root,
    })
}

fn is_mui(source: &str) -> bool {
    source
        .strip_prefix("git+")
        .and_then(|s| s.strip_prefix(MUI_GIT))
        .is_some_and(|rest| rest.is_empty() || rest.starts_with(['?', '#', '/']) || rest == ".git")
}

/// The MUI packages a `Cargo.lock` names.
fn locked_mui(lock: &str) -> Vec<String> {
    lock.split("[[package]]")
        .filter(|b| {
            b.lines()
                .any(|l| l.strip_prefix("source = \"").is_some_and(is_mui))
        })
        .filter_map(|b| {
            b.lines()
                .find_map(|l| l.strip_prefix("name = \"")?.strip_suffix('"'))
                .map(str::to_owned)
        })
        .collect()
}

/// The MUI dependencies a manifest pins with `rev =`: plugins follow MUI's
/// main branch (`tools/mui-sync`), and a pin freezes one in the past.
pub fn pinned(manifest: &str) -> Vec<String> {
    manifest
        .lines()
        .map(str::trim)
        .filter(|l| !l.starts_with('#') && l.contains(MUI_GIT) && l.contains("rev"))
        .filter(|l| {
            l.split([',', '{'])
                .any(|kv| kv.split('=').next().is_some_and(|k| k.trim() == "rev"))
        })
        .filter_map(|l| l.split('=').next().map(|k| k.trim().to_owned()))
        .collect()
}

/// Every `.rs` under `dir`, with its text: `(path relative to the
/// package, text)`.
fn sources(dir: &Path) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "rs")
                && let Ok(text) = std::fs::read_to_string(&p)
            {
                let rel = p
                    .strip_prefix(dir.parent().unwrap_or(dir))
                    .unwrap_or(&p)
                    .to_string_lossy()
                    .into_owned();
                out.push((rel, text));
            }
        }
    }
    out.sort();
    out
}

/// The first line of `src` where one of `needles` appears: `file:line`.
fn find(src: &[(String, String)], needles: &[&str]) -> Option<String> {
    src.iter().find_map(|(f, text)| {
        text.lines()
            .position(|l| needles.iter().any(|n| l.contains(n)))
            .map(|i| format!("{f}:{}", i + 1))
    })
}

/// The type the adapter opens, and where the editor is made; what is
/// missing goes to `missing`.
fn entry(
    fw: Framework,
    src: &[(String, String)],
    missing: &mut Vec<String>,
) -> (String, Option<String>) {
    let editor = find(
        src,
        &[
            "MuiEditor::new",
            "mui_baseview::open",
            "window::open(",
            "fn mui_editor(",
        ],
    );
    let lib = src
        .iter()
        .find(|(f, _)| f == "src/lib.rs")
        .map_or("", |(_, t)| t.as_str());
    let entry = match fw {
        Framework::Moose | Framework::Truce => {
            let mac = format!("{}::plugin!", fw.name());
            if find(src, &[&mac]).is_none() {
                missing.push(format!(
                    "no `{mac} {{ logic: …, params: … }}`: it makes the `Plugin` type the adapter opens"
                ));
            }
            "Plugin".to_owned()
        }
        Framework::NicePlug => {
            // `nice_export_clap!(Type)`: the type, public at the crate root.
            let ty = lib.lines().find_map(|l| {
                let rest = l.trim().strip_prefix("nice_export_clap!(")?;
                Some(rest.split(')').next()?.trim().to_owned())
            });
            match ty {
                Some(ty) if ty.contains("::") || !public(lib, &ty) => {
                    let last = ty.rsplit("::").next().unwrap_or(&ty).to_owned();
                    missing.push(format!(
                        "make the plugin type public at the crate root: `pub use {}{ty};` in src/lib.rs",
                        if ty.starts_with("crate::") { "" } else { "crate::" },
                    ));
                    last
                }
                Some(ty) => ty,
                None => {
                    missing.push("no `nice_export_clap!(Type)` in src/lib.rs".into());
                    String::new()
                }
            }
        }
        Framework::Mui => {
            if find(src, &["pub fn mui_editor("]).is_none() {
                missing.push(
                    "export the editor from src/lib.rs: `pub fn mui_editor() -> (mui::Ui, (u32, u32), impl mui::host::View + Send + 'static)` (its Ui, its size in points, its view)"
                        .into(),
                );
            }
            "mui_editor".to_owned()
        }
    };
    (entry, editor)
}

/// Whether `lib` makes `ty` public at its root.
fn public(lib: &str, ty: &str) -> bool {
    lib.lines().map(str::trim).any(|l| {
        (l.starts_with("pub struct ") || l.starts_with("pub enum "))
            && l.split_whitespace()
                .nth(2)
                .is_some_and(|n| n.trim_end_matches(['{', ';', '(']) == ty)
            || l.starts_with("pub use ")
                && (l.ends_with(&format!(" as {ty};")) || l.ends_with(&format!("::{ty};")))
    })
}

/// This MUI tree: `MUI_ROOT`, or the one mui-cut was built from.
pub fn mui_root() -> PathBuf {
    let root = std::env::var_os("MUI_ROOT").map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."),
        PathBuf::from,
    );
    // Cargo finds a path dependency's workspace by its path: no `..`.
    root.canonicalize().unwrap_or(root)
}

/// Every package in the MUI tree, by name: `crates/*`, `media/*` and
/// `vendor/*` (a git dependency on MUI finds those too).
pub fn local_crates(root: &Path) -> BTreeMap<String, PathBuf> {
    let mut out = BTreeMap::new();
    for group in ["crates", "media", "vendor"] {
        let Ok(rd) = std::fs::read_dir(root.join(group)) else {
            continue;
        };
        for e in rd.flatten() {
            let Ok(text) = std::fs::read_to_string(e.path().join("Cargo.toml")) else {
                continue;
            };
            let name = text
                .split("[package]")
                .nth(1)
                .and_then(|s| s.lines().find_map(|l| l.trim().strip_prefix("name = \"")))
                .and_then(|n| n.strip_suffix('"'));
            if let Some(n) = name {
                out.insert(n.to_owned(), e.path());
            }
        }
    }
    out
}

/// The `--config` values that send each of `used` to the local tree.
pub fn patch_config(used: &[String], local: &BTreeMap<String, PathBuf>) -> Result<Vec<String>> {
    used.iter()
        .map(|c| {
            let dir = local.get(c).ok_or_else(|| {
                format!("the plugin uses MUI crate `{c}`, which this MUI tree lacks")
            })?;
            let dir = dir.canonicalize().unwrap_or_else(|_| dir.clone());
            Ok(format!(
                "patch.\"{MUI_GIT}\".{c}.path={}",
                Value::from(dir.to_string_lossy())
            ))
        })
        .collect()
}

/// mui-cut's cache: `MUI_CUT_CACHE`, or `$XDG_CACHE_HOME/mui-cut`.
pub fn cache() -> PathBuf {
    let env = |k| {
        std::env::var_os(k)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    env("MUI_CUT_CACHE")
        .or_else(|| env("XDG_CACHE_HOME").map(|d| d.join("mui-cut")))
        .or_else(|| env("HOME").map(|d| d.join(".cache/mui-cut")))
        .unwrap_or_else(|| PathBuf::from(".mui-cut-cache"))
}

/// Whether a source names a git repository rather than a folder.
pub fn is_git(s: &str) -> bool {
    s.contains("://") || s.starts_with("git@")
}

/// A git repository's checkout under the cache, cloned on first use;
/// `update` pulls it to the remote's latest.
pub fn checkout(url: &str, update: bool) -> Result<PathBuf> {
    let name = url
        .trim_end_matches('/')
        .trim_end_matches(".git")
        .rsplit(['/', ':'])
        .next()
        .unwrap_or("plugin");
    let dir = cache().join("src").join(format!(
        "{name}-{:08x}",
        mui_cut::plugin::fnv(mui_cut::plugin::FNV_OFFSET, url.as_bytes()) as u32
    ));
    let git = |args: &[&str], cwd: &Path| -> Result<()> {
        let out = Command::new("git")
            .args(args)
            .current_dir(cwd)
            .output()
            .map_err(|e| format!("git: {e}"))?;
        if out.status.success() {
            Ok(())
        } else {
            Err(format!(
                "git {}: {}",
                args.join(" "),
                String::from_utf8_lossy(&out.stderr).trim()
            ))
        }
    };
    if !dir.join(".git").is_dir() {
        std::fs::create_dir_all(dir.parent().unwrap_or(&dir)).map_err(|e| e.to_string())?;
        let d = dir.to_string_lossy();
        git(
            &["clone", "--depth", "1", url, &d],
            dir.parent().unwrap_or(&dir),
        )?;
    } else if update {
        git(&["pull", "--ff-only", "--depth", "1"], &dir)?;
    }
    Ok(dir)
}

/// The plugin a source's `plugin` names: a folder relative to `project_dir`,
/// or a git URL.
pub fn resolve(plugin: &str, project_dir: &Path) -> Result<PathBuf> {
    if is_git(plugin) {
        checkout(plugin, false)
    } else {
        Ok(project_dir.join(plugin))
    }
}

/// Build the generic adapter for the plugin a source names; its executable.
pub fn adapter(plugin: &str, project_dir: &Path) -> Result<PathBuf> {
    let p = detect(&resolve(plugin, project_dir)?)?;
    if !p.missing.is_empty() {
        return Err(format!(
            "`{}` needs, for the generic adapter:\n  - {}",
            p.package,
            p.missing.join("\n  - ")
        ));
    }
    let root = mui_root();
    let local = local_crates(&root);
    let mut used = p.mui.clone();
    // The adapter's own MUI (the headless hook is in it).
    used.push("mui".into());
    used.sort();
    used.dedup();
    let config = patch_config(&used, &local)?;
    let dir = write_adapter(&p, &root, &used)?;
    let name = adapter_name(&p);
    let mut cmd = cargo(&dir);
    cmd.args([
        "build",
        "--message-format=json-render-diagnostics",
        "--bin",
        &name,
        "--manifest-path",
    ])
    .arg(dir.join("Cargo.toml"));
    for c in &config {
        cmd.args(["--config", c]);
    }
    if std::env::var_os("CARGO_TARGET_DIR").is_none() {
        cmd.arg("--target-dir").arg(cache().join("target"));
    }
    eprintln!(
        "mui-cut: building the adapter for `{}` against {}",
        p.package,
        root.display()
    );
    let out = cmd
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("cargo: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "building the adapter for `{}` failed:\n{}",
            p.package,
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(|m| m["reason"] == "compiler-artifact" && m["target"]["name"] == name.as_str())
        .find_map(|m| m["executable"].as_str().map(PathBuf::from))
        .ok_or_else(|| "cargo built no adapter".into())
}

/// Cargo in `dir`, with the toolchain `dir` asks for rather than the one
/// mui-cut runs under.
fn cargo(dir: &Path) -> Command {
    let mut c = Command::new("cargo");
    c.current_dir(dir).env_remove("RUSTUP_TOOLCHAIN");
    c
}

/// The adapter's package, binary and directory name: one per plugin
/// folder, so adapters sharing a target directory never overwrite another.
fn adapter_name(p: &Plugin) -> String {
    let hash = mui_cut::plugin::fnv(
        mui_cut::plugin::FNV_OFFSET,
        p.dir.to_string_lossy().as_bytes(),
    );
    format!("adapter-{}-{:08x}", p.package, hash as u32)
}

/// Write the adapter workspace for `p`; its directory.
fn write_adapter(p: &Plugin, root: &Path, patched: &[String]) -> Result<PathBuf> {
    let dir = cache().join("adapters").join(adapter_name(p));
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let write = |name: &str, text: &str| {
        let f = dir.join(name);
        // Unchanged files keep their times: Cargo rebuilds nothing.
        if std::fs::read_to_string(&f).ok().as_deref() == Some(text) {
            return Ok(());
        }
        std::fs::write(&f, text).map_err(|e| format!("{}: {e}", f.display()))
    };
    write("Cargo.toml", &manifest(p, root, patched)?)?;
    write("main.rs", &main_rs(p))?;
    // The plugin's versions for everything but MUI, and its toolchain.
    for f in ["Cargo.lock", "rust-toolchain.toml", "rust-toolchain"] {
        match std::fs::read_to_string(p.workspace_root.join(f)) {
            Ok(text) => write(f, &text)?,
            Err(_) if f != "Cargo.lock" => {
                let _ = std::fs::remove_file(dir.join(f));
            }
            Err(_) => {}
        }
    }
    Ok(dir)
}

fn toml_str(s: &str) -> String {
    Value::from(s).to_string()
}

/// The adapter's Cargo.toml.
fn manifest(p: &Plugin, root: &Path, patched: &[String]) -> Result<String> {
    let path = |d: &Path| toml_str(&d.to_string_lossy());
    let mut deps = vec![
        format!(
            "plugin = {{ package = {}, path = {} }}",
            toml_str(&p.package),
            path(&p.dir)
        ),
        format!(
            "mui-motion-bridge = {{ path = {} }}",
            path(&root.join("media/mui-motion-bridge"))
        ),
        "serde_json = \"1\"".into(),
    ];
    if let Some(d) = &p.framework_dep {
        deps.push(format!(
            "{} = {}",
            d["name"].as_str().unwrap_or(""),
            dep_spec(d, p)?
        ));
    }
    let root_manifest =
        std::fs::read_to_string(p.workspace_root.join("Cargo.toml")).unwrap_or_default();
    Ok(format!(
        "# Generated by mui-cut for `{pkg}` ({dir}); rewritten on every build.\n\
         [package]\nname = \"{name}\"\nversion = \"0.0.0\"\nedition = \"2024\"\npublish = false\n\n\
         [[bin]]\nname = \"{name}\"\npath = \"main.rs\"\n\n\
         [dependencies]\n{deps}\n\n\
         [workspace]\n\n\
         # Optimised dependencies (the capture rasterises on the CPU), no debug info.\n\
         [profile.dev]\ndebug = 0\n[profile.dev.package.\"*\"]\nopt-level = 3\n\n{patches}",
        pkg = p.package,
        name = adapter_name(p),
        dir = p.dir.display(),
        deps = deps.join("\n"),
        patches = patches(&root_manifest, &p.workspace_root, patched),
    ))
}

/// A dependency as `cargo metadata` lists it, as a Cargo.toml table:
/// the same source and version, no default features.
fn dep_spec(d: &Value, p: &Plugin) -> Result<String> {
    let mut kv = vec!["default-features = false".to_owned()];
    if let Some(path) = d["path"].as_str() {
        kv.push(format!("path = {}", toml_str(path)));
    } else if let Some(git) = d["source"].as_str().and_then(|s| s.strip_prefix("git+")) {
        let (url, query) = git.split_once('?').unwrap_or((git, ""));
        kv.push(format!(
            "git = {}",
            toml_str(url.split('#').next().unwrap_or(url))
        ));
        for q in query.split(['&', '#']).filter(|q| !q.is_empty()) {
            if let Some((k @ ("rev" | "branch" | "tag"), v)) = q.split_once('=') {
                kv.push(format!("{k} = {}", toml_str(v)));
            }
        }
    } else if let Some(req) = d["req"].as_str() {
        kv.push(format!("version = {}", toml_str(req)));
    } else {
        return Err(format!(
            "`{}`: cannot read its `{}` dependency",
            p.package, d["name"]
        ));
    }
    Ok(format!("{{ {} }}", kv.join(", ")))
}

/// The workspace's `[patch.*]` tables, paths made absolute, less the MUI
/// crates the local tree replaces.
fn patches(manifest: &str, root: &Path, patched: &[String]) -> String {
    let mut out = String::new();
    let mut section: Option<bool> = None; // Some(is a MUI patch)
    for line in manifest.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            section = t.starts_with("[patch").then(|| t.contains(MUI_GIT));
            if section.is_some() {
                out.push_str(t);
                out.push('\n');
            }
            continue;
        }
        let Some(mui) = section else { continue };
        let key = t.split('=').next().unwrap_or("").trim();
        if t.is_empty() || t.starts_with('#') || mui && patched.iter().any(|c| c == key) {
            continue;
        }
        out.push_str(&absolute_paths(t, root));
        out.push('\n');
    }
    out
}

/// `path = "rel"` inside `line`, made absolute against `root`.
fn absolute_paths(line: &str, root: &Path) -> String {
    let Some(i) = line.find("path = \"") else {
        return line.to_owned();
    };
    let start = i + "path = \"".len();
    let Some(len) = line[start..].find('"') else {
        return line.to_owned();
    };
    let rel = &line[start..start + len];
    let abs = root.join(rel);
    format!(
        "{}path = {}{}",
        &line[..i],
        toml_str(&abs.to_string_lossy()),
        &line[start + len + 1..]
    )
}

/// The adapter's `main.rs` for `p`'s framework.
fn main_rs(p: &Plugin) -> String {
    let body = match p.framework {
        Framework::Moose | Framework::Truce => include_str!("adapter/moose.rs"),
        Framework::NicePlug => include_str!("adapter/nice.rs"),
        Framework::Mui => include_str!("adapter/mui.rs"),
    };
    format!(
        "//! Generated by mui-cut: `{pkg}`'s editor, headless, for mui-cut.\n{}",
        body.replace("FRAMEWORK", &p.framework.name().replace('-', "_"))
            .replace("ENTRY", &p.entry)
            .replace("NAME", &p.package),
        pkg = p.package,
    )
}

/// `mui-cut add`, up to the project: read the plugin at `from` (a folder,
/// relative to `base`, or a git URL, pulled to its latest) and make its
/// `sources` entry for a project in `project_dir`, named `id` or after
/// the package, unique among `taken`. `(entry, report)`; an error says
/// what the plugin lacks.
pub fn onboard(
    from: &str,
    base: &Path,
    project_dir: &Path,
    id: Option<&str>,
    taken: &[&str],
) -> Result<(Value, Value)> {
    let (dir, plugin) = if is_git(from) {
        (checkout(from, true)?, from.to_owned())
    } else {
        let dir = base.join(from);
        let rel = relative(&dir, project_dir)?;
        (dir, rel)
    };
    let p = detect(&dir)?;
    if !p.missing.is_empty() {
        return Err(format!(
            "`{}` ({}) needs, for mui-cut:\n  - {}",
            p.package,
            p.framework.name(),
            p.missing.join("\n  - ")
        ));
    }
    let stem = id.unwrap_or(&p.package).to_owned();
    let mut id = stem.clone();
    let mut n = 2;
    while taken.contains(&id.as_str()) {
        id = format!("{stem} {n}");
        n += 1;
    }
    let entry = json!({"id": id, "kind": "plugin", "source": {"plugin": plugin}});
    Ok((entry, p.report()))
}

/// `to` from `from` (both folders), with `..` where needed.
fn relative(to: &Path, from: &Path) -> Result<String> {
    let abs = |p: &Path| {
        p.canonicalize()
            .map_err(|e| format!("{}: {e}", p.display()))
    };
    let (to, from) = (abs(to)?, abs(from)?);
    let common = to
        .components()
        .zip(from.components())
        .take_while(|(a, b)| a == b)
        .count();
    let mut out = PathBuf::new();
    for _ in from.components().skip(common) {
        out.push("..");
    }
    out.extend(to.components().skip(common));
    let s = out.to_string_lossy().replace('\\', "/");
    Ok(if s.is_empty() { ".".into() } else { s })
}

/// Add the plugin at `from` to the project at `project` (written), build
/// its adapter and capture it: the report, with the source's `id` and its
/// discovered part tree (`parts`).
pub fn add(project: &Path, from: &str, base: &Path, id: Option<&str>) -> Result<Value> {
    let text =
        std::fs::read_to_string(project).map_err(|e| format!("{}: {e}", project.display()))?;
    let mut raw: Value =
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", project.display()))?;
    let dir = project
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let p = mui_cut::Project::load(&text)?;
    let all = p.all_sources();
    let taken: Vec<&str> = all.iter().map(|m| m.id.as_str()).collect();
    let (entry, mut report) = onboard(from, base, dir, id, &taken)?;
    raw.as_object_mut()
        .ok_or("the project is not an object")?
        .entry("sources")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .ok_or("`sources` is not a list")?
        .push(entry.clone());
    let p = mui_cut::Project::load(&raw.to_string())?;
    crate::write_atomic(project, &p.to_json())?;
    report["id"] = entry["id"].clone();
    report["source"] = entry["source"].clone();
    report["parts"] = parts(&p, project, entry["id"].as_str().unwrap_or(""))?;
    Ok(report)
}

/// A plugin source's part tree, capturing it first if needed.
pub fn parts(p: &mui_cut::Project, project: &Path, id: &str) -> Result<Value> {
    let errs = crate::host::capture_sources(p, project);
    let all = p.all_sources();
    let m = all
        .iter()
        .find(|m| m.id == id)
        .ok_or_else(|| format!("no source `{id}`"))?;
    let state = m.state().ok_or_else(|| format!("`{id}` is not a plugin"))?;
    let file = project
        .parent()
        .unwrap_or(Path::new("."))
        .join(mui_cut::plugin::CACHE)
        .join(format!("{state}.json"));
    let cap: mui_cut::Capture = std::fs::read(&file)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .ok_or_else(|| format!("`{id}` was added but not captured:\n{}", errs.join("\n")))?;
    Ok(mui_cut::plugin::home_tree(&cap, &state))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patches_each_used_crate_to_the_local_tree() {
        let root = mui_root();
        let local = local_crates(&root);
        assert!(local.contains_key("mui") && local.contains_key("mui-baseview"));
        let cfg = patch_config(&["mui".into(), "mui-text".into()], &local).unwrap();
        let mui = local["mui"].canonicalize().unwrap();
        assert_eq!(
            cfg[0],
            format!(
                "patch.\"https://github.com/Matari-Audio/MUI\".mui.path=\"{}\"",
                mui.display()
            )
        );
        assert!(cfg[1].contains(".mui-text.path="));
        let e = patch_config(&["mui-nope".into()], &local).unwrap_err();
        assert!(e.contains("mui-nope"), "{e}");
    }

    #[test]
    fn finds_mui_crates_in_a_lock_and_rev_pins_in_a_manifest() {
        let lock = "[[package]]\nname = \"mui\"\nversion = \"0.4.0\"\nsource = \"git+https://github.com/Matari-Audio/MUI#abc\"\n\n\
                    [[package]]\nname = \"moose\"\nversion = \"0.1.0\"\nsource = \"git+https://github.com/Matari-Audio/moose#def\"\n\n\
                    [[package]]\nname = \"mui-baseview\"\nversion = \"0.4.0\"\nsource = \"git+https://github.com/Matari-Audio/MUI?rev=48dde27#48dde27\"\n";
        assert_eq!(locked_mui(lock), ["mui", "mui-baseview"]);
        let manifest = r#"
mui = { git = "https://github.com/Matari-Audio/MUI", rev = "48dde277444db4c4b9fa1099ecd52f997a18402a", features = ["cpu"] }
mui-text = { git = "https://github.com/Matari-Audio/MUI" }
# mui-old = { git = "https://github.com/Matari-Audio/MUI", rev = "1" }
moose = { git = "https://github.com/Matari-Audio/moose", rev = "48879b1" }
mui-truce = { git = "https://github.com/Matari-Audio/MUI", branch = "revamp" }
"#;
        assert_eq!(pinned(manifest), ["mui"]);
    }

    #[test]
    fn copies_the_plugins_patches_but_not_over_local_mui() {
        let manifest = "[package]\nname = \"k\"\n\n[patch.\"https://github.com/Matari-Audio/moose\"]\nmoose-mui = { path = \"vendor/moose-mui\" }\n\n\
                        # a fork\n[patch.\"https://github.com/Matari-Audio/MUI\"]\nvello = { path = \"vendor/vello\" }\nmui-baseview = { path = \"vendor/mui-baseview\" }\n\n[workspace]\nmembers = [\".\"]\n";
        let out = patches(manifest, Path::new("/k"), &["mui-baseview".into()]);
        assert_eq!(
            out,
            "[patch.\"https://github.com/Matari-Audio/moose\"]\nmoose-mui = { path = \"/k/vendor/moose-mui\" }\n\
             [patch.\"https://github.com/Matari-Audio/MUI\"]\nvello = { path = \"/k/vendor/vello\" }\n"
        );
    }

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }

    #[test]
    fn detects_each_framework_and_what_it_lacks() {
        for (dir, fw, entry) in [
            ("moose", Framework::Moose, "Plugin"),
            ("truce", Framework::Truce, "Plugin"),
            ("nice", Framework::NicePlug, "Synth"),
            ("plain", Framework::Mui, "mui_editor"),
        ] {
            let p = detect(&fixture(dir)).unwrap_or_else(|e| panic!("{dir}: {e}"));
            assert_eq!((p.framework, p.entry.as_str()), (fw, entry), "{dir}");
            assert!(p.missing.is_empty(), "{dir}: {:?}", p.missing);
            assert!(p.mui.contains(&"mui".to_owned()), "{dir}: {:?}", p.mui);
            assert!(p.editor.is_some(), "{dir}");
        }
        // A nice-plug type behind a private path, and a plain crate without
        // its one function: each says exactly what to add.
        let p = detect(&fixture("nice-private")).unwrap();
        assert_eq!(
            p.missing,
            [
                "make the plugin type public at the crate root: `pub use crate::synth::Synth;` in src/lib.rs"
            ]
        );
        let p = detect(&fixture("plain-bare")).unwrap();
        assert!(
            p.missing[0].contains("pub fn mui_editor()"),
            "{:?}",
            p.missing
        );
        let deps =
            |names: &[&str]| -> Vec<Value> { names.iter().map(|n| json!({"name": n})).collect() };
        assert_eq!(framework(&deps(&["serde"])), None);
        assert_eq!(framework(&deps(&["mui", "truce"])), Some(Framework::Truce));
    }
}

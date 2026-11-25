//! Lowers MUI's WGSL to SPIR-V, MSL and HLSL SM 5.0 at build time and writes
//! `$OUT_DIR/shaders.rs`: the artifacts plus the binding reflection every
//! backend of `mui-hal` binds by. naga never reaches the runtime crate.
//!
//! Precompiled blobs & hash-checked manifest:
//! - If committed precompiled blobs exist under `prebuilt/dxbc/` or
//!   `prebuilt/metallib/` alongside `manifest.sha256`, every entry's SHA-256
//!   (both source-input hash and blob-content hash) is verified on EVERY host.
//!   Any hash mismatch or missing blob fails the build loudly to prevent silent
//!   stale-shader drift. Verified blobs are embedded on the matching target OS
//!   (including Linux -> Windows `x86_64-pc-windows-gnu` / `-msvc` cross-builds).
//! - When no prebuilt directory exists yet, native compilers run when the build
//!   host matches the target OS: FXC on Windows (required; Windows SDK or
//!   `MUI_FXC`) produces SM 5.0 DXBC; `xcrun metal` + `xcrun metallib` on macOS
//!   produces `.metallib`. Cross-builds without prebuilt blobs fall back to
//!   runtime compilation (`D3DCompile` / `newLibraryWithSource`).
//! - Setting `MUI_SHADERS_EMIT_PREBUILT=<dir>` writes the freshly compiled
//!   native DXBC / `.metallib` blobs and deterministic `manifest.sha256` files
//!   into `<dir>/{dxbc,metallib}/` for CI refresh workflows. Emission ignores
//!   the committed manifests (it is how stale ones get regenerated after a
//!   shader edit); normal builds keep verifying them strictly. Cargo caches
//!   build-script runs when env and tracked inputs are unchanged, so run
//!   `cargo clean -p mui-shaders` (or use a fresh target directory) before
//!   repeating emission to a deleted directory with the same path.
//!
//! Slot rule, shared by Metal and D3D11: per shader, resources in
//! (group, binding) order take consecutive indices per class, buffers from 0,
//! textures from 0, samplers from 0. So a binding's `msl_slot` equals its
//! `hlsl_register` (b#, t# or s#), and Vulkan keeps the WGSL set/binding.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::{env, fs};

use naga::back::{glsl, hlsl, msl, spv};
use naga::valid::{Capabilities, ValidationFlags, Validator};
use naga::{AddressSpace, ImageClass, ImageDimension, ResourceBinding, ScalarKind, TypeInner};

const SHADERS: [(&str, &str); 4] = [
    ("Quad", "quad"),
    ("Blur", "blur"),
    ("Weld", "weld"),
    ("Present", "present"),
];

const MANIFEST_FILE: &str = "manifest.sha256";

#[derive(Clone, Copy, PartialEq)]
enum Class {
    Buffer,
    Texture,
    Sampler,
}

struct Binding {
    rb: ResourceBinding,
    name: String,
    kind: &'static str,
    class: Class,
    slot: u32,
    uniform_type: Option<naga::Handle<naga::Type>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ManifestEntry {
    src_sha256: String,
    blob_sha256: String,
}

/// Pure-Rust FIPS 180-4 SHA-256 (zero external build-dependencies).
fn sha256_hex(parts: &[&[u8]]) -> String {
    const K: [u32; 64] = [
        0x428a_2f98,
        0x7137_4491,
        0xb5c0_fbcf,
        0xe9b5_dba5,
        0x3956_c25b,
        0x59f1_11f1,
        0x923f_82a4,
        0xab1c_5ed5,
        0xd807_aa98,
        0x1283_5b01,
        0x2431_85be,
        0x550c_7dc3,
        0x72be_5d74,
        0x80de_b1fe,
        0x9bdc_06a7,
        0xc19b_f174,
        0xe49b_69c1,
        0xefbe_4786,
        0x0fc1_9dc6,
        0x240c_a1cc,
        0x2de9_2c6f,
        0x4a74_84aa,
        0x5cb0_a9dc,
        0x76f9_88da,
        0x983e_5152,
        0xa831_c66d,
        0xb003_27c8,
        0xbf59_7fc7,
        0xc6e0_0bf3,
        0xd5a7_9147,
        0x06ca_6351,
        0x1429_2967,
        0x27b7_0a85,
        0x2e1b_2138,
        0x4d2c_6dfc,
        0x5338_0d13,
        0x650a_7354,
        0x766a_0abb,
        0x81c2_c92e,
        0x9272_2c85,
        0xa2bf_e8a1,
        0xa81a_664b,
        0xc24b_8b70,
        0xc76c_51a3,
        0xd192_e819,
        0xd699_0624,
        0xf40e_3585,
        0x106a_a070,
        0x19a4_c116,
        0x1e37_6c08,
        0x2748_774c,
        0x34b0_bcb5,
        0x391c_0cb3,
        0x4ed8_aa4a,
        0x5b9c_ca4f,
        0x682e_6ff3,
        0x748f_82ee,
        0x78a5_636f,
        0x84c8_7814,
        0x8cc7_0208,
        0x90be_fffa,
        0xa450_6ceb,
        0xbef9_a3f7,
        0xc671_78f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09_e667,
        0xbb67_ae85,
        0x3c6e_f372,
        0xa54f_f53a,
        0x510e_527f,
        0x9b05_688c,
        0x1f83_d9ab,
        0x5be0_cd19,
    ];
    let total_len: usize = parts.iter().map(|p| p.len()).sum();
    let mut msg = Vec::with_capacity(total_len + 72);
    for p in parts {
        msg.extend_from_slice(p);
    }
    let bit_len = (total_len as u64).wrapping_mul(8);
    msg.push(0x80);
    while (msg.len() % 64) != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    for block in msg.as_chunks::<64>().0 {
        let mut w = [0u32; 64];
        for (i, chunk) in block.as_chunks::<4>().0.iter().enumerate() {
            w[i] = u32::from_be_bytes(*chunk);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }

    let mut hex = String::with_capacity(64);
    for word in h {
        let _ = write!(hex, "{word:08x}");
    }
    hex
}

fn parse_manifest(dir: &Path) -> Option<BTreeMap<String, ManifestEntry>> {
    if !dir.is_dir() {
        return None;
    }
    println!("cargo:rerun-if-changed={}", dir.display());
    let manifest_path = dir.join(MANIFEST_FILE);
    let text = fs::read_to_string(&manifest_path).unwrap_or_else(|e| {
        panic!(
            "mui-shaders: prebuilt directory {} exists but {} cannot be read: {e}",
            dir.display(),
            manifest_path.display()
        )
    });
    println!("cargo:rerun-if-changed={}", manifest_path.display());
    let mut map = BTreeMap::new();
    for (lineno, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        assert!(
            parts.len() == 3,
            "mui-shaders: malformed line {} in {}: expected `src:<hex> blob:<hex> <file>`",
            lineno + 1,
            manifest_path.display()
        );
        let src_sha256 = parts[0].strip_prefix("src:").unwrap_or_else(|| {
            panic!(
                "mui-shaders: line {} in {} missing `src:` prefix",
                lineno + 1,
                manifest_path.display()
            )
        });
        let blob_sha256 = parts[1].strip_prefix("blob:").unwrap_or_else(|| {
            panic!(
                "mui-shaders: line {} in {} missing `blob:` prefix",
                lineno + 1,
                manifest_path.display()
            )
        });
        assert!(
            src_sha256.len() == 64 && blob_sha256.len() == 64,
            "mui-shaders: line {} in {} has invalid SHA-256 hex length",
            lineno + 1,
            manifest_path.display()
        );
        let file = parts[2].to_owned();
        let prev = map.insert(
            file.clone(),
            ManifestEntry {
                src_sha256: src_sha256.to_owned(),
                blob_sha256: blob_sha256.to_owned(),
            },
        );
        assert!(
            prev.is_none(),
            "mui-shaders: duplicate manifest entry `{file}` in {}",
            manifest_path.display()
        );
    }
    Some(map)
}

fn verify_prebuilt_blob(
    dir: &Path,
    manifest: &BTreeMap<String, ManifestEntry>,
    file: &str,
    expected_src_sha256: &str,
    magic: &[u8; 4],
) -> PathBuf {
    let entry = manifest.get(file).unwrap_or_else(|| {
        panic!(
            "mui-shaders: prebuilt manifest in {} is missing `{file}`; regenerate with \
             MUI_SHADERS_EMIT_PREBUILT=prebuilt",
            dir.display()
        )
    });
    assert!(
        entry.src_sha256 == expected_src_sha256,
        "mui-shaders: STALE prebuilt shader `{file}` in {}!\n  \
         manifest src sha256: {}\n  \
         current  src sha256: {expected_src_sha256}\n\
         Regenerate prebuilt blobs on Windows/macOS with MUI_SHADERS_EMIT_PREBUILT.",
        dir.display(),
        entry.src_sha256
    );
    let blob_path = dir.join(file);
    println!("cargo:rerun-if-changed={}", blob_path.display());
    let bytes = fs::read(&blob_path).unwrap_or_else(|e| {
        panic!(
            "mui-shaders: prebuilt blob {} missing or unreadable: {e}",
            blob_path.display()
        )
    });
    assert!(
        bytes.len() >= 4 && &bytes[..4] == magic,
        "mui-shaders: prebuilt blob {} failed {:?} magic check",
        blob_path.display(),
        std::str::from_utf8(magic).unwrap_or("????")
    );
    let actual_blob_sha256 = sha256_hex(&[&bytes]);
    assert!(
        actual_blob_sha256 == entry.blob_sha256,
        "mui-shaders: CORRUPT prebuilt blob `{file}` in {}!\n  \
         manifest blob sha256: {}\n  \
         actual   blob sha256: {actual_blob_sha256}",
        dir.display(),
        entry.blob_sha256
    );
    blob_path
}

/// Returns `true` iff `dir` holds a parseable manifest that lists every file in
/// `required`, and every listed blob is present and matches its recorded SHA-256.
fn is_complete_generation<'a>(dir: &Path, required: impl IntoIterator<Item = &'a String>) -> bool {
    let Ok(text) = fs::read_to_string(dir.join(MANIFEST_FILE)) else {
        return false;
    };
    let mut listed = BTreeSet::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        let (Some(src), Some(blob), Some(file), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return false;
        };
        let (Some(src_hex), Some(blob_hex)) =
            (src.strip_prefix("src:"), blob.strip_prefix("blob:"))
        else {
            return false;
        };
        if src_hex.len() != 64 || blob_hex.len() != 64 {
            return false;
        }
        let Ok(bytes) = fs::read(dir.join(file)) else {
            return false;
        };
        if sha256_hex(&[&bytes]) != blob_hex {
            return false;
        }
        listed.insert(file);
    }
    let mut required = required.into_iter().peekable();
    required.peek().is_some() && required.all(|f| listed.contains(f.as_str()))
}

/// Publishes one prebuilt generation (blobs + manifest) only after every shader compiled:
/// stage it beside `dir`, then swap it in, so a failed run leaves the previous generation
/// (or no directory) in place.
fn publish_prebuilt(dir: &Path, entries: &BTreeMap<String, ManifestEntry>, out: &Path) {
    let name = dir.file_name().unwrap().to_string_lossy();
    let parent = dir.parent().unwrap();
    fs::create_dir_all(parent).unwrap_or_else(|e| panic!("mkdir {}: {e}", parent.display()));
    // Serialize emissions into the same destination (separate build dirs may share it);
    // held until the backup is cleaned up, released on drop.
    let lock_path = dir.with_file_name(format!(".{name}.lock"));
    let lock = fs::File::create(&lock_path)
        .unwrap_or_else(|e| panic!("create {}: {e}", lock_path.display()));
    lock.lock()
        .unwrap_or_else(|e| panic!("lock {}: {e}", lock_path.display()));
    let stage = dir.with_file_name(format!(".{name}.staging"));
    let old = dir.with_file_name(format!(".{name}.old"));
    // A backup left by a failed promotion is the last good generation: restore it
    // before staging so a staging failure never leaves it stranded at `.old`.
    if old.exists() && !dir.exists() {
        fs::rename(&old, dir).unwrap_or_else(|e| panic!("restore {}: {e}", old.display()));
    }
    let _ = fs::remove_dir_all(&stage);
    fs::create_dir_all(&stage).unwrap_or_else(|e| panic!("mkdir {}: {e}", stage.display()));
    for file in entries.keys() {
        fs::copy(out.join(file), stage.join(file))
            .unwrap_or_else(|e| panic!("stage {file} into {}: {e}", stage.display()));
    }
    write_manifest(&stage, entries);
    // Both exist: keep exactly one fallback. A provably complete `dir` supersedes
    // the leftover backup; otherwise `dir` is the dubious one and the backup stays
    // put until the new generation is published (a failed promotion leaves it for
    // the restore above).
    if old.exists() {
        let stale = if is_complete_generation(dir, entries.keys()) {
            &old
        } else {
            dir
        };
        fs::remove_dir_all(stale)
            .unwrap_or_else(|e| panic!("remove superseded {}: {e}", stale.display()));
    }
    let had_previous = dir.exists();
    if had_previous {
        fs::rename(dir, &old).unwrap_or_else(|e| panic!("move aside {}: {e}", dir.display()));
    }
    if let Err(e) = fs::rename(&stage, dir) {
        if had_previous && let Err(r) = fs::rename(&old, dir) {
            panic!(
                "publish {}: {e}; restoring the previous generation also failed ({r}), \
                 it is kept at {}",
                dir.display(),
                old.display()
            );
        }
        panic!(
            "publish {}: {e} (previous generation left in place)",
            dir.display()
        );
    }
    let _ = fs::remove_dir_all(&old);
}

fn write_manifest(dir: &Path, entries: &BTreeMap<String, ManifestEntry>) {
    fs::create_dir_all(dir).unwrap_or_else(|e| panic!("mkdir {}: {e}", dir.display()));
    let mut out = String::from(
        "# Generated by mui-shaders/build.rs (MUI_SHADERS_EMIT_PREBUILT). Do not edit by hand.\n",
    );
    for (file, entry) in entries {
        let _ = writeln!(
            out,
            "src:{} blob:{} {file}",
            entry.src_sha256, entry.blob_sha256
        );
    }
    let path = dir.join(MANIFEST_FILE);
    fs::write(&path, out).unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
}

/// Native shader compilers and prebuilt state this build runs with.
struct BuildCtx {
    target_os: String,
    fxc: Option<PathBuf>,
    metal: bool,
    prebuilt_dxbc_dir: PathBuf,
    prebuilt_dxbc: Option<BTreeMap<String, ManifestEntry>>,
    prebuilt_metallib_dir: PathBuf,
    prebuilt_metallib: Option<BTreeMap<String, ManifestEntry>>,
    emit_prebuilt_dir: Option<PathBuf>,
    emitted_dxbc: BTreeMap<String, ManifestEntry>,
    emitted_metallib: BTreeMap<String, ManifestEntry>,
    expected_dxbc_files: BTreeSet<String>,
    expected_metallib_files: BTreeSet<String>,
}

impl BuildCtx {
    fn detect(manifest_dir: &Path) -> Self {
        println!("cargo:rerun-if-env-changed=MUI_FXC");
        println!("cargo:rerun-if-env-changed=MUI_SHADERS_EMIT_PREBUILT");
        let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
        let emit_prebuilt_dir = env::var_os("MUI_SHADERS_EMIT_PREBUILT")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from);

        let prebuilt_root = manifest_dir.join("prebuilt");
        let prebuilt_dxbc_dir = prebuilt_root.join("dxbc");
        let prebuilt_metallib_dir = prebuilt_root.join("metallib");
        // Emission regenerates the committed blobs, so it must not verify (and
        // panic on) the stale manifests it is about to replace, nor track the
        // prebuilt output directory as a build input; every normal build still
        // tracks and verifies them strictly.
        let (prebuilt_dxbc, prebuilt_metallib) = if emit_prebuilt_dir.is_some() {
            (None, None)
        } else {
            if prebuilt_root.exists() {
                println!("cargo:rerun-if-changed={}", prebuilt_root.display());
            } else {
                println!("cargo:rerun-if-changed={}", manifest_dir.display());
            }
            (
                parse_manifest(&prebuilt_dxbc_dir),
                parse_manifest(&prebuilt_metallib_dir),
            )
        };

        let want_fxc = (target_os == "windows" || emit_prebuilt_dir.is_some())
            && cfg!(windows)
            && prebuilt_dxbc.is_none();
        let fxc = want_fxc.then(|| {
            find_fxc().unwrap_or_else(|| {
                panic!(
                    "mui-shaders: FXC (fxc.exe from the Windows 10/11 SDK) is required to \
                     build for Windows; install the SDK or set MUI_FXC to fxc.exe"
                )
            })
        });
        if target_os == "windows" && fxc.is_none() && prebuilt_dxbc.is_none() {
            println!(
                "cargo:warning=mui-shaders: cross build for Windows ships no prebuilt DXBC \
                 (falling back to runtime D3DCompile)"
            );
        }
        let metal = (target_os == "macos" || emit_prebuilt_dir.is_some())
            && cfg!(target_os = "macos")
            && prebuilt_metallib.is_none();

        Self {
            target_os,
            fxc,
            metal,
            prebuilt_dxbc_dir,
            prebuilt_dxbc,
            prebuilt_metallib_dir,
            prebuilt_metallib,
            emit_prebuilt_dir,
            emitted_dxbc: BTreeMap::new(),
            emitted_metallib: BTreeMap::new(),
            expected_dxbc_files: BTreeSet::new(),
            expected_metallib_files: BTreeSet::new(),
        }
    }
}

/// `MUI_FXC`, `fxc` on PATH, or the newest Windows Kits 10 x64 `fxc.exe`.
fn find_fxc() -> Option<PathBuf> {
    if let Some(path) = env::var_os("MUI_FXC") {
        return Some(PathBuf::from(path));
    }
    if std::process::Command::new("fxc").arg("/?").output().is_ok() {
        return Some(PathBuf::from("fxc"));
    }
    let root = env::var_os("ProgramFiles(x86)").map(PathBuf::from)?;
    let bin = root.join("Windows Kits").join("10").join("bin");
    let mut versions: Vec<PathBuf> = fs::read_dir(&bin)
        .ok()?
        .filter_map(Result::ok)
        .map(|e| e.path().join("x64").join("fxc.exe"))
        .filter(|p| p.is_file())
        .collect();
    versions.sort();
    versions.pop()
}

fn run(command: &mut std::process::Command, what: &str) {
    let output = command
        .output()
        .unwrap_or_else(|e| panic!("{what}: cannot run {command:?}: {e}"));
    assert!(
        output.status.success(),
        "{what}: {command:?} failed\n{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn main() {
    // Self-verify our pure-Rust SHA-256 against the FIPS 180-4 empty & "abc" vectors.
    assert_eq!(
        sha256_hex(&[b""]),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        sha256_hex(&[b"abc"]),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );

    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let mut ctx = BuildCtx::detect(&manifest);
    let mut code = String::new();
    code.push_str("pub static ARTIFACTS: [ShaderArtifact; 4] = [\n");
    let mut statics = String::new();
    for (id, name) in SHADERS {
        let path = manifest.join("wgsl").join(format!("{name}.wgsl"));
        println!("cargo:rerun-if-changed={}", path.display());
        lower(id, name, &path, &out, &mut ctx, &mut code, &mut statics);
    }
    code.push_str("];\n");
    let expect_dxbc =
        ctx.target_os == "windows" && (ctx.prebuilt_dxbc.is_some() || ctx.fxc.is_some());
    writeln!(
        code,
        "#[cfg(test)]\nconst EXPECT_DXBC: bool = {expect_dxbc};"
    )
    .unwrap();
    let expect_metallib =
        ctx.target_os == "macos" && (ctx.prebuilt_metallib.is_some() || ctx.metal);
    writeln!(
        code,
        "#[cfg(test)]\nconst EXPECT_METALLIB: bool = {expect_metallib};"
    )
    .unwrap();
    statics.push_str(&code);

    if let Some(manifest) = &ctx.prebuilt_dxbc {
        for file in manifest.keys() {
            assert!(
                ctx.expected_dxbc_files.contains(file),
                "mui-shaders: obsolete entry `{file}` in {}; regenerate with \
                 MUI_SHADERS_EMIT_PREBUILT=prebuilt",
                ctx.prebuilt_dxbc_dir.join(MANIFEST_FILE).display()
            );
        }
    }
    if let Some(manifest) = &ctx.prebuilt_metallib {
        for file in manifest.keys() {
            assert!(
                ctx.expected_metallib_files.contains(file),
                "mui-shaders: obsolete entry `{file}` in {}; regenerate with \
                 MUI_SHADERS_EMIT_PREBUILT=prebuilt",
                ctx.prebuilt_metallib_dir.join(MANIFEST_FILE).display()
            );
        }
    }

    fs::write(out.join("shaders.rs"), statics).unwrap();

    if let Some(emit_root) = &ctx.emit_prebuilt_dir {
        if !ctx.emitted_dxbc.is_empty() {
            publish_prebuilt(&emit_root.join("dxbc"), &ctx.emitted_dxbc, &out);
        }
        if !ctx.emitted_metallib.is_empty() {
            publish_prebuilt(&emit_root.join("metallib"), &ctx.emitted_metallib, &out);
        }
    }
}

fn lower(
    id: &str,
    name: &str,
    path: &Path,
    out: &Path,
    ctx: &mut BuildCtx,
    code: &mut String,
    statics: &mut String,
) {
    let src = fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let module = naga::front::wgsl::parse_str(&src).unwrap_or_else(|e| {
        panic!(
            "{}",
            e.emit_to_string_with_path(&src, &path.display().to_string())
        )
    });
    let info = Validator::new(ValidationFlags::all(), Capabilities::empty())
        .validate(&module)
        .unwrap_or_else(|e| {
            panic!(
                "{}",
                e.emit_to_string_with_path(&src, &path.display().to_string())
            )
        });

    let bindings = reflect(&module, name);
    validate_webgl(name, &module, &info);

    // SPIR-V 1.3 (Vulkan 1.1). ADJUST_COORDINATE_SPACE flips clip-space Y in
    // the shader, so every backend takes WGSL's clip space with an ordinary
    // positive-height viewport.
    let spv_options = spv::Options {
        lang_version: (1, 3),
        flags: spv::WriterFlags::ADJUST_COORDINATE_SPACE | spv::WriterFlags::LABEL_VARYINGS,
        binding_map: bindings
            .iter()
            .map(|b| {
                let target = spv::BindingInfo {
                    descriptor_set: b.rb.group,
                    binding: b.rb.binding,
                    binding_array_size: None,
                };
                (b.rb, target)
            })
            .collect(),
        fake_missing_bindings: false,
        zero_initialize_workgroup_memory: spv::ZeroInitializeWorkgroupMemoryMode::Polyfill,
        ..Default::default()
    };
    let spirv = spv::write_vec(&module, &info, &spv_options, None)
        .unwrap_or_else(|e| panic!("{name}: SPIR-V: {e}"));

    // MSL 2.3, every entry point sharing one explicit resource map.
    let mut ep_resources = msl::EntryPointResources::default();
    for b in &bindings {
        let slot = msl::Slot::try_from(b.slot).expect("Metal resource slot fits in u8");
        let target = match b.class {
            Class::Buffer => msl::BindTarget {
                buffer: Some(slot),
                ..Default::default()
            },
            Class::Texture => msl::BindTarget {
                texture: Some(slot),
                ..Default::default()
            },
            Class::Sampler => msl::BindTarget {
                sampler: Some(msl::BindSamplerTarget::Resource(slot)),
                ..Default::default()
            },
        };
        ep_resources.resources.insert(b.rb, target);
    }
    let msl_options = msl::Options {
        lang_version: (2, 3),
        per_entry_point_map: module
            .entry_points
            .iter()
            .map(|ep| (ep.name.clone(), ep_resources.clone()))
            .collect(),
        fake_missing_bindings: false,
        zero_initialize_workgroup_memory: true,
        ..Default::default()
    };
    let (msl_src, msl_info) = msl::write_string(
        &module,
        &info,
        &msl_options,
        &msl::PipelineOptions::default(),
    )
    .unwrap_or_else(|e| panic!("{name}: MSL: {e}"));

    // HLSL SM 5.0 (D3D11), registers per the slot rule, all in space 0.
    let mut binding_map = BTreeMap::new();
    for b in &bindings {
        binding_map.insert(
            b.rb,
            hlsl::BindTarget {
                space: 0,
                register: b.slot,
                binding_array_size: None,
                dynamic_storage_buffer_offsets_index: None,
                restrict_indexing: false,
            },
        );
    }
    let hlsl_options = hlsl::Options {
        shader_model: hlsl::ShaderModel::V5_0,
        binding_map,
        // Placeholder: d3d11_samplers strips the index buffers naga declares.
        sampler_buffer_binding_map: (0..4)
            .map(|group| {
                let target = hlsl::BindTarget {
                    space: 0,
                    register: 64 + group,
                    binding_array_size: None,
                    dynamic_storage_buffer_offsets_index: None,
                    restrict_indexing: false,
                };
                (hlsl::SamplerIndexBufferKey { group }, target)
            })
            .collect(),
        fake_missing_bindings: false,
        ..Default::default()
    };
    let mut hlsl_src = String::new();
    let hlsl_info = hlsl::Writer::new(
        &mut hlsl_src,
        &hlsl_options,
        &hlsl::PipelineOptions::default(),
    )
    .write(&module, &info, None)
    .unwrap_or_else(|e| panic!("{name}: HLSL: {e}"));
    let hlsl_src = d3d11_samplers(name, hlsl_src, &bindings);

    let metal_path = out.join(format!("{name}.metal"));
    let hlsl_path = out.join(format!("{name}.hlsl"));
    fs::write(&metal_path, &msl_src).unwrap();
    fs::write(&hlsl_path, &hlsl_src).unwrap();

    // Metallib resolution: verify committed prebuilt (if present on any host),
    // compile natively on macOS, and embed on target_os == "macos".
    let metallib_file = format!("{name}.metallib");
    ctx.expected_metallib_files.insert(metallib_file.clone());
    let msl_src_hash = sha256_hex(&[
        b"metallib-v1\0macos-metal2.3\0",
        name.as_bytes(),
        b"\0",
        msl_src.as_bytes(),
    ]);
    let verified_prebuilt_metallib = ctx.prebuilt_metallib.as_ref().map(|manifest| {
        verify_prebuilt_blob(
            &ctx.prebuilt_metallib_dir,
            manifest,
            &metallib_file,
            &msl_src_hash,
            b"MTLB",
        )
    });
    let native_metallib = if ctx.metal {
        let (major, minor) = (2, 3);
        let air_path = out.join(format!("{name}.air"));
        let lib_path = out.join(&metallib_file);
        run(
            std::process::Command::new("xcrun")
                .args(["-sdk", "macosx", "metal"])
                .arg(format!("-std=macos-metal{major}.{minor}"))
                .arg("-c")
                .arg(&metal_path)
                .arg("-o")
                .arg(&air_path),
            &format!("{name}: xcrun metal"),
        );
        run(
            std::process::Command::new("xcrun")
                .args(["-sdk", "macosx", "metallib"])
                .arg(&air_path)
                .arg("-o")
                .arg(&lib_path),
            &format!("{name}: xcrun metallib"),
        );
        if ctx.emit_prebuilt_dir.is_some() {
            // Recorded only; `publish_prebuilt` copies from OUT_DIR once every shader built.
            let bytes = fs::read(&lib_path).unwrap();
            let blob_sha256 = sha256_hex(&[&bytes]);
            ctx.emitted_metallib.insert(
                metallib_file.clone(),
                ManifestEntry {
                    src_sha256: msl_src_hash,
                    blob_sha256,
                },
            );
        }
        Some(lib_path)
    } else {
        None
    };
    let metallib_expr = if ctx.target_os == "macos" {
        if let Some(p) = verified_prebuilt_metallib
            .as_ref()
            .or(native_metallib.as_ref())
        {
            format!("Some(include_bytes!({:?}))", p.display().to_string())
        } else {
            "None".to_owned()
        }
    } else {
        "None".to_owned()
    };

    let upper = name.to_uppercase();
    writeln!(statics, "static {upper}_SPIRV: [u32; {}] = [", spirv.len()).unwrap();
    for row in spirv.chunks(8) {
        let row: Vec<String> = row.iter().map(|w| format!("{w:#010x}")).collect();
        writeln!(statics, "    {},", row.join(", ")).unwrap();
    }
    statics.push_str("];\n");

    writeln!(code, "    ShaderArtifact {{").unwrap();
    writeln!(code, "        id: ShaderId::{id},\n        name: {name:?},").unwrap();
    writeln!(code, "        spirv: &{upper}_SPIRV,").unwrap();
    writeln!(
        code,
        "        msl: include_str!(concat!(env!(\"OUT_DIR\"), \"/{name}.metal\")),"
    )
    .unwrap();
    writeln!(code, "        metallib: {metallib_expr},").unwrap();
    writeln!(
        code,
        "        hlsl: include_str!(concat!(env!(\"OUT_DIR\"), \"/{name}.hlsl\")),"
    )
    .unwrap();
    code.push_str("        entry_points: &[\n");
    for (i, ep) in module.entry_points.iter().enumerate() {
        let stage = match ep.stage {
            naga::ShaderStage::Vertex => "Vertex",
            naga::ShaderStage::Fragment => "Fragment",
            other => panic!("{name}: unsupported stage {other:?}"),
        };
        let msl_name = msl_info.entry_point_names[i].as_ref().unwrap();
        let hlsl_name = hlsl_info.entry_point_names[i].as_ref().unwrap();
        let profile = if stage == "Vertex" {
            "vs_5_0"
        } else {
            "ps_5_0"
        };
        let dxbc_file = format!("{name}-{hlsl_name}.dxbc");
        ctx.expected_dxbc_files.insert(dxbc_file.clone());
        let hlsl_src_hash = sha256_hex(&[
            b"dxbc-v1\0",
            profile.as_bytes(),
            b"\0",
            hlsl_name.as_bytes(),
            b"\0",
            hlsl_src.as_bytes(),
        ]);

        let verified_prebuilt_dxbc = ctx.prebuilt_dxbc.as_ref().map(|manifest| {
            verify_prebuilt_blob(
                &ctx.prebuilt_dxbc_dir,
                manifest,
                &dxbc_file,
                &hlsl_src_hash,
                b"DXBC",
            )
        });
        let native_dxbc = ctx.fxc.as_ref().map(|fxc| {
            let out_path = out.join(&dxbc_file);
            run(
                std::process::Command::new(fxc)
                    .args(["/nologo", "/O3", "/T", profile, "/E", hlsl_name, "/Fo"])
                    .arg(&out_path)
                    .arg(&hlsl_path),
                &format!("{name} {}: fxc", ep.name),
            );
            if ctx.emit_prebuilt_dir.is_some() {
                let bytes = fs::read(&out_path).unwrap();
                let blob_sha256 = sha256_hex(&[&bytes]);
                ctx.emitted_dxbc.insert(
                    dxbc_file.clone(),
                    ManifestEntry {
                        src_sha256: hlsl_src_hash.clone(),
                        blob_sha256,
                    },
                );
            }
            out_path
        });
        let dxbc = if ctx.target_os == "windows" {
            if let Some(p) = verified_prebuilt_dxbc.as_ref().or(native_dxbc.as_ref()) {
                format!("Some(include_bytes!({:?}))", p.display().to_string())
            } else {
                "None".to_owned()
            }
        } else {
            "None".to_owned()
        };
        writeln!(
            code,
            "            EntryPointInfo {{ name: {:?}, stage: Stage::{stage}, \
             msl_name: {msl_name:?}, hlsl_name: {hlsl_name:?}, dxbc: {dxbc} }},",
            ep.name
        )
        .unwrap();
    }
    code.push_str("        ],\n        bindings: &[\n");
    for b in &bindings {
        write!(
            code,
            "            BindingInfo {{ group: {}, binding: {}, name: {:?}, kind: BindingKind::{}, \
             msl_slot: {}, hlsl_register: {}, uniform_layout: ",
            b.rb.group, b.rb.binding, b.name, b.kind, b.slot, b.slot
        )
        .unwrap();
        emit_uniform_layout(&module, b, code);
        code.push_str(" },\n");
    }
    code.push_str("        ],\n    },\n");
}

fn validate_webgl(name: &str, module: &naga::Module, info: &naga::valid::ModuleInfo) {
    // Keep every existing shader compatible with the downlevel renderer.
    let options = glsl::Options {
        version: glsl::Version::Embedded {
            version: 300,
            is_webgl: true,
        },
        ..Default::default()
    };
    for entry in &module.entry_points {
        assert!(
            matches!(
                entry.stage,
                naga::ShaderStage::Vertex | naga::ShaderStage::Fragment
            ),
            "{name}: expected a raster entry point, got {:?} for {}",
            entry.stage,
            entry.name
        );
        let pipeline = glsl::PipelineOptions {
            shader_stage: entry.stage,
            entry_point: entry.name.clone(),
            multiview: None,
        };
        let mut output = String::new();
        glsl::Writer::new(
            &mut output,
            module,
            info,
            &options,
            &pipeline,
            naga::proc::BoundsCheckPolicies::default(),
        )
        .and_then(|mut writer| writer.write())
        .unwrap_or_else(|e| panic!("{name}: GLSL ES 3.00 {}: {e}", entry.name));
    }
}

fn emit_uniform_layout(module: &naga::Module, binding: &Binding, output: &mut String) {
    let Some(ty) = binding.uniform_type else {
        output.push_str("None");
        return;
    };
    let TypeInner::Struct { members, span } = &module.types[ty].inner else {
        panic!("{} must use a uniform structure", binding.name);
    };
    write!(output, "Some(UniformLayout {{ size: {span}, members: &[").unwrap();
    for member in members {
        let array_stride = match module.types[member.ty].inner {
            TypeInner::Array { stride, .. } => Some(stride),
            _ => None,
        };
        write!(
            output,
            "UniformMember {{ offset: {}, array_stride: {array_stride:?} }},",
            member.offset
        )
        .unwrap();
    }
    output.push_str("] })");
}

fn reflect(module: &naga::Module, shader: &str) -> Vec<Binding> {
    let mut bindings: Vec<Binding> = module
        .global_variables
        .iter()
        .filter_map(|(_, g)| {
            let rb = g.binding?;
            let name = g.name.clone().unwrap_or_default();
            let (kind, class) = match (g.space, &module.types[g.ty].inner) {
                (AddressSpace::Uniform, _) => ("UniformBuffer", Class::Buffer),
                (
                    AddressSpace::Handle,
                    TypeInner::Image {
                        dim: ImageDimension::D2,
                        arrayed,
                        class:
                            ImageClass::Sampled {
                                kind: ScalarKind::Float,
                                multi: false,
                            },
                    },
                ) => (
                    if *arrayed {
                        "Texture2dArray"
                    } else {
                        "Texture2d"
                    },
                    Class::Texture,
                ),
                (AddressSpace::Handle, TypeInner::Sampler { comparison: false }) => {
                    ("Sampler", Class::Sampler)
                }
                (space, ty) => panic!("{shader}: `{name}` has unsupported {space:?} {ty:?}"),
            };
            Some(Binding {
                rb,
                name,
                kind,
                class,
                slot: 0,
                uniform_type: (class == Class::Buffer).then_some(g.ty),
            })
        })
        .collect();
    bindings.sort_by_key(|b| (b.rb.group, b.rb.binding));
    let mut next = [0u32; 3];
    for b in &mut bindings {
        let n = &mut next[b.class as usize];
        b.slot = *n;
        *n += 1;
    }
    bindings
}

/// naga 30 writes samplers through a D3D12 sampler heap
/// (`nagaSamplerHeap[nagaGroupNSamplerIndexArray[i]]`), which D3D11 has no
/// form of. Rewrite each sampler to a plain `SamplerState name : register(sN)`
/// and drop the heap and index-array declarations. Panics if naga's output
/// shape changes, so a naga bump cannot silently ship broken HLSL.
fn d3d11_samplers(shader: &str, src: String, bindings: &[Binding]) -> String {
    let samplers: Vec<&Binding> = bindings
        .iter()
        .filter(|b| b.class == Class::Sampler)
        .collect();
    if samplers.is_empty() {
        return src;
    }
    let mut out = String::with_capacity(src.len());
    let mut rewritten = 0;
    for line in src.lines() {
        let t = line.trim_start();
        if t.contains("nagaSamplerHeap[")
            && t.starts_with("SamplerState ")
            && !t.starts_with("static")
            || t.contains("nagaComparisonSamplerHeap[") && t.starts_with("SamplerComparisonState ")
            || t.starts_with("StructuredBuffer<uint> nagaGroup") && t.contains("SamplerIndexArray")
        {
            continue;
        }
        // `static const SamplerState NAME = nagaSamplerHeap[nagaGroupGSamplerIndexArray[SLOT]];`
        // NAME may be renamed (`linear` is an HLSL keyword), so match by SLOT.
        if let Some(rest) = t.strip_prefix("static const SamplerState ") {
            let var = rest.split_whitespace().next().unwrap_or_default();
            let slot: u32 = rest
                .rsplit_once("SamplerIndexArray[")
                .and_then(|(_, r)| r.split(']').next())
                .and_then(|n| n.parse().ok())
                .unwrap_or_else(|| panic!("{shader}: unexpected HLSL sampler line `{t}`"));
            // naga renames reserved words with a trailing `_` (`linear_`).
            assert!(
                samplers.iter().any(|b| b.slot == slot
                    && (var == b.name || var.strip_suffix('_') == Some(b.name.as_str()))),
                "{shader}: HLSL sampler `{var}` slot {slot} is not that slot's WGSL sampler"
            );
            writeln!(out, "SamplerState {var} : register(s{slot});").unwrap();
            rewritten += 1;
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    assert_eq!(
        rewritten,
        samplers.len(),
        "{shader}: naga's HLSL sampler shape changed"
    );
    assert!(
        !out.contains("nagaSamplerHeap"),
        "{shader}: sampler heap survived the rewrite"
    );
    assert!(
        !out.contains("SamplerIndexArray"),
        "{shader}: index array survived the rewrite"
    );
    out
}

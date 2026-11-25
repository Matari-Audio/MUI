//! Prebuilt shader generations: manifest hashing, completeness checks and
//! crash-safe publication. Included by `build.rs` and by `tests/prebuilt.rs`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

pub const MANIFEST_FILE: &str = "manifest.sha256";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManifestEntry {
    pub src_sha256: String,
    pub blob_sha256: String,
}

/// Pure-Rust FIPS 180-4 SHA-256 (zero external build-dependencies).
pub fn sha256_hex(parts: &[&[u8]]) -> String {
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

/// Returns `true` iff `dir` holds a parseable manifest that lists every file in
/// `required` exactly once (the normal reader rejects duplicate rows), and every
/// listed blob is present and matches its recorded SHA-256.
pub fn is_complete_generation<'a>(
    dir: &Path,
    required: impl IntoIterator<Item = &'a String>,
) -> bool {
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
        if !listed.insert(file) {
            return false;
        }
    }
    let mut required = required.into_iter().peekable();
    required.peek().is_some() && required.all(|f| listed.contains(f.as_str()))
}

/// Publishes one prebuilt generation (blobs + manifest) only after every shader compiled:
/// stage it beside `dir`, then swap it in, so a failed run leaves the previous generation
/// (or no directory) in place.
pub fn publish_prebuilt(dir: &Path, entries: &BTreeMap<String, ManifestEntry>, out: &Path) {
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
    restore_backup(dir, &old).unwrap_or_else(|e| panic!("restore {}: {e}", old.display()));
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
    if dir.exists() {
        fs::rename(dir, &old).unwrap_or_else(|e| panic!("move aside {}: {e}", dir.display()));
    }
    if let Err(e) = fs::rename(&stage, dir) {
        // `.old` is the previous generation whether `dir` was just moved aside or
        // was removed above as invalid; either way `dir` is now absent.
        if let Err(r) = restore_backup(dir, &old) {
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

/// Moves the backup `old` back to `dir` when `dir` is absent; otherwise a no-op.
pub fn restore_backup(dir: &Path, old: &Path) -> std::io::Result<()> {
    if old.exists() && !dir.exists() {
        fs::rename(old, dir)?;
    }
    Ok(())
}

pub fn write_manifest(dir: &Path, entries: &BTreeMap<String, ManifestEntry>) {
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

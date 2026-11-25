//! Prebuilt-generation publication rules from `build/prebuilt.rs`.

#[expect(dead_code, reason = "build-script module; tests use part of it")]
#[path = "../build/prebuilt.rs"]
mod prebuilt;

use std::fs;
use std::path::PathBuf;

use prebuilt::{MANIFEST_FILE, is_complete_generation, restore_backup, sha256_hex};

fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn prebuilt_duplicate_manifest_row_is_not_a_complete_generation() {
    let dir = scratch("prebuilt-duplicate-row");
    let blob = b"MTLBblob";
    fs::write(dir.join("quad.metallib"), blob).unwrap();
    let row = format!(
        "src:{} blob:{} quad.metallib\n",
        "0".repeat(64),
        sha256_hex(&[blob])
    );
    let required = ["quad.metallib".to_owned()];

    fs::write(dir.join(MANIFEST_FILE), &row).unwrap();
    assert!(is_complete_generation(&dir, &required));

    fs::write(dir.join(MANIFEST_FILE), row.repeat(2)).unwrap();
    assert!(!is_complete_generation(&dir, &required));
}

#[test]
fn prebuilt_backup_is_restored_only_when_canonical_dir_is_absent() {
    let root = scratch("prebuilt-restore");
    let (dir, old) = (root.join("metallib"), root.join(".metallib.old"));
    fs::create_dir(&old).unwrap();
    fs::write(old.join(MANIFEST_FILE), "backup").unwrap();

    // Canonical dir present: the backup stays put.
    fs::create_dir(&dir).unwrap();
    restore_backup(&dir, &old).unwrap();
    assert!(old.exists());

    // Canonical dir removed (invalid) before a failed promotion: backup returns.
    fs::remove_dir_all(&dir).unwrap();
    restore_backup(&dir, &old).unwrap();
    assert!(!old.exists());
    assert_eq!(
        fs::read_to_string(dir.join(MANIFEST_FILE)).unwrap(),
        "backup"
    );
}

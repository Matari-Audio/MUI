extern crate std;

use std::collections::BTreeMap;
use std::format;
use std::path::PathBuf;
use std::process::Command;
use std::string::{String, ToString};
use std::vec::Vec;

use super::*;

const WGSL: [(&str, &str); 4] = [
    ("quad", QUAD_WGSL),
    ("blur", BLUR_WGSL),
    ("weld", WELD_WGSL),
    ("present", PRESENT_WGSL),
];

/// The binding ABI `mui-hal` is written against. Changing a shader's
/// resources must change this table on purpose.
#[test]
fn binding_abi_is_pinned() {
    use BindingKind::*;
    type Row = (u32, u32, &'static str, BindingKind, u32);
    let expected: [&[Row]; 4] = [
        &[
            (0, 0, "view", UniformBuffer, 0),
            (0, 1, "atlas", Texture2dArray, 0),
            (0, 2, "linear", Sampler, 0),
            (0, 3, "paths", Texture2d, 1),
            (1, 0, "image", Texture2d, 2),
            (1, 1, "under", Texture2d, 3),
        ],
        &[(0, 0, "src", Texture2d, 0), (0, 1, "u", UniformBuffer, 0)],
        &[
            (0, 0, "u", UniformBuffer, 0),
            (0, 1, "boundary", UniformBuffer, 1),
        ],
        &[(0, 0, "frame", Texture2d, 0)],
    ];
    for (a, want) in artifacts().iter().zip(expected) {
        let got: Vec<_> = a
            .bindings
            .iter()
            .map(|b| {
                assert_eq!(b.msl_slot, b.hlsl_register, "{}: {}", a.name, b.name);
                (b.group, b.binding, b.name, b.kind, b.msl_slot)
            })
            .collect();
        assert_eq!(got, want, "{}", a.name);
    }
}

#[test]
fn artifacts_are_in_id_order() {
    let names: Vec<_> = artifacts().iter().map(|a| a.name).collect();
    assert_eq!(names, ["quad", "blur", "weld", "present"]);
    for id in ShaderId::ALL {
        assert_eq!(id.artifact().id, id);
    }
}

#[test]
fn entry_points_match_wgsl() {
    for (a, (name, src)) in artifacts().iter().zip(WGSL) {
        assert_eq!(a.name, name);
        let got: Vec<_> = a
            .entry_points
            .iter()
            .map(|e| (e.name.to_string(), e.stage))
            .collect();
        assert_eq!(got, wgsl_entry_points(src), "{name}");
    }
    let quad = ShaderId::Quad.artifact();
    assert_eq!(quad.entry_point("vs").unwrap().stage, Stage::Vertex);
    assert_eq!(quad.entry_point("fs").unwrap().stage, Stage::Fragment);
    assert!(quad.entry_point("nope").is_none());
}

#[test]
fn bindings_match_wgsl() {
    for (a, (name, src)) in artifacts().iter().zip(WGSL) {
        let got: Vec<_> = a
            .bindings
            .iter()
            .map(|b| (b.group, b.binding, b.name.to_string()))
            .collect();
        assert_eq!(got, wgsl_bindings(src), "{name}");
        // Slots are dense per class, in (group, binding) order.
        let mut next = BTreeMap::new();
        for b in a.bindings {
            let n = next.entry(b.kind.hlsl_register_class()).or_insert(0);
            assert_eq!(b.msl_slot, *n, "{name}: {}", b.name);
            *n += 1;
        }
    }
}

#[test]
fn spirv_header_entry_points_and_decorations() {
    for a in artifacts() {
        let w = a.spirv;
        assert!(w.len() > 5, "{}", a.name);
        assert_eq!(w[0], 0x0723_0203, "{}: magic", a.name);
        let (major, minor) = SPIRV_VERSION;
        assert_eq!(
            w[1],
            (u32::from(major) << 16) | (u32::from(minor) << 8),
            "{}",
            a.name
        );

        let mut sets = BTreeMap::new();
        let mut bindings = BTreeMap::new();
        let mut entries = Vec::new();
        let mut i = 5;
        while i < w.len() {
            let (count, op) = ((w[i] >> 16) as usize, w[i] & 0xffff);
            assert!(
                count > 0 && i + count <= w.len(),
                "{}: bad instruction at {i}",
                a.name
            );
            let ins = &w[i..i + count];
            match op {
                // OpEntryPoint model id "name" interface...
                15 => entries.push((ins[1], spirv_string(&ins[3..]))),
                // OpDecorate id Binding(33)/DescriptorSet(34) n
                71 if ins[2] == 33 => {
                    bindings.insert(ins[1], ins[3]);
                }
                71 if ins[2] == 34 => {
                    sets.insert(ins[1], ins[3]);
                }
                _ => {}
            }
            i += count;
        }
        assert_eq!(i, w.len(), "{}: truncated", a.name);

        let want: Vec<_> = a
            .entry_points
            .iter()
            .map(|e| {
                let model = match e.stage {
                    Stage::Vertex => 0,
                    Stage::Fragment => 4,
                };
                (model, e.name.to_string())
            })
            .collect();
        assert_eq!(entries, want, "{}: OpEntryPoint", a.name);

        // No OpName without the DEBUG flag, so compare (set, binding) pairs.
        let mut decorated: Vec<_> = sets.iter().map(|(id, set)| (*set, bindings[id])).collect();
        decorated.sort();
        let want: Vec<_> = a.bindings.iter().map(|b| (b.group, b.binding)).collect();
        assert_eq!(decorated, want, "{}: descriptor decorations", a.name);
    }
}

#[test]
fn msl_has_entry_points_and_slots() {
    for a in artifacts() {
        assert!(!a.msl.is_empty());
        assert!(a.msl.contains("#include <metal_stdlib>"), "{}", a.name);
        for e in a.entry_points {
            let qualifier = match e.stage {
                Stage::Vertex => "vertex",
                Stage::Fragment => "fragment",
            };
            let found = a
                .msl
                .lines()
                .any(|l| l.starts_with(qualifier) && l.contains(&format!(" {}(", e.msl_name)));
            assert!(found, "{}: no `{qualifier} ... {}(`", a.name, e.msl_name);
        }
        for b in a.bindings {
            let attr = match b.kind {
                BindingKind::UniformBuffer => "buffer",
                BindingKind::Texture2d | BindingKind::Texture2dArray => "texture",
                BindingKind::Sampler => "sampler",
            };
            let want = format!(" {} [[{attr}({})]]", b.name, b.msl_slot);
            assert!(a.msl.contains(&want), "{}: missing `{want}`", a.name);
        }
    }
}

#[test]
fn hlsl_has_entry_points_and_d3d11_registers() {
    for a in artifacts() {
        assert!(!a.hlsl.is_empty());
        for e in a.entry_points {
            let found = a
                .hlsl
                .lines()
                .any(|l| !l.starts_with(' ') && l.contains(&format!(" {}(", e.hlsl_name)));
            assert!(found, "{}: no HLSL function `{}`", a.name, e.hlsl_name);
        }
        // (variable, register) per declaration, e.g. `SamplerState linear_ : register(s0)`.
        let mut registers: Vec<(String, String)> = Vec::new();
        for l in a.hlsl.lines() {
            if let Some((decl, r)) = l.split_once(" : register(") {
                let var = decl.split_whitespace().last().unwrap();
                // naga renames reserved words with a trailing `_`.
                let var = a
                    .binding(var)
                    .map_or_else(|| var.trim_end_matches('_'), |b| b.name);
                registers.push((var.to_string(), r.split(')').next().unwrap().to_string()));
            }
        }
        let mut want: Vec<(String, String)> = a
            .bindings
            .iter()
            .map(|b| {
                let register = format!("{}{}", b.kind.hlsl_register_class(), b.hlsl_register);
                (b.name.to_string(), register)
            })
            .collect();
        registers.sort();
        want.sort();
        assert_eq!(registers, want, "{}: each binding's register", a.name);
        // D3D12-only constructs that D3D11 / SM 5.0 cannot compile.
        for bad in [
            "space",
            "nagaSamplerHeap",
            "SamplerIndexArray",
            "StructuredBuffer",
        ] {
            assert!(!a.hlsl.contains(bad), "{}: contains `{bad}`", a.name);
        }
    }
}

/// FXC's SM 5.0 bytecode ships for every entry point exactly when built on
/// Windows for Windows.
#[test]
fn dxbc_ships_with_native_windows_builds() {
    for a in artifacts() {
        for e in a.entry_points {
            assert_eq!(
                e.dxbc.is_some(),
                cfg!(windows),
                "{} {}: DXBC ships exactly on Windows",
                a.name,
                e.name
            );
            if let Some(bytes) = e.dxbc {
                assert_eq!(&bytes[..4], b"DXBC", "{} {}", a.name, e.name);
            }
        }
    }
}

/// `spirv-val` (SPIRV-Tools) on every module, when installed. CI sets
/// `MUI_SHADERS_REQUIRE_VALIDATORS=spirv-val,fxc,metal` (per job) to fail
/// instead of skipping.
#[test]
fn spirv_val_accepts_every_module() {
    let Some(tool) = external("spirv-val", &["--version"]) else {
        return;
    };
    for a in artifacts() {
        let path = scratch(&format!("{}.spv", a.name));
        let bytes: Vec<u8> = a.spirv.iter().flat_map(|w| w.to_le_bytes()).collect();
        std::fs::write(&path, bytes).unwrap();
        run(
            Command::new(&tool)
                .args(["--target-env", "vulkan1.1"])
                .arg(&path),
            a.name,
        );
    }
}

/// Check HLSL source on hosts without the Windows SDK.
#[test]
fn glslang_accepts_every_hlsl_entry_point() {
    let Some(tool) = external("glslangValidator", &["--version"]) else {
        return;
    };
    for artifact in artifacts() {
        let source = scratch(&format!("glslang-{}.hlsl", artifact.name));
        std::fs::write(&source, artifact.hlsl).unwrap();
        for entry in artifact.entry_points {
            let stage = match entry.stage {
                Stage::Vertex => "vert",
                Stage::Fragment => "frag",
            };
            let output = scratch(&format!(
                "glslang-{}-{}.spv",
                artifact.name, entry.hlsl_name
            ));
            run(
                Command::new(&tool)
                    .args(["-D", "-V", "-S", stage, "-e", entry.hlsl_name, "-o"])
                    .arg(output)
                    .arg(&source),
                &format!("{} {}", artifact.name, entry.name),
            );
        }
    }
}

/// The real D3D11 compiler, `fxc.exe` from the Windows SDK, when on PATH.
#[test]
fn fxc_compiles_every_entry_point() {
    let Some(tool) = external("fxc", &["/?"]) else {
        return;
    };
    for a in artifacts() {
        let path = scratch(&format!("{}.hlsl", a.name));
        std::fs::write(&path, a.hlsl).unwrap();
        for e in a.entry_points {
            let profile = match e.stage {
                Stage::Vertex => "vs_5_0",
                Stage::Fragment => "ps_5_0",
            };
            let out = scratch(&format!("{}-{}.dxbc", a.name, e.hlsl_name));
            run(
                Command::new(&tool)
                    .args(["/nologo", "/T", profile, "/E", e.hlsl_name, "/Fo"])
                    .arg(&out)
                    .arg(&path),
                &format!("{} {}", a.name, e.name),
            );
        }
    }
}

/// Apple's Metal compiler (`xcrun metal`), when on a Mac with Xcode.
#[test]
fn metal_compiles_every_module() {
    let Some(xcrun) = external("metal", &[]) else {
        return;
    };
    let (major, minor) = MSL_VERSION;
    for a in artifacts() {
        let path = scratch(&format!("{}.metal", a.name));
        std::fs::write(&path, a.msl).unwrap();
        run(
            Command::new(&xcrun)
                .args([
                    "-sdk",
                    "macosx",
                    "metal",
                    &format!("-std=macos-metal{major}.{minor}"),
                ])
                .args(["-c", "-o"])
                .arg(scratch(&format!("{}.air", a.name)))
                .arg(&path),
            a.name,
        );
    }
}

// --- helpers ---

fn wgsl_entry_points(src: &str) -> Vec<(String, Stage)> {
    let mut out = Vec::new();
    let mut pending = None;
    for line in src.lines().map(|l| l.split("//").next().unwrap()) {
        for (attr, stage) in [("@vertex", Stage::Vertex), ("@fragment", Stage::Fragment)] {
            if line.contains(attr) {
                pending = Some(stage);
            }
        }
        if let (Some(stage), Some((_, rest))) = (pending, line.split_once("fn ")) {
            let name = rest
                .split(|c: char| c == '(' || c.is_whitespace())
                .next()
                .unwrap();
            out.push((name.to_string(), stage));
            pending = None;
        }
    }
    out
}

fn wgsl_bindings(src: &str) -> Vec<(u32, u32, String)> {
    let num = |s: &str, key: &str| -> u32 {
        let (_, r) = s.split_once(key).unwrap();
        r.split(')').next().unwrap().trim().parse().unwrap()
    };
    let mut out: Vec<_> = src
        .lines()
        .map(|l| l.split("//").next().unwrap())
        .filter(|l| l.contains("@group("))
        .map(|l| {
            let decl = l.split_once("var").unwrap().1;
            // Drop an address space, `var<uniform>`, but not the type's `<f32>`.
            let decl = match decl.strip_prefix('<') {
                Some(r) => r.split_once('>').unwrap().1,
                None => decl,
            };
            let name = decl.trim_start().split(':').next().unwrap().trim();
            (num(l, "@group("), num(l, "@binding("), name.to_string())
        })
        .collect();
    out.sort();
    out
}

fn spirv_string(words: &[u32]) -> String {
    let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8(bytes[..end].to_vec()).unwrap()
}

/// Resolves an external validator, or `None` to skip. `metal` resolves to
/// `xcrun` when it can find the Metal toolchain.
fn external(name: &str, probe: &[&str]) -> Option<PathBuf> {
    let required = std::env::var("MUI_SHADERS_REQUIRE_VALIDATORS")
        .is_ok_and(|v| v.split(',').any(|t| t.trim() == name));
    let (exe, args): (&str, Vec<&str>) = match name {
        "metal" => ("xcrun", std::vec!["-sdk", "macosx", "-f", "metal"]),
        _ => (name, probe.to_vec()),
    };
    let ok = Command::new(exe).args(&args).output().is_ok_and(|o| {
        // fxc /? exits non-zero on some SDKs; existence is what counts.
        o.status.success() || name == "fxc"
    });
    match (ok, required) {
        (true, _) => Some(PathBuf::from(exe)),
        (false, true) => panic!("{name} required by MUI_SHADERS_REQUIRE_VALIDATORS but not found"),
        (false, false) => {
            std::eprintln!("skipping: {name} not found");
            None
        }
    }
}

fn scratch(file: &str) -> PathBuf {
    let dir = PathBuf::from(env!("OUT_DIR")).join("validate");
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(file)
}

fn run(cmd: &mut Command, what: &str) {
    let out = cmd.output().unwrap_or_else(|e| panic!("{what}: {e}"));
    assert!(
        out.status.success(),
        "{what}: {:?} failed\n{}{}",
        cmd,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

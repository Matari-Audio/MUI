//! `mui-cut gen`: a Rhai script writes a project (or a scene's layers), so
//! hundreds of layers and keys come from a loop instead of by hand.
//!
//! The script is pure: no clock, no files, no modules, bounded operations
//! and sizes, and the only randomness is a seeded generator, so the same
//! script and `--seed` always write the same file. It returns a map (the
//! whole project) or an array (layers, merged into `--into`'s scene by id).
//!
//! Helpers on top of Rhai's own maths: `rand()`, `rand(a, b)`,
//! `rand_int(a, b)` (inclusive), `pick(array)`, `seed(n)`, `noise(x, y)`
//! (seeded Perlin, -1..1), `hsl(h, s, l)` / `rgb(r, g, b)` / `rgba(r, g, b, a)`
//! to `#rrggbb[aa]`, `key(t, v)` / `key(t, v, interp)`, `lerp(a, b, u)`,
//! `clamp(v, lo, hi)`. `print` goes to stderr.
use std::cell::Cell;
use std::path::Path;
use std::rc::Rc;

use mui_cut::Project;
use noise::NoiseFn as _;
use rhai::{Array, Dynamic, Engine, FLOAT, INT, Map};
use serde_json::Value;

use crate::{Args, Result, write_atomic};

/// splitmix64: small, good enough for art, and the same everywhere.
fn next(state: &Cell<u64>) -> u64 {
    let s = state.get().wrapping_add(0x9E37_79B9_7F4A_7C15);
    state.set(s);
    let mut z = s;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}
fn unit(state: &Cell<u64>) -> FLOAT {
    (next(state) >> 11) as FLOAT / (1u64 << 53) as FLOAT
}

fn hex(c: [FLOAT; 4]) -> String {
    let b = c.map(|v| (v.clamp(0., 1.) * 255.).round() as u8);
    if b[3] == 255 {
        format!("#{:02x}{:02x}{:02x}", b[0], b[1], b[2])
    } else {
        format!("#{:02x}{:02x}{:02x}{:02x}", b[0], b[1], b[2], b[3])
    }
}

fn hsl(h: FLOAT, s: FLOAT, l: FLOAT) -> String {
    let h = h.rem_euclid(360.) / 60.;
    let c = (1. - (2. * l - 1.).abs()) * s;
    let x = c * (1. - (h % 2. - 1.).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.),
        1 => (x, c, 0.),
        2 => (0., c, x),
        3 => (0., x, c),
        4 => (x, 0., c),
        _ => (c, 0., x),
    };
    let m = l - c / 2.;
    hex([r + m, g + m, b + m, 1.])
}

/// The sandbox: Rhai's language and maths, the helpers above, and limits
/// that stop a runaway loop instead of the machine.
fn engine(seed: u64) -> Engine {
    let mut e = Engine::new();
    e.set_max_operations(200_000_000)
        .set_max_call_levels(64)
        .set_max_expr_depths(128, 64)
        .set_max_string_size(1 << 20)
        .set_max_array_size(1 << 20)
        .set_max_map_size(1 << 16);
    e.on_print(|s| eprintln!("{s}"));
    e.on_debug(|s, _, pos| eprintln!("{pos:?}: {s}"));
    let state = Rc::new(Cell::new(seed));
    let perlin = Rc::new(Cell::new(noise::Perlin::new(seed as u32)));
    let r = state.clone();
    e.register_fn("rand", move || unit(&r));
    let r = state.clone();
    e.register_fn("rand", move |a: FLOAT, b: FLOAT| a + (b - a) * unit(&r));
    let r = state.clone();
    e.register_fn("rand_int", move |a: INT, b: INT| {
        let span = (b - a).unsigned_abs() + 1;
        a.min(b) + (next(&r) % span) as INT
    });
    let r = state.clone();
    e.register_fn("pick", move |v: Array| {
        if v.is_empty() {
            Dynamic::UNIT
        } else {
            v[(next(&r) % v.len() as u64) as usize].clone()
        }
    });
    let (r, n) = (state, perlin.clone());
    e.register_fn("seed", move |s: INT| {
        r.set(s as u64);
        n.set(noise::Perlin::new(s as u32));
    });
    e.register_fn("noise", move |x: FLOAT, y: FLOAT| perlin.get().get([x, y]));
    e.register_fn("hsl", hsl);
    e.register_fn("rgb", |r: FLOAT, g: FLOAT, b: FLOAT| hex([r, g, b, 1.]));
    e.register_fn("rgba", |r: FLOAT, g: FLOAT, b: FLOAT, a: FLOAT| {
        hex([r, g, b, a])
    });
    e.register_fn("lerp", |a: FLOAT, b: FLOAT, u: FLOAT| a + (b - a) * u);
    e.register_fn("clamp", |v: FLOAT, lo: FLOAT, hi: FLOAT| v.clamp(lo, hi));
    e.register_fn("key", |t: Dynamic, v: Dynamic| {
        let mut m = Map::new();
        m.insert("t".into(), t);
        m.insert("v".into(), v);
        m
    });
    e.register_fn("key", |t: Dynamic, v: Dynamic, interp: &str| {
        let mut m = Map::new();
        m.insert("t".into(), t);
        m.insert("v".into(), v);
        m.insert("interp".into(), interp.into());
        m
    });
    e
}

/// Run `script`; its result as JSON.
pub fn run(script: &str, seed: u64) -> Result<Value> {
    let out: Dynamic = engine(seed)
        .eval(script)
        .map_err(|e| format!("script: {e}"))?;
    rhai::serde::from_dynamic(&out).map_err(|e| format!("script result: {e}"))
}

/// A generated project (or, when the script returns layers, `into` with
/// them merged into `scene`), validated and in canonical form. Fields a load would drop are
/// an error: they are always a typo.
pub fn generate(script: &str, seed: u64, into: Option<(&Path, Option<&str>)>) -> Result<String> {
    let v = run(script, seed)?;
    let doc = match (v, into) {
        (Value::Object(o), _) => Value::Object(o),
        (Value::Array(layers), Some((path, scene))) => {
            let src = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
            let mut doc: Value = serde_json::from_str(&src).map_err(|e| e.to_string())?;
            let scenes = doc["scenes"].as_array_mut().ok_or("--into has no scenes")?;
            let s = match scene {
                Some(n) => scenes
                    .iter_mut()
                    .find(|s| s["name"] == n)
                    .ok_or_else(|| format!("no scene `{n}`"))?,
                None => scenes.first_mut().ok_or("--into has no scenes")?,
            };
            let list = s["layers"].as_array_mut().ok_or("the scene has no layers list")?;
            for l in layers {
                match list.iter_mut().find(|o| o["id"] == l["id"]) {
                    Some(old) => *old = l,
                    None => list.push(l),
                }
            }
            doc
        }
        (Value::Array(_), None) => return Err("the script returned layers: pass --into PROJECT [--scene NAME]".into()),
        (_, _) => return Err("the script must return a project map #{ size, fps, scenes } (or, with --into, an array of layers)".into()),
    };
    let src = doc.to_string();
    let bad: Vec<String> = mui_cut::check::lint_fields(&src)
        .iter()
        .filter(|i| matches!(i.code, "load" | "unknown_field"))
        .map(ToString::to_string)
        .collect();
    if !bad.is_empty() {
        return Err(bad.join("\n"));
    }
    Ok(Project::load(&src)?.to_json())
}

pub fn cmd(args: &Args) -> Result<()> {
    let script = std::fs::read_to_string(&args.project)
        .map_err(|e| format!("{}: {e}", args.project.display()))?;
    let into = args.get("into").map(Path::new);
    let json = generate(
        &script,
        args.num("seed", 0)?,
        into.map(|p| (p, args.get("scene"))),
    )?;
    match args.get("o").map(Path::new).or(into) {
        Some(out) => {
            write_atomic(out, &json)?;
            let p = Project::load(&json)?;
            let layers: usize = p.scenes.iter().map(|s| s.layers.len()).sum();
            eprintln!(
                "wrote {}: {} scenes, {layers} layers",
                out.display(),
                p.scenes.len()
            );
        }
        None => print!("{json}"),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCRIPT: &str = r#"
        let layers = [];
        for i in 0..50 {
            layers.push(#{ id: `dot${i}`, kind: "ellipse", x: rand(0.0, 1280.0), y: rand(0.0, 720.0),
                           width: 10, height: 10, fill: hsl(rand(0.0, 360.0), 0.7, 0.6),
                           opacity: [key(0.0, 0.0, "linear"), key(rand(0.2, 1.0), 1.0)] });
        }
        #{ size: [1280, 720], fps: 30, scenes: [#{ name: "dots", duration: 2, layers: layers }] }
    "#;

    #[test]
    fn a_seed_fixes_the_output_and_another_changes_it() {
        let a = generate(SCRIPT, 7, None).unwrap();
        assert_eq!(a, generate(SCRIPT, 7, None).unwrap());
        assert_ne!(a, generate(SCRIPT, 8, None).unwrap());
        let p = Project::load(&a).unwrap();
        assert_eq!(p.scenes[0].layers.len(), 50);
    }

    #[test]
    fn the_sandbox_has_no_clock_or_modules_and_stops_runaway_loops() {
        assert!(run("timestamp()", 0).is_err());
        assert!(run(r#"import "x" as y; 1"#, 0).is_err());
        assert!(run("loop {}", 0).unwrap_err().contains("operations"));
    }

    #[test]
    fn typos_and_bad_projects_are_errors() {
        let typo = r#"#{ size: [10, 10], fps: 30, scenes: [#{ name: "s", duration: 1, layers: [#{ id: "a", kind: "rect", opcity: 1 }] }] }"#;
        assert!(
            generate(typo, 0, None)
                .unwrap_err()
                .contains("did you mean `opacity`")
        );
        assert!(generate("42", 0, None).is_err());
        assert!(generate("[]", 0, None).unwrap_err().contains("--into"));
    }

    #[test]
    fn colours_and_helpers() {
        assert_eq!(run(r#"hsl(0.0, 1.0, 0.5)"#, 0).unwrap(), "#ff0000");
        assert_eq!(run(r#"rgba(0.0, 0.0, 1.0, 0.5)"#, 0).unwrap(), "#0000ff80");
        let v = run(
            "let a = []; for i in 0..100 { a.push(rand_int(1, 3)) } a",
            0,
        )
        .unwrap();
        let v: Vec<i64> = serde_json::from_value(v).unwrap();
        assert!(v.iter().all(|x| (1..=3).contains(x)) && v.contains(&1) && v.contains(&3));
    }
}

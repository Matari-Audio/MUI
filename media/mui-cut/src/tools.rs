//! The agent's tools on the CLI: `check` (what is wrong, where, when and
//! how to fix it), `sheet` (many frames in one image), `strip` (one layer's
//! motion as onion skins and a trail) and `diff` (what changed between two
//! versions, as pictures). The MCP server calls the same functions.
use std::path::Path;

use mui_cut::check::{Issue, Severity};
use mui_cut::{Project, Renderer};

use crate::{Args, Result, load_assets};

/// Check the file at `path`: its load error, or every lint.
pub fn check_file(path: &Path) -> Result<Vec<Issue>> {
    let src = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let Ok(p) = Project::load(&src) else {
        return Ok(mui_cut::check::check(
            &src,
            &mut Renderer::new(2, 2),
            &|_| true,
        ));
    };
    // Contrast is sampled small: an average over a box needs few pixels.
    let w = 480u16;
    let h = (f64::from(w) * f64::from(p.size[1]) / f64::from(p.size[0]))
        .round()
        .max(2.) as u16;
    let mut r = Renderer::new(w, h);
    let _ = load_assets(&p, path, &mut r.assets);
    let dir = path.parent().unwrap_or(Path::new("."));
    Ok(mui_cut::check::check(&src, &mut r, &|a| {
        dir.join(a).is_file()
    }))
}

pub fn check(args: &Args) -> Result<()> {
    let issues = check_file(&args.project)?;
    let count = |s| issues.iter().filter(|i| i.severity == s).count();
    let (e, w, i) = (
        count(Severity::Error),
        count(Severity::Warning),
        count(Severity::Info),
    );
    if args.has("json") {
        let out = serde_json::json!({ "errors": e, "warnings": w, "infos": i, "issues": issues });
        println!(
            "{}",
            serde_json::to_string_pretty(&out).map_err(|e| e.to_string())?
        );
    } else {
        for issue in &issues {
            println!("{issue}");
        }
        println!(
            "{}: {e} errors, {w} warnings, {i} notes",
            args.project.display()
        );
    }
    if e > 0 {
        return Err(format!("{e} errors"));
    }
    Ok(())
}

//! `mui-migrate [--dry-run] [--diff] [--include-vendored] [--no-docs] [--no-widgets] [--mui-root <dir>] <paths...>`
//!
//! Rewrites Rust sources, their doc examples, Markdown code blocks and
//! Cargo.toml files to the MUI DSL v2 API.
//! See README.md and `src/rules.rs`.

mod cargo;
mod docs;
mod lex;
mod rewrite;
mod rules;
#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "usage: mui-migrate [--dry-run] [--diff] [--include-vendored] [--no-docs] [--no-widgets] [--mui-root <dir>] <paths...>";

fn main() -> ExitCode {
    let (mut dry, mut diff, mut vendored, mut paths) = (false, false, false, Vec::new());
    let (mut docs, mut widgets, mut mui_root) = (true, true, None);
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--mui-root" => match args.next().map(|r| std::fs::canonicalize(&r).unwrap_or(PathBuf::from(r))) {
                Some(r) => mui_root = Some(r),
                None => {
                    eprintln!("--mui-root needs a directory\n{USAGE}");
                    return ExitCode::from(2);
                }
            },
            "--dry-run" | "-n" => dry = true,
            "--diff" => diff = true,
            "--include-vendored" => vendored = true,
            "--no-docs" => docs = false,
            "--no-widgets" => widgets = false,
            "-h" | "--help" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            s if s.starts_with('-') => {
                eprintln!("unknown flag {s}\n{USAGE}");
                return ExitCode::from(2);
            }
            _ => paths.push(PathBuf::from(a)),
        }
    }
    if paths.is_empty() {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    }

    let mut files = Vec::new();
    for p in &paths {
        walk(p, vendored, docs, &mut files);
    }
    // Crate aliases (`mui2 = { package = "mui" }`) and v1 path deps from
    // every manifest we walk plus the ancestors of each argument.
    let mut manifests: Vec<PathBuf> = files.iter().filter(|f| f.ends_with("Cargo.toml")).cloned().collect();
    for p in &paths {
        let abs = std::fs::canonicalize(p).unwrap_or(p.clone());
        manifests.extend(abs.ancestors().map(|a| a.join("Cargo.toml")).filter(|m| m.is_file()));
    }
    let (mut v2, mut v1) = (Vec::new(), Vec::new());
    for m in &manifests {
        let dir = std::fs::canonicalize(m).ok().and_then(|p| p.parent().map(Path::to_path_buf)).unwrap_or_default();
        if let Ok(s) = std::fs::read_to_string(m) {
            let (a, b) = cargo::mui_deps(&s, &dir, mui_root.as_deref());
            v2.extend(a);
            v1.extend(b);
        }
    }
    let mut ctx = rewrite::Ctx::with_roots(v2);
    for k in &v1 {
        ctx.roots.remove(k);
    }
    ctx.widgets = widgets;
    let sources: Vec<Option<String>> = files.iter().map(|f| std::fs::read_to_string(f).ok()).collect();
    for (f, src) in files.iter().zip(&sources) {
        if f.ends_with("Cargo.toml")
            && let Some(name) = src.as_deref().and_then(cargo::package)
            && rules::MUI_CRATES.contains(&name.as_str())
            && let Some(dir) = f.parent()
        {
            ctx.own_crate(dir);
        }
    }
    for (f, src) in files.iter().zip(&sources) {
        if let Some(src) = src
            && f.extension().is_some_and(|e| e == "rs")
        {
            ctx.index(f, src);
        }
    }

    let (mut changed, mut edits, mut warnings, mut errors) = (0, 0, 0, 0);
    for (f, src) in files.iter().zip(sources) {
        let Some(src) = src else {
            eprintln!("{}: unreadable", f.display());
            errors += 1;
            continue;
        };
        let result = if f.ends_with("Cargo.toml") {
            let dir = std::fs::canonicalize(f).ok().and_then(|p| p.parent().map(Path::to_path_buf)).unwrap_or_default();
            cargo::rewrite(&src, &dir).map(|(text, edits)| rewrite::Outcome { text, edits, warnings: Vec::new() })
        } else if f.extension().is_some_and(|e| e == "md") {
            docs::migrate_docs(&src, Some(f), &ctx, true, true)
        } else {
            rewrite::migrate(&src, Some(f), &ctx).and_then(|code| {
                if !docs {
                    return Ok(code);
                }
                let host = rewrite::uses_mui(&code.text, Some(f), &ctx);
                let mut d = docs::migrate_docs(&code.text, Some(f), &ctx, false, host)?;
                d.edits += code.edits;
                d.warnings.extend(code.warnings);
                d.warnings.sort();
                d.warnings.dedup();
                Ok(d)
            })
        };
        let out = match result {
            Ok(o) => o,
            Err(e) => {
                eprintln!("{}: error: {e}", f.display());
                errors += 1;
                continue;
            }
        };
        for (line, note) in &out.warnings {
            println!("{}:{line}: {note}", f.display());
        }
        warnings += out.warnings.len();
        if out.text == src {
            continue;
        }
        changed += 1;
        edits += out.edits;
        println!("{}: {} edit{}", f.display(), out.edits, if out.edits == 1 { "" } else { "s" });
        if diff {
            let name = f.display().to_string();
            print!("{}", similar::TextDiff::from_lines(&src, &out.text).unified_diff().header(&name, &name));
        }
        if !dry && let Err(e) = std::fs::write(f, &out.text) {
            eprintln!("{}: write failed: {e}", f.display());
            errors += 1;
        }
    }
    println!(
        "{} files scanned, {changed} {}, {edits} edits, {warnings} manual warnings, {errors} errors",
        files.len(),
        if dry { "would change" } else { "changed" }
    );
    if errors > 0 { ExitCode::FAILURE } else { ExitCode::SUCCESS }
}

fn walk(p: &Path, vendored: bool, docs: bool, out: &mut Vec<PathBuf>) {
    if p.is_file() {
        if p.extension().is_some_and(|e| e == "rs" || (docs && e == "md")) || p.ends_with("Cargo.toml") {
            out.push(p.to_path_buf());
        }
        return;
    }
    let Ok(rd) = std::fs::read_dir(p) else { return };
    let mut entries: Vec<_> = rd.filter_map(Result::ok).map(|e| e.path()).collect();
    entries.sort();
    for e in entries {
        let name = e.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if e.is_dir() && (matches!(name, "target" | ".git") || (!vendored && matches!(name, "vendor" | ".build-inputs"))) {
            continue;
        }
        walk(&e, vendored, docs, out);
    }
}

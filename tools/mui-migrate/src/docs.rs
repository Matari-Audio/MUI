//! Docs mode: the Rust code blocks inside `///` / `//!` doc comments and in
//! Markdown files, migrated with the same rules as code.
//!
//! Each block is cut out as a *shadow*: the same bytes, with the comment
//! markers and rustdoc's hidden-line `#` blanked to spaces. Offsets into the
//! shadow are offsets into the file, so the rules' edits land in place and
//! the comment markers around them survive.

use crate::lex::lex;
use crate::rewrite::{Ctx, Edit, File, Outcome, apply, widen};
use std::path::Path;

/// A code block: where its first line starts, and its shadow.
struct Block {
    lo: usize,
    shadow: String,
}

/// Migrate the doc examples in `src`. `md` for a Markdown file, where every
/// block is taken to be mui code; in a `.rs` file a block is mui when it
/// imports mui itself or `host_mui` (the file around it uses mui).
pub fn migrate_docs(src: &str, path: Option<&Path>, ctx: &Ctx, md: bool, host_mui: bool) -> Result<Outcome, String> {
    let mut out = Outcome { text: src.to_string(), ..Default::default() };
    for pass in 0..16 {
        let mut edits = Vec::new();
        let mut warnings = Vec::new();
        for b in blocks(&out.text, md) {
            let first = out.text[..b.lo].matches('\n').count();
            let Ok(toks) = lex(&b.shadow) else {
                if pass == 0 {
                    warnings.push((first + 1, "doc example does not lex; not migrated".to_string()));
                }
                continue;
            };
            let f = File::new(&b.shadow, &toks, ctx, path, md || host_mui);
            if pass == 0 {
                warnings.extend(f.manual().into_iter().map(|(l, n)| (first + l, n)));
            }
            let (found, warns) = f.pass();
            warnings.extend(warns.into_iter().map(|(l, n)| (first + l, n)));
            for mut e in found {
                if e.remove {
                    widen(&b.shadow, 0, &mut e);
                }
                edits.push(Edit { lo: e.lo + b.lo, hi: e.hi + b.lo, text: e.text, remove: false });
            }
        }
        for w in warnings {
            if !out.warnings.contains(&w) {
                out.warnings.push(w);
            }
        }
        if edits.is_empty() {
            out.warnings.sort();
            return Ok(out);
        }
        let (text, n) = apply(&out.text, edits);
        out.text = text;
        out.edits += n;
    }
    Err("doc rewrite did not converge".into())
}

/// Tags rustdoc compiles as Rust; a fence with none of them (or with another
/// language) is not Rust.
const RUST_TAGS: &[&str] = &["rust", "ignore", "no_run", "should_panic", "compile_fail", "test_harness"];

fn is_rust(info: &str, md: bool) -> bool {
    let tags: Vec<&str> = info.split([',', ' ']).map(str::trim).filter(|t| !t.is_empty()).collect();
    if md {
        return tags.first() == Some(&"rust");
    }
    tags.iter().all(|t| RUST_TAGS.contains(t) || t.starts_with("edition"))
}

fn blocks(src: &str, md: bool) -> Vec<Block> {
    let mut out = Vec::new();
    // (block start, shadow so far, closing fence, is Rust) inside a block.
    let mut open: Option<(usize, String, &str, bool)> = None;
    let mut at = 0;
    for line in src.split_inclusive('\n') {
        at += line.len();
        let body = line.strip_suffix('\n').unwrap_or(line);
        // The marker ends where the doc text starts; `md` has none.
        let marker = if md {
            Some(0)
        } else {
            let t = body.trim_start();
            let ws = body.len() - t.len();
            (t.starts_with("//!") || (t.starts_with("///") && !t.starts_with("////"))).then_some(ws + 3)
        };
        let Some(m) = marker else {
            open = None; // a doc comment ended mid-block
            continue;
        };
        let text = &body[m..];
        let fence = text.trim_start();
        match &mut open {
            Some((start, shadow, close, rust)) if fence.starts_with(*close) && fence[close.len()..].trim().is_empty() => {
                // A before/after example (`// old` .. `// new`) documents the old
                // API on purpose: it is left as written.
                let frozen = shadow.lines().any(|l| matches!(l.trim(), "// old" | "// before"));
                if *rust && !frozen {
                    out.push(Block { lo: *start, shadow: std::mem::take(shadow) });
                }
                open = None;
            }
            Some((_, _, _, false)) => {}
            Some((_, shadow, _, true)) => {
                shadow.push_str(&" ".repeat(m));
                let t = text.trim_start();
                let ws = text.len() - t.len();
                if t == "#" || t.starts_with("# ") {
                    shadow.push_str(&text[..ws]);
                    shadow.push(' ');
                    shadow.push_str(&t[1..]);
                } else {
                    shadow.push_str(text);
                }
                shadow.push_str(&line[body.len()..]);
            }
            None => {
                for close in ["```", "~~~"] {
                    if let Some(info) = fence.strip_prefix(close) {
                        open = Some((at, String::new(), close, is_rust(info, md)));
                    }
                }
            }
        }
    }
    out
}

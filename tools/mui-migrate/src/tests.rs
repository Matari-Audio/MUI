use crate::docs::migrate_docs;
use crate::rewrite::{Ctx, Outcome, migrate};

fn run(src: &str) -> Outcome {
    run_in(src, &Ctx::with_roots([]))
}

fn widgets() -> Ctx {
    let mut ctx = Ctx::with_roots([]);
    ctx.widgets = true;
    ctx
}

fn run_in(src: &str, ctx: &Ctx) -> Outcome {
    let out = migrate(src, None, ctx).expect("migrates");
    let again = migrate(&out.text, None, ctx).expect("migrates twice");
    assert_eq!(again.text, out.text, "not idempotent");
    assert_eq!(again.edits, 0);
    out
}

fn check(before: &str, after: &str) {
    assert_eq!(run(before).text, after);
}

const P: &str = "use mui::prelude::*;\n";

fn with_prelude(body: &str) -> String {
    format!("{P}{body}")
}

#[test]
fn functions_and_use_lists() {
    check(
        "use mui_scene::{leaf, column, overlay, row};\nfn f() { column([leaf(1.0, 2.0), overlay([])]) }\n",
        "use mui_scene::{block, col, stack, row};\nfn f() { col([block(1.0, 2.0), stack([])]) }\n",
    );
    check(&with_prelude("fn f() { label(\"x\"); canvas_cached(k, g); resolve_scene(&s) }\n"), &with_prelude("fn f() { body(\"x\"); canvas_keyed(k, g); resolve(&s) }\n"));
}

#[test]
fn function_guards() {
    // Local definitions, foreign paths, and non-mui files are left alone.
    let local = "use mui::prelude::*;\nfn label(x: u8) -> u8 { x }\nfn f() { label(1); }\n";
    check(local, local);
    let foreign = "use mui::prelude::*;\nfn f() { kit::column(1); other::leaf(2); }\n";
    check(foreign, foreign);
    let no_mui = "fn f() { column(1); x.label(\"a\"); x.disabled(true); }\n";
    check(no_mui, no_mui);
    // Methods in an impl don't shadow the free fn.
    check(
        "use mui::prelude::*;\nimpl A { fn label(self) -> &'static str { \"\" } }\nfn f(a: A) { label(a.label()); }\n",
        "use mui::prelude::*;\nimpl A { fn label(self) -> &'static str { \"\" } }\nfn f(a: A) { body(a.label()); }\n",
    );
    // `let name = |..|` shadows too.
    let closure = "use mui::prelude::*;\nfn f() { let leaf = |x| x; leaf(1); }\n";
    check(closure, closure);
}

#[test]
fn strings_and_comments_untouched() {
    let src = with_prelude("// column(leaf(1)) and .role(Kind::X)\n/// leaf(1.0, 2.0)\nfn f() { let s = \"column(leaf(1))\"; /* overlay([]) */ r#\"Kind::X\"#; }\n");
    check(&src, &src);
}

#[test]
fn macro_bodies_rewritten() {
    check(
        &with_prelude("fn f() { row![leaf(1.0, 1.0).disabled(true), column(vec![leaf(2.0, 2.0)])]; Node::<u8>::overlay([]); }\n"),
        &with_prelude("fn f() { row![block(1.0, 1.0).disabled(), col(vec![block(2.0, 2.0)])]; Node::<u8>::stack([]); }\n"),
    );
}

#[test]
fn alias_mui2() {
    check(
        "use mui2::prelude::{Kind, Sugar, El};\nfn f() -> mui2::scene::Kind { mui2::scene::leaf(1.0, 1.0).role(Kind::Group); Kind::Group }\n",
        "use mui2::prelude::{A11y, El};\nfn f() -> mui2::scene::A11y { mui2::scene::block(1.0, 1.0).a11y(A11y::Group); A11y::Group }\n",
    );
}

#[test]
fn type_guard() {
    // A local `Kind` is not the a11y enum.
    let local = "use mui::prelude::*;\nenum Kind { A }\nfn f() -> Kind { Kind::A }\n";
    check(local, local);
    let foreign = "use crate::cards::Kind;\nuse mui::prelude::El;\nfn f() -> Kind { Kind::A }\n";
    check(foreign, foreign);
}

#[test]
fn methods_and_call_shapes() {
    check(
        &with_prelude(
            "fn f() {\n    e.label(name).role(Kind::Slider { v })\n        .disabled(false)\n        .disabled(!on)\n        .transition(s)\n        .layout_transition(s)\n        .scroll_bar(false)\n        .scroll_bar(show)\n        .scroll_bar(a && b);\n    ramp.transition(0.3, 0.6);\n    a.role(accent.role());\n    pal.disabled(c);\n}\n",
        ),
        &with_prelude(
            "fn f() {\n    e.named(name).a11y(A11y::Slider { v })\n        .when(!on, |e| e.disabled())\n        .animate_with(s)\n        .animate_layout_with(s)\n        .no_scrollbar()\n        .when(!show, |e| e.no_scrollbar())\n        .when(!(a && b), |e| e.no_scrollbar());\n    ramp.transition(0.3, 0.6);\n    a.role(accent.role());\n    pal.disabled(c);\n}\n",
        ),
    );
}

#[test]
fn anchor_offset() {
    check(
        &with_prelude("fn f() {\n    a.anchor(Align::Start, Align::Start).offset(x, y + 1.0);\n    b.anchor(Align::Center, Align::Center)\n        .offset(dx, dy);\n    c.anchor(Align::End, Align::Start).offset(x, y);\n}\n"),
        &with_prelude("fn f() {\n    a.at(x, y + 1.0);\n    b.centered_at(dx, dy);\n    c.anchor(Align::End, Align::Start).offset(x, y);\n}\n"),
    );
}

#[test]
fn weld() {
    check(
        &with_prelude("fn f() { e.weld_shape().weld_borders().weld_morph(p).weld_quality(q).without_weld().exclude_from_weld().weld_with(w).gpu_weld(w); weld_morph![0.5; a, b]; weld_morph![Weld::shape(), 0.75; a] }\n"),
        &with_prelude("fn f() { e.weld(Weld::shape()).weld(Weld::borders().morph(p).quality(q)).weld(Weld::off()).unwelded().weld(w).weld(w); weld![Weld::default().morph(0.5); a, b]; weld![Weld::shape().morph(0.75); a] }\n"),
    );
    let out = run("use mui::scene::El;\nfn f(e: El) -> El { e.weld_shape() }\n");
    assert!(out.warnings.iter().any(|(_, w)| w.contains("`Weld`")), "{:?}", out.warnings);
}

#[test]
fn join_only_on_chains() {
    check(
        &with_prelude("fn f() { row([a, b]).join(); handle.join(); }\n"),
        &with_prelude("fn f() { row([a, b]).segmented(); handle.join(); }\n"),
    );
}

#[test]
fn tuple_fields() {
    let check = |before: &str, after: &str| assert_eq!(run_in(before, &widgets()).text, after);
    let run = |src: &str| run_in(src, &widgets());
    // The widget phase is opt-in.
    let tuple = with_prelude("fn f() { let a = knob(ui, id, \"G\", &mut v, r).0; }\n");
    assert_eq!(super::tests::run(&tuple).text, tuple);
    check(
        &with_prelude("fn f() {\n    let a = knob(ui, id, \"G\", &mut v, 0.0..=1.0).0;\n    let hit = button(ui, id, \"Go\").1;\n    let (el, changed) = toggle(ui, id, &mut on);\n    let (field, _) = mui::widgets::text_input(ui, id, &mut s);\n    let (h, p) = ui.state(&id);\n    let (x, y) = kit::button(ui, id, \"a\", accent);\n}\n"),
        &with_prelude("fn f() {\n    let a = knob(ui, id, \"G\", &mut v, 0.0..=1.0).el;\n    let hit = button(ui, id, \"Go\").changed;\n    let Response { el, changed } = toggle(ui, id, &mut on);\n    let Response { el: field, .. } = mui::widgets::text_input(ui, id, &mut s);\n    let Interaction { hover: h, press: p } = ui.state(&id);\n    let (x, y) = kit::button(ui, id, \"a\", accent);\n}\n"),
    );
    // Explicit imports: no glob, so the new type needs an import.
    let out = run("use mui::widgets::knob;\nfn f() { let (el, c) = knob(ui, id, \"G\", &mut v, r); }\n");
    assert!(out.text.contains("let Response { el, changed: c } = knob("));
    assert!(out.warnings.iter().any(|(_, w)| w.contains("`Response`")));
    // KURV's own `button` (imported from elsewhere) is not the widget.
    let own = "use mui2::prelude::*;\nuse super::controls::button;\nfn f() { let (a, b) = button(ui, id, \"x\", accent); }\n";
    check(own, own);
}

#[test]
fn qualify_role_variants() {
    check(&with_prelude("fn f() { e.fill(Raised).fill(Role::Dim).fill(Level(2)); }\n"), &with_prelude("fn f() { e.fill(Role::Raised).fill(Role::Dim).fill(Role::Level(2)); }\n"));
    // Local enums with the same variant names are left alone.
    let local = "use mui::prelude::*;\nenum Slot { Level(f32), Field }\nfn f() {}\n";
    check(local, local);
    let foreign_glob = "use mui::prelude::*;\nuse crate::params::*;\nfn f() { g(Level); }\n";
    check(foreign_glob, foreign_glob);
}

#[test]
fn drop_import() {
    check("use mui2::prelude::Sugar;\nuse mui2::prelude::{El, IntoLen};\nfn f() {}\n", "use mui2::prelude::{El};\nfn f() {}\n");
    check("pub use mui::prelude::{Sugar as _, El};\n", "pub use mui::prelude::{El};\n");
    check("use std::fmt;\n    use mui2::prelude::Sugar;\nfn f() {}\n", "use std::fmt;\nfn f() {}\n");
}

#[test]
fn manual_warnings() {
    let src = with_prelude("fn f() {\n    let s = resolve_scene_cached(&spec, &mut c);\n    let t = \"resolve_scene_retained\";\n    impl Sugar for X {}\n}\n");
    let out = run(&src);
    let lines: Vec<usize> = out.warnings.iter().map(|w| w.0).collect();
    assert_eq!(lines, [3, 5], "{:?}", out.warnings);
    assert_eq!(out.text, src);
}

#[test]
fn doc_example_before_after() {
    // The spec's before/after, minus its `text` -> `body` inconsistency.
    let before = with_prelude(
        "fn f() -> El {\ncolumn([\n    overlay([\n        leaf(size, size).pill().preset(look.face(Role::Raised))\n            .role(Kind::Slider { value, min, max }).label(label.clone())\n            .focusable().id(id),\n        leaf(dot, dot).pill().fill(look.role)\n            .anchor(Align::Center, Align::Center).offset(x, y),\n    ]),\n    text(caption).fill(Role::Dim),\n]).gap(Xs).align(Align::Center)\n}\n",
    );
    let after = with_prelude(
        "fn f() -> El {\ncol([\n    stack([\n        block(size, size).pill().preset(look.face(Role::Raised))\n            .a11y(A11y::Slider { value, min, max }).named(label.clone())\n            .focusable().id(id),\n        block(dot, dot).pill().fill(look.role)\n            .centered_at(x, y),\n    ]),\n    text(caption).fill(Role::Dim),\n]).gap(Xs).align(Align::Center)\n}\n",
    );
    check(&before, &after);
}

#[test]
fn cross_file_super_imports() {
    // `child.rs` imports mui names through its parent's glob; the parent
    // also defines its own `button`, which is not the widget.
    let dir = std::env::temp_dir().join(format!("mui-migrate-test-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("ui")).unwrap();
    let parent = dir.join("ui.rs");
    let child = dir.join("ui/child.rs");
    let parent_src = "mod child;\nuse mui2::prelude::*;\npub fn button(a: u8) -> (El, bool) { todo!() }\n";
    let child_src = "use super::{Kind, Sugar, El, button, col, column};\nfn f() -> El { let (a, b) = button(1); column([a]).role(Kind::Group) }\n";
    std::fs::write(&parent, parent_src).unwrap();
    std::fs::write(&child, child_src).unwrap();
    let mut ctx = Ctx::with_roots([]);
    ctx.index(&parent, parent_src);
    ctx.index(&child, child_src);
    let out = migrate(child_src, Some(&child), &ctx).unwrap();
    assert_eq!(out.text, "use super::{A11y, El, button, col};\nfn f() -> El { let (a, b) = button(1); col([a]).a11y(A11y::Group) }\n");
    // Without the index the child is not recognisably mui.
    assert_eq!(migrate(child_src, None, &ctx).unwrap().text, child_src);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn manual_pattern_with_an_open_paren() {
    let src = with_prelude("fn f() {\n    color_picker (ui, id, &mut c, true);\n}\n");
    let out = run_in(&src, &widgets());
    assert_eq!(out.warnings.iter().map(|w| w.0).collect::<Vec<_>>(), [3], "{:?}", out.warnings);
    assert!(run(&src).warnings.is_empty(), "widget rules are opt-in");
}

#[test]
fn free_fn_call_shape() {
    check(
        "use mui_scene::{TextCache, resolve_scene_with};\nfn f() { let mut c = TextCache::default(); let s = resolve_scene_with(&spec, &mut c); mui_scene::resolve_scene_with(&a, &mut self.c); }\n",
        "use mui_scene::{Resolver};\nfn f() { let mut c = Resolver::default(); let s = c.resolve(&spec); self.c.resolve(&a); }\n",
    );
}

#[test]
fn own_crate_names() {
    // Inside a mui crate, the crate's own names are mui names: `crate::`,
    // `Self::`, bare names with no visible source. A name the crate defines
    // (here its own `Kind`) is still left alone.
    let dir = std::env::temp_dir().join(format!("mui-migrate-own-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("src")).unwrap();
    let lib = dir.join("src/lib.rs");
    let node = dir.join("src/node.rs");
    let lib_src = "mod node;\npub(crate) use node::Kind;\npub fn f() { crate::leaf(1.0, 1.0); Self::column([]); overlay([]); let k: Kind = Kind::A; let a: crate::Kind = x; }\n";
    let node_src = "pub(crate) enum Kind { A }\n";
    std::fs::write(&lib, lib_src).unwrap();
    std::fs::write(&node, node_src).unwrap();
    let mut ctx = Ctx::with_roots([]);
    ctx.own_crate(&dir);
    ctx.index(&lib, lib_src);
    ctx.index(&node, node_src);
    let out = migrate(lib_src, Some(&lib), &ctx).unwrap();
    assert_eq!(out.text, "mod node;\npub(crate) use node::Kind;\npub fn f() { crate::block(1.0, 1.0); Self::col([]); stack([]); let k: Kind = Kind::A; let a: crate::A11y = x; }\n");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn doc_comment_examples() {
    let ctx = Ctx::with_roots([]);
    let src = "/// A card.\n///\n/// ```\n/// # use mui::prelude::*;\n/// let c = leaf(1., 1.)\n///     .disabled(false)\n///     .role(Kind::Button);\n/// ```\n///\n/// ```text\n/// leaf(1., 1.)\n/// ```\n//! ```rust,ignore\n//! column([]);\n//! ```\nfn leaf() {}\n";
    let out = migrate_docs(src, None, &ctx, false, false).unwrap();
    assert_eq!(
        out.text,
        "/// A card.\n///\n/// ```\n/// # use mui::prelude::*;\n/// let c = block(1., 1.)\n///     .a11y(A11y::Button);\n/// ```\n///\n/// ```text\n/// leaf(1., 1.)\n/// ```\n//! ```rust,ignore\n//! column([]);\n//! ```\nfn leaf() {}\n",
        "the second rust block has no mui import and the host file does not use mui"
    );
    assert_eq!(migrate_docs(&out.text, None, &ctx, false, false).unwrap().edits, 0);
    let hosted = migrate_docs(src, None, &ctx, false, true).unwrap();
    assert!(hosted.text.contains("//! col([]);"));
}

#[test]
fn markdown_blocks() {
    let ctx = Ctx::with_roots([]);
    let src = "# Title\n\n`leaf(1., 1.)` in prose stays.\n\n```rust\nlet t = overlay([label(\"x\")]);\n```\n\n```sh\nleaf(1)\n```\n\n```rust,ignore\nbridge.bind(ui, \"gain\", P::Gain, |ui, v| knob(ui, \"gain\", v))\n```\n";
    let out = migrate_docs(src, None, &ctx, true, true).unwrap();
    assert_eq!(
        out.text,
        "# Title\n\n`leaf(1., 1.)` in prose stays.\n\n```rust\nlet t = stack([body(\"x\")]);\n```\n\n```sh\nleaf(1)\n```\n\n```rust,ignore\nbridge.bind(ui, P::Gain, |ui, id, v| knob(ui, \"gain\", v))\n```\n"
    );
    assert_eq!(out.warnings.iter().map(|w| w.0).collect::<Vec<_>>(), [14]);
}

#[test]
fn new_name_collides_with_a_local_definition() {
    let out = run("use mui_layout::{leaf, row};\nfn block(i: usize) -> u8 { 0 }\nfn f() { row([leaf(1., 1.)]); }\n");
    assert!(out.warnings.iter().any(|(l, w)| *l == 1 && w.contains("`block`")), "{:?}", out.warnings);
    let out = run("use mui_scene::resolve_scene;\nfn f(s: &S) { let resolve = 1; resolve_scene(s); }\n");
    assert!(out.warnings.iter().any(|(_, w)| w.contains("`resolve`")), "{:?}", out.warnings);
}

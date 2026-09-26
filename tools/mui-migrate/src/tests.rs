use crate::docs::migrate_docs;
use crate::rewrite::{Ctx, Outcome, migrate};

fn run(src: &str) -> Outcome {
    run_in(src, &Ctx::with_roots([]))
}

fn no_widgets() -> Ctx {
    let mut ctx = Ctx::with_roots([]);
    ctx.widgets = false;
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
            "fn f(e: El) {\n    e.label(name).role(Kind::Slider { v })\n        .disabled(false)\n        .disabled(!on)\n        .transition(s)\n        .layout_transition(s)\n        .scroll_bar(false)\n        .scroll_bar(show)\n        .scroll_bar(a && b);\n    ramp.transition(0.3, 0.6);\n    a.role(accent.role());\n    pal.disabled(c);\n}\n",
        ),
        &with_prelude(
            "fn f(e: El) {\n    e.named(name).a11y(A11y::Slider { v })\n        .when(!on, Styled::disabled)\n        .animate_with(s)\n        .animate_layout_with(s)\n        .no_scrollbar()\n        .when(!show, Styled::no_scrollbar)\n        .when(!(a && b), Styled::no_scrollbar);\n    ramp.transition(0.3, 0.6);\n    a.role(accent.role());\n    pal.disabled(c);\n}\n",
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
    // `--no-widgets` turns the widget phase off.
    let tuple = with_prelude("fn f() { let a = knob(ui, id, \"G\", &mut v, r).0; }\n");
    assert_eq!(run_in(&tuple, &no_widgets()).text, tuple);
    check(
        &with_prelude("fn f() {\n    let a = knob(ui, id, \"G\", &mut v, 0.0..=1.0).0;\n    let hit = button(ui, id, \"Go\").1;\n    let (el, changed) = toggle(ui, id, \"On\", &mut on);\n    let (field, _) = mui::widgets::text_input(ui, id, &mut s);\n    let (mut dial, _) = knob(ui, id, \"G\", &mut v, r);\n    let (h, p) = ui.state(&id);\n    let (x, y) = kit::button(ui, id, \"a\", accent);\n}\n"),
        &with_prelude("fn f() {\n    let a = knob(ui, id, \"G\", &mut v, 0.0..=1.0).el;\n    let hit = button(ui, id, \"Go\").changed;\n    let Response { el, changed } = toggle(ui, id, \"On\", &mut on);\n    let field = mui::widgets::text_input(ui, id, &mut s).el;\n    let mut dial = knob(ui, id, \"G\", &mut v, r).el;\n    let Interaction { hover: h, press: p } = ui.state(&id);\n    let (x, y) = kit::button(ui, id, \"a\", accent);\n}\n"),
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
fn element_flags() {
    // Writes through `payload_mut()` become `set`; reads are flagged.
    let src = "use mui::prelude::*;\nfn f(n: &mut El) { let a = n.payload().focusable; n.payload_mut().disabled = !on; n.payload_mut().scroll_bar_heat = None; let b = n.payload_mut().baseline; }\n";
    let out = run(src);
    assert_eq!(out.text, "use mui::prelude::*;\nfn f(n: &mut El) { let a = n.payload().focusable; n.payload_mut().set(Element::DISABLED, !on); n.payload_mut().scroll_bar_heat = None; let b = n.payload_mut().baseline; }\n");
    assert_eq!(out.warnings.len(), 3, "{:?}", out.warnings);
    // Not a payload: `state.disabled = x` is left alone.
    let other = "use mui::prelude::*;\nfn f(s: &mut S) { s.disabled = true; s.payload.disabled = true; }\n";
    check(other, other);
}

#[test]
fn capture_moved_to_material() {
    let src = "use mui::prelude::*;\nfn f(e: mui::scene::CaptureError) { mui_scene::resize_capture(&t, k, s); }\n";
    let out = run(src);
    assert_eq!(out.warnings.len(), 2, "{:?}", out.warnings);
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
fn before_after_doc_blocks_stay_as_written() {
    let src = "```rust\n// old\nlet t = leaf(1., 1.);\n// new\nlet t = block(1., 1.);\n```\n\n```rust\nlet t = leaf(1., 1.);\n```\n";
    let out = migrate_docs(src, None, &Ctx::with_roots([]), true, true).unwrap();
    assert_eq!(out.text, src.replacen("let t = leaf(1., 1.);\n```\n", "let t = block(1., 1.);\n```\n", 1).replace("```rust\nlet t = leaf", "```rust\nlet t = block"));
}

#[test]
fn a_local_closure_only_shadows_its_own_block() {
    // `let button = |..|` in one fn is that fn's; the widget elsewhere still migrates.
    check(
        &with_prelude("fn a() {\n    let button = |ui, id| (1, 2);\n    let (x, y) = button(ui, id);\n}\nfn b() {\n    let (save, _) = button(ui, \"s\", \"Save\");\n}\n"),
        &with_prelude("fn a() {\n    let button = |ui, id| (1, 2);\n    let (x, y) = button(ui, id);\n}\nfn b() {\n    let save = button(ui, \"s\", \"Save\").el;\n}\n"),
    );
}

#[test]
fn own_crate_module_import() {
    // Inside mui itself, `use crate::widgets;` names the crate's own module.
    let dir = std::env::temp_dir().join(format!("mui-migrate-own-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("Cargo.toml"), "[package]\nname = \"mui\"\n").unwrap();
    let (lib, ui) = (dir.join("src/lib.rs"), dir.join("src/ui.rs"));
    let lib_src = "pub mod widgets;\nmod ui;\n";
    let ui_src = "use mui_scene::El;\nuse crate::widgets;\nfn f() -> El { widgets::text_input(ui, \"f\", v).0 }\n";
    std::fs::write(&lib, lib_src).unwrap();
    std::fs::write(&ui, ui_src).unwrap();
    let mut ctx = Ctx::with_roots([]);
    ctx.own_crate(&dir);
    ctx.index(&lib, lib_src);
    ctx.index(&ui, ui_src);
    let out = migrate(ui_src, Some(&ui), &ctx).unwrap();
    assert!(out.text.contains("text_input(ui, \"f\", v).el"), "{}", out.text);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn manual_pattern_with_an_open_paren() {
    // `Bridge::bind(` is a call; a wgpu `.bind` field is not.
    let src = with_prelude("fn f() {\n    let g = desc.bind;\n    bridge.bind (ui, P::Gain, |ui, id, v| knob(ui, id, \"G\", v, r));\n}\n");
    let out = run(&src);
    assert_eq!(out.warnings.iter().map(|w| w.0).collect::<Vec<_>>(), [4], "{:?}", out.warnings);
}

#[test]
fn widget_calls() {
    check(
        &with_prelude("fn f() {\n    let s = toggle(ui, \"b\", &mut on).0.size(S).el();\n    row![drag_value(ui, \"bpm\", &mut bpm, 20.0..=300.0).0];\n    mui::widgets::toggle(ui, id, &mut on);\n    color_picker(ui, \"a\", &mut c, true);\n    color_picker(ui, \"b\", &mut c, false);\n    color_picker(ui, \"c\", &mut c, self.alpha);\n    color_picker(ui, \"d\", &mut c, ColorOpts::default());\n    let (plot, edit) = bins(ui, \"spec\", &model);\n    bins(ui, \"spec\", &mut model);\n    let el = knob(ui, id, \"G\", v, r).0.el();\n    let el = knob(ui, id, \"G\", v, r).0.into();\n    let h = ui.state(&id).0;\n}\n"),
        &with_prelude("fn f() {\n    let s = toggle(ui, \"b\", \"\", &mut on).el.size(S).el();\n    row![drag_value(ui, \"bpm\", \"\", &mut bpm, 20.0..=300.0).el];\n    mui::widgets::toggle(ui, id, \"\", &mut on);\n    color_picker(ui, \"a\", &mut c, ColorOpts { alpha: true });\n    color_picker(ui, \"b\", &mut c, ColorOpts::default());\n    color_picker(ui, \"c\", &mut c, ColorOpts { alpha: self.alpha });\n    color_picker(ui, \"d\", &mut c, ColorOpts::default());\n    let Response { el: plot, changed: edit } = bins(ui, \"spec\", &mut model);\n    bins(ui, \"spec\", &mut model);\n    let el = knob(ui, id, \"G\", v, r).el.into_el();\n    let el = knob(ui, id, \"G\", v, r).el.into_el();\n    let h = ui.state(&id).hover;\n}\n"),
    );
    // A project's own `toggle` keeps its shape.
    let own = "use mui::prelude::*;\nfn toggle(a: u8, b: u8, c: u8) {}\nfn f() { toggle(1, 2, 3); }\n";
    check(own, own);
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
    let tests = dir.join("src/tests.rs");
    let out = migrate("use super::*;\nfn t() { Node::<u8>::column([]); }\n", Some(&tests), &ctx).unwrap();
    assert_eq!(out.text, "use super::*;\nfn t() { Node::<u8>::col([]); }\n");
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
    assert!(out.warnings.iter().any(|(l, w)| *l == 3 && w.contains("`block`")), "{:?}", out.warnings);
    let out = run("use mui_scene::resolve_scene;\nfn f(s: &S) { let resolve = 1; resolve_scene(s); }\n");
    assert!(out.warnings.iter().any(|(_, w)| w.contains("`resolve`")), "{:?}", out.warnings);
    // A closure parameter named like the old function is not a call.
    let out = run("use mui::prelude::*;\nfn f() { let body = 1; let g = |label: &str| label.len(); }\n");
    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
}

#[test]
fn geometry_moves_and_associated_calls() {
    check(
        "use mui_geometry::Spacing;\nfn f() -> mui_geometry::SpacingToken { mui_geometry::SpacingToken::M }\n",
        "use mui_layout::Spacing;\nfn f() -> mui_layout::SpacingToken { mui_layout::SpacingToken::M }\n",
    );
    check("use mui_geometry::{Spacing, SpacingToken};\n", "use mui_layout::{Spacing, SpacingToken};\n");
    check(
        "use mui_geometry::{Point, Spacing};\nfn f() {}\n",
        "use mui_layout::Spacing;\nuse mui_geometry::{Point};\nfn f() {}\n",
    );
    check(
        "use mui_geometry::{Affine, Bounds};\nfn f() { let b = Bounds::from_points(ps); Affine::translation(1.0, 2.0); Affine::scale(2.0, 3.0); Affine::scale(2.0); Affine::rotation_about(a, p); }\n",
        "use mui_geometry::{Affine, Rect};\nfn f() { let b = mui_geometry::bounds(ps); Affine::translate((1.0, 2.0)); Affine::scale_non_uniform(2.0, 3.0); Affine::scale(2.0); Affine::rotate_about(a, p); }\n",
    );
    check(
        "use mui_geometry::Bounds;\nfn f(b: &Bounds, q: Q) -> f64 { b.min.x + b.max.y + p.bounds().min.y + q.min.x }\n",
        "use mui_geometry::Rect;\nfn f(b: &Rect, q: Q) -> f64 { b.x0 + b.y1 + p.bounds().min.y + q.min.x }\n",
    );
}

#[test]
fn ui_fields_become_accessors_and_the_wheel_a_vec2() {
    check(
        &with_prelude("fn f(ui: &mut Ui) { ui.scale = Some(2.0); let s = self.ui.scale.unwrap_or(1.0); let c = ui.theme.control; ui.theme = t; ui.font = Some(f); other.scale = 3.0; }\n"),
        &with_prelude("fn f(ui: &mut Ui) { ui.set_scale(Some(2.0)); let s = self.ui.scale().unwrap_or(1.0); let c = ui.theme().control; ui.set_theme(t); ui.set_font(Some(f)); other.scale = 3.0; }\n"),
    );
    check(
        &with_prelude("fn f() { let p = PointerInput { wheel: Point::new(0.0, 3.0), ..Default::default() }; if r.wheel != Point::ZERO {} let at: Point = Point::ZERO; }\n").replace("f()", "f(r: &Response)"),
        &with_prelude("fn f(r: &Response) { let p = PointerInput { wheel: Vec2::new(0.0, 3.0), ..Default::default() }; if r.wheel != Vec2::ZERO {} let at: Point = Point::ZERO; }\n"),
    );
    check(
        &with_prelude("fn f() { p.translate(Point::new(1.0, 2.0)); p.rigid_transform(Point::new(x, y), a); r.translated(d); Plate { half: Point::new(w, h) }; }\n"),
        &with_prelude("fn f() { p.translate(Vec2::new(1.0, 2.0)); p.rigid_transform(Vec2::new(x, y), a); r.translated(d); Plate { half: Vec2::new(w, h) }; }\n"),
    );
}

#[test]
fn corner_needs_a_mui_rect_in_the_enclosing_fn() {
    // egui's Rect, in a file that never imports mui: untouched, no note.
    let egui = "use egui::Rect;\nfn a(r: Rect) -> f32 { r.min.x }\n";
    let out = run(egui);
    assert_eq!((out.text.as_str(), out.warnings.len()), (egui, 0));
    // A mixed file: the egui one stays, the mui one moves.
    let mixed = "use egui::Rect;\nuse mui2::prelude::El;\nfn a(r: Rect) -> f32 { r.min.x }\nfn b(r: mui2::geometry::Bounds) -> f64 { r.min.x }\n";
    let out = run(mixed);
    assert_eq!(out.text, "use egui::Rect;\nuse mui2::prelude::El;\nfn a(r: Rect) -> f32 { r.min.x }\nfn b(r: mui2::geometry::Rect) -> f64 { r.x0 }\n");
    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
    // `use mui::Bounds` next to `use egui::Rect`: the rename collides, and says so.
    let out = run("use egui::Rect;\nuse mui2::geometry::Bounds;\nfn b(r: Bounds) {}\n");
    assert!(out.text.contains("use mui2::geometry::Rect;"), "{}", out.text);
    assert!(out.warnings.iter().any(|(_, n)| n.contains("`Bounds` becomes `Rect`")), "{:?}", out.warnings);
    // Another fn's `b: Bounds` says nothing about this `b`; a closure
    // parameter or a pattern shadows: notes, not rewrites.
    let src = with_prelude("fn a(b: Bounds) {}\nfn c(b: Thing) -> f64 { b.min.x }\nfn d(b: Bounds) -> f64 { let f = |b| b.min.x; if let Some(b) = o { b.max.y } else { 0.0 } }\nfn e(s: S) -> f64 { let clip = s.clip.unwrap(); clip.min.y }\n");
    let out = run(&src);
    assert_eq!(out.text, src.replace("Bounds", "Rect"));
    assert_eq!(out.warnings.iter().filter(|(_, n)| n.contains("x0")).count(), 2, "{:?}", out.warnings);
    // `let r = Bounds::new(..);` and `let r: &Bounds = ..` are known.
    check(
        &with_prelude("fn f() -> f64 { let r = Bounds::new(a, b); let s: &Bounds = &r; r.min.x + s.max.y }\n"),
        &with_prelude("fn f() -> f64 { let r = Rect::new(a, b); let s: &Rect = &r; r.x0 + s.y1 }\n"),
    );
}

#[test]
fn builder_chain_methods() {
    // `.join()`: a thread handle, `Iterator::join`-like calls and unknown receivers stay.
    let src = with_prelude("fn f(parts: Vec<String>) {\n    std::thread::spawn(g).join().unwrap();\n    handle.join();\n    parts.iter().join();\n    row![a, b].gap(4.0).join();\n    col([a]).join();\n    mui2::row([a]).fill(Role::Raised).join();\n}\n");
    assert_eq!(
        run(&src).text,
        with_prelude("fn f(parts: Vec<String>) {\n    std::thread::spawn(g).join().unwrap();\n    handle.join();\n    parts.iter().join();\n    row![a, b].gap(4.0).segmented();\n    col([a]).segmented();\n    mui2::row([a]).fill(Role::Raised).segmented();\n}\n")
    );
    // `.disabled(..)` on a palette or a non-builder call stays; on an `El` it moves.
    let src = with_prelude("fn f(ui: &mut Ui, e: El, t: &Theme) {\n    ui.palette().disabled(c);\n    t.colors().disabled(true);\n    e.disabled(true);\n    e.fill(x).disabled(on);\n}\n");
    assert_eq!(
        run(&src).text,
        with_prelude("fn f(ui: &mut Ui, e: El, t: &Theme) {\n    ui.palette().disabled(c);\n    t.colors().disabled(true);\n    e.disabled();\n    e.fill(x).when(on, Styled::disabled);\n}\n")
    );
    // egui's `ui.label("x")` in a mixed file stays; an unresolved receiver gets a note.
    let src = "use mui2::prelude::{El, row};\nfn f(ui: &mut egui::Ui, e: El, x: X) { ui.label(\"x\"); e.label(\"y\"); row([]).label(\"z\"); x.label(\"w\"); }\n";
    let out = run(src);
    assert_eq!(out.text, "use mui2::prelude::{El, row};\nfn f(ui: &mut egui::Ui, e: El, x: X) { ui.label(\"x\"); e.named(\"y\"); row([]).named(\"z\"); x.label(\"w\"); }\n");
    assert_eq!(out.warnings.iter().filter(|(_, n)| n.contains(".named(name)")).count(), 0, "{:?}", out.warnings);
    let out = run("use mui2::prelude::*;\nfn f() { let x = g(); x.label(\"w\"); }\n");
    assert_eq!(out.warnings.iter().filter(|(_, n)| n.contains(".named(name)")).count(), 1, "{:?}", out.warnings);
}

#[test]
fn any_gated_rules_need_mui() {
    let src = "fn f() { e.scroll_bar(false).weld_with(w).gpu_weld(w).weld_shape().layout_transition(t); }\n";
    check(src, src);
}

#[test]
fn a_foreign_glob_makes_bare_names_ambiguous() {
    let src = "use mui2::prelude::*;\nuse crate::helpers::*;\nfn f() -> Kind { label(ui, \"Gain\"); label(\"Gain\"); Kind::A }\n";
    let out = run(src);
    assert_eq!(out.text, "use mui2::prelude::*;\nuse crate::helpers::*;\nfn f() -> Kind { label(ui, \"Gain\"); body(\"Gain\"); Kind::A }\n");
    assert_eq!(out.warnings.iter().filter(|(_, n)| n.contains("another glob")).count(), 2, "{:?}", out.warnings);
    // `std` globs and a test module's `use super::*` are not foreign.
    check(
        "use mui2::prelude::*;\nuse std::f64::consts::*;\nfn f() { label(ui, \"G\"); }\nmod tests {\n    use super::*;\n    fn g() -> Kind { Kind::A }\n}\n",
        "use mui2::prelude::*;\nuse std::f64::consts::*;\nfn f() { body(ui, \"G\"); }\nmod tests {\n    use super::*;\n    fn g() -> A11y { A11y::A }\n}\n",
    );
}

#[test]
fn retype_needs_a_mui_target() {
    // Someone else's `half` / `wheel`, and a local variable: untouched.
    let src = with_prelude("struct MyKnob { half: Point }\nfn f(ev: &Event) { let k = MyKnob { half: Point::new(1.0, 2.0) }; let mut half = Point::ZERO; half = Point::new(0.0, 1.0); if ev.wheel == Point::ZERO {} }\n");
    check(&src, &src);
    // mui's, through a struct literal, a typed variable, and `if` branches.
    check(
        &with_prelude("fn f(p: &mut Input) { let pl = Plate { center, half: Point::new(w, h) }; let mut input = Input::default(); input.wheel = if n == 1 { Point::new(0.0, 30.0) } else if n == 2 { super::Point::default() } else { Point::ZERO }; p.wheel = Point::ZERO; }\n"),
        &with_prelude("fn f(p: &mut Input) { let pl = Plate { center, half: Vec2::new(w, h) }; let mut input = Input::default(); input.wheel = if n == 1 { Vec2::new(0.0, 30.0) } else if n == 2 { super::Vec2::default() } else { Vec2::ZERO }; p.wheel = Vec2::ZERO; }\n"),
    );
    // An unknown receiver gets a note; an `if` without `else` is not touched.
    let out = run(&with_prelude("fn f() { self.input.wheel = Point::ZERO; }\n"));
    assert!(out.warnings.iter().any(|(_, n)| n.contains("`Input::wheel` is a `Vec2`")), "{:?}", out.warnings);
}

#[test]
fn state_tuple_only_on_the_ui() {
    let src = with_prelude("fn f(fsm: &Fsm, ui: &Ui, u: &mut Ui) { let a = fsm.state(k).0; let (x, y) = fsm.state(k); let b = ui.state(&id).0; let c = u.state(&id).1; }\n");
    assert_eq!(
        run(&src).text,
        with_prelude("fn f(fsm: &Fsm, ui: &Ui, u: &mut Ui) { let a = fsm.state(k).0; let (x, y) = fsm.state(k); let b = ui.state(&id).hover; let c = u.state(&id).press; }\n")
    );
}

#[test]
fn point_methods_need_a_point() {
    let src = with_prelude("fn f(p: Point, v: kurbo::Vec2, s: Sample) -> bool { let q = p.perpendicular(); v.finite() && s.finite() && (p - q).finite() }\n");
    let out = run(&src);
    assert_eq!(out.text, with_prelude("fn f(p: Point, v: kurbo::Vec2, s: Sample) -> bool { let q = p.turn_90(); v.is_finite() && s.finite() && (p - q).finite() }\n"));
    assert_eq!(out.warnings.iter().filter(|(_, n)| n.contains("is_finite")).count(), 1, "{:?}", out.warnings);
}

#[test]
fn glide_keys_become_ids() {
    let out = run(&with_prelude("fn f(r: &mut Resolver) { r.resolve_animated(&spec, &mut |key: &str, e, f| f, None); r.resolve_animated(&spec, &mut glide, None); }\n"));
    assert_eq!(out.text, with_prelude("fn f(r: &mut Resolver) { r.resolve_animated(&spec, &mut |key: &Id, e, f| f, None); r.resolve_animated(&spec, &mut glide, None); }\n"));
    assert_eq!(out.warnings.iter().filter(|(_, n)| n.contains("glide passed by name")).count(), 1, "{:?}", out.warnings);
}

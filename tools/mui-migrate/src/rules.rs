//! The DSL v2 migration rules, as data.
//!
//! Adding a rule is one line in `RULES` (order matters only among `Call`
//! rules for the same method: the first match wins):
//!
//! - `Method { old, new, gate }`     `.old(` / `.old::<` -> `.new(`
//! - `Function { old, new }`         `old(` (free fn, `mui::old(`, `use mui::{old}`) -> `new`
//! - `FnCall { name, args, to, needs }` call-shape rewrite of a free fn: `name(args)` (or a
//!   mui path to it) becomes template `to`, with `$n` as for `Call` and `$path` the
//!   path before the name (`mui::widgets::`); the first matching shape wins
//! - `Type { old, new }`             path/type ident, only where it resolves to a mui crate
//! - `Macro { old, new }`            `old!` -> `new!`
//! - `MacroHead { old, new, heads }` `old![a, b; ..]` -> `new![<heads[n-1] with $1 = a, $2 = b>; ..]`,
//!   the template picked by the head's argument count n
//! - `Call { chain, to, gate, needs }` call-shape rewrite: match `.m(args)` (or a
//!   chain of them) with arg patterns, replace with template `to` (`$1`.. are
//!   the `Any`/`Has` args in order, `$!1` is the logical negation of `$1`). `needs`
//!   names idents the output uses so a missing import is warned about.
//!   `Arg::After(p)` matches an argument starting with `p` and captures the rest.
//! - `DropImport { name }`           remove `name` from `use <mui>::..` lists
//! - `Qualify { names, prefix }`     bare glob-imported `Name` -> `prefix` + `Name`
//! - `Manual { pattern, note }`      report `file:line: note` wherever the token
//!   sequence `pattern` appears in a file that uses mui (no rewrite)
//!
//! `Gate::Any` applies everywhere; `Gate::Mui` only in files that import a
//! mui crate; `Gate::MuiChain` additionally needs the receiver to end in
//! `)` / `]` (a builder chain, not `handle.join()`).
//!
//! Tuple results that became named structs: add a line to `TUPLES`.
//! Moved crates: add `(old path suffix, new path suffix)` to `CARGO_MOVES`.

#[derive(Clone, Copy, Debug)]
pub enum Gate {
    Any,
    Mui,
    MuiChain,
}

#[derive(Clone, Copy, Debug)]
pub enum Arg {
    /// Any argument, captured as `$n`.
    Any,
    /// Exactly this text (whitespace-insensitive).
    Is(&'static str),
    /// Argument text contains this (whitespace-insensitive).
    Has(&'static str),
    /// Argument starts with this (whitespace-insensitive); the rest is captured.
    After(&'static str),
    /// Any argument that does not contain this (whitespace-insensitive), captured.
    Not(&'static str),
    /// A shared borrow `&x` (not `&mut x`); `x` is captured.
    Shared,
}

#[derive(Clone, Copy, Debug)]
pub enum Rule {
    Method { old: &'static str, new: &'static str, gate: Gate },
    Function { old: &'static str, new: &'static str },
    FnCall { name: &'static str, args: &'static [Arg], to: &'static str, needs: &'static [&'static str] },
    Type { old: &'static str, new: &'static str },
    #[expect(dead_code, reason = "no plain macro renames in the spec yet")]
    Macro { old: &'static str, new: &'static str },
    MacroHead { old: &'static str, new: &'static str, heads: &'static [&'static str] },
    Call { chain: &'static [(&'static str, &'static [Arg])], to: &'static str, gate: Gate, needs: &'static [&'static str] },
    DropImport { name: &'static str },
    Qualify { names: &'static [&'static str], prefix: &'static str },
    Manual { pattern: &'static str, note: &'static str },
}

use Arg::{After, Any as A, Has, Is, Not, Shared};
use Gate::{Any as G, Mui, MuiChain};
use Rule::*;

const SS: &str = "Align::Start";
const CC: &str = "Align::Center";

pub const RULES: &[Rule] = &[
    // Constructors.
    Function { old: "leaf", new: "block" },
    Function { old: "column", new: "col" },
    Function { old: "overlay", new: "stack" },
    Function { old: "canvas_cached", new: "canvas_keyed" },
    // Text roles.
    Function { old: "label", new: "body" },
    // Layout sugar: absolute placement.
    Call { chain: &[("anchor", &[Is(SS), Is(SS)]), ("offset", &[A, A])], to: ".at($1, $2)", gate: Mui, needs: &[] },
    Call { chain: &[("offset", &[A, A]), ("anchor", &[Is(SS), Is(SS)])], to: ".at($1, $2)", gate: Mui, needs: &[] },
    Call { chain: &[("anchor", &[Is(CC), Is(CC)]), ("offset", &[A, A])], to: ".centered_at($1, $2)", gate: Mui, needs: &[] },
    Call { chain: &[("offset", &[A, A]), ("anchor", &[Is(CC), Is(CC)])], to: ".centered_at($1, $2)", gate: Mui, needs: &[] },
    DropImport { name: "Sugar" },
    DropImport { name: "IntoLen" },
    Manual { pattern: "Sugar", note: "`Sugar` is gone: layout verbs are inherent on `Node`" },
    Manual { pattern: "IntoLen", note: "`IntoLen` is gone: `Len` has `From<i32/f32/f64>`" },
    // Accessibility.
    Type { old: "Kind", new: "A11y" },
    Call { chain: &[("role", &[Has("Kind::")])], to: ".a11y($1)", gate: Mui, needs: &[] },
    Call { chain: &[("role", &[Has("A11y::")])], to: ".a11y($1)", gate: Mui, needs: &[] },
    Call { chain: &[("label", &[A])], to: ".named($1)", gate: Mui, needs: &[] },
    Qualify {
        names: &["Background", "Surface", "Raised", "Field", "Level", "Primary", "Secondary", "Tertiary", "Success", "Warning", "Danger", "Ink", "Dim"],
        prefix: "Role::",
    },
    // Switches.
    // On builder chains only: `palette.disabled(color)` is a colour, not a switch.
    Call { chain: &[("disabled", &[Is("true")])], to: ".disabled()", gate: MuiChain, needs: &[] },
    Call { chain: &[("disabled", &[Is("false")])], to: "", gate: MuiChain, needs: &[] },
    Call { chain: &[("disabled", &[A])], to: ".when($1, Styled::disabled)", gate: MuiChain, needs: &["Styled"] },
    // The overlay scrollbar is on by default, so the switch is `no_scrollbar`.
    Call { chain: &[("scroll_bar", &[Is("true")])], to: "", gate: G, needs: &[] },
    Call { chain: &[("scroll_bar", &[Is("false")])], to: ".no_scrollbar()", gate: G, needs: &[] },
    Call { chain: &[("scroll_bar", &[A])], to: ".when($!1, Styled::no_scrollbar)", gate: G, needs: &["Styled"] },
    // Motion.
    Call { chain: &[("transition", &[A])], to: ".animate_with($1)", gate: Mui, needs: &[] },
    Method { old: "layout_transition", new: "animate_layout_with", gate: G },
    // Weld and material.
    Method { old: "weld_with", new: "weld", gate: G },
    // `.weld_morph(p)` / `.weld_quality(q)` kept the weld set before them:
    // once both sides are `.weld(..)`, the second folds into the first.
    Call { chain: &[("weld", &[A]), ("weld", &[After("Weld::default()")])], to: ".weld($1$2)", gate: G, needs: &[] },
    Call { chain: &[("weld_shape", &[])], to: ".weld(Weld::shape())", gate: G, needs: &["Weld"] },
    Call { chain: &[("weld_borders", &[])], to: ".weld(Weld::borders())", gate: G, needs: &["Weld"] },
    Call { chain: &[("weld_morph", &[A])], to: ".weld(Weld::default().morph($1))", gate: G, needs: &["Weld"] },
    Call { chain: &[("weld_quality", &[A])], to: ".weld(Weld::default().quality($1))", gate: G, needs: &["Weld"] },
    Call { chain: &[("without_weld", &[])], to: ".weld(Weld::off())", gate: G, needs: &["Weld"] },
    Method { old: "exclude_from_weld", new: "unwelded", gate: G },
    Manual { pattern: ". gpu_weld", note: "`.gpu_weld(w)` became `.weld(w)`: pick the backend with `SceneSpec::weld_backend(WeldBackend::..)`" },
    Manual { pattern: ". reference_weld", note: "`.reference_weld(w)` became `.weld(w)`: pick the backend with `SceneSpec::weld_backend(WeldBackend::..)`" },
    Method { old: "gpu_weld", new: "weld", gate: G },
    Method { old: "reference_weld", new: "weld", gate: G },
    MacroHead { old: "weld_morph", new: "weld", heads: &["Weld::default().morph($1)", "$1.morph($2)"] },
    Call { chain: &[("join", &[])], to: ".segmented()", gate: MuiChain, needs: &[] },
    // Icons.
    Manual { pattern: "material_symbols :: codepoint", note: "use the `mui_symbols::sym::NAME` char consts instead of `codepoint(\"name\")`" },
    // Resolving.
    Function { old: "resolve_scene", new: "resolve" },
    Type { old: "TextCache", new: "Resolver" },
    FnCall { name: "resolve_scene_with", args: &[A, After("&mut")], to: "$2.resolve($1)", needs: &[] },
    DropImport { name: "resolve_scene_with" },
    DropImport { name: "resolve_scene_cached" },
    DropImport { name: "resolve_scene_animated" },
    DropImport { name: "resolve_scene_retained" },
    Manual { pattern: "resolve_scene_cached", note: "use `Resolver`: `let mut r = Resolver::new(); r.resolve(&spec)`, `r.welds` is the weld cache" },
    Manual { pattern: "resolve_scene_animated", note: "use `Resolver`: `r.resolve_animated(&spec, glide, None)`" },
    Manual { pattern: "resolve_scene_retained", note: "use `Resolver`: `r.resolve_animated(&spec, glide, prev)`" },
    // mui-truce: `Bridge::bind` derives the widget id from the parameter.
    // ponytail: `args` splits the old `|ui, v|` closure at its comma ($4, $5), which is
    // what lets the rule insert `id`; if `args` learns closures, this becomes 4 args.
    Call { chain: &[("bind", &[A, Has("\""), A, A, A])], to: ".bind($1, $3, $4, id, $5)", gate: Mui, needs: &[] },
    Manual { pattern: ". bind (", note: "`Bridge::bind`'s closure is `|ui, id, v|`: pass `id` to the widget; a toggle can use `bind_bool(ui, P, |ui, id, on| ..)`" },
];

/// The widget phase of the spec (`Response`, option structs, argument
/// order). Applied, with [`TUPLES`], unless `--no-widgets`.
pub const WIDGET_RULES: &[Rule] = &[
    // Every control takes a label after its id (its accessible name).
    FnCall { name: "toggle", args: &[A, A, A], to: "$pathtoggle($1, $2, \"\", $3)", needs: &[] },
    FnCall { name: "drag_value", args: &[A, A, A, A], to: "$pathdrag_value($1, $2, \"\", $3, $4)", needs: &[] },
    // Positional options became option structs.
    FnCall { name: "color_picker", args: &[A, A, A, Is("true")], to: "$pathcolor_picker($1, $2, $3, ColorOpts { alpha: true })", needs: &["ColorOpts"] },
    FnCall { name: "color_picker", args: &[A, A, A, Is("false")], to: "$pathcolor_picker($1, $2, $3, ColorOpts::default())", needs: &["ColorOpts"] },
    FnCall { name: "color_picker", args: &[A, A, A, Not("ColorOpts")], to: "$pathcolor_picker($1, $2, $3, ColorOpts { alpha: $4 })", needs: &["ColorOpts"] },
    // `bins` edits in place, as `curve` does.
    FnCall { name: "bins", args: &[A, A, Shared], to: "$pathbins($1, $2, &mut $3)", needs: &[] },
];

/// Calls whose tuple result became a named struct. `f(..).0` -> `.el`,
/// `let (a, b) = f(..)` -> `let Response { el: a, changed: b } = f(..)`.
/// `method: true` matches `.name(..)` (in mui files) instead of a free fn.
pub struct Tuple {
    pub name: &'static str,
    pub method: bool,
    pub ty: &'static str,
    pub fields: &'static [&'static str],
}

const fn widget(name: &'static str) -> Tuple {
    Tuple { name, method: false, ty: "Response", fields: &["el", "changed"] }
}

pub const TUPLES: &[Tuple] = &[
    widget("knob"),
    widget("slider"),
    widget("toggle"),
    widget("button"),
    widget("text_input"),
    widget("text_edit"),
    widget("color_picker"),
    widget("bins"),
    widget("curve"),
    widget("drag_value"),
    Tuple { name: "state", method: true, ty: "Interaction", fields: &["hover", "press"] },
];

/// Crate package names that make an import a "mui" import. A dependency
/// renamed with `package = "<one of these>"` becomes a root too.
pub const MUI_CRATES: &[&str] = &[
    "mui", "mui-scene", "mui-layout", "mui-style", "mui-text", "mui-vello", "mui-truce", "mui-geometry",
    "mui-input", "mui-motion", "mui-access", "mui-weld", "mui-material", "mui-symbols",
];

/// Extra path roots always treated as mui (KURV imports `mui` as `mui2`).
pub const EXTRA_ROOTS: &[&str] = &["mui2"];

/// Path dependencies that moved: a `path = ".../<old>"` becomes `.../<new>`.
pub const CARGO_MOVES: &[(&str, &str)] = &[
    ("crates/mui-stage", "media/mui-stage"),
    ("crates/mui-reel", "media/mui-reel"),
    ("crates/mui-motion-bridge", "media/mui-motion-bridge"),
];

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
//! - `Assoc { ty, name, args, to, bare }` call-shape rewrite of an associated fn
//!   `Ty::name(args)` (optionally path-qualified) into template `to`; `$path` is
//!   the path before `Ty`, or `bare` when `Ty` was named bare
//! - `Moved { names, from, to }`     `from::Name` -> `to::Name` for each moved `Name`,
//!   in paths and `use` trees (a mixed brace list is split into two `use`s)
//! - `Field { recv, name, read, write }` a public field that became accessors:
//!   `<recv>.name = e` -> `<recv>` + `write` (`$1` = e), any other `<recv>.name`
//!   -> `<recv>` + `read` (`""`: reads are left for rustc to report); the
//!   receiver's last segment must be one of `recv`
//! - `Corner { field, axis, to }`    `.field.axis` -> `.to` where the receiver is
//!   a mui `Rect`/`Bounds` by its type in the enclosing fn (see `rect_receiver`);
//!   an unresolved receiver gets a manual note
//! - `Retype { field, from, to, on }` `field: From::..`, `field = From::..` and
//!   `field ==/!= From::..` -> `To::..`: a field whose type changed, where the
//!   struct literal or the receiver is a mui type named in `on`
//! - `Flag { name, flag }`           `.payload_mut().name = x` -> `.payload_mut().set(Element::flag, x)`
//! - `Manual { pattern, note }`      report `file:line: note` wherever the token
//!   sequence `pattern` appears in a file that uses mui (no rewrite)
//!
//! `Gate::Mui` applies only in files that import a mui crate;
//! `Gate::MuiChain` additionally needs the receiver to be a builder chain (see
//! `BUILDERS`: not `handle.join()` or `ui.palette().disabled(c)`);
//! `Gate::Typed { on, note }` needs the receiver to be a variable of a mui (or
//! kurbo) type in `on`, by its binding in the enclosing fn; with `note`, an
//! unresolved receiver gets a manual note.
//!
//! Tuple results that became named structs: add a line to `TUPLES`.
//! Moved crates: add `(old path suffix, new path suffix)` to `CARGO_MOVES`.

#[derive(Clone, Copy, Debug)]
pub enum Gate {
    Mui,
    MuiChain,
    Typed { on: &'static [&'static str], note: bool },
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
    Assoc { ty: &'static str, name: &'static str, args: &'static [Arg], to: &'static str, bare: &'static str },
    Moved { names: &'static [&'static str], from: &'static str, to: &'static str },
    Field { recv: &'static [&'static str], name: &'static str, read: &'static str, write: &'static str },
    Corner { field: &'static str, axis: &'static str, to: &'static str },
    Retype { field: &'static str, from: &'static str, to: &'static str, on: &'static [&'static str] },
    Flag { name: &'static str, flag: &'static str },
    Manual { pattern: &'static str, note: &'static str },
}

use Arg::{After, Any as A, Has, Is, Not, Shared};
use Gate::{Mui, MuiChain, Typed};

const PT: Gate = Typed { on: &["Point", "Vec2"], note: true };
const RESOLVER: Gate = Typed { on: &["Resolver", "TextCache"], note: false };
const RESOLVE_REF: &str = "`Resolver::resolve(&spec)` returns `&ResolvedScene` now (kept for the next call's memo reuse): `.clone()` it where the scene must outlive the next resolve, and drop `recycle` calls for scenes it returned";
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
    Call { chain: &[("label", &[A])], to: ".named($1)", gate: MuiChain, needs: &[] },
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
    Call { chain: &[("scroll_bar", &[Is("true")])], to: "", gate: Mui, needs: &[] },
    Call { chain: &[("scroll_bar", &[Is("false")])], to: ".no_scrollbar()", gate: Mui, needs: &[] },
    Call { chain: &[("scroll_bar", &[A])], to: ".when($!1, Styled::no_scrollbar)", gate: Mui, needs: &["Styled"] },
    // Motion.
    Call { chain: &[("transition", &[A])], to: ".animate_with($1)", gate: Mui, needs: &[] },
    Method { old: "layout_transition", new: "animate_layout_with", gate: Mui },
    // Weld and material.
    Method { old: "weld_with", new: "weld", gate: Mui },
    // `.weld_morph(p)` / `.weld_quality(q)` kept the weld set before them:
    // once both sides are `.weld(..)`, the second folds into the first.
    Call { chain: &[("weld", &[A]), ("weld", &[After("Weld::default()")])], to: ".weld($1$2)", gate: Mui, needs: &[] },
    Call { chain: &[("weld_shape", &[])], to: ".weld(Weld::shape())", gate: Mui, needs: &["Weld"] },
    Call { chain: &[("weld_borders", &[])], to: ".weld(Weld::borders())", gate: Mui, needs: &["Weld"] },
    Call { chain: &[("weld_morph", &[A])], to: ".weld(Weld::default().morph($1))", gate: Mui, needs: &["Weld"] },
    Call { chain: &[("weld_quality", &[A])], to: ".weld(Weld::default().quality($1))", gate: Mui, needs: &["Weld"] },
    Call { chain: &[("without_weld", &[])], to: ".weld(Weld::off())", gate: Mui, needs: &["Weld"] },
    Method { old: "exclude_from_weld", new: "unwelded", gate: Mui },
    Manual { pattern: ". gpu_weld", note: "`.gpu_weld(w)` became `.weld(w)`: pick the backend with `SceneSpec::weld_backend(WeldBackend::..)`" },
    Manual { pattern: ". reference_weld", note: "`.reference_weld(w)` became `.weld(w)`: pick the backend with `SceneSpec::weld_backend(WeldBackend::..)`" },
    Method { old: "gpu_weld", new: "weld", gate: Mui },
    Method { old: "reference_weld", new: "weld", gate: Mui },
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
    Manual { pattern: "resolve_scene_animated", note: "use `Resolver`: `r.resolve_after(&spec, glide, None)`" },
    Manual { pattern: "resolve_scene_retained", note: "use `Resolver`: `r.resolve_after(&spec, glide, prev)`" },
    Manual { pattern: "resolve_scene_with (", note: RESOLVE_REF },
    Method { old: "resolve_animated", new: "resolve_after", gate: Mui },
    Method { old: "len", new: "text_runs", gate: RESOLVER },
    Manual { pattern: ". scroll_bars", note: "`SceneSpec::scroll_bars` is an `FxHashMap<Id, f64>` (was `BTreeMap<String, f64>`): key it with `Id::runtime(key)`" },
    Manual { pattern: "Stroke {", note: "`Stroke::fill` is an `Option<Fill>` (`None` = unset, merged per field by `Style::over`): wrap literals in `Some(..)`" },
    Manual { pattern: ". stroke . fill", note: "`Stroke::fill` is an `Option<Fill>`: read it with `.and_then(|s| s.fill)`" },
    // `.resolve_after(..)`'s glide closure: `|key: &str, ..|` is rewritten to
    // `&Id` in code; a glide passed by name gets a note (see `rewrite::glide`).
    // mui-truce: `Bridge::bind` derives the widget id from the parameter.
    // ponytail: `args` splits the old `|ui, v|` closure at its comma ($4, $5), which is
    // what lets the rule insert `id`; if `args` learns closures, this becomes 4 args.
    Call { chain: &[("bind", &[A, Has("\""), A, A, A])], to: ".bind($1, $3, $4, id, $5)", gate: Mui, needs: &[] },
    Manual { pattern: ". bind (", note: "`Bridge::bind`'s closure is `|ui, id, v|`: pass `id` to the widget; a toggle can use `bind_bool(ui, P, |ui, id, on| ..)`" },
    // Geometry: kurbo is the one Point/Vec2/Rect/Affine (mui-geometry, mui-weld re-export it).
    // `Rect::from_points` is kurbo's two-corner constructor, so the ring form is flagged too.
    Assoc { ty: "Bounds", name: "from_points", args: &[A], to: "$pathbounds($1)", bare: "mui_geometry::" },
    Type { old: "Bounds", new: "Rect" },
    Corner { field: "min", axis: "x", to: "x0" },
    Corner { field: "min", axis: "y", to: "y0" },
    Corner { field: "max", axis: "x", to: "x1" },
    Corner { field: "max", axis: "y", to: "y1" },
    Manual { pattern: ". translated (", note: "`Bounds::translated(d)` is `rect + d`; `RoundedRect::translated` takes a `Vec2`" },
    Method { old: "finite", new: "is_finite", gate: PT },
    Method { old: "perpendicular", new: "turn_90", gate: PT },
    Manual { pattern: ". rotated (", note: "`Point::rotated(a)` is gone: `Vec2::from_angle(a) * r`, or `Affine::rotate(a) * p`" },
    Assoc { ty: "Affine", name: "translation", args: &[A, A], to: "$pathAffine::translate(($1, $2))", bare: "" },
    Assoc { ty: "Affine", name: "scale", args: &[A, A], to: "$pathAffine::scale_non_uniform($1, $2)", bare: "" },
    Assoc { ty: "Affine", name: "rotation", args: &[A], to: "$pathAffine::rotate($1)", bare: "" },
    Assoc { ty: "Affine", name: "rotation_about", args: &[A, A], to: "$pathAffine::rotate_about($1, $2)", bare: "" },
    // `.then` and `.apply` are too common a name to rewrite without the receiver's type.
    Manual { pattern: "Affine :: translation", note: "kurbo `Affine`: `a.then(b)` -> `b * a`, `t.apply(p)` -> `t * p`" },
    Manual { pattern: "Affine :: rotation", note: "kurbo `Affine`: `a.then(b)` -> `b * a`, `t.apply(p)` -> `t * p`" },
    // A displacement argument is a `Vec2`: a literal `Point::new(..)` there is rewritten.
    Call { chain: &[("translate", &[After("Point::new")])], to: ".translate(Vec2::new$1)", gate: Mui, needs: &["Vec2"] },
    Call { chain: &[("translated", &[After("Point::new")])], to: ".translated(Vec2::new$1)", gate: Mui, needs: &["Vec2"] },
    Call { chain: &[("rigid_transform", &[After("Point::new"), A])], to: ".rigid_transform(Vec2::new$1, $2)", gate: Mui, needs: &["Vec2"] },
    Retype { field: "half", from: "Point", to: "Vec2", on: &["Plate"] },
    Manual { pattern: ". rigid_transform (", note: "`Path::rigid_transform` and `Path::translate` take a `Vec2` (`p.to_vec2()`)" },
    Manual { pattern: "drag_delta : Point", note: "`Response::drag_delta` and `drag_total` are `Vec2` (a kurbo `Point` has no `+ Point`)" },
    Manual { pattern: "drag_total : Point", note: "`Response::drag_total` is a `Vec2`" },
    Manual { pattern: "Edge :: Arc {", note: "build a weld arc with `Edge::arc(center, radius, start, sweep)`; the variant also carries unit `from`/`to` vectors, so match it with `..`" },
    // Spacing tokens moved crates.
    Moved { names: &["Spacing", "SpacingScale", "SpacingToken"], from: "mui_geometry", to: "mui_layout" },
    // Element's switches are one `flags` field; a surface's are unchanged, so only
    // reads through `payload()` / `payload_mut()` are flagged.
    Manual { pattern: "payload ( ) . focusable", note: "`Element::focusable` is a flag: `e.has(Element::FOCUSABLE)`" },
    Flag { name: "focusable", flag: "FOCUSABLE" },
    Manual { pattern: "payload ( ) . captures_wheel", note: "`Element::captures_wheel` is a flag: `e.has(Element::CAPTURES_WHEEL)`" },
    Flag { name: "captures_wheel", flag: "CAPTURES_WHEEL" },
    Manual { pattern: "payload ( ) . tracks_pointer", note: "`Element::tracks_pointer` is a flag: `e.has(Element::TRACKS_POINTER)`" },
    Flag { name: "tracks_pointer", flag: "TRACKS_POINTER" },
    Manual { pattern: "payload ( ) . disabled", note: "`Element::disabled` is a flag: `e.has(Element::DISABLED)`" },
    Flag { name: "disabled", flag: "DISABLED" },
    Manual { pattern: "payload ( ) . baseline", note: "`Element::baseline` is a flag: `e.has(Element::BASELINE)`" },
    Flag { name: "baseline", flag: "BASELINE" },
    Manual { pattern: "payload ( ) . segmented", note: "`Element::segmented` is a flag: `e.has(Element::SEGMENTED)`" },
    Flag { name: "segmented", flag: "SEGMENTED" },
    Manual { pattern: "payload ( ) . weld_excluded", note: "`Element::weld_excluded` is a flag: `e.has(Element::WELD_EXCLUDED)`" },
    Flag { name: "weld_excluded", flag: "WELD_EXCLUDED" },
    Manual { pattern: "payload ( ) . scroll_bar_off", note: "`Element::scroll_bar_off` is a flag: `e.has(Element::SCROLL_BAR_OFF)`" },
    Flag { name: "scroll_bar_off", flag: "SCROLL_BAR_OFF" },
    Manual { pattern: ". scroll_bar_heat", note: "scrollbar heat is runtime state: `SceneSpec::scroll_bars` holds it by node key" },
    // Capture moved to mui-material (reported, not rewritten, as above). The
    // `Material` / `Capture` methods need no rule: rustc names the trait to import.
    Manual { pattern: "mui_scene :: resize_capture", note: "`resize_capture` lives in `mui_material` (`mui::material`)" },
    Manual { pattern: "mui_scene :: CaptureError", note: "`CaptureError` lives in `mui_material` (`mui::material`)" },
    Manual { pattern: "mui_scene :: CaptureLayer", note: "`CaptureLayer` lives in `mui_material` (`mui::material`)" },
    Manual { pattern: "scene :: resize_capture", note: "`mui::scene::resize_capture` is `mui::material::resize_capture`" },
    Manual { pattern: "scene :: CaptureError", note: "`mui::scene::CaptureError` is `mui::material::CaptureError`" },
    Manual { pattern: "scene :: CaptureLayer", note: "`mui::scene::CaptureLayer` is `mui::material::CaptureLayer`" },
    // `Ui`'s settings are accessors; the builders (`.font(f)`, `.gpu_welding()`) stay.
    Field { recv: &["ui"], name: "theme", read: ".theme()", write: ".set_theme($1)" },
    Field { recv: &["ui"], name: "scale", read: ".scale()", write: ".set_scale($1)" },
    Field { recv: &["ui"], name: "font", read: "", write: ".set_font($1)" },
    Field { recv: &["ui"], name: "double_click", read: "", write: ".set_double_click($1)" },
    Manual { pattern: "ui . fallback_fonts", note: "`Ui::fallback_fonts` is private: add faces with the `.fallback_font(f)` builder" },
    Manual { pattern: "ui . weld_backend", note: "`Ui::weld_backend` is private: build with `.gpu_welding()`" },
    // The wheel is a delta, not a place.
    Retype { field: "wheel", from: "Point", to: "Vec2", on: &["Input", "PointerInput", "Response"] },
    // v2.1, one way to say a thing: `w`/`h`, `min_w`/`min_h`, `grow(1)`, one `pad`,
    // `stroke` + `stroke_width`, `centered()`.
    Call { chain: &[("width", &[A])], to: ".w($1)", gate: MuiChain, needs: &[] },
    Call { chain: &[("height", &[A])], to: ".h($1)", gate: MuiChain, needs: &[] },
    Call { chain: &[("min_width", &[A])], to: ".min_w($1)", gate: MuiChain, needs: &[] },
    Call { chain: &[("min_height", &[A])], to: ".min_h($1)", gate: MuiChain, needs: &[] },
    Call { chain: &[("expand", &[])], to: ".grow(1)", gate: MuiChain, needs: &[] },
    Call { chain: &[("pad_xy", &[A, A])], to: ".pad(($1, $2))", gate: MuiChain, needs: &[] },
    Call { chain: &[("insets", &[A])], to: ".pad($1)", gate: MuiChain, needs: &[] },
    Call { chain: &[("border", &[A, A])], to: ".stroke($1).stroke_width($2)", gate: Mui, needs: &[] },
    Method { old: "no_border", new: "no_stroke", gate: Mui },
    Call { chain: &[("centered_at", &[Is("0."), Is("0.")])], to: ".centered()", gate: Mui, needs: &[] },
    Call { chain: &[("centered_at", &[Is("0.0"), Is("0.0")])], to: ".centered()", gate: Mui, needs: &[] },
    Call { chain: &[("centered_at", &[Is("0"), Is("0")])], to: ".centered()", gate: Mui, needs: &[] },
    Call { chain: &[("anchor", &[Is(CC), Is(CC)])], to: ".centered()", gate: Mui, needs: &[] },
    Call { chain: &[("apply", &[A])], to: ".when(true, $1)", gate: MuiChain, needs: &[] },
    // `Ui::default()` is the default theme.
    Assoc { ty: "Ui", name: "new", args: &[Is("Theme::DEFAULT")], to: "$pathUi::default()", bare: "" },
    Assoc { ty: "Ui", name: "new", args: &[Is("mui::prelude::Theme::DEFAULT")], to: "$pathUi::default()", bare: "" },
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

/// Methods that build an element (old and new names): a receiver chain made of
/// these, from a variable or a `CONSTRUCTORS` call, is a builder chain.
pub const BUILDERS: &[&str] = &[
    "a11y", "align", "align_self", "anchor", "animate", "animate_layout", "animate_layout_with", "animate_with", "appear", "apply", "area",
    "aspect", "at", "backdrop_blur", "baseline", "basis", "border", "border_align", "border_ramp", "captures_wheel", "center",
    "centered", "centered_at", "clip", "corners", "cursor", "delay", "disabled", "dividers", "el", "elevation", "end", "exclude_from_weld", "expand",
    "fill", "flex", "float", "focusable", "full", "gap", "gpu_weld", "grow", "h", "height", "hold", "icon_fill", "id", "insets",
    "inset_surface", "inset_surface_of", "into_el", "join", "join_border", "justify", "keep", "label", "line_gap", "lines", "match_height",
    "match_width", "max_size", "min_h", "min_height", "min_size", "min_w", "min_width", "named", "no_border", "no_fill", "no_stroke", "no_scrollbar", "offset", "opacity",
    "order", "pad", "pad_xy", "pill", "pin", "placed_at", "preset", "radius", "reference_weld", "reserve", "role", "scale", "scroll",
    "scrolled", "segmented", "shadow", "shadows", "sharp", "shrink", "size", "span", "square", "start", "sticky", "stroke", "stroke_width",
    "surface_layout", "tag", "text_axis", "text_size", "text_weight", "tip", "tracks_pointer", "transition", "unwelded", "value_text",
    "variant", "w", "weld", "weld_borders", "weld_morph", "weld_quality", "weld_shape", "weld_with", "when", "width", "without_weld", "wrap",
];

/// Free functions and macros (`row![..]`) that make an element.
pub const CONSTRUCTORS: &[&str] = &[
    "block", "body", "canvas", "canvas_cached", "canvas_keyed", "caption", "chip", "col", "column", "grid", "icon", "image", "label", "leaf",
    "overlay", "row", "spacer", "stack", "text", "tile", "title", "weld", "weld_morph",
];

/// Element types: a variable of one of these starts a builder chain.
pub const ELEMENT_TYPES: &[&str] = &["El", "Node", "Element"];

/// A bare name that a foreign glob could also supply is left alone, unless
/// the call has a shape only mui's has: `(name, argument count)`.
pub const MUI_SHAPES: &[(&str, usize)] = &[("label", 1)];

/// Notes for `.method(` calls on a variable of a mui type: `(method, types, note)`.
pub const TYPED_NOTES: &[(&str, &[&str], &str)] = &[
    ("is_empty", &["Resolver", "TextCache"], "`Resolver::is_empty()` is gone: `r.text_runs() == 0`"),
    ("resolve", &["Resolver", "TextCache"], RESOLVE_REF),
];

/// Notes for `MuiChain` calls whose receiver's type is not resolved.
pub const CHAIN_NOTES: &[(&str, &str)] = &[
    ("label", "`.label(name)` on an element is `.named(name)` (receiver type not resolved: check it is an `El`)"),
    ("width", "`.width(l)` on an element is `.w(l)` (receiver type not resolved: check it is an `El`)"),
    ("height", "`.height(l)` on an element is `.h(l)` (receiver type not resolved: check it is an `El`)"),
    ("min_width", "`.min_width(l)` on an element is `.min_w(l)` (receiver type not resolved: check it is an `El`)"),
    ("min_height", "`.min_height(l)` on an element is `.min_h(l)` (receiver type not resolved: check it is an `El`)"),
    ("insets", "`.insets(i)` on an element is `.pad(i)` (receiver type not resolved: check it is an `El`)"),
    ("disabled", "`.disabled(on)` on an element is `.disabled()` / `.when(on, Styled::disabled)` (receiver type not resolved: check it is an `El`)"),
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

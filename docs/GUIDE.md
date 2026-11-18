# Writing MUI UI

How to build an editor the way MUI is designed to be used. Every `rust`
block here is compiled and run by `cargo test -p mui --doc`. The README's
DSL table is the full vocabulary; this page is about which words to pick.

## The model

You build the whole tree every frame and hand it to `Ui::frame`. MUI measures
every node from its content (text runs, control sizes, padding, gaps), then
shares out the room the host offers. Widgets read last frame's gestures from
the `Ui`, so your state stays in your own variables.

Three consequences shape everything below:

- A node's size comes from its content and the theme. You rarely need a
  number.
- Position comes from the parent's axis, `gap`, `pad`, `align` and
  `justify`. Coordinates are for things that really live at a point.
- Colours, radii, spacing, text sizes and shadows come from the `Theme`.
  Change the theme and every editor follows.

## Placement, in order of preference

Use the first rung that expresses the design.

1. **Flow.** `row!`, `col!`, `grid!`, with `.gap(..)`, `.pad(..)`,
   `.grow(1)`, `.left()`/`.right()`/`.top()`/`.bottom()` (auto margins along a
   line, `align_self` across it), `bar(start, middle, end)`, `.center()`,
   `.between()`, `.around()`, `.evenly()`, `.align(..)`,
   `.justify(..)`. This covers almost every panel.
2. **Responsive flow.** `.wrap()`, `grid(..).min_col(px)`,
   `clamp(min, pct, max)`, `pct(..)`, `cq(..)` and `fits![wide, narrow]`.
   These replace breakpoints and window-size `if`s.
3. **Shape-aware flow.** `.inside(d)` and `.bend(k)` split a parent's
   contour; `.segmented()` butts controls into one strip; `.union(fill)`,
   `weld!` and `.surface_layout(p)` share outlines and materials. See
   [Shapes and welding](#shapes-and-welding).
4. **Overlays.** A `stack!` child with `.center()`, `.anchor(x, y)` or
   `.full()`. A float placed against another node with
   `.pin(Pin::to("id").area(Area::Bottom))`. A tooltip with `.tip("..")`.
   Menus, selects, popovers and dialogs come from `menu`, `select`,
   `popover` and `dialog`.
5. **Coordinates.** `.at(x, y)` and `.offset(dx, dy)`. Use them for content
   whose position is data: a node on a graph, a marker at a frequency, a
   drag ghost under the pointer. Inside a `canvas`, draw paths in the node's
   own space instead.

```rust
use mui::prelude::*;

let mut ui = Ui::default();
let (mut drive, mut mix, mut bypass) = (0.4, 1.0, false);

let header = row![
    title("Saturator"),
    spacer(),
    toggle(&mut ui, "bypass", "Bypass", &mut bypass).size(S),
]
.center();
let controls = row![
    knob(&mut ui, "drive", "Drive", &mut drive, 0.0..=1.0)
        .value_text(format!("{:.0} %", drive * 100.0)),
    knob(&mut ui, "mix", "Mix", &mut mix, 0.0..=1.0),
    meter(&mut ui, "out", [0.5_f32, 0.4]).ticks(),
]
.gap(L)
.center();
let root = col![header, controls]
    .gap(M)
    .pad(L)
    .fill(Role::Surface)
    .radius(Corner::Box);

let frame = ui
    .frame(root, Some(Size::new(360.0, 220.0)), Input::default(), 1.0 / 60.0)
    .unwrap();
assert!(frame.scene.surface("drive").is_some());
```

No widths, no coordinates, no colours: the theme picks the knob size, the
text sizes, the surface and the ink.

### Responsive without breakpoints

```rust
use mui::prelude::*;

let knobs = |n: usize| (0..n).map(|i| block(48.0, 48.0).pill().id(Id::of("k").slot(i)));
let editor = || {
    col![
        row![title("Delay"), spacer(), caption("v1")].baseline(),
        // Four columns at most; fewer when a column would drop under 96 px.
        grid(4, knobs(8)).gap(S).min_col(96.0),
        // The first candidate that fits the width is the one laid out.
        fits![
            row![body("Feedback path"), spacer(), body("Ping-pong")],
            col![body("Feedback path"), body("Ping-pong")],
        ],
    ]
    .gap(M)
    .pad(M)
    .w(pct(100.0))
    .h(pct(100.0))
};

let wraps = |w: f64| {
    let scene = resolve(&SceneSpec::new(editor()).offered(Size::new(w, 600.0))).unwrap();
    let y = |k: usize| scene.surface(Id::of("k").slot(k).as_str()).unwrap().frame.y;
    y(0) != y(3)
};
assert_eq!((wraps(240.0), wraps(1000.0)), (true, false));
```

## Tokens, not numbers

| need | write | not |
|---|---|---|
| spacing | `.gap(M)`, `.pad(L)`, `.pad((S, Xs))`, `.gap(step(1.5))` | `.gap(12.0)` |
| colour | `.fill(Role::Raised)`, `.stroke(Role::Ink.alpha(0.12))` | `Color::oklch(..)` constants |
| text | `title(..)`, `body(..)`, `caption(..)` | `text(..).text_size(9.0)` |
| corners | `.radius(Corner::Field)`, `.pill()` | `.radius(7.0)` |
| depth | `.elevation(Elevation::Raised)`, `.fill(Role::Level(1))` | hand-tuned `Shadow`s |
| control size | `.size(S)` on a control | `.w(..)`/`.h(..)` on a knob |
| widest readout | `.value_reserve("-88.8 dB")`, `.reserve(..)` | a fixed width |

Ink is not chosen. `Role::Ink` and `Role::Dim` resolve against the fill they
sit on and stay legible in both modes. A plugin's look is a `Theme`, built
once (`Palette::from_seed(accent, Mode::Dark)` or a `const Theme`) and
handed to `Ui::new(theme)`:

```rust
use mui::prelude::*;

let accent = Color::oklch(0.7, 0.15, 30.0);
let theme = Theme { palette: Palette::from_seed(accent, Mode::Dark), ..Theme::DEFAULT };
let mut ui = Ui::new(theme);
let mut v = 0.5;
let _ = col![knob(&mut ui, "gain", "Gain", &mut v, 0.0..=1.0)].pad(M);
```

A raw number is right when it is a fact about the product rather than the
design: a hardware-matched dial (`.px(72)`), a fixed meter length, an
aspect ratio, a data value.

## Controls

Every widget has one shape: `widget(&mut ui, id, label, &mut value, range)`
returns a `Response { el, changed }`. Finish a control with its look
(`.size`, `.variant`, `.role`, `.value_text`), then any element verb (`.tip`,
`.grow`, ..). The widgets are `button`, `toggle`, `slider`, `knob`,
`drag_value`, `meter`, `text_input`, `text_edit`, `color_picker`, `curve`,
`bins`, `select`, `menu`, `tabs`, `table` and the list views. `setting(..)`
and `group(..)` give labelled rows and titled groups with accessible names.
Their rustdoc in `mui::widgets` has a compiled example each.

In a plugin, bind a control to a parameter through `mui-truce`'s `Bridge`;
the drag becomes the host's begin/perform/end edit:

```rust,ignore
let gain = bridge.bind(ui, P::Gain, |ui, id, v| knob(ui, id, "Gain", v, 0.0..=1.0));
```

Give every interactive node a stable `.id(..)`. Compose ids for repeated
parts with `Id::of("osc").slot(i).field("gain")`, which costs no `format!`.

## Shapes and welding

MUI treats shared outlines as layout. Pick by what should be shared.

| you want | write |
|---|---|
| one outline, each child keeps its paint | `row![a, b].union(Role::Surface)` |
| one control strip with square inner seams | `row![a, b, c].segmented()` |
| a constant-thickness rim inside an outline | `.shell(d, Role::Raised)` |
| split a shaped parent into regions | `.inside(d)`, `.bend(k)` on the children's container |
| a hole or an intersection | `.cut(el)`, `.keep(el)` |
| one material blended across children | `weld![a, b]` or `row![a, b].weld(Weld::all())` |
| recessed panels and joined labels in one body | `.surface_layout(p)`, `.inset_surface()`, `.join_border(id)` |
| a border that changes colour and width along an outline | `.border_ramp(BorderRamp::horizontal(..))` |

Material welding blends fills and borders across the seam, so two plates
read as one object. Each child keeps its layout slot, id, text and role.
[MATERIAL-WELDING.md](MATERIAL-WELDING.md) has the policies, limits and GPU
backend; the short form:

```rust
use mui::prelude::*;

let plate = |id: &str, role: Role| block(80.0, 48.0).radius(Corner::Box).fill(role).id(id);
let mut ui = Ui::default().gpu_welding();
let joined = row![plate("osc", Role::Primary), plate("filter", Role::Secondary)]
    .gap(S)
    .weld(Weld::all().reach(24.0).morph(1.0))
    .id("chain");
ui.frame(col![joined].pad(L), None, Input::default(), 1.0 / 60.0).unwrap();
// Animate the join without a relayout:
ui.set_weld_morph("chain", 0.5).unwrap();
```

## Motion

Declare the end state and name what should move. MUI springs the rest and
retargets a spring in flight instead of restarting it.

| verb | what moves |
|---|---|
| `.animate()` | fill, stroke, radius, text size, shadow, shell depths |
| `.animate_layout()` | the solved frame: a panel that opens, a slot that moves |
| `.appear(Appear::Slide(0.0, 12.0))` | entry and exit (the exit needs an `.id`) |
| `.morph("pause")` | the outline, from the old named shape to the new one |
| `.identity(uid)` | carries springs, focus, scroll and drags when an id changes, so a reordered rack slot glides |
| `ui.tween(id, target)` | a number MUI cannot see, such as a canvas sweep |

```rust
use mui::prelude::*;

let playing = true;
let open = true;
let transport = block(24.0, 24.0)
    .fill(Role::Primary)
    .morph(if playing { "pause" } else { "play" })
    .animate()
    .id("transport");
let drawer = col![body("Advanced")]
    .pad(M)
    .fill(Role::Raised)
    .when(!open, |e| e.h(0).clip())
    .animate_layout()
    .id("drawer");
let mut ui = Ui::default();
let frame = ui.frame(col![transport, drawer], None, Input::default(), 1.0 / 60.0).unwrap();
let _schedule_next_frame = frame.animating;
```

`frame.animating` is the only signal a host needs. Prefer these verbs to
hand-written interpolation, timers or per-frame position maths.

For readouts that change every frame, `.measure_as("-88.8 dB")` lays the
text out as its widest value, so nothing re-lays out and neighbours stay
put.

## Check what you built

Read the pixels and the tree; don't guess. `mui-headless` renders an editor
with no window or GPU, on a fixed clock, so its output is byte-identical
from run to run.

```sh
cargo run -p mui-gain-plugin --example headless -- --snap out.png --scale 2   # look at it
cargo run -p mui-gain-plugin --example headless -- --tree tree.json           # ids, roles, rects
cargo run -p mui-preview -- --scene widgets --snap w.png                      # a gallery scene
```

Add your own editor with `mui_headless::Scene::new(name, size, ui, build)`
and `mui_headless::main(vec![scene])`; see `crates/mui-headless/src/lib.rs`.
Open the PNG and check alignment, clipping and contrast at two window sizes.
Check `--tree` for overlapping rects and for controls without ids.

## Patterns to replace

These are common in older plugin code. Convert them when you touch the view.

- **A `stack` of absolutely placed children** (`text("LOW").at(309.0, 439.0)`)
  that reproduces a mock-up. Rebuild it as rows and columns. A resize, a
  longer label or a new theme breaks hand-placed layouts.
- **Positions computed from a centre and a gap**
  (`x0 + gap * i`, `CX - w / 2.0`). Use `row(..).gap(..)`,
  `.justify(Justify::SpaceEvenly)` or `.center()`.
- **Module-level colour constants** (`const DIM: Color = ..`). Use roles, or
  put the colour in the plugin's `Theme`.
- **`text_size(n)` on every label.** Use `title`, `body` and `caption`, and
  change `Theme::type_scale` if the sizes are wrong.
- **Fixed `w`/`h` on text or controls.** Let them measure. Use
  `.value_reserve(..)` when a readout must not jitter.
- **A hand-drawn rounded rectangle path for a panel.** Use `.radius(..)`,
  `.union(..)`, `.shell(..)` or `.surface_layout(..)`.
- **Two shapes overlapped to fake a joint.** Use `.union(..)` or `weld!`.

//! A bounded expression interpreter for the browser playground, not a Rust compiler.
#![forbid(unsafe_code)]
use mui_scene::{Role::*, prelude::*};
use std::sync::LazyLock;
use syn::{Expr, Lit, Token, parse::Parser, punctuated::Punctuated, spanned::Spanned};
use wasm_bindgen::prelude::*;

/// Material Symbols Outlined, cut down to the icons `icon("name")` accepts;
/// see `fonts/README.md`. Variable in FILL, GRAD, opsz and wght. Parsed once,
/// so every render shares one face and its glyph caches.
static ICONS: LazyLock<Font> = LazyLock::new(|| {
    Font::new(&include_bytes!("../fonts/MaterialSymbolsOutlined-ui.ttf")[..])
        .expect("the bundled icon font parses")
});
static TEXT: LazyLock<Font> = LazyLock::new(|| {
    Font::new(epaint_default_fonts::HACK_REGULAR).expect("the bundled text font parses")
});

type Args = Punctuated<Expr, Token![,]>;
fn error(at: &impl Spanned, message: &str) -> syn::Error {
    syn::Error::new(at.span(), message)
}
fn count(args: &Args, wanted: usize) -> syn::Result<()> {
    if args.len() == wanted {
        Ok(())
    } else {
        Err(error(args, &format!("expected {wanted} arguments")))
    }
}
fn number(e: &Expr) -> syn::Result<f64> {
    let value = match e {
        Expr::Lit(l) => match &l.lit {
            Lit::Float(n) => n.base10_parse()?,
            Lit::Int(n) => n.base10_parse()?,
            _ => return Err(error(e, "expected a number")),
        },
        Expr::Unary(u) if matches!(u.op, syn::UnOp::Neg(_)) => -number(&u.expr)?,
        _ => return Err(error(e, "use a numeric literal")),
    };
    if value.is_finite() && value.abs() <= 4096. {
        Ok(value)
    } else {
        Err(error(
            e,
            "numbers must be finite and between -4096 and 4096",
        ))
    }
}
fn string(e: &Expr) -> syn::Result<String> {
    if let Expr::Lit(l) = e
        && let Lit::Str(s) = &l.lit
    {
        return Ok(s.value());
    }
    Err(error(e, "expected a quoted string"))
}
fn paint(e: &Expr) -> syn::Result<Fill> {
    if let Expr::Path(p) = e {
        let name = p
            .path
            .segments
            .last()
            .map(|s| s.ident.to_string())
            .unwrap_or_default();
        let role = match name.as_str() {
            "Background" => Background,
            "Surface" => Surface,
            "Raised" => Raised,
            "Field" => Field,
            "Primary" => Primary,
            "Secondary" => Secondary,
            "Tertiary" => Tertiary,
            "Success" => Success,
            "Warning" => Warning,
            "Danger" => Danger,
            "Ink" => Ink,
            "Dim" => Dim,
            _ => return Err(error(e, "unknown palette role")),
        };
        if p.path.segments.len() == 1
            || (p.path.segments.len() == 2 && p.path.segments[0].ident == "Role")
        {
            return Ok(role.into());
        }
    }
    if let Expr::Call(c) = e
        && let Expr::Path(p) = c.func.as_ref()
    {
        let names: Vec<_> = p
            .path
            .segments
            .iter()
            .map(|s| s.ident.to_string())
            .collect();
        if names == ["Color", "oklch"] {
            count(&c.args, 3)?;
            return Ok(Color::oklch(
                number(&c.args[0])? as f32,
                number(&c.args[1])? as f32,
                number(&c.args[2])? as f32,
            )
            .into());
        }
        if names == ["Gradient", "vertical"] {
            count(&c.args, 2)?;
            return Ok(Gradient::vertical(paint(&c.args[0])?, paint(&c.args[1])?).into());
        }
    }
    Err(error(
        e,
        "use a palette role, Color::oklch(l, c, h), or Gradient::vertical(a, b)",
    ))
}
fn element(e: &Expr, depth: usize, nodes: &mut usize) -> syn::Result<El> {
    *nodes += 1;
    if depth > 48 || *nodes > 512 {
        return Err(error(e, "scene exceeds the playground complexity limit"));
    }
    let child = |e: &Expr, nodes: &mut usize| element(e, depth + 1, nodes);
    match e {
        Expr::Paren(p) => child(&p.expr, nodes),
        Expr::Macro(m) => {
            let name = m
                .mac
                .path
                .get_ident()
                .map(ToString::to_string)
                .unwrap_or_default();
            let args = Args::parse_terminated.parse2(m.mac.tokens.clone())?;
            let children = args
                .iter()
                .map(|a| child(a, nodes))
                .collect::<syn::Result<Vec<_>>>()?;
            match name.as_str() {
                "row" => Ok(row(children)),
                "col" => Ok(col(children)),
                "stack" => Ok(stack(children)),
                _ => Err(error(e, "supported macros: row!, col!, stack!")),
            }
        }
        Expr::Call(c) => {
            let name = if let Expr::Path(p) = c.func.as_ref() {
                p.path.get_ident().map(ToString::to_string)
            } else {
                None
            };
            match name.as_deref() {
                Some("block") => {
                    count(&c.args, 2)?;
                    Ok(block(number(&c.args[0])?, number(&c.args[1])?))
                }
                Some("text") => {
                    count(&c.args, 1)?;
                    Ok(text(string(&c.args[0])?))
                }
                Some("spacer") => {
                    count(&c.args, 0)?;
                    Ok(spacer())
                }
                Some("icon") => {
                    count(&c.args, 1)?;
                    let name = string(&c.args[0])?;
                    // The subset carries a few dozen of the font's names; a
                    // name it lacks would draw .notdef, so refuse it here.
                    let in_subset =
                        |ch: &char| mui_text::glyph_path(&ICONS, *ch, 24., &[], 1.).is_ok();
                    match mui_symbols::codepoint(&name).filter(in_subset) {
                        Some(ch) => Ok(icon(ICONS.clone(), ch)),
                        None => Err(error(
                            &c.args[0],
                            "not an icon in the playground's Material Symbols subset; see crates/mui-playground/fonts/README.md",
                        )),
                    }
                }
                _ => Err(error(
                    e,
                    "supported constructors: block(w, h), text(\"…\"), icon(\"home\"), spacer()",
                )),
            }
        }
        Expr::MethodCall(m) => {
            let el = child(&m.receiver, nodes)?;
            let name = m.method.to_string();
            let args = &m.args;
            let arity = match name.as_str() {
                "pill" | "center" | "start" | "end" | "between" | "clip" | "wrap" | "full"
                | "segmented" | "no_fill" | "no_border" => 0,
                "offset" | "border" | "shell" | "pad_xy" => 2,
                _ => 1,
            };
            count(args, arity)?;
            Ok(match name.as_str() {
                "w" | "width" => el.width(number(&args[0])?),
                "h" | "height" => el.height(number(&args[0])?),
                "square" => el.square(number(&args[0])?),
                "gap" => el.gap(number(&args[0])?),
                "pad" => el.pad(number(&args[0])?),
                "pad_xy" => el.pad_xy(number(&args[0])?, number(&args[1])?),
                "flex" => el.flex(number(&args[0])?),
                "grow" => el.grow(number(&args[0])?),
                "inside" => el.inside(number(&args[0])?),
                "bend" => el.bend(number(&args[0])?),
                "radius" => el.radius(number(&args[0])?),
                "pill" => el.pill(),
                "fill" => el.fill(paint(&args[0])?),
                "union" => el.union(paint(&args[0])?),
                "stroke" => el.stroke(paint(&args[0])?),
                "border" => el.border(paint(&args[0])?, number(&args[1])?),
                "shell" => el.shell(number(&args[0])?, paint(&args[1])?),
                "opacity" => el.opacity(number(&args[0])? as f32),
                "offset" => el.offset(number(&args[0])?, number(&args[1])?),
                "id" => el.id(string(&args[0])?),
                "text_size" => el.text_size(number(&args[0])?),
                "icon_fill" => el.icon_fill(number(&args[0])? as f32),
                "grade" => el.grade(number(&args[0])? as f32),
                "weight" => el.text_axis("wght", number(&args[0])? as f32),
                "cut" => el.cut(child(&args[0], nodes)?),
                "keep" => el.keep(child(&args[0], nodes)?),
                "center" => el.center(),
                "start" => el.start(),
                "end" => el.end(),
                "between" => el.between(),
                "clip" => el.clip(),
                "wrap" => el.wrap(),
                "full" => el.full(),
                "segmented" => el.segmented(),
                "no_fill" => el.no_fill(),
                "no_border" => el.no_border(),
                _ => {
                    return Err(error(
                        e,
                        "method is not in the playground subset; see Syntax",
                    ));
                }
            })
        }
        _ => Err(error(
            e,
            "enter one MUI expression; variables, loops and arbitrary Rust are not supported",
        )),
    }
}
fn parse(source: &str) -> Result<El, String> {
    if source.len() > 16_384 {
        return Err("Keep the scene below 16 KiB.".into());
    }
    // Bound nesting before the Rust parser sees user-controlled tokens. Quoted
    // brackets count too: conservative limits are fine for this small editor.
    let mut nesting = 0usize;
    for ch in source.chars() {
        if "([{".contains(ch) {
            nesting += 1;
        }
        if ")]}".contains(ch) {
            nesting = nesting.saturating_sub(1);
        }
        if nesting > 64 {
            return Err("Scene nesting is limited to 64 levels.".into());
        }
    }
    let expr = syn::parse_str::<Expr>(source).and_then(|e| element(&e, 0, &mut 0));
    expr.map_err(|e| {
        let at = e.span().start();
        format!("{}:{}: {e}", at.line, at.column + 1)
    })
}
#[wasm_bindgen]
pub struct Frame {
    pixels: Vec<u8>,
    pub surfaces: u32,
    pub paint_ops: u32,
}
#[wasm_bindgen]
impl Frame {
    /// Consumes the frame so repeated edits do not retain WASM pixel buffers.
    pub fn pixels(self) -> Vec<u8> {
        self.pixels
    }
}
#[wasm_bindgen]
pub fn render(source: &str, width: u16, height: u16) -> Result<Frame, String> {
    if !(64..=1024).contains(&width) || !(64..=1024).contains(&height) {
        return Err("Canvas dimensions must be between 64 and 1024.".into());
    }
    let root = stack([parse(source)?]).center().pad(32.).fill(Background);
    let mut spec = SceneSpec::new(root).offered(Size::new(width.into(), height.into()));
    spec.font = Some(TEXT.clone());
    spec.theme.palette =
        mui_scene::Palette::from_seed(Color::oklch(0.75, 0.14, 260.), mui_scene::Mode::Dark);
    spec.theme.palette.neutral = mui_scene::Palette::NEUTRAL.neutral;
    let scene = resolve(&spec).map_err(|e| e.to_string())?;
    let mut ctx = vello_cpu::RenderContext::new(width, height);
    let mut resources = vello_cpu::Resources::default();
    mui_vello::paint(
        &mut mui_vello::Cpu {
            ctx: &mut ctx,
            resources: &mut resources,
            cache: &mut mui_vello::Cache::default(),
        },
        &scene,
        mui_vello::kurbo::Affine::IDENTITY,
    )
    .map_err(|e| e.to_string())?;
    ctx.flush();
    let mut pixmap = vello_cpu::Pixmap::new(width, height);
    ctx.render(&mut pixmap, &mut resources);
    Ok(Frame {
        surfaces: scene.surfaces().count() as u32,
        paint_ops: scene.paint.len() as u32,
        pixels: pixmap
            .take_unpremultiplied()
            .iter()
            .flat_map(|p| [p.r, p.g, p.b, p.a])
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    const EXAMPLES: [&str; 5] = [
        include_str!("../../../playground/examples/icons.mui"),
        include_str!("../../../playground/examples/weld.mui"),
        include_str!("../../../playground/examples/regions.mui"),
        include_str!("../../../playground/examples/cut.mui"),
        include_str!("../../../playground/examples/layout.mui"),
    ];
    #[test]
    fn published_examples_render_and_edits_change_pixels() {
        for source in EXAMPLES {
            let frame = render(source, 640, 480).expect(source);
            assert_eq!(frame.pixels.len(), 640 * 480 * 4);
            assert!(frame.surfaces > 1);
            assert!(
                frame
                    .pixels
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|p| p != &frame.pixels[..4])
            );
        }
        let a = render("block(100., 100.).fill(Primary)", 320, 240).unwrap();
        let b = render("block(100., 100.).fill(Secondary)", 320, 240).unwrap();
        assert_ne!(a.pixels, b.pixels);
    }
    #[test]
    fn icons_morph_with_fill_and_reject_names_outside_the_subset() {
        let at = |fill: &str| {
            render(
                &format!("icon(\"favorite\").text_size(96.).icon_fill({fill}).fill(Ink)"),
                160,
                160,
            )
            .unwrap()
            .pixels
        };
        let blank = render("spacer()", 160, 160).unwrap().pixels;
        let ink = |px: &[u8]| px.iter().zip(&blank).filter(|(a, b)| a != b).count();
        let (hollow, half, solid) = (ink(&at("0.")), ink(&at("0.5")), ink(&at("1.")));
        assert!(hollow < half && half < solid, "{hollow} {half} {solid}");
        assert!(parse("icon(\"home\").weight(700.).grade(200.)").is_ok());
        // In the font, not in the subset.
        assert!(parse("icon(\"10k\")").is_err());
        assert!(parse("icon(\"no_such_icon\")").is_err());
    }
    #[test]
    fn malformed_and_unbounded_programs_are_rejected() {
        for source in [
            "loop {}",
            "std::process::exit(0)",
            "block(2.)",
            "block(2., 3.).unknown(0.)",
            "block(1e99, 20.)",
            "block(2., 3.).fill(Unknown)",
            "block(2., 3.).fill(Primary).offset(0.)",
        ] {
            assert!(parse(source).is_err(), "accepted {source}");
        }
        assert!(parse(&"(".repeat(100)).is_err());
        assert!(parse(&"x".repeat(17_000)).is_err());
        assert!(render("block(10., 10.)", 65535, 65535).is_err());
    }
}

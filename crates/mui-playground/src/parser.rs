use mui::prelude::*;
#[derive(Clone, Debug)]
enum Value {
    Number(f64),
    Text(String),
    Name(String),
    List(Vec<Value>),
    Item(Box<Item>),
    Flow(Flow),
    Round(Rounding),
    Track(Track),
}
#[derive(Clone, Debug)]
struct Token {
    value: String,
    start: usize,
    end: usize,
    kind: u8,
}
#[derive(Debug)]
pub struct Parameter {
    pub start: usize,
    pub end: usize,
    pub label: String,
    pub value: f64,
    pub pixels: bool,
}
struct Parser<'a> {
    source: &'a str,
    tokens: Vec<Token>,
    pos: usize,
    depth: usize,
    parameters: Vec<Parameter>,
}
pub fn parse(source: &str) -> Result<(Item, Vec<Parameter>), String> {
    if source.len() > 65536 {
        return Err("Code exceeds 64 KiB".into());
    }
    let tokens = lex(source)?;
    let mut p = Parser {
        source,
        tokens,
        pos: 0,
        depth: 0,
        parameters: Vec::new(),
    };
    let result = p.expr("value")?;
    if p.peek() == Some(";") {
        p.pos += 1;
    }
    if p.pos != p.tokens.len() {
        return p.error("Unexpected trailing code");
    }
    match result {
        Value::Item(item) => Ok((*item, p.parameters)),
        _ => Err("The code must produce an item or container".into()),
    }
}
fn lex(s: &str) -> Result<Vec<Token>, String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < s.len() {
        let c = s.as_bytes()[i];
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if s[i..].starts_with("//") {
            while i < s.len() && s.as_bytes()[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        let start = i;
        let (kind, value) = if c == b'"' {
            i += 1;
            let mut value = String::new();
            let mut closed = false;
            while i < s.len() {
                let ch = s[i..].chars().next().unwrap();
                i += ch.len_utf8();
                if ch == '"' {
                    closed = true;
                    break;
                }
                if ch == '\\' {
                    let escaped = s[i..].chars().next().ok_or("Unclosed escape")?;
                    i += escaped.len_utf8();
                    value.push(match escaped {
                        'n' => '\n',
                        't' => '\t',
                        'r' => '\r',
                        '"' => '"',
                        '\\' => '\\',
                        _ => return Err("Supported escapes: n, r, t, quote, backslash".into()),
                    });
                } else {
                    value.push(ch);
                }
            }
            if !closed {
                return Err("Unclosed string".into());
            }
            (1, value)
        } else if c.is_ascii_digit() || c == b'-' {
            i += 1;
            while i < s.len() && (s.as_bytes()[i].is_ascii_digit() || s.as_bytes()[i] == b'.') {
                i += 1;
            }
            if s[i..].starts_with("px") {
                i += 2;
            }
            (2, s[start..i].to_owned())
        } else if c.is_ascii_alphabetic() || c == b'_' {
            i += 1;
            while i < s.len()
                && (s.as_bytes()[i].is_ascii_alphanumeric() || b"_:".contains(&s.as_bytes()[i]))
            {
                i += 1;
            }
            (0, s[start..i].to_owned())
        } else if b".(),[];".contains(&c) {
            i += 1;
            (3, s[start..i].to_owned())
        } else {
            return Err(format!("Unexpected character at byte {i}"));
        };
        out.push(Token {
            value,
            start,
            end: i,
            kind,
        });
        if out.len() > 8192 {
            return Err("Too many tokens".into());
        }
    }
    Ok(out)
}
impl Parser<'_> {
    fn peek(&self) -> Option<&str> {
        self.tokens.get(self.pos).map(|t| t.value.as_str())
    }
    fn error<T>(&self, message: &str) -> Result<T, String> {
        Err(format!(
            "{message} near byte {}",
            self.tokens
                .get(self.pos)
                .map_or(self.source.len(), |t| t.start)
        ))
    }
    fn take(&mut self, s: &str) -> Result<(), String> {
        if self.peek() != Some(s) {
            return self.error(&format!("Expected {s}"));
        }
        self.pos += 1;
        Ok(())
    }
    fn args(&mut self, label: &str, end: &str) -> Result<Vec<Value>, String> {
        let mut args = Vec::new();
        while self.peek() != Some(end) {
            args.push(self.expr(label)?);
            if self.peek() != Some(",") {
                break;
            }
            self.pos += 1;
        }
        self.take(end)?;
        Ok(args)
    }
    fn expr(&mut self, label: &str) -> Result<Value, String> {
        self.depth += 1;
        if self.depth > 96 {
            return self.error("Expression nesting limit");
        }
        let t = self
            .tokens
            .get(self.pos)
            .ok_or("Unexpected end of code")?
            .clone();
        self.pos += 1;
        let mut value = match t.kind {
            1 => Value::Text(t.value),
            2 => {
                let pixels = t.value.ends_with("px");
                let number = t
                    .value
                    .trim_end_matches("px")
                    .parse::<f64>()
                    .map_err(|_| "Invalid number")?;
                if !number.is_finite() || number < 0. || number > 1e6 {
                    return self.error("Number must be finite and in 0..1,000,000");
                }
                self.parameters.push(Parameter {
                    start: self.source[..t.start].encode_utf16().count(),
                    end: self.source[..t.end].encode_utf16().count(),
                    label: label.into(),
                    value: number,
                    pixels,
                });
                Value::Number(number)
            }
            0 => {
                if self.peek() == Some("(") {
                    self.pos += 1;
                    let a = self.args(&t.value, ")")?;
                    call(&t.value, a)?
                } else {
                    Value::Name(t.value)
                }
            }
            3 if t.value == "[" => Value::List(self.args(label, "]")?),
            _ => return self.error("Expected an item, value or list"),
        };
        while self.peek() == Some(".") {
            self.pos += 1;
            let name = self
                .tokens
                .get(self.pos)
                .ok_or("Missing method")?
                .value
                .clone();
            self.pos += 1;
            self.take("(")?;
            let args = self.args(&name, ")")?;
            value = match value {
                Value::Item(item) => Value::Item(Box::new(method(*item, &name, args)?)),
                _ => return self.error("Methods require an item"),
            };
        }
        self.depth -= 1;
        Ok(value)
    }
}
fn number(v: Value) -> Result<f64, String> {
    match v {
        Value::Number(n) => Ok(n),
        _ => Err("Expected number".into()),
    }
}
fn integer(v: Value) -> Result<usize, String> {
    let n = number(v)?;
    if n.fract() != 0. || n < 1. || n > 256. {
        return Err("Expected integer in 1..256".into());
    }
    Ok(n as usize)
}
fn text(v: Value) -> Result<String, String> {
    match v {
        Value::Text(s) => Ok(s),
        _ => Err("Expected quoted text".into()),
    }
}
fn name(v: Value) -> Result<String, String> {
    match v {
        Value::Name(s) => Ok(s),
        _ => Err("Expected a named option".into()),
    }
}
fn list(v: Value) -> Result<Vec<Value>, String> {
    match v {
        Value::List(s) => Ok(s),
        _ => Err("Expected [...]".into()),
    }
}
fn items(v: Value) -> Result<Vec<Item>, String> {
    list(v)?
        .into_iter()
        .map(|v| match v {
            Value::Item(i) => Ok(*i),
            _ => Err("Expected item".into()),
        })
        .collect()
}
fn spacing(v: Value) -> Result<Spacing, String> {
    match v {
        Value::Number(n) => Ok(n.into()),
        Value::Name(s) => Ok(match s.as_str() {
            "S" => S.into(),
            "M" => M.into(),
            "L" => L.into(),
            "Xs" => Xs.into(),
            "Xl" => Xl.into(),
            _ => return Err("Use Xs, S, M, L, Xl or pixels".into()),
        }),
        _ => Err("Expected spacing".into()),
    }
}
fn sizing(v: Value) -> Result<Sizing, String> {
    match v {
        Value::Number(n) => Ok(n.into()),
        Value::Name(s) if s == "Fill" => Ok(Fill),
        Value::Name(s) if s == "Hug" => Ok(Hug),
        _ => Err("Use Fill, Hug or pixels".into()),
    }
}
fn horizontal(v: Value) -> Result<Horizontal, String> {
    Ok(match name(v)?.as_str() {
        "Left" => Left,
        "Center" => Center,
        "Right" => Right,
        "Stretch" => Horizontal::Stretch,
        _ => return Err("Use Left, Center, Right, Stretch".into()),
    })
}
fn vertical(v: Value) -> Result<Vertical, String> {
    Ok(match name(v)?.as_str() {
        "Top" => Top,
        "Middle" => Middle,
        "Bottom" => Bottom,
        "Stretch" => Vertical::Stretch,
        _ => return Err("Use Top, Middle, Bottom, Stretch".into()),
    })
}
fn color(v: Value) -> Result<Color, String> {
    Ok(match name(v)?.trim_start_matches("Color::") {
        "Canvas" => Color::Canvas,
        "Panel" => Color::Panel,
        "Raised" => Color::Raised,
        "Text" => Color::Text,
        "Muted" => Color::Muted,
        "Outline" => Color::Outline,
        "Primary1" => Color::Primary(0),
        "Primary2" => Color::Primary(1),
        "Primary3" => Color::Primary(2),
        _ => return Err("Use Panel, Raised, Canvas or Primary1/2/3".into()),
    })
}
fn count(a: &[Value], n: usize) -> Result<(), String> {
    if a.len() != n {
        Err(format!("Expected {n} arguments; got {}", a.len()))
    } else {
        Ok(())
    }
}
fn call(f: &str, a: Vec<Value>) -> Result<Value, String> {
    if f == "Rounding::separate" {
        count(&a, 2)?;
        let mut a = a.into_iter();
        return Ok(Value::Round(Rounding::separate(
            spacing(a.next().unwrap())?,
            spacing(a.next().unwrap())?,
        )));
    }
    count(&a, 1)?;
    let v = a.into_iter().next().unwrap();
    Ok(match f {
        "item" => Value::Item(Box::new(item(text(v)?))),
        "container" => Value::Item(Box::new(container(items(v)?))),
        "Grid" => Value::Flow(Grid(integer(v)?)),
        "Track::Fixed" => Value::Track(Track::Fixed(number(v)?)),
        "Track::Fraction" => Value::Track(Track::Fraction(number(v)?)),
        _ => return Err(format!("Unknown function {f}")),
    })
}
fn method(item: Item, m: &str, a: Vec<Value>) -> Result<Item, String> {
    let expected = match m {
        "center" | "wrap" | "hoverable" => 0,
        "position" | "place" | "cell" | "span" | "min" | "max" | "stroke" => 2,
        _ => 1,
    };
    count(&a, expected)?;
    let mut args = a.into_iter();
    let mut arg = || args.next().unwrap();
    Ok(match m {
        "children" => item.children(items(arg())?),
        "text" => item.text(text(arg())?),
        "on_tap" => item.on_tap(text(arg())?),
        "scope" => item.scope(text(arg())?),
        "id" => item.id(text(arg())?),
        "extend_to" => item.extend_to(text(arg())?),
        "merge" => item.merge(
            list(arg())?
                .into_iter()
                .map(text)
                .collect::<Result<Vec<_>, _>>()?,
        ),
        "layout" => item.layout(match arg() {
            Value::Flow(f) => f,
            v => match name(v)?.as_str() {
                "Row" => Row,
                "Column" => Column,
                "Auto" => Auto,
                "Overlay" => Overlay,
                _ => return Err("Use Row, Column, Auto, Overlay or Grid(n)".into()),
            },
        }),
        "width" => item.width(sizing(arg())?),
        "height" => item.height(sizing(arg())?),
        "pad" => item.pad(spacing(arg())?),
        "gap" => item.gap(spacing(arg())?),
        "round" => item.round(match arg() {
            Value::Round(r) => r,
            v => Rounding::Both(spacing(v)?),
        }),
        "color" => item.color(color(arg())?),
        "hover_color" => item.hover_color(color(arg())?),
        "stroke" => item.stroke(color(arg())?, number(arg())?),
        "center" => item.center(),
        "wrap" => item.wrap(),
        "hoverable" => item.hoverable(),
        "position" => item.position(horizontal(arg())?, vertical(arg())?),
        "place" => item.place(horizontal(arg())?, vertical(arg())?),
        "cell" => item.cell(integer(arg())?, integer(arg())?),
        "span" => item.span(integer(arg())?, integer(arg())?),
        "min" => item.min(number(arg())?, number(arg())?),
        "max" => item.max(number(arg())?, number(arg())?),
        "grow" => item.grow(number(arg())?),
        "shrink" => item.shrink(number(arg())?),
        "pack" => item.pack(match name(arg())?.as_str() {
            "Start" => Start,
            "Center" => Justify::Center,
            "End" => End,
            "SpaceBetween" => SpaceBetween,
            "SpaceEvenly" => SpaceEvenly,
            "SpaceAround" => SpaceAround,
            _ => return Err("Unknown distribution".into()),
        }),
        "columns" | "rows" => {
            let tracks = list(arg())?
                .into_iter()
                .map(|v| match v {
                    Value::Track(t) => Ok::<Track, String>(t),
                    Value::Name(s) if s == "Track::Hug" => Ok(Track::Hug),
                    _ => Err("Expected Track::Fixed/Fraction/Hug".into()),
                })
                .collect::<Result<Vec<_>, _>>()?;
            if m == "columns" {
                item.columns(tracks)
            } else {
                item.rows(tracks)
            }
        }
        _ => return Err(format!("Unsupported playground method .{m}")),
    })
}

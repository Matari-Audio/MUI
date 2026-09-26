//! Flat token stream over a Rust source file, with byte ranges into the
//! original text. Comments and string contents never become identifiers.

use proc_macro2::{Delimiter, Spacing, TokenStream, TokenTree};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum K {
    Ident,
    Punct(char),
    Lit,
    Open(char),
    Close(char),
}

#[derive(Clone, Debug)]
pub struct Tok {
    pub k: K,
    pub text: String,
    pub lo: usize,
    pub hi: usize,
    /// Punct glued to the next punct (`::`, `=>`).
    pub joint: bool,
    /// For Open/Close: index of the matching delimiter.
    pub pair: usize,
}

pub fn lex(src: &str) -> Result<Vec<Tok>, String> {
    // proc-macro2 rejects a leading shebang / `#!` only when it isn't an
    // inner attribute; strip it by blanking so offsets stay valid.
    let owned;
    let src = if src.starts_with("#!") && !src.starts_with("#![") {
        let end = src.find('\n').unwrap_or(src.len());
        owned = format!("{}{}", " ".repeat(end), &src[end..]);
        owned.as_str()
    } else {
        src
    };
    let ts: TokenStream = src.parse().map_err(|e: proc_macro2::LexError| {
        let s = e.span().start();
        format!("{}:{}: lex error", s.line, s.column + 1)
    })?;
    let mut out = Vec::new();
    flatten(ts, &mut out);
    Ok(out)
}

fn flatten(ts: TokenStream, out: &mut Vec<Tok>) {
    for tt in ts {
        match tt {
            TokenTree::Ident(i) => {
                let r = i.span().byte_range();
                out.push(tok(K::Ident, i.to_string(), r.start, r.end));
            }
            TokenTree::Punct(p) => {
                let r = p.span().byte_range();
                let mut t = tok(K::Punct(p.as_char()), p.as_char().to_string(), r.start, r.end);
                t.joint = p.spacing() == Spacing::Joint;
                out.push(t);
            }
            TokenTree::Literal(l) => {
                let r = l.span().byte_range();
                out.push(tok(K::Lit, l.to_string(), r.start, r.end));
            }
            TokenTree::Group(g) => {
                let (o, c) = match g.delimiter() {
                    Delimiter::Parenthesis => ('(', ')'),
                    Delimiter::Bracket => ('[', ']'),
                    Delimiter::Brace => ('{', '}'),
                    Delimiter::None => {
                        flatten(g.stream(), out);
                        continue;
                    }
                };
                let open = out.len();
                let ro = g.span_open().byte_range();
                out.push(tok(K::Open(o), o.to_string(), ro.start, ro.end));
                flatten(g.stream(), out);
                let close = out.len();
                let rc = g.span_close().byte_range();
                let mut t = tok(K::Close(c), c.to_string(), rc.start, rc.end);
                t.pair = open;
                out.push(t);
                out[open].pair = close;
            }
        }
    }
}

fn tok(k: K, text: String, lo: usize, hi: usize) -> Tok {
    Tok { k, text, lo, hi, joint: false, pair: 0 }
}

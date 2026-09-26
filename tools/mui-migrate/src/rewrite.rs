//! The rewrite engine: analyse one file's tokens, apply `rules::RULES` as
//! byte-range edits on the original text, repeat until nothing changes.

use crate::lex::{K, Tok, lex};
use crate::rules::{Arg, Gate, RULES, Rule, TUPLES};
use std::collections::{HashMap, HashSet};

pub struct Ctx {
    /// Path roots that name a mui crate (`mui`, `mui2`, `mui_scene`, ..).
    pub roots: HashSet<String>,
}

impl Ctx {
    pub fn with_roots<I: IntoIterator<Item = String>>(extra: I) -> Ctx {
        let mut roots: HashSet<String> = crate::rules::MUI_CRATES.iter().map(|c| c.replace('-', "_")).collect();
        roots.extend(crate::rules::EXTRA_ROOTS.iter().map(|s| s.to_string()));
        roots.extend(extra);
        Ctx { roots }
    }
}

#[derive(Debug, Default)]
pub struct Outcome {
    pub text: String,
    pub edits: usize,
    /// `(line, note)`.
    pub warnings: Vec<(usize, String)>,
}

pub fn migrate(src: &str, ctx: &Ctx) -> Result<Outcome, String> {
    let mut out = Outcome { text: src.to_string(), ..Default::default() };
    let mut seen = HashSet::new();
    let mut push = |out: &mut Outcome, w: (usize, String)| {
        if seen.insert(w.clone()) {
            out.warnings.push(w);
        }
    };
    {
        let toks = lex(src)?;
        let f = File::new(src, &toks, ctx);
        for w in f.manual() {
            push(&mut out, w);
        }
    }
    // Overlapping edits are deferred to the next pass; nested rewrites
    // converge in a couple of passes.
    for _ in 0..16 {
        let toks = lex(&out.text)?;
        let f = File::new(&out.text, &toks, ctx);
        let (edits, warns) = f.pass();
        if edits.is_empty() {
            out.warnings.sort();
            return Ok(out);
        }
        for w in warns {
            push(&mut out, w);
        }
        let (text, n) = apply(&out.text, edits);
        out.text = text;
        out.edits += n;
    }
    Err("rewrite did not converge".into())
}

#[derive(Debug)]
struct Edit {
    lo: usize,
    hi: usize,
    text: String,
    /// A whole-construct removal: may take its now-blank line with it.
    remove: bool,
}

fn apply(src: &str, mut edits: Vec<Edit>) -> (String, usize) {
    edits.sort_by_key(|e| (e.lo, e.hi));
    let mut out = String::with_capacity(src.len());
    let mut at = 0;
    let mut n = 0;
    for mut e in edits {
        if e.lo < at {
            continue; // overlaps an earlier edit; next pass
        }
        if e.remove {
            // A removed chain link on its own line takes its line with it.
            let before = &src[at..e.lo];
            let trimmed = before.trim_end_matches([' ', '\t']);
            if trimmed.ends_with('\n') {
                e.lo = at + trimmed.trim_end().len();
            } else if trimmed.is_empty() && at == 0 {
                // Start of file: take the following newline instead.
                let rest = &src[e.hi..];
                let ws = rest.len() - rest.trim_start_matches([' ', '\t']).len();
                if rest[ws..].starts_with('\n') {
                    e.hi += ws + 1;
                }
            }
        }
        out.push_str(&src[at..e.lo]);
        out.push_str(&e.text);
        at = e.hi;
        n += 1;
    }
    out.push_str(&src[at..]);
    (out, n)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Frame {
    Impl,
    Enum,
    Other,
}

struct UseEntry {
    path: Vec<String>,
    /// Local name; `None` for a glob.
    local: Option<String>,
    mui: bool,
    /// First and last token of the entry's own tree.
    start: usize,
    end: usize,
    in_brace: bool,
    /// For a top-level entry: the `use` item's first token and its `;`.
    item: (usize, usize),
}

struct File<'a> {
    src: &'a str,
    t: &'a [Tok],
    ctx: &'a Ctx,
    uses: Vec<UseEntry>,
    /// Token in a `use` item -> whether that item is rooted in a mui crate.
    use_tok: HashMap<usize, bool>,
    /// Innermost brace frame each token sits in.
    frame: Vec<Frame>,
    defined: HashSet<String>,
    locals: HashMap<String, bool>,
    mui_glob: bool,
    foreign_glob: bool,
    is_mui: bool,
    line_starts: Vec<usize>,
}

impl<'a> File<'a> {
    fn new(src: &'a str, t: &'a [Tok], ctx: &'a Ctx) -> File<'a> {
        let mut f = File {
            src,
            t,
            ctx,
            uses: Vec::new(),
            use_tok: HashMap::new(),
            frame: vec![Frame::Other; t.len()],
            defined: HashSet::new(),
            locals: HashMap::new(),
            mui_glob: false,
            foreign_glob: false,
            is_mui: false,
            line_starts: std::iter::once(0).chain(src.match_indices('\n').map(|(i, _)| i + 1)).collect(),
        };
        f.scan();
        f
    }

    // ---- token helpers ----

    fn ident(&self, i: usize, s: &str) -> bool {
        self.t.get(i).is_some_and(|t| t.k == K::Ident && t.text == s)
    }
    fn punct(&self, i: usize, c: char) -> bool {
        self.t.get(i).is_some_and(|t| t.k == K::Punct(c))
    }
    fn open(&self, i: usize, c: char) -> bool {
        self.t.get(i).is_some_and(|t| t.k == K::Open(c))
    }
    fn close_any(&self, i: usize) -> bool {
        self.t.get(i).is_some_and(|t| matches!(t.k, K::Close(_)))
    }
    /// `::` starting at `i`.
    fn sep(&self, i: usize) -> bool {
        self.punct(i, ':') && self.t[i].joint && self.punct(i + 1, ':')
    }
    /// `::` ending at `i - 1` (i.e. tokens i-2, i-1).
    fn sep_before(&self, i: usize) -> bool {
        i >= 2 && self.sep(i - 2)
    }
    /// Method-call dot at `i` (not part of `..`).
    fn dot(&self, i: usize) -> bool {
        self.punct(i, '.') && !self.t[i].joint && !(i > 0 && self.punct(i - 1, '.') && self.t[i - 1].joint)
    }
    fn text(&self, a: usize, b: usize) -> &'a str {
        &self.src[self.t[a].lo..self.t[b].hi]
    }
    fn norm(&self, a: usize, b: usize) -> String {
        self.t[a..=b].iter().map(|t| t.text.as_str()).collect()
    }
    fn line(&self, i: usize) -> usize {
        let lo = self.t[i].lo;
        self.line_starts.partition_point(|&s| s <= lo)
    }
    /// Top-level comma-separated args of the group opened at `open`.
    fn args(&self, open: usize) -> Vec<(usize, usize)> {
        let close = self.t[open].pair;
        let mut out = Vec::new();
        let mut a = open + 1;
        let mut j = open + 1;
        while j < close {
            match self.t[j].k {
                K::Open(_) => j = self.t[j].pair + 1,
                K::Punct(',') => {
                    out.push((a, j - 1));
                    j += 1;
                    a = j;
                }
                _ => j += 1,
            }
        }
        if a < close {
            out.push((a, close - 1));
        }
        out
    }

    // ---- analysis ----

    fn scan(&mut self) {
        let t = self.t;
        let mut stack: Vec<Frame> = Vec::new();
        let mut pending: Option<Frame> = None;
        let mut i = 0;
        while i < t.len() {
            self.frame[i] = *stack.last().unwrap_or(&Frame::Other);
            match &t[i].k {
                K::Open('{') => stack.push(pending.take().unwrap_or(Frame::Other)),
                K::Close('}') => {
                    stack.pop();
                }
                K::Punct(';') => pending = None,
                K::Ident => {
                    let s = t[i].text.as_str();
                    let item_start = i == 0
                        || matches!(t[i - 1].k, K::Punct(';') | K::Open('{') | K::Close('}') | K::Close(']') | K::Close(')'))
                        || ["pub", "unsafe", "default"].iter().any(|k| self.ident(i - 1, k));
                    let not_path = !(i > 0 && (self.dot(i - 1) || self.sep_before(i)));
                    match s {
                        "impl" | "trait" if item_start => pending = Some(Frame::Impl),
                        "enum" if not_path => pending = Some(Frame::Enum),
                        "use" if item_start => {
                            i = self.parse_use(i);
                            continue;
                        }
                        _ => {}
                    }
                    if not_path
                        && matches!(s, "fn" | "struct" | "enum" | "trait" | "type" | "mod" | "const" | "static" | "union")
                        && t.get(i + 1).is_some_and(|n| n.k == K::Ident)
                    {
                        let in_impl = stack.last() == Some(&Frame::Impl);
                        if !(in_impl && matches!(s, "fn" | "type" | "const")) {
                            self.defined.insert(t[i + 1].text.clone());
                        }
                    }
                    if s == "macro_rules" && self.punct(i + 1, '!') && t.get(i + 2).is_some_and(|n| n.k == K::Ident) {
                        self.defined.insert(t[i + 2].text.clone());
                    }
                    // `let name = [move] |..|` binds a callable.
                    if s == "let" {
                        let mut j = i + 1;
                        if self.ident(j, "mut") {
                            j += 1;
                        }
                        if t.get(j).is_some_and(|n| n.k == K::Ident) && self.punct(j + 1, '=') {
                            let mut k = j + 2;
                            if self.ident(k, "move") {
                                k += 1;
                            }
                            if self.punct(k, '|') {
                                self.defined.insert(t[j].text.clone());
                            }
                        }
                    }
                    // An inline mui path (`mui2::scene::Kind`) marks the file.
                    if not_path && self.ctx.roots.contains(s) && self.sep(i + 1) && !self.defined.contains(s) {
                        self.is_mui = true;
                    }
                }
                _ => {}
            }
            i += 1;
        }
        for u in &self.uses {
            match &u.local {
                None if u.mui => self.mui_glob = true,
                None => {
                    // `use super::*` in a test module and enum-variant
                    // globs are foreign globs too.
                    self.foreign_glob = true
                }
                Some(l) => {
                    self.locals.insert(l.clone(), u.mui);
                }
            }
            if u.mui {
                self.is_mui = true;
            }
        }
    }

    /// Parse the `use` item at `i`; returns the index after its `;`.
    fn parse_use(&mut self, i: usize) -> usize {
        let mut semi = i + 1;
        while semi < self.t.len() && !self.punct(semi, ';') {
            if let K::Open(_) = self.t[semi].k {
                semi = self.t[semi].pair;
            }
            semi += 1;
        }
        if semi >= self.t.len() {
            return semi;
        }
        let mut start = i;
        if i > 0 && self.ident(i - 1, "pub") {
            start = i - 1;
        } else if i > 1 && matches!(self.t[i - 1].k, K::Close(')')) && self.ident(self.t[i - 1].pair.wrapping_sub(1), "pub") {
            start = self.t[i - 1].pair - 1;
        }
        let first = self.uses.len();
        self.tree(i + 1, Vec::new(), false, (start, semi));
        let mui = self.uses[first..].first().is_some_and(|u| u.path.first().is_some_and(|r| self.ctx.roots.contains(r)));
        for u in &mut self.uses[first..] {
            u.mui = mui;
        }
        for j in i..=semi {
            self.use_tok.insert(j, mui);
        }
        semi + 1
    }

    fn tree(&mut self, mut j: usize, prefix: Vec<String>, in_brace: bool, item: (usize, usize)) -> usize {
        let start = j;
        let mut path = prefix.clone();
        if self.sep(j) {
            j += 2;
        }
        loop {
            let Some(tok) = self.t.get(j) else { return j };
            match tok.k {
                K::Ident => {
                    let s = tok.text.clone();
                    j += 1;
                    if self.sep(j) {
                        path.push(s);
                        j += 2;
                        continue;
                    }
                    let mut local = if s == "self" { path.last().cloned() } else { Some(s.clone()) };
                    if s != "self" {
                        path.push(s);
                    }
                    if self.ident(j, "as") && self.t.get(j + 1).is_some_and(|t| t.k == K::Ident) {
                        local = Some(self.t[j + 1].text.clone());
                        j += 2;
                    }
                    self.uses.push(UseEntry { path, local, mui: false, start, end: j - 1, in_brace, item });
                    return j;
                }
                K::Punct('*') => {
                    self.uses.push(UseEntry { path, local: None, mui: false, start, end: j, in_brace, item });
                    return j + 1;
                }
                K::Open('{') => {
                    let close = self.t[j].pair;
                    let mut k = j + 1;
                    while k < close {
                        k = self.tree(k, path.clone(), true, item);
                        if self.punct(k, ',') {
                            k += 1;
                        } else {
                            break;
                        }
                    }
                    return close + 1;
                }
                _ => return j + 1,
            }
        }
    }

    /// Does the name at token `i` (bare or path-qualified) resolve to mui?
    fn is_mui_name(&self, i: usize) -> bool {
        if let Some(&m) = self.use_tok.get(&i) {
            return m;
        }
        // Walk back over `a::b::` to the first segment.
        let mut j = i;
        while self.sep_before(j) {
            if j >= 3 && self.t[j - 3].k == K::Ident {
                j -= 3;
            } else if j >= 3 && (self.punct(j - 3, '>') || self.close_any(j - 3)) {
                return false; // `<T as X>::name`, `Vec::<T>::name`
            } else {
                break; // absolute `::root::name`
            }
        }
        let s = self.t[j].text.as_str();
        if j == i {
            // Bare name.
            if self.defined.contains(s) {
                return false;
            }
            if let Some(&m) = self.locals.get(s) {
                return m;
            }
            return self.mui_glob;
        }
        if self.defined.contains(s) {
            return false;
        }
        if let Some(&m) = self.locals.get(s) {
            return m;
        }
        self.ctx.roots.contains(s)
    }

    fn gate(&self, g: Gate, dot: usize) -> bool {
        match g {
            Gate::Any => true,
            Gate::Mui => self.is_mui,
            Gate::MuiChain => self.is_mui && dot > 0 && matches!(self.t[dot - 1].k, K::Close(')') | K::Close(']')),
        }
    }

    // ---- rules ----

    fn manual(&self) -> Vec<(usize, String)> {
        let mut out = Vec::new();
        if !self.is_mui {
            return out;
        }
        for r in RULES {
            let Rule::Manual { pattern, note } = r else { continue };
            let Ok(pat) = lex(pattern) else { continue };
            'at: for i in 0..self.t.len() {
                if self.use_tok.contains_key(&i) {
                    continue;
                }
                for (k, p) in pat.iter().enumerate() {
                    match self.t.get(i + k) {
                        Some(t) if t.text == p.text && std::mem::discriminant(&t.k) == std::mem::discriminant(&p.k) => {}
                        _ => continue 'at,
                    }
                }
                out.push((self.line(i), note.to_string()));
            }
        }
        out
    }

    fn pass(&self) -> (Vec<Edit>, Vec<(usize, String)>) {
        let t = self.t;
        let mut edits = Vec::new();
        let mut needs: Vec<(usize, &str)> = Vec::new();
        let src = self.src;
        let mut edit = |lo: usize, hi: usize, text: String| edits.push(minimal(src, lo, hi, text));

        for u in &self.uses {
            if !u.mui {
                continue;
            }
            let name = u.path.last().map(String::as_str).unwrap_or("");
            if !RULES.iter().any(|r| matches!(r, Rule::DropImport { name: n } if *n == name)) || u.local.is_none() {
                continue;
            }
            if !u.in_brace {
                let (a, b) = u.item;
                edit(t[a].lo, t[b].hi, String::new());
            } else if self.punct(u.end + 1, ',') {
                edit(t[u.start].lo, t[u.end + 2].lo, String::new());
            } else if self.punct(u.start - 1, ',') {
                edit(t[u.start - 1].lo, t[u.end].hi, String::new());
            } else {
                edit(t[u.start].lo, t[u.end].hi, String::new());
            }
        }

        for i in 0..t.len() {
            if t[i].k != K::Ident || (i > 0 && self.punct(i - 1, '\'')) {
                continue;
            }
            let name = t[i].text.as_str();
            let method = i > 0 && self.dot(i - 1);
            let called = self.open(i + 1, '(') || (self.sep(i + 1) && self.punct(i + 3, '<'));
            let in_use = self.use_tok.contains_key(&i);

            if method {
                let dot = i - 1;
                let mut done = false;
                for r in RULES {
                    match *r {
                        Rule::Call { chain, to, gate, needs: nd } if chain[0].0 == name && self.gate(gate, dot) => {
                            if let Some((end, caps)) = self.chain(dot, chain) {
                                edit(t[dot].lo, t[end].hi, expand(to, &caps));
                                needs.extend(nd.iter().map(|n| (self.line(i), *n)));
                                done = true;
                                break;
                            }
                        }
                        Rule::Method { old, new, gate } if old == name && called && self.gate(gate, dot) => {
                            edit(t[i].lo, t[i].hi, new.to_string());
                            done = true;
                            break;
                        }
                        _ => {}
                    }
                }
                if !done && self.is_mui {
                    self.tuple(i, true, &mut edit, &mut needs);
                }
                continue;
            }

            let macro_call = self.punct(i + 1, '!') && !self.punct(i + 2, '=');
            if macro_call && !in_use {
                for r in RULES {
                    match *r {
                        Rule::Macro { old, new } if old == name && self.is_mui => {
                            edit(t[i].lo, t[i].hi, new.to_string());
                        }
                        Rule::MacroHead { old, new, head } if old == name && self.is_mui => {
                            let open = i + 2;
                            if !matches!(t.get(open).map(|t| t.k), Some(K::Open(_))) {
                                continue;
                            }
                            let close = t[open].pair;
                            let mut j = open + 1;
                            while j < close && !self.punct(j, ';') {
                                if let K::Open(_) = t[j].k {
                                    j = t[j].pair;
                                }
                                j += 1;
                            }
                            if j == close || j == open + 1 {
                                continue;
                            }
                            let arg = self.text(open + 1, j - 1).to_string();
                            edit(t[i].lo, t[i].hi, new.to_string());
                            edit(t[open + 1].lo, t[j - 1].hi, expand(head, &[arg]));
                            needs.push((self.line(i), "Weld"));
                        }
                        _ => {}
                    }
                }
                continue;
            }
            if macro_call {
                if let Some(true) = self.use_tok.get(&i) {
                    for r in RULES {
                        if let Rule::Macro { old, new } | Rule::MacroHead { old, new, .. } = *r
                            && old == name
                        {
                            edit(t[i].lo, t[i].hi, new.to_string());
                        }
                    }
                }
                continue;
            }

            // Definitions and bindings are never renamed.
            if i > 0
                && ["fn", "struct", "enum", "trait", "type", "mod", "const", "static", "let", "mut", "macro_rules"]
                    .iter()
                    .any(|k| self.ident(i - 1, k))
            {
                continue;
            }
            // A struct field or named argument `name: ..` outside a path.
            let field = self.punct(i + 1, ':') && !self.sep(i + 1);
            // Variant name position inside an enum body.
            let variant_pos = self.frame[i] == Frame::Enum
                && !self.sep_before(i)
                && (i == 0 || matches!(t[i - 1].k, K::Open('{') | K::Punct(',') | K::Close(']')));
            if variant_pos {
                continue;
            }

            for r in RULES {
                match *r {
                    Rule::Function { old, new } if old == name && (called || in_use) && !field => {
                        if self.is_mui_name(i) {
                            edit(t[i].lo, t[i].hi, new.to_string());
                        }
                    }
                    Rule::Type { old, new } if old == name && !field => {
                        if self.is_mui_name(i) {
                            edit(t[i].lo, t[i].hi, new.to_string());
                        }
                    }
                    Rule::Qualify { names, prefix } if names.contains(&name) => {
                        if !in_use
                            && !field
                            && !self.sep_before(i)
                            && !self.sep(i + 1)
                            && !self.open(i + 1, '{')
                            && self.frame[i] != Frame::Enum
                            && self.mui_glob
                            && !self.foreign_glob
                            && !self.defined.contains(name)
                            && !self.locals.contains_key(name)
                        {
                            edit(t[i].lo, t[i].lo, prefix.to_string());
                        }
                    }
                    _ => {}
                }
            }
            if called && !in_use {
                self.tuple(i, false, &mut edit, &mut needs);
            }
        }

        let mut warns = Vec::new();
        let mut warned = HashSet::new();
        for (line, n) in needs {
            if self.mui_glob || self.locals.contains_key(n) || self.defined.contains(n) || !warned.insert(n) {
                continue;
            }
            warns.push((line, format!("the rewrite uses `{n}`: import it from mui (e.g. `use mui::prelude::{n};`)")));
        }
        (edits, warns)
    }

    /// Match a chain of `.m(args)` calls starting at the dot `dot`.
    fn chain(&self, dot: usize, chain: &[(&str, &[Arg])]) -> Option<(usize, Vec<String>)> {
        let mut j = dot;
        let mut caps = Vec::new();
        let mut end = dot;
        for (name, pats) in chain {
            if !self.dot(j) || !self.ident(j + 1, name) || !self.open(j + 2, '(') {
                return None;
            }
            let args = self.args(j + 2);
            if args.len() != pats.len() {
                return None;
            }
            for (&(a, b), p) in args.iter().zip(pats.iter()) {
                let norm = self.norm(a, b);
                let ok = match p {
                    Arg::Any => true,
                    Arg::Is(s) => norm == strip_ws(s),
                    Arg::Has(s) => norm.contains(&strip_ws(s)),
                };
                if !ok {
                    return None;
                }
                if !matches!(p, Arg::Is(_)) {
                    caps.push(self.text(a, b).to_string());
                }
            }
            end = self.t[j + 2].pair;
            j = end + 1;
        }
        Some((end, caps))
    }

    /// Tuple-result rewrites for the call whose name is at `i`.
    fn tuple(&self, i: usize, method: bool, edit: &mut impl FnMut(usize, usize, String), needs: &mut Vec<(usize, &'static str)>) {
        let t = self.t;
        let Some(spec) = TUPLES.iter().find(|s| s.name == t[i].text && s.method == method) else { return };
        if !self.open(i + 1, '(') || (!method && !self.is_mui_name(i)) {
            return;
        }
        let close = t[i + 1].pair;
        // `f(..).0`
        if self.dot(close + 1)
            && let Some(lit) = t.get(close + 2)
            && lit.k == K::Lit
            && let Ok(n) = lit.text.parse::<usize>()
            && let Some(field) = spec.fields.get(n)
        {
            edit(lit.lo, lit.hi, field.to_string());
            return;
        }
        // `let (a, b) = f(..)` / `let (a, b) = recv.f(..)`
        let mut s = i;
        if method {
            // The receiver: a simple `a.b.c` chain before the dot at i - 1.
            s = i - 1;
            if s == 0 || t[s - 1].k != K::Ident {
                return;
            }
            s -= 1;
            while s >= 2 && self.dot(s - 1) && t[s - 2].k == K::Ident {
                s -= 2;
            }
        } else {
            while self.sep_before(s) && s >= 3 && t[s - 3].k == K::Ident {
                s -= 3;
            }
        }
        if s < 3 || !self.punct(s - 1, '=') || (s >= 2 && t[s - 2].joint && matches!(t[s - 2].k, K::Punct(_))) {
            return;
        }
        let K::Close(')') = t[s - 2].k else { return };
        let open = t[s - 2].pair;
        if open == 0 || !self.ident(open - 1, "let") {
            return;
        }
        let items = self.args(open);
        if items.len() != spec.fields.len() {
            return;
        }
        let mut parts = Vec::new();
        let mut rest = false;
        for (&(a, b), field) in items.iter().zip(spec.fields) {
            let pat = self.text(a, b);
            let norm = self.norm(a, b);
            if norm == "_" {
                rest = true;
            } else if norm == *field || norm == format!("mut{field}") {
                parts.push(pat.to_string());
            } else if a == b || (b == a + 1 && self.ident(a, "mut")) {
                parts.push(format!("{field}: {pat}"));
            } else {
                return; // nested pattern: leave it alone
            }
        }
        if rest {
            parts.push("..".into());
        }
        edit(t[open].lo, t[s - 2].hi, format!("{} {{ {} }}", spec.ty, parts.join(", ")));
        needs.push((self.line(i), spec.ty));
    }
}

/// Shrink an edit to the part that actually changes, so a rewrite of a
/// whole call doesn't block renames inside its arguments in the same pass.
fn minimal(src: &str, mut lo: usize, mut hi: usize, text: String) -> Edit {
    if text.is_empty() {
        return Edit { lo, hi, text, remove: true };
    }
    let old = &src[lo..hi];
    let pre = old.char_indices().zip(text.chars()).take_while(|((_, a), b)| a == b).last().map_or(0, |((i, a), _)| i + a.len_utf8());
    let (old_rest, new_rest) = (&old[pre..], &text[pre..]);
    let suf = old_rest.chars().rev().zip(new_rest.chars().rev()).take_while(|(a, b)| a == b).map(|(a, _)| a.len_utf8()).sum::<usize>();
    let new = new_rest[..new_rest.len() - suf].to_string();
    lo += pre;
    hi -= suf;
    Edit { lo, hi, text: new, remove: false }
}

fn strip_ws(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

/// Expand `$n` / `$!n` in a template.
fn expand(tpl: &str, caps: &[String]) -> String {
    let mut out = String::new();
    let mut it = tpl.chars().peekable();
    while let Some(c) = it.next() {
        if c != '$' {
            out.push(c);
            continue;
        }
        let neg = it.peek() == Some(&'!');
        if neg {
            it.next();
        }
        let mut n = 0usize;
        while let Some(d) = it.peek().and_then(|d| d.to_digit(10)) {
            n = n * 10 + d as usize;
            it.next();
        }
        let arg = caps.get(n.wrapping_sub(1)).map(|s| s.trim()).unwrap_or("");
        out.push_str(&if neg { negate(arg) } else { arg.to_string() });
    }
    out
}

fn negate(x: &str) -> String {
    let simple = |s: &str| !s.contains(char::is_whitespace) && !s.contains(['&', '|', '=', '<', '>', '+', '-', '*', '/', '%', '^']);
    match x.strip_prefix('!') {
        Some(rest) if simple(rest) => rest.to_string(),
        _ if simple(x) => format!("!{x}"),
        _ => format!("!({x})"),
    }
}

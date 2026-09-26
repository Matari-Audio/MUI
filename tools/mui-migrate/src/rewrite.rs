//! The rewrite engine: analyse one file's tokens, apply `rules::RULES` as
//! byte-range edits on the original text, repeat until nothing changes.

use crate::lex::{K, Tok, lex};
use crate::rules::{Arg, Gate, RULES, Rule, TUPLES, WIDGET_RULES};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

pub struct Ctx {
    /// Path roots that name a mui crate (`mui`, `mui2`, `mui_scene`, ..).
    pub roots: HashSet<String>,
    /// Per-file names and imports, so `use super::{Kind}` can be traced to
    /// the parent module's `use mui2::prelude::*`.
    summaries: HashMap<PathBuf, Summary>,
    /// Directories of the mui crates themselves: inside one, the crate's own
    /// `crate::`/`super::` names are mui names.
    own: Vec<PathBuf>,
    /// Also apply the widget phase (`WIDGET_RULES`, `TUPLES`); on unless `--no-widgets`.
    pub widgets: bool,
}

#[derive(Default)]
struct Summary {
    defined: HashSet<String>,
    /// `(path, local name or None for a glob)`.
    entries: Vec<(Vec<String>, Option<String>)>,
}

impl Ctx {
    pub fn with_roots<I: IntoIterator<Item = String>>(extra: I) -> Ctx {
        let mut roots: HashSet<String> = crate::rules::MUI_CRATES.iter().map(|c| c.replace('-', "_")).collect();
        roots.extend(crate::rules::EXTRA_ROOTS.iter().map(|s| s.to_string()));
        roots.extend(extra);
        Ctx { roots, summaries: HashMap::new(), own: Vec::new(), widgets: true }
    }

    /// Mark `dir` as the root of a mui crate.
    pub fn own_crate(&mut self, dir: &Path) {
        self.own.push(dir.to_path_buf());
    }

    fn is_own(&self, file: &Path) -> bool {
        self.own.iter().any(|d| file.starts_with(d))
    }

    fn rules(&self) -> impl Iterator<Item = &'static Rule> {
        RULES.iter().chain(if self.widgets { WIDGET_RULES } else { &[] })
    }

    /// Record a file's definitions and imports for cross-file resolution.
    /// Call for every file before migrating any of them.
    pub fn index(&mut self, path: &Path, src: &str) {
        let Ok(toks) = lex(src) else { return };
        let f = File::new(src, &toks, self, None, false);
        let s = Summary { defined: f.defined.clone(), entries: f.uses.iter().map(|u| (u.path.clone(), u.local.clone())).collect() };
        self.summaries.insert(path.to_path_buf(), s);
    }

    /// Does `name`, used bare in `file`, come from a mui crate?
    fn resolve(&self, file: &Path, name: &str, depth: usize) -> Option<bool> {
        let s = self.summaries.get(file)?;
        if depth > 8 || s.defined.contains(name) {
            return Some(false);
        }
        if let Some((path, _)) = s.entries.iter().find(|(_, l)| l.as_deref() == Some(name)) {
            return self.resolve_path(file, path, &s.defined, depth + 1);
        }
        for (path, _) in s.entries.iter().filter(|(_, l)| l.is_none()) {
            if path.first().is_some_and(|r| self.roots.contains(r)) {
                return Some(true);
            }
            if let Some(m) = self.module(file, path)
                && self.resolve(&m, name, depth + 1) == Some(true)
            {
                return Some(true);
            }
        }
        None
    }

    /// Does the imported item `path` (last segment = the item) come from
    /// mui? `None` when it leads into a module that is not indexed.
    fn resolve_path(&self, file: &Path, path: &[String], defined: &HashSet<String>, depth: usize) -> Option<bool> {
        let root = path.first()?;
        if self.roots.contains(root) && !defined.contains(root) {
            return Some(true);
        }
        match self.module(file, &path[..path.len() - 1]) {
            Some(m) if path.len() > 1 => self.resolve(&m, &path[path.len() - 1], depth),
            _ => None,
        }
    }

    /// The file of the module named by a `crate::` / `self::` / `super::`
    /// path, if it is one of the indexed files.
    fn module(&self, file: &Path, path: &[String]) -> Option<PathBuf> {
        let mut m = file.to_path_buf();
        let mut rest = path;
        match path.first().map(String::as_str) {
            Some("crate") => {
                let root = file.ancestors().find(|d| d.join("Cargo.toml").is_file())?;
                m = ["src/lib.rs", "src/main.rs"].iter().map(|f| root.join(f)).find(|f| self.summaries.contains_key(f))?;
                rest = &path[1..];
            }
            Some("self") => rest = &path[1..],
            // `use node::X` from the parent of `node.rs`.
            Some(seg) if self.file_for(&module_dir(file).join(seg)).is_some() => {}
            Some("super") => {
                while rest.first().map(String::as_str) == Some("super") {
                    m = self.file_for(module_dir(&m).parent()?)?;
                    rest = &rest[1..];
                }
            }
            _ => return None,
        }
        for seg in rest {
            m = self.file_for(&module_dir(&m).join(seg))?;
        }
        Some(m)
    }

    fn file_for(&self, dir: &Path) -> Option<PathBuf> {
        [dir.with_extension("rs"), dir.join("mod.rs"), dir.join("lib.rs"), dir.join("main.rs")]
            .into_iter()
            .find(|f| self.summaries.contains_key(f))
    }
}

/// The directory a module file's children live in.
fn module_dir(file: &Path) -> PathBuf {
    match file.file_name().and_then(|n| n.to_str()) {
        Some("mod.rs" | "lib.rs" | "main.rs") => file.parent().map(Path::to_path_buf).unwrap_or_default(),
        _ => file.with_extension(""),
    }
}

#[derive(Debug, Default)]
pub struct Outcome {
    pub text: String,
    pub edits: usize,
    /// `(line, note)`.
    pub warnings: Vec<(usize, String)>,
}

/// Whether the file uses mui at all (imports it, or is part of it).
pub fn uses_mui(src: &str, path: Option<&Path>, ctx: &Ctx) -> bool {
    lex(src).is_ok_and(|toks| File::new(src, &toks, ctx, path, false).is_mui)
}

pub fn migrate(src: &str, path: Option<&Path>, ctx: &Ctx) -> Result<Outcome, String> {
    let mut out = Outcome { text: src.to_string(), ..Default::default() };
    let mut seen = HashSet::new();
    let mut push = |out: &mut Outcome, w: (usize, String)| {
        if seen.insert(w.clone()) {
            out.warnings.push(w);
        }
    };
    {
        let toks = lex(src)?;
        let f = File::new(src, &toks, ctx, path, false);
        for w in f.manual() {
            push(&mut out, w);
        }
    }
    // Overlapping edits are deferred to the next pass; nested rewrites
    // converge in a couple of passes.
    for _ in 0..16 {
        let toks = lex(&out.text)?;
        let f = File::new(&out.text, &toks, ctx, path, false);
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
pub(crate) struct Edit {
    pub(crate) lo: usize,
    pub(crate) hi: usize,
    pub(crate) text: String,
    /// A whole-construct removal: may take its now-blank line with it.
    pub(crate) remove: bool,
}

/// A removed chain link on its own line takes its line with it: widen the
/// removal back to the end of the previous line's content, but not before `at`.
pub(crate) fn widen(src: &str, at: usize, e: &mut Edit) -> bool {
    let before = &src[at..e.lo];
    let trimmed = before.trim_end_matches([' ', '\t']);
    if trimmed.ends_with('\n') {
        e.lo = at + trimmed.trim_end().len();
        return true;
    }
    false
}

pub(crate) fn apply(src: &str, mut edits: Vec<Edit>) -> (String, usize) {
    edits.sort_by_key(|e| (e.lo, e.hi));
    let mut out = String::with_capacity(src.len());
    let mut at = 0;
    let mut n = 0;
    for mut e in edits {
        if e.lo < at {
            continue; // overlaps an earlier edit; next pass
        }
        if e.remove && !widen(src, at, &mut e) {
            let trimmed = src[at..e.lo].trim_end_matches([' ', '\t']);
            if trimmed.is_empty() && at == 0 {
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

pub(crate) struct File<'a> {
    path: Option<&'a Path>,
    /// Inside a mui crate (or a doc example assumed to be mui): names that
    /// come from the crate itself, or from nowhere this tool can see, are mui.
    own: bool,
    src: &'a str,
    t: &'a [Tok],
    ctx: &'a Ctx,
    uses: Vec<UseEntry>,
    /// Token in a `use` item -> whether that item is rooted in a mui crate.
    use_tok: HashMap<usize, bool>,
    /// Innermost brace frame each token sits in.
    frame: Vec<Frame>,
    defined: HashSet<String>,
    /// `let name = |..|` closures: the name and the token span of the block
    /// it is visible in, so one fn's local `button` does not hide the widget
    /// from the rest of the file.
    closures: Vec<(String, usize, usize)>,
    locals: HashMap<String, bool>,
    mui_glob: bool,
    foreign_glob: bool,
    /// Modules glob-imported with a relative path (`use super::*`).
    glob_mods: Vec<PathBuf>,
    /// Token ranges of `use` items.
    items: Vec<(usize, usize)>,
    is_mui: bool,
    line_starts: Vec<usize>,
}

impl<'a> File<'a> {
    pub(crate) fn new(src: &'a str, t: &'a [Tok], ctx: &'a Ctx, path: Option<&'a Path>, assume: bool) -> File<'a> {
        let own = assume || path.is_some_and(|p| ctx.is_own(p));
        let mut f = File {
            path,
            own,
            src,
            t,
            ctx,
            uses: Vec::new(),
            use_tok: HashMap::new(),
            frame: vec![Frame::Other; t.len()],
            defined: HashSet::new(),
            closures: Vec::new(),
            locals: HashMap::new(),
            mui_glob: false,
            foreign_glob: false,
            glob_mods: Vec::new(),
            items: Vec::new(),
            is_mui: own,
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
        // The open brace of each frame on `stack`.
        let mut opens: Vec<usize> = Vec::new();
        let mut pending: Option<Frame> = None;
        let mut i = 0;
        while i < t.len() {
            self.frame[i] = *stack.last().unwrap_or(&Frame::Other);
            match &t[i].k {
                K::Open('{') => {
                    stack.push(pending.take().unwrap_or(Frame::Other));
                    opens.push(i);
                }
                K::Close('}') => {
                    stack.pop();
                    opens.pop();
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
                                let end = opens.last().map_or(t.len(), |&o| t[o].pair);
                                self.closures.push((t[j].text.clone(), i, end));
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
        for &(a, b) in &self.items {
            for j in a..=b {
                self.use_tok.insert(j, false);
            }
        }
        for k in 0..self.uses.len() {
            let u = &self.uses[k];
            let root_mui = u.path.first().is_some_and(|r| self.ctx.roots.contains(r));
            let relative = u.path.first().is_some_and(|r| matches!(r.as_str(), "crate" | "self" | "super"))
                || self.path.is_some_and(|p| u.path.len() > 1 && self.ctx.module(p, &u.path[..1]).is_some());
            let own = self.own && relative;
            let mui = match (&u.local, self.path) {
                (None, p) => root_mui || (own && p.and_then(|p| self.ctx.module(p, &u.path)).is_none()),
                (Some(_), None) => root_mui || own,
                (Some(_), Some(p)) => self.ctx.resolve_path(p, &u.path, &self.defined, 0).unwrap_or(own),
            };
            match &u.local {
                None if mui => self.mui_glob = true,
                None => {
                    self.foreign_glob |= !own;
                    if let Some(m) = self.path.and_then(|p| self.ctx.module(p, &u.path)) {
                        self.glob_mods.push(m);
                    }
                }
                Some(l) => {
                    self.locals.insert(l.clone(), mui);
                }
            }
            self.is_mui |= mui;
            for j in u.start..=u.end {
                self.use_tok.insert(j, mui);
            }
            self.uses[k].mui = mui;
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
        self.tree(i + 1, Vec::new(), false, (start, semi));
        self.items.push((i, semi));
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
            } else if j >= 3 && self.punct(j - 3, '>') {
                // `Node::<T>::name` walks on to `Node`; `<T as X>::name` stops.
                let (mut k, mut depth) = (j - 3, 0i32);
                loop {
                    depth += i32::from(self.punct(k, '>')) - i32::from(self.punct(k, '<'));
                    if depth == 0 || k == 0 {
                        break;
                    }
                    k -= 1;
                }
                if depth != 0 || !self.sep_before(k) || k < 3 || self.t[k - 3].k != K::Ident {
                    return false;
                }
                j = k - 3;
            } else if j >= 3 && self.close_any(j - 3) {
                return false;
            } else {
                break; // absolute `::root::name`
            }
        }
        let s = self.t[j].text.as_str();
        // A type root (`Node::leaf`) resolves like a bare name; inside a mui
        // crate every type is the crate's own.
        let ty = j != i && s != "Self" && s.starts_with(char::is_uppercase) && !self.defined.contains(s);
        if ty && self.own {
            return true;
        }
        if j == i || ty {
            if self.defined.contains(s) || self.closures.iter().any(|(n, a, b)| n == s && (*a..=*b).contains(&j)) {
                return false;
            }
            if let Some(&m) = self.locals.get(s) {
                return m;
            }
            if self.mui_glob {
                return true;
            }
            let mut found = None;
            for m in &self.glob_mods {
                match self.ctx.resolve(m, s, 1) {
                    Some(true) => return true,
                    Some(false) => found = Some(false),
                    None => {}
                }
            }
            return found.unwrap_or(self.own);
        }
        if self.defined.contains(s) {
            return self.own;
        }
        // Inside a mui crate, an imported module (`use crate::widgets;`) is its own.
        if self.own && self.locals.contains_key(s) && s.starts_with(char::is_lowercase) {
            return true;
        }
        if let Some(&m) = self.locals.get(s) {
            return m;
        }
        (self.own && matches!(s, "Self" | "crate" | "self" | "super")) || self.ctx.roots.contains(s)
    }

    fn gate(&self, g: Gate, dot: usize) -> bool {
        match g {
            Gate::Any => true,
            Gate::Mui => self.is_mui,
            Gate::MuiChain => self.is_mui && dot > 0 && matches!(self.t[dot - 1].k, K::Close(')') | K::Close(']')),
        }
    }

    // ---- rules ----

    pub(crate) fn manual(&self) -> Vec<(usize, String)> {
        let mut out = Vec::new();
        if !self.is_mui {
            return out;
        }
        // A `let` of the new name shadows the renamed function as surely as an item does.
        let bound = |n: &str| (0..self.t.len()).any(|i| self.ident(i, "let") && (self.ident(i + 1, n) || (self.ident(i + 1, "mut") && self.ident(i + 2, n))));
        for r in self.ctx.rules() {
            if let Rule::Function { old, new } | Rule::Type { old, new } = *r
                && (self.defined.contains(new) || bound(new))
                // A function only collides where it is called.
                && let fun = matches!(r, Rule::Function { .. })
                && let Some(i) = (0..self.t.len()).find(|&i| self.ident(i, old) && (!fun || self.open(i + 1, '(')) && self.is_mui_name(i))
            {
                out.push((self.line(i), format!("`{old}` becomes `{new}`, which this file also defines or binds: rename the local one")));
            }
        }
        for r in self.ctx.rules() {
            let Rule::Manual { pattern, note } = r else { continue };
            let Ok(pat) = crate::lex::lex_fragment(pattern) else { continue };
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

    pub(crate) fn pass(&self) -> (Vec<Edit>, Vec<(usize, String)>) {
        let t = self.t;
        let mut edits = Vec::new();
        let mut needs: Vec<(usize, &str)> = Vec::new();
        let src = self.src;
        let mut edit = |lo: usize, hi: usize, text: String| edits.push(minimal(src, lo, hi, text));

        for u in &self.uses {
            if !u.mui || u.local.is_none() {
                continue;
            }
            if let Some((from, to)) = self.moved(u) {
                // The item's other entries from the same root: all moving means
                // only the root changes; a mixed list loses this entry to a `use` of its own.
                let all = self
                    .uses
                    .iter()
                    .filter(|v| v.item == u.item && v.path.first().is_some_and(|p| p == from))
                    .all(|v| self.moved(v).is_some());
                if !u.in_brace || all {
                    if let Some(k) = (u.item.0..=u.item.1).find(|&k| self.ident(k, from)) {
                        edit(t[k].lo, t[k].hi, to.to_string());
                    }
                } else if let Some(kw) = (u.item.0..=u.item.1).find(|&k| self.ident(k, "use")) {
                    let at = t[u.item.0].lo;
                    let indent = &src[src[..at].rfind('\n').map_or(0, |n| n + 1)..at];
                    let head = self.text(u.item.0, kw);
                    edit(at, at, format!("{head} {to}::{};\n{indent}", self.text(u.start, u.end)));
                    let (lo, hi) = self.entry_span(u);
                    edit(lo, hi, String::new());
                }
                continue;
            }
            let name = u.path.last().map(String::as_str).unwrap_or("");
            let mut drop = self.ctx.rules().any(|r| matches!(r, Rule::DropImport { name: n } if *n == name));
            for r in self.ctx.rules() {
                if let Rule::Function { old, new } | Rule::Type { old, new } | Rule::Macro { old, new } | Rule::MacroHead { old, new, .. } = *r
                    && old == name
                {
                    if self.locals.contains_key(new) {
                        drop = true; // already imported under the new name
                    } else {
                        // The name token is the last ident before any `as`.
                        let k = (u.start..=u.end).rev().find(|&k| self.ident(k, name)).unwrap_or(u.end);
                        edit(t[k].lo, t[k].hi, new.to_string());
                    }
                }
            }
            if !drop {
                continue;
            }
            let (lo, hi) = self.entry_span(u);
            edit(lo, hi, String::new());
        }

        for i in 0..t.len() {
            if t[i].k != K::Ident || (i > 0 && self.punct(i - 1, '\'')) {
                continue;
            }
            let name = t[i].text.as_str();
            let method = i > 0 && self.dot(i - 1);
            let called = self.open(i + 1, '(') || (self.sep(i + 1) && self.punct(i + 3, '<'));
            let in_use = self.use_tok.contains_key(&i);

            for r in self.ctx.rules() {
                if let Rule::Retype { field: f, from, to } = *r
                    && f == name
                    && self.is_mui
                    && !called
                    && !in_use
                    && let Some(k) = self.value_at(i)
                    && self.ident(k, from)
                    && (self.sep(k + 1) || self.open(k + 1, '{'))
                {
                    edit(t[k].lo, t[k].hi, to.to_string());
                    needs.push((self.line(i), to));
                }
            }

            if method {
                let dot = i - 1;
                let mut done = false;
                for r in self.ctx.rules() {
                    match *r {
                        Rule::Field { recv, name: n, read, write }
                            if n == name && !called && self.is_mui && dot > 0 && t[dot - 1].k == K::Ident && recv.contains(&t[dot - 1].text.as_str()) =>
                        {
                            if self.punct(i + 1, '=') && !t[i + 1].joint {
                                // The assigned expression runs to the statement's end.
                                let mut j = i + 2;
                                while j < t.len() && !matches!(t[j].k, K::Punct(';' | ',') | K::Close(_)) {
                                    if let K::Open(_) = t[j].k {
                                        j = t[j].pair;
                                    }
                                    j += 1;
                                }
                                if j > i + 2 {
                                    edit(t[dot].lo, t[j - 1].hi, expand(write, &[self.text(i + 2, j - 1).to_string()]));
                                }
                            } else if !read.is_empty() && !(self.dot(i + 1) && self.punct(i + 3, '=') && !t[i + 3].joint) {
                                // A write through it (`ui.theme.text = ..`) is left for rustc.
                                edit(t[dot].lo, t[i].hi, read.to_string());
                            }
                            done = true;
                            break;
                        }
                        Rule::Corner { field: f, axis, to }
                            if f == name && !called && self.dot(i + 1) && self.ident(i + 2, axis) && !self.open(i + 3, '(') && self.rect_receiver(dot) =>
                        {
                            edit(t[dot].lo, t[i + 2].hi, format!(".{to}"));
                            done = true;
                            break;
                        }
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
            if in_use {
                continue; // handled per use entry above
            }
            for r in self.ctx.rules() {
                if let Rule::Moved { names, from, to } = *r
                    && from == name
                    && !self.sep_before(i)
                    && self.sep(i + 1)
                    && t.get(i + 3).is_some_and(|n| n.k == K::Ident && names.contains(&n.text.as_str()))
                {
                    edit(t[i].lo, t[i].hi, to.to_string());
                }
            }
            if macro_call {
                for r in self.ctx.rules() {
                    match *r {
                        Rule::Macro { old, new } if old == name && self.is_mui => {
                            edit(t[i].lo, t[i].hi, new.to_string());
                        }
                        Rule::MacroHead { old, new, heads } if old == name && self.is_mui => {
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
                            // The head's top-level comma-separated parts pick the template.
                            let (mut parts, mut a, mut k) = (Vec::new(), open + 1, open + 1);
                            while k < j {
                                match t[k].k {
                                    K::Open(_) => k = t[k].pair + 1,
                                    K::Punct(',') => {
                                        parts.push(self.text(a, k - 1).to_string());
                                        k += 1;
                                        a = k;
                                    }
                                    _ => k += 1,
                                }
                            }
                            parts.push(self.text(a, j - 1).to_string());
                            let Some(head) = heads.get(parts.len() - 1) else { continue };
                            edit(t[i].lo, t[i].hi, new.to_string());
                            edit(t[open + 1].lo, t[j - 1].hi, expand(head, &parts));
                            needs.push((self.line(i), "Weld"));
                        }
                        _ => {}
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
            // An associated-fn shape replaces its type's path, so it runs
            // before (and instead of) a rename of the type.
            let assoc = self.ctx.rules().find_map(|r| {
                let Rule::Assoc { ty, name: n, args, to, bare } = *r else { return None };
                if ty != name || !self.is_mui || field || !self.sep(i + 1) || !self.ident(i + 3, n) || !self.open(i + 4, '(') {
                    return None;
                }
                let mut caps = Vec::new();
                self.match_args(i + 4, args, &mut caps).then_some((to, bare, caps))
            });
            if let Some((to, bare, caps)) = assoc {
                let mut s = i;
                while self.sep_before(s) && s >= 3 && t[s - 3].k == K::Ident {
                    s -= 3;
                }
                let path = if s == i { bare } else { &src[t[s].lo..t[i].lo] };
                edit(t[s].lo, t[t[i + 4].pair].hi, expand(&to.replace("$path", path), &caps));
                continue;
            }

            let mut shaped = false;
            for r in self.ctx.rules() {
                match *r {
                    Rule::Function { old, new } if old == name && called && !field => {
                        if self.is_mui_name(i) {
                            edit(t[i].lo, t[i].hi, new.to_string());
                        }
                    }
                    // The first call shape that matches wins.
                    Rule::FnCall { name: n, args, to, needs: nd } if n == name && !shaped && self.open(i + 1, '(') && !field => {
                        let mut caps = Vec::new();
                        if self.is_mui_name(i) && self.match_args(i + 1, args, &mut caps) {
                            let mut s = i;
                            while self.sep_before(s) && s >= 3 && t[s - 3].k == K::Ident {
                                s -= 3;
                            }
                            let to = to.replace("$path", &src[t[s].lo..t[i].lo]);
                            edit(t[s].lo, t[t[i + 1].pair].hi, expand(&to, &caps));
                            needs.extend(nd.iter().map(|n| (self.line(i), *n)));
                            shaped = true;
                        }
                    }
                    Rule::Type { old, new } if old == name && !field => {
                        if self.is_mui_name(i) {
                            edit(t[i].lo, t[i].hi, new.to_string());
                        }
                    }
                    Rule::Qualify { names, prefix }
                        if names.contains(&name)
                            && !in_use
                            && !field
                            && !self.sep_before(i)
                            && !self.sep(i + 1)
                            && !self.open(i + 1, '{')
                            && self.frame[i] != Frame::Enum
                            && self.mui_glob
                            && !self.foreign_glob
                            && !self.defined.contains(name)
                            && !self.locals.contains_key(name) =>
                    {
                        edit(t[i].lo, t[i].lo, prefix.to_string());
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

    /// The source span that removes use entry `u`: the whole item, or the
    /// entry and one comma beside it in a brace list.
    fn entry_span(&self, u: &UseEntry) -> (usize, usize) {
        let t = self.t;
        if !u.in_brace {
            (t[u.item.0].lo, t[u.item.1].hi)
        } else if self.punct(u.end + 1, ',') {
            (t[u.start].lo, t[u.end + 2].lo)
        } else if self.punct(u.start - 1, ',') {
            (t[u.start - 1].lo, t[u.end].hi)
        } else {
            (t[u.start].lo, t[u.end].hi)
        }
    }

    /// The `Moved` rule use entry `u` falls under: `from::Name`, exactly.
    fn moved(&self, u: &UseEntry) -> Option<(&'static str, &'static str)> {
        self.ctx.rules().find_map(|r| match *r {
            Rule::Moved { names, from, to } if u.path.len() == 2 && u.path[0] == from && names.contains(&u.path[1].as_str()) => Some((from, to)),
            _ => None,
        })
    }

    /// Where the value given to the name at `i` starts: after `name:` (a
    /// struct literal field, not a path), `name =`, or `name ==` / `!=`.
    fn value_at(&self, i: usize) -> Option<usize> {
        let t = self.t;
        if self.punct(i + 1, ':') && !self.sep(i + 1) {
            return Some(i + 2);
        }
        if self.punct(i + 1, '=') && !t[i + 1].joint {
            return Some(i + 2);
        }
        ((self.punct(i + 1, '=') || self.punct(i + 1, '!')) && t[i + 1].joint && self.punct(i + 2, '=')).then_some(i + 3)
    }

    /// Whether the receiver ending just before the dot at `dot` is clearly a
    /// `Rect` (or the `Bounds` it replaced): a call to a function named in
    /// `RECT_FNS`, or a name this file declares `: Rect` / `: &Rect` or binds
    /// to `Rect::..`.
    fn rect_receiver(&self, dot: usize) -> bool {
        const RECT_FNS: &[&str] = &["bounds", "bounding_box"];
        const RECT: &[&str] = &["Rect", "Bounds"];
        let t = self.t;
        let Some(j) = dot.checked_sub(1) else { return false };
        let rect = |k: usize| RECT.iter().any(|r| self.ident(k, r));
        match t[j].k {
            K::Close(')') => {
                let open = t[j].pair;
                open > 0 && t[open - 1].k == K::Ident && RECT_FNS.contains(&t[open - 1].text.as_str())
            }
            K::Ident if !self.dot(j.wrapping_sub(1)) || self.ident(j - 2, "self") => {
                let v = t[j].text.as_str();
                (0..t.len()).any(|k| {
                    self.ident(k, v)
                        && ((self.punct(k + 1, ':') && !self.sep(k + 1) && (rect(k + 2) || (self.punct(k + 2, '&') && rect(k + 3))))
                            || (self.ident(k.wrapping_sub(1), "let") && self.punct(k + 1, '=') && rect(k + 2) && self.sep(k + 3)))
                })
            }
            _ => false,
        }
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
            if !self.match_args(j + 2, pats, &mut caps) {
                return None;
            }
            end = self.t[j + 2].pair;
            j = end + 1;
        }
        Some((end, caps))
    }

    /// Match the args of the group opened at `open` against `pats`,
    /// pushing the captures.
    fn match_args(&self, open: usize, pats: &[Arg], caps: &mut Vec<String>) -> bool {
        let args = self.args(open);
        if args.len() != pats.len() {
            return false;
        }
        for (&(a, b), p) in args.iter().zip(pats) {
            let norm = self.norm(a, b);
            let text = self.text(a, b);
            match p {
                Arg::Any => caps.push(text.to_string()),
                Arg::Is(s) if norm == strip_ws(s) => {}
                Arg::Has(s) if norm.contains(&strip_ws(s)) => caps.push(text.to_string()),
                Arg::After(s) => match strip_prefix_ws(text, s) {
                    Some(rest) => caps.push(rest.to_string()),
                    None => return false,
                },
                Arg::Not(s) if !norm.contains(&strip_ws(s)) => caps.push(text.to_string()),
                Arg::Shared => match strip_prefix_ws(text, "&") {
                    Some(rest) if !rest.strip_prefix("mut").is_some_and(|r| r.starts_with(char::is_whitespace)) => caps.push(rest.to_string()),
                    _ => return false,
                },
                _ => return false,
            }
        }
        true
    }

    /// Tuple-result rewrites for the call whose name is at `i`.
    fn tuple(&self, i: usize, method: bool, edit: &mut impl FnMut(usize, usize, String), needs: &mut Vec<(usize, &'static str)>) {
        let t = self.t;
        if !self.ctx.widgets {
            return;
        }
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
            // A control finished on the spot: `.0.el()` / `.0.into()` is `.el.into_el()`
            // (`Bridge::bind` takes any `IntoEl` now, so `.into()` has no target).
            if *field == "el" && self.dot(close + 3) && (self.ident(close + 4, "el") || self.ident(close + 4, "into")) && self.open(close + 5, '(') && t[close + 5].pair == close + 6 {
                edit(lit.lo, t[close + 6].hi, "el.into_el()".into());
                needs.push((self.line(i), "IntoEl"));
                return;
            }
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
        // One binding and the rest `_`: `let a = f(..).el;`.
        let bound: Vec<_> = items.iter().zip(spec.fields).filter(|&(&(a, b), _)| self.norm(a, b) != "_").collect();
        if let [(&(a, b), field)] = bound[..]
            && (a == b || (b == a + 1 && self.ident(a, "mut")))
        {
            edit(t[open].lo, t[s - 2].hi, self.text(a, b).to_string());
            edit(t[close].hi, t[close].hi, format!(".{field}"));
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

/// `text` without `prefix`, whitespace-insensitively: `"&mut  x"` less `"&mut"` is `"x"`.
fn strip_prefix_ws<'s>(text: &'s str, prefix: &str) -> Option<&'s str> {
    let mut want = prefix.chars().filter(|c| !c.is_whitespace()).peekable();
    for (i, c) in text.char_indices() {
        if want.peek().is_none() {
            return Some(text[i..].trim_start());
        }
        if c.is_whitespace() {
            continue;
        }
        if want.next() != Some(c) {
            return None;
        }
    }
    want.peek().is_none().then_some("")
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

# MUI string, path, ID, and text audit

## Scope and result

I audited the TypeScript-to-Rust compiler and the Rust ownership, ID, path,
input, preview, and text layers. The main correctness issue is at the codegen
boundary: `JSON.stringify` emits JSON escapes, but the generated source is Rust,
so some valid TypeScript strings produce invalid Rust. The numeric emitter has a
separate large-exponent failure that overlaps the compiler-focused audit. The
`mui-text` shaping behavior is a documented scope boundary rather than an
implementation bug; it is appropriate for the current simple preview contract,
but insufficient for localized display text.

The inspected spans were `packages/mui-ts/src/compiler.ts:1-64`,
`packages/mui-ts/src/index.ts:1-67`, `crates/mui-core/src/scene.rs:51-97,245-292`,
`crates/mui-layout/src/lib.rs:163-167,359-405,664-684`,
`crates/mui-input/src/lib.rs:32-80,201-220`,
`crates/mui-preview/src/main.rs:57-115,255-314,355-359`,
`crates/mui-preview/src/ui.rs:35-80,250-303`,
`crates/mui-preview/src/scenes.rs:154-196,237-263`, and
`crates/mui-text/src/lib.rs:1-11,50-79,130-213,494-538`. I also inspected the
generated TypeScript output in `/tmp/mui-pattern-audit/ts-dist` and retained the
minimal reproductions in `/tmp/mui-pattern-audit/q-case.mjs`,
`q-case.rs`, `n-case.mjs`, and `n-case.rs`. This audit made no source changes.

## Findings

### P2/medium: JSON string escaping emits invalid Rust for uncommon controls

**Location:** `packages/mui-ts/src/compiler.ts:10,24,47,59-62`; accepted
string fields are declared in `packages/mui-ts/src/index.ts:5-15,57-60`.

**Trigger:** A scene ID, parent ID, or surface ID contains a control character
that JSON permits, for example `"unit\\u0001"`, backspace, form feed, or NUL.
The compiler's `q` helper is `JSON.stringify`, and its result is inserted
directly into Rust source. For the input ID `unit\u0001`, the output contains
`"unit\\u0001"`. `rustfmt --edition 2021 --check` and `rustc` both reject this
with “incorrect unicode escape sequence”; Rust's arbitrary code-point form is
`\\u{0001}`. The same failure occurs for JSON's `\\b`, `\\f`, and `\\u0000`.
JSON escapes for newline, carriage return, and tab happen to be accepted by
Rust, which is why ordinary multiline-like control data can hide this bug.

**Impact:** Code generation fails after accepting a valid JavaScript string when
an ID or reference contains one of these uncommon controls. The failure affects
all emitted string positions using `q`, including node IDs, parent references,
and frame/merge/inset/outset surface IDs. It can block that build before the
Rust resolver gets a chance to apply its normal empty and duplicate-ID checks,
but ordinary identifiers are unaffected.

**Minimal recommendation:** Replace `q` with a Rust-string emitter that escapes
backslash and quote, uses Rust's short escapes only where supported, and emits
`\\u{...}` for other control code points. Add a generated-source test covering
newline, tab, backspace, form feed, NUL, U+0001, quotes, backslashes, non-ASCII
text, and an empty string; compile or rustfmt the generated result. If the
product forbids controls in identifiers, rejecting them with a precise
validation error is also sound, but it should be an explicit API rule rather
than an accidental compiler failure.

**Confidence:** High. The failure was reproduced against the generated output
with both `rustfmt` and `rustc`; severity is medium because the trigger is a
valid but unusual identifier payload.

### P2/high (compiler-audit overlap): integer-valued numbers can be emitted with
an invalid `.0` suffix

**Location:** `packages/mui-ts/src/compiler.ts:11-14`.

**Trigger:** `n` appends `.0` to every JavaScript integer. A finite value such as
`1e21` is an integer according to `Number.isInteger`, while `String(1e21)` is
`1e+21`, so the generated token becomes `1e+21.0`.

**Impact:** Rust parses this as an invalid field access on a floating-point
literal and reports E0610 (`{float} primitive therefore doesn't have fields`).
This affects otherwise finite numeric scene dimensions, radii, or coordinates
when callers pass sufficiently large values.

**Minimal recommendation:** Decide the target numeric contract first, then emit
a Rust literal from a canonical formatter. At minimum, append `.0` only when
the decimal representation contains neither `e` nor `E`; test ordinary integer,
fractional, negative, zero, and large-exponent values. Independently validate
the permitted geometry range so values that compile still have meaningful UI
behavior.

**Confidence:** High. The minimal `n-case` generated `mui_layout::leaf(1e+21.0,
1.0)`, and `rustc` reproduced E0610 (alongside expected unresolved-crate errors
from compiling the isolated snippet). This finding overlaps the compiler-focused
audit and should be deduplicated there.

### Documented scope limit: `mui-text` does not shape, reorder, or substitute

**Location:** `crates/mui-text/src/lib.rs:149-166,167-213`.

`text_run` accepts a Rust `&str`, so its input is valid UTF-8 and its `chars()`
iteration stays on Unicode scalar-value boundaries. It maps each scalar to one
font glyph, draws its outline, and advances by that glyph's nominal advance.
The module documentation explicitly says this is “advance-only positioning”:
there is no shaping, kerning, ligature substitution, combining-mark placement,
bidirectional reordering, or script/language shaping. Missing characters use
`.notdef`; spaces contribute advance without an outline. Axis tags are owned as
strings, unknown tags are ignored, and values are clamped as documented at
`50-79`.

This is not a hidden UTF-8 corruption bug or a P2 defect under the current
contract. It is a clear scope boundary for drawing simple UI labels and glyph
previews, and the tests at `494-515` and `531-538` exercise ordinary side-by-
side glyphs and missing-glyph fallback. It would become a product correctness
gap only if user-facing internationalized or display text were promised:
Arabic/Hebrew direction and joining, Devanagari-style reordering, combining
marks, kerning, ligatures, emoji sequences, and variation-selector choices
cannot be represented correctly by one scalar-to-glyph lookup. The single-
character `glyph_path` preview and `char_field` are likewise intentional
preview controls, not general grapheme editors.

**Minimal recommendation:** Preserve this documented contract for the current
glyph/geometry preview. If the preview or product takes arbitrary localized
text, add a real shaping boundary that accepts text, direction, script,
language, and font variation data, then convert shaped glyph IDs and positions
to geometry. Do not try to “fix” shaping by changing `chars()` or by manually
special-casing variation selectors. **Confidence:** High for the absence of a
shaper and medium for the visual result of every particular font/selector pair.

### Documented scope limit: preview text budgeting is approximate

**Location:** `crates/mui-preview/src/ui.rs:35-41,43-74,250-268`.

The preview estimates capacity as `width / (size * 0.62)` and truncates/wraps
using `chars()`. The comments openly describe this as an estimate for the
Hack-like UI font, and the implementation never slices inside a UTF-8 byte
sequence. That is a reasonable small-preview policy for the current font.

For CJK, wide emoji, combining sequences, and right-to-left text, the estimate
can leave overflow, truncate a grapheme sequence, or produce a visually poor
line boundary. This is an acknowledged scope limitation, not a P2 defect in
the current preview absent a supported promise of localized or user-authored
text, and it is not evidence of memory unsafety or malformed UTF-8.
**Recommendation:** if labels become localized or user-authored, shape and
measure candidate runs using actual advances, and truncate on grapheme
boundaries with an explicit bidi policy. **Confidence:** Medium; the estimate
is intentionally approximate and its impact depends on font metrics and the
caller's text.

## Positive patterns and why they are correct

IDs are owned at storage boundaries. `Node::id` accepts `impl Into<String>` at
`crates/mui-layout/src/lib.rs:163-167`; `SurfaceSpec` constructors do the same
at `crates/mui-core/src/scene.rs:58-97`. Read-only layout code borrows IDs as
`&str` while checking duplicates at `389-405`, then allocates only for an error;
validation rejects empty IDs at `359-371`. Core resolver construction rejects
empty and duplicate surface IDs at `245-259`. This gives callers ergonomic
`&str`/`String` inputs while making stored identity independent of the caller's
buffer and avoiding repeated read-path clones.

The hit-test layer follows the same ownership rule: `Hit` owns each target ID
as a `String` and path at `crates/mui-input/src/lib.rs:32-60`, while lookup
returns a borrowed `&str` and scans reverse paint order at `67-80`. Unknown
interaction IDs are inert at `201-220`. Preview scopes include the selected
scene and ordinal (`crates/mui-preview/src/ui.rs:305-314` and
`crates/mui-preview/src/main.rs:355-359`), which prevents stale controls from
colliding across scenes. `Baked::build` pushes paths and hit targets in the same
authored/overlay order (`main.rs:73-115`), matching the documented topmost-hit
rule.

Font path handling is also sound. `crates/mui-preview/src/scenes.rs:170-187`
reads `MUI_PREVIEW_FONT` with `var_os` and passes the `OsString` path directly
to `fs::read`, preserving paths that are not valid UTF-8. It uses
`to_string_lossy` only for a human-facing source label. The UI stores font
bytes (`crates/mui-preview/src/ui.rs:76-80`), so text rendering does not retain
or reopen a filesystem path.

## Verification and limits

`cargo test -p mui-text` passed 13 tests; `cargo test -p mui-layout -p mui-core
-p mui-input` passed the crate and documentation tests (mui-core 24, mui-input
15, mui-layout 12, with their docs). I did not run `cargo test -p mui-preview`
or the full workspace. No source files were modified. The q-case and n-case
reproductions remain under `/tmp/mui-pattern-audit` for parent-agent review.

🌱 graft saved approximately 407,277 tokens this turn across 32 successful
calls. This tally covers the graph queries used before source inspection; the
one invalid exploratory regex query is excluded.

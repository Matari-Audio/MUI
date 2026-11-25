---
status: in-progress
project: mui
owner: derpcat
created: 2026-11-24
updated: 2026-11-24
proof_scope: external-api
evidence_freshness_days: 180
---

# MUI Style 4-Level WCAG-Checked Text Ink Ramp

## Problem & Goal

- **Problem:** `mui-style` previously exposed only two AA-floored ink helpers (`Palette::on` and `Palette::dim`) with no typed 4-level text emphasis hierarchy for primary, secondary, tertiary, and disabled copy. Applications building dense interfaces had to maintain private text-contrast ramps or hand-typed mix factors outside `mui-style`.
- **Goal:** Provide a generic, WCAG-checked 4-level text ink ramp (`InkLevel`, `InkRamp`, `Palette::ink_for`, `Palette::ink_ramp`, `Color::max_contrast`, and `Palette::DISABLED_TEXT`) in `mui-style` while keeping existing `Palette::on` / `Palette::dim` and `Role::Ink` / `Role::Dim` AA semantics unchanged so no downstream visuals or tests regress, and prove contrast compliance across dark, light, custom-ground, extreme-layer, chromatic, and seeded palettes.
- **Plan Reference:** MUI style system contrast guarantees (`frameworks/mui/crates/mui-style`).

## Primary-Source & Version Proof

| Standard / Vendor API | Pinned Version / Spec Rev | Primary-Source URL | Verified Date | Key Invariants / Deprecations Checked |
|---|---|---|---|---|
| W3C WCAG 2.1 Contrast (SC 1.4.3, SC 1.4.6, SC 1.4.11) | W3C Recommendation (WCAG 2.1) | https://www.w3.org/TR/WCAG21/ | 2026-11-24 | Normal text requires >= 4.5:1 (AA, SC 1.4.3) and >= 7.0:1 (AAA, SC 1.4.6); inactive/disabled UI components are explicitly exempt from SC 1.4.3 minimum contrast; worst-case opaque relative luminance `Y = sqrt(1.05 * 0.05) - 0.05 ≈ 0.179` has physical ceiling `sqrt(21) ≈ 4.58:1` |
| W3C CSS Color Module Level 4 (Oklch & Gamut Mapping §13.2) | W3C Candidate Recommendation (CSS Color 4) | https://www.w3.org/TR/css-color-4/ | 2026-11-24 | Perceptual lightness interpolation in Oklch with chroma-bisection gamut mapping to sRGB preserves hue and guarantees `[0, 0, 0]` at `L <= 0` and `[1, 1, 1]` at `L >= 1` |

## Pinned Crate & SDK Ground Truth

| Dependency / Tool | Pinned Version | Lockfile / Header Source | Verified Symbols / Features |
|---|---|---|---|
| `mui-style` | `0.4.2` | `frameworks/mui/Cargo.lock` | `Color`, `Mode`, `Pigment`, `Palette`, `Role`, `Fill`, `InkLevel`, `InkRamp` |
| `color` | `0.3.3` | `frameworks/mui/Cargo.lock` | `AlphaColor`, `ColorSpace`, `Oklab`, `Oklch`, `Srgb`, `relative_luminance` |
| `kurbo` | `0.13.1` | `frameworks/mui/Cargo.lock` | Shared vector geometry primitives |
| `peniko` | `0.6.1` | `frameworks/mui/Cargo.lock` | Shared fill/paint primitives |
| `mui-geometry` | `0.4.2` | `frameworks/mui/Cargo.lock` | `CornerStyle` |
| `mui-layout` | `0.4.2` | `frameworks/mui/Cargo.lock` | `Px`, `Spacing`, `SpacingScale` |
| `mui-text` | `0.4.2` | `frameworks/mui/Cargo.lock` | `Font` |

## Invariants & Non-Goals

### Hard Invariants

- **Backward-Compatible `Palette::on` / `Palette::dim` / `Role`:** `Palette::on(bg)` and `Palette::dim(bg)` (and therefore `Role::Ink` and `Role::Dim`) preserve their existing WCAG AA (`4.5:1`) contrast and visual behavior verbatim so existing `mui-scene`, `mui-material`, `mui`, and plugin callers experience zero visual regressions. `pub enum Role` is unchanged (zero semver break).
- **WCAG 2.1 Contrast Compliance (Exact, Opaque Only):**
  - `InkLevel::Primary`: `>= Palette::AAA_TEXT` (`7.0:1`) on every named surface (`field`, `background`, `surface`, `raised`) at the mode's default ground; on custom grounds near `Mode::limit()` (e.g. `with_ground(0.5)` in Dark mode where white reaches `~6.0:1`) or mid-tone chromatic backdrops, `Primary` saturates cleanly at exact `bg.max_contrast()` (`>= sqrt(21) ≈ 4.58:1 > 4.5:1`).
  - `InkLevel::Secondary`: `>= Palette::AA_TEXT` (`4.5:1`) on every opaque sRGB surface.
  - `InkLevel::Tertiary`: `>= Palette::AA_TEXT` (`4.5:1`) on every opaque sRGB surface, so normal-size quiet text (captions, hints, units) remains WCAG 2.1 SC 1.4.3 AA compliant. On mid-tone backdrops whose `max_contrast()` sits near `4.5:1`, `Secondary` and `Tertiary` intentionally converge to the `4.5:1` floor because AA legibility takes precedence over visual dimming.
  - `InkLevel::Disabled`: `>= Palette::DISABLED_TEXT` (`1.8:1`) on every opaque sRGB surface (WCAG 2.1 SC 1.4.3 inactive-control exemption with an ergonomic visibility floor).
  - Every colour returned by `Palette::ink_for` and `Palette::ink_ramp` is opaque (`alpha == 1.0`); no `.alpha()` helper exists on `InkLevel` or `InkRamp` that could dilute verified ink below its floor.
- **Monotonicity, Polarity Consistency & Performance:** `Palette::ink_ramp(bg)` computes `bg.luminance()` and the white/black polarity (`max_contrast_for_lum`) once for all four levels. On every surface `bg`, all four levels share one polarity and satisfy exact `contrast(primary) >= contrast(secondary) >= contrast(tertiary) >= contrast(disabled)`, with strict ordering and `> 0.05` Oklch lightness separation on all default-ground named surfaces.
- **Mirror Safety:** All code, docs, and tests in `frameworks/mui` remain 100% generic and free of private paths or application-specific types.

### Non-Goals

- Altering the visual output or contrast floor of `Palette::on`, `Palette::dim`, `Role::Ink`, or `Role::Dim`.
- Adding new variants to `Role` without `#[non_exhaustive]` or breaking downstream exhaustive matches.
- Introducing translucent ink contrast compositing into `mui-style` (compositing remains the scene resolver's responsibility).

## Acceptance Criteria

- [x] `Color::max_contrast` computes the physical contrast ceiling (black vs white) via `max_contrast_for_lum` and shares that polarity helper with `Palette::ink_for` and `Palette::ink_ramp`.
- [x] `InkLevel` (`Primary`, `Secondary`, `Tertiary`, `Disabled`) and `InkRamp` (`primary`, `secondary`, `tertiary`, `disabled`) are exported from `mui-style` with `Palette::ink_ramp` as the single ramp constructor.
- [x] `Palette::on`, `Palette::dim`, `Role::Ink`, and `Role::Dim` remain unchanged at WCAG AA (`4.5:1`) with zero downstream regressions.
- [x] Unit tests assert exact (`ratio >= floor`, zero tolerance) contrast, monotonic ordering, lightness separation, polarity consistency, and `sqrt(21)` mid-tone ceiling saturation across dark, light, custom-ground, extreme-layer, chromatic, and seeded palettes.
- [x] `mui-style`, `mui-scene`, `mui-material`, `mui`, and downstream plugin tests pass cleanly alongside `clippy -D warnings`, `rustfmt --check`, `check-spec-proof.py`, and `sync-stack.py`.

## API / Data / File Contract

- **Public Types & Constants (`mui_style`):**
  - `Color::max_contrast(self) -> f32`
  - `Palette::DISABLED_TEXT: f32 = 1.8`
  - `Palette::ink_for(&self, level: InkLevel, bg: Color) -> Color`
  - `Palette::ink_ramp(&self, bg: Color) -> InkRamp`
  - `InkLevel::{Primary, Secondary, Tertiary, Disabled}`, `InkLevel::ALL`, `InkLevel::min_contrast(self) -> f32`
  - `InkRamp { pub primary: Color, pub secondary: Color, pub tertiary: Color, pub disabled: Color }`, `InkRamp::get(self, InkLevel) -> Color`, `InkRamp::levels(self) -> [Color; 4]`, `InkRamp::is_valid(self) -> bool`

## Verification Matrix

| Check | Tier | Target / OS | Exact Command | Pass Criterion | Status / Observed Evidence |
|---|---|---|---|---|---|
| Spec proof gate | Doc | any | `python3 tools/check-spec-proof.py frameworks/mui/docs/specs/mui-style-ink-ramp.md` | Exit 0, zero proof errors | PASS (2026-11-24, linux-x86_64) |
| Scoped `mui-style` nextest & lib tests | Compile/ABI | linux-x86_64 | `matari fast mui -p mui-style && build-slot cargo test --manifest-path frameworks/mui/Cargo.toml -p mui-style --lib` | Exit 0, all unit tests pass | PASS (2026-11-24, linux-x86_64) |
| `mui-style` doctests | Compile/ABI | linux-x86_64 | `build-slot cargo test --manifest-path frameworks/mui/Cargo.toml -p mui-style --doc` | Exit 0, all doctests pass | PASS (2026-11-24, linux-x86_64) |
| `mui-style` clippy & rustfmt | Validator/Runtime | linux-x86_64 | `build-slot cargo clippy --manifest-path frameworks/mui/Cargo.toml -p mui-style --all-targets -- -D warnings && cargo fmt --manifest-path frameworks/mui/Cargo.toml -p mui-style --check` | Exit 0, zero warnings or fmt diffs | PASS (2026-11-24, linux-x86_64) |
| Downstream MUI crates tests (`mui-scene`, `mui-material`, `mui`) | Validator/Runtime | linux-x86_64 | `build-slot cargo nextest run --manifest-path frameworks/mui/Cargo.toml -p mui-scene -p mui-material -p mui --no-tests=pass` | Exit 0, zero failures | PASS (2026-11-24, linux-x86_64) |
| Downstream plugin tests (first) | Validator/Runtime | linux-x86_64 | `matari fast` on the dependent plugins | Exit 0, zero failures | PASS (2026-11-24, linux-x86_64) |
| Downstream plugin tests | Validator/Runtime | linux-x86_64 | `matari fast` on the dependent plugins | Exit 0, zero failures | PASS (2026-11-24, linux-x86_64) |
| Stack sync & mirror hygiene | Validator/Runtime | linux-x86_64 | `python3 tools/sync-stack.py --check --skip-lock-refresh` | Exit 0, all projects & mirrors clean | PASS (2026-11-24, linux-x86_64) |

## Unverified Boundaries & Promotion Gate

None — all verification tiers execute on the current host.

**Hard Promotion Rules:**
1. This spec **MUST NOT** move to `status: implemented` while any required row in `## Verification Matrix` is `PENDING`, `PENDING_OS_CI`, or `BLOCKED`, or while any unchecked `- [ ]` acceptance criterion remains.
2. Any future `Role` expansion must be specified and migrated across callers in its own scoped PR.

## Task Decomposition

| # | Subtask | Owned Paths | Depends On | Verification |
|---|---|---|---|---|
| 1 | Author `mui-style` 4-level WCAG ink ramp spec | `frameworks/mui/docs/specs/mui-style-ink-ramp.md` | — | `python3 tools/check-spec-proof.py frameworks/mui/docs/specs/mui-style-ink-ramp.md` |
| 2 | Implement `InkLevel`, `InkRamp`, `Color::max_contrast`, and `Palette::{ink_for,ink_ramp}` + contrast tests | `frameworks/mui/crates/mui-style/**` | 1 | `matari fast mui -p mui-style && build-slot cargo nextest run --manifest-path frameworks/mui/Cargo.toml -p mui-scene -p mui-material -p mui --no-tests=pass` |

## Rollout & Affected Dependents

- **Affected Projects:** `frameworks/mui` (`mui-style`, plus downstream tests run across `mui-scene`, `mui-material`, `mui`, and the dependent plugins). Existing `Role::Ink`, `Role::Dim`, `Palette::on`, and `Palette::dim` callers keep identical AA contrast and visual output.
- **Mirror Impact:** Copybara mirror (`frameworks/mui` -> `Matari-Audio/MUI`); spec and implementation contain zero private paths or downstream-specific types.
- **Versioning & Changelog:** Additive, backward-compatible minor enhancement to `mui-style` (`0.4.2`).

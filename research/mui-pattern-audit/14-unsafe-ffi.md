# MUI unsafe, FFI, and GPU-boundary audit

## Scope and method

This is a source audit of the MUI workspace, using the unsafe/FFI guidance
linked from [`mre/idiomatic-rust`](https://github.com/mre/idiomatic-rust), the
[Rust Reference](https://doc.rust-lang.org/reference/), the
[Rustonomicon FFI chapter](https://doc.rust-lang.org/nomicon/ffi.html), and
the current [wgpu surface documentation](https://docs.rs/wgpu/latest/wgpu/struct.Surface.html).
The graph covers 37 indexed files. Exhaustive `graft grep` searches covered
`unsafe`, bytemuck, raw-pointer operations, `from_raw`, `as_ptr`, `transmute`,
`repr(C)`, shader/WGSL names, `Mutex`, `RwLock`, `static mut`, and GPU calls;
the final repository-wide `rg` pass excluded only `.git` and `target`.

There is no MUI unsafe block, unsafe trait implementation, FFI declaration,
raw pointer conversion, manual byte cast, `repr(C)` type, or local shader
source. Every reusable crate and the preview binary has
`#![forbid(unsafe_code)]` (for example `crates/mui-preview/src/main.rs:17`,
`crates/mui-vello/src/lib.rs:12`, and `crates/mui-tessellate/src/lib.rs:2`).
Those attributes are coverage controls, not a positive unsafe finding. The
lockfile contains transitive `bytemuck` and WGSL packages, but no MUI crate
uses them directly. No UB was executed and no application source was edited.

The relevant call flow is single-threaded and typed: `resumed` constructs
`Gpu` with an `Arc<Window>` and event-loop-owned display handle
(`crates/mui-preview/src/main.rs:429-441`); resize events call `Gpu::resize`
(`main.rs:443-450`); redraw calls the scene/render path
(`main.rs:481-504`, `host.rs:139-209`). `mui-vello` consumes Vello's typed
`RenderTargetConfig`, `RenderSize`, view, and `TextureBindings`; it does not
own a wgpu device or perform a byte-level GPU upload
(`crates/mui-vello/Cargo.toml:7-14`, `crates/mui-preview/src/host.rs:90-107`).

## Defensive observation — capability indexing is contract-dependent, not a confirmed defect

`crates/mui-preview/src/host.rs:72-87` calls
`surface.get_capabilities(&adapter)` and indexes `caps.formats[0]` at line 77.
At first glance this looks like a Low availability issue because the wgpu
[`SurfaceCapabilities::formats` documentation](https://docs.rs/wgpu/29.0.0/wgpu/struct.SurfaceCapabilities.html#structfield.formats)
says the vector is empty when the surface is incompatible with the adapter.
The stronger companion contract matters here: `RequestAdapterOptions::compatible_surface`
is a surface “required to be presentable with the requested adapter”
([wgpu docs](https://docs.rs/wgpu/29.0.0/wgpu/type.RequestAdapterOptions.html)),
and `request_adapter` returns an error if no adapter satisfies its hard
options. Therefore, after the successful request at `host.rs:58-64`, an empty
format list should be unreachable on a conforming backend. It would require a
backend/driver contract failure or a capability change that invalidates the
adapter result, neither of which is established by this repository.

This is consequently a defensive observation, not a confirmed MUI defect.
The code could still be hardened for a reusable host by using
`caps.formats.first().copied()` and returning a typed `NoSurfaceFormat` error;
`Surface::get_default_config` already follows this style and returns `None`
when the surface is unsupported ([wgpu docs](https://docs.rs/wgpu/29.0.0/wgpu/struct.Surface.html#method.get_default_config)).
The preview's nearby `expect` calls at lines 57, 64, 68, and 86 likewise make
backend/device initialization fail fast by policy. They are availability and
diagnostic choices, not memory-safety findings. Confidence that the index is
defensively worth checking is High; confidence that it is reachable in a
correct wgpu implementation is Low.

## Boundary review: no local FFI or shader-layout defect found

The preview host deliberately retains the owning `Arc<Window>` next to
`wgpu::Surface<'static>` (`host.rs:27-35`). It creates the surface through
`SurfaceTarget::from_window_without_display(window.clone())`
(`host.rs:50-57`) while passing the event-loop display handle exactly once.
That is a good ownership pattern: the safe wgpu constructor gets an owned
window handle and the `Gpu` value keeps the owner alive. A bad pattern would
manufacture a `'static` surface from a borrowed window, call a raw
`from_raw`/`transmute`, or keep a foreign display pointer after its owner is
dropped. MUI does none of these, so there is no provenance or lifetime proof
left for a local unsafe wrapper.

The host also maintains a useful layout invariant. `target_size` rejects zero
dimensions and clamps each dimension to `u16::MAX` (`host.rs:23-25`), which
proves the casts at scene creation (`host.rs:98-100`) and resize
(`host.rs:126-137`) cannot truncate a larger value. Resize updates the surface
configuration and Vello scene together. `present` uses the same configured
format for the texture view and passes the same width/height to Vello
(`host.rs:180-198`). This is the correct pattern for this API: validate once at
the boundary, then carry one typed source of truth through the renderer. A bad
pattern would cast arbitrary window dimensions to `u16`, or let the surface,
scene, and render target retain independent dimensions.

The color-format choice is also explicit (`host.rs:73-83`): it removes the
sRGB suffix only when the linear sibling is advertised, then uses the selected
format for both `SurfaceConfiguration` and `RenderTargetConfig`. That avoids a
silent double-encoding mismatch. It is a semantic color contract, not a memory
layout proof; the actual shader pipeline is in `vello_hybrid`, outside this
repository. MUI contains no `.wgsl`, SPIR-V, vertex-layout, or uniform-buffer
source to inspect. The dependency feature declarations are visible in
`crates/mui-preview/Cargo.toml:17-22` and `crates/mui-vello/Cargo.toml:8-14,22-24`.
Any future direct shader or buffer addition should keep the layout contract in
one small module with `repr(C)`, explicit alignment/size checks, and a documented
Pod/byte-cast proof; only use that pattern when the GPU ABI actually requires
raw bytes. It should not be introduced merely for a perceived performance win.

## Safe data and thread patterns observed

`mui-vello::bez_path` validates the incoming path and tolerance before handing
typed curves to Vello (`crates/mui-vello/src/lib.rs:22-56`). The tessellation
adapter bounds flattening, converts to `f32`, rejects non-finite results, and
constructs a typed Lyon buffer (`crates/mui-tessellate/src/lib.rs:48-94`).
These checks are the right replacement for an unsafe GPU upload: reject invalid
domain data before a renderer boundary rather than fabricate a slice or cast a
buffer. A bad pattern would use `bytemuck::cast_slice` on a struct whose
padding, alignment, initialization, or shader layout had not been proved, or
would use `slice::from_raw_parts` on an arbitrary pointer/length pair. There is
no such code in MUI.

`mui-egui::paint` builds egui vertices from the typed `TriangleMesh` and clones
indices (`crates/mui-egui/src/lib.rs:134-162`). Its public mesh fields mean a
future caller could supply out-of-range indices; egui remains a safe API, so
this is at most a dependency-defined validation/panic concern, not UB. If the
adapter becomes a public untrusted-input boundary, validate that indices are
triangle-aligned and below `positions.len()` before constructing the egui
mesh, returning an error instead of relying on backend behavior.

Thread ownership is intentionally simple. The event loop owns `Gpu`; there are
no `unsafe impl Send/Sync`, global mutable GPU handles, locks, callback
contexts, or cross-thread raw pointers. Geometry has a compile-time
`Send + Sync` check in `crates/mui-geometry/src/tests.rs:522-527`, while UI
state documents its single-thread transactional model. This is preferable to
adding `Arc<Mutex<Gpu>>` or unsafe marker implementations when winit already
serializes the access. A different design would be justified only if a host
actually moved the device across threads and could prove the wgpu backend's
thread and destruction requirements.

## Coverage and unresolved semantics

No MUI FFI boundary exists, so C ABI representation, allocator pairing,
callback lifetime, pointer provenance, unwinding, and `Send`/`Sync` proofs are
not applicable to current application code. Those remain obligations of the
transitive wgpu/Vello dependencies and platform backends. The WGSL and
bytemuck entries in `Cargo.lock` are evidence of dependency machinery, not a
local cast site. If MUI adds an FFI or shader module, review it against the
[Rust Reference undefined-behavior rules](https://doc.rust-lang.org/reference/behavior-considered-undefined.html),
[`slice::from_raw_parts`](https://doc.rust-lang.org/std/slice/fn.from_raw_parts.html),
and the [Rustonomicon's FFI guidance](https://doc.rust-lang.org/nomicon/ffi.html);
document caller obligations for arbitrary pointers and keep every unsafe block
next to its checked proof. No assumptions about currently evolving pointer
provenance or foreign unwinding semantics should be promoted to a stable MUI
guarantee.

**Graft tally:** graph queries saved approximately **668,341 tokens** this
turn. Audit result: no confirmed availability or memory-safety finding, one
defensive capability check, and no local unsafe/FFI/shader-casting defect.

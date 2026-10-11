# MUI image isolation patch

This is AccessKit `accesskit_macos` 0.26.3 from revision
[c88605b96d04431f9c3c792464a0f2f253480e94](https://github.com/AccessKit/accesskit/tree/c88605b96d04431f9c3c792464a0f2f253480e94/platforms/macos).
Its production public API and package version are unchanged. All three owned
Objective-C class families (node, associated object, view subclass) get a namespace
reserved by a callback-free NSObject anchor. A private non-ZST atomic's address
identifies a loaded Rust image; generation retries avoid reusing classes after an
unloaded image's address is recycled. Rust-bearing classes from other images are
never reused. The associated-object key uses the same private marker.

`class_macro.rs`, `class_ivars.rs`, and the registration portions of `class.rs`
are a restricted private adaptation of [objc2 0.5.2](https://github.com/madsmtm/objc2/tree/objc2-0.5.2/crates/objc2/src):
`macros/declare_class.rs`, `__macro_helpers/declared_ivars.rs`, `runtime/declare.rs`, and
`runtime/mod.rs`'s byte-offset ivar access. Original copyright notices remain;
`LICENSE-OBJC2` is the [license at that exact release tag](https://github.com/madsmtm/objc2/blob/objc2-0.5.2/LICENSE.txt).

The layout, byte sizes, alignment, opaque ivar encoding, cached offsets, drop
flags (0x00/0x0f/0xff), class-before-ivar destruction, and final superclass dealloc
ordering follow that version. Original objc2 initialization routines still write
the flags. Deliberate adaptations: runtime class names; public FFI instead of
private builder fields; public selector/superclass signature checks; raw-pointer
writes to the three Once-protected statics for Rust 2024; Apple ivar names only
(no GNUstep branch). The helper only supports the inherent instance methods
used here; unused protocol and class-method builder APIs are omitted.

`objc2` is pinned to `=0.5.2`. Updating it, changing the copied helper, or adding
macro uses requires review of the exact layout/initialization/deallocation
contract and the native test below. Do not silently substitute a different
objc2 implementation. The `image-probe` feature exists only for that fixture,
with no default-feature or production API change.

Native regression: `python3 tools/ci/check-macos-accesskit-images.py` in the MUI
repository. Two independently linked libraries run in one process, exercise all
three provider class families and actual accessibility activation/actions,
updates and destruction, and check uninitialized, partial and full helper
initialization. It also reserves a second namespace at the same marker address
to test the reload/address-reuse guard.

AccessKit's original MIT/Apache/Chromium notices are preserved alongside this file.

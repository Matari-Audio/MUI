# MUI controls and documents for Truce

Truce is the plugin framework. This crate adds a renderer-independent control
adapter and persistent editor document; it does not implement a competing host
wrapper, parameter store, audio runtime or state format.

```rust
use mui_truce::Document;
use truce::prelude::*;

#[derive(Params)]
pub struct PluginParams {
    #[param(id = 10, name = "Gain", range = "linear(0, 1)", default = 0.5)]
    pub gain: FloatParam,
    #[persist]
    pub editor: Document,
}
```

Keep each parameter's explicit Truce ID unchanged after release. Display order
and labels may change without changing IDs. Declare the plugin's fixed host
parameter slots in Truce; removing a module does not free its automation IDs for
an unrelated module. Document module/route IDs increase monotonically and are
never inferred from row indexes. Register a module's parameter list with these
same Truce IDs.

`Parameter::new(params, id, modulatable, edits)` reads Truce's `ParamInfo` directly.
Use `begin` / `drag` / `end` for pointer gestures, `step` for keyboard edits,
`reset` for defaults, `cancel` to restore the initial value, and `set_enabled`
when a control becomes disabled. `text` uses Truce formatting; `parse` uses the
plugin's Truce `#[param(parse = "...")]` hook and returns false when no parser is
provided. Quantization follows Truce's declared range. GPUI continues to own
focus, hit testing, pointer capture, text and accessibility; the view binds those
events to this shared controller.

Drain `Edit` messages into `Automation::dispatch` from the editor's host thread.
Call `Automation::close` after draining the closing worker: it balances every
open parameter gesture, including worker failure. Duplicate begin/end and
non-finite values are ignored. The internal `modulatable` capability is separate
from Truce's host-modulation flags: do not advertise host modulation until DSP
consumes it. This crate supplies the capability; the existing GPUI pie/routing
component supplies its appearance and hit handling.

`Document` belongs to each plugin's `Arc<Params>`, through `#[persist]`. Parameter
values remain in Truce atomics, not a duplicated document map. Document state
contains module identities/enabled states, routes/parent routes, preset name and
an OKLCH theme seed. `edit` validates a copy before publishing; `revision` lets an
open editor detect host recall. `snapshot` and `edit` are UI/host-thread methods,
not audio-callback methods. DSP should consume prepared snapshots separately.

The version-1 document accepts at most 256 modules and 4096 routes. It rejects
missing references, duplicate IDs/connections, non-finite depths/colors, parent
cycles and modulation feedback. A source cannot parent its own route. Deleting
a module removes dependent routes and orphaned parent routes transitively.
Unknown versions/malformed documents leave the previous document untouched.
Truce's `PersistField` has no error return: rejection preserves the document but
does not make the enclosing host state-load callback report failure. This is not
a transaction across Truce parameter values and the document.

The GPUI probe consumes the shared controller, gesture dispatcher and persisted
preset name. Its host recall/open-editor/close-reopen checks exercise this wiring.
The larger KURV composition still uses its experimental view-owned values and
routing/theme state: migrate those consumers onto this contract before calling
that composition a complete plugin editor. Undo/redo, DSP graph publication and
DAW integration are not implemented by this crate.

Run `cargo test -p mui-truce`. The contract check uses two parameter types and
reordered Truce declarations, per-instance values/documents, invalid-state
rollback, parent-route cleanup and balanced host gestures. The runnable GPUI
probe and CLAP validator provide the separate integration checks.

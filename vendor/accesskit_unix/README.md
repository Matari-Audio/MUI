> MUI carries the upstream 0.22.1 API with a generation-owned Linux provider
> runtime. The last adapter joins its worker before drop returns, so plugin code
> is not left running after library unload. Both historical runtime feature names
> use owned Tokio internally. See [MUI-PATCHES.md](MUI-PATCHES.md) for ownership,
> callback-thread teardown and upstream attribution.

# AccessKit Unix adapter

This is the Unix adapter for [AccessKit](https://accesskit.dev/). It exposes an AccessKit accessibility tree through the AT-SPI protocol.

## Compatibility with async runtimes

While this crate's API is purely blocking, it internally spawns asynchronous tasks on an executor.

- If you use tokio, make sure to enable the `tokio` feature of this crate.
- If you use another async runtime or if you don't use one at all, the default feature will suit your needs.

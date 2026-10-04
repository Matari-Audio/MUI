# MUI dependency routing

This is upstream `accesskit_winit` 0.33.2 from AccessKit revision
[c88605b96d04431f9c3c792464a0f2f253480e94](https://github.com/AccessKit/accesskit/tree/c88605b96d04431f9c3c792464a0f2f253480e94/platforms/winit). Its Rust source and public API are
unchanged. The macOS `accesskit_macos` dependency points to the sibling MUI fork
so mui-winit and mui-preview use the same isolated provider as mui-baseview.
This path dependency works for downstream Git users without Cargo patches;
shared registry AccessKit node types retain their original identity.

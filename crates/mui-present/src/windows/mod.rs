//! Windows native presentation:
//!
//! - [`DibWmPaintPresenter`]: retained `WM_PAINT` `CreateDIBSection` CPU presenter.
//! - [`HwndFlipSwapChain`]: default opaque HWND flip-model DXGI 1.2 swapchain
//!   (`CreateSwapChainForHwnd`, `DXGI_ALPHA_MODE_IGNORE`).
//! - [`DirectCompositionPresenter`]: DirectComposition visual + flip-model
//!   DXGI 1.2 swapchain (`CreateSwapChainForComposition`) for translucent D3D11 frames.
//! - [`DxgiSwapChainPresenter`]: unified presenter selecting HWND flip for opaque
//!   and DirectComposition for translucent surfaces.

pub mod dcomp;
pub mod dib;

pub use dcomp::{DirectCompositionPresenter, DxgiSwapChainPresenter, HwndFlipSwapChain};
pub use dib::DibWmPaintPresenter;

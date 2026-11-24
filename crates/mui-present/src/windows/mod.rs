//! Windows native presentation:
//!
//! - [`DibWmPaintPresenter`]: retained `WM_PAINT` `CreateDIBSection` CPU presenter.
//! - [`DirectCompositionPresenter`]: owned DirectComposition visual + flip-model
//!   DXGI 1.2 swapchain (`CreateSwapChainForComposition`) for D3D11 GPU frames.

pub mod dcomp;
pub mod dib;

pub use dcomp::DirectCompositionPresenter;
pub use dib::DibWmPaintPresenter;

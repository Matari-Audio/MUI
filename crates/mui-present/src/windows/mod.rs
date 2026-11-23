//! Windows native CPU presentation: a retained `WM_PAINT` `CreateDIBSection`
//! presenter. GPU frames go through wgpu's DirectComposition visual
//! (`Dx12SwapchainKind::DxgiFromVisual`) on the same `HWND`.

pub mod dib;

pub use dib::DibWmPaintPresenter;

//! Windows retained `WM_PAINT` `CreateDIBSection` CPU presenter.
//!
//! Eliminates out-of-`WM_PAINT` `GetDC`/`StretchDIBits` tearing and DWM
//! flickering: pixels are written into a retained top-down 32-bpp DIB section
//! selected into a persistent compatible memory DC, and a lightweight Win32
//! window subclass handles `WM_PAINT` (`BeginPaint` → `BitBlt`/`StretchBlt` →
//! `EndPaint`). Updating pixels simply writes to the DIB section and calls
//! `InvalidateRect` + `UpdateWindow`.

#![expect(
    unsafe_code,
    reason = "Win32 GDI CreateDIBSection and WM_PAINT window subclassing"
)]

use std::ffi::c_void;
use std::marker::PhantomData;
use std::mem::size_of;
use std::ptr;
use std::rc::Rc;

use raw_window_handle::RawWindowHandle;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BeginPaint, BitBlt, COLORONCOLOR, CreateCompatibleDC,
    CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, EndPaint, GdiFlush, HBITMAP, HDC,
    HGDIOBJ, InvalidateRect, PAINTSTRUCT, RGBQUAD, SRCCOPY, SelectObject, SetStretchBltMode,
    StretchBlt, UpdateWindow,
};
use windows_sys::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::{WM_ERASEBKGND, WM_NCDESTROY, WM_PAINT};

use crate::{DamageRect, PresentError, PresentationBackend};

const SUBCLASS_ID: usize = 0x4D55_4950; // "MUIP"

struct DibSurface {
    mem_dc: HDC,
    bitmap: HBITMAP,
    old_obj: HGDIOBJ,
    bits: *mut u32,
    size: (u32, u32),
}

impl DibSurface {
    fn new(size: (u32, u32)) -> Result<Self, PresentError> {
        let width = i32::try_from(size.0).map_err(|_| PresentError::InvalidSize(size))?;
        let height = i32::try_from(size.1).map_err(|_| PresentError::InvalidSize(size))?;
        // SAFETY: creates a screen-compatible memory DC owned by this struct.
        let mem_dc = unsafe { CreateCompatibleDC(ptr::null_mut()) };
        if mem_dc.is_null() {
            return Err(PresentError::Platform("CreateCompatibleDC failed".into()));
        }
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                // Negative height selects top-down row order matching MUI pixels.
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                biSizeImage: 0,
                biXPelsPerMeter: 0,
                biYPelsPerMeter: 0,
                biClrUsed: 0,
                biClrImportant: 0,
            },
            bmiColors: [RGBQUAD::default()],
        };
        let mut bits: *mut c_void = ptr::null_mut();
        // SAFETY: `info` is a valid top-down 32-bpp BITMAPINFO; `bits` receives the DIB memory.
        let bitmap = unsafe {
            CreateDIBSection(mem_dc, &info, DIB_RGB_COLORS, &mut bits, ptr::null_mut(), 0)
        };
        if bitmap.is_null() || bits.is_null() {
            // SAFETY: clean up `mem_dc` on failure.
            unsafe {
                if !bitmap.is_null() {
                    DeleteObject(bitmap.cast());
                }
                DeleteDC(mem_dc);
            }
            return Err(PresentError::Platform("CreateDIBSection failed".into()));
        }
        // SAFETY: selects our DIB bitmap into `mem_dc` and saves the previous stock object.
        let old_obj = unsafe { SelectObject(mem_dc, bitmap.cast()) };
        if old_obj.is_null() || old_obj as isize == -1 {
            // SAFETY: selection failed, so the bitmap can still be deleted.
            unsafe {
                DeleteObject(bitmap.cast());
                DeleteDC(mem_dc);
            }
            return Err(PresentError::Platform("SelectObject failed".into()));
        }
        Ok(Self {
            mem_dc,
            bitmap,
            old_obj,
            bits: bits.cast::<u32>(),
            size,
        })
    }

    fn pixels_mut(&mut self) -> &mut [u32] {
        let len = self.size.0 as usize * self.size.1 as usize;
        // SAFETY: `self.bits` points to `size.0 * size.1` 32-bit pixels owned by `self.bitmap`.
        unsafe {
            // Flush this GUI thread's batched GDI reads before writing the pixels.
            GdiFlush();
            std::slice::from_raw_parts_mut(self.bits, len)
        }
    }
}

impl Drop for DibSurface {
    fn drop(&mut self) {
        // SAFETY: restores original stock bitmap before deleting DIB and memory DC.
        unsafe {
            if !self.old_obj.is_null() {
                SelectObject(self.mem_dc, self.old_obj);
            }
            DeleteObject(self.bitmap.cast());
            DeleteDC(self.mem_dc);
        }
    }
}

struct SubclassState {
    dib: Option<DibSurface>,
    dst_size: (u32, u32),
    subclass_installed: bool,
}

unsafe extern "system" fn paint_subclass_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    uid_subclass: usize,
    ref_data: usize,
) -> LRESULT {
    let state_ptr = ref_data as *mut SubclassState;
    if msg == WM_ERASEBKGND && !state_ptr.is_null() {
        // SAFETY: `state_ptr` is pinned inside `DibWmPaintPresenter` on this GUI thread.
        let state = unsafe { &*state_ptr };
        if state.dib.is_some() {
            return 1;
        }
    }
    if msg == WM_PAINT && !state_ptr.is_null() {
        // SAFETY: `state_ptr` lives on this GUI thread until subclass removal.
        let state = unsafe { &*state_ptr };
        if let Some(dib) = &state.dib {
            // SAFETY: PAINTSTRUCT is plain data; all-zero is a valid value.
            let mut ps: PAINTSTRUCT = unsafe { std::mem::zeroed() };
            // SAFETY: balanced BeginPaint/EndPaint on this thread's live HWND.
            let hdc = unsafe { BeginPaint(hwnd, &mut ps) };
            if !hdc.is_null() {
                let dst = state.dst_size;
                if dib.size == dst {
                    let x = ps.rcPaint.left.max(0);
                    let y = ps.rcPaint.top.max(0);
                    let w = (ps.rcPaint.right - x).max(0);
                    let h = (ps.rcPaint.bottom - y).max(0);
                    if w > 0 && h > 0 {
                        // SAFETY: blits damaged region from our retained DIB memory DC.
                        unsafe {
                            BitBlt(hdc, x, y, w, h, dib.mem_dc, x, y, SRCCOPY);
                        }
                    }
                } else if dst.0 > 0 && dst.1 > 0 {
                    // SAFETY: scales budgeted DIB surface to full client area inside WM_PAINT.
                    unsafe {
                        SetStretchBltMode(hdc, COLORONCOLOR);
                        StretchBlt(
                            hdc,
                            0,
                            0,
                            dst.0 as i32,
                            dst.1 as i32,
                            dib.mem_dc,
                            0,
                            0,
                            dib.size.0 as i32,
                            dib.size.1 as i32,
                            SRCCOPY,
                        );
                    }
                }
                // SAFETY: pairs with BeginPaint above.
                unsafe {
                    EndPaint(hwnd, &ps);
                }
                return 0;
            }
        }
    }
    if msg == WM_NCDESTROY && !state_ptr.is_null() {
        // SAFETY: automatically unhook if the host destroys the HWND before Drop.
        unsafe {
            (*state_ptr).subclass_installed = false;
            RemoveWindowSubclass(hwnd, Some(paint_subclass_proc), uid_subclass);
        }
    }
    // SAFETY: forward unhandled messages down the subclass chain.
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}

/// Retained `WM_PAINT` `CreateDIBSection` presenter for a Win32 `HWND`.
///
/// `!Send` / `!Sync`: must remain on the Win32 thread that owns `HWND`.
pub struct DibWmPaintPresenter {
    hwnd: HWND,
    state: Box<SubclassState>,
    _gui_thread: PhantomData<Rc<()>>,
}

impl DibWmPaintPresenter {
    /// Installs a retained `WM_PAINT` DIB presenter on a live Win32 `HWND`.
    ///
    /// # Safety
    /// Must be called on the thread owning `HWND`, which must remain valid
    /// until this presenter is dropped.
    ///
    /// # Errors
    /// Returns [`PresentError`] when the handle is not Win32 or subclassing fails.
    pub unsafe fn new(window: RawWindowHandle) -> Result<Self, PresentError> {
        let RawWindowHandle::Win32(handle) = window else {
            return Err(PresentError::UnsupportedHandle(
                "expected Win32 handle on Windows",
            ));
        };
        let hwnd = handle.hwnd.get() as HWND;
        let mut state = Box::new(SubclassState {
            dib: None,
            dst_size: (0, 0),
            subclass_installed: false,
        });
        let state_ptr = (&mut *state as *mut SubclassState) as usize;
        // SAFETY: `state` is heap-allocated and removed from the subclass before Drop frees it.
        let ok =
            unsafe { SetWindowSubclass(hwnd, Some(paint_subclass_proc), SUBCLASS_ID, state_ptr) };
        if ok == 0 {
            return Err(PresentError::Platform("SetWindowSubclass failed".into()));
        }
        state.subclass_installed = true;
        Ok(Self {
            hwnd,
            state,
            _gui_thread: PhantomData,
        })
    }

    /// Always returns [`PresentationBackend::WinDibWmPaint`].
    #[must_use]
    pub const fn backend(&self) -> PresentationBackend {
        PresentationBackend::WinDibWmPaint
    }

    fn ensure_dib(&mut self, size: (u32, u32)) -> Result<&mut DibSurface, PresentError> {
        if self.state.dib.as_ref().is_none_or(|d| d.size != size) {
            self.state.dib = Some(DibSurface::new(size)?);
        }
        Ok(self.state.dib.as_mut().expect("DIB surface created"))
    }

    fn flush_rect(&self, rect: DamageRect) {
        let win_rect = RECT {
            left: rect.x as i32,
            top: rect.y as i32,
            right: (rect.x + rect.width) as i32,
            bottom: (rect.y + rect.height) as i32,
        };
        // SAFETY: triggers WM_PAINT synchronously inside the OS paint pipeline.
        unsafe {
            InvalidateRect(self.hwnd, &win_rect, 0);
            UpdateWindow(self.hwnd);
        }
    }

    /// Writes a dirty rectangle of `0x00RRGGBB` pixels into the DIB and schedules `WM_PAINT`.
    pub fn present_patch(
        &mut self,
        rect: DamageRect,
        pixels_0rgb: &[u32],
        surface_size: (u32, u32),
    ) -> Result<(), PresentError> {
        self.state.dst_size = surface_size;
        let dib = self.ensure_dib(surface_size)?;
        let dst = dib.pixels_mut();
        let stride = surface_size.0 as usize;
        let w = rect.width as usize;
        for row in 0..rect.height as usize {
            let dst_start = (rect.y as usize + row) * stride + rect.x as usize;
            let src_start = row * w;
            dst[dst_start..dst_start + w].copy_from_slice(&pixels_0rgb[src_start..src_start + w]);
        }
        self.flush_rect(rect);
        Ok(())
    }

    /// Writes a full-surface `0x00RRGGBB` frame into the DIB and flushes damaged rects.
    pub fn present_full_with_damage(
        &mut self,
        pixels_0rgb: &[u32],
        surface_size: (u32, u32),
        damage: &[DamageRect],
    ) -> Result<(), PresentError> {
        self.state.dst_size = surface_size;
        let dib = self.ensure_dib(surface_size)?;
        dib.pixels_mut().copy_from_slice(pixels_0rgb);
        if damage.is_empty() {
            self.flush_rect(DamageRect::from_array([
                0,
                0,
                surface_size.0,
                surface_size.1,
            ]));
        } else {
            for &r in damage {
                if !r.is_empty() {
                    let win_rect = RECT {
                        left: r.x as i32,
                        top: r.y as i32,
                        right: (r.x + r.width) as i32,
                        bottom: (r.y + r.height) as i32,
                    };
                    // SAFETY: marks each damaged region before one UpdateWindow pass.
                    unsafe {
                        InvalidateRect(self.hwnd, &win_rect, 0);
                    }
                }
            }
            // SAFETY: dispatches one coalesced WM_PAINT for all invalidated regions.
            unsafe {
                UpdateWindow(self.hwnd);
            }
        }
        Ok(())
    }

    /// Writes a budgeted `src_size` bitmap and lets `WM_PAINT` `StretchBlt` it to `dst_size`.
    pub fn present_scaled(
        &mut self,
        src_size: (u32, u32),
        pixels_0rgb: &[u32],
        dst_size: (u32, u32),
    ) -> Result<(), PresentError> {
        self.state.dst_size = dst_size;
        let dib = self.ensure_dib(src_size)?;
        dib.pixels_mut().copy_from_slice(pixels_0rgb);
        self.flush_rect(DamageRect::from_array([0, 0, dst_size.0, dst_size.1]));
        Ok(())
    }
}

impl Drop for DibWmPaintPresenter {
    fn drop(&mut self) {
        if self.state.subclass_installed {
            self.state.subclass_installed = false;
            // SAFETY: removes subclass before `self.state` is freed.
            unsafe {
                RemoveWindowSubclass(self.hwnd, Some(paint_subclass_proc), SUBCLASS_ID);
            }
        }
        self.state.dib.take();
    }
}

//! macOS native CPU presentation via an `IOSurface` ring bound to `CALayer.contents`.
//!
//! Replaces both `softbuffer`'s leaking CPU sublayer and the `CGImage` mosaic
//! transport (`mac_transport.rs`): a 3-slot `IOSurface` ring eliminates per-patch
//! `CGImage` allocations, sublayer splitting, and extra compositor copies, and
//! cleanly detaches its single `CALayer` on drop so `CAMetalLayer` promotion is
//! never occluded.

#![expect(
    unsafe_code,
    reason = "CoreAnimation CALayer and IOSurface interop on macOS main thread"
)]

use std::ffi::c_void;
use std::marker::PhantomData;
use std::ptr::{self, NonNull};
use std::rc::Rc;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::NSView;
use objc2_core_foundation::{
    CFDictionary, CFNumber, CFRetained, CFString, kCFTypeDictionaryKeyCallBacks,
    kCFTypeDictionaryValueCallBacks,
};
use objc2_io_surface::{
    IOSurfaceLockOptions, IOSurfaceRef, kIOSurfaceAllocSize, kIOSurfaceBytesPerElement,
    kIOSurfaceBytesPerRow, kIOSurfaceHeight, kIOSurfacePixelFormat, kIOSurfaceWidth,
};
use objc2_quartz_core::{CALayer, CATransaction};
use raw_window_handle::RawWindowHandle;

use crate::{DamageRect, PresentError, PresentationBackend, blit_scaled_0rgb};

const RING_LEN: usize = 3;
/// `'BGRA'` FourCC (`kCVPixelFormatType_32BGRA`): matches `0xFFRRGGBB` on little-endian.
const PIXEL_FORMAT_BGRA: i32 = 0x4247_5241;

struct SurfaceSlot {
    surface: CFRetained<IOSurfaceRef>,
    size: (u32, u32),
    stride_u32: usize,
}

impl SurfaceSlot {
    fn new(size: (u32, u32)) -> Result<Self, PresentError> {
        let width = i32::try_from(size.0).map_err(|_| PresentError::InvalidSize(size))?;
        let height = i32::try_from(size.1).map_err(|_| PresentError::InvalidSize(size))?;
        let bytes_per_row = (size.0 as usize)
            .checked_mul(4)
            .and_then(|b| b.checked_next_multiple_of(64))
            .ok_or(PresentError::InvalidSize(size))?;
        let alloc_size = bytes_per_row
            .checked_mul(size.1 as usize)
            .ok_or(PresentError::InvalidSize(size))?;
        let bpr_i32 = i32::try_from(bytes_per_row).map_err(|_| PresentError::InvalidSize(size))?;
        let alloc_i32 = i32::try_from(alloc_size).map_err(|_| PresentError::InvalidSize(size))?;

        let n_width = CFNumber::new_i32(width);
        let n_height = CFNumber::new_i32(height);
        let n_bpe = CFNumber::new_i32(4);
        let n_bpr = CFNumber::new_i32(bpr_i32);
        let n_alloc = CFNumber::new_i32(alloc_i32);
        let n_fmt = CFNumber::new_i32(PIXEL_FORMAT_BGRA);

        // SAFETY: kIOSurface* statics are immutable CoreFoundation CFString keys
        // and every CFNumber value is live across CFDictionary::new.
        let dict = unsafe {
            let keys: [*const c_void; 6] = [
                kIOSurfaceWidth as *const CFString as *const c_void,
                kIOSurfaceHeight as *const CFString as *const c_void,
                kIOSurfaceBytesPerElement as *const CFString as *const c_void,
                kIOSurfaceBytesPerRow as *const CFString as *const c_void,
                kIOSurfaceAllocSize as *const CFString as *const c_void,
                kIOSurfacePixelFormat as *const CFString as *const c_void,
            ];
            let values: [*const c_void; 6] = [
                n_width.as_ref() as *const CFNumber as *const c_void,
                n_height.as_ref() as *const CFNumber as *const c_void,
                n_bpe.as_ref() as *const CFNumber as *const c_void,
                n_bpr.as_ref() as *const CFNumber as *const c_void,
                n_alloc.as_ref() as *const CFNumber as *const c_void,
                n_fmt.as_ref() as *const CFNumber as *const c_void,
            ];
            CFDictionary::new(
                None,
                keys.as_ptr().cast_mut(),
                values.as_ptr().cast_mut(),
                keys.len() as isize,
                &kCFTypeDictionaryKeyCallBacks,
                &kCFTypeDictionaryValueCallBacks,
            )
            .ok_or_else(|| PresentError::Platform("CFDictionary creation failed".into()))?
        };

        // SAFETY: `dict` is a valid IOSurface properties dictionary.
        let surface = unsafe { IOSurfaceRef::new(&dict) }
            .ok_or_else(|| PresentError::Platform("IOSurfaceCreate returned null".into()))?;
        let actual_bpr = surface.bytes_per_row();
        Ok(Self {
            surface,
            size,
            stride_u32: actual_bpr / 4,
        })
    }

    fn is_in_use(&self) -> bool {
        self.surface.is_in_use()
    }

    fn write(&mut self, write_fn: impl FnOnce(&mut [u32], usize)) -> Result<(), PresentError> {
        let mut seed = 0u32;
        // SAFETY: locking our owned IOSurface for CPU write before accessing its base address.
        let kr = unsafe { self.surface.lock(IOSurfaceLockOptions::empty(), &mut seed) };
        if kr != 0 {
            return Err(PresentError::Platform(format!(
                "IOSurfaceLock failed with kern_return {kr}"
            )));
        }
        let base = self.surface.base_address();
        let total = self.stride_u32 * self.size.1 as usize;
        // SAFETY: surface is locked exclusively for CPU access; `total` fits in allocation.
        let slice = unsafe { std::slice::from_raw_parts_mut(base.as_ptr().cast::<u32>(), total) };
        write_fn(slice, self.stride_u32);
        // SAFETY: matches the successful IOSurfaceLock above.
        unsafe {
            self.surface
                .unlock(IOSurfaceLockOptions::empty(), &mut seed);
        }
        Ok(())
    }
}

/// Owned macOS `IOSurface` ring presenter backed by a single `CALayer`.
///
/// `!Send` / `!Sync`: must be created, driven, and dropped on the AppKit main thread.
pub struct IoSurfacePresenter {
    layer: Retained<CALayer>,
    ring: [Option<SurfaceSlot>; RING_LEN],
    cursor: usize,
    retained: Vec<u32>,
    retained_size: (u32, u32),
    _main_thread: PhantomData<Rc<()>>,
}

impl IoSurfacePresenter {
    /// Creates an `IOSurface`-backed `CALayer` sublayer on an AppKit `NSView`.
    ///
    /// # Safety
    /// Must be called on the main thread with a live `RawWindowHandle::AppKit`
    /// whose `NSView` outlives this presenter.
    ///
    /// # Errors
    /// Returns [`PresentError`] when the handle is not AppKit or layer setup fails.
    pub unsafe fn new(window: RawWindowHandle) -> Result<Self, PresentError> {
        let RawWindowHandle::AppKit(handle) = window else {
            return Err(PresentError::UnsupportedHandle(
                "expected AppKit handle on macOS",
            ));
        };
        // AppKit allows its views to be touched only on the main thread.
        if objc2::MainThreadMarker::new().is_none() {
            return Err(PresentError::Platform(
                "IOSurface presenter must be created on the AppKit main thread".into(),
            ));
        }
        let view_ptr: NonNull<NSView> = handle.ns_view.cast();
        // SAFETY: caller guarantees a live NSView; the main thread is checked above.
        let view: &NSView = unsafe { view_ptr.as_ref() };
        view.setWantsLayer(true);
        let root = view
            .layer()
            .ok_or_else(|| PresentError::Platform("NSView has no backing CALayer".into()))?;
        let layer = CALayer::new();
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        layer.setFrame(root.bounds());
        layer.setContentsScale(root.contentsScale());
        root.addSublayer(&layer);
        CATransaction::commit();

        Ok(Self {
            layer,
            ring: [const { None }; RING_LEN],
            cursor: 0,
            retained: Vec::new(),
            retained_size: (0, 0),
            _main_thread: PhantomData,
        })
    }

    /// Always returns [`PresentationBackend::MacIoSurface`].
    #[must_use]
    pub const fn backend(&self) -> PresentationBackend {
        PresentationBackend::MacIoSurface
    }

    fn acquire_slot(&mut self, size: (u32, u32)) -> Result<usize, PresentError> {
        let (idx, replace) = select_slot(self.cursor, |i| {
            self.ring[i].as_ref().is_none_or(|slot| !slot.is_in_use())
        });
        if replace || self.ring[idx].as_ref().is_none_or(|slot| slot.size != size) {
            // Core Animation retains the old IOSurface while displaying it.
            self.ring[idx] = Some(SurfaceSlot::new(size)?);
        }
        self.cursor = (idx + 1) % RING_LEN;
        Ok(idx)
    }

    fn publish(&self, idx: usize) {
        let slot = self.ring[idx].as_ref().expect("acquired IOSurface slot");
        // SAFETY: IOSurfaceRef is toll-free bridged to ObjC `id` for CALayer.contents.
        let contents: &AnyObject =
            unsafe { &*(slot.surface.as_ref() as *const IOSurfaceRef as *const AnyObject) };
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        if let Some(root) = self.layer.superlayer() {
            self.layer.setFrame(root.bounds());
            self.layer.setContentsScale(root.contentsScale());
        }
        // SAFETY: `contents` is a valid IOSurface object accepted by CALayer.setContents.
        unsafe {
            self.layer.setContents(Some(contents));
        }
        CATransaction::commit();
    }

    /// Uploads and presents a dirty rectangle of `0x00RRGGBB` pixels.
    pub fn present_patch(
        &mut self,
        rect: DamageRect,
        pixels_0rgb: &[u32],
        surface_size: (u32, u32),
    ) -> Result<(), PresentError> {
        if self.retained_size != surface_size {
            self.retained.resize(
                surface_size.0 as usize * surface_size.1 as usize,
                0xff00_0000,
            );
            self.retained_size = surface_size;
        }
        let stride = surface_size.0 as usize;
        let w = rect.width as usize;
        for row in 0..rect.height as usize {
            let dst = (rect.y as usize + row) * stride + rect.x as usize;
            let src = row * w;
            for (out, &px) in self.retained[dst..dst + w]
                .iter_mut()
                .zip(&pixels_0rgb[src..src + w])
            {
                *out = 0xff00_0000 | px;
            }
        }
        let idx = self.acquire_slot(surface_size)?;
        let retained = &self.retained;
        let (sw, sh) = (surface_size.0 as usize, surface_size.1 as usize);
        self.ring[idx]
            .as_mut()
            .expect("acquired IOSurface slot")
            .write(|dst, dst_stride| {
                for y in 0..sh {
                    dst[y * dst_stride..y * dst_stride + sw]
                        .copy_from_slice(&retained[y * sw..(y + 1) * sw]);
                }
            })?;
        self.publish(idx);
        Ok(())
    }

    /// Uploads and presents a full-surface `0x00RRGGBB` frame.
    pub fn present_full_with_damage(
        &mut self,
        pixels_0rgb: &[u32],
        surface_size: (u32, u32),
        _damage: &[DamageRect],
    ) -> Result<(), PresentError> {
        if self.retained_size != surface_size {
            self.retained.resize(
                surface_size.0 as usize * surface_size.1 as usize,
                0xff00_0000,
            );
            self.retained_size = surface_size;
        }
        for (out, &px) in self.retained.iter_mut().zip(pixels_0rgb) {
            *out = 0xff00_0000 | px;
        }
        let idx = self.acquire_slot(surface_size)?;
        let retained = &self.retained;
        let (sw, sh) = (surface_size.0 as usize, surface_size.1 as usize);
        self.ring[idx]
            .as_mut()
            .expect("acquired IOSurface slot")
            .write(|dst, dst_stride| {
                for y in 0..sh {
                    dst[y * dst_stride..y * dst_stride + sw]
                        .copy_from_slice(&retained[y * sw..(y + 1) * sw]);
                }
            })?;
        self.publish(idx);
        Ok(())
    }

    /// Presents a budgeted `src_size` bitmap scaled by CoreAnimation (or into the
    /// `IOSurface` ring) to fill the view bounds.
    pub fn present_scaled(
        &mut self,
        src_size: (u32, u32),
        pixels_0rgb: &[u32],
        dst_size: (u32, u32),
    ) -> Result<(), PresentError> {
        let _ = dst_size;
        // CoreAnimation hardware-scales `CALayer.contents` across `layer.frame`,
        // so uploading only the budgeted `src_size` IOSurface avoids CPU scaling.
        let idx = self.acquire_slot(src_size)?;
        let (sw, sh) = (src_size.0 as usize, src_size.1 as usize);
        self.ring[idx]
            .as_mut()
            .expect("acquired IOSurface slot")
            .write(|dst, dst_stride| {
                if dst_stride == sw {
                    for (out, &px) in dst[..sw * sh].iter_mut().zip(pixels_0rgb) {
                        *out = 0xff00_0000 | px;
                    }
                } else {
                    blit_scaled_0rgb(
                        pixels_0rgb,
                        src_size,
                        dst,
                        src_size,
                        dst_stride,
                        0xff00_0000,
                    );
                }
            })?;
        self.publish(idx);
        Ok(())
    }
}

impl Drop for IoSurfacePresenter {
    fn drop(&mut self) {
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        // SAFETY: clearing layer contents to nil before detaching from superlayer.
        unsafe {
            self.layer.setContents(None);
        }
        self.layer.removeFromSuperlayer();
        CATransaction::commit();
        let _ = ptr::null::<c_void>();
    }
}

fn select_slot(cursor: usize, free: impl Fn(usize) -> bool) -> (usize, bool) {
    (0..RING_LEN)
        .map(|offset| (cursor + offset) % RING_LEN)
        .find(|&index| free(index))
        .map_or((cursor, true), |index| (index, false))
}

#[cfg(test)]
mod tests {
    use super::select_slot;

    #[test]
    fn all_busy_slots_require_a_fresh_surface() {
        assert_eq!(select_slot(1, |_| false), (1, true));
        assert_eq!(select_slot(1, |index| index == 0), (0, false));
    }
}

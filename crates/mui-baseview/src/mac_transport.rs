//! CoreAnimation bitmap mosaic. Replacing a dirty rectangle splits existing
//! image layers into cropped pieces; unchanged pieces reuse their CGImage.
//! Only the new dirty image is uploaded. No AppKit view leaves the main thread.
#![expect(
    unsafe_code,
    reason = "own CoreAnimation/CGImage objects on the native main thread"
)]
#![expect(
    unexpected_cfgs,
    reason = "objc 0.2 msg_send checks the retired cargo-clippy feature"
)]
use super::*;
use objc::runtime::Object;
use objc::{class, msg_send, sel, sel_impl};
use std::ffi::c_void;

#[repr(C)]
#[derive(Clone, Copy)]
struct Rect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}
// SAFETY: CGRect is two CGPoint/CGSize pairs of CGFloat doubles on 64-bit macOS.
unsafe impl objc::Encode for Rect {
    fn encode() -> objc::Encoding {
        // SAFETY: this static encoding describes the repr(C) four-double ABI.
        unsafe { objc::Encoding::from_str("{CGRect={CGPoint=dd}{CGSize=dd}}") }
    }
}
#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGColorSpaceCreateDeviceRGB() -> *mut c_void;
    fn CGColorSpaceRelease(value: *mut c_void);
    fn CGDataProviderCreateWithData(
        info: *mut c_void,
        data: *const c_void,
        size: usize,
        release: Option<unsafe extern "C" fn(*mut c_void, *const c_void, usize)>,
    ) -> *mut c_void;
    fn CGDataProviderRelease(value: *mut c_void);
    fn CGImageCreate(
        w: usize,
        h: usize,
        bits: usize,
        pixel_bits: usize,
        stride: usize,
        color: *mut c_void,
        bitmap: u32,
        provider: *mut c_void,
        decode: *const f64,
        interpolate: bool,
        intent: i32,
    ) -> *mut c_void;
    fn CGImageRetain(value: *mut c_void) -> *mut c_void;
    fn CGImageRelease(value: *mut c_void);
}
unsafe extern "C" fn release_pixels(_: *mut c_void, data: *const c_void, bytes: usize) {
    // SAFETY: make_image transferred this exact boxed u32 slice to the provider.
    drop(unsafe {
        Box::from_raw(std::ptr::slice_from_raw_parts_mut(
            data as *mut u32,
            bytes / 4,
        ))
    });
}
fn make_image(pixels: Vec<u32>, w: u32, h: u32) -> Result<*mut c_void, String> {
    let pixels = pixels.into_boxed_slice();
    let bytes = pixels.len() * 4;
    let data = Box::into_raw(pixels).cast::<c_void>();
    // SAFETY: the provider assumes this allocation's lifetime and invokes our
    // deallocator. CGImage retains the provider after this function returns.
    unsafe {
        let provider =
            CGDataProviderCreateWithData(std::ptr::null_mut(), data, bytes, Some(release_pixels));
        if provider.is_null() {
            release_pixels(std::ptr::null_mut(), data, bytes);
            return Err("CGDataProvider creation failed".into());
        }
        let color = CGColorSpaceCreateDeviceRGB();
        let image = CGImageCreate(
            w as usize,
            h as usize,
            8,
            32,
            w as usize * 4,
            color,
            0x2000 | 6,
            provider,
            std::ptr::null(),
            false,
            0,
        );
        CGColorSpaceRelease(color);
        CGDataProviderRelease(provider);
        if image.is_null() {
            Err("CGImage creation failed".into())
        } else {
            Ok(image)
        }
    }
}
struct Piece {
    rect: [u32; 4],
    source: [u32; 4],
    image: *mut c_void,
    layer: *mut Object,
}
impl Drop for Piece {
    fn drop(&mut self) {
        // SAFETY: each piece owns a retained image and an allocated layer.
        unsafe {
            let _: () = msg_send![self.layer, removeFromSuperlayer];
            let _: () = msg_send![self.layer, release];
            CGImageRelease(self.image);
        }
    }
}
pub(super) struct Presenter {
    container: *mut Object,
    pieces: Vec<Piece>,
    size: (u32, u32),
}
impl Presenter {
    pub(super) fn new(window: &WindowContext) -> Result<Self, String> {
        let handle = window.window_handle().map_err(|e| e.to_string())?;
        let raw_window_handle::RawWindowHandle::AppKit(handle) = handle.as_raw() else {
            return Err("expected an AppKit view".into());
        };
        let view = handle.ns_view.as_ptr().cast::<Object>();
        // SAFETY: called on the live view's main thread before showing it.
        unsafe {
            let _: () = msg_send![view,setWantsLayer:true];
            let root: *mut Object = msg_send![view, layer];
            let container: *mut Object = msg_send![class!(CALayer), new];
            if container.is_null() {
                return Err("CALayer creation failed".into());
            }
            let _: () = msg_send![container,setGeometryFlipped:true];
            let _: () = msg_send![root,addSublayer:container];
            Ok(Self {
                container,
                pieces: Vec::new(),
                size: (0, 0),
            })
        }
    }
    pub(super) fn present(
        &mut self,
        window: &WindowContext,
        rect: [u32; 4],
        pixels: Vec<u32>,
    ) -> Result<(), String> {
        let image = make_image(pixels, rect[2], rect[3])?;
        let physical = window.size().physical;
        if self.size != (physical.width, physical.height) {
            self.pieces.clear();
            self.size = (physical.width, physical.height);
        }
        // SAFETY: all layers belong to this native thread. Transactions disable
        // implicit animations; the compositor presents completed image updates.
        unsafe {
            let root: *mut Object = msg_send![self.container, superlayer];
            let bounds: Rect = msg_send![root, bounds];
            let scale: f64 = msg_send![root, contentsScale];
            let scale = if scale > 0. {
                scale
            } else {
                window.size().scale_factor
            };
            let _: () = msg_send![class!(CATransaction), begin];
            let _: () = msg_send![class!(CATransaction),setDisableActions:true];
            let _: () = msg_send![self.container,setFrame:bounds];
            let _: () = msg_send![self.container,setContentsScale:scale];
            let mut next = Vec::new();
            for piece in self.pieces.drain(..) {
                let [x, y, w, h] = piece.rect;
                let x0 = x.max(rect[0]);
                let y0 = y.max(rect[1]);
                let x1 = (x + w).min(rect[0] + rect[2]);
                let y1 = (y + h).min(rect[1] + rect[3]);
                if x0 >= x1 || y0 >= y1 {
                    next.push(piece);
                    continue;
                }
                for r in [
                    [x, y, w, y0 - y],
                    [x, y1, w, y + h - y1],
                    [x, y0, x0 - x, y1 - y0],
                    [x1, y0, x + w - x1, y1 - y0],
                ] {
                    if r[2] > 0 && r[3] > 0 {
                        next.push(make_piece(
                            self.container,
                            r,
                            piece.source,
                            CGImageRetain(piece.image),
                            scale,
                        ));
                    }
                }
            }
            next.push(make_piece(self.container, rect, rect, image, scale));
            self.pieces = next;
            let _: () = msg_send![class!(CATransaction), commit];
        }
        Ok(())
    }
}
unsafe fn make_piece(
    parent: *mut Object,
    rect: [u32; 4],
    source: [u32; 4],
    image: *mut c_void,
    scale: f64,
) -> Piece {
    // SAFETY: caller transfers an owned image reference; parent is our live,
    // main-thread container. The crop references only pixels within source.
    unsafe {
        let layer: *mut Object = msg_send![class!(CALayer), new];
        let frame = Rect {
            x: f64::from(rect[0]) / scale,
            y: f64::from(rect[1]) / scale,
            w: f64::from(rect[2]) / scale,
            h: f64::from(rect[3]) / scale,
        };
        let crop = Rect {
            x: f64::from(rect[0] - source[0]) / f64::from(source[2]),
            y: f64::from(rect[1] - source[1]) / f64::from(source[3]),
            w: f64::from(rect[2]) / f64::from(source[2]),
            h: f64::from(rect[3]) / f64::from(source[3]),
        };
        let _: () = msg_send![layer,setFrame:frame];
        let _: () = msg_send![layer,setContentsScale:scale];
        let _: () = msg_send![layer,setContentsRect:crop];
        let _: () = msg_send![layer,setContents:image];
        let _: () = msg_send![parent,addSublayer:layer];
        Piece {
            rect,
            source,
            image,
            layer,
        }
    }
}
impl Drop for Presenter {
    fn drop(&mut self) {
        self.pieces.clear();
        // SAFETY: the native adapter drops before its view context.
        unsafe {
            let _: () = msg_send![self.container, removeFromSuperlayer];
            let _: () = msg_send![self.container, release];
        }
    }
}

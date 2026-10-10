use crate::dpi::PhysicalPosition;
use std::cell::{Cell, RefCell};
use std::ffi::OsString;
use std::os::windows::prelude::OsStringExt;
use std::ptr::null_mut;
use std::rc::Weak;
use windows::core::implement;
use windows::Win32::Foundation::POINTL;
use windows::Win32::System::Com::{IDataObject, DVASPECT_CONTENT, FORMATETC, TYMED_HGLOBAL};
use windows::Win32::System::Ole::*;
use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;
use windows_core::Ref;
use windows_sys::Win32::UI::Shell::DragQueryFileW;

use super::window_state::WindowState;
use super::{callback, native};
use crate::wrappers::win32::window::HWnd;
use crate::{DropData, DropEffect, Event, EventStatus, MouseEvent};

#[implement(IDropTarget)]
pub(crate) struct DropTarget {
    hwnd: HWnd,
    revoked: Cell<bool>,
    window_state: Weak<WindowState>,

    // These are cached since DragOver and DragLeave callbacks don't provide them,
    // and handling drag move events gets awkward on the client end otherwise
    drag_position: Cell<PhysicalPosition<i32>>,
    drop_data: RefCell<DropData>,
}

impl DropTarget {
    pub(crate) fn new(window_state: Weak<WindowState>, hwnd: HWnd) -> Self {
        Self {
            hwnd,
            revoked: false.into(),
            window_state,
            drag_position: Cell::new(PhysicalPosition::new(0, 0)),
            drop_data: RefCell::new(DropData::None),
        }
    }

    pub(crate) fn revoke(&self) {
        self.revoked.set(true);
    }

    fn on_event(&self, pdw_effect: Option<*mut DROPEFFECT>, event: MouseEvent) {
        if self.revoked.get() {
            return;
        }
        let event_status =
            unsafe { native::with_window(self.hwnd, |w| w.handle_event(Event::Mouse(event))) };

        let effect = match event_status {
            Some(EventStatus::AcceptDrop(DropEffect::Copy)) => DROPEFFECT_COPY,
            Some(EventStatus::AcceptDrop(DropEffect::Move)) => DROPEFFECT_MOVE,
            Some(EventStatus::AcceptDrop(DropEffect::Link)) => DROPEFFECT_LINK,
            Some(EventStatus::AcceptDrop(DropEffect::Scroll)) => DROPEFFECT_SCROLL,
            _ => DROPEFFECT_NONE,
        };

        if let Some(pdw_effect) = pdw_effect.filter(|p| !p.is_null()) {
            unsafe { pdw_effect.write(effect) };
        }
    }

    fn parse_coordinates(&self, pt: POINTL) {
        let Ok(phy_point) = self.hwnd.screen_to_client(PhysicalPosition::new(pt.x, pt.y)) else {
            return;
        };

        self.drag_position.set(phy_point);
    }

    fn parse_drop_data(&self, data_object: &IDataObject) {
        let format = FORMATETC {
            cfFormat: CF_HDROP.0,
            ptd: null_mut(),
            dwAspect: DVASPECT_CONTENT.0,
            lindex: -1,
            tymed: TYMED_HGLOBAL.0 as u32,
        };

        unsafe {
            let Ok(medium) = data_object.GetData(&format) else {
                self.drop_data.replace(DropData::None);
                return;
            };

            // Release the IDataObject-owned allocation on every return, including
            // panic containment. Hosts repeatedly dragging files must not leak it.
            struct Medium(windows::Win32::System::Com::STGMEDIUM);
            impl Drop for Medium {
                fn drop(&mut self) {
                    unsafe {
                        ReleaseStgMedium(&mut self.0);
                    }
                }
            }
            let medium = Medium(medium);
            let hdrop = medium.0.u.hGlobal.0;

            let item_count = DragQueryFileW(hdrop, 0xFFFFFFFF, null_mut(), 0);
            if item_count == 0 {
                self.drop_data.replace(DropData::None);
                return;
            }

            let mut paths = Vec::with_capacity(item_count as usize);

            for i in 0..item_count {
                let characters = DragQueryFileW(hdrop, i, null_mut(), 0);
                let buffer_size = (characters as usize).saturating_add(1);
                let mut buffer = vec![0u16; buffer_size];

                DragQueryFileW(hdrop, i, buffer.as_mut_ptr().cast(), buffer_size as u32);

                if let Some(chars) = buffer.get(..characters as usize) {
                    paths.push(OsString::from_wide(chars).into());
                }
            }

            self.drop_data.replace(DropData::Files(paths));
        }
    }
}

#[allow(non_snake_case, reason = "To match trait")]
impl IDropTarget_Impl for DropTarget_Impl {
    fn DragEnter(
        &self, data: Ref<IDataObject>, keys: MODIFIERKEYS_FLAGS, pt: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> windows_core::Result<()> {
        self.guarded_drag(effect, || {
            let Some(state) = self.window_state.upgrade() else {
                return Ok(());
            };
            let Some(data) = data.as_ref() else {
                return Ok(());
            };
            self.parse_coordinates(*pt);
            self.parse_drop_data(data);
            self.on_event(
                Some(effect),
                MouseEvent::DragEntered {
                    position: self.drag_position.get().cast(),
                    modifiers: state
                        .keyboard_state()
                        .get_modifiers_from_mouse_wparam(keys.0 as usize),
                    data: self.drop_data.borrow().clone(),
                },
            );
            Ok(())
        })
    }
    fn DragOver(
        &self, keys: MODIFIERKEYS_FLAGS, pt: &POINTL, effect: *mut DROPEFFECT,
    ) -> windows_core::Result<()> {
        self.guarded_drag(effect, || {
            let Some(state) = self.window_state.upgrade() else {
                return Ok(());
            };
            self.parse_coordinates(*pt);
            self.on_event(
                Some(effect),
                MouseEvent::DragMoved {
                    position: self.drag_position.get().cast(),
                    modifiers: state
                        .keyboard_state()
                        .get_modifiers_from_mouse_wparam(keys.0 as usize),
                    data: self.drop_data.borrow().clone(),
                },
            );
            Ok(())
        })
    }
    fn DragLeave(&self) -> windows_core::Result<()> {
        self.guarded_drag(null_mut(), || {
            self.on_event(None, MouseEvent::DragLeft);
            Ok(())
        })
    }
    fn Drop(
        &self, data: Ref<IDataObject>, keys: MODIFIERKEYS_FLAGS, pt: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> windows_core::Result<()> {
        self.guarded_drag(effect, || {
            let Some(state) = self.window_state.upgrade() else {
                return Ok(());
            };
            let Some(data) = data.as_ref() else {
                return Ok(());
            };
            self.parse_coordinates(*pt);
            self.parse_drop_data(data);
            self.on_event(
                Some(effect),
                MouseEvent::DragDropped {
                    position: self.drag_position.get().cast(),
                    modifiers: state
                        .keyboard_state()
                        .get_modifiers_from_mouse_wparam(keys.0 as usize),
                    data: self.drop_data.borrow().clone(),
                },
            );
            Ok(())
        })
    }
}

impl DropTarget {
    fn guarded_drag(
        &self, effect: *mut DROPEFFECT, body: impl FnOnce() -> windows_core::Result<()>,
    ) -> windows_core::Result<()> {
        // S_OK with no accepted effect is safe for late calls, absent data and
        // contained panics. Null effect pointers are permitted by our boundary.
        if !effect.is_null() {
            unsafe {
                effect.write(DROPEFFECT_NONE);
            }
        }
        if self.revoked.get() {
            return Ok(());
        }
        callback::guard(
            "IDropTarget",
            || {
                if !effect.is_null() {
                    unsafe {
                        effect.write(DROPEFFECT_NONE);
                    }
                }
                Ok(())
            },
            body,
        )
    }
}

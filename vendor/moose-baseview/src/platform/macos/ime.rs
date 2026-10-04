//! Cocoa NSTextInputClient state. Native callbacks release every RefCell borrow before dispatch.
use crate::{ime::utf16_to_byte, ImeConfiguration};
use keyboard_types::KeyboardEvent;
use objc2::{msg_send, runtime::AnyObject, ClassType};
use objc2_foundation::{NSAttributedString, NSRange, NSString};
use std::cell::{Cell, RefCell};

#[derive(Default)]
pub(crate) struct NativeIme {
    pub configuration: RefCell<Option<ImeConfiguration>>,
    pub marked: RefCell<String>,
    pub marked_start: Cell<usize>,
    pub selected: Cell<(usize, usize)>,
    pub pending_key: RefCell<Option<KeyboardEvent>>,
}
impl NativeIme {
    pub fn enabled(&self) -> bool {
        self.configuration.borrow().is_some()
    }
    pub fn selection(&self) -> NSRange {
        if !self.marked.borrow().is_empty() {
            let (at, len) = self.selected.get();
            return NSRange::new(self.marked_start.get() + at, len);
        }
        let config = self.configuration.borrow();
        let Some(c) = config.as_ref() else {
            return NSRange::new(usize::MAX, 0);
        };
        let at = c.text[..c.selection.start].encode_utf16().count();
        let len = c.text[c.selection.clone()].encode_utf16().count();
        NSRange::new(at, len)
    }
    pub fn marked_range(&self) -> NSRange {
        let marked = self.marked.borrow();
        if marked.is_empty() {
            NSRange::new(usize::MAX, 0)
        } else {
            NSRange::new(self.marked_start.get(), marked.encode_utf16().count())
        }
    }
    pub fn replacement(&self, range: NSRange) -> Option<crate::Ime> {
        if range.location == usize::MAX {
            return None;
        }
        let config = self.configuration.borrow();
        let config = config.as_ref()?;
        let start = utf16_to_byte(&config.text, range.location);
        let end = utf16_to_byte(&config.text, range.location.saturating_add(range.length));
        Some(crate::Ime::Selection(start..end))
    }
    pub fn mark(&self, text: String, selected: NSRange, replacement: NSRange) -> crate::Ime {
        if self.marked.borrow().is_empty() {
            let start = if replacement.location == usize::MAX {
                self.selection().location
            } else {
                replacement.location
            };
            self.marked_start.set(start);
        }
        self.selected.set((selected.location, selected.length));
        let cursor = if selected.location == usize::MAX {
            None
        } else {
            Some((
                utf16_to_byte(&text, selected.location),
                utf16_to_byte(&text, selected.location.saturating_add(selected.length)),
            ))
        };
        *self.marked.borrow_mut() = text.clone();
        crate::Ime::Preedit { text, cursor }
    }
}

/// AppKit supplies NSString or NSAttributedString; `string` must precede UTF8 extraction.
pub(super) fn text(object: &AnyObject) -> String {
    unsafe {
        let attributed: bool = msg_send![object, isKindOfClass: NSAttributedString::class()];
        let string: &NSString = if attributed {
            msg_send![object, string]
        } else {
            &*(object as *const AnyObject).cast::<NSString>()
        };
        string.to_string()
    }
}

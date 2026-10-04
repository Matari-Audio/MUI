//! XIM composition over the window's existing XCB connection; no global locale mutation.
//! Uses the MIT licensed xim-rs transport also used by GPUI. See README-MUI.md.
use crate::ime::char_to_byte;
use crate::wrappers::xlib::XlibXcbConnection;
use crate::{Event, Ime, ImeConfiguration};
use std::{collections::VecDeque, sync::Arc};
use x11rb::{
    protocol::{xproto::KeyPressEvent, Event as XEvent},
    xcb_ffi::XCBConnection,
};
use xim::x11rb::{HasConnection, X11rbClient};
use xim::{AttributeName, Client, ClientError, ClientHandler, InputStyle};

struct Connection(Arc<XlibXcbConnection>);
impl HasConnection for Connection {
    type Connection = XCBConnection;
    fn conn(&self) -> &XCBConnection {
        self.0.xcb_connection()
    }
}

pub(super) struct NativeIme {
    client: X11rbClient<Connection>,
    handler: Handler,
}
#[derive(Default)]
struct Handler {
    window: u32,
    im: u16,
    ic: u16,
    enabled: bool,
    focused: bool,
    preedit: String,
    caret: usize,
    configuration: Option<ImeConfiguration>,
    pending: VecDeque<NativeEvent>,
}
pub(super) enum NativeEvent {
    Input(Event),
    Key(KeyPressEvent),
}

impl NativeIme {
    pub fn new(connection: Arc<XlibXcbConnection>, window: u32) -> Option<Self> {
        let screen = connection.default_screen_index().into();
        match X11rbClient::init(Connection(connection), screen, None) {
            Ok(client) => Some(Self { client, handler: Handler { window, ..Handler::default() } }),
            Err(e) => {
                crate::warn!("XIM unavailable: {} (set XMODIFIERS before process startup)", e);
                None
            }
        }
    }
    pub fn configure(&mut self, config: Option<ImeConfiguration>, focused: bool) {
        let config = config.filter(ImeConfiguration::valid);
        if self.handler.configuration == config && self.handler.focused == focused {
            return;
        }
        let was_enabled = self.handler.enabled;
        let switched =
            self.handler.configuration.as_ref().map(|c| &c.id) != config.as_ref().map(|c| &c.id);
        if was_enabled && (switched || config.is_none() || !focused) && self.handler.ic != 0 {
            let _ = self.client.reset_ic(self.handler.im, self.handler.ic);
            self.handler.preedit.clear();
            self.handler.emit(Ime::Disabled);
        }
        self.handler.configuration = config;
        self.handler.focused = focused;
        self.handler.enabled = self.handler.configuration.is_some() && focused;
        if self.handler.ic != 0 {
            let result = if self.handler.enabled {
                self.handler.set_spot(&mut self.client).and_then(|()| {
                    if !was_enabled {
                        self.client.set_focus(self.handler.im, self.handler.ic)
                    } else {
                        Ok(())
                    }
                })
            } else if was_enabled {
                self.client.unset_focus(self.handler.im, self.handler.ic)
            } else {
                Ok(())
            };
            if let Err(e) = result {
                crate::warn!("XIM configuration failed: {}", e);
            }
        }
        if was_enabled != self.handler.enabled || (switched && self.handler.enabled) {
            if !self.handler.enabled {
                self.handler.preedit.clear();
            }
            self.handler.emit(if self.handler.enabled { Ime::Enabled } else { Ime::Disabled });
        }
    }
    pub fn filter(&mut self, event: &XEvent) -> bool {
        match self.client.filter_event(event, &mut self.handler) {
            Ok(filtered) => filtered,
            Err(e) => {
                crate::warn!("XIM protocol error: {}", e);
                false
            }
        }
    }
    pub fn forward(&mut self, event: &KeyPressEvent) -> bool {
        if !self.handler.enabled || self.handler.ic == 0 {
            return false;
        }
        if let Err(e) = self.client.forward_event(
            self.handler.im,
            self.handler.ic,
            xim::ForwardEventFlag::empty(),
            event,
        ) {
            crate::warn!("XIM key forwarding failed: {}", e);
            return false;
        }
        true
    }
    pub fn drain(&mut self) -> VecDeque<NativeEvent> {
        std::mem::take(&mut self.handler.pending)
    }
}
impl Drop for NativeIme {
    fn drop(&mut self) {
        if self.handler.ic != 0 {
            let _ = self.client.destroy_ic(self.handler.im, self.handler.ic);
        }
        if self.handler.im != 0 {
            let _ = self.client.close(self.handler.im);
        }
        let _ = self.client.disconnect();
    }
}
impl Handler {
    fn emit(&mut self, input: Ime) {
        self.pending.push_back(NativeEvent::Input(Event::Ime(input)));
    }
    fn preedit(&mut self) {
        let byte = char_to_byte(&self.preedit, self.caret);
        self.emit(Ime::Preedit { text: self.preedit.clone(), cursor: Some((byte, byte)) });
    }
    fn set_spot<C: Client>(&self, client: &mut C) -> Result<(), ClientError> {
        let Some(config) = &self.configuration else { return Ok(()) };
        let attrs = client
            .build_ic_attributes()
            .nested_list(AttributeName::PreeditAttributes, |b| {
                b.push(
                    AttributeName::SpotLocation,
                    xim::Point {
                        x: config.position.x.clamp(i16::MIN as f64, i16::MAX as f64) as i16,
                        y: (config.position.y + config.size.height)
                            .clamp(i16::MIN as f64, i16::MAX as f64)
                            as i16,
                    },
                );
            })
            .build();
        client.set_ic_values(self.im, self.ic, attrs)
    }
}
impl<C: Client<XEvent = KeyPressEvent>> ClientHandler<C> for Handler {
    fn handle_connect(&mut self, c: &mut C) -> Result<(), ClientError> {
        // XIM negotiates a locale string; never call setlocale in a plugin host.
        let locale = std::env::var("LC_ALL")
            .ok()
            .filter(|s| !s.is_empty())
            .or_else(|| std::env::var("LC_CTYPE").ok().filter(|s| !s.is_empty()))
            .or_else(|| std::env::var("LANG").ok())
            .unwrap_or_else(|| "en_US.UTF-8".into());
        c.open(&locale)
    }
    fn handle_open(&mut self, c: &mut C, im: u16) -> Result<(), ClientError> {
        self.im = im;
        let attrs = c
            .build_ic_attributes()
            .push(
                AttributeName::InputStyle,
                InputStyle::PREEDIT_CALLBACKS | InputStyle::STATUS_NOTHING,
            )
            .push(AttributeName::ClientWindow, self.window)
            .push(AttributeName::FocusWindow, self.window)
            .build();
        c.create_ic(im, attrs)
    }
    fn handle_create_ic(&mut self, c: &mut C, im: u16, ic: u16) -> Result<(), ClientError> {
        self.im = im;
        self.ic = ic;
        self.set_spot(c)?;
        if self.enabled {
            c.set_focus(im, ic)?;
        }
        Ok(())
    }
    fn handle_forward_event(
        &mut self, _: &mut C, _: u16, _: u16, _: xim::ForwardEventFlag, event: KeyPressEvent,
    ) -> Result<(), ClientError> {
        self.pending.push_back(NativeEvent::Key(event));
        Ok(())
    }
    fn handle_commit(&mut self, _: &mut C, _: u16, _: u16, text: &str) -> Result<(), ClientError> {
        if self.enabled {
            self.emit(Ime::Commit(text.into()));
        }
        self.preedit.clear();
        Ok(())
    }
    fn handle_preedit_start(&mut self, _: &mut C, _: u16, _: u16) -> Result<(), ClientError> {
        self.preedit.clear();
        self.caret = 0;
        Ok(())
    }
    fn handle_preedit_done(&mut self, _: &mut C, _: u16, _: u16) -> Result<(), ClientError> {
        self.preedit.clear();
        self.caret = 0;
        if self.enabled {
            self.preedit();
        }
        Ok(())
    }
    fn handle_preedit_draw(
        &mut self, _: &mut C, _: u16, _: u16, caret: i32, first: i32, len: i32,
        _: xim::PreeditDrawStatus, text: &str, _: Vec<xim::Feedback>,
    ) -> Result<(), ClientError> {
        replace_preedit(&mut self.preedit, first, len, text);
        self.caret = caret.max(0) as usize;
        if self.enabled {
            self.preedit();
        }
        Ok(())
    }
    fn handle_preedit_caret(
        &mut self, _: &mut C, _: u16, _: u16, position: &mut i32, direction: xim::CaretDirection,
        _: xim::CaretStyle,
    ) -> Result<(), ClientError> {
        self.caret = match direction {
            xim::CaretDirection::ForwardChar => self.caret.saturating_add(1),
            xim::CaretDirection::BackwardChar => self.caret.saturating_sub(1),
            xim::CaretDirection::AbsolutePosition => (*position).max(0) as usize,
            _ => self.caret,
        }
        .min(self.preedit.chars().count());
        *position = self.caret as i32;
        if self.enabled {
            self.preedit();
        }
        Ok(())
    }
    fn handle_disconnect(&mut self) {
        self.ic = 0;
        self.im = 0;
        self.preedit.clear();
        self.emit(Ime::Disabled);
    }
}

fn replace_preedit(preedit: &mut String, first: i32, len: i32, text: &str) {
    let start = char_to_byte(preedit, first.max(0) as usize);
    let end = char_to_byte(preedit, first.max(0).saturating_add(len.max(0)) as usize);
    preedit.replace_range(start..end, text);
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn xim_incremental_changes_are_character_ranges() {
        let mut text = "a😀漢z".to_owned();
        replace_preedit(&mut text, 1, 2, "日本");
        assert_eq!(text, "a日本z");
        assert_eq!(char_to_byte(&text, 3), 7);
        replace_preedit(&mut text, 1, 2, "");
        assert_eq!(text, "az");
        replace_preedit(&mut text, 100, 100, "é");
        assert_eq!(text, "azé");
    }
}

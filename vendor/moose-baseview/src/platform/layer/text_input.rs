//! Text-input-v3 transactions stay on the layer surface's queue.
use super::*;
use smithay_client_toolkit::dispatch2::Dispatch2;
use wayland_client::{globals::GlobalList, protocol::wl_seat::WlSeat};
use wayland_protocols::wp::text_input::zv3::client::{
    zwp_text_input_manager_v3::ZwpTextInputManagerV3,
    zwp_text_input_v3::{self, ZwpTextInputV3},
};

#[derive(Default)]
pub(super) struct TextInput {
    manager: Option<ZwpTextInputManagerV3>,
    input: Option<ZwpTextInputV3>,
    entered: bool,
    enabled: bool,
    composing: bool,
    config: Option<ImeConfiguration>,
    serial: u32,
    pending: Vec<Ime>,
}
impl TextInput {
    pub fn bind(globals: &GlobalList, qh: &QueueHandle<Runtime>) -> Self {
        let mut text = Self::default();
        text.manager = globals.bind(qh, 1..=1, TextData).ok();
        text
    }
    pub fn seat(&mut self, seat: &WlSeat, qh: &QueueHandle<Runtime>) {
        if self.input.is_none() {
            self.input = self.manager.as_ref().map(|m| m.get_text_input(seat, qh, TextData));
        }
    }
    pub fn active(&self) -> bool {
        self.composing
    }
    pub fn configure(&mut self, config: Option<ImeConfiguration>, scale: f64) {
        let config = config.filter(ImeConfiguration::valid);
        if self.config == config {
            return;
        }
        let changed_field = self.config.as_ref().map(|c| &c.id) != config.as_ref().map(|c| &c.id);
        if changed_field || crate::ime::composition_cancelled(&self.config, &config, self.composing)
        {
            if let Some(input) = self.input.as_ref().filter(|_| self.entered && self.enabled) {
                input.disable();
                input.commit();
                self.serial = self.serial.wrapping_add(1);
            }
            self.enabled = false;
            self.composing = false;
            self.pending.clear();
        }
        self.config = config;
        self.apply(scale);
    }
    fn apply(&mut self, scale: f64) {
        let Some(input) = &self.input else {
            return;
        };
        if self.entered {
            if let Some(config) = &self.config {
                if !self.enabled {
                    input.enable();
                }
                // The protocol caps surrounding text at 4000 bytes. Omitting a
                // larger value is valid and avoids truncating UTF-8/selection.
                if config.text.len() <= 4000 {
                    input.set_surrounding_text(
                        config.text.clone(),
                        config.selection.end as i32,
                        config.selection.start as i32,
                    );
                } else {
                    input.set_surrounding_text(String::new(), 0, 0);
                }
                input.set_cursor_rectangle(
                    (config.position.x / scale) as i32,
                    (config.position.y / scale) as i32,
                    (config.size.width / scale).ceil().max(1.0) as i32,
                    (config.size.height / scale).ceil().max(1.0) as i32,
                );
                self.enabled = true;
            } else if self.enabled {
                input.disable();
                self.enabled = false;
            }
            input.commit();
            self.serial = self.serial.wrapping_add(1);
        }
    }
}
impl Drop for TextInput {
    fn drop(&mut self) {
        if let Some(input) = self.input.take() {
            input.destroy();
        }
        if let Some(manager) = self.manager.take() {
            manager.destroy();
        }
    }
}
#[derive(Debug)]
struct TextData;
impl Dispatch2<ZwpTextInputManagerV3, Runtime> for TextData {
    fn event(
        &self, _: &mut Runtime, _: &ZwpTextInputManagerV3,
        _: <ZwpTextInputManagerV3 as Proxy>::Event, _: &Connection, _: &QueueHandle<Runtime>,
    ) {
    }
}
impl Dispatch2<ZwpTextInputV3, Runtime> for TextData {
    fn event(
        &self, runtime: &mut Runtime, _: &ZwpTextInputV3, event: zwp_text_input_v3::Event,
        _: &Connection, _: &QueueHandle<Runtime>,
    ) {
        let Some(cx) = &runtime.context else {
            return;
        };
        let mut text = cx.text.borrow_mut();
        let mut events = Vec::new();
        match event {
            zwp_text_input_v3::Event::Enter { surface } if &surface == cx.surface() => {
                text.entered = true;
                text.apply(cx.scale_factor());
                events.push(Ime::Enabled);
            }
            zwp_text_input_v3::Event::Leave { .. } => {
                text.entered = false;
                text.composing = false;
                text.enabled = false;
                text.pending.clear();
                events.push(Ime::Disabled);
            }
            zwp_text_input_v3::Event::PreeditString { text: value, cursor_begin, cursor_end } => {
                let value = value.unwrap_or_default();
                let cursor = usize::try_from(cursor_begin)
                    .ok()
                    .zip(usize::try_from(cursor_end).ok())
                    .filter(|(a, b)| value.is_char_boundary(*a) && value.is_char_boundary(*b));
                text.pending.push(Ime::Preedit { text: value, cursor });
            }
            zwp_text_input_v3::Event::CommitString { text: Some(value) } => {
                text.pending.push(Ime::Commit(value));
            }
            zwp_text_input_v3::Event::DeleteSurroundingText { before_length, after_length } => {
                if let Some(config) = &text.config {
                    let cursor = config.selection.end;
                    let start = cursor.saturating_sub(before_length as usize);
                    let end = cursor.saturating_add(after_length as usize).min(config.text.len());
                    if config.text.is_char_boundary(start) && config.text.is_char_boundary(end) {
                        text.pending.push(Ime::Selection(start..end));
                    }
                }
            }
            zwp_text_input_v3::Event::Done { serial } => {
                if serial == text.serial {
                    events = std::mem::take(&mut text.pending);
                    events.sort_by_key(|e| match e {
                        Ime::Selection(_) => 0,
                        Ime::Commit(_) => 1,
                        _ => 2,
                    });
                    for event in &events {
                        match event {
                            Ime::Commit(_) => text.composing = false,
                            Ime::Preedit { text: value, .. } => text.composing = !value.is_empty(),
                            _ => {}
                        }
                    }
                } else {
                    text.pending.clear();
                }
            }
            _ => {}
        }
        drop(text);
        for event in events {
            runtime.event(Event::Ime(event));
        }
    }
}

use super::*;
use keyboard_types::{Key, KeyState, KeyboardEvent, Modifiers, NamedKey};
use smithay_client_toolkit::seat::{
    keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers as NativeMods, RawModifiers},
    pointer::{
        PointerData, PointerEvent, PointerEventKind, PointerHandler, BTN_LEFT, BTN_MIDDLE,
        BTN_RIGHT,
    },
    Capability, SeatHandler,
};
use wayland_client::protocol::{wl_keyboard::WlKeyboard, wl_pointer::WlPointer, wl_seat::WlSeat};

impl SeatHandler for Runtime {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seats
    }
    fn new_seat(&mut self, _: &Connection, qh: &QueueHandle<Self>, seat: WlSeat) {
        self.data.add_seat(qh, &seat);
    }
    fn new_capability(
        &mut self, _: &Connection, qh: &QueueHandle<Self>, seat: WlSeat, capability: Capability,
    ) {
        self.data.add_seat(qh, &seat);
        match capability {
            Capability::Keyboard if self.keyboard.is_none() => {
                self.keyboard = self.seats.get_keyboard(qh, &seat, None).ok();
                if let Some(cx) = &self.context {
                    cx.text.borrow_mut().seat(&seat, qh);
                }
            }
            Capability::Pointer if self.pointer.is_none() => {
                self.pointer = self.seats.get_pointer(qh, &seat).ok();
            }
            _ => {}
        }
    }
    fn remove_capability(
        &mut self, _: &Connection, _: &QueueHandle<Self>, _: WlSeat, capability: Capability,
    ) {
        match capability {
            Capability::Keyboard => {
                if let Some(k) = self.keyboard.take() {
                    k.release();
                }
            }
            Capability::Pointer => {
                if let Some(p) = self.pointer.take() {
                    p.release();
                }
                if let Some(cx) = &self.context {
                    cx.primary.set(false);
                    cx.press.borrow_mut().take();
                }
            }
            _ => {}
        }
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, seat: WlSeat) {
        self.data.remove_seat(&seat);
    }
}

impl KeyboardHandler for Runtime {
    fn enter(
        &mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlKeyboard,
        _: &wl_surface::WlSurface, _: u32, _: &[u32], _: &[Keysym],
    ) {
        if let Some(cx) = &self.context {
            cx.focused(true);
        }
        self.event(Event::Window(WindowEvent::Focused));
    }
    fn leave(
        &mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlKeyboard,
        _: &wl_surface::WlSurface, _: u32,
    ) {
        if let Some(cx) = &self.context {
            cx.focused(false);
        }
        self.modifiers = Modifiers::empty();
        self.event(Event::Window(WindowEvent::Unfocused));
    }
    fn press_key(
        &mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlKeyboard, _: u32, event: KeyEvent,
    ) {
        self.key(event, KeyState::Down, false);
    }
    fn repeat_key(
        &mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlKeyboard, _: u32, event: KeyEvent,
    ) {
        self.key(event, KeyState::Down, true);
    }
    fn release_key(
        &mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlKeyboard, _: u32, event: KeyEvent,
    ) {
        self.key(event, KeyState::Up, false);
    }
    fn update_modifiers(
        &mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlKeyboard, _: u32, mods: NativeMods,
        _: RawModifiers, _: u32,
    ) {
        let mut out = Modifiers::empty();
        for (on, flag) in [
            (mods.shift, Modifiers::SHIFT),
            (mods.ctrl, Modifiers::CONTROL),
            (mods.alt, Modifiers::ALT),
            (mods.logo, Modifiers::META),
            (mods.caps_lock, Modifiers::CAPS_LOCK),
            (mods.num_lock, Modifiers::NUM_LOCK),
        ] {
            if on {
                out.insert(flag);
            }
        }
        self.modifiers = out;
    }
}
impl Runtime {
    fn key(&self, event: KeyEvent, state: KeyState, repeat: bool) {
        let named = match event.keysym {
            Keysym::Escape => Some(NamedKey::Escape),
            Keysym::Return | Keysym::KP_Enter => Some(NamedKey::Enter),
            Keysym::BackSpace => Some(NamedKey::Backspace),
            Keysym::Delete => Some(NamedKey::Delete),
            Keysym::Tab | Keysym::ISO_Left_Tab => Some(NamedKey::Tab),
            Keysym::Left => Some(NamedKey::ArrowLeft),
            Keysym::Right => Some(NamedKey::ArrowRight),
            Keysym::Up => Some(NamedKey::ArrowUp),
            Keysym::Down => Some(NamedKey::ArrowDown),
            Keysym::Home => Some(NamedKey::Home),
            Keysym::End => Some(NamedKey::End),
            Keysym::Page_Up => Some(NamedKey::PageUp),
            Keysym::Page_Down => Some(NamedKey::PageDown),
            _ => None,
        };
        let key = named.map(Key::Named).unwrap_or_else(|| {
            event
                .utf8
                .clone()
                .filter(|s| !s.is_empty())
                .map(Key::Character)
                .unwrap_or(Key::Named(NamedKey::Unidentified))
        });
        let code = super::super::x11::keyboard::hardware_keycode_to_code(
            event.raw_code.saturating_add(8).min(u32::from(u16::MAX)) as u16,
        );
        let composing = self.context.as_ref().is_some_and(|cx| cx.text.borrow().active());
        if composing
            && matches!(key, Key::Character(_))
            && !self.modifiers.intersects(Modifiers::CONTROL | Modifiers::META | Modifiers::ALT)
        {
            return;
        }
        self.event(Event::Keyboard(KeyboardEvent {
            state,
            key,
            code,
            location: crate::keyboard::code_to_location(code),
            modifiers: self.modifiers,
            repeat,
            is_composing: composing,
        }));
    }
}
impl PointerHandler for Runtime {
    fn pointer_frame(
        &mut self, _: &Connection, _: &QueueHandle<Self>, pointer: &WlPointer,
        events: &[PointerEvent],
    ) {
        for event in events {
            let Some(cx) = &self.context else { continue };
            if &event.surface != cx.surface() {
                continue;
            }
            let scale = cx.scale_factor();
            let position =
                PhysicalPosition::new(event.position.0 * scale, event.position.1 * scale);
            let modifiers = self.modifiers;
            if !matches!(event.kind, PointerEventKind::Leave { .. }) {
                self.event(Event::Mouse(MouseEvent::CursorMoved { position, modifiers }));
            }
            match event.kind {
                PointerEventKind::Enter { serial } => {
                    cx.pointer_enter(pointer, serial);
                    self.event(Event::Mouse(MouseEvent::CursorEntered));
                }
                PointerEventKind::Leave { .. } => {
                    cx.pointer_leave();
                    self.event(Event::Mouse(MouseEvent::CursorLeft));
                }
                PointerEventKind::Press { button, serial, .. } => {
                    if button == BTN_LEFT {
                        cx.primary.set(true);
                        *cx.press.borrow_mut() = pointer
                            .data::<PointerData<()>>()
                            .and_then(|data| self.data.device(data.seat()))
                            .map(|d| (d, serial));
                    }
                    if let Some(button) = mouse_button(button) {
                        self.event(Event::Mouse(MouseEvent::ButtonPressed { button, modifiers }));
                    }
                }
                PointerEventKind::Release { button, .. } => {
                    if button == BTN_LEFT {
                        cx.primary.set(false);
                        cx.press.borrow_mut().take();
                    }
                    if let Some(button) = mouse_button(button) {
                        self.event(Event::Mouse(MouseEvent::ButtonReleased { button, modifiers }));
                    }
                }
                PointerEventKind::Axis { horizontal, vertical, .. } => {
                    let delta = if horizontal.value120 != 0 || vertical.value120 != 0 {
                        ScrollDelta::Lines {
                            x: -(horizontal.value120 as f32) / 120.0,
                            y: -(vertical.value120 as f32) / 120.0,
                        }
                    } else {
                        ScrollDelta::Pixels {
                            x: -horizontal.absolute as f32,
                            y: -vertical.absolute as f32,
                        }
                    };
                    self.event(Event::Mouse(MouseEvent::WheelScrolled { delta, modifiers }));
                }
                _ => {}
            }
        }
    }
}
fn mouse_button(button: u32) -> Option<MouseButton> {
    match button {
        BTN_LEFT => Some(MouseButton::Left),
        BTN_RIGHT => Some(MouseButton::Right),
        BTN_MIDDLE => Some(MouseButton::Middle),
        _ => None,
    }
}

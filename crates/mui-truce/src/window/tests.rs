//! Headless: the handler without a window or a GPU.
use super::*;
use keyboard_types::KeyboardEvent;
use mui::prelude::{knob, Theme};

/// A knob and a count of the trees built.
struct Knob {
    value: f64,
    builds: usize,
    changed: bool,
}

impl View for Knob {
    fn build(&mut self, ui: &mut Ui) -> El {
        self.builds += 1;
        knob(ui, "k", "K", &mut self.value, 0.0..=1.0).0.into()
    }
    fn changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }
    fn request_resize(&mut self, _: u32, _: u32) -> bool {
        false
    }
}

fn handler(size: (u32, u32), scale: f64) -> Handler<Knob> {
    let view = Knob {
        value: 0.5,
        builds: 0,
        changed: false,
    };
    let shared = Arc::new(Mutex::new(Shared {
        ui: Ui::new(Theme::DEFAULT),
        view,
    }));
    Handler::new(shared, Arc::default(), size, scale)
}

fn builds(h: &Handler<Knob>) -> usize {
    lock(&h.shared).view.builds
}

fn moved(x: f64, y: f64, modifiers: Modifiers) -> Event {
    Event::Mouse(MouseEvent::CursorMoved {
        position: baseview::Point::new(x, y),
        modifiers,
    })
}

fn button(pressed: bool) -> Event {
    let (button, modifiers) = (baseview::MouseButton::Left, Modifiers::default());
    Event::Mouse(if pressed {
        MouseEvent::ButtonPressed { button, modifiers }
    } else {
        MouseEvent::ButtonReleased { button, modifiers }
    })
}

fn key(key: HostKey, state: KeyState, modifiers: Modifiers) -> Event {
    Event::Keyboard(KeyboardEvent {
        state,
        key,
        modifiers,
        ..KeyboardEvent::default()
    })
}

fn inputs(h: &Handler<Knob>) -> Vec<&Input> {
    h.pending
        .iter()
        .map(|p| match p {
            Pending::Input(i) | Pending::Move(i) => i,
            Pending::Cancel(_) => panic!("unexpected cancel"),
        })
        .collect()
}

/// The two ways a resize breaks: the `as u16` wrap, and a minimised
/// window configuring a zero-sized surface.
#[test]
fn target_size_clamps_and_refuses_nothing_to_draw_into() {
    assert_eq!(target_size((0, 600)), None);
    assert_eq!(target_size((800, 0)), None);
    assert_eq!(target_size((99_999, 600)), Some((65_535, 600)));
}

#[test]
fn layout_is_logical_when_the_surface_is_physical() {
    assert_eq!(logical_size((2560, 1600), 2.0), Size::new(1280.0, 800.0));
    let mut h = handler((640, 400), 1.0);
    h.on_event_inner(&Event::Window(WindowEvent::Resized(
        baseview::WindowInfo::from_logical_size(baseview::Size::new(320.0, 200.0), 2.0),
    )));
    assert_eq!((h.size, h.scale), ((640, 400), 2.0));
    assert!(h.dirty);
}

#[test]
fn pointer_stays_logical_and_keyboard_modifiers_track_their_own_edges() {
    let mut h = handler((2560, 1600), 2.0);
    h.on_event_inner(&moved(10.0, 20.0, Modifiers::default()));
    assert_eq!(h.pointer.pos, Some(Point::new(10.0, 20.0)));
    // X11 samples a press's state before the modifier is set...
    h.on_event_inner(&key(HostKey::Alt, KeyState::Down, Modifiers::default()));
    assert!(h.pointer.mods.alt);
    // ...and a release's while it is still held.
    h.on_event_inner(&key(HostKey::Alt, KeyState::Up, Modifiers::ALT));
    assert!(!h.pointer.mods.alt);
}

#[test]
fn space_is_a_key_and_text_but_a_shortcut_is_only_a_key() {
    let mut h = handler((200, 200), 1.0);
    h.on_event_inner(&key(
        HostKey::Character(" ".into()),
        KeyState::Down,
        Modifiers::default(),
    ));
    h.on_event_inner(&key(
        HostKey::Character("c".into()),
        KeyState::Down,
        Modifiers::CONTROL,
    ));
    h.on_event_inner(&key(
        HostKey::Character("c".into()),
        KeyState::Down,
        Modifiers::default(),
    ));
    let i = inputs(&h);
    assert_eq!(i[0].text, " ");
    assert_eq!(i[0].keys[0].key, Key::Space);
    assert_eq!((i[1].text.as_str(), i[1].keys[0].key), ("", Key::Char('c')));
    assert!(i[1].keys[0].mods.ctrl);
    assert_eq!((i[2].text.as_str(), i[2].keys.len()), ("c", 0));
}

#[test]
fn queued_pointer_edges_keep_their_order() {
    let mut h = handler((200, 200), 1.0);
    h.on_event_inner(&moved(10.0, 20.0, Modifiers::default()));
    h.on_event_inner(&button(true));
    h.on_event_inner(&button(false));
    let i = inputs(&h);
    assert_eq!(i.len(), 3);
    assert_eq!(i[0].pointer.pos, Some(Point::new(10.0, 20.0)));
    assert!(i[1].pointer.buttons.contains(Button::Primary));
    assert!(!i[2].pointer.buttons.contains(Button::Primary));
}

#[test]
fn hover_bursts_coalesce_but_drag_samples_and_modifier_changes_survive() {
    let mut h = handler((1280, 800), 1.0);
    for x in 0..1000 {
        h.on_event_inner(&moved(f64::from(x), 20.0, Modifiers::default()));
    }
    assert_eq!(h.pending.len(), 1);
    assert_eq!(inputs(&h)[0].pointer.pos, Some(Point::new(999.0, 20.0)));
    h.on_event_inner(&moved(1000.0, 20.0, Modifiers::SHIFT));
    assert_eq!(h.pending.len(), 2);
    h.on_event_inner(&button(true));
    for x in 0..10 {
        h.on_event_inner(&moved(f64::from(x), 30.0, Modifiers::SHIFT));
    }
    h.on_event_inner(&button(false));
    assert_eq!(h.pending.len(), 14);
}

#[test]
fn focus_loss_queues_a_cancel_after_the_pressed_snapshot() {
    let mut h = handler((200, 200), 1.0);
    h.on_event_inner(&button(true));
    h.on_event_inner(&Event::Window(WindowEvent::Unfocused));
    assert!(h.pointer.buttons.is_empty());
    assert!(
        matches!(h.pending.front(), Some(Pending::Input(i)) if i.pointer.buttons.contains(Button::Primary))
    );
    assert!(matches!(h.pending.get(1), Some(Pending::Cancel(p)) if p.buttons.is_empty()));
}

#[test]
fn an_idle_tick_builds_nothing_until_something_changes() {
    let mut h = handler((400, 300), 1.0);
    assert!(h.step(), "the first tick paints");
    let settled = builds(&h);
    // Knob springs may still be settling from the first frame.
    for _ in 0..600 {
        h.step();
    }
    let idle = builds(&h);
    assert!(!h.step(), "a settled editor paints nothing");
    assert_eq!(builds(&h), idle);
    assert!(idle >= settled);

    lock(&h.shared).view.changed = true;
    assert!(h.step(), "a model change is a frame");
    h.requests.redraw();
    assert!(h.step(), "so is a redraw request");
    h.on_event_inner(&moved(1.0, 1.0, Modifiers::default()));
    assert!(h.step(), "and an event");
}

#[test]
fn a_press_and_release_between_two_ticks_are_two_frames() {
    let mut h = handler((400, 300), 1.0);
    h.step();
    let before = builds(&h);
    h.on_event_inner(&moved(5.0, 5.0, Modifiers::default()));
    h.on_event_inner(&button(true));
    h.on_event_inner(&button(false));
    assert!(h.step());
    assert_eq!(builds(&h) - before, 3, "one frame per queued event");
    assert!(h.pending.is_empty());
}

#[test]
fn a_minimised_window_holds_its_input_until_it_has_a_size() {
    let mut h = handler((400, 300), 1.0);
    h.step();
    h.on_event_inner(&Event::Window(WindowEvent::Resized(
        baseview::WindowInfo::from_logical_size(baseview::Size::new(0.0, 0.0), 1.0),
    )));
    h.on_event_inner(&button(true));
    assert!(!h.step());
    assert_eq!(h.pending.len(), 1, "the press is kept, not dropped");
    h.on_event_inner(&Event::Window(WindowEvent::Resized(
        baseview::WindowInfo::from_logical_size(baseview::Size::new(400.0, 300.0), 1.0),
    )));
    assert!(h.step());
    assert!(h.pending.is_empty());
}

/// A plain surface: no springs, so nothing but its gesture edges asks for
/// another frame.
struct Pad;

impl View for Pad {
    fn build(&mut self, _: &mut Ui) -> El {
        mui::prelude::leaf(100.0, 100.0).id("pad")
    }
    fn changed(&mut self) -> bool {
        false
    }
    fn request_resize(&mut self, _: u32, _: u32) -> bool {
        false
    }
}

#[test]
fn a_delivered_edge_gets_the_tree_that_dispatches_it() {
    let shared = Arc::new(Mutex::new(Shared {
        ui: Ui::new(Theme::DEFAULT),
        view: Pad,
    }));
    let mut h = Handler::new(Arc::clone(&shared), Arc::default(), (200, 200), 1.0);
    h.step();
    h.on_event_inner(&moved(50.0, 50.0, Modifiers::default()));
    h.step();
    assert!(h.step(), "the new hover is owed one tree");
    assert!(!h.step(), "then hovering a plain surface settles");
    h.on_event_inner(&button(true));
    assert!(h.step());
    assert_eq!(
        lock(&shared).ui.edit("pad"),
        Some(mui::Edit::Begin),
        "the press delivered a Begin"
    );
    assert!(h.step(), "and the next tree, which dispatches it, comes");
}

/// Counts the Enter presses its trees read; refuses layout while `refuse`.
struct Keys {
    enters: usize,
    refuse: bool,
}

impl View for Keys {
    fn build(&mut self, ui: &mut Ui) -> El {
        self.enters += ui
            .shortcuts()
            .iter()
            .filter(|k| k.key == Key::Enter)
            .count();
        let side = if self.refuse { f64::NAN } else { 100.0 };
        mui::prelude::leaf(side, side).id("pad")
    }
    fn changed(&mut self) -> bool {
        false
    }
    fn request_resize(&mut self, _: u32, _: u32) -> bool {
        false
    }
}

#[test]
fn a_refused_layout_does_not_replay_the_keys_it_took() {
    let shared = Arc::new(Mutex::new(Shared {
        ui: Ui::new(Theme::DEFAULT),
        view: Keys {
            enters: 0,
            refuse: false,
        },
    }));
    let mut h = Handler::new(Arc::clone(&shared), Arc::default(), (200, 200), 1.0);
    h.step();
    lock(&shared).view.refuse = true;
    h.on_event_inner(&key(HostKey::Enter, KeyState::Down, Modifiers::default()));
    assert!(!h.step(), "the layout is refused");
    lock(&shared).view.refuse = false;
    for x in 0..4 {
        h.on_event_inner(&moved(f64::from(x), 1.0, Modifiers::default()));
        h.step();
    }
    assert_eq!(lock(&shared).view.enters, 1, "one press, read once");
}

#[test]
fn a_key_goes_back_to_the_host_unless_something_here_has_focus() {
    let mut h = handler((200, 200), 1.0);
    h.step();
    let space = key(
        HostKey::Character(" ".into()),
        KeyState::Down,
        Modifiers::default(),
    );
    assert_eq!(h.on_event_inner(&space), EventStatus::Ignored);
    assert_eq!(h.pending.len(), 1, "still read, for global shortcuts");
    lock(&h.shared).ui.focus("k");
    assert_eq!(h.on_event_inner(&space), EventStatus::Captured);
}

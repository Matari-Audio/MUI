use super::*;
use mui::prelude::{El, Input, Ui, block};

struct Pad {
    frames: usize,
    cancels: usize,
}
impl View for Pad {
    fn build(&mut self, _: &mut Ui, _: &Input) -> El {
        self.frames += 1;
        block(100., 100.).id("pad")
    }
    fn changed(&mut self) -> bool {
        false
    }
    fn request_resize(&mut self, _: u32, _: u32) -> bool {
        false
    }
    fn cancel(&mut self, _: &Ui) {
        self.cancels += 1;
    }
}
fn rig() -> (Driver, Shared<Pad>) {
    (
        Driver::new(
            (800, 600),
            1.,
            Box::new(Clipboard {
                native: None,
                fallback: String::new(),
            }),
        ),
        Shared {
            ui: Ui::default(),
            view: Pad {
                frames: 0,
                cancels: 0,
            },
        },
    )
}

#[test]
fn first_frame_does_not_wait_for_input_and_idle_ticks_build_nothing() {
    let (mut driver, mut shared) = rig();
    assert!(driver.advance(&mut shared, Instant::now()));
    assert!(shared.ui.scene().is_some());
    let frames = shared.view.frames;
    assert!(!driver.advance(&mut shared, Instant::now()));
    assert_eq!(shared.view.frames, frames);
}

#[test]
fn scale_changes_use_the_current_physical_size_and_pointer_units() {
    let mut state = WindowState::default();
    state.update((1600, 1200), 2.);
    assert_eq!(state.points(200., 100.), Point::new(100., 50.));
    let (mut driver, mut shared) = rig();
    driver.resized(state.size, state.scale);
    driver.pointer_moved(state.points(200., 100.), Mods::default());
    assert!(driver.advance(&mut shared, Instant::now()));
    assert_eq!(driver.ui_scale(), 2.);
    assert_eq!(driver.pointer().pos, Some(Point::new(100., 50.)));
    state.update((2400, 1800), 3.);
    driver.resized(state.size, state.scale);
    assert!(driver.advance(&mut shared, Instant::now()));
    assert_eq!(driver.ui_scale(), 3.);
}

#[test]
fn occlusion_is_independent_of_resize_and_restore_is_drawable() {
    let mut state = WindowState::default();
    state.update((800, 600), 1.);
    assert!(state.visible());
    state.occluded = true;
    state.update((1600, 1200), 2.);
    assert!(!state.visible());
    state.occluded = false;
    assert!(state.visible());
    state.update((0, 0), 2.);
    assert!(!state.visible());
}

#[test]
fn focus_loss_cancels_a_pressed_pointer_before_the_next_presentation() {
    let (mut driver, mut shared) = rig();
    driver.advance(&mut shared, Instant::now());
    driver.pointer_moved(Point::new(20., 20.), Mods::default());
    driver.button(Button::Primary, true, Mods::default());
    driver.focus(false);
    driver.advance(&mut shared, Instant::now());
    assert_eq!(shared.view.cancels, 1);
    assert_eq!(driver.pointer().pos, None);
    driver.close(&mut shared);
    assert_eq!(shared.view.cancels, 2);
}

#[test]
fn invalid_initial_options_fail_without_creating_an_event_loop() {
    let (_, shared) = rig();
    let result = run_shared(
        Arc::new(Mutex::new(shared)),
        Options {
            size: (0, 600),
            ..Options::default()
        },
    );
    assert!(result.is_err());
    let (_, shared) = rig();
    let result = run_shared(
        Arc::new(Mutex::new(shared)),
        Options {
            poll_interval: Some(Duration::ZERO),
            ..Options::default()
        },
    );
    assert!(result.is_err());
}

#[test]
fn native_key_adaptation_keeps_modifiers_shortcuts_and_composition_distinct() {
    assert_eq!(
        native_key(
            &WinitKey::Named(NamedKey::Shift),
            None,
            Mods::default(),
            false
        ),
        NativeKey::Modifier(Modifier::Shift)
    );
    assert_eq!(
        native_key(
            &WinitKey::Named(NamedKey::ArrowLeft),
            None,
            Mods::default(),
            false
        ),
        NativeKey::Named(Key::Left)
    );
    assert_eq!(
        native_key(
            &WinitKey::Character("e".into()),
            Some("é"),
            Mods::default(),
            false
        ),
        NativeKey::Text("é".into())
    );
    assert_eq!(
        native_key(
            &WinitKey::Character("e".into()),
            Some("é"),
            Mods::default(),
            true
        ),
        NativeKey::Other
    );
    let mods = Mods {
        ctrl: true,
        ..Mods::default()
    };
    assert_eq!(
        native_key(&WinitKey::Character("c".into()), Some("\u{3}"), mods, true),
        NativeKey::Text("c".into())
    );
    assert_eq!(button_of(MouseButton::Back), None);
    assert_eq!(button_of(MouseButton::Left), Some(Button::Primary));
}

struct Editor(String);
impl View for Editor {
    fn build(&mut self, ui: &mut Ui, _: &Input) -> El {
        mui::prelude::text_input(ui, "text", &mut self.0).into()
    }
    fn changed(&mut self) -> bool {
        false
    }
    fn request_resize(&mut self, _: u32, _: u32) -> bool {
        false
    }
}

#[test]
fn native_ime_preedit_stays_tentative_and_commit_is_not_duplicated() {
    let mut driver = Driver::new(
        (800, 600),
        2.,
        Box::new(Clipboard {
            native: None,
            fallback: String::new(),
        }),
    );
    let mut shared = Shared {
        ui: Ui::default(),
        view: Editor(String::new()),
    };
    driver.advance(&mut shared, Instant::now());
    shared.ui.focus("text");
    driver.redraw();
    driver.advance(&mut shared, Instant::now());
    assert!(
        driver.ime_area().is_some(),
        "focused text field supplies native candidate bounds"
    );
    driver.ime(Ime::Enabled);
    driver.ime(Ime::Preedit {
        text: "に".into(),
        cursor: Some((0, 3)),
    });
    for _ in 0..4 {
        driver.advance(&mut shared, Instant::now());
    }
    assert_eq!(shared.view.0, "");
    driver.ime(Ime::Commit("日本".into()));
    for _ in 0..4 {
        driver.advance(&mut shared, Instant::now());
    }
    assert_eq!(shared.view.0, "日本");
    driver.focus(false);
    driver.advance(&mut shared, Instant::now());
    // Losing window focus preserves the UI's focused field for return.
    // An explicit field blur clears the candidate request.
    shared.ui.blur();
    driver.redraw();
    driver.advance(&mut shared, Instant::now());
    assert!(
        driver.ime_area().is_none(),
        "field blur clears candidate request"
    );
}

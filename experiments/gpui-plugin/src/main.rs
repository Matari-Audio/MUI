//! Runnable host-contract check; this is not a DAW certification test.
use mui_gpui_plugin_probe::{GpuiEditor, ProbeParams};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use truce::prelude::{Editor, Params};
use truce_core::editor::{ClosureBridge, PluginContext, RawWindowHandle};
use x11rb::{
    connection::Connection,
    protocol::{Event, xproto::*},
    wrapper::ConnectionExt as _,
};

fn main() -> anyhow::Result<()> {
    let (connection, screen) = x11rb::connect(None)?;
    let root = connection.setup().roots[screen].root;
    let parent = connection.generate_id()?;
    connection
        .create_window(
            x11rb::COPY_DEPTH_FROM_PARENT,
            parent,
            root,
            0,
            0,
            1280,
            720,
            0,
            WindowClass::INPUT_OUTPUT,
            0,
            &CreateWindowAux::new().event_mask(EventMask::STRUCTURE_NOTIFY),
        )?
        .check()?;
    connection.map_window(parent)?.check()?;
    let host_thread = std::thread::current().id();
    let value = Arc::new(Mutex::new(0.0));
    let events = Arc::new(Mutex::new(Vec::new()));
    let params = Arc::new(ProbeParams::default());
    let bridge = Arc::new(ClosureBridge {
        begin_edit: {
            let events = events.clone();
            Box::new(move |_| {
                assert_eq!(std::thread::current().id(), host_thread);
                events.lock().unwrap().push("begin");
            })
        },
        set_param: {
            let events = events.clone();
            let value = value.clone();
            let params = params.clone();
            Box::new(move |id, v| {
                assert_eq!(std::thread::current().id(), host_thread);
                params.set_normalized(id, v);
                *value.lock().unwrap() = v;
                events.lock().unwrap().push("set");
            })
        },
        end_edit: {
            let events = events.clone();
            Box::new(move |_| {
                assert_eq!(std::thread::current().id(), host_thread);
                events.lock().unwrap().push("end");
            })
        },
        get_param: {
            let value = value.clone();
            Box::new(move |_| *value.lock().unwrap())
        },
        get_param_plain: Box::new(|_| 0.0),
        format_param: Box::new(|_| String::new()),
        request_resize: Box::new(|_, _| false),
        get_meter: Box::new(|_| 0.0),
        get_state: Box::new(Vec::new),
        set_state: Box::new(|_| {}),
        transport: Box::new(|| None),
    });
    let second_params = Arc::new(ProbeParams::default());
    let context = PluginContext::new(bridge, params.clone()).dyn_erase();
    let mut first = GpuiEditor::new(params.clone());
    let mut second = GpuiEditor::new(second_params.clone());
    first.open(RawWindowHandle::X11(parent.into()), context.clone());
    anyhow::ensure!(first.last_error.is_none(), "{:?}", first.last_error);
    if std::env::args().any(|arg| arg == "--manual") {
        let protocols = connection
            .intern_atom(false, b"WM_PROTOCOLS")?
            .reply()?
            .atom;
        let delete = connection
            .intern_atom(false, b"WM_DELETE_WINDOW")?
            .reply()?
            .atom;
        connection.change_property32(
            PropMode::REPLACE,
            parent,
            protocols,
            AtomEnum::ATOM,
            &[delete],
        )?;
        connection.change_property8(
            PropMode::REPLACE,
            parent,
            AtomEnum::WM_NAME,
            AtomEnum::STRING,
            b"MUI - interactive GPUI panel",
        )?;
        connection.configure_window(parent, &ConfigureWindowAux::new().width(800).height(450))?;
        anyhow::ensure!(first.set_size(800, 450), "initial resize failed");
        connection.flush()?;
        println!("Manual panel open; close the window to exit.");
        'running: loop {
            while let Some(event) = connection.poll_for_event()? {
                match event {
                    Event::ClientMessage(event)
                        if event.type_ == protocols && event.data.as_data32()[0] == delete =>
                    {
                        break 'running;
                    }
                    Event::DestroyNotify(event) if event.window == parent => break 'running,
                    Event::ConfigureNotify(event) if event.window == parent => {
                        first.set_size(event.width.into(), event.height.into());
                    }
                    _ => {}
                }
            }
            first.idle();
            events.lock().unwrap().clear();
            std::thread::sleep(Duration::from_millis(8));
        }
        first.close();
        return Ok(());
    }
    second.open(
        RawWindowHandle::X11(parent.into()),
        truce_core::editor::for_test_params(second_params.clone()),
    );
    anyhow::ensure!(second.last_error.is_none(), "{:?}", second.last_error);
    let tick = |editor: &mut GpuiEditor| {
        for _ in 0..30 {
            editor.idle();
            std::thread::sleep(Duration::from_millis(10));
        }
    };
    tick(&mut first);
    assert!(first.set_size(320, 180));
    tick(&mut first);
    let snapshot = first.snapshot()?;
    assert!(
        snapshot.render_error.is_none(),
        "minimum-size panel failed: {:?}",
        snapshot.render_error
    );
    let child = first.child.unwrap();
    assert_eq!(connection.query_tree(child)?.reply()?.parent, parent);
    assert!(first.set_size(800, 450));
    assert!(!first.set_size(0, 450));
    tick(&mut first);
    let geometry = connection.get_geometry(child)?.reply()?;
    assert_eq!((geometry.width, geometry.height), (800, 450));
    // XTest goes through the server's real pointer dispatch (GPUI uses XI2).
    connection
        .configure_window(
            child,
            &ConfigureWindowAux::new().stack_mode(StackMode::ABOVE),
        )?
        .check()?;
    connection
        .warp_pointer(0u32, child, 0, 0, 0, 0, 60, 100)?
        .check()?;
    connection.flush()?;
    tick(&mut first);
    x11rb::protocol::xtest::fake_input(
        &connection,
        BUTTON_PRESS_EVENT,
        1,
        x11rb::CURRENT_TIME,
        root,
        0,
        0,
        0,
    )?
    .check()?;
    x11rb::protocol::xtest::fake_input(
        &connection,
        BUTTON_RELEASE_EVENT,
        1,
        x11rb::CURRENT_TIME,
        root,
        0,
        0,
        0,
    )?
    .check()?;
    connection.flush()?;
    tick(&mut first);
    assert_eq!(&*events.lock().unwrap(), &["begin", "set", "end"]);
    assert_eq!(*value.lock().unwrap(), 1.0);
    connection
        .set_input_focus(InputFocus::PARENT, child, x11rb::CURRENT_TIME)?
        .check()?;
    let setup = connection.setup();
    let mapping = connection
        .get_keyboard_mapping(setup.min_keycode, setup.max_keycode - setup.min_keycode + 1)?
        .reply()?;
    let enter = mapping
        .keysyms
        .chunks(mapping.keysyms_per_keycode as usize)
        .position(|keys| keys.contains(&0xff0d))
        .expect("Return key in X11 keymap") as u8
        + setup.min_keycode;
    for kind in [KEY_PRESS_EVENT, KEY_RELEASE_EVENT] {
        x11rb::protocol::xtest::fake_input(
            &connection,
            kind,
            enter,
            x11rb::CURRENT_TIME,
            root,
            0,
            0,
            0,
        )?
        .check()?;
    }
    connection.flush()?;
    tick(&mut first);
    assert_eq!(
        &*events.lock().unwrap(),
        &["begin", "set", "end", "begin", "set", "end"]
    );
    assert_eq!(*value.lock().unwrap(), 0.0);
    let button = |kind, detail| -> anyhow::Result<()> {
        x11rb::protocol::xtest::fake_input(
            &connection,
            kind,
            detail,
            x11rb::CURRENT_TIME,
            root,
            0,
            0,
            0,
        )?
        .check()?;
        connection.flush()?;
        Ok(())
    };
    let move_to = |x, y| -> anyhow::Result<()> {
        connection
            .warp_pointer(0u32, child, 0, 0, 0, 0, x, y)?
            .check()?;
        connection.flush()?;
        Ok(())
    };
    let click = |x, y| -> anyhow::Result<()> {
        move_to(x, y)?;
        button(BUTTON_PRESS_EVENT, 1)?;
        button(BUTTON_RELEASE_EVENT, 1)
    };
    let key = |kind, symbol| -> anyhow::Result<()> {
        let code = mapping
            .keysyms
            .chunks(mapping.keysyms_per_keycode as usize)
            .position(|keys| keys.contains(&symbol))
            .expect("test key in X11 map") as u8
            + setup.min_keycode;
        button(kind, code)
    };
    let type_key = |symbol| -> anyhow::Result<()> {
        key(KEY_PRESS_EVENT, symbol)?;
        key(KEY_RELEASE_EVENT, symbol)
    };
    let assert_gesture = |closed: bool| {
        let events = events.lock().unwrap().clone();
        assert_eq!(events.first(), Some(&"begin"));
        let end = if closed {
            assert_eq!(events.last(), Some(&"end"));
            events.len() - 1
        } else {
            events.len()
        };
        assert!(
            events[1..end].iter().all(|event| *event == "set"),
            "nested or unbalanced gesture: {events:?}"
        );
    };
    events.lock().unwrap().clear();
    // Host/audio updates are read from Truce atomics, never echoed as GUI edits.
    let gain_id = params
        .param_infos()
        .into_iter()
        .find(|p| p.name == "Gain")
        .unwrap()
        .id;
    params.set_normalized(gain_id, 0.37);
    tick(&mut first);
    assert!((first.snapshot()?.normalized_gain - 0.37).abs() < 1e-6);
    assert!(
        events.lock().unwrap().is_empty(),
        "host update echoed as a GUI edit"
    );
    params.set_normalized(gain_id, 0.);
    tick(&mut first);
    // Inside the layout rectangle, outside the rounded painted shape.
    click(17, 57)?;
    tick(&mut first);
    assert!(
        events.lock().unwrap().is_empty(),
        "rounded cutout accepted a click"
    );
    // Drag out of the control and release: capture must end exactly one gesture.
    move_to(100, 85)?;
    button(BUTTON_PRESS_EVENT, 1)?;
    tick(&mut first);
    move_to(250, 150)?;
    tick(&mut first);
    button(BUTTON_RELEASE_EVENT, 1)?;
    tick(&mut first);
    assert_gesture(true);
    assert!(
        (0.01..=1.0).contains(&*value.lock().unwrap()),
        "drag must update normalized gain"
    );
    events.lock().unwrap().clear();
    let before_escape = *value.lock().unwrap();
    // Escape restores the initial value. The following mouse-up must not activate the button.
    move_to(100, 85)?;
    button(BUTTON_PRESS_EVENT, 1)?;
    tick(&mut first);
    move_to(160, 85)?;
    tick(&mut first);
    type_key(0xff1b)?;
    tick(&mut first);
    button(BUTTON_RELEASE_EVENT, 1)?;
    tick(&mut first);
    assert_gesture(true);
    events.lock().unwrap().clear();
    assert!(
        (*value.lock().unwrap() - before_escape).abs() < 1e-6,
        "Escape did not restore the initial parameter value"
    );
    // Switching to another editor must close automation before mouse-up arrives.
    move_to(100, 85)?;
    button(BUTTON_PRESS_EVENT, 1)?;
    tick(&mut first);
    move_to(130, 85)?;
    tick(&mut first);
    assert_gesture(false);
    connection
        .set_input_focus(
            InputFocus::PARENT,
            second.child.unwrap(),
            x11rb::CURRENT_TIME,
        )?
        .check()?;
    tick(&mut first);
    assert_gesture(true);
    let after_blur = events.lock().unwrap().clone();
    button(BUTTON_RELEASE_EVENT, 1)?;
    connection
        .set_input_focus(InputFocus::PARENT, child, x11rb::CURRENT_TIME)?
        .check()?;
    move_to(160, 85)?;
    tick(&mut first);
    assert_eq!(
        *events.lock().unwrap(),
        after_blur,
        "returning pointer resumed a cancelled drag"
    );
    events.lock().unwrap().clear();
    type_key(0xff09)?; // Tab from gain to the text field.
    tick(&mut first);
    for symbol in *b"mui" {
        type_key(u32::from(symbol))?;
    }
    tick(&mut first);
    assert_eq!(first.snapshot()?.preset_name, "mui");
    key(KEY_PRESS_EVENT, 0xffe3)?;
    type_key(u32::from(b'a'))?;
    type_key(u32::from(b'c'))?;
    key(KEY_RELEASE_EVENT, 0xffe3)?;
    type_key(u32::from(b'x'))?;
    tick(&mut first);
    assert_eq!(first.snapshot()?.preset_name, "x", "selection replacement");
    key(KEY_PRESS_EVENT, 0xffe3)?;
    type_key(u32::from(b'a'))?;
    type_key(u32::from(b'v'))?;
    key(KEY_RELEASE_EVENT, 0xffe3)?;
    tick(&mut first);
    assert_eq!(first.snapshot()?.preset_name, "mui", "clipboard paste");
    assert_eq!(
        params.document.snapshot().name,
        "mui",
        "text edits reach Truce persistence"
    );
    assert!(
        second_params.document.snapshot().name.is_empty(),
        "editor instances share state"
    );
    let saved_editor = params.serialize_persist();
    params
        .document
        .edit(|state| {
            state.name = "Changed externally".into();
            Ok(())
        })
        .unwrap();
    tick(&mut first);
    assert_eq!(first.snapshot()?.preset_name, "Changed externally");
    params.load_persist(&saved_editor);
    tick(&mut first);
    assert_eq!(
        first.snapshot()?.preset_name,
        "mui",
        "Truce recall reaches the open editor"
    );

    key(KEY_PRESS_EVENT, 0xffe3)?;
    type_key(u32::from(b'a'))?;
    key(KEY_RELEASE_EVENT, 0xffe3)?;
    type_key(0xff08)?;
    tick(&mut first);
    assert_eq!(first.snapshot()?.preset_name, "", "backspace");
    assert!(
        events.lock().unwrap().is_empty(),
        "typing changed the gain parameter"
    );
    let snapshot = first.snapshot()?;
    assert!(
        snapshot.baseline_error < 0.01,
        "GPUI font baselines differ: {}",
        snapshot.baseline_error
    );
    move_to(100, (snapshot.nested_y + 25.) as i16)?;
    button(BUTTON_PRESS_EVENT, 5)?;
    button(BUTTON_RELEASE_EVENT, 5)?;
    tick(&mut first);
    let snapshot = first.snapshot()?;
    assert!(
        snapshot.inner_scroll_y < -30.,
        "nested viewport did not scroll"
    );
    assert_eq!(
        snapshot.scroll_y, 0.,
        "nested wheel moved the outer viewport"
    );
    assert_eq!(
        snapshot.painted_scroll,
        (snapshot.scroll_y, snapshot.inner_scroll_y),
        "paint/native scroll disagreement"
    );
    let wheel_step = -snapshot.inner_scroll_y;
    // More input exhausts the inner range; only the remainder reaches the outer scroller.
    for _ in 0..5 {
        button(BUTTON_PRESS_EVENT, 5)?;
        button(BUTTON_RELEASE_EVENT, 5)?;
    }
    tick(&mut first);
    let chained = first.snapshot()?;
    assert!(
        (chained.scroll_y + chained.inner_scroll_y + wheel_step * 6.).abs() < 1.,
        "wheel movement was duplicated or lost"
    );
    assert!(
        chained.scroll_y < 0.,
        "nested scroll did not chain at its boundary"
    );
    assert_eq!(
        chained.painted_scroll,
        (chained.scroll_y, chained.inner_scroll_y)
    );
    move_to(700, 400)?;
    for _ in 0..10 {
        button(BUTTON_PRESS_EVENT, 4)?;
        button(BUTTON_RELEASE_EVENT, 4)?;
    }
    tick(&mut first);
    assert_eq!(first.snapshot()?.scroll_y, 0.);
    move_to(100, (first.snapshot()?.nested_y + 25.) as i16)?;
    for _ in 0..10 {
        button(BUTTON_PRESS_EVENT, 4)?;
        button(BUTTON_RELEASE_EVENT, 4)?;
    }
    tick(&mut first);
    assert_eq!(first.snapshot()?.inner_scroll_y, 0.);
    move_to(700, 400)?;
    for _ in 0..10 {
        button(BUTTON_PRESS_EVENT, 5)?;
        button(BUTTON_RELEASE_EVENT, 5)?;
    }
    tick(&mut first);
    assert!(
        first.snapshot()?.scroll_y < -120.,
        "wheel did not scroll content"
    );
    let snapshot = first.snapshot()?;
    assert_eq!(
        snapshot.painted_scroll,
        (snapshot.scroll_y, snapshot.inner_scroll_y),
        "scrolled painting is stale"
    );
    click(100, 85)?;
    tick(&mut first);
    assert!(
        events.lock().unwrap().is_empty(),
        "clipped gain control received a click"
    );
    for _ in 0..10 {
        button(BUTTON_PRESS_EVENT, 4)?;
        button(BUTTON_RELEASE_EVENT, 4)?;
    }
    tick(&mut first);
    assert_eq!(first.snapshot()?.scroll_y, 0.);
    move_to(100, 85)?;
    button(BUTTON_PRESS_EVENT, 1)?;
    tick(&mut first);
    move_to(130, 85)?;
    tick(&mut first);
    assert_gesture(false);
    first.close();
    button(BUTTON_RELEASE_EVENT, 1)?;
    assert_gesture(true);
    anyhow::ensure!(first.last_error.is_none(), "{:?}", first.last_error);
    tick(&mut second);
    assert!(
        connection
            .get_geometry(second.child.unwrap())?
            .reply()
            .is_ok()
    );
    params.load_persist(&saved_editor);
    first.open(RawWindowHandle::X11(parent.into()), context);
    anyhow::ensure!(first.last_error.is_none(), "{:?}", first.last_error);
    tick(&mut first);
    assert_eq!(
        first.snapshot()?.preset_name,
        "mui",
        "state survives editor destruction"
    );
    first.close();
    second.close();
    anyhow::ensure!(
        first.last_error.is_none() && second.last_error.is_none(),
        "close failed: {:?} {:?}",
        first.last_error,
        second.last_error
    );
    connection.destroy_window(parent)?.check()?;
    println!(
        "PASS: real GPUI views, X11 parenting, resize validation, MUI layout/path hits, pointer + keyboard, drag/cancel automation, Tab traversal, text selection/editing/clipboard, GPUI baselines + nested native scroll/MUI clips, close during drag, two instances, close/reopen"
    );
    Ok(())
}

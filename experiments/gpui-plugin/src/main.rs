//! Runnable host-contract check; this is not a DAW certification test.
use mui_gpui_plugin_probe::{GpuiEditor, ProbeParams};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use truce::prelude::Editor;
use truce_core::editor::{ClosureBridge, PluginContext, RawWindowHandle};
use x11rb::{connection::Connection, protocol::xproto::*};

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
            &CreateWindowAux::new(),
        )?
        .check()?;
    connection.map_window(parent)?.check()?;
    let host_thread = std::thread::current().id();
    let value = Arc::new(Mutex::new(0.0));
    let events = Arc::new(Mutex::new(Vec::new()));
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
            Box::new(move |_, v| {
                assert_eq!(std::thread::current().id(), host_thread);
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
    let context = PluginContext::new(bridge, Arc::new(ProbeParams::default())).dyn_erase();
    let mut first = GpuiEditor::default();
    let mut second = GpuiEditor::default();
    first.open(RawWindowHandle::X11(parent.into()), context.clone());
    anyhow::ensure!(first.last_error.is_none(), "{:?}", first.last_error);
    second.open(RawWindowHandle::X11(parent.into()), context.clone());
    anyhow::ensure!(second.last_error.is_none(), "{:?}", second.last_error);
    let tick = |editor: &mut GpuiEditor| {
        for _ in 0..30 {
            editor.idle();
            std::thread::sleep(Duration::from_millis(10));
        }
    };
    tick(&mut first);
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
    first.close();
    anyhow::ensure!(first.last_error.is_none(), "{:?}", first.last_error);
    tick(&mut second);
    assert!(connection
        .get_geometry(second.child.unwrap())?
        .reply()
        .is_ok());
    first.open(RawWindowHandle::X11(parent.into()), context);
    anyhow::ensure!(first.last_error.is_none(), "{:?}", first.last_error);
    tick(&mut first);
    first.close();
    second.close();
    anyhow::ensure!(
        first.last_error.is_none() && second.last_error.is_none(),
        "close failed: {:?} {:?}",
        first.last_error,
        second.last_error
    );
    connection.destroy_window(parent)?.check()?;
    println!("PASS: real GPUI views, X11 parenting, resize validation, pointer + keyboard → host-thread begin/set/end, two instances, close/reopen");
    Ok(())
}

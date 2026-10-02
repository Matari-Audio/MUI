use super::*;
use mui::prelude::{Input, Key, KeyPress, Mods, SemanticAction, row};
use std::sync::Mutex;
use truce::prelude::*;
use truce_core::editor::ClosureBridge;

#[derive(Params)]
struct Values {
    #[param(id = 1, name = "Offset", range = "discrete(-3, 3)", default = -3)]
    offset: IntParam,
    #[param(
        id = 2,
        name = "Frequency",
        range = "log(20, 20000)",
        default = 440.0,
        unit = "Hz"
    )]
    frequency: FloatParam,
    #[param(id = 3, name = "Bypass", default = false)]
    bypass: BoolParam,
    #[param(id = 4, name = "Read only", default = 0.25, flags = "readonly")]
    readonly: FloatParam,
}

#[derive(Debug, PartialEq)]
enum Call {
    Begin(u32),
    Set(u32, f64),
    End(u32),
}
type Log = Arc<Mutex<Vec<Call>>>;
fn bridge() -> (Bridge<Values>, Arc<Values>, Log) {
    let params = Arc::new(Values::default());
    let log = Log::default();
    let (begins, sets, ends, writer, reader) = (
        log.clone(),
        log.clone(),
        log.clone(),
        params.clone(),
        params.clone(),
    );
    let context = PluginContext::from_closures(
        ClosureBridge {
            begin_edit: Box::new(move |id| begins.lock().unwrap().push(Call::Begin(id))),
            end_edit: Box::new(move |id| ends.lock().unwrap().push(Call::End(id))),
            set_param: Box::new(move |id, v| {
                writer.set_normalized(id, v);
                sets.lock().unwrap().push(Call::Set(id, v));
            }),
            get_param: Box::new(move |id| reader.get_normalized(id).unwrap_or(0.0)),
            get_param_plain: Box::new(|_| 0.0),
            format_param: Box::new(|_| String::new()),
            request_resize: Box::new(|_, _| false),
            get_meter: Box::new(|_| 0.0),
            get_state: Box::new(Vec::new),
            set_state: Box::new(|_| {}),
            transport: Box::new(|| None),
        },
        params.clone() as Arc<dyn Params>,
    );
    let mut bridge = Bridge::new(params.clone());
    bridge.attach(context.with_params(params.clone()));
    (bridge, params, log)
}
fn frame(bridge: &mut Bridge<Values>, ui: &mut Ui, input: Input) {
    let tree = row([
        bridge.knob(ui, 1u32).el(),
        bridge.slider(ui, 2u32).el(),
        bridge.toggle(ui, 3u32).el(),
        bridge.knob(ui, 4u32).el(),
        bridge.slider(ui, 999u32).el(),
    ]);
    bridge.end_unbound();
    ui.frame(tree, None, input, 0.016).unwrap();
}
fn key(k: Key) -> Input {
    Input {
        keys: vec![KeyPress {
            key: k,
            mods: Mods::default(),
        }],
        ..Input::default()
    }
}

#[test]
fn metadata_controls_name_format_and_quantize_negative_integer_keys() {
    let (mut bridge, params, log) = bridge();
    let mut ui = Ui::default();
    frame(&mut bridge, &mut ui, Input::default());
    let id = widget_id(1u32);
    let surface = ui.scene().unwrap().surface(&id).unwrap();
    assert_eq!(
        surface.semantics.as_ref().unwrap().label.as_deref(),
        Some("Offset")
    );
    assert_eq!(
        surface.value_description.as_deref(),
        params.format_value(1, -3.0).as_deref()
    );
    assert_eq!(surface.numeric_step, Some(1.0 / 6.0));
    ui.focus(id.clone());
    for (k, expected) in [
        (Key::Left, -3.0),
        (Key::Right, -2.0),
        (Key::Right, -1.0),
        (Key::Right, 0.0),
        (Key::End, 3.0),
        (Key::Right, 3.0),
        (Key::Left, 2.0),
        (Key::Home, -3.0),
    ] {
        let before = params.get_plain(1).unwrap();
        log.lock().unwrap().clear();
        frame(&mut bridge, &mut ui, key(k));
        for _ in 0..3 {
            frame(&mut bridge, &mut ui, Input::default());
        }
        assert_eq!(params.get_plain(1), Some(expected));
        let calls = log.lock().unwrap();
        if before == expected {
            assert!(calls.is_empty());
        } else {
            assert!(
                matches!(
                    calls.as_slice(),
                    [Call::Begin(1), Call::Set(1, _), Call::End(1)]
                ),
                "{calls:?}"
            );
        }
    }
    assert!(ui.request_action(SemanticAction::increment(id.as_str())));
    frame(&mut bridge, &mut ui, Input::default());
    assert_eq!(params.get_plain(1), Some(-2.0));
    assert!(ui.request_action(SemanticAction::decrement(id.as_str())));
    frame(&mut bridge, &mut ui, Input::default());
    assert_eq!(params.get_plain(1), Some(-3.0));
}

#[test]
fn normalized_mapping_keeps_logarithmic_plain_values_and_formatted_units() {
    let (mut bridge, params, _) = bridge();
    let mut ui = Ui::default();
    frame(&mut bridge, &mut ui, Input::default());
    let id = widget_id(2u32);
    assert!(ui.request_action(SemanticAction::set_value(id.as_str(), 0.5)));
    frame(&mut bridge, &mut ui, Input::default());
    let expected = (20.0_f64 * 20_000.0).sqrt();
    assert!((params.get_plain(2).unwrap() - expected).abs() < 0.001);
    let surface = ui.scene().unwrap().surface(&id).unwrap();
    assert_eq!(
        surface.value_description.as_deref(),
        params.format_value(2, expected).as_deref()
    );
    ui.focus(id);
    frame(&mut bridge, &mut ui, key(Key::Right));
    frame(&mut bridge, &mut ui, Input::default());
    assert!((params.get_normalized(2).unwrap() - 0.51).abs() < 1e-6);
}

#[test]
fn readonly_and_unknown_controls_reject_every_semantic_route_and_keys() {
    let (mut bridge, params, log) = bridge();
    let mut ui = Ui::default();
    frame(&mut bridge, &mut ui, Input::default());
    for param in [4u32, 999] {
        let id = widget_id(param);
        assert!(ui.scene().unwrap().surface(&id).unwrap().disabled);
        for action in [
            SemanticAction::focus(id.as_str()),
            SemanticAction::activate(id.as_str()),
            SemanticAction::set_value(id.as_str(), 0.9),
            SemanticAction::increment(id.as_str()),
            SemanticAction::decrement(id.as_str()),
        ] {
            assert!(!ui.request_action(action));
        }
        ui.focus(id);
        frame(&mut bridge, &mut ui, key(Key::Right));
        frame(&mut bridge, &mut ui, Input::default());
    }
    assert_eq!(params.get_normalized(4), Some(0.25));
    assert!(log.lock().unwrap().is_empty());
}

#[test]
fn toggles_activate_once_and_automation_is_not_echoed() {
    let (mut bridge, params, log) = bridge();
    let mut ui = Ui::default();
    frame(&mut bridge, &mut ui, Input::default());
    let id = widget_id(3u32);
    assert!(ui.request_action(SemanticAction::activate(id.as_str())));
    assert!(ui.request_action(SemanticAction::activate(id.as_str())));
    for _ in 0..3 {
        frame(&mut bridge, &mut ui, Input::default());
    }
    assert!(params.bypass.value());
    assert_eq!(
        *log.lock().unwrap(),
        [Call::Begin(3), Call::Set(3, 1.0), Call::End(3)]
    );
    log.lock().unwrap().clear();
    params.set_normalized(3, 0.0);
    frame(&mut bridge, &mut ui, Input::default());
    assert!(!params.bypass.value());
    assert!(log.lock().unwrap().is_empty());
}

#[test]
fn reserves_are_bounded_cached_and_replaced_once() {
    let (mut bridge, _, _) = bridge();
    assert_eq!(
        bridge.readouts[0].len(),
        7,
        "every integer in the small range"
    );
    assert!(
        bridge.readouts[1].len() <= 3,
        "continuous domain has bounded setup work"
    );
    let samples: Arc<[String]> = vec!["999.99 Hz".into(), "20.0 kHz".into()].into();
    assert!(bridge.reserve_readout(2u32, samples.clone()));
    assert!(Arc::ptr_eq(&bridge.readouts[1], &samples));
    assert!(!bridge.reserve_readout(999u32, samples));
    let mut ui = Ui::default();
    frame(&mut bridge, &mut ui, Input::default());
    let original = bridge.readouts[1].clone();
    frame(&mut bridge, &mut ui, Input::default());
    assert!(Arc::ptr_eq(&bridge.readouts[1], &original));
}

#[test]
fn metadata_pointer_gestures_end_on_cancel_unbind_and_repeated_close() {
    use mui::prelude::{Button, Buttons, Point, PointerInput};
    let (mut bridge, _, log) = bridge();
    let mut ui = Ui::default();
    frame(&mut bridge, &mut ui, Input::default());
    let f = ui.scene().unwrap().surface(&widget_id(1u32)).unwrap().frame;
    let p = Point::new(f.x + f.size.width / 2.0, f.y + f.size.height / 2.0);
    let down = || {
        Input::from(PointerInput {
            pos: Some(p),
            buttons: Buttons::default().set(Button::Primary, true),
            ..PointerInput::default()
        })
    };
    for interrupted in 0..3 {
        frame(&mut bridge, &mut ui, down());
        frame(&mut bridge, &mut ui, down());
        assert!(bridge.is_open(1));
        match interrupted {
            0 => {
                ui.cancel();
                ui.cancel();
                for _ in 0..2 {
                    frame(&mut bridge, &mut ui, Input::default());
                }
            }
            1 => {
                bridge.end_unbound();
                ui.frame(row([]), None, Input::default(), 0.016).unwrap();
            }
            _ => {
                bridge.close();
                bridge.close();
            }
        }
        assert!(!bridge.is_open(1));
        assert_eq!(
            *log.lock().unwrap(),
            [Call::Begin(1), Call::End(1)],
            "interruption {interrupted}"
        );
        log.lock().unwrap().clear();
        frame(&mut bridge, &mut ui, Input::default());
    }
}

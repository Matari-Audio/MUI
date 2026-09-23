use mui_truce::{Automation, Document, Edit, Error, Parameter, Target};
use std::sync::{mpsc, Arc, Mutex};
use truce::prelude::*;
use truce_core::{
    custom_state::State,
    editor::{ClosureBridge, PluginContext},
};

#[derive(Params)]
struct Synth {
    #[param(id = 10, name = "Gain", range = "linear(0, 1)", default = 0.5)]
    gain: FloatParam,
    #[param(
        id = 20,
        name = "Voices",
        range = "discrete(1, 8)",
        default = 1,
        parse = "parse_voices"
    )]
    voices: IntParam,
    #[persist]
    editor: Document,
}
impl Synth {
    fn parse_voices(&self, text: &str) -> Option<f64> {
        text.trim().parse().ok()
    }
}
#[derive(Params)]
struct Reordered {
    #[param(id = 20, name = "Voices", range = "discrete(1, 8)", default = 1)]
    voices: IntParam,
    #[param(id = 10, name = "Gain", range = "linear(0, 1)", default = 0.5)]
    gain: FloatParam,
}

#[test]
fn truce_parameters_and_documents_survive_reorder_recall_and_instances() {
    let a = Arc::new(Synth::default());
    let b = Synth::default();
    let writer = a.clone();
    let reader = a.clone();
    let delivered = Arc::new(Mutex::new(Vec::new()));
    let begins = delivered.clone();
    let values = delivered.clone();
    let ends = delivered.clone();
    let context = PluginContext::from_closures(
        ClosureBridge {
            begin_edit: Box::new(move |id| begins.lock().unwrap().push(Edit::Begin(id))),
            end_edit: Box::new(move |id| ends.lock().unwrap().push(Edit::End(id))),
            set_param: Box::new(move |id, value| {
                writer.set_normalized(id, value);
                values.lock().unwrap().push(Edit::Value(id, value));
            }),
            get_param: Box::new(move |id| reader.get_normalized(id).unwrap()),
            get_param_plain: Box::new(|_| 0.),
            format_param: Box::new(|_| String::new()),
            request_resize: Box::new(|_, _| false),
            get_meter: Box::new(|_| 0.),
            get_state: Box::new(Vec::new),
            set_state: Box::new(|_| {}),
            transport: Box::new(|| None),
        },
        a.clone(),
    );
    let (tx, rx) = mpsc::channel();
    let mut automation = Automation::default();
    let mut gain = Parameter::new(a.clone(), 10, true, tx.clone()).unwrap();
    let mut voices = Parameter::new(a.clone(), 20, false, tx).unwrap();
    assert!(gain.begin());
    gain.drag(0.2, false);
    gain.cancel();
    let edits: Vec<_> = rx.try_iter().collect();
    assert_eq!(
        edits,
        [
            Edit::Begin(10),
            Edit::Value(10, 0.7),
            Edit::Value(10, 0.5),
            Edit::End(10)
        ]
    );
    for edit in edits {
        automation.dispatch(&context, edit);
    }
    voices.step(0.51);
    for edit in rx.try_iter() {
        automation.dispatch(&context, edit);
    }
    assert_eq!(a.get_plain(20).unwrap(), 5.);
    assert_eq!(b.get_plain(20).unwrap(), 1.);
    gain.set_enabled(false);
    assert!(!gain.begin());
    assert!(rx.try_recv().is_err());
    gain.set_enabled(true);
    gain.step(0.25);
    for edit in rx.try_iter() {
        automation.dispatch(&context, edit);
    }
    assert_eq!(a.gain.read(), 0.75);
    assert_eq!(b.gain.read(), 0.5);
    let reordered = Reordered::default();
    let (ids, values) = a.collect_values();
    reordered.restore_values(&ids.into_iter().zip(values).collect::<Vec<_>>());
    assert_eq!(reordered.gain.read(), a.gain.read());
    assert_eq!(reordered.get_plain(20).unwrap(), a.get_plain(20).unwrap());
    let (osc, lfo, env, route) = a
        .editor
        .edit(|doc| {
            let osc = doc.add_module("oscillator", vec![10, 20])?;
            let lfo = doc.add_module("lfo", vec![])?;
            let env = doc.add_module("envelope", vec![])?;
            let route = doc.connect(lfo, Target::Parameter(10))?;
            doc.connect(env, Target::Depth(route))?;
            doc.name = "First instance".into();
            doc.chroma = 0.12;
            Ok((osc, lfo, env, route))
        })
        .unwrap();
    let saved = a.serialize_persist();
    assert!(b.editor.snapshot().modules.is_empty());
    b.load_persist(&saved);
    assert_eq!(a.editor.snapshot(), b.editor.snapshot());
    a.editor
        .edit(|doc| {
            doc.modules.reverse();
            Ok(())
        })
        .unwrap();
    assert_eq!(a.editor.snapshot().routes[0].target, Target::Parameter(10));
    let previous = a.editor.snapshot();
    let revision = a.editor.revision();
    assert!(a
        .editor
        .edit(|doc| {
            doc.connect(osc, Target::Input(lfo))?;
            Ok(())
        })
        .is_err());
    assert_eq!(a.editor.snapshot(), previous);
    assert_eq!(a.editor.revision(), revision);
    let mut invalid = previous.clone();
    invalid.version = 99;
    assert!(a.editor.restore(&invalid.serialize()).is_err());
    assert!(a.editor.restore(b"bad").is_err());
    assert_eq!(a.editor.snapshot(), previous);
    a.editor
        .edit(|doc| {
            doc.remove_module(lfo);
            Ok(())
        })
        .unwrap();
    assert!(a.editor.snapshot().routes.iter().all(|r| r.id != route));
    assert!(a.editor.snapshot().routes.is_empty());
    assert!(b.editor.snapshot().modules.iter().any(|m| m.id == lfo));
    let id = a
        .editor
        .edit(|doc| doc.add_module("replacement", vec![]))
        .unwrap();
    assert!(id > env);
    a.editor
        .edit(|doc| {
            doc.remove_module(osc);
            Ok(())
        })
        .unwrap();
    assert!(a
        .editor
        .edit(|doc| doc.add_module("unrelated", vec![10]))
        .is_err());
    assert!(a
        .editor
        .edit(|doc| {
            doc.next_module = 1;
            Ok(())
        })
        .is_err());
    assert!(a
        .editor
        .edit(|doc| {
            doc.retired_parameters.clear();
            Ok(())
        })
        .is_err());
    assert!(voices.parse("3"));
    assert!(!voices.parse("NaN"));
    for edit in rx.try_iter() {
        automation.dispatch(&context, edit);
    }
    assert_eq!(a.get_plain(20), Some(3.));
    gain.reset();
    for edit in rx.try_iter() {
        automation.dispatch(&context, edit);
    }
    assert_eq!(a.gain.read(), 0.5);
    delivered.lock().unwrap().clear();
    for edit in [
        Edit::Begin(10),
        Edit::Begin(10),
        Edit::Value(10, f64::NAN),
        Edit::Value(20, 0.5),
        Edit::Begin(999),
        Edit::Begin(20),
    ] {
        automation.dispatch(&context, edit);
    }
    automation.close(&context);
    automation.close(&context);
    assert_eq!(
        *delivered.lock().unwrap(),
        [
            Edit::Begin(10),
            Edit::Begin(20),
            Edit::End(10),
            Edit::End(20)
        ]
    );
}

#[test]
fn read_sees_the_committed_edit_and_its_revision() {
    let document = Document::default();
    let before = document.revision();
    let id = document
        .edit(|doc| {
            doc.name = "session".into();
            doc.add_module("oscillator", vec![7])
        })
        .unwrap();
    let (name, modules, revision) =
        document.read(|state, revision| (state.name.clone(), state.modules.len(), revision));
    assert_eq!((name.as_str(), modules), ("session", 1));
    assert_eq!(revision, before + 1);
    assert_eq!(document.revision(), revision);
    assert_eq!(document.read(|state, _| state.clone()), document.snapshot());
    // A closure error is the caller's, and leaves the document untouched.
    assert_eq!(
        document.edit(|_| Err::<(), _>(Error::Other("caller said no"))),
        Err(Error::Other("caller said no"))
    );
    assert_eq!(
        document.edit(|doc| {
            doc.version = 99;
            Ok(())
        }),
        Err(Error::UnsupportedVersion(99))
    );
    assert_eq!(document.restore(b"bad"), Err(Error::Malformed));
    assert_eq!(
        document.read(|state, r| (state.modules[0].id, r)),
        (id, revision)
    );
}

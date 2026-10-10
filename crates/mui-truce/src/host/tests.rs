use super::*;
use truce_core::editor::ClosureBridge;

fn host() -> (Arc<dyn EditorBridge>, Arc<Mutex<Vec<Mutation>>>) {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let (begins, sets, ends, sizes) = (calls.clone(), calls.clone(), calls.clone(), calls.clone());
    let bridge = ClosureBridge {
        begin_edit: Box::new(move |id| begins.lock().unwrap().push(Mutation::Begin(id))),
        set_param: Box::new(move |id, value| sets.lock().unwrap().push(Mutation::Set(id, value))),
        end_edit: Box::new(move |id| ends.lock().unwrap().push(Mutation::End(id))),
        request_resize: Box::new(move |w, h| {
            sizes.lock().unwrap().push(Mutation::Resize(w, h));
            true
        }),
        get_param: Box::new(|_| 0.0),
        get_param_plain: Box::new(|_| 0.0),
        format_param: Box::new(|_| String::new()),
        get_meter: Box::new(|_| 0.0),
        get_state: Box::new(Vec::new),
        set_state: Box::new(|_| {}),
        transport: Box::new(|| None),
    };
    (Arc::new(bridge), calls)
}

#[test]
fn sets_coalesce_without_crossing_gesture_edges_or_losing_end() {
    let (host, calls) = host();
    let pump = HostPump::default();
    assert!(pump.attach(host));
    for command in [
        Mutation::Begin(1),
        Mutation::Set(1, 0.2),
        Mutation::Begin(2),
        Mutation::Set(2, 0.3),
        Mutation::Set(1, 0.8),
        Mutation::End(1),
        Mutation::Begin(1),
        Mutation::Set(1, 0.9),
        Mutation::End(2),
        Mutation::End(1),
    ] {
        assert!(pump.enqueue(command));
    }
    assert!(calls.lock().unwrap().is_empty());
    assert!(pump.flush());
    assert_eq!(
        *calls.lock().unwrap(),
        vec![
            Mutation::Begin(1),
            Mutation::Set(1, 0.8),
            Mutation::Begin(2),
            Mutation::Set(2, 0.3),
            Mutation::End(1),
            Mutation::Begin(1),
            Mutation::Set(1, 0.9),
            Mutation::End(2),
            Mutation::End(1)
        ]
    );
}

#[test]
fn wrong_thread_cannot_deliver_and_close_flushes_the_last_end() {
    let (host, calls) = host();
    let pump = Arc::new(HostPump::default());
    assert!(pump.attach(host));
    assert!(pump.enqueue(Mutation::Begin(1)));
    let worker = pump.clone();
    assert!(!thread::spawn(move || worker.flush()).join().unwrap());
    assert!(calls.lock().unwrap().is_empty());
    assert!(pump.flush());
    pump.enqueue(Mutation::Set(1, 0.75));
    pump.close();
    assert!(pump.flush());
    assert_eq!(
        *calls.lock().unwrap(),
        vec![Mutation::Begin(1), Mutation::Set(1, 0.75), Mutation::End(1)]
    );
    assert!(!pump.flush());
}

#[test]
fn callbacks_run_without_the_queue_lock_and_recursive_flush_is_deferred() {
    let pump = Arc::new(HostPump::default());
    let reentrant = pump.clone();
    let host = Arc::new(ClosureBridge {
        begin_edit: Box::new(move |id| {
            assert!(reentrant.state.try_lock().is_ok());
            assert!(!reentrant.flush());
            reentrant.enqueue(Mutation::End(id));
        }),
        set_param: Box::new(|_, _| {}),
        end_edit: Box::new(|_| {}),
        request_resize: Box::new(|_, _| true),
        get_param: Box::new(|_| 0.0),
        get_param_plain: Box::new(|_| 0.0),
        format_param: Box::new(|_| String::new()),
        get_meter: Box::new(|_| 0.0),
        get_state: Box::new(Vec::new),
        set_state: Box::new(|_| {}),
        transport: Box::new(|| None),
    }) as Arc<dyn EditorBridge>;
    assert!(pump.attach(host));
    pump.enqueue(Mutation::Begin(1));
    assert!(pump.flush());
    assert_eq!(
        pump.state.lock().unwrap().pending.front(),
        Some(&Mutation::End(1))
    );
    assert!(pump.flush());
    assert!(pump.state.lock().unwrap().delivered.is_empty());
    pump.detach(); // Break the test host's Arc cycle.
}

#[test]
fn queue_overflow_ends_delivered_gestures_instead_of_dropping_their_end() {
    let (host, calls) = host();
    let pump = HostPump::default();
    assert!(pump.attach(host));
    pump.enqueue(Mutation::Begin(42));
    pump.flush();
    for _ in 0..MAX_PENDING / 2 {
        pump.enqueue(Mutation::Begin(1));
        pump.enqueue(Mutation::End(1));
    }
    assert!(!pump.enqueue(Mutation::Set(42, 0.5)));
    assert!(!pump.enqueue(Mutation::End(42)));
    assert!(!pump.flush());
    assert_eq!(
        *calls.lock().unwrap(),
        vec![Mutation::Begin(42), Mutation::End(42)]
    );
    assert!(pump.state.lock().unwrap().host.is_none());
}

#[test]
fn a_callback_panic_does_not_skip_teardown_or_the_other_gestures() {
    let (host, calls) = host();
    let ends = calls.clone();
    let base = host.clone();
    let host = Arc::new(ClosureBridge {
        begin_edit: Box::new(move |id| base.begin_edit(id)),
        set_param: Box::new(|_, _| panic!("host edit")),
        end_edit: Box::new(move |id| ends.lock().unwrap().push(Mutation::End(id))),
        request_resize: Box::new(|_, _| true),
        get_param: Box::new(|_| 0.0),
        get_param_plain: Box::new(|_| 0.0),
        format_param: Box::new(|_| String::new()),
        get_meter: Box::new(|_| 0.0),
        get_state: Box::new(Vec::new),
        set_state: Box::new(|_| {}),
        transport: Box::new(|| None),
    }) as Arc<dyn EditorBridge>;
    let pump = HostPump::default();
    assert!(pump.attach(host));
    pump.enqueue(Mutation::Begin(1));
    pump.enqueue(Mutation::Begin(2));
    pump.enqueue(Mutation::Set(1, 0.3));
    pump.enqueue(Mutation::End(1));
    assert!(!pump.flush());
    assert_eq!(
        *calls.lock().unwrap(),
        vec![
            Mutation::Begin(1),
            Mutation::Begin(2),
            Mutation::End(1),
            Mutation::End(2)
        ]
    );
    assert!(pump.state.lock().unwrap().host.is_none());
}

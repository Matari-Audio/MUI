//! Owned-native registration fixture, not a transfer adapter.
//! X11: env -u WAYLAND_DISPLAY -u WAYLAND_SOCKET MUI_RENDERER=cpu xvfb-run -a
//! cargo run --locked -p mui-winit --features external-xdnd --example external_xdnd
//! Wayland: run under an owned compositor with DISPLAY unset and
//! MUI_XDND_EXPECT_UNSUPPORTED=1; both ordinary windows must still present.

#[cfg(target_os = "linux")]
mod native {
    use std::sync::{Arc, Mutex, mpsc};
    use std::time::Duration;

    use mui::prelude::*;
    use mui_winit::external_xdnd::{self, Host, NativePhase, Phase, Worker};
    use mui_winit::{HostEvent, Options, View, WindowController, WindowSpec, WindowToken};
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{self, AtomEnum, ConnectionExt, PropMode};
    use x11rb::rust_connection::RustConnection;
    use x11rb::wrapper::ConnectionExt as _;

    struct Pad;

    impl View for Pad {
        fn build(&mut self, _: &mut Ui, _: &Input) -> El {
            block(180., 120.).id("external-xdnd-native-fixture")
        }
        fn changed(&mut self) -> bool {
            false
        }
        fn request_resize(&mut self, _: u32, _: u32) -> bool {
            false
        }
    }

    enum Event {
        Presented(WindowToken),
        Closed(WindowToken),
    }

    #[derive(Default)]
    struct FixtureState {
        stopping: bool,
        tokens: Vec<WindowToken>,
    }

    fn window(
        index: usize,
        host: Host,
        events: mpsc::Sender<Event>,
        fixture: Arc<Mutex<FixtureState>>,
    ) -> WindowSpec<Pad> {
        let mut presented = false;
        WindowSpec::new(
            Pad,
            Ui::default(),
            Options {
                title: format!("MUI external XDND lifecycle {}", index + 1),
                size: (320, 240),
                external_xdnd: Some(host.clone()),
                ..Options::default()
            },
        )
        .on_event(move |token, event, controller| match event {
            HostEvent::Presented { .. } if !presented => {
                presented = true;
                events.send(Event::Presented(token)).unwrap();
            }
            HostEvent::Closed => {
                // App must revoke/drop the host guard before its observer runs.
                assert_eq!(host.status().phase, Phase::Revoked);
                events.send(Event::Closed(token)).unwrap();
                let mut state = fixture.lock().unwrap();
                if index == 0 && !state.stopping {
                    let host = host.clone();
                    let events = events.clone();
                    let fixture = Arc::clone(&fixture);
                    let token = controller
                        .open(move || window(1, host, events, fixture))
                        .unwrap();
                    state.tokens.push(token);
                }
            }
            _ => {}
        })
    }

    fn proxy(peer: &RustConnection, challenge: &external_xdnd::Challenge) -> Result<u32, String> {
        let id = peer.generate_id().map_err(|error| error.to_string())?;
        peer.create_window(
            0,
            id,
            challenge.root(),
            0,
            0,
            1,
            1,
            0,
            xproto::WindowClass::INPUT_ONLY,
            0,
            &xproto::CreateWindowAux::new(),
        )
        .map_err(|error| error.to_string())?
        .check()
        .map_err(|error| error.to_string())?;
        let atom = |name: &[u8]| -> Result<u32, String> {
            peer.intern_atom(false, name)
                .map_err(|error| error.to_string())?
                .reply()
                .map(|reply| reply.atom)
                .map_err(|error| error.to_string())
        };
        for (name, proof) in [
            (challenge.creation_property(), challenge.creation_proof()),
            (
                challenge.registration_property(),
                challenge.registration_proof(),
            ),
        ] {
            let atom = atom(name.as_bytes())?;
            peer.change_property8(PropMode::REPLACE, id, atom, atom, proof)
                .map_err(|error| error.to_string())?
                .check()
                .map_err(|error| error.to_string())?;
        }
        for (name, type_, value) in [
            (b"XdndProxy".as_slice(), AtomEnum::WINDOW, id),
            (b"XdndAware".as_slice(), AtomEnum::ATOM, 5),
        ] {
            peer.change_property32(PropMode::REPLACE, id, atom(name)?, type_, &[value])
                .map_err(|error| error.to_string())?
                .check()
                .map_err(|error| error.to_string())?;
        }
        Ok(id)
    }

    fn backend(
        worker: &Worker,
        events: &mpsc::Receiver<Event>,
        controller: &WindowController<Pad>,
        anchor: WindowToken,
        unsupported: bool,
    ) -> Result<usize, String> {
        let mut previous = None;
        for _ in 0..2 {
            let Event::Presented(token) = events
                .recv_timeout(Duration::from_secs(20))
                .map_err(|error| error.to_string())?
            else {
                return Err("host closed before first native presentation".into());
            };
            let (id, mut registration, peer) = if unsupported {
                if worker.status().phase != Phase::UnsupportedBackend || worker.poll().is_some() {
                    return Err(
                        "unsupported backend must remain usable without an invitation".into(),
                    );
                }
                (
                    worker
                        .status()
                        .attachment
                        .ok_or("missing unsupported host generation")?,
                    None,
                    None,
                )
            } else {
                let invitation = worker.poll().ok_or("missing actual-window invitation")?;
                let id = invitation.id();
                if !worker.is_current(id) || previous == Some(id) {
                    return Err("stale/reused native host generation".into());
                }
                let (peer, _) = x11rb::connect(Some(invitation.challenge().display()))
                    .map_err(|error| error.to_string())?;
                let proxy = proxy(&peer, invitation.challenge())?;
                let mut registration = invitation.connect().map_err(|error| error.to_string())?;
                registration
                    .install_proxy(proxy)
                    .map_err(|error| error.to_string())?;
                if worker.status().phase != Phase::Registered {
                    return Err("typed host did not observe acknowledged registration".into());
                }
                (id, Some(registration), Some(peer))
            };
            if let Some(old) = previous
                && worker.is_current(old)
            {
                return Err("old attachment ID remained current after recreation".into());
            }
            controller.close(token)?;
            let Event::Closed(closed) = events
                .recv_timeout(Duration::from_secs(20))
                .map_err(|error| error.to_string())?
            else {
                return Err("expected closed notification before replacement frame".into());
            };
            if closed != token || worker.is_current(id) {
                return Err("closed host did not revoke its exact generation".into());
            }
            if let Some(registration) = &mut registration {
                let phase = registration.detach().map_err(|error| error.to_string())?;
                if !matches!(phase, NativePhase::Detached | NativePhase::Gone) {
                    return Err(format!("native cleanup not established: {phase:?}"));
                }
            }
            // Retain the proxy connection until matching detach is established.
            drop(peer);
            previous = Some(id);
        }
        controller.close(anchor)?;
        Ok(2)
    }

    pub fn run() -> Result<(), String> {
        let unsupported = std::env::var("MUI_XDND_EXPECT_UNSUPPORTED").as_deref() == Ok("1");
        let (host, worker) = external_xdnd::channel().map_err(|error| error.to_string())?;
        let (send, receive) = mpsc::channel();
        let fixture = Arc::new(Mutex::new(FixtureState::default()));
        let windows = vec![
            WindowSpec::new(
                Pad,
                Ui::default(),
                Options {
                    title: "MUI lifecycle anchor".into(),
                    size: (120, 80),
                    ..Options::default()
                },
            ),
            window(0, host, send, Arc::clone(&fixture)),
        ];
        let mut backend_worker = None;
        let result = mui_winit::run_windows(windows, |controller, tokens| {
            let anchor = tokens[0];
            fixture.lock().unwrap().tokens = tokens;
            backend_worker = Some(std::thread::spawn(move || {
                let result = backend(&worker, &receive, &controller, anchor, unsupported);
                if result.is_err() {
                    // Fail fixture closed; never synthesize an accepted transfer.
                    let tokens = {
                        let mut fixture = fixture.lock().unwrap();
                        fixture.stopping = true;
                        fixture.tokens.clone()
                    };
                    for token in tokens {
                        let _ = controller.close(token);
                    }
                }
                result
            }));
        });
        // Fixture join occurs after the event loop; production hook Drop never joins.
        let count = backend_worker
            .ok_or("backend fixture did not start")?
            .join()
            .map_err(|_| "backend fixture panicked")??;
        result?;
        if count != 2 {
            return Err("native lifecycle fixture incomplete".into());
        }
        println!(
            "two actual MUI hosts presented, attached/recreated/revoked; unsupported={unsupported}; no payload transferred"
        );
        Ok(())
    }
}

#[cfg(target_os = "linux")]
fn main() -> Result<(), String> {
    native::run()
}

#[cfg(not(target_os = "linux"))]
fn main() {
    println!("external XDND registration fixture is Linux-only");
}

use std::{
    io::{BufRead, BufReader, Write},
    process::{Child, Command, Stdio},
    sync::{Mutex, atomic::AtomicU32},
    time::Instant,
};

use super::*;
use crate::DialogFilter;

struct Recorded {
    method: &'static str,
    parent: String,
    title: String,
    options: HashMap<String, OwnedValue>,
}

struct FakeChooser {
    mode: Arc<AtomicU32>,
    version: Arc<AtomicU32>,
    opened: mpsc::Sender<Recorded>,
    closed: mpsc::Sender<String>,
    sequence: Mutex<u32>,
}

impl FakeChooser {
    async fn open(
        &self,
        method: &'static str,
        parent: String,
        title: String,
        options: HashMap<String, OwnedValue>,
        connection: &Connection,
    ) -> zbus::fdo::Result<OwnedObjectPath> {
        let mode = self.mode.load(Ordering::Acquire);
        if mode == 5 {
            return Err(zbus::fdo::Error::NotSupported(
                "No desktop file chooser backend".into(),
            ));
        }
        let sequence = {
            let mut sequence = self.sequence.lock().unwrap();
            *sequence += 1;
            *sequence
        };
        // Deliberately ignore handle_token: test legacy returned-path handling.
        let path = format!("{PORTAL_PATH}/request/legacy/request{sequence}");
        connection
            .object_server()
            .at(
                path.clone(),
                FakeRequest {
                    path: path.clone(),
                    closed: self.closed.clone(),
                },
            )
            .await?;
        self.opened
            .send(Recorded {
                method,
                parent,
                title,
                options,
            })
            .unwrap();
        if mode != 6 {
            // An unrelated request must never provide this job's answer.
            let wrong = HashMap::from([("uris", Value::from(vec!["file:///wrong".to_string()]))]);
            connection
                .emit_signal(
                    None::<&str>,
                    format!("{PORTAL_PATH}/request/unrelated"),
                    REQUEST,
                    "Response",
                    &(0_u32, wrong),
                )
                .await?;
            let mut results = HashMap::new();
            let code = match mode {
                1 => 1_u32,
                2 => 2,
                _ => 0,
            };
            if mode == 4 {
                results.insert("uris", Value::from(true));
            } else if mode != 9 {
                let uris = match mode {
                    3 => vec!["https://example.com/not-a-file".to_string()],
                    7 => vec!["file:///one".to_string(), "file:///two".to_string()],
                    8 => vec![],
                    10 => vec!["file:///tmp/bad%ZZ".to_string()],
                    11 => vec!["file:///tmp/bad%00".to_string()],
                    12 => vec!["file:///tmp/raw space".to_string()],
                    13 => vec!["file://remote.example/file".to_string()],
                    _ => vec!["file:///tmp/test%20file.wav".to_string()],
                };
                results.insert("uris", Value::from(uris));
            }
            // Emit before replying to OpenFile/SaveFile: the response race matters.
            connection
                .emit_signal(
                    None::<&str>,
                    path.as_str(),
                    REQUEST,
                    "Response",
                    &(code, results),
                )
                .await?;
        }
        OwnedObjectPath::try_from(path).map_err(|error| zbus::fdo::Error::Failed(error.to_string()))
    }
}

#[zbus::interface(name = "org.freedesktop.portal.FileChooser")]
impl FakeChooser {
    async fn open_file(
        &self,
        parent: String,
        title: String,
        options: HashMap<String, OwnedValue>,
        #[zbus(connection)] connection: &Connection,
    ) -> zbus::fdo::Result<OwnedObjectPath> {
        self.open("OpenFile", parent, title, options, connection)
            .await
    }

    async fn save_file(
        &self,
        parent: String,
        title: String,
        options: HashMap<String, OwnedValue>,
        #[zbus(connection)] connection: &Connection,
    ) -> zbus::fdo::Result<OwnedObjectPath> {
        self.open("SaveFile", parent, title, options, connection)
            .await
    }

    #[zbus(property, name = "version")]
    fn version(&self) -> u32 {
        self.version.load(Ordering::Acquire)
    }
}

struct FakeRequest {
    path: String,
    closed: mpsc::Sender<String>,
}

#[zbus::interface(name = "org.freedesktop.portal.Request")]
impl FakeRequest {
    fn close(&self) {
        self.closed.send(self.path.clone()).unwrap();
    }
}

struct Portal {
    service: DialogService,
    runtime: tokio::runtime::Runtime,
    bus: Child,
    connection: Connection,
    address: String,
    mode: Arc<AtomicU32>,
    version: Arc<AtomicU32>,
    opened: mpsc::Receiver<Recorded>,
    closed: mpsc::Receiver<String>,
}

impl Portal {
    fn new() -> Self {
        // No service directories: missing fake portals must not autoactivate
        // the machine's actual desktop picker on this private test bus.
        let mut config = tempfile::NamedTempFile::new().unwrap();
        config
            .write_all(
                br#"<busconfig>
          <type>session</type><listen>unix:tmpdir=/tmp</listen><auth>EXTERNAL</auth>
          <policy context="default"><allow send_destination="*"/>
          <allow receive_sender="*"/><allow own="*"/></policy>
        </busconfig>"#,
            )
            .unwrap();
        let mut bus = Command::new("dbus-daemon")
            .arg("--config-file")
            .arg(config.path())
            .args(["--nofork", "--print-address=1"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("private dbus-daemon");
        let mut address = String::new();
        BufReader::new(bus.stdout.take().unwrap())
            .read_line(&mut address)
            .unwrap();
        let address = address.trim().to_owned();
        let mode = Arc::new(AtomicU32::new(0));
        let version = Arc::new(AtomicU32::new(4));
        let (opened, records) = mpsc::channel();
        let (closed, closes) = mpsc::channel();
        let chooser = FakeChooser {
            mode: Arc::clone(&mode),
            version: Arc::clone(&version),
            opened,
            closed,
            sequence: Mutex::new(0),
        };
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap();
        let connection = runtime
            .block_on(
                zbus::connection::Builder::address(address.as_str())
                    .unwrap()
                    .name(SERVICE)
                    .unwrap()
                    .serve_at(PORTAL_PATH, chooser)
                    .unwrap()
                    .build(),
            )
            .unwrap();
        Self {
            service: DialogService::default(),
            runtime,
            bus,
            connection,
            address,
            mode,
            version,
            opened: records,
            closed: closes,
        }
    }

    fn start(&self, request: DialogRequest) -> DialogJob {
        self.service
            .spawn_at(request, Some(self.address.clone()))
            .unwrap()
    }
}

impl Drop for Portal {
    fn drop(&mut self) {
        self.service.shutdown();
        self.runtime
            .block_on(self.connection.clone().close())
            .unwrap();
        self.bus.kill().unwrap();
        self.bus.wait().unwrap();
    }
}

fn request(kind: DialogKind) -> DialogRequest {
    DialogRequest {
        parent: Some(Parent::X11(0xabcd)),
        title: "Select test file".into(),
        kind,
        directory: Some("/tmp/test folder".into()),
        filters: vec![DialogFilter {
            name: "Audio".into(),
            extensions: vec!["wav".into(), "flac".into()],
        }],
    }
}

fn answer(job: &DialogJob) -> DialogResult {
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(result) = job.try_result() {
            return result;
        }
        assert!(Instant::now() < until, "file picker did not finish");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn string(value: &OwnedValue) -> String {
    String::try_from(value.try_clone().unwrap()).unwrap()
}

#[test]
fn private_portal_preserves_options_and_handles_early_legacy_responses() {
    let portal = Portal::new();
    for kind in [
        DialogKind::OpenFile { multiple: true },
        DialogKind::SaveFile {
            file_name: Some("test.wav".into()),
        },
        DialogKind::PickFolder { multiple: false },
    ] {
        let mut request = request(kind.clone());
        if matches!(kind, DialogKind::SaveFile { .. }) {
            request.parent = Some(Parent::Wayland("exported-handle".into()));
        }
        let job = portal.start(request);
        assert_eq!(
            answer(&job).unwrap(),
            Some(vec![PathBuf::from("/tmp/test file.wav")])
        );
        let record = portal.opened.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(record.title, "Select test file");
        assert_eq!(
            record.parent,
            if matches!(kind, DialogKind::SaveFile { .. }) {
                "wayland:exported-handle"
            } else {
                "x11:abcd"
            }
        );
        assert!(bool::try_from(record.options["modal"].try_clone().unwrap()).unwrap());
        let token = string(&record.options["handle_token"]);
        assert!(
            token.starts_with("mui_")
                && token
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        );
        assert_eq!(
            Vec::<u8>::try_from(record.options["current_folder"].try_clone().unwrap()).unwrap(),
            b"/tmp/test folder\0"
        );
        let filters = Vec::<(String, Vec<(u32, String)>)>::try_from(
            record.options["filters"].try_clone().unwrap(),
        )
        .unwrap();
        assert_eq!(
            filters,
            vec![(
                "Audio".into(),
                vec![(0, "*.wav".into()), (0, "*.flac".into())]
            )]
        );
        match kind {
            DialogKind::SaveFile { .. } => {
                assert_eq!(record.method, "SaveFile");
                assert_eq!(string(&record.options["current_name"]), "test.wav");
            }
            DialogKind::OpenFile { .. } | DialogKind::PickFolder { .. } => {
                assert_eq!(record.method, "OpenFile");
                assert_eq!(
                    bool::try_from(record.options["directory"].try_clone().unwrap()).unwrap(),
                    matches!(kind, DialogKind::PickFolder { .. })
                );
            }
        }
    }
}

#[test]
fn cancel_and_drop_close_the_exact_request_without_waiting_for_response() {
    let portal = Portal::new();
    portal.mode.store(6, Ordering::Release);
    let job = portal.start(request(DialogKind::OpenFile { multiple: false }));
    portal.opened.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(job.try_result().is_none());
    let cancel = job.cancel_handle();
    let began = Instant::now();
    cancel.cancel();
    cancel.cancel();
    assert!(began.elapsed() < Duration::from_millis(50));
    assert!(
        portal
            .closed
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .ends_with("/legacy/request1")
    );
    assert!(
        job.try_result().is_none(),
        "cancelled jobs must never deliver a stale result"
    );
    let next = portal.start(request(DialogKind::PickFolder { multiple: false }));
    portal.opened.recv_timeout(Duration::from_secs(2)).unwrap();
    let began = Instant::now();
    drop(next);
    assert!(began.elapsed() < Duration::from_millis(50));
    assert!(
        portal
            .closed
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .ends_with("/legacy/request2")
    );
}

#[test]
fn cancellation_is_distinct_from_backend_and_invalid_response_errors() {
    let portal = Portal::new();
    portal.mode.store(1, Ordering::Release);
    assert_eq!(
        answer(&portal.start(request(DialogKind::OpenFile { multiple: false }))).unwrap(),
        None
    );
    for mode in [2, 3, 4, 5, 7, 8, 9, 10, 11, 12, 13] {
        portal.mode.store(mode, Ordering::Release);
        assert!(
            answer(&portal.start(request(DialogKind::OpenFile { multiple: false }))).is_err(),
            "mode {mode}"
        );
    }
    portal.version.store(2, Ordering::Release);
    let error =
        answer(&portal.start(request(DialogKind::PickFolder { multiple: false }))).unwrap_err();
    assert!(error.contains("version 3"), "{error}");
}

#[test]
fn service_shutdown_drains_workers_and_rejects_new_requests() {
    let portal = Portal::new();
    portal.mode.store(6, Ordering::Release);
    let first = portal.start(request(DialogKind::OpenFile { multiple: false }));
    portal.opened.recv_timeout(Duration::from_secs(2)).unwrap();
    let second = portal.start(request(DialogKind::PickFolder { multiple: false }));
    portal.opened.recv_timeout(Duration::from_secs(2)).unwrap();
    portal.service.shutdown();
    let mut paths = [
        portal.closed.recv_timeout(Duration::from_secs(1)).unwrap(),
        portal.closed.recv_timeout(Duration::from_secs(1)).unwrap(),
    ];
    paths.sort();
    assert!(paths[0].ends_with("/legacy/request1"));
    assert!(paths[1].ends_with("/legacy/request2"));
    // Sender is dropped only after the owned runtime was dropped and worker
    // finished. Explicit shutdown also works with live Job/CancelHandle owners.
    for job in [&first, &second] {
        assert!(matches!(
            job.result.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));
        assert!(job.try_result().is_none());
        assert!(job.is_cancelled());
        assert!(job.is_finished());
    }
    portal.service.shutdown();
    assert!(
        portal
            .service
            .spawn(request(DialogKind::OpenFile { multiple: false }))
            .is_err()
    );
}

#[test]
fn missing_portal_and_backend_disappearance_are_errors() {
    let portal = Portal::new();
    portal.mode.store(6, Ordering::Release);
    let pending = portal.start(request(DialogKind::OpenFile { multiple: false }));
    portal.opened.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(
        portal
            .runtime
            .block_on(portal.connection.release_name(SERVICE))
            .unwrap()
    );
    assert!(answer(&pending).is_err());
    let began = Instant::now();
    assert!(answer(&portal.start(request(DialogKind::OpenFile { multiple: false }))).is_err());
    assert!(began.elapsed() < Duration::from_secs(5));
}

#[test]
fn locally_cancelled_jobs_discard_queued_success() {
    let portal = Portal::new();
    let job = portal.start(request(DialogKind::OpenFile { multiple: false }));
    portal.opened.recv_timeout(Duration::from_secs(2)).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while !job.is_finished() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    // Success is already queued, but a window close must invalidate it.
    job.cancel_handle().cancel();
    assert!(job.is_cancelled());
    assert!(job.try_result().is_none());
}

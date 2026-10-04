use std::{
    collections::HashMap,
    ffi::CString,
    io::Read,
    os::unix::ffi::OsStrExt,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::JoinHandle,
    time::Duration,
};

use async_channel::{Receiver, Sender};
use futures_lite::{StreamExt, future};
use zbus::{
    Connection, MatchRule, MessageStream,
    message::Type,
    zvariant::{OwnedObjectPath, OwnedValue, Value},
};

use crate::{DialogKind, DialogRequest, DialogResult, Parent};

const SERVICE: &str = "org.freedesktop.portal.Desktop";
const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
const CHOOSER: &str = "org.freedesktop.portal.FileChooser";
const REQUEST: &str = "org.freedesktop.portal.Request";
// Only D-Bus setup/method calls/cleanup have deadlines, never user selection.
const CALL_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone)]
pub struct CancelHandle {
    cancelled: Arc<AtomicBool>,
    send: Sender<()>,
}

impl CancelHandle {
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    /// Nonblocking and idempotent, including from a host's close callback.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        let _ = self.send.try_send(());
    }
}

pub struct DialogJob {
    cancel: CancelHandle,
    result: mpsc::Receiver<DialogResult>,
    finished: Arc<AtomicBool>,
}

struct Completion(Arc<AtomicBool>);
impl Drop for Completion {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

/// Owns every dialog worker until final plugin teardown. Keep this outside the
/// GUI lifetime; closing a window only cancels its job.
#[derive(Default)]
pub struct DialogService {
    registry: Mutex<Registry>,
    draining: Mutex<()>,
}

#[derive(Default)]
struct Registry {
    closed: bool,
    workers: Vec<(CancelHandle, JoinHandle<()>)>,
}

impl DialogService {
    pub fn spawn(&self, request: DialogRequest) -> Result<DialogJob, String> {
        self.spawn_at(request, None)
    }

    fn spawn_at(
        &self,
        request: DialogRequest,
        address: Option<String>,
    ) -> Result<DialogJob, String> {
        let mut registry = self
            .registry
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if registry.closed {
            return Err("File dialog service has shut down".into());
        }
        let mut index = 0;
        while index < registry.workers.len() {
            if registry.workers[index].1.is_finished() {
                let (_, worker) = registry.workers.swap_remove(index);
                let _ = worker.join();
            } else {
                index += 1;
            }
        }
        let (send, receive) = async_channel::bounded(1);
        let cancel = CancelHandle {
            cancelled: Arc::default(),
            send,
        };
        let cancelled = Arc::clone(&cancel.cancelled);
        let (result, answer) = mpsc::channel();
        let finished = Arc::new(AtomicBool::new(false));
        let completion = Completion(Arc::clone(&finished));
        let worker = std::thread::Builder::new()
            .name("mui-file-dialog".into())
            .spawn(move || {
                let _completion = completion;
                // A per-worker runtime owns its tasks/blocking pool. async-io's
                // permanent global reactor thread cannot survive plugin unload.
                let outcome = tokio::runtime::Builder::new_current_thread()
                    .enable_io()
                    .enable_time()
                    .build()
                    .map_err(|error| format!("Could not start file picker runtime: {error}"))
                    .and_then(|runtime| {
                        runtime.block_on(run(request, address.as_deref(), receive))
                    });
                if !cancelled.load(Ordering::Acquire) {
                    let _ = result.send(outcome);
                }
            })
            .map_err(|error| format!("Could not start file dialog: {error}"))?;
        registry.workers.push((cancel.clone(), worker));
        Ok(DialogJob {
            cancel,
            result: answer,
            finished,
        })
    }

    /// Cancels and joins workers before plugin code is unloaded. This waits for
    /// cleanup, so call it at plugin teardown, never from a GUI close callback.
    /// Idempotent; further spawn calls fail, even when other Arc owners remain.
    pub fn shutdown(&self) {
        let _drain = self
            .draining
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let workers = {
            let mut registry = self
                .registry
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            registry.closed = true;
            std::mem::take(&mut registry.workers)
        };
        for (cancel, _) in &workers {
            cancel.cancel();
        }
        for (_, worker) in workers {
            let _ = worker.join();
        }
    }
}

impl Drop for DialogService {
    fn drop(&mut self) {
        self.shutdown();
    }
}

impl DialogJob {
    /// Cancellation suppresses result delivery; it is distinct from a pending
    /// job. Workers polling this job must stop when cancellation is requested.
    pub fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }

    /// The owned runtime has finished cleanup, including on worker panic.
    /// Final plugin teardown still calls service.shutdown() to join workers.
    pub fn is_finished(&self) -> bool {
        self.finished.load(Ordering::Acquire)
    }

    pub fn cancel_handle(&self) -> CancelHandle {
        self.cancel.clone()
    }

    pub fn cancel(&self) {
        self.cancel.cancel();
    }

    /// Poll once per GUI tick. None means pending or locally cancelled; check
    /// is_cancelled/is_finished when polling from another owned worker.
    pub fn try_result(&self) -> Option<DialogResult> {
        if self.cancel.cancelled.load(Ordering::Acquire) {
            return None;
        }
        match self.result.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                Some(Err("File dialog worker disconnected".into()))
            }
        }
    }
}

impl Drop for DialogJob {
    fn drop(&mut self) {
        self.cancel();
    }
}

async fn bounded<T>(
    work: impl std::future::Future<Output = Result<T, String>>,
) -> Result<T, String> {
    future::race(work, async {
        tokio::time::sleep(CALL_TIMEOUT).await;
        Err("Desktop file picker did not respond to a D-Bus method call".into())
    })
    .await
}

fn parent_identifier(parent: Option<&Parent>) -> Result<String, String> {
    match parent {
        None => Ok(String::new()),
        Some(Parent::X11(0)) => Err("File picker parent has no X11 window".into()),
        Some(Parent::X11(window)) => Ok(format!("x11:{window:x}")),
        Some(Parent::Wayland(handle)) if !handle.is_empty() && !handle.contains('\0') => {
            Ok(format!("wayland:{handle}"))
        }
        Some(Parent::Wayland(_)) => Err("File picker parent has no exported Wayland handle".into()),
    }
}

fn options(
    request: &DialogRequest,
    token: String,
) -> Result<HashMap<&'static str, Value<'static>>, String> {
    let mut options = HashMap::from([
        ("handle_token", Value::from(token)),
        ("modal", Value::from(true)),
    ]);
    match &request.kind {
        DialogKind::OpenFile { multiple } | DialogKind::PickFolder { multiple } => {
            options.insert("multiple", Value::from(*multiple));
            options.insert(
                "directory",
                Value::from(matches!(request.kind, DialogKind::PickFolder { .. })),
            );
        }
        DialogKind::SaveFile { file_name } => {
            if let Some(name) = file_name {
                if name.contains('\0') {
                    return Err("File picker filename contains a NUL byte".into());
                }
                options.insert("current_name", Value::from(name.clone()));
            }
        }
    }
    if let Some(directory) = &request.directory {
        let bytes = CString::new(directory.as_os_str().as_bytes())
            .map_err(|_| "File picker directory contains a NUL byte")?
            .into_bytes_with_nul();
        options.insert("current_folder", Value::from(bytes));
    }
    let filters: Vec<_> = request
        .filters
        .iter()
        .filter(|filter| !filter.extensions.is_empty())
        .map(|filter| {
            let patterns: Vec<_> = filter
                .extensions
                .iter()
                .map(|extension| {
                    (
                        0_u32,
                        if extension == "*" {
                            "*".into()
                        } else {
                            format!("*.{extension}")
                        },
                    )
                })
                .collect();
            (filter.name.clone(), patterns)
        })
        .collect();
    if !filters.is_empty() {
        options.insert("filters", Value::from(filters));
    }
    Ok(options)
}

async fn cancellable<T>(
    cancel: &Receiver<()>,
    work: impl std::future::Future<Output = Result<T, String>>,
) -> Result<T, String> {
    future::race(bounded(work), async {
        let _ = cancel.recv().await;
        Err("File dialog cancelled".into())
    })
    .await
}

async fn close(connection: &Connection, path: &OwnedObjectPath) {
    let _ = bounded(async {
        connection
            .call_method(Some(SERVICE), path, Some(REQUEST), "Close", &())
            .await
            .map(|_| ())
            .map_err(|error| error.to_string())
    })
    .await;
}

async fn run(request: DialogRequest, address: Option<&str>, cancel: Receiver<()>) -> DialogResult {
    let parent = parent_identifier(request.parent.as_ref())?;
    let mut random = [0_u8; 16];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut random))
        .map_err(|error| format!("Could not create file picker request token: {error}"))?;
    let token = format!("mui_{:032x}", u128::from_ne_bytes(random));
    let options = options(&request, token.clone())?;
    let connection = cancellable(&cancel, async {
        match address {
            Some(address) => {
                zbus::connection::Builder::address(address)
                    .map_err(|error| error.to_string())?
                    .build()
                    .await
            }
            None => Connection::session().await,
        }
        .map_err(|error| format!("Could not connect to desktop file picker: {error}"))
    })
    .await?;
    let expected = OwnedObjectPath::try_from(format!(
        "{PORTAL_PATH}/request/{}/{}",
        connection
            .unique_name()
            .ok_or("Desktop file picker connection has no bus name")?
            .as_str()
            .trim_start_matches(':')
            .replace('.', "_"),
        token
    ))
    .map_err(|error| error.to_string())?;
    let result = async {
        let rule = MatchRule::builder()
            .msg_type(Type::Signal)
            .sender(SERVICE).map_err(|error| error.to_string())?
            .interface(REQUEST).map_err(|error| error.to_string())?
            .member("Response").map_err(|error| error.to_string())?
            .build();
        // Let a started method reply supply the actual request path even if
        // cancellation arrives first. This handshake is bounded, not user wait.
        let setup = bounded(async {
            let responses = MessageStream::for_match_rule(rule, &connection, Some(8))
                .await.map_err(|error| error.to_string())?;
            let proxy = zbus::Proxy::new(&connection, SERVICE, PORTAL_PATH, CHOOSER)
                .await.map_err(|error| error.to_string())?;
            let owner = proxy.receive_owner_changed()
                .await.map_err(|error| error.to_string())?;
            if matches!(request.kind, DialogKind::PickFolder { .. })
                && proxy.get_property::<u32>("version")
                    .await.map_err(|error| error.to_string())? < 3
            {
                return Err("Desktop file picker does not support selecting folders (portal version 3 required)".into());
            }
            if cancel.is_closed() || !cancel.is_empty() {
                return Err("File dialog cancelled before opening".into());
            }
            let method = if matches!(request.kind, DialogKind::SaveFile { .. }) {
                "SaveFile"
            } else {
                "OpenFile"
            };
            let reply = proxy.call_method(method, &(parent, &request.title, options)).await
                .map_err(|error| format!("Desktop file picker is unavailable: {error}"))?;
            let path: OwnedObjectPath = reply.body().deserialize()
                .map_err(|error| error.to_string())?;
            let sender = reply.header().sender()
                .ok_or("Desktop file picker reply has no sender")?.to_owned();
            Ok((responses, owner, path, sender))
        }).await;
        let (mut responses, mut owner, path, sender) = match setup {
            Ok(opened) => opened,
            Err(error) => {
                // A timed-out call may already have created a request.
                close(&connection, &expected).await;
                return Err(error);
            }
        };
        let answer = future::race(async {
            let _ = cancel.recv().await;
            None
        }, future::race(async {
            loop {
                if owner.next().await.as_ref() != Some(&Some(sender.clone())) {
                    return Some(Err("Desktop file picker backend disconnected before responding".into()));
                }
            }
        }, async {
            loop {
                match responses.next().await {
                    Some(Ok(message)) if message.header().path() == Some(&path.as_ref()) && message.header().sender() == Some(&sender.as_ref()) => {
                        return Some(decode_response(&message, &request.kind));
                    }
                    Some(Ok(_)) => {},
                    Some(Err(error)) => return Some(Err(format!("Desktop file picker response failed: {error}"))),
                    None => return Some(Err("Desktop file picker disconnected before responding".into())),
                }
            }
        })).await;
        if let Some(result) = answer { result } else {
            // Close does not emit Response: do not keep awaiting one.
            close(&connection, &path).await;
            Ok(None)
        }
    }.await;
    let _ = bounded(async { connection.close().await.map_err(|error| error.to_string()) }).await;
    result
}

fn decode_response(message: &zbus::Message, kind: &DialogKind) -> DialogResult {
    let (response, mut results): (u32, HashMap<String, OwnedValue>) = message
        .body()
        .deserialize()
        .map_err(|error| format!("Invalid desktop file picker response: {error}"))?;
    match response {
        1 => return Ok(None),
        0 => {}
        code => {
            return Err(format!(
                "Desktop file picker ended with an error (response {code})"
            ));
        }
    }
    let uris = results
        .remove("uris")
        .ok_or("Desktop file picker returned no URIs")?;
    let uris = Vec::<String>::try_from(uris)
        .map_err(|error| format!("Invalid desktop file picker URIs: {error}"))?;
    let paths: Vec<PathBuf> = uris
        .iter()
        .map(|uri| {
            // URL parsing is intentionally tolerant of invalid percent escapes;
            // a portal file URI must not silently turn those into a filename.
            if uri.bytes().any(|byte| byte <= b' ' || byte == 127)
                || uri.split('%').skip(1).any(|escape| {
                    escape.len() < 2 || !escape.as_bytes()[..2].iter().all(u8::is_ascii_hexdigit)
                })
            {
                return Err(format!("Invalid desktop file picker URI: {uri}"));
            }
            let path = url::Url::parse(uri)
                .map_err(|error| format!("Invalid desktop file picker URI: {error}"))?
                .to_file_path()
                .map_err(|()| {
                    format!("Desktop file picker returned a non-local file URI: {uri}")
                })?;
            if path.as_os_str().as_bytes().contains(&0) {
                return Err("Desktop file picker path contains a NUL byte".into());
            }
            Ok(path)
        })
        .collect::<Result<_, _>>()?;
    let multiple = matches!(
        kind,
        DialogKind::OpenFile { multiple: true } | DialogKind::PickFolder { multiple: true }
    );
    if paths.is_empty() || !multiple && paths.len() != 1 {
        return Err("Desktop file picker returned an invalid number of paths".into());
    }
    Ok(Some(paths))
}

#[cfg(test)]
mod tests;

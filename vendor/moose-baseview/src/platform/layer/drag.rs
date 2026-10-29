//! Native file transfers share the dock's connection, seat and event queue.
use super::*;
use matari_audio_drag_and_drop::{
    DragPreview, FileSet, LinuxOutcome, NativeProtocol, PreviewFailureStage, PreviewStatus,
    SessionRoute, SourceContext, WaylandIcon, WaylandReporter, WaylandRuntimePort,
    WaylandSourceReporter,
};
use smithay_client_toolkit::{
    compositor::CompositorState,
    data_device_manager::{
        data_device::{DataDevice, DataDeviceHandler},
        data_offer::{DataOfferHandler, DragOffer},
        data_source::{DataSourceHandler, DragSource},
        DataDeviceManagerState, ReadPipe, WritePipe,
    },
    shm::{
        slot::{Buffer, SlotPool},
        Shm, ShmHandler,
    },
};
use std::{
    io::{Read, Write},
    os::fd::AsRawFd,
    os::unix::ffi::OsStrExt,
};
use wayland_client::{
    globals::GlobalList,
    protocol::{
        wl_data_device::WlDataDevice, wl_data_device_manager::DndAction,
        wl_data_source::WlDataSource, wl_seat::WlSeat, wl_shm,
    },
};

const URI_LIST: &str = "text/uri-list";
const MAX_TRANSFER: usize = 1024 * 1024;
const PATH_ESCAPES: &percent_encoding::AsciiSet = &percent_encoding::CONTROLS
    .add(b' ')
    .add(b'%')
    .add(b'#')
    .add(b'?')
    .add(b'"')
    .add(b'<')
    .add(b'>')
    .add(b'`');

struct Source {
    native: DragSource,
    bytes: Arc<[u8]>,
    reporter: Rc<RefCell<WaylandSourceReporter>>,
    _icon: Option<Icon>,
}
/// Dock-owned drag thumbnail. It stays alive until the native source terminates.
struct Icon {
    surface: wl_surface::WlSurface,
    _pool: SlotPool,
    _buffer: Buffer,
    icon: WaylandIcon,
}
impl Icon {
    fn new(
        data: &Data, qh: &QueueHandle<Runtime>, preview: &DragPreview, scale: i32,
    ) -> std::io::Result<Self> {
        let icon = preview.wayland_icon(scale).map_err(std::io::Error::other)?;
        let [width, height] = icon.buffer_size;
        let stride = width
            .checked_mul(4)
            .ok_or_else(|| std::io::Error::other("drag icon stride overflow"))?;
        let mut pool =
            SlotPool::new(icon.pixels.len(), &data.shm).map_err(std::io::Error::other)?;
        let (buffer, canvas) = pool
            .create_buffer(width, height, stride, wl_shm::Format::Argb8888)
            .map_err(std::io::Error::other)?;
        canvas.copy_from_slice(&icon.pixels);
        Ok(Self { surface: data.compositor.create_surface(qh), _pool: pool, _buffer: buffer, icon })
    }
    /// Commit only after `start_drag` has assigned the icon role.
    fn show(&self) {
        let (surface, icon, buffer) = (&self.surface, &self.icon, self._buffer.wl_buffer());
        if surface.version() >= 3 {
            surface.set_buffer_scale(icon.scale);
        }
        if surface.version() >= 5 {
            surface.attach(Some(buffer), 0, 0);
            surface.offset(icon.offset[0], icon.offset[1]);
        } else {
            surface.attach(Some(buffer), icon.offset[0], icon.offset[1]);
        }
        if surface.version() >= 4 {
            surface.damage_buffer(0, 0, icon.buffer_size[0], icon.buffer_size[1]);
        } else {
            surface.damage(0, 0, icon.logical_size[0], icon.logical_size[1]);
        }
        surface.commit();
    }
}
impl Drop for Icon {
    fn drop(&mut self) {
        self.surface.destroy();
    }
}
struct Transfer {
    pipe: WritePipe,
    bytes: Arc<[u8]>,
    sent: usize,
    reporter: Rc<RefCell<WaylandSourceReporter>>,
    deadline: Instant,
}
struct Inbound {
    offer: DragOffer,
    pipe: Option<ReadPipe>,
    bytes: Vec<u8>,
    paths: Vec<std::path::PathBuf>,
    dropped: bool,
    announced: bool,
    position: PhysicalPosition<f64>,
    deadline: Instant,
}

pub(super) struct Data {
    manager: DataDeviceManagerState,
    compositor: CompositorState,
    shm: Shm,
    devices: Vec<DataDevice>,
    source: Option<Source>,
    transfers: Vec<Transfer>,
    inbound: Option<Inbound>,
}
impl Data {
    pub fn new(
        globals: &GlobalList, qh: &QueueHandle<Runtime>, compositor: CompositorState,
    ) -> linux::Result<Self> {
        Ok(Self {
            manager: DataDeviceManagerState::bind(globals, qh).map_err(failure)?,
            compositor,
            shm: Shm::bind(globals, qh).map_err(failure)?,
            devices: Vec::new(),
            source: None,
            transfers: Vec::new(),
            inbound: None,
        })
    }
    pub fn add_seat(&mut self, qh: &QueueHandle<Runtime>, seat: &WlSeat) {
        if self.device(seat).is_none() {
            self.devices.push(self.manager.get_data_device(qh, seat));
        }
    }
    pub fn remove_seat(&mut self, seat: &WlSeat) {
        self.devices.retain(|d| d.data().seat() != seat);
    }
    pub fn device(&self, seat: &WlSeat) -> Option<WlDataDevice> {
        self.devices.iter().find(|d| d.data().seat() == seat).map(|d| d.inner().clone())
    }
    /// Retire the source once the dock's grab is gone without a terminal event.
    pub fn retire(&mut self) {
        if let Some(source) = self.source.take() {
            source.reporter.borrow().retire();
        }
    }
    pub fn cancel(&mut self) {
        self.retire();
        self.transfers.clear();
        self.inbound = None;
    }
    pub fn has_transfers(&self) -> bool {
        !self.transfers.is_empty() || self.inbound.as_ref().is_some_and(|i| i.pipe.is_some())
    }
    pub fn progress(&mut self, handler: Option<&dyn WindowHandler>) {
        self.transfers.retain_mut(|transfer| {
            if Instant::now() > transfer.deadline {
                return false;
            }
            match transfer.pipe.write(transfer.bytes.get(transfer.sent..).unwrap_or_default()) {
                Ok(0) => false,
                Ok(written) => {
                    transfer.sent += written;
                    if transfer.sent == transfer.bytes.len() {
                        transfer.reporter.borrow_mut().data_requested();
                        false
                    } else {
                        true
                    }
                }
                Err(e) => e.kind() == std::io::ErrorKind::WouldBlock,
            }
        });
        let Some(inbound) = &mut self.inbound else { return };
        let mut failed = Instant::now() > inbound.deadline;
        if let Some(pipe) = &mut inbound.pipe {
            let mut chunk = [0u8; 8192];
            match pipe.read(&mut chunk) {
                Ok(0) => {
                    inbound.paths = parse_uris(&inbound.bytes);
                    inbound.pipe = None;
                }
                Ok(count) if inbound.bytes.len().saturating_add(count) <= MAX_TRANSFER => {
                    inbound.bytes.extend_from_slice(&chunk[..count])
                }
                Ok(_) => failed = true,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(_) => failed = true,
            }
        }
        if !failed && inbound.pipe.is_none() && !inbound.announced {
            inbound.announced = true;
            if let Some(handler) = handler {
                handler.on_event(Event::Mouse(MouseEvent::DragEntered {
                    position: inbound.position,
                    modifiers: keyboard_types::Modifiers::empty(),
                    data: DropData::Files(inbound.paths.clone()),
                }));
            }
        }
        if failed || (inbound.dropped && inbound.pipe.is_none()) {
            if let Some(inbound) = self.inbound.take() {
                if let Some(handler) = handler.filter(|_| !failed) {
                    let accepted = handler.on_event(Event::Mouse(MouseEvent::DragDropped {
                        position: inbound.position,
                        modifiers: keyboard_types::Modifiers::empty(),
                        data: DropData::Files(inbound.paths),
                    }));
                    if matches!(accepted, EventStatus::AcceptDrop(DropEffect::Copy)) {
                        inbound.offer.finish();
                    }
                }
            }
        }
    }
}

struct Port<'a> {
    data: &'a mut Data,
    context: &'a Context,
}
impl WaylandRuntimePort for Port<'_> {
    type Error = std::io::Error;
    fn start_drag(
        self, files: FileSet, reporter: WaylandReporter,
    ) -> std::io::Result<SessionRoute> {
        let (device, serial) = self
            .context
            .press
            .borrow_mut()
            .take()
            .ok_or_else(|| std::io::Error::other("dock drag has no live pointer press"))?;
        let mut uris = String::new();
        for path in files.paths() {
            uris.push_str("file://");
            uris.push_str(
                &percent_encoding::percent_encode(path.as_os_str().as_bytes(), PATH_ESCAPES)
                    .to_string(),
            );
            uris.push_str("\r\n");
            if uris.len() > MAX_TRANSFER {
                return Err(std::io::Error::other("drag file list is too large"));
            }
        }
        let native = self.data.manager.create_drag_and_drop_source(
            &self.context.qh,
            [URI_LIST],
            DndAction::Copy,
        );
        // Buffer scale is only legal on surfaces v3+; match the dock's output.
        let scale = if self.context.surface().version() >= 3 {
            self.context.scale_factor().ceil().max(1.0) as i32
        } else {
            1
        };
        // A thumbnail that cannot be built must not stop the file drag.
        let icon = files.preview().and_then(|preview| {
            match Icon::new(&*self.data, &self.context.qh, preview, scale) {
                Ok(icon) => Some(icon),
                Err(_) => {
                    reporter.preview(PreviewStatus::Unavailable {
                        stage: PreviewFailureStage::Bitmap,
                        native_code: None,
                    });
                    None
                }
            }
        });
        if icon.is_some() {
            reporter.preview(PreviewStatus::Attached);
        }
        // This device, surface and serial all came from this runtime's current
        // pointer queue; no connection or foreign event queue is synthesized.
        device.start_drag(
            Some(native.inner()),
            self.context.surface(),
            icon.as_ref().map(|icon| &icon.surface),
            serial,
        );
        if let Some(icon) = &icon {
            icon.show();
        }
        self.data.source = Some(Source {
            native,
            bytes: uris.into_bytes().into(),
            reporter: Rc::new(RefCell::new(WaylandSourceReporter::new(reporter))),
            _icon: icon,
        });
        self.context.drag_active.set(true);
        Ok(SessionRoute {
            protocol: NativeProtocol::WaylandDataDevice,
            source: SourceContext::Wayland,
        })
    }
}
impl Runtime {
    pub(super) fn start_pending_drag(&mut self) {
        let Some(cx) = &self.context else { return };
        let ticket = cx.pending_drag.borrow_mut().take();
        if let Some(ticket) = ticket {
            if cx.primary.get() {
                let _ = ticket.start_wayland(Port { data: &mut self.data, context: cx });
            }
        }
    }
    fn terminal_drag(&mut self, source: &WlDataSource, outcome: LinuxOutcome) {
        if self.data.source.as_ref().is_some_and(|s| s.native.inner() == source) {
            if let Some(source) = self.data.source.take() {
                source.reporter.borrow().finish_linux(outcome);
            }
            if let Some(cx) = &self.context {
                cx.drag_active.set(false);
                cx.primary.set(false);
            }
            self.event(Event::Mouse(MouseEvent::ButtonReleased {
                button: MouseButton::Left,
                modifiers: self.modifiers,
            }));
        }
    }
}
impl ShmHandler for Runtime {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.data.shm
    }
}
impl DataSourceHandler for Runtime {
    fn accept_mime(
        &mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataSource, _: Option<String>,
    ) {
    }
    fn send_request(
        &mut self, _: &Connection, _: &QueueHandle<Self>, source: &WlDataSource, mime: String,
        fd: WritePipe,
    ) {
        if mime != URI_LIST
            || self.data.transfers.len() >= 8
            || nonblocking(fd.as_raw_fd()).is_err()
        {
            return;
        }
        if let Some(source) = self.data.source.as_ref().filter(|s| s.native.inner() == source) {
            self.data.transfers.push(Transfer {
                pipe: fd,
                bytes: Arc::clone(&source.bytes),
                sent: 0,
                reporter: Rc::clone(&source.reporter),
                deadline: Instant::now() + Duration::from_secs(10),
            });
        }
    }
    fn cancelled(&mut self, _: &Connection, _: &QueueHandle<Self>, source: &WlDataSource) {
        self.terminal_drag(source, LinuxOutcome::Cancelled);
    }
    fn dnd_dropped(&mut self, _: &Connection, _: &QueueHandle<Self>, source: &WlDataSource) {
        if let Some(source) = self.data.source.as_ref().filter(|s| s.native.inner() == source) {
            source.reporter.borrow_mut().drop_performed();
        }
    }
    fn dnd_finished(&mut self, _: &Connection, _: &QueueHandle<Self>, source: &WlDataSource) {
        self.terminal_drag(source, LinuxOutcome::Exported);
    }
    fn action(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataSource, _: DndAction) {}
}
impl DataDeviceHandler for Runtime {
    fn enter(
        &mut self, _: &Connection, _: &QueueHandle<Self>, device: &WlDataDevice, x: f64, y: f64,
        surface: &wl_surface::WlSurface,
    ) {
        let Some(cx) = &self.context else { return };
        if cx.surface() != surface {
            return;
        }
        let Some(device) = self.data.devices.iter().find(|d| d.inner() == device) else { return };
        let Some(offer) = device.data().drag_offer() else { return };
        if !offer.with_mime_types(|m| m.iter().any(|m| m == URI_LIST)) {
            return;
        }
        offer.accept_mime_type(offer.serial, Some(URI_LIST.into()));
        offer.set_actions(DndAction::Copy, DndAction::Copy);
        if let Ok(pipe) = offer
            .receive(URI_LIST.into())
            .and_then(|pipe| nonblocking(pipe.as_raw_fd()).map(|()| pipe))
        {
            self.data.inbound = Some(Inbound {
                offer,
                pipe: Some(pipe),
                bytes: Vec::new(),
                paths: Vec::new(),
                dropped: false,
                announced: false,
                position: PhysicalPosition::new(x * cx.scale_factor(), y * cx.scale_factor()),
                deadline: Instant::now() + Duration::from_secs(10),
            });
        }
    }
    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice) {
        if self.data.inbound.as_ref().is_some_and(|i| !i.dropped) {
            self.data.inbound = None;
            self.event(Event::Mouse(MouseEvent::DragLeft));
        }
    }
    fn motion(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice, x: f64, y: f64) {
        let scale = self.context.as_ref().map_or(1.0, |c| c.scale_factor());
        if let Some(i) = &mut self.data.inbound {
            i.position = PhysicalPosition::new(x * scale, y * scale);
            if i.announced {
                let event = Event::Mouse(MouseEvent::DragMoved {
                    position: i.position,
                    modifiers: self.modifiers,
                    data: DropData::Files(i.paths.clone()),
                });
                self.event(event);
            }
        }
    }
    fn selection(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice) {}
    fn drop_performed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &WlDataDevice) {
        if let Some(i) = &mut self.data.inbound {
            i.dropped = true;
        }
    }
}
impl DataOfferHandler for Runtime {
    fn source_actions(
        &mut self, _: &Connection, _: &QueueHandle<Self>, _: &mut DragOffer, _: DndAction,
    ) {
    }
    fn selected_action(
        &mut self, _: &Connection, _: &QueueHandle<Self>, _: &mut DragOffer, _: DndAction,
    ) {
    }
}
fn nonblocking(fd: std::os::fd::RawFd) -> std::io::Result<()> {
    // SAFETY: the borrowed, live pipe descriptor remains owned by its transfer;
    // fcntl changes flags only and never closes or aliases ownership of it.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}
fn parse_uris(bytes: &[u8]) -> Vec<std::path::PathBuf> {
    use std::os::unix::ffi::OsStringExt;
    String::from_utf8_lossy(bytes)
        .lines()
        .filter(|s| !s.starts_with('#'))
        .filter_map(|uri| {
            let path =
                uri.strip_prefix("file://localhost/").map(|p| format!("/{p}")).or_else(|| {
                    uri.strip_prefix("file://").filter(|p| p.starts_with('/')).map(str::to_owned)
                })?;
            let decoded = percent_encoding::percent_decode_str(&path).collect::<Vec<u8>>();
            if decoded.contains(&0) {
                return None;
            }
            Some(std::path::PathBuf::from(std::ffi::OsString::from_vec(decoded)))
        })
        .take(4096)
        .collect()
}

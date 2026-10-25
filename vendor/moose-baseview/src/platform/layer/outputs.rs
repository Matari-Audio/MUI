//! Enumerate the backend a future desktop dock will use, even from XWayland.
use super::*;

struct Outputs {
    registry: RegistryState,
    outputs: OutputState,
}
impl OutputHandler for Outputs {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.outputs
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}
impl ProvidesRegistryState for Outputs {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry
    }
    registry_handlers!(OutputState);
}
smithay_client_toolkit::delegate_registry!(Outputs);
smithay_client_toolkit::delegate_dispatch2!(Outputs);

pub(in crate::platform) fn names() -> Vec<String> {
    // No host/editor state enters this probe. A compositor that stalls cannot
    // stall the caller indefinitely; pinning keeps a late probe's code mapped.
    if !crate::pin_current_image_for_detached_work() {
        return Vec::new();
    }
    let (send, receive) = mpsc::sync_channel(1);
    let worker = std::thread::Builder::new().name("mui-dock-outputs".into()).spawn(move || {
        let query = || -> std::result::Result<Vec<String>, Box<dyn std::error::Error>> {
            let conn = Connection::connect_to_env()?;
            let (globals, mut queue) = registry_queue_init::<Outputs>(&conn)?;
            let mut state = Outputs {
                registry: RegistryState::new(&globals),
                outputs: OutputState::new(&globals, &queue.handle()),
            };
            queue.roundtrip(&mut state)?;
            Ok(state
                .outputs
                .outputs()
                .filter_map(|output| state.outputs.info(&output).and_then(|info| info.name))
                .collect())
        };
        let _ = send.send(query().unwrap_or_default());
    });
    if worker.is_err() {
        return Vec::new();
    }
    receive.recv_timeout(Duration::from_millis(100)).unwrap_or_default()
}

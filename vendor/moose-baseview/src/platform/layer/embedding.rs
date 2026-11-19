//! `clap_surface_embedding_v1` from the CLAP `clap-wayland-experiment` draft
//! (`protocols/clap-surface-embedding-v1.xml`, copied verbatim): a host's
//! nested compositor places our surface in its window by a token.
use super::Runtime;
use smithay_client_toolkit::dispatch2::Dispatch2;
use wayland_client::{Connection, QueueHandle};

#[allow(
    dead_code,
    non_camel_case_types,
    non_upper_case_globals,
    unused_imports,
    missing_docs,
    clippy::all,
    clippy::pedantic,
    clippy::restriction,
    reason = "wayland-scanner output"
)]
pub(super) mod protocol {
    use wayland_client;
    use wayland_client::protocol::*;
    use wayland_protocols::xdg::shell::client::*;
    pub mod __interfaces {
        use wayland_client::protocol::__interfaces::*;
        use wayland_protocols::xdg::shell::client::__interfaces::*;
        wayland_scanner::generate_interfaces!("protocols/clap-surface-embedding-v1.xml");
    }
    use self::__interfaces::*;
    wayland_scanner::generate_client_code!("protocols/clap-surface-embedding-v1.xml");
}
use protocol::clap_embedded_surface_v1::{ClapEmbeddedSurfaceV1, Event};

/// User data of our `clap_embedded_surface_v1`.
#[derive(Debug)]
pub(super) struct Embedding;
impl Dispatch2<ClapEmbeddedSurfaceV1, Runtime> for Embedding {
    fn event(
        &self, state: &mut Runtime, _: &ClapEmbeddedSurfaceV1, event: Event, _: &Connection,
        _: &QueueHandle<Runtime>,
    ) {
        // ponytail: `position` only places popups, and editors open none yet.
        if let Event::Closed = event {
            // The host dropped or refused the slot: the window is gone for good.
            state.exit = true;
        }
    }
}

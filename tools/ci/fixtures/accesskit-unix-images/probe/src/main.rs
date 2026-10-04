use libloading::Library;
use std::{
    ffi::c_void,
    mem::ManuallyDrop,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use zbus::{Connection, Proxy, interface, zvariant::OwnedObjectPath};

const ROOT: &str = "/org/a11y/atspi/accessible/root";
type Address = (String, OwnedObjectPath);

struct Status(bool);
#[interface(name = "org.a11y.Status")]
impl Status {
    #[zbus(property)]
    fn is_enabled(&self) -> bool {
        self.0
    }
    #[zbus(property)]
    fn screen_reader_enabled(&self) -> bool {
        self.0
    }
}
struct Bus(String);
#[interface(name = "org.a11y.Bus")]
impl Bus {
    fn get_address(&self) -> &str {
        &self.0
    }
}
struct Registry {
    names: Arc<Mutex<Vec<String>>>,
    address: Address,
    blocked: bool,
}
#[interface(name = "org.a11y.atspi.Socket")]
impl Registry {
    async fn embed(&self, plug: Address) -> Address {
        self.names.lock().unwrap().push(plug.0);
        if self.blocked {
            std::future::pending::<()>().await;
        }
        self.address.clone()
    }
    fn unembed(&self, _plug: Address) {}
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Stats {
    activations: u64,
    actions: u64,
    drops: u64,
}
struct Image {
    // Native pointers/functions are valid only while this library is loaded.
    library: ManuallyDrop<Library>,
    image: *mut c_void,
}
impl Image {
    unsafe fn load(path: &str) -> Self {
        let library = unsafe { Library::new(path) }.unwrap();
        let open = *unsafe {
            library.get::<unsafe extern "C" fn() -> *mut c_void>(b"accesskit_probe_open")
        }
        .unwrap();
        // Let Rust TLS destructors from the foreign image finish on its native
        // calling thread before dlclose; no loader pinning workaround is used.
        let pointer = std::thread::spawn(move || unsafe { open() } as usize)
            .join()
            .unwrap();
        let image = Self {
            library: ManuallyDrop::new(library),
            image: pointer as *mut c_void,
        };
        assert_eq!(
            image.stats().drops,
            0,
            "image was not actually unloaded on the prior cycle"
        );
        image
    }
    fn stats(&self) -> Stats {
        unsafe {
            self.library
                .get::<unsafe extern "C" fn() -> Stats>(b"accesskit_probe_stats")
                .unwrap()()
        }
    }
    fn publish(&self) {
        let publish = *unsafe {
            self.library
                .get::<unsafe extern "C" fn(*mut c_void)>(b"accesskit_probe_publish")
        }
        .unwrap();
        let pointer = self.image as usize;
        std::thread::spawn(move || unsafe { publish(pointer as *mut c_void) })
            .join()
            .unwrap();
    }
    fn close_one(&self) {
        let close = *unsafe {
            self.library
                .get::<unsafe extern "C" fn(*mut c_void) -> usize>(b"accesskit_probe_close_one")
        }
        .unwrap();
        let pointer = self.image as usize;
        assert_eq!(
            std::thread::spawn(move || unsafe { close(pointer as *mut c_void) })
                .join()
                .unwrap(),
            1
        );
    }
    fn close(&mut self, check_drops: bool) {
        let close = *unsafe {
            self.library
                .get::<unsafe extern "C" fn(*mut c_void)>(b"accesskit_probe_close")
        }
        .unwrap();
        let pointer = self.image as usize;
        std::thread::spawn(move || unsafe { close(pointer as *mut c_void) })
            .join()
            .unwrap();
        self.image = std::ptr::null_mut();
        if check_drops {
            assert_eq!(
                self.stats().drops,
                6,
                "close returned before native handlers were destroyed"
            );
        }
    }
}
// Retain library mappings on failure: never deliberately unmap live workers.
impl Drop for Image {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            unsafe {
                ManuallyDrop::drop(&mut self.library);
            }
        }
    }
}
fn threads() -> usize {
    std::fs::read_dir("/proc/self/task").unwrap().count()
}
async fn wait(mut ready: impl FnMut() -> bool, what: &str) {
    let until = Instant::now() + Duration::from_secs(5);
    while !ready() {
        assert!(Instant::now() < until, "timed out: {what}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}
async fn proxy<'a>(
    connection: &'a Connection,
    name: &'a str,
    path: &'a str,
    interface: &'a str,
) -> Proxy<'a> {
    Proxy::new(connection, name, path, interface).await.unwrap()
}
async fn action(connection: &Connection, name: &str) {
    let root = proxy(connection, name, ROOT, "org.a11y.atspi.Accessible").await;
    let windows: Vec<Address> = root.call("GetChildren", &()).await.unwrap();
    assert!(!windows.is_empty());
    let window = proxy(
        connection,
        &windows[0].0,
        windows[0].1.as_str(),
        "org.a11y.atspi.Accessible",
    )
    .await;
    let children: Vec<Address> = window.call("GetChildren", &()).await.unwrap();
    assert_eq!(children.len(), 1);
    let button = proxy(
        connection,
        &children[0].0,
        children[0].1.as_str(),
        "org.a11y.atspi.Action",
    )
    .await;
    assert!(button.call::<_, _, bool>("DoAction", &0_i32).await.unwrap());
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let args: Vec<_> = std::env::args().collect();
    let active = args[3] == "active";
    let blocked = args[3] == "blocked";
    let old_worker = args.get(4).is_some_and(|a| a == "--old-worker");
    let address = std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap();
    let names = Arc::new(Mutex::new(Vec::new()));
    let connection = zbus::connection::Builder::session()
        .unwrap()
        .name("org.a11y.Bus")
        .unwrap()
        .name("org.a11y.atspi.Registry")
        .unwrap()
        .serve_at("/org/a11y/bus", Status(active || blocked))
        .unwrap()
        .serve_at("/org/a11y/bus", Bus(address))
        .unwrap()
        .build()
        .await
        .unwrap();
    connection
        .object_server()
        .at(
            ROOT,
            Registry {
                names: names.clone(),
                blocked,
                address: (
                    connection.unique_name().unwrap().to_string(),
                    ROOT.try_into().unwrap(),
                ),
            },
        )
        .await
        .unwrap();
    // The host's own blocking connection job has finished before the baseline.
    tokio::time::sleep(Duration::from_millis(100)).await;
    let baseline = threads();
    for cycle in 0..3 {
        names.lock().unwrap().clear();
        let mut a = unsafe { Image::load(&args[1]) };
        let mut b = unsafe { Image::load(&args[2]) };
        if active {
            wait(
                || a.stats().activations == 2 && b.stats().activations == 2,
                "activate both images",
            )
            .await;
            a.publish();
            b.publish();
            wait(|| names.lock().unwrap().len() == 2, "embed both images").await;
            // Interface registrations follow the Embed reply asynchronously.
            tokio::time::sleep(Duration::from_millis(100)).await;
            let peers = names.lock().unwrap().clone();
            action(&connection, &peers[0]).await;
            assert_eq!(a.stats().actions + b.stats().actions, 1);
            action(&connection, &peers[1]).await;
            assert_eq!(a.stats().actions, 1);
            assert_eq!(b.stats().actions, 1);
        } else {
            tokio::time::sleep(Duration::from_millis(100)).await;
            assert_eq!(a.stats().activations, 0);
            assert_eq!(b.stats().activations, 0);
        }
        assert!(threads() > baseline);
        a.close_one();
        b.close_one();
        if !blocked {
            wait(
                || a.stats().drops == 3 && b.stats().drops == 3,
                "remove first adapters",
            )
            .await;
        }
        a.close(!old_worker);
        assert!(threads() > baseline, "other image worker ended prematurely");
        b.close(!old_worker);
        if old_worker {
            wait(
                || a.stats().drops == 6 && b.stats().drops == 6,
                "old native handlers removed",
            )
            .await;
        }
        // Detect the original detached workers before any unsafe dlclose.
        if threads() != baseline {
            std::mem::forget(a);
            std::mem::forget(b);
            panic!(
                "native accessibility workers survived final close: {} vs {baseline}",
                threads()
            );
        }
        let peers = names.lock().unwrap().clone();
        let dbus = proxy(
            &connection,
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
        )
        .await;
        for name in peers {
            assert!(
                !dbus
                    .call::<_, _, bool>("NameHasOwner", &name)
                    .await
                    .unwrap(),
                "provider bus survived close"
            );
        }
        drop(a);
        drop(b);
        assert_eq!(threads(), baseline);
        println!(
            "PASS {active} cycle={cycle} native-actions handlers-drained workers-joined disconnected unload"
        );
    }
}

//! The OS blur behind a window: a frosted-glass window policy. [`frosts`] decides; each platform only gathers what
//! it asks and makes the call. A top-level window gets it directly
//! ([`blur_behind`]); the editor embedded in a host frosts the host's
//! window under it for as long as it stays ([`HostBlur`]). A frosted window
//! is see-through; the OS blurs what shows through only while the Blur
//! knob is above 0 ([`blurs`]), and follows it live.

use std::sync::atomic::{AtomicU8, Ordering};

use raw_window_handle::HasWindowHandle;

/// What the desktop under a window offers for a blur behind it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Desktop {
    /// X11 or XWayland: whether the window has a 32-bit (ARGB) visual and a
    /// compositing manager owns `_NET_WM_CM_S<screen>`. KWin and picom blur
    /// under `_KDE_NET_WM_BLUR_BEHIND_REGION`, Hyprland (`decoration:blur`)
    /// blurs any translucent window; the rest show the desktop through the
    /// tint unblurred.
    X11 { argb: bool, compositor: bool },
    /// Windows, by build: the system backdrop needs 11 22H2 (22621).
    Windows { build: u32 },
    /// macOS: an `NSVisualEffectView` under the content.
    MacOs,
    /// Native Wayland or anything else. (`ext-background-effect-v1` is
    /// Wayland's cross-compositor way; baseview has no Wayland backend.)
    Other,
}

/// Windows 11 22H2, the first build with `DWMWA_SYSTEMBACKDROP_TYPE`.
const WINDOWS_BACKDROP_BUILD: u32 = 22621;

/// Whether a panel window gets a blur behind it: the preference is on and
/// the desktop can.
pub fn frosts(preference: bool, desktop: Desktop) -> bool {
    preference
        && match desktop {
            Desktop::X11 { argb, compositor } => argb && compositor,
            Desktop::Windows { build } => build >= WINDOWS_BACKDROP_BUILD,
            Desktop::MacOs => true,
            Desktop::Other => false,
        }
}

/// Whether the OS blurs what shows through a frosted window: only with the
/// Blur knob (`Metrics::blur`, in points) above 0. At 0 the desktop shows
/// through sharp.
pub fn blurs(amount: f64) -> bool {
    amount > 0.0
}

/// Make the top-level `window` see-through to the desktop when [`frosts`]
/// says so, the OS blurring what shows through only if `blur` ([`blurs`]).
/// Call it again when `blur` changes: it sets exactly what `blur` asks and
/// takes back the other. `Err` says why not; the window is then untouched
/// and stays opaque.
pub fn blur_behind(
    window: &impl HasWindowHandle,
    preference: bool,
    blur: bool,
) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    return x11::blur_behind(window, preference, blur);
    #[cfg(target_os = "macos")]
    return macos::blur_behind(window, preference, blur);
    #[cfg(target_os = "windows")]
    return windows::blur_behind(window, preference, blur);
    #[expect(unreachable_code, reason = "every supported platform returned above")]
    {
        let _ = (window, blur);
        refuse(preference, Desktop::Other)
    }
}

#[cfg(target_os = "macos")]
pub use macos::HostBlur;
/// The host's blur under the embedded editor: the DWM backdrop on its
/// Windows 11 top-level window, an `NSVisualEffectView` under the editor's
/// view on macOS. Dropping it puts back everything it changed.
#[cfg(target_os = "windows")]
pub use windows::HostBlur;
/// The host's blur under the embedded editor: never on X11, where a
/// translucent child window composites as black.
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub struct HostBlur(std::convert::Infallible);
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
impl HostBlur {
    pub fn new(_window: &impl HasWindowHandle, _blur: bool) -> Result<Self, String> {
        Err("no blur under an embedded editor here".to_owned())
    }
    pub fn sync(&mut self) -> bool {
        match self.0 {}
    }
    pub fn set_blur(&mut self, _blur: bool) {
        match self.0 {}
    }
}

/// The `DWM_SYSTEMBACKDROP_TYPE` behind a frosted window: the transient
/// (acrylic) backdrop when it blurs, none (the desktop sharp through the
/// extended frame) at Blur 0.
#[cfg_attr(not(target_os = "windows"), expect(dead_code, reason = "Windows only"))]
const fn backdrop(blur: bool) -> i32 {
    const DWMSBT_NONE: i32 = 1;
    const DWMSBT_TRANSIENTWINDOW: i32 = 3;
    if blur {
        DWMSBT_TRANSIENTWINDOW
    } else {
        DWMSBT_NONE
    }
}

/// Frame margins (left, right, top, bottom, as `MARGINS`) extending the DWM
/// frame into a host window's `client` area (width, height) just far enough
/// to lie under the `editor` (left, top, right, bottom, in that client
/// area): the one edge strip holding it with the least of the host's own
/// drawing, which the backdrop shows through wherever it is black. `None`
/// when that strip holds more than a sliver of the host besides the editor
/// (a host embedding plugins in its main window).
#[cfg_attr(not(target_os = "windows"), expect(dead_code, reason = "Windows only"))]
fn band(client: (i32, i32), editor: (i32, i32, i32, i32)) -> Option<[i32; 4]> {
    let (w, h) = client;
    let (l, t, r, b) = (
        editor.0.max(0),
        editor.1.max(0),
        editor.2.min(w),
        editor.3.min(h),
    );
    if l >= r || t >= b {
        return None;
    }
    let area = |x: i32, y: i32| i64::from(x) * i64::from(y);
    let (margins, strip) = [
        ([r, 0, 0, 0], area(r, h)),
        ([0, w - l, 0, 0], area(w - l, h)),
        ([0, 0, b, 0], area(w, b)),
        ([0, 0, 0, h - t], area(w, h - t)),
    ]
    .into_iter()
    .min_by_key(|s| s.1)?;
    let own = area(r - l, b - t);
    // ponytail: a 5% sliver covers host borders round the editor; a host
    // that frames it thicker stays opaque.
    (strip - own <= own / 20).then_some(margins)
}

/// Whether a host window has a system caption: the only kind whose frame
/// margins are known (none) and so can be put back. `caption_gap` is how far
/// its client area starts below its window's top, in pixels; a custom title
/// bar drawn in the client area (and likely its own extended frame) has none.
#[cfg_attr(not(target_os = "windows"), expect(dead_code, reason = "Windows only"))]
fn system_caption(caption_style: bool, caption_gap: i32) -> bool {
    // ponytail: 10 px is half the smallest caption; a host drawing a thin
    // custom title bar with WS_CAPTION set passes it.
    caption_style && caption_gap >= 10
}

/// The `Err` for a window [`frosts`] turns down, or `Ok` to go ahead.
fn refuse(preference: bool, desktop: Desktop) -> Result<(), String> {
    if frosts(preference, desktop) {
        REFUSED.store(0, Ordering::Relaxed);
        Ok(())
    } else {
        if preference {
            let reason = match desktop {
                Desktop::X11 {
                    compositor: false, ..
                } => 1,
                Desktop::X11 { .. } => 2,
                Desktop::Windows { .. } => 3,
                Desktop::MacOs | Desktop::Other => 4,
            };
            REFUSED.store(reason, Ordering::Relaxed);
        }
        Err(format!("no blur behind on {desktop:?}"))
    }
}

/// Why the last window that asked for frost did not get it.
static REFUSED: AtomicU8 = AtomicU8::new(0);

/// Why a frosted window stayed opaque on this desktop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// No compositing manager owns `_NET_WM_CM_S<screen>`.
    NoCompositor,
    /// The X11 window has no 32-bit (ARGB) visual.
    NoAlphaVisual,
    /// Windows before 11 22H2.
    WindowsTooOld,
    /// Native Wayland or another desktop with no blur API here.
    Unsupported,
}

/// Why frost cannot apply on this desktop, once a window has asked.
pub fn refused() -> Option<Refusal> {
    match REFUSED.load(Ordering::Relaxed) {
        1 => Some(Refusal::NoCompositor),
        2 => Some(Refusal::NoAlphaVisual),
        3 => Some(Refusal::WindowsTooOld),
        4 => Some(Refusal::Unsupported),
        _ => None,
    }
}

#[cfg(target_os = "linux")]
mod x11 {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{Atom, AtomEnum, ConnectionExt, PropMode};
    use x11rb::wrapper::ConnectionExt as _;

    use super::{Desktop, refuse};

    pub(super) fn blur_behind(
        window: &impl HasWindowHandle,
        preference: bool,
        blur: bool,
    ) -> Result<(), String> {
        let e = |err: &dyn std::fmt::Display| err.to_string();
        let id = match window.window_handle().map_err(|x| e(&x))?.as_raw() {
            RawWindowHandle::Xlib(h) if h.window != 0 => h.window as u32,
            RawWindowHandle::Xcb(h) => h.window.get(),
            _ => return refuse(preference, Desktop::Other),
        };
        let (conn, screen) = x11rb::connect(None).map_err(|x| e(&x))?;
        let depth = conn
            .get_geometry(id)
            .map_err(|x| e(&x))?
            .reply()
            .map_err(|x| e(&x))?
            .depth;
        let manager = atom(&conn, format!("_NET_WM_CM_S{screen}").as_bytes())?;
        let owner = conn
            .get_selection_owner(manager)
            .map_err(|x| e(&x))?
            .reply()
            .map_err(|x| e(&x))?
            .owner;
        refuse(
            preference,
            Desktop::X11 {
                argb: depth == 32,
                compositor: owner != x11rb::NONE,
            },
        )?;
        blur_region(&conn, id, blur)?;
        // Hyprland blurs every translucent window unless told not to.
        if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some() {
            // Quietly: the window stays see-through either way.
            if let Ok(title) = title(&conn, id) {
                super::hyprland::set_no_blur(title, !blur);
            }
        }
        Ok(())
    }

    /// KWin's and picom's blur region on window `id`: the whole window
    /// (an empty region) with `blur`, none without.
    fn blur_region(conn: &impl Connection, id: u32, blur: bool) -> Result<(), String> {
        let e = |err: &dyn std::fmt::Display| err.to_string();
        let region = atom(conn, b"_KDE_NET_WM_BLUR_BEHIND_REGION")?;
        if blur {
            conn.change_property32(PropMode::REPLACE, id, region, AtomEnum::CARDINAL, &[])
                .map_err(|x| e(&x))?
                .check()
        } else {
            conn.delete_property(id, region).map_err(|x| e(&x))?.check()
        }
        .map_err(|x| e(&x))?;
        conn.flush().map_err(|x| e(&x))
    }

    /// Window `id`'s title, as Hyprland lists it.
    fn title(conn: &impl Connection, id: u32) -> Result<String, String> {
        let e = |err: &dyn std::fmt::Display| err.to_string();
        let names = [
            (atom(conn, b"_NET_WM_NAME")?, atom(conn, b"UTF8_STRING")?),
            (AtomEnum::WM_NAME.into(), AtomEnum::STRING.into()),
        ];
        for (name, kind) in names {
            let value = conn
                .get_property(false, id, name, kind, 0, 1024)
                .map_err(|x| e(&x))?
                .reply()
                .map_err(|x| e(&x))?
                .value;
            if !value.is_empty() {
                return Ok(String::from_utf8_lossy(&value).into_owned());
            }
        }
        Err("the window has no title".to_owned())
    }

    fn atom(conn: &impl Connection, name: &[u8]) -> Result<Atom, String> {
        conn.intern_atom(false, name)
            .map_err(|err| err.to_string())?
            .reply()
            .map(|reply| reply.atom)
            .map_err(|err| err.to_string())
    }
}

/// Retry what the desktop has not taken yet: Hyprland's `no_blur` on a
/// window it has not mapped yet. Call it every frame on the UI
/// thread; with nothing pending it only checks an empty list.
pub fn retry() {
    #[cfg(target_os = "linux")]
    hyprland::retry(std::time::Instant::now());
}

/// Hyprland's per-window blur, over its IPC socket: it blurs every
/// translucent window unless the window's `no_blur` is set.
#[cfg(target_os = "linux")]
mod hyprland {
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;
    use std::sync::{Mutex, PoisonError};
    use std::time::{Duration, Instant};

    /// How often a window Hyprland has not listed yet is looked for again.
    const AGAIN: Duration = Duration::from_millis(250);
    /// How long it is looked for before giving up.
    const GIVE_UP: Duration = Duration::from_secs(5);

    /// A `no_blur` Hyprland has not taken yet: its window is not listed.
    #[derive(Debug)]
    struct Pending {
        title: String,
        no_blur: bool,
        next: Instant,
        until: Instant,
    }

    static PENDING: Mutex<Vec<Pending>> = Mutex::new(Vec::new());

    /// Set `no_blur` on this process's XWayland windows titled `title`,
    /// now if Hyprland lists them, else on later [`retry`]s for a while:
    /// a window just opened is often not mapped yet. The newest ask for a
    /// title replaces an older one still pending.
    pub(super) fn set_no_blur(title: String, no_blur: bool) {
        let now = Instant::now();
        let mut pending = PENDING.lock().unwrap_or_else(PoisonError::into_inner);
        ask(&mut pending, title, no_blur, now);
        step(&mut pending, now, apply);
    }

    pub(super) fn retry(now: Instant) {
        let mut pending = PENDING.lock().unwrap_or_else(PoisonError::into_inner);
        if !pending.is_empty() {
            step(&mut pending, now, apply);
        }
    }

    fn ask(pending: &mut Vec<Pending>, title: String, no_blur: bool, now: Instant) {
        pending.retain(|p| p.title != title);
        pending.push(Pending {
            title,
            no_blur,
            next: now,
            until: now + GIVE_UP,
        });
    }

    /// Try each pending ask that is due with `apply` (`true`: taken), and
    /// drop those taken or past their deadline.
    fn step(pending: &mut Vec<Pending>, now: Instant, mut apply: impl FnMut(&str, bool) -> bool) {
        pending.retain_mut(|p| {
            if now >= p.until {
                return false;
            }
            if now < p.next {
                return true;
            }
            p.next = now + AGAIN;
            !apply(&p.title, p.no_blur)
        });
    }

    /// Set `no_blur` on the windows titled `title`; `false` if Hyprland
    /// does not list one (yet) or did not answer.
    fn apply(title: &str, no_blur: bool) -> bool {
        let Ok(clients) = request("j/clients") else {
            return false;
        };
        let found = windows(&clients, std::process::id(), title);
        for address in &found {
            let [current, legacy] = commands(address, no_blur);
            if request(&current).is_ok_and(|reply| reply.trim() != "ok") {
                let _ = request(&legacy);
            }
        }
        !found.is_empty()
    }

    /// The addresses of `pid`'s XWayland windows titled `title` in
    /// Hyprland's `j/clients` reply. (It lists no X11 window ids.)
    pub(super) fn windows(clients: &str, pid: u32, title: &str) -> Vec<String> {
        let Ok(serde_json::Value::Array(clients)) = serde_json::from_str(clients) else {
            return Vec::new();
        };
        clients
            .iter()
            .filter(|c| c["pid"] == pid && c["xwayland"] == true && c["title"] == title)
            .filter_map(|c| c["address"].as_str())
            .filter(|a| {
                a.strip_prefix("0x")
                    .is_some_and(|hex| hex.chars().all(|c| c.is_ascii_hexdigit()))
            })
            .map(str::to_owned)
            .collect()
    }

    /// The requests setting `no_blur` on window `address`: Hyprland 0.55+'s
    /// Lua `set_prop` dispatcher, then the `setprop` request before it.
    pub(super) fn commands(address: &str, no_blur: bool) -> [String; 2] {
        let value = u8::from(no_blur);
        [
            format!(
                "/dispatch hl.dsp.window.set_prop({{ prop = \"no_blur\", value = \"{value}\", window = \"address:{address}\" }})"
            ),
            format!("/setprop address:{address} noblur {value}"),
        ]
    }

    /// One request on Hyprland's socket and its reply.
    fn request(body: &str) -> Result<String, String> {
        let e = |err: std::io::Error| err.to_string();
        let instance = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").map_err(|x| x.to_string())?;
        let runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_default();
        // Before 0.40 the socket lived under /tmp/hypr.
        let mut stream = [
            format!("{runtime}/hypr/{instance}/.socket.sock"),
            format!("/tmp/hypr/{instance}/.socket.sock"),
        ]
        .iter()
        .find_map(|path| UnixStream::connect(path).ok())
        .ok_or("no Hyprland socket")?;
        // Hyprland answers from its main loop at once; never hold a frame.
        let wait = Some(Duration::from_millis(50));
        stream.set_read_timeout(wait).map_err(e)?;
        stream.set_write_timeout(wait).map_err(e)?;
        stream.write_all(body.as_bytes()).map_err(e)?;
        let mut reply = String::new();
        stream.read_to_string(&mut reply).map_err(e)?;
        Ok(reply)
    }
}

#[cfg(target_os = "windows")]
mod windows {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Foundation::{HANDLE, HWND, POINT, RECT};
    use windows::Win32::Graphics::Dwm::{
        DWM_SYSTEMBACKDROP_TYPE, DWMWA_SYSTEMBACKDROP_TYPE, DwmExtendFrameIntoClientArea,
        DwmGetWindowAttribute, DwmSetWindowAttribute,
    };
    use windows::Win32::Graphics::Gdi::{InvalidateRect, MapWindowPoints};
    use windows::Win32::UI::Controls::MARGINS;
    use windows::Win32::UI::WindowsAndMessaging::{
        GA_ROOT, GWL_STYLE, GetAncestor, GetClientRect, GetWindowLongW, GetWindowRect,
        GetWindowThreadProcessId, IsWindow, RemovePropW, SetPropW, WS_CAPTION,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcessId;

    use super::{Desktop, refuse};

    /// The host client size and the editor rectangle in host client coordinates.
    type HostPlacement = ((i32, i32), (i32, i32, i32, i32));

    pub(super) fn blur_behind(
        window: &impl HasWindowHandle,
        preference: bool,
        blur: bool,
    ) -> Result<(), String> {
        let hwnd = match window.window_handle().map_err(|e| e.to_string())?.as_raw() {
            RawWindowHandle::Win32(h) => HWND(h.hwnd.get() as *mut _),
            _ => return refuse(preference, Desktop::Other),
        };
        refuse(preference, Desktop::Windows { build: build() })?;
        // SAFETY: `hwnd` is the live top-level window baseview handed us;
        // the margins outlive the call.
        unsafe {
            set_backdrop(hwnd, DWM_SYSTEMBACKDROP_TYPE(super::backdrop(blur)))?;
            DwmExtendFrameIntoClientArea(hwnd, &margins_of([-1; 4]))
                .map_err(|e| format!("DwmExtendFrameIntoClientArea: {e}"))?;
            erase_black(hwnd, true);
        }
        Ok(())
    }

    /// The Windows 11 system backdrop behind the host's top-level window
    /// (none at Blur 0: see-through, sharp), with its frame extended into
    /// the client area under the editor's child window (baseview erases it
    /// to black), so it shows through the editor's DirectComposition alpha.
    /// Holds what it found to put back: the backdrop type it read, and no
    /// extended frame (the only margins a window with a system caption has;
    /// DWM cannot read them back).
    pub struct HostBlur {
        editor: HWND,
        root: HWND,
        backdrop: DWM_SYSTEMBACKDROP_TYPE,
        margins: [i32; 4],
    }

    impl HostBlur {
        pub fn new(window: &impl HasWindowHandle, blur: bool) -> Result<Self, String> {
            let editor = match window.window_handle().map_err(|e| e.to_string())?.as_raw() {
                RawWindowHandle::Win32(h) => HWND(h.hwnd.get() as *mut _),
                _ => return Err("the editor is not a Win32 window".to_owned()),
            };
            refuse(true, Desktop::Windows { build: build() })?;
            // SAFETY: `editor` is the live child window baseview handed us;
            // every call below only reads or sets attributes of live windows
            // through pointers to locals that outlive it.
            unsafe {
                let root = GetAncestor(editor, GA_ROOT);
                if root.is_invalid() || root == editor {
                    return Err("the editor has no host window".to_owned());
                }
                let mut pid = 0u32;
                GetWindowThreadProcessId(root, Some(&raw mut pid));
                if pid != GetCurrentProcessId() {
                    return Err("the host window belongs to another process".to_owned());
                }
                let style = GetWindowLongW(root, GWL_STYLE) as u32;
                let mut frame = RECT::default();
                GetWindowRect(root, &raw mut frame).map_err(|e| e.to_string())?;
                let mut origin = [POINT::default()];
                MapWindowPoints(Some(root), None, &mut origin);
                let caption = style & WS_CAPTION.0 == WS_CAPTION.0;
                if !super::system_caption(caption, origin[0].y - frame.top) {
                    return Err("the host window draws its own frame".to_owned());
                }
                let mut backdrop = DWM_SYSTEMBACKDROP_TYPE::default();
                DwmGetWindowAttribute(
                    root,
                    DWMWA_SYSTEMBACKDROP_TYPE,
                    (&raw mut backdrop).cast(),
                    size_of_val(&backdrop) as u32,
                )
                .map_err(|e| format!("DWMWA_SYSTEMBACKDROP_TYPE: {e}"))?;
                // From here, dropping `host` puts the host window back.
                let mut host = Self {
                    editor,
                    root,
                    backdrop,
                    margins: [0; 4],
                };
                if !host.sync() {
                    return Err("the editor does not fill its host window".to_owned());
                }
                set_backdrop(root, DWM_SYSTEMBACKDROP_TYPE(super::backdrop(blur)))?;
                erase_black(editor, true);
                Ok(host)
            }
        }

        /// Blur what shows through, or not ([`super::blurs`]), live.
        pub fn set_blur(&mut self, blur: bool) {
            // SAFETY: `root` is checked live.
            unsafe {
                if IsWindow(Some(self.root)).as_bool() {
                    let _ = set_backdrop(self.root, DWM_SYSTEMBACKDROP_TYPE(super::backdrop(blur)));
                }
            }
        }

        /// Keep the extended frame under the editor as the host moves or
        /// resizes it. `false` once it no longer can: drop it.
        pub fn sync(&mut self) -> bool {
            let Some(margins) = self.placement().and_then(|(c, e)| super::band(c, e)) else {
                return false;
            };
            if margins != self.margins {
                let inset = margins_of(margins);
                // SAFETY: `root` is checked live; `inset` outlives the call.
                let extended = unsafe {
                    IsWindow(Some(self.root)).as_bool()
                        && DwmExtendFrameIntoClientArea(self.root, &inset).is_ok()
                };
                if !extended {
                    return false;
                }
                self.margins = margins;
            }
            true
        }

        /// The host's client size and the editor's rectangle in it.
        fn placement(&self) -> Option<HostPlacement> {
            let (mut client, mut editor) = (RECT::default(), RECT::default());
            // SAFETY: plain queries on window handles; a dead one fails.
            unsafe {
                GetClientRect(self.root, &raw mut client).ok()?;
                GetWindowRect(self.editor, &raw mut editor).ok()?;
                let mut corners = [
                    POINT {
                        x: editor.left,
                        y: editor.top,
                    },
                    POINT {
                        x: editor.right,
                        y: editor.bottom,
                    },
                ];
                MapWindowPoints(None, Some(self.root), &mut corners);
                let [a, b] = corners;
                Some(((client.right, client.bottom), (a.x, a.y, b.x, b.y)))
            }
        }
    }

    impl Drop for HostBlur {
        fn drop(&mut self) {
            // SAFETY: each window is checked live; the values outlive the calls.
            unsafe {
                if IsWindow(Some(self.editor)).as_bool() {
                    erase_black(self.editor, false);
                }
                if !IsWindow(Some(self.root)).as_bool() {
                    return;
                }
                if self.margins != [0; 4] {
                    let _ = DwmExtendFrameIntoClientArea(self.root, &margins_of([0; 4]));
                }
                let _ = set_backdrop(self.root, self.backdrop);
            }
        }
    }

    /// Have baseview erase `hwnd` to black from now on (erasing it now), or
    /// stop: its `baseview.EraseBlack` property. Other windows erase as ever.
    ///
    /// SAFETY: `hwnd` must be a live window of this process.
    unsafe fn erase_black(hwnd: HWND, on: bool) {
        let name = windows::core::w!("baseview.EraseBlack");
        // SAFETY: the caller's; `name` is a static string. Any non-null
        // value turns it on: the window's own handle is one.
        unsafe {
            if on {
                let _ = SetPropW(hwnd, name, Some(HANDLE(hwnd.0)));
                let _ = InvalidateRect(Some(hwnd), None, true);
            } else {
                let _ = RemovePropW(hwnd, name);
            }
        }
    }

    fn margins_of([left, right, top, bottom]: [i32; 4]) -> MARGINS {
        MARGINS {
            cxLeftWidth: left,
            cxRightWidth: right,
            cyTopHeight: top,
            cyBottomHeight: bottom,
        }
    }

    /// SAFETY: `hwnd` must be a live top-level window.
    unsafe fn set_backdrop(hwnd: HWND, kind: DWM_SYSTEMBACKDROP_TYPE) -> Result<(), String> {
        // SAFETY: the caller's; `kind` outlives the call and its size is the attribute's.
        unsafe {
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_SYSTEMBACKDROP_TYPE,
                (&raw const kind).cast(),
                size_of_val(&kind) as u32,
            )
        }
        .map_err(|e| format!("DWMWA_SYSTEMBACKDROP_TYPE: {e}"))
    }

    /// The Windows build number, `0` if unknown.
    fn build() -> u32 {
        use windows_sys::Wdk::System::SystemServices::RtlGetVersion;
        use windows_sys::Win32::System::SystemInformation::OSVERSIONINFOW;
        let mut version = OSVERSIONINFOW {
            dwOSVersionInfoSize: size_of::<OSVERSIONINFOW>() as u32,
            ..OSVERSIONINFOW::default()
        };
        // SAFETY: `version` carries its size and points to writable memory.
        if unsafe { RtlGetVersion(&mut version) } >= 0 {
            version.dwBuildNumber
        } else {
            0
        }
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use objc2::rc::Retained;
    use objc2::{MainThreadMarker, MainThreadOnly};
    use objc2_app_kit::{
        NSAutoresizingMaskOptions, NSColor, NSView, NSVisualEffectBlendingMode,
        NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView, NSWindowOrderingMode,
    };
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    use super::{Desktop, refuse};

    /// The window's content view clear over the desktop, with an effect
    /// view under it blurring the desktop only if `blur`.
    pub(super) fn blur_behind(
        window: &impl HasWindowHandle,
        preference: bool,
        blur: bool,
    ) -> Result<(), String> {
        let ns_view: *mut NSView = match window.window_handle().map_err(|e| e.to_string())?.as_raw()
        {
            RawWindowHandle::AppKit(h) => h.ns_view.as_ptr().cast(),
            _ => return refuse(preference, Desktop::Other),
        };
        refuse(preference, Desktop::MacOs)?;
        let mtm = MainThreadMarker::new().ok_or("blur behind must run on the main thread")?;
        // SAFETY: baseview hands out a live NSView for the open window, and
        // this runs on the AppKit main thread (checked above).
        let view = unsafe { &*ns_view };
        let window = view.window().ok_or("the view is not in a window")?;
        // SAFETY: main thread (checked above); the frame view outlives this call.
        let frame = unsafe { view.superview() }.ok_or("the content view has no frame view")?;
        match (under(&frame, view), blur) {
            (None, true) => {
                let effect = effect_under(mtm, &frame, view);
                effect.setAutoresizingMask(
                    NSAutoresizingMaskOptions::ViewWidthSizable
                        | NSAutoresizingMaskOptions::ViewHeightSizable,
                );
            }
            (Some(effect), false) => effect.removeFromSuperview(),
            _ => {}
        }
        window.setOpaque(false);
        window.setBackgroundColor(Some(&NSColor::clearColor()));
        Ok(())
    }

    /// The effect view [`effect_under`] put directly under `view` in
    /// `host`, if there is one.
    fn under(host: &NSView, view: &NSView) -> Option<Retained<NSVisualEffectView>> {
        let subviews = host.subviews();
        let at = (0..subviews.count()).find(|&i| &*subviews.objectAtIndex(i) == view)?;
        subviews.objectAtIndex(at.checked_sub(1)?).downcast().ok()
    }

    /// A behind-window effect view directly under `view` in `host`, at its frame.
    fn effect_under(
        mtm: MainThreadMarker,
        host: &NSView,
        view: &NSView,
    ) -> Retained<NSVisualEffectView> {
        let effect =
            NSVisualEffectView::initWithFrame(NSVisualEffectView::alloc(mtm), view.frame());
        effect.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
        effect.setMaterial(NSVisualEffectMaterial::UnderWindowBackground);
        // A plugin or panel window is seldom key: stay frosted when inactive.
        effect.setState(NSVisualEffectState::Active);
        host.addSubview_positioned_relativeTo(&effect, NSWindowOrderingMode::Below, Some(view));
        effect
    }

    /// An `NSVisualEffectView` blending behind the host's window, directly
    /// under the editor's view in the host's view and sized to it (none at
    /// Blur 0); the editor's non-opaque Metal layer shows it through. The
    /// host's window is left as it is: behind-window blending samples the
    /// desktop in an opaque window too. Dropping it removes the view.
    pub struct HostBlur {
        mtm: MainThreadMarker,
        view: Retained<NSView>,
        effect: Option<Retained<NSVisualEffectView>>,
    }

    impl HostBlur {
        pub fn new(window: &impl HasWindowHandle, blur: bool) -> Result<Self, String> {
            let ns_view: *mut NSView =
                match window.window_handle().map_err(|e| e.to_string())?.as_raw() {
                    RawWindowHandle::AppKit(h) => h.ns_view.as_ptr().cast(),
                    _ => return Err("the editor is not an AppKit view".to_owned()),
                };
            refuse(true, Desktop::MacOs)?;
            let mtm = MainThreadMarker::new().ok_or("blur behind must run on the main thread")?;
            // SAFETY: baseview hands out a live NSView for the open editor.
            let view = unsafe { Retained::retain(ns_view) }.ok_or("no editor view")?;
            // SAFETY: main thread (checked above).
            unsafe { view.superview() }.ok_or("the editor is not in a host view")?;
            let mut host = Self {
                mtm,
                view,
                effect: None,
            };
            host.set_blur(blur);
            Ok(host)
        }

        /// Blur what shows through, or not ([`super::blurs`]), live.
        pub fn set_blur(&mut self, blur: bool) {
            match (&self.effect, blur) {
                (None, true) => {
                    // SAFETY: main thread: `mtm` proves it.
                    if let Some(host) = unsafe { self.view.superview() } {
                        self.effect = Some(effect_under(self.mtm, &host, &self.view));
                    }
                }
                (Some(effect), false) => {
                    effect.removeFromSuperview();
                    self.effect = None;
                }
                _ => {}
            }
        }

        /// Keep the effect view under the editor as the host resizes it.
        /// `false` once the editor left the view it was in: drop it.
        pub fn sync(&mut self) -> bool {
            // SAFETY: main thread: the editor's frames run on it.
            let host = unsafe { self.view.superview() };
            let Some(effect) = &self.effect else {
                return host.is_some();
            };
            // SAFETY: as above.
            if host.is_none() || host != unsafe { effect.superview() } {
                return false;
            }
            let frame = self.view.frame();
            if effect.frame() != frame {
                effect.setFrame(frame);
            }
            true
        }
    }

    impl Drop for HostBlur {
        fn drop(&mut self) {
            if let Some(effect) = &self.effect {
                effect.removeFromSuperview();
            }
        }
    }
}

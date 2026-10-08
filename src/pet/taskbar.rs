//! Screen and taskbar detection for anchoring the desktop pet.
#![allow(dead_code)]

#[derive(Debug, Clone, Copy)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    pub fn width(&self) -> i32 {
        (self.right - self.left).max(0)
    }

    pub fn height(&self) -> i32 {
        (self.bottom - self.top).max(0)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct TaskbarInfo {
    pub bounds: Rect,
    pub tray_target: (f32, f32),
}

/// Query the Windows taskbar and system tray location.
#[cfg(windows)]
pub fn get_taskbar_info() -> Option<TaskbarInfo> {
    use std::mem::MaybeUninit;
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::UI::WindowsAndMessaging::{FindWindowExW, FindWindowW, GetWindowRect};

    unsafe {
        // "Shell_TrayWnd" is the standard Windows taskbar window class name.
        let class_name: Vec<u16> = "Shell_TrayWnd\0".encode_utf16().collect();
        let hwnd = FindWindowW(class_name.as_ptr(), std::ptr::null());
        if hwnd == std::ptr::null_mut() {
            return fallback_taskbar_info();
        }

        let mut rect = MaybeUninit::<RECT>::uninit();
        if GetWindowRect(hwnd, rect.as_mut_ptr()) == 0 {
            return fallback_taskbar_info();
        }
        let r = rect.assume_init();
        let taskbar_rect = Rect {
            left: r.left,
            top: r.top,
            right: r.right,
            bottom: r.bottom,
        };

        // Locate TrayNotifyWnd (the notification tray icons area)
        let tray_class: Vec<u16> = "TrayNotifyWnd\0".encode_utf16().collect();
        let tray_hwnd = FindWindowExW(hwnd, std::ptr::null_mut(), tray_class.as_ptr(), std::ptr::null());

        let tray_target = if tray_hwnd != std::ptr::null_mut() {
            let mut tray_rect = MaybeUninit::<RECT>::uninit();
            if GetWindowRect(tray_hwnd, tray_rect.as_mut_ptr()) != 0 {
                let tr = tray_rect.assume_init();
                (
                    (tr.left + (tr.right - tr.left) / 2) as f32,
                    (tr.top + (tr.bottom - tr.top) / 2) as f32,
                )
            } else {
                default_tray_target(&taskbar_rect)
            }
        } else {
            default_tray_target(&taskbar_rect)
        };

        Some(TaskbarInfo {
            bounds: taskbar_rect,
            tray_target,
        })
    }
}

#[cfg(windows)]
fn default_tray_target(taskbar: &Rect) -> (f32, f32) {
    (
        (taskbar.right - 100).max(taskbar.left) as f32,
        (taskbar.top + taskbar.height() / 2) as f32,
    )
}

#[cfg(windows)]
fn fallback_taskbar_info() -> Option<TaskbarInfo> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};
    unsafe {
        let sw = GetSystemMetrics(SM_CXSCREEN);
        let sh = GetSystemMetrics(SM_CYSCREEN);
        let taskbar_h = 48;
        let bounds = Rect {
            left: 0,
            top: (sh - taskbar_h).max(0),
            right: sw,
            bottom: sh,
        };
        let tray_target = ((sw - 120) as f32, (sh - taskbar_h / 2) as f32);
        Some(TaskbarInfo { bounds, tray_target })
    }
}

#[cfg(not(windows))]
pub fn get_taskbar_info() -> Option<TaskbarInfo> {
    let bounds = Rect {
        left: 0,
        top: 720,
        right: 1280,
        bottom: 768,
    };
    let tray_target = (1160.0, 744.0);
    Some(TaskbarInfo { bounds, tray_target })
}

#[cfg(windows)]
pub fn get_global_cursor_pos() -> Option<(f32, f32)> {
    use windows_sys::Win32::Foundation::POINT;
    extern "system" {
        fn GetCursorPos(lpPoint: *mut POINT) -> i32;
    }
    unsafe {
        let mut pt = POINT { x: 0, y: 0 };
        if GetCursorPos(&mut pt) != 0 {
            Some((pt.x as f32, pt.y as f32))
        } else {
            None
        }
    }
}

/// A shared, lazily-opened connection to the X server (XWayland), if any.
#[cfg(target_os = "linux")]
fn x11_conn() -> Option<&'static x11rb::rust_connection::RustConnection> {
    use std::sync::OnceLock;

    static CONN: OnceLock<Option<x11rb::rust_connection::RustConnection>> = OnceLock::new();
    CONN.get_or_init(|| x11rb::connect(None).ok().map(|(conn, _screen)| conn))
        .as_ref()
}

/// Whether the app can drive X11 (XWayland) helpers such as window styling and
/// shaped input regions. Cheap after the first call (connection is cached).
#[cfg(target_os = "linux")]
pub fn x11_available() -> bool {
    x11_conn().is_some()
}

#[cfg(not(target_os = "linux"))]
pub fn x11_available() -> bool {
    false
}

/// Query the X11 pointer position (physical pixels on the XWAYLAND/X screen).
///
/// The pet overlay is click-through, so it never receives pointer events from
/// the compositor; polling the global pointer is the only way to support
/// dragging the pet and clicking the papers. Returns `None` without a usable X
/// connection (e.g. a pure-Wayland session), which disables those gestures.
#[cfg(target_os = "linux")]
fn x11_pointer() -> Option<(f32, f32, u16)> {
    use x11rb::connection::Connection as _;
    use x11rb::protocol::xproto::ConnectionExt as _;

    let conn = x11_conn()?;
    let root = conn.setup().roots.first()?.root;
    let reply = conn.query_pointer(root).ok()?.reply().ok()?;
    Some((reply.root_x as f32, reply.root_y as f32, reply.mask.into()))
}

/// Add or remove `_NET_WM_STATE_SKIP_TASKBAR`/`_NET_WM_STATE_SKIP_PAGER` on the
/// window whose title matches `title`.
///
/// winit only implements "skip taskbar" on Windows, so on Linux a mapped window
/// always appears in the GNOME dock and the Activities overview — including the
/// collapsed 1x1 window used to hide the settings UI and the desktop pet. We
/// drive the EWMH state ourselves over X11 (no-op on a pure-Wayland session).
#[cfg(target_os = "linux")]
pub fn x11_set_skip_taskbar(title: &str, skip: bool) {
    use x11rb::connection::Connection as _;
    use x11rb::protocol::xproto::{
        ClientMessageData, ClientMessageEvent, ConnectionExt as _, EventMask,
    };

    let Some(conn) = x11_conn() else {
        return;
    };
    let Some(root) = conn.setup().roots.first().map(|r| r.root) else {
        return;
    };
    let atom = |name: &str| -> Option<u32> {
        conn.intern_atom(false, name.as_bytes())
            .ok()?
            .reply()
            .ok()
            .map(|r| r.atom)
    };
    let (Some(state), Some(skip_tb), Some(skip_pg)) = (
        atom("_NET_WM_STATE"),
        atom("_NET_WM_STATE_SKIP_TASKBAR"),
        atom("_NET_WM_STATE_SKIP_PAGER"),
    ) else {
        return;
    };

    for window in x11_find_windows_by_title(conn, title) {
        for property in [skip_tb, skip_pg] {
            let event = ClientMessageEvent::new(
                32,
                window,
                state,
                ClientMessageData::from([action(skip), property, 0, 0, 0]),
            );
            let _ = conn.send_event(
                false,
                root,
                EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
                event,
            );
        }
    }
    let _ = conn.flush();
}

/// 0 = `_NET_WM_STATE_REMOVE`, 1 = `_NET_WM_STATE_ADD`
#[cfg(target_os = "linux")]
fn action(skip: bool) -> u32 {
    u32::from(skip)
}

/// All managed windows whose `_NET_WM_NAME`/`WM_NAME` equals `title`.
#[cfg(target_os = "linux")]
fn x11_find_windows_by_title(
    conn: &x11rb::rust_connection::RustConnection,
    title: &str,
) -> Vec<u32> {
    use x11rb::connection::Connection as _;
    use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _};

    let Some(root) = conn.setup().roots.first().map(|r| r.root) else {
        return Vec::new();
    };
    let Some(list) = conn
        .intern_atom(false, b"_NET_CLIENT_LIST")
        .ok()
        .and_then(|c| c.reply().ok())
        .map(|r| r.atom)
    else {
        return Vec::new();
    };
    let windows: Vec<u32> = conn
        .get_property(false, root, list, AtomEnum::WINDOW, 0, u32::MAX)
        .ok()
        .and_then(|cookie| cookie.reply().ok())
        .and_then(|reply| reply.value32().map(|iter| iter.collect()))
        .unwrap_or_default();

    windows
        .into_iter()
        .filter(|w| x11_window_title(conn, *w).as_deref() == Some(title))
        .collect()
}

/// Restrict the clickable area of the window titled `title` (the desktop-pet
/// overlay) to `rects`, given in window-local physical pixels.
///
/// Everything outside these rects stays click-through, so clicks keep reaching
/// the desktop below. Clicks that land inside the rects are received by the
/// pet's own X11 window — and that is what makes dragging the pet work on
/// GNOME/XWayland: a click on the sprite starts an implicit pointer grab that
/// keeps pointer motion flowing to the pet window until release, even when the
/// cursor travels over native Wayland surfaces (global polling cannot see that
/// motion: `QueryPointer` freezes and raw events stop outside X windows).
///
/// An empty `rects` list makes the window fully click-through again.
#[cfg(target_os = "linux")]
pub fn x11_set_input_regions(title: &str, rects: &[(i32, i32, i32, i32)]) {
    use x11rb::connection::Connection as _;
    use x11rb::protocol::shape::{ConnectionExt as _, SK, SO};
    use x11rb::protocol::xproto::{ClipOrdering, Rectangle};

    let Some(conn) = x11_conn() else {
        return;
    };
    let Some(window) = x11_find_windows_by_title(conn, title).into_iter().next() else {
        return;
    };
    let shapes: Vec<Rectangle> = rects
        .iter()
        .filter(|(_, _, w, h)| *w > 0 && *h > 0)
        .map(|(x, y, w, h)| Rectangle {
            x: (*x).clamp(i16::MIN as i32, i16::MAX as i32) as i16,
            y: (*y).clamp(i16::MIN as i32, i16::MAX as i32) as i16,
            width: (*w).clamp(1, u16::MAX as i32) as u16,
            height: (*h).clamp(1, u16::MAX as i32) as u16,
        })
        .collect();
    let result = conn.shape_rectangles(
        SO::SET,
        SK::INPUT,
        ClipOrdering::UNSORTED,
        window,
        0,
        0,
        &shapes,
    );
    if let Err(err) = result {
        // Missing shape extension / not an X window (e.g. the app is on the
        // Wayland backend) — retrying will not help.
        log::debug!("x11_set_input_regions failed for '{title}': {err}");
    }
    let _ = conn.flush();
}

#[cfg(not(target_os = "linux"))]
pub fn x11_set_input_regions(_title: &str, _rects: &[(i32, i32, i32, i32)]) {}

/// Add or remove the titlebar/borders of the window titled `title` via
/// `_MOTIF_WM_HINTS` (mutter honours this for XWayland clients).
///
/// GNOME decorates XWayland windows with a separate frame window. When the
/// settings window is parked (collapsed to a 1x1 client) that frame is still
/// drawn as a titlebar-sized rectangle. Turning decorations off removes the
/// frame so the parked window is truly invisible; turning them back on
/// restores the titlebar when the settings window is shown again.
#[cfg(target_os = "linux")]
pub fn x11_set_decorations(title: &str, decorated: bool) {
    use x11rb::connection::Connection as _;
    use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _, PropMode};
    use x11rb::wrapper::ConnectionExt as _;

    let Some(conn) = x11_conn() else {
        return;
    };
    let Some(hints) = conn
        .intern_atom(false, b"_MOTIF_WM_HINTS")
        .ok()
        .and_then(|c| c.reply().ok())
        .map(|r| r.atom)
    else {
        return;
    };
    // flags = 2 (_MOTIF_WM_HINTS_DECORATIONS), functions = 0,
    // decorations = 0/1, input_mode = 0, status = 0.
    let values = [2u32, 0, u32::from(decorated), 0, 0];
    for window in x11_find_windows_by_title(conn, title) {
        let _ = conn.change_property32(
            PropMode::REPLACE,
            window,
            hints,
            AtomEnum::CARDINAL,
            &values,
        );
    }
    let _ = conn.flush();
}

#[cfg(not(target_os = "linux"))]
pub fn x11_set_decorations(_title: &str, _decorated: bool) {}

/// Style a specific X11 window by id, before or after it is mapped.
///
/// Unlike the title-based helpers above (which can only find a window once the
/// WM has added it to `_NET_CLIENT_LIST`, i.e. after it is already in the
/// dock), this one works on a window that has been created but not yet mapped.
/// Applying the decoration hint and the initial `_NET_WM_STATE` here is what
/// stops an autostart launch from flashing into the dock/taskbar.
#[cfg(target_os = "linux")]
pub fn x11_style_window(window: u32, skip_taskbar: bool, decorated: bool) {
    use x11rb::connection::Connection as _;
    use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _, PropMode};
    use x11rb::wrapper::ConnectionExt as _;

    let Some(conn) = x11_conn() else {
        return;
    };
    let atom = |name: &[u8]| -> Option<u32> {
        conn.intern_atom(false, name)
            .ok()?
            .reply()
            .ok()
            .map(|r| r.atom)
    };

    if let Some(hints) = atom(b"_MOTIF_WM_HINTS") {
        let values = [2u32, 0, u32::from(decorated), 0, 0];
        let _ = conn.change_property32(
            PropMode::REPLACE,
            window,
            hints,
            AtomEnum::CARDINAL,
            &values,
        );
    }
    if skip_taskbar {
        if let (Some(state), Some(tb), Some(pg)) = (
            atom(b"_NET_WM_STATE"),
            atom(b"_NET_WM_STATE_SKIP_TASKBAR"),
            atom(b"_NET_WM_STATE_SKIP_PAGER"),
        ) {
            let _ = conn.change_property32(
                PropMode::REPLACE,
                window,
                state,
                AtomEnum::ATOM,
                &[tb, pg],
            );
        }
    }
    let _ = conn.flush();
}

#[cfg(not(target_os = "linux"))]
pub fn x11_style_window(_window: u32, _skip_taskbar: bool, _decorated: bool) {}

#[cfg(target_os = "linux")]
fn x11_window_title(conn: &x11rb::rust_connection::RustConnection, window: u32) -> Option<String> {
    use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _};

    let net_wm_name = conn
        .intern_atom(false, b"_NET_WM_NAME")
        .ok()?
        .reply()
        .ok()?
        .atom;
    let utf8 = conn
        .intern_atom(false, b"UTF8_STRING")
        .ok()?
        .reply()
        .ok()?
        .atom;
    if let Ok(reply) = conn
        .get_property(false, window, net_wm_name, utf8, 0, 256)
        .ok()?
        .reply()
    {
        if !reply.value.is_empty() {
            return Some(String::from_utf8_lossy(&reply.value).into_owned());
        }
    }
    let reply = conn
        .get_property(false, window, AtomEnum::WM_NAME, AtomEnum::STRING, 0, 256)
        .ok()?
        .reply()
        .ok()?;
    Some(String::from_utf8_lossy(&reply.value).into_owned())
}

#[cfg(not(target_os = "linux"))]
pub fn x11_set_skip_taskbar(_title: &str, _skip: bool) {}

#[cfg(target_os = "linux")]
pub fn get_global_cursor_pos() -> Option<(f32, f32)> {
    // Prefer the position from raw events when it is fresh. On XWayland the
    // raw events are the only source that keeps updating while the pointer is
    // grabbed to an X11 window (i.e. while the pet is being dragged); a plain
    // QueryPointer freezes as soon as the pointer is over a Wayland surface.
    x11_raw::start();
    if let Some((x, y, age_ms)) = x11_raw::pointer_hint() {
        if age_ms < 250 {
            return Some((x, y));
        }
    }
    x11_pointer().map(|(x, y, _)| (x, y))
}

#[cfg(not(any(windows, target_os = "linux")))]
pub fn get_global_cursor_pos() -> Option<(f32, f32)> {
    None
}

#[cfg(windows)]
pub fn is_lbutton_pressed() -> bool {
    extern "system" {
        fn GetAsyncKeyState(vKey: i32) -> i16;
    }
    unsafe {
        // VK_LBUTTON = 0x01
        (GetAsyncKeyState(0x01) as u16 & 0x8000) != 0
    }
}

/// Global left-button state, tracked with XInput2 raw events.
///
/// A plain `QueryPointer` mask does not see a click that lands on a Wayland
/// surface — which is exactly what happens when clicking a click-through
/// overlay on GNOME — so the pet could never be dragged. XInput2 *raw* events
/// are device-global and independent of the window under the pointer, so a
/// background thread listens for them and publishes the button state.
#[cfg(target_os = "linux")]
mod x11_raw {
    use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, Ordering};
    use std::sync::Once;

    use x11rb::connection::Connection as _;
    use x11rb::protocol::xinput::{self, ConnectionExt as _};
    use x11rb::protocol::Event;

    static START: Once = Once::new();
    static BUTTON1: AtomicBool = AtomicBool::new(false);
    /// Pointer position (physical X-screen pixels) seen in the most recent raw
    /// event, and the `Instant` (millis) at which it was seen.
    static POS_X: AtomicI32 = AtomicI32::new(i32::MIN);
    static POS_Y: AtomicI32 = AtomicI32::new(i32::MIN);
    static POS_AT: AtomicU32 = AtomicU32::new(0);

    /// Millis since the process started; timestamp source for `POS_AT`.
    fn now_ms() -> u32 {
        use std::sync::OnceLock;
        static START_TIME: OnceLock<std::time::Instant> = OnceLock::new();
        let start = START_TIME.get_or_init(std::time::Instant::now);
        start.elapsed().as_millis().min(u32::MAX as u128) as u32
    }

    pub fn start() {
        START.call_once(|| {
            std::thread::spawn(listen);
        });
    }

    pub fn button1_down() -> bool {
        BUTTON1.load(Ordering::Relaxed)
    }

    /// Most recent raw-event pointer position and its age in milliseconds.
    ///
    /// Raw motion carries device-global screen coordinates that remain correct
    /// while the pointer is grabbed by an X11 window (exactly the situation
    /// while the pet is being dragged). `QueryPointer`, in contrast, reports a
    /// frozen position whenever the pointer is over a native Wayland surface.
    pub fn pointer_hint() -> Option<(f32, f32, u32)> {
        let at = POS_AT.load(Ordering::Relaxed);
        if at == 0 {
            return None;
        }
        let x = POS_X.load(Ordering::Relaxed);
        let y = POS_Y.load(Ordering::Relaxed);
        if x == i32::MIN || y == i32::MIN {
            return None;
        }
        let age = now_ms().wrapping_sub(at);
        Some((x as f32, y as f32, age))
    }

    fn store_pos(ev: &x11rb::protocol::xinput::RawButtonPressEvent) {
        let vals = &ev.axisvalues_raw;
        if vals.len() >= 2 {
            let x = vals[0].integral + (vals[0].frac as f64 / (1u64 << 32) as f64).round() as i32;
            let y = vals[1].integral + (vals[1].frac as f64 / (1u64 << 32) as f64).round() as i32;
            POS_X.store(x, Ordering::Relaxed);
            POS_Y.store(y, Ordering::Relaxed);
            POS_AT.store(now_ms(), Ordering::Relaxed);
        }
    }

    fn listen() {
        let Ok((conn, screen_num)) = x11rb::connect(None) else {
            return;
        };
        let Some(root) = conn.setup().roots.get(screen_num).map(|r| r.root) else {
            return;
        };
        let mask = xinput::XIEventMask::RAW_BUTTON_PRESS
            | xinput::XIEventMask::RAW_BUTTON_RELEASE
            | xinput::XIEventMask::RAW_MOTION;
        // deviceid 1 == XIAllMasterDevices
        let events = xinput::EventMask {
            deviceid: xinput::DeviceId::from(1u16),
            mask: vec![mask],
        };
        if conn.xinput_xi_select_events(root, &[events]).is_err() {
            return;
        }
        if conn.flush().is_err() {
            return;
        }
        loop {
            match conn.wait_for_event() {
                Ok(Event::XinputRawButtonPress(ev)) => {
                    store_pos(&ev);
                    if ev.detail == 1 {
                        BUTTON1.store(true, Ordering::Relaxed);
                        log::debug!(
                            "raw button1 press at ({},{})",
                            POS_X.load(Ordering::Relaxed),
                            POS_Y.load(Ordering::Relaxed)
                        );
                    }
                }
                Ok(Event::XinputRawButtonRelease(ev)) => {
                    store_pos(&ev);
                    if ev.detail == 1 {
                        BUTTON1.store(false, Ordering::Relaxed);
                        log::debug!("raw button1 release");
                    }
                }
                Ok(Event::XinputRawMotion(ev)) => {
                    store_pos(&ev);
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
    }
}

#[cfg(target_os = "linux")]
pub fn is_lbutton_pressed() -> bool {
    x11_raw::start();
    x11_raw::button1_down()
}

#[cfg(not(any(windows, target_os = "linux")))]
pub fn is_lbutton_pressed() -> bool {
    false
}

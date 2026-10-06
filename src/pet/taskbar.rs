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
        AtomEnum, ClientMessageData, ClientMessageEvent, ConnectionExt as _, EventMask,
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
    let (Some(state), Some(skip_tb), Some(skip_pg), Some(list)) = (
        atom("_NET_WM_STATE"),
        atom("_NET_WM_STATE_SKIP_TASKBAR"),
        atom("_NET_WM_STATE_SKIP_PAGER"),
        atom("_NET_CLIENT_LIST"),
    ) else {
        return;
    };

    let windows: Vec<u32> = conn
        .get_property(false, root, list, AtomEnum::WINDOW, 0, u32::MAX)
        .ok()
        .and_then(|cookie| cookie.reply().ok())
        .and_then(|reply| reply.value32().map(|iter| iter.collect()))
        .unwrap_or_default();

    // 0 = _NET_WM_STATE_REMOVE, 1 = _NET_WM_STATE_ADD
    let action = if skip { 1u32 } else { 0u32 };
    for window in windows {
        if x11_window_title(conn, window).as_deref() != Some(title) {
            continue;
        }
        for property in [skip_tb, skip_pg] {
            let event = ClientMessageEvent::new(
                32,
                window,
                state,
                ClientMessageData::from([action, property, 0, 0, 0]),
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
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Once;

    use x11rb::connection::Connection as _;
    use x11rb::protocol::xinput::{self, ConnectionExt as _};
    use x11rb::protocol::Event;

    static START: Once = Once::new();
    static BUTTON1: AtomicBool = AtomicBool::new(false);

    pub fn start() {
        START.call_once(|| {
            std::thread::spawn(listen);
        });
    }

    pub fn button1_down() -> bool {
        BUTTON1.load(Ordering::Relaxed)
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
                    if ev.detail == 1 {
                        BUTTON1.store(true, Ordering::Relaxed);
                    }
                }
                Ok(Event::XinputRawButtonRelease(ev)) => {
                    if ev.detail == 1 {
                        BUTTON1.store(false, Ordering::Relaxed);
                    }
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

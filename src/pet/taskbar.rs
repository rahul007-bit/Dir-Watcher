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

#[cfg(windows)]
pub fn apply_pet_window_transparency(title: &str) -> bool {
    use std::sync::atomic::{AtomicIsize, Ordering};
    static LAST_HWND: AtomicIsize = AtomicIsize::new(0);

    use windows_sys::Win32::UI::WindowsAndMessaging::{
        FindWindowW, GetWindowLongW, SetLayeredWindowAttributes, SetWindowLongW,
        GWL_EXSTYLE, LWA_COLORKEY, WS_EX_LAYERED, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT,
    };

    let title_wide: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let hwnd = FindWindowW(std::ptr::null(), title_wide.as_ptr());
        if hwnd != std::ptr::null_mut() {
            let hwnd_val = hwnd as isize;
            if LAST_HWND.load(Ordering::SeqCst) == hwnd_val {
                return true;
            }

            // Apply layered, toolwindow (no taskbar item / no alt-tab), and transparent (desktop pass-through)
            let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE);
            let new_style = ex_style | (WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TRANSPARENT) as i32;
            SetWindowLongW(hwnd, GWL_EXSTYLE, new_style);

            // Colorkey: Treat black 0x00000000 (0,0,0) as 100% transparent desktop alpha!
            SetLayeredWindowAttributes(hwnd, 0x00000000, 255, LWA_COLORKEY);

            LAST_HWND.store(hwnd_val, Ordering::SeqCst);
            log::info!("Applied desktop layered colorkey transparency to pet window: {hwnd_val}");
            return true;
        }
    }
    false
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

#[cfg(not(windows))]
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

#[cfg(not(windows))]
pub fn is_lbutton_pressed() -> bool {
    false
}

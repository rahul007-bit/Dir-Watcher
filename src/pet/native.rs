//! Native Win32 per-pixel-alpha layered window for the desktop pet.
//!
//! The pet runs on its own thread with its own message pump and is drawn with
//! GDI+ into a 32-bpp premultiplied BGRA DIB, which is presented via
//! `UpdateLayeredWindow`. `PetController` still owns all the logic/state; this
//! module only renders it and talks to the settings UI through `SharedPet`.
#![allow(dead_code)]

use std::ffi::c_void;
use std::mem::size_of;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use windows::core::PCWSTR;
use windows::Win32::Graphics::GdiPlus::*;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, SIZE, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC, SelectObject,
    AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, DIB_RGB_COLORS,
    HDC,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetSystemMetrics,
    PeekMessageW, RegisterClassExW, ShowWindow, TranslateMessage, UpdateLayeredWindow, MSG,
    PM_REMOVE, SM_CXSCREEN, SM_CYSCREEN, SW_HIDE, SW_SHOWNOACTIVATE, ULW_ALPHA, WNDCLASSEXW,
    WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

use super::character::{CharacterKind, CharacterSpec, FILE_ICON, FOLDER_SLOT, ZZZ_SHEET};
use super::{PetController, PetEvent, PetState};

/// GDI+ pixel formats (not exported as constants by the `windows` crate).
const PIXEL_FORMAT_32BPP_ARGB: i32 = 0x0026_200A;
const PIXEL_FORMAT_32BPP_PARGB: i32 = 0x000E_200B;

/// `StringFormatFlagsNoWrap`.
const STRING_FORMAT_FLAGS_NO_WRAP: i32 = 0x0000_1000;

fn argb(a: u8, r: u8, g: u8, b: u8) -> u32 {
    ((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
}

fn state_to_u8(state: PetState) -> u8 {
    match state {
        PetState::Sleeping => 0,
        PetState::Alert => 1,
        PetState::Collecting => 2,
        PetState::WalkingToTray => 3,
        PetState::Arranging => 4,
        PetState::WalkingHome => 5,
        PetState::Stuck => 6,
    }
}

fn u8_to_state(value: u8) -> PetState {
    match value {
        1 => PetState::Alert,
        2 => PetState::Collecting,
        3 => PetState::WalkingToTray,
        4 => PetState::Arranging,
        5 => PetState::WalkingHome,
        6 => PetState::Stuck,
        _ => PetState::Sleeping,
    }
}

/// Commands that cannot be expressed as simple shared values.
pub enum PetCommand {
    HelpClean,
    SayHi(String),
    Realign,
    Shutdown,
}

/// Values shared between the eframe settings UI and the pet thread.
pub struct SharedPet {
    enabled: AtomicBool,
    speed: AtomicU32,
    position_offset: AtomicU32,
    state: AtomicU8,
    frame: AtomicUsize,
    facing_left: AtomicBool,
    dragging: AtomicBool,
    papers: AtomicUsize,
    carried: AtomicUsize,
}

impl SharedPet {
    fn new(enabled: bool, speed: f32, position_offset: f32) -> Self {
        SharedPet {
            enabled: AtomicBool::new(enabled),
            speed: AtomicU32::new(speed.to_bits()),
            position_offset: AtomicU32::new(position_offset.to_bits()),
            state: AtomicU8::new(state_to_u8(PetState::Sleeping)),
            frame: AtomicUsize::new(0),
            facing_left: AtomicBool::new(false),
            dragging: AtomicBool::new(false),
            papers: AtomicUsize::new(0),
            carried: AtomicUsize::new(0),
        }
    }

    pub fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }
    pub fn set_enabled(&self, value: bool) {
        self.enabled.store(value, Ordering::Relaxed);
    }
    pub fn speed(&self) -> f32 {
        f32::from_bits(self.speed.load(Ordering::Relaxed))
    }
    pub fn set_speed(&self, value: f32) {
        self.speed.store(value.to_bits(), Ordering::Relaxed);
    }
    pub fn position_offset(&self) -> f32 {
        f32::from_bits(self.position_offset.load(Ordering::Relaxed))
    }
    pub fn set_position_offset(&self, value: f32) {
        self.position_offset.store(value.to_bits(), Ordering::Relaxed);
    }
    pub fn state(&self) -> PetState {
        u8_to_state(self.state.load(Ordering::Relaxed))
    }
    pub fn frame(&self) -> usize {
        self.frame.load(Ordering::Relaxed)
    }
    pub fn facing_left(&self) -> bool {
        self.facing_left.load(Ordering::Relaxed)
    }
    pub fn dragging(&self) -> bool {
        self.dragging.load(Ordering::Relaxed)
    }
    pub fn paper_count(&self) -> usize {
        self.papers.load(Ordering::Relaxed)
    }
    pub fn carried_count(&self) -> usize {
        self.carried.load(Ordering::Relaxed)
    }
}

/// Global stop flag so the tray Quit handler can ask the pet thread to exit
/// before the process is torn down.
static SHUTDOWN: OnceLock<Arc<AtomicBool>> = OnceLock::new();

pub fn request_shutdown() {
    if let Some(stop) = SHUTDOWN.get() {
        stop.store(true, Ordering::SeqCst);
    }
}

pub struct NativePet {
    shared: Arc<SharedPet>,
    cmd_tx: Sender<PetCommand>,
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl NativePet {
    pub fn spawn(
        enabled: bool,
        speed: f32,
        position_offset: f32,
        rx: Receiver<PetEvent>,
    ) -> NativePet {
        let shared = Arc::new(SharedPet::new(enabled, speed, position_offset));
        let (cmd_tx, cmd_rx) = channel::<PetCommand>();
        let stop = Arc::new(AtomicBool::new(false));
        let _ = SHUTDOWN.set(stop.clone());

        let thread_shared = shared.clone();
        let thread_stop = stop.clone();
        let join = thread::spawn(move || {
            run(
                thread_shared,
                cmd_rx,
                thread_stop,
                enabled,
                speed,
                position_offset,
                rx,
            );
        });

        NativePet {
            shared,
            cmd_tx,
            stop,
            join: Some(join),
        }
    }

    pub fn shared(&self) -> &Arc<SharedPet> {
        &self.shared
    }

    pub fn help_clean(&self) {
        let _ = self.cmd_tx.send(PetCommand::HelpClean);
    }
    pub fn say_hi(&self, text: String) {
        let _ = self.cmd_tx.send(PetCommand::SayHi(text));
    }
    pub fn realign(&self) {
        let _ = self.cmd_tx.send(PetCommand::Realign);
    }
}

impl Drop for NativePet {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

// ---------------------------------------------------------------------------
// GDI+ surface
// ---------------------------------------------------------------------------

/// A PNG decoded into a GDI+ bitmap backed by straight-alpha BGRA memory.
struct GdiImage {
    ptr: *mut GpBitmap,
    _buf: Vec<u8>,
    #[allow(dead_code)]
    w: i32,
    #[allow(dead_code)]
    h: i32,
}

impl GdiImage {
    unsafe fn from_png(bytes: &[u8]) -> Option<GdiImage> {
        let decoded = image::load_from_memory(bytes).ok()?.to_rgba8();
        let w = decoded.width() as i32;
        let h = decoded.height() as i32;
        let mut buf = vec![0u8; (w * h * 4) as usize];
        for (i, px) in decoded.pixels().enumerate() {
            let [r, g, b, a] = px.0;
            buf[i * 4] = b;
            buf[i * 4 + 1] = g;
            buf[i * 4 + 2] = r;
            buf[i * 4 + 3] = a;
        }

        let mut bitmap: *mut GpBitmap = null_mut();
        let status = GdipCreateBitmapFromScan0(
            w,
            h,
            w * 4,
            PIXEL_FORMAT_32BPP_ARGB,
            Some(buf.as_ptr()),
            &mut bitmap,
        );
        if status != Ok || bitmap.is_null() {
            return None;
        }
        Some(GdiImage {
            ptr: bitmap,
            _buf: buf,
            w,
            h,
        })
    }

    fn image(&self) -> *mut GpImage {
        self.ptr as *mut GpImage
    }
}

impl Drop for GdiImage {
    fn drop(&mut self) {
        unsafe {
            if !self.ptr.is_null() {
                GdipDisposeImage(self.image());
            }
        }
    }
}

struct Fonts {
    family: *mut GpFontFamily,
    small: *mut GpFont,
    large: *mut GpFont,
    left: *mut GpStringFormat,
    center: *mut GpStringFormat,
}

impl Drop for Fonts {
    fn drop(&mut self) {
        unsafe {
            if !self.small.is_null() {
                GdipDeleteFont(self.small);
            }
            if !self.large.is_null() {
                GdipDeleteFont(self.large);
            }
            if !self.left.is_null() {
                GdipDeleteStringFormat(self.left);
            }
            if !self.center.is_null() {
                GdipDeleteStringFormat(self.center);
            }
            if !self.family.is_null() {
                GdipDeleteFontFamily(self.family);
            }
        }
    }
}

struct Renderer {
    token: usize,
    character: GdiImage,
    file_icon: GdiImage,
    folder: GdiImage,
    zzz: GdiImage,
    spec: CharacterSpec,
    fonts: Option<Fonts>,
}

impl Renderer {
    unsafe fn new(spec: CharacterSpec) -> Option<Renderer> {
        let mut token = 0usize;
        let input = GdiplusStartupInput {
            GdiplusVersion: 1,
            DebugEventCallback: 0,
            SuppressBackgroundThread: windows::Win32::Foundation::BOOL(0),
            SuppressExternalCodecs: windows::Win32::Foundation::BOOL(0),
        };
        if GdiplusStartup(&mut token, &input, null_mut()) != Ok {
            return None;
        }

        let character = GdiImage::from_png(spec.sheet_bytes)?;
        let file_icon = GdiImage::from_png(FILE_ICON)?;
        let folder = GdiImage::from_png(FOLDER_SLOT)?;
        let zzz = GdiImage::from_png(ZZZ_SHEET)?;

        Some(Renderer {
            token,
            character,
            file_icon,
            folder,
            zzz,
            spec,
            fonts: None,
        })
    }

    unsafe fn ensure_fonts(&mut self) {
        if self.fonts.is_some() {
            return;
        }
        let name: Vec<u16> = "Segoe UI\0".encode_utf16().collect();
        let mut family: *mut GpFontFamily = null_mut();
        if GdipCreateFontFamilyFromName(PCWSTR(name.as_ptr()), null_mut(), &mut family) != Ok
            || family.is_null()
        {
            return;
        }

        let mut small: *mut GpFont = null_mut();
        let mut large: *mut GpFont = null_mut();
        if GdipCreateFont(family, 11.0, 0, UnitPixel, &mut small) != Ok {
            GdipDeleteFontFamily(family);
            return;
        }
        if GdipCreateFont(family, 16.0, 0, UnitPixel, &mut large) != Ok {
            GdipDeleteFont(small);
            GdipDeleteFontFamily(family);
            return;
        }

        let mut left: *mut GpStringFormat = null_mut();
        let mut center: *mut GpStringFormat = null_mut();
        GdipStringFormatGetGenericDefault(&mut left);
        GdipStringFormatGetGenericDefault(&mut center);
        if !left.is_null() {
            GdipSetStringFormatFlags(left, STRING_FORMAT_FLAGS_NO_WRAP);
            GdipSetStringFormatAlign(left, StringAlignmentNear);
            GdipSetStringFormatLineAlign(left, StringAlignmentNear);
        }
        if !center.is_null() {
            GdipSetStringFormatFlags(center, STRING_FORMAT_FLAGS_NO_WRAP);
            GdipSetStringFormatAlign(center, StringAlignmentCenter);
            GdipSetStringFormatLineAlign(center, StringAlignmentCenter);
        }

        self.fonts = Some(Fonts {
            family,
            small,
            large,
            left,
            center,
        });
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        unsafe {
            self.fonts = None;
            if self.token != 0 {
                GdiplusShutdown(self.token);
            }
        }
    }
}

/// A cached top-down 32-bpp premultiplied BGRA DIB used as the layered surface.
struct Canvas {
    w: i32,
    h: i32,
    mem_dc: HDC,
    hbmp: *mut c_void,
    old_bmp: *mut c_void,
    bits: *mut u8,
}

impl Canvas {
    unsafe fn new(screen_dc: HDC, w: i32, h: i32) -> Canvas {
        let mem_dc = CreateCompatibleDC(screen_dc);
        let mut bmi: BITMAPINFO = std::mem::zeroed();
        bmi.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
        bmi.bmiHeader.biWidth = w;
        bmi.bmiHeader.biHeight = -h; // top-down
        bmi.bmiHeader.biPlanes = 1;
        bmi.bmiHeader.biBitCount = 32;
        bmi.bmiHeader.biCompression = BI_RGB;

        let mut bits: *mut c_void = null_mut();
        let hbmp = CreateDIBSection(mem_dc, &bmi, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
        let old_bmp = SelectObject(mem_dc, hbmp as *mut c_void);

        Canvas {
            w,
            h,
            mem_dc,
            hbmp: hbmp as *mut c_void,
            old_bmp,
            bits: bits as *mut u8,
        }
    }

    unsafe fn clear(&self) {
        std::ptr::write_bytes(self.bits, 0, (self.w * self.h * 4) as usize);
    }

    unsafe fn present(&self, screen_dc: HDC, hwnd: HWND, x: i32, y: i32) {
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let dst = POINT { x, y };
        let size = SIZE {
            cx: self.w,
            cy: self.h,
        };
        let src = POINT { x: 0, y: 0 };
        UpdateLayeredWindow(
            hwnd,
            screen_dc,
            &dst,
            &size,
            self.mem_dc,
            &src,
            0,
            &blend,
            ULW_ALPHA,
        );
    }
}

impl Drop for Canvas {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.mem_dc, self.old_bmp);
            DeleteObject(self.hbmp);
            DeleteDC(self.mem_dc);
        }
    }
}

// ---------------------------------------------------------------------------
// Drawing helpers
// ---------------------------------------------------------------------------

struct Gfx {
    g: *mut GpGraphics,
}

impl Gfx {
    unsafe fn fill_rect(&self, color: u32, x: f32, y: f32, w: f32, h: f32) {
        self.fill_rect_i(color, x.round() as i32, y.round() as i32, w.round() as i32, h.round() as i32);
    }

    unsafe fn fill_rect_i(&self, color: u32, x: i32, y: i32, w: i32, h: i32) {
        if w <= 0 || h <= 0 {
            return;
        }
        let mut brush: *mut GpSolidFill = null_mut();
        if GdipCreateSolidFill(color, &mut brush) == Ok && !brush.is_null() {
            GdipFillRectangleI(self.g, brush as *mut GpBrush, x, y, w, h);
            GdipDeleteBrush(brush as *mut GpBrush);
        }
    }

    unsafe fn fill_ellipse(&self, color: u32, cx: f32, cy: f32, rx: f32, ry: f32) {
        let x = (cx - rx).round() as i32;
        let y = (cy - ry).round() as i32;
        let w = (rx * 2.0).round() as i32;
        let h = (ry * 2.0).round() as i32;
        if w <= 0 || h <= 0 {
            return;
        }
        let mut brush: *mut GpSolidFill = null_mut();
        if GdipCreateSolidFill(color, &mut brush) == Ok && !brush.is_null() {
            GdipFillEllipseI(self.g, brush as *mut GpBrush, x, y, w, h);
            GdipDeleteBrush(brush as *mut GpBrush);
        }
    }

    unsafe fn fill_poly(&self, color: u32, points: &[Point]) {
        if points.len() < 3 {
            return;
        }
        let mut brush: *mut GpSolidFill = null_mut();
        if GdipCreateSolidFill(color, &mut brush) == Ok && !brush.is_null() {
            GdipFillPolygonI(
                self.g,
                brush as *mut GpBrush,
                points.as_ptr(),
                points.len() as i32,
                FillModeWinding,
            );
            GdipDeleteBrush(brush as *mut GpBrush);
        }
    }

    /// Rounded rectangle as the union of two rects and four corner ellipses.
    unsafe fn fill_round_rect(&self, color: u32, x: f32, y: f32, w: f32, h: f32, r: f32) {
        let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
        self.fill_rect(color, x + r, y, w - 2.0 * r, h);
        self.fill_rect(color, x, y + r, w, h - 2.0 * r);
        if r > 0.0 {
            self.fill_ellipse(color, x + r, y + r, r, r);
            self.fill_ellipse(color, x + w - r, y + r, r, r);
            self.fill_ellipse(color, x + r, y + h - r, r, r);
            self.fill_ellipse(color, x + w - r, y + h - r, r, r);
        }
    }

    unsafe fn draw_line(&self, color: u32, x1: f32, y1: f32, x2: f32, y2: f32, width: f32) {
        let mut pen: *mut GpPen = null_mut();
        if GdipCreatePen1(color, width, UnitPixel, &mut pen) == Ok && !pen.is_null() {
            GdipDrawLineI(
                self.g,
                pen,
                x1.round() as i32,
                y1.round() as i32,
                x2.round() as i32,
                y2.round() as i32,
            );
            GdipDeletePen(pen);
        }
    }

    unsafe fn draw_image(&self, image: &GdiImage, dx: f32, dy: f32, dw: f32, dh: f32) {
        GdipDrawImageRectRectI(
            self.g,
            image.image(),
            dx.round() as i32,
            dy.round() as i32,
            dw.round() as i32,
            dh.round() as i32,
            0,
            0,
            image.w,
            image.h,
            UnitPixel,
            null(),
            0,
            null_mut(),
        );
    }

    unsafe fn draw_image_src(
        &self,
        image: &GdiImage,
        dx: f32,
        dy: f32,
        dw: f32,
        dh: f32,
        sx: f32,
        sy: f32,
        sw: f32,
        sh: f32,
    ) {
        GdipDrawImageRectRectI(
            self.g,
            image.image(),
            dx.round() as i32,
            dy.round() as i32,
            dw.round() as i32,
            dh.round() as i32,
            sx.round() as i32,
            sy.round() as i32,
            sw.round() as i32,
            sh.round() as i32,
            UnitPixel,
            null(),
            0,
            null_mut(),
        );
    }

    /// Draws `image` into an arbitrary parallelogram (rotation / mirroring).
    unsafe fn draw_image_points(
        &self,
        image: &GdiImage,
        p0: (f32, f32),
        p1: (f32, f32),
        p2: (f32, f32),
        sx: f32,
        sy: f32,
        sw: f32,
        sh: f32,
    ) {
        let points = [
            PointF { X: p0.0, Y: p0.1 },
            PointF { X: p1.0, Y: p1.1 },
            PointF { X: p2.0, Y: p2.1 },
        ];
        GdipDrawImagePointsRect(
            self.g,
            image.image(),
            points.as_ptr(),
            3,
            sx,
            sy,
            sw,
            sh,
            UnitPixel,
            null(),
            0,
            null_mut(),
        );
    }
}

unsafe fn brush_solid(color: u32) -> *mut GpSolidFill {
    let mut brush: *mut GpSolidFill = null_mut();
    if GdipCreateSolidFill(color, &mut brush) == Ok && !brush.is_null() {
        brush
    } else {
        null_mut()
    }
}

// ---------------------------------------------------------------------------
// Pet thread
// ---------------------------------------------------------------------------

unsafe extern "system" fn wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

fn compute_strip(folder_x: f32, current_y: f32, screen_w: i32, screen_h: i32) -> (i32, i32, i32, i32) {
    let mut strip_x = (folder_x - 400.0).max(0.0);
    let mut strip_w = ((folder_x + 120.0) - strip_x).max(440.0);
    let mut strip_y = current_y - 85.0;
    let strip_h = 145.0_f32;

    if strip_w > screen_w as f32 {
        strip_w = screen_w as f32;
        strip_x = 0.0;
    } else if strip_x + strip_w > screen_w as f32 {
        strip_x = (screen_w as f32 - strip_w).max(0.0);
    }
    let max_y = (screen_h as f32 - strip_h).max(0.0);
    strip_y = strip_y.clamp(0.0, max_y);

    (
        strip_x.round() as i32,
        strip_y.round() as i32,
        strip_w.round() as i32,
        strip_h.round() as i32,
    )
}

#[allow(clippy::too_many_arguments)]
fn run(
    shared: Arc<SharedPet>,
    cmd_rx: Receiver<PetCommand>,
    stop: Arc<AtomicBool>,
    enabled: bool,
    speed: f32,
    position_offset: f32,
    rx: Receiver<PetEvent>,
) {
    unsafe {
        let hinstance = GetModuleHandleW(null());
        let class_name: Vec<u16> = "DirWatcherPetWnd\0".encode_utf16().collect();
        let title: Vec<u16> = "DirWatcherPet\0".encode_utf16().collect();

        let wc = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            style: 0,
            lpfnWndProc: Some(wndproc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinstance,
            hIcon: null_mut(),
            hCursor: null_mut(),
            hbrBackground: null_mut(),
            lpszMenuName: null(),
            lpszClassName: class_name.as_ptr(),
            hIconSm: null_mut(),
        };
        RegisterClassExW(&wc);

        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
            class_name.as_ptr(),
            title.as_ptr(),
            WS_POPUP,
            0,
            0,
            1,
            1,
            null_mut(),
            null_mut(),
            hinstance,
            null(),
        );
        if hwnd.is_null() {
            log::error!("native pet: CreateWindowExW failed");
            return;
        }

        let mut controller = PetController::new(Some(rx));
        controller.set_character(CharacterKind::Slime);
        controller.enabled = enabled;
        controller.speed = speed;
        controller.position_offset = position_offset;
        controller.ppp = 1.0;
        controller.refresh_taskbar_coords();

        let Some(mut renderer) = Renderer::new(CharacterSpec::for_kind(CharacterKind::Slime)) else {
            log::error!("native pet: GDI+ init failed");
            DestroyWindow(hwnd);
            return;
        };
        renderer.ensure_fonts();

        let screen_dc = GetDC(null_mut());
        let screen_w = GetSystemMetrics(SM_CXSCREEN);
        let screen_h = GetSystemMetrics(SM_CYSCREEN);

        let mut canvas: Option<Canvas> = None;
        let mut shown = false;
        let mut running = true;

        while running && !stop.load(Ordering::SeqCst) {
            // Pump any pending window messages.
            let mut msg: MSG = std::mem::zeroed();
            while PeekMessageW(&mut msg, null_mut(), 0, 0, PM_REMOVE) != 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }

            // Commands from the settings UI.
            while let std::result::Result::Ok(cmd) = cmd_rx.try_recv() {
                match cmd {
                    PetCommand::HelpClean => controller.help_clean(),
                    PetCommand::SayHi(text) => controller.say_funny(&text, 3.0),
                    PetCommand::Realign => controller.refresh_taskbar_coords(),
                    PetCommand::Shutdown => running = false,
                }
            }

            // Sync simple values from shared state.
            let shared_enabled = shared.enabled();
            if shared_enabled != controller.enabled {
                controller.enabled = shared_enabled;
            }
            let shared_speed = shared.speed();
            if (shared_speed - controller.speed).abs() > 0.01 {
                controller.speed = shared_speed;
            }
            let shared_offset = shared.position_offset();
            if (shared_offset - controller.position_offset).abs() > 0.01 {
                controller.position_offset = shared_offset;
                controller.refresh_taskbar_coords();
            }

            if !controller.enabled {
                if shown {
                    ShowWindow(hwnd, SW_HIDE);
                    shown = false;
                }
                // Drain pending events so the queue does not grow while hidden.
                let _ = controller.update();
                thread::sleep(Duration::from_millis(200));
                continue;
            }

            let delay = controller.update();

            shared.state.store(state_to_u8(controller.state), Ordering::Relaxed);
            shared.frame.store(controller.player.current_frame(), Ordering::Relaxed);
            shared
                .facing_left
                .store(controller.facing_left, Ordering::Relaxed);
            shared
                .dragging
                .store(controller.is_dragging_desk, Ordering::Relaxed);
            shared
                .papers
                .store(controller.ground_papers.len(), Ordering::Relaxed);
            shared
                .carried
                .store(controller.carried_stack.len(), Ordering::Relaxed);
            shared
                .position_offset
                .store(controller.position_offset.to_bits(), Ordering::Relaxed);

            let (sx, sy, sw, sh) = compute_strip(
                controller.folder_x,
                controller.current_y,
                screen_w,
                screen_h,
            );
            let sw = sw.max(1);
            let sh = sh.max(1);

            let recreate = match &canvas {
                Some(c) => c.w != sw || c.h != sh,
                None => true,
            };
            if recreate {
                canvas = Some(Canvas::new(screen_dc, sw, sh));
            }
            let canvas = canvas.as_ref().unwrap();
            canvas.clear();

            let mut gp_bitmap: *mut GpBitmap = null_mut();
            let status = GdipCreateBitmapFromScan0(
                sw,
                sh,
                sw * 4,
                PIXEL_FORMAT_32BPP_PARGB,
                Some(canvas.bits),
                &mut gp_bitmap,
            );
            if status == Ok && !gp_bitmap.is_null() {
                let mut g: *mut GpGraphics = null_mut();
                if GdipGetImageGraphicsContext(gp_bitmap as *mut GpImage, &mut g) == Ok
                    && !g.is_null()
                {
                    GdipSetCompositingMode(g, CompositingModeSourceOver);
                    GdipSetCompositingQuality(g, CompositingQualityHighSpeed);
                    GdipSetInterpolationMode(g, InterpolationModeNearestNeighbor);
                    GdipSetPixelOffsetMode(g, PixelOffsetModeHalf);
                    GdipSetSmoothingMode(g, SmoothingModeAntiAlias);
                    GdipSetTextRenderingHint(g, TextRenderingHintAntiAlias);

                    let gfx = Gfx { g };
                    draw_pet(&gfx, &renderer, &controller, sx, sy, sw as f32, sh as f32);

                    GdipDeleteGraphics(g);
                }
                GdipDisposeImage(gp_bitmap as *mut GpImage);
            }

            canvas.present(screen_dc, hwnd, sx, sy);
            if !shown {
                ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                shown = true;
            }

            let sleep = delay
                .max(Duration::from_millis(4))
                .min(Duration::from_millis(250));
            thread::sleep(sleep);
        }

        if shown {
            ShowWindow(hwnd, SW_HIDE);
        }
        drop(canvas);
        ReleaseDC(null_mut(), screen_dc);
        drop(renderer);
        DestroyWindow(hwnd);
        log::info!("native pet thread exited");
    }
}

// ---------------------------------------------------------------------------
// Frame painting (port of PetController::render)
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
unsafe fn draw_pet(
    gfx: &Gfx,
    renderer: &Renderer,
    pet: &PetController,
    strip_x: i32,
    strip_y: i32,
    strip_w: f32,
    _strip_h: f32,
) {
    let spec = &renderer.spec;
    let strip_x = strip_x as f32;
    let strip_y = strip_y as f32;

    let anim_def = match pet.state {
        PetState::Sleeping => &spec.anim_sleep,
        PetState::Alert => &spec.anim_alert,
        PetState::Collecting => &spec.anim_walk,
        PetState::WalkingToTray => &spec.anim_walk,
        PetState::Arranging => &spec.anim_drop,
        PetState::WalkingHome => &spec.anim_walk,
        PetState::Stuck => &spec.anim_alert,
    };

    let frame = pet.player.current_frame();
    let char_w = spec.frame_width as f32 * pet.scale;
    let char_h = spec.frame_height as f32 * pet.scale;

    let folder_local_x = pet.folder_x - strip_x;
    let folder_local_y = (pet.current_y + 14.0) - strip_y;

    // 1. Destination folder slot.
    if pet.folder_alpha > 0.01 {
        gfx.draw_image_src(
            &renderer.folder,
            folder_local_x,
            folder_local_y,
            16.0,
            16.0,
            0.0,
            0.0,
            16.0,
            16.0,
        );
    }

    // 2. Falling / landed papers.
    for paper in &pet.ground_papers {
        let px = paper.x - strip_x;
        let py = paper.current_y - strip_y;
        if !paper.landed {
            let tilt = paper.flutter_t.cos() * 0.35;
            let (sin_r, cos_r) = (tilt.sin(), tilt.cos());
            let rot = |ox: f32, oy: f32| -> (f32, f32) {
                (px + ox * cos_r - oy * sin_r, py + ox * sin_r + oy * cos_r)
            };
            let p0 = rot(-8.0, -8.0);
            let p1 = rot(8.0, -8.0);
            let p2 = rot(-8.0, 8.0);
            gfx.draw_image_points(&renderer.file_icon, p0, p1, p2, 0.0, 0.0, 16.0, 16.0);
        } else {
            gfx.draw_image_src(
                &renderer.file_icon,
                px - 8.0,
                py,
                16.0,
                16.0,
                0.0,
                0.0,
                16.0,
                16.0,
            );
        }
    }

    // 3. Sparkle bursts.
    for (sx, sy, timer) in &pet.sparkle_bursts {
        let local_sx = sx - strip_x;
        let local_sy = sy - strip_y;
        let progress = 1.0 - (*timer / 0.7).clamp(0.0, 1.0);
        let sp_y = local_sy - progress * 22.0;
        let sp_alpha = ((1.0 - progress) * 255.0) as u8;
        let size = (1.0 - (progress - 0.5).abs() * 2.0).max(0.4) * 6.5;
        draw_star(gfx, local_sx, sp_y, size, sp_alpha);
        draw_star(
            gfx,
            local_sx - 9.0 - progress * 4.0,
            sp_y + 3.0 - progress * 6.0,
            size * 0.65,
            sp_alpha,
        );
        draw_star(
            gfx,
            local_sx + 9.0 + progress * 4.0,
            sp_y + 2.0 - progress * 8.0,
            size * 0.75,
            sp_alpha,
        );
    }

    // 4. Pet sprite.
    let pet_local_x = pet.current_x - strip_x;
    let pet_local_y = pet.current_y - strip_y;
    let pet_rect = (
        pet_local_x,
        pet_local_y,
        char_w,
        char_h,
        pet_local_x + char_w / 2.0,
        pet_local_y + char_h / 2.0,
    );

    let col = anim_def.start_col + (frame % anim_def.frame_count);
    let src_x = (col * spec.frame_width as usize) as f32;
    let src_y = (anim_def.row * spec.frame_height as usize) as f32;
    let src_w = spec.frame_width as f32;
    let src_h = spec.frame_height as f32;
    if pet.facing_left {
        gfx.draw_image_points(
            &renderer.character,
            (pet_local_x + char_w, pet_local_y),
            (pet_local_x, pet_local_y),
            (pet_local_x + char_w, pet_local_y + char_h),
            src_x,
            src_y,
            src_w,
            src_h,
        );
    } else {
        gfx.draw_image_points(
            &renderer.character,
            (pet_local_x, pet_local_y),
            (pet_local_x + char_w, pet_local_y),
            (pet_local_x, pet_local_y + char_h),
            src_x,
            src_y,
            src_w,
            src_h,
        );
    }

    // 5. Sweat drop.
    if pet.state == PetState::Stuck || (pet.is_overwhelmed && pet.state != PetState::Sleeping) {
        let sweat_bob = (pet.sweat_timer * 7.0).sin() * 2.0;
        let drop_x = pet_rect.0 - 4.0;
        let drop_y = pet_rect.1 + 8.0 + sweat_bob;
        let drop_color = argb(255, 110, 195, 255);
        gfx.fill_ellipse(drop_color, drop_x, drop_y, 2.5, 2.5);
        gfx.fill_poly(
            drop_color,
            &[
                Point {
                    X: drop_x.round() as i32,
                    Y: (drop_y - 4.5).round() as i32,
                },
                Point {
                    X: (drop_x - 2.2).round() as i32,
                    Y: drop_y.round() as i32,
                },
                Point {
                    X: (drop_x + 2.2).round() as i32,
                    Y: drop_y.round() as i32,
                },
            ],
        );
        gfx.fill_ellipse(argb(255, 255, 255, 255), drop_x - 0.7, drop_y - 0.7, 0.8, 0.8);
    }

    // 6. Speech bubble.
    if pet.speech_timer > 0.0 {
        if let Some(ref msg) = pet.speech_text {
            draw_speech_bubble(gfx, renderer, pet, msg, pet_rect, strip_w);
        }
    }

    // 7. Sleeping Zzz particles.
    if pet.state == PetState::Sleeping {
        let z_frame = (frame / 2) % 4;
        let zzz_x = pet_rect.4 + 2.0;
        let zzz_y = pet_rect.1 - 2.0;
        gfx.draw_image_src(
            &renderer.zzz,
            zzz_x,
            zzz_y,
            16.0,
            16.0,
            (z_frame * 32) as f32,
            0.0,
            32.0,
            32.0,
        );
    }

    // 8. Carried stack on the pet's head.
    let stack_count = pet.carried_stack.len();
    if stack_count > 0 && pet.state != PetState::Arranging {
        let bounce_oy = match frame % 10 {
            1 | 5 => -1.0,
            2 | 4 => -3.0,
            3 => -4.0,
            _ => 0.0,
        };
        let head_surface_y = pet_rect.1 + 16.0 + bounce_oy;
        for i in 0..stack_count.min(3) {
            let layer = i as f32;
            let tilt_x = if i == 1 {
                1.5
            } else if i == 2 {
                -1.5
            } else {
                0.0
            };
            let stack_y = head_surface_y - 12.0 - layer * 3.0;
            gfx.draw_image_src(
                &renderer.file_icon,
                pet_rect.4 - 8.0 + tilt_x,
                stack_y,
                16.0,
                16.0,
                0.0,
                0.0,
                16.0,
                16.0,
            );
        }
    }

    // 9. Arranging animation.
    if pet.state == PetState::Arranging && pet.arrange_timer > 0.0 {
        let progress = 1.0 - (pet.arrange_timer / 1.1).clamp(0.0, 1.0);
        let start_slot_x = pet_rect.4 - 8.0;
        let start_slot_y = pet_rect.1 + 4.0;
        let end_slot_x = folder_local_x;
        let end_slot_y = folder_local_y - 2.0;
        let slide = (progress * 1.6).min(1.0);
        let cur_x = start_slot_x + (end_slot_x - start_slot_x) * slide;
        let cur_y = start_slot_y + (end_slot_y - start_slot_y) * slide;
        gfx.draw_image_src(
            &renderer.file_icon,
            cur_x,
            cur_y,
            16.0,
            16.0,
            0.0,
            0.0,
            16.0,
            16.0,
        );

        if progress > 0.55 {
            let sparkle_alpha = ((1.0 - (progress - 0.55) / 0.45) * 255.0) as u8;
            draw_star(
                gfx,
                folder_local_x + 8.0,
                folder_local_y - 10.0,
                6.5,
                sparkle_alpha,
            );

            let check = argb(sparkle_alpha, 80, 225, 120);
            let ck_cx = folder_local_x + 8.0;
            let ck_cy = folder_local_y - 20.0;
            gfx.draw_line(check, ck_cx - 20.0, ck_cy, ck_cx - 17.0, ck_cy + 3.0, 1.8);
            gfx.draw_line(
                check,
                ck_cx - 17.0,
                ck_cy + 3.0,
                ck_cx - 11.5,
                ck_cy - 4.5,
                1.8,
            );
            draw_text(
                gfx,
                renderer,
                "Sorted!",
                ck_cx + 4.0,
                ck_cy,
                check,
                TextAnchor::Center,
            );
        }
    }

    // 10. Alert exclamation.
    if pet.state == PetState::Alert {
        draw_text(
            gfx,
            renderer,
            "!",
            pet_rect.4,
            pet_rect.1 + 6.0,
            argb(255, 255, 205, 50),
            TextAnchor::Center,
        );
    }
}

unsafe fn draw_star(gfx: &Gfx, cx: f32, cy: f32, r: f32, alpha: u8) {
    if r <= 0.5 || alpha == 0 {
        return;
    }
    let color = argb(alpha, 255, 215, 0);
    let inner = r * 0.28;
    gfx.fill_poly(
        color,
        &[
            Point { X: cx.round() as i32, Y: (cy - r).round() as i32 },
            Point { X: (cx + inner).round() as i32, Y: cy.round() as i32 },
            Point { X: cx.round() as i32, Y: (cy + r).round() as i32 },
            Point { X: (cx - inner).round() as i32, Y: cy.round() as i32 },
        ],
    );
    gfx.fill_poly(
        color,
        &[
            Point { X: (cx - r).round() as i32, Y: cy.round() as i32 },
            Point { X: cx.round() as i32, Y: (cy - inner).round() as i32 },
            Point { X: (cx + r).round() as i32, Y: cy.round() as i32 },
            Point { X: cx.round() as i32, Y: (cy + inner).round() as i32 },
        ],
    );
    gfx.fill_ellipse(
        argb(alpha, 255, 255, 255),
        cx,
        cy,
        (r * 0.25).max(1.0),
        (r * 0.25).max(1.0),
    );
}

#[derive(Clone, Copy)]
enum TextAnchor {
    Center,
    Left,
}

#[allow(clippy::too_many_arguments)]
unsafe fn draw_text(
    gfx: &Gfx,
    renderer: &Renderer,
    text: &str,
    x: f32,
    y: f32,
    color: u32,
    anchor: TextAnchor,
) {
    let Some(fonts) = renderer.fonts.as_ref() else {
        return;
    };
    let font = if text == "!" { fonts.large } else { fonts.small };
    if font.is_null() {
        return;
    }
    let mut wide: Vec<u16> = text.encode_utf16().collect();
    wide.push(0);
    let format = match anchor {
        TextAnchor::Center => fonts.center,
        TextAnchor::Left => fonts.left,
    };
    let rect = RectF {
        X: x,
        Y: y,
        Width: 400.0,
        Height: 40.0,
    };
    let brush = brush_solid(color);
    if brush.is_null() {
        return;
    }
    GdipDrawString(
        gfx.g,
        PCWSTR(wide.as_ptr()),
        (wide.len() - 1) as i32,
        font,
        &rect,
        format,
        brush as *mut GpBrush,
    );
    GdipDeleteBrush(brush as *mut GpBrush);
}

unsafe fn draw_speech_bubble(
    gfx: &Gfx,
    renderer: &Renderer,
    pet: &PetController,
    msg: &str,
    pet_rect: (f32, f32, f32, f32, f32, f32),
    strip_w: f32,
) {
    let Some(fonts) = renderer.fonts.as_ref() else {
        return;
    };
    if fonts.small.is_null() {
        return;
    }

    let mut wide: Vec<u16> = msg.encode_utf16().collect();
    wide.push(0);

    let mut bbox = RectF {
        X: 0.0,
        Y: 0.0,
        Width: 0.0,
        Height: 0.0,
    };
    let layout = RectF {
        X: 0.0,
        Y: 0.0,
        Width: 4000.0,
        Height: 100.0,
    };
    let mut fitted = 0i32;
    let mut lines = 0i32;
    GdipMeasureString(
        gfx.g,
        PCWSTR(wide.as_ptr()),
        (wide.len() - 1) as i32,
        fonts.small,
        &layout,
        fonts.left,
        &mut bbox,
        &mut fitted,
        &mut lines,
    );

    let pad_x = 10.0_f32;
    let pad_y = 5.0_f32;
    let bubble_w = bbox.Width + pad_x * 2.0;
    let bubble_h = bbox.Height + pad_y * 2.0;

    let bubble_center_x = pet_rect.4.clamp(
        bubble_w / 2.0 + 4.0,
        (strip_w - bubble_w / 2.0 - 4.0).max(bubble_w / 2.0 + 4.0),
    );
    let stack_offset = if !pet.carried_stack.is_empty() {
        8.0 + pet.carried_stack.len() as f32 * 3.0
    } else {
        0.0
    };
    let bubble_bottom_y = pet_rect.1 - 6.0 - stack_offset;
    let bubble_left = bubble_center_x - bubble_w / 2.0;
    let bubble_top = bubble_bottom_y - bubble_h;

    let alpha_factor = if pet.speech_timer < 0.4 {
        pet.speech_timer / 0.4
    } else {
        1.0
    }
    .clamp(0.0, 1.0);

    let bg_alpha = (alpha_factor * 235.0) as u8;
    let border_alpha = (alpha_factor * 255.0) as u8;
    let text_alpha = (alpha_factor * 255.0) as u8;

    // Border then background (1px inset), a cheap way to get a crisp outline.
    let border_color = if pet.state == PetState::Stuck {
        argb(border_alpha, 255, 110, 100)
    } else {
        argb(border_alpha, 120, 180, 255)
    };
    gfx.fill_round_rect(border_color, bubble_left, bubble_top, bubble_w, bubble_h, 6.0);
    gfx.fill_round_rect(
        argb(bg_alpha, 22, 26, 36),
        bubble_left + 1.0,
        bubble_top + 1.0,
        bubble_w - 2.0,
        bubble_h - 2.0,
        5.0,
    );

    // Tail.
    let tail_x = pet_rect.4.clamp(bubble_left + 8.0, bubble_left + bubble_w - 8.0);
    gfx.fill_poly(
        border_color,
        &[
            Point { X: (tail_x - 4.0).round() as i32, Y: bubble_bottom_y.round() as i32 },
            Point { X: (tail_x + 4.0).round() as i32, Y: bubble_bottom_y.round() as i32 },
            Point { X: tail_x.round() as i32, Y: (bubble_bottom_y + 4.0).round() as i32 },
        ],
    );

    // Text.
    let text_color = argb(text_alpha, 255, 255, 255);
    let brush = brush_solid(text_color);
    if !brush.is_null() {
        let rect = RectF {
            X: bubble_left + pad_x,
            Y: bubble_top + pad_y,
            Width: bubble_w,
            Height: bubble_h,
        };
        GdipDrawString(
            gfx.g,
            PCWSTR(wide.as_ptr()),
            (wide.len() - 1) as i32,
            fonts.small,
            &rect,
            fonts.left,
            brush as *mut GpBrush,
        );
        GdipDeleteBrush(brush as *mut GpBrush);
    }
}

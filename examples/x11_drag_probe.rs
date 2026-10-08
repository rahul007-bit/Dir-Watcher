//! Diagnostics for the desktop-pet drag path on X11/XWayland.
//!
//! The pet overlay is click-through and never receives pointer events, so the
//! app instead polls the global pointer (`QueryPointer`) and reads the left
//! button state from XInput2 *raw* events. This probe exercises exactly those
//! code paths, injecting synthetic pointer motion/clicks with XTEST so it can
//! run unattended:
//!
//! 1. `QueryPointer` must track the pointer everywhere (including over native
//!    Wayland surfaces, where XWayland is known to report stale coordinates).
//! 2. XInput2 raw motion events must fire and carry usable coordinates.
//! 3. XInput2 raw button press/release must fire even when the click lands on
//!    a Wayland surface.
//!
//! Run with: `cargo run --example x11_drag_probe`
#![allow(dead_code)]

use std::time::{Duration, Instant};

use x11rb::connection::Connection as _;
use x11rb::protocol::xinput::{self, ConnectionExt as _};
use x11rb::protocol::xproto::ConnectionExt as _;
use x11rb::protocol::xtest::ConnectionExt as _;
use x11rb::protocol::Event;
use x11rb::CURRENT_TIME;

use x11rb::rust_connection::RustConnection;

// XTEST fake_input event types.
const XI_MOTION: u8 = 2;
const XI_BUTTON_PRESS: u8 = 4;
const XI_BUTTON_RELEASE: u8 = 5;

fn main() {
    let Ok((conn, screen)) = x11rb::connect(None) else {
        eprintln!("FAIL: could not connect to the X server (is XWayland running? DISPLAY set?)");
        return;
    };
    let root = conn.setup().roots.get(screen).unwrap().root;
    let geometry = conn
        .get_geometry(root)
        .ok()
        .and_then(|c| c.reply().ok())
        .map(|g| (g.width as i32, g.height as i32))
        .unwrap_or((0, 0));
    println!("connected to X server; root screen = {geometry:?}");

    // --- Raw-event listener setup (same as the app's x11_raw module) ---
    let mask = xinput::XIEventMask::RAW_BUTTON_PRESS
        | xinput::XIEventMask::RAW_BUTTON_RELEASE
        | xinput::XIEventMask::RAW_MOTION;
    let events = xinput::EventMask {
        deviceid: xinput::DeviceId::from(1u16), // XIAllMasterDevices
        mask: vec![mask],
    };
    if let Err(err) = conn.xinput_xi_select_events(root, &[events]) {
        eprintln!("FAIL: xinput_xi_select_events: {err}");
        return;
    }
    conn.flush().expect("flush");

    // --- 1. QueryPointer accuracy while sweeping the pointer ---
    println!("\n--- test 1: QueryPointer tracks an injected pointer sweep ---");
    let w = geometry.0;
    let h = geometry.1;
    let mut stale = 0;
    let mut total = 0;
    for i in 0..=20 {
        let x = (w * i / 20).min(w - 1) as i16;
        let y = (h * i / 40 + h / 4).min(h - 1) as i16;
        fake_motion(&conn, root, x, y);
        std::thread::sleep(Duration::from_millis(30));
        total += 1;
        match query_pointer(&conn, root) {
            Some((qx, qy)) => {
                let ok = (qx - x as f64).abs() < 2.0 && (qy - y as f64).abs() < 2.0;
                if !ok {
                    stale += 1;
                }
                println!(
                    "  injected=({x:>5},{y:>5})  query=({qx:>5.0},{qy:>5.0})  {}",
                    if ok { "ok" } else { "STALE/MISMATCH" }
                );
            }
            None => {
                stale += 1;
                println!("  injected=({x},{y})  query=None");
            }
        }
    }
    println!(
        "test 1: {}/{} queries accurate ({})",
        total - stale,
        total,
        if stale == 0 { "PASS" } else { "FAIL" }
    );

    // --- 2. Raw motion events during a sweep over the desktop ---
    println!("\n--- test 2: XInput2 raw motion events ---");
    let (tx, rx) = std::sync::mpsc::channel();
    let listener_conn = x11rb::connect(None).unwrap().0;
    std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(8);
        while Instant::now() < deadline {
            match listener_conn.poll_for_event() {
                Ok(Some(event)) => {
                    let _ = tx.send(describe_raw(&event));
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(10)),
                Err(_) => break,
            }
        }
    });
    // Give the listener thread a moment to subscribe on its own connection.
    std::thread::sleep(Duration::from_millis(200));

    for i in 0..=30 {
        let x = (w * i / 30).min(w - 1) as i16;
        let y = (h * 3 / 4) as i16; // sweep along the bottom quarter (near the pet desk)
        fake_motion(&conn, root, x, y);
        std::thread::sleep(Duration::from_millis(40));
    }
    std::thread::sleep(Duration::from_millis(300));
    let mut motion_events = 0;
    let mut with_coords = 0;
    while let Ok(desc) = rx.try_recv() {
        println!("  raw: {desc}");
        if desc.starts_with("RawMotion") {
            motion_events += 1;
            if desc.contains("at(") {
                with_coords += 1;
            }
        }
    }
    println!(
        "test 2: {motion_events} raw motion events, {with_coords} with coordinates ({})",
        if motion_events > 0 && with_coords > 0 {
            "PASS"
        } else {
            "FAIL"
        }
    );

    // --- 3. Raw button press/release over the desktop ---
    println!("\n--- test 3: XInput2 raw button events (click on desktop) ---");
    let bx = (w - w / 5) as i16; // bottom-right desktop area (near the pet desk)
    let by = (h - h / 8) as i16;
    fake_motion(&conn, root, bx, by);
    std::thread::sleep(Duration::from_millis(200));

    // Drain any pending motion events before clicking.
    while rx.try_recv().is_ok() {}

    conn.xtest_fake_input(XI_BUTTON_PRESS, 1, CURRENT_TIME, root, bx, by, 0)
        .expect("button press");
    conn.flush().expect("flush");
    std::thread::sleep(Duration::from_millis(150));

    // Small drag while the button is held (like dragging the pet desk).
    for i in 1..=5 {
        fake_motion(&conn, root, bx - 10 * i as i16, by - 3 * i as i16);
        std::thread::sleep(Duration::from_millis(60));
    }

    conn.xtest_fake_input(XI_BUTTON_RELEASE, 1, CURRENT_TIME, root, bx - 50, by - 15, 0)
        .expect("button release");
    conn.flush().expect("flush");
    std::thread::sleep(Duration::from_millis(400));

    let mut pressed = false;
    let mut released = false;
    while let Ok(desc) = rx.try_recv() {
        println!("  raw: {desc}");
        if desc.starts_with("RawButtonPress") {
            pressed = true;
        }
        if desc.starts_with("RawButtonRelease") {
            released = true;
        }
    }
    println!(
        "test 3: press={} release={} ({})",
        pressed,
        released,
        if pressed && released { "PASS" } else { "FAIL" }
    );

    // Move the pointer somewhere neutral so we don't leave it over a corner.
    fake_motion(&conn, root, (w / 2) as i16, (h / 2) as i16);

    println!("\ndone.");
}

fn fake_motion(conn: &RustConnection, root: u32, x: i16, y: i16) {
    // XTEST FakeInput: detail 0 = absolute, 1 = relative.
    conn.xtest_fake_input(XI_MOTION, 0, CURRENT_TIME, root, x, y, 0)
        .expect("fake_input motion");
    conn.flush().expect("flush");
}

fn query_pointer(conn: &impl x11rb::protocol::xproto::ConnectionExt, root: u32) -> Option<(f64, f64)> {
    let reply = conn.query_pointer(root).ok()?.reply().ok()?;
    Some((reply.root_x as f64, reply.root_y as f64))
}

fn describe_raw(event: &Event) -> String {
    match event {
        Event::XinputRawMotion(ev) => {
            let vals = &ev.axisvalues_raw;
            let (x, y) = match vals.len() {
                0 => (f64::NAN, f64::NAN),
                1 => (fp(&vals[0]), f64::NAN),
                _ => (fp(&vals[0]), fp(&vals[1])),
            };
            format!(
                "RawMotion src={} axes={} at({x:.0},{y:.0})",
                ev.sourceid,
                vals.len()
            )
        }
        Event::XinputRawButtonPress(ev) => {
            format!("RawButtonPress src={} detail={}", ev.sourceid, ev.detail)
        }
        Event::XinputRawButtonRelease(ev) => {
            format!("RawButtonRelease src={} detail={}", ev.sourceid, ev.detail)
        }
        other => format!("Other({:?})", std::mem::discriminant(other)),
    }
}

fn fp(v: &x11rb::protocol::xinput::Fp3232) -> f64 {
    v.integral as f64 + v.frac as f64 / (1u64 << 32) as f64
}

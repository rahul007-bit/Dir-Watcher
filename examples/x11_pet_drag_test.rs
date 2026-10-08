//! End-to-end test of pet dragging on X11/XWayland, without human input.
//!
//! 1. Locates the `DirWatcherPet` X11 window.
//! 2. Reads its shaped input region (the rects the app installs for the
//!    sprite/desk/papers) and clicks the center of the pet-sprite rect.
//! 3. Drags left with XTEST injection, releases.
//! 4. Verifies the pet window actually moved.
//!
//! Run with: `cargo run --example x11_pet_drag_test`
#![allow(dead_code)]

use std::time::{Duration, Instant};

use x11rb::connection::Connection as _;
use x11rb::protocol::shape::{ConnectionExt as _, SK};
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _, Rectangle};
use x11rb::protocol::xtest::ConnectionExt as _;
use x11rb::rust_connection::RustConnection;
use x11rb::CURRENT_TIME;

const XI_MOTION: u8 = 2;
const XI_BUTTON_PRESS: u8 = 4;
const XI_BUTTON_RELEASE: u8 = 5;

const PET_TITLE: &str = "DirWatcherPet";

fn main() {
    let Ok((conn, screen)) = x11rb::connect(None) else {
        eprintln!("FAIL: no X connection");
        return;
    };
    let root = conn.setup().roots.get(screen).unwrap().root;

    let Some(pet) = find_window(&conn, PET_TITLE) else {
        eprintln!("FAIL: no window titled '{PET_TITLE}' (is the app running on the X11 backend?)");
        return;
    };
    let pos = window_pos(&conn, root, pet);
    println!("pet window id={pet} at {pos:?}");

    // Read the app's shaped input region and pick the sprite rect (largest).
    let rects = match conn
        .shape_get_rectangles(pet, SK::INPUT)
        .ok()
        .and_then(|c| c.reply().ok())
    {
        Some(reply) => reply.rectangles.clone(),
        None => {
            eprintln!("FAIL: cannot query XShape input region");
            return;
        }
    };
    if rects.is_empty() {
        eprintln!("FAIL: pet window input region is empty (pet disabled, or app not on X11 backend)");
        return;
    }
    println!("input rects: {rects:?}");
    let Some(sprite) = rects
        .iter()
        .max_by_key(|r| r.width as u32 * r.height as u32)
        .copied()
    else {
        unreachable!()
    };
    let (cx, cy) = (
        pos.0 + sprite.x as i32 + sprite.width as i32 / 2,
        pos.1 + sprite.y as i32 + sprite.height as i32 / 2,
    );
    println!("clicking sprite center at ({cx},{cy}) and dragging left 150px");

    // Move there, press, drag left in small steps, release.
    fake_motion(&conn, root, cx as i16, cy as i16);
    std::thread::sleep(Duration::from_millis(120));
    conn.xtest_fake_input(XI_BUTTON_PRESS, 1, CURRENT_TIME, root, cx as i16, cy as i16, 0)
        .expect("press");
    conn.flush().expect("flush");
    std::thread::sleep(Duration::from_millis(100));

    let total = 150;
    let steps = 15;
    for i in 1..=steps {
        let x = cx - total * i / steps;
        let y = cy;
        fake_motion(&conn, root, x as i16, y as i16);
        std::thread::sleep(Duration::from_millis(50));
    }
    std::thread::sleep(Duration::from_millis(100));

    conn.xtest_fake_input(
        XI_BUTTON_RELEASE,
        1,
        CURRENT_TIME,
        root,
        (cx - total) as i16,
        cy as i16,
        0,
    )
    .expect("release");
    conn.flush().expect("flush");

    // Wait for the window to catch up (egui applies the new position on the
    // next frames).
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut after = pos;
    while Instant::now() < deadline {
        after = window_pos(&conn, root, pet);
        if (after.0 - pos.0).abs() > 40 {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    println!("before = {pos:?}  after = {after:?}");
    let dx = after.0 - pos.0;
    if dx < -40 {
        println!("PASS: pet window moved left by {dx}px (drag followed the pointer)");
    } else {
        println!("FAIL: pet window did not follow the drag (dx={dx})");
    }

    // Put the pointer somewhere neutral.
    fake_motion(&conn, root, 1024, 576);
}

fn fake_motion(conn: &RustConnection, root: u32, x: i16, y: i16) {
    // XTEST FakeInput: detail 0 = absolute root_x/root_y, 1 = relative.
    conn.xtest_fake_input(XI_MOTION, 0, CURRENT_TIME, root, x, y, 0)
        .expect("fake motion");
    conn.flush().expect("flush");
}

fn find_window(conn: &RustConnection, title: &str) -> Option<u32> {
    use x11rb::protocol::xproto::ConnectionExt as _;

    let root = conn.setup().roots.first()?.root;
    let list = conn
        .intern_atom(false, b"_NET_CLIENT_LIST")
        .ok()?
        .reply()
        .ok()?
        .atom;
    let windows: Vec<u32> = conn
        .get_property(false, root, list, AtomEnum::WINDOW, 0, u32::MAX)
        .ok()?
        .reply()
        .ok()?
        .value32()?
        .collect();
    for w in windows {
        if window_title(conn, w).as_deref() == Some(title) {
            return Some(w);
        }
    }
    None
}

fn window_title(conn: &RustConnection, window: u32) -> Option<String> {
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
    if let Some(v) = conn
        .get_property(false, window, net_wm_name, utf8, 0, 256)
        .ok()
        .and_then(|c| c.reply().ok())
        .filter(|r| !r.value.is_empty())
        .map(|r| String::from_utf8_lossy(&r.value).into_owned())
    {
        return Some(v);
    }
    conn.get_property(false, window, AtomEnum::WM_NAME, AtomEnum::STRING, 0, 256)
        .ok()
        .and_then(|c| c.reply().ok())
        .map(|r| String::from_utf8_lossy(&r.value).into_owned())
}

fn window_pos(conn: &RustConnection, root: u32, window: u32) -> (i32, i32) {
    let reply = conn
        .translate_coordinates(window, root, 0, 0)
        .expect("translate")
        .reply()
        .expect("reply");
    (reply.dst_x as i32, reply.dst_y as i32)
}

// Keep `Rectangle` import used even if rects printing changes.
#[allow(unused)]
fn _rect_debug(r: &Rectangle) -> String {
    format!("{}x{}@{},{}", r.width, r.height, r.x, r.y)
}

//! Focus probe: what X delivers when a click lands on the pet sprite.
//!
//! Listens on (a) a raw-event thread like the app's `x11_raw`, (b) core
//! Enter/Leave/Button events selected on the pet window itself, then injects
//! one click at the center of the pet's shaped sprite rect.
//!
//! Run with: `cargo run --example x11_click_probe`
#![allow(dead_code)]

use std::sync::mpsc::channel;
use std::time::{Duration, Instant};

use x11rb::connection::Connection as _;
use x11rb::protocol::shape::{ConnectionExt as _, SK};
use x11rb::protocol::xinput::{self, ConnectionExt as _};
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _, EventMask};
use x11rb::protocol::xtest::ConnectionExt as _;
use x11rb::CURRENT_TIME;

const PET_TITLE: &str = "DirWatcherPet";

fn main() {
    let Ok((conn, screen)) = x11rb::connect(None) else {
        eprintln!("FAIL: no X connection");
        return;
    };
    let root = conn.setup().roots.get(screen).unwrap().root;

    let Some(pet) = find_window(&conn, PET_TITLE) else {
        eprintln!("FAIL: no pet window");
        return;
    };
    let pos = window_pos(&conn, root, pet);
    println!("pet window {pet} at {pos:?}");

    // Sprite rect = largest input rect, plus the window position.
    let rects = conn
        .shape_get_rectangles(pet, SK::INPUT)
        .ok()
        .and_then(|c| c.reply().ok())
        .map(|r| r.rectangles.clone())
        .unwrap_or_default();
    if rects.is_empty() {
        eprintln!("FAIL: empty input region");
        return;
    }
    let sprite = *rects
        .iter()
        .max_by_key(|r| r.width as u32 * r.height as u32)
        .unwrap();
    let cx = pos.0 + sprite.x as i32 + sprite.width as i32 / 2;
    let cy = pos.1 + sprite.y as i32 + sprite.height as i32 / 2;
    println!("rects: {:?}", rects.len());
    let sprite_local = (sprite.x, sprite.y, sprite.width, sprite.height);
    println!("sprite rect (local): {sprite_local:?}  click at ({cx},{cy})");

    // Raw-event listener (mirrors the app's x11_raw thread setup).
    let (tx, rx) = channel();
    let raw_conn = x11rb::connect(None).unwrap().0;
    let raw_root = raw_conn.setup().roots.first().unwrap().root;
    let mask = xinput::XIEventMask::RAW_BUTTON_PRESS
        | xinput::XIEventMask::RAW_BUTTON_RELEASE
        | xinput::XIEventMask::RAW_MOTION;
    let events = xinput::EventMask {
        deviceid: xinput::DeviceId::from(1u16),
        mask: vec![mask],
    };
    if raw_conn.xinput_xi_select_events(raw_root, &[events]).is_err() {
        eprintln!("FAIL: xi_select_events");
        return;
    }
    raw_conn.flush().unwrap();
    std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(6);
        while Instant::now() < deadline {
            match raw_conn.poll_for_event() {
                Ok(Some(ev)) => {
                    let _ = tx.send(match ev {
                        x11rb::protocol::Event::XinputRawMotion(_) => "RawMotion".to_string(),
                        x11rb::protocol::Event::XinputRawButtonPress(_) => {
                            "RawButtonPress".to_string()
                        }
                        x11rb::protocol::Event::XinputRawButtonRelease(_) => {
                            "RawButtonRelease".to_string()
                        }
                        other => format!("Other({:?})", std::mem::discriminant(&other)),
                    });
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(10)),
                Err(_) => break,
            }
        }
    });
    std::thread::sleep(Duration::from_millis(200)); // let the listener subscribe

    // Select core events on the pet window from this connection.
    conn.change_window_attributes(
        pet,
        &x11rb::protocol::xproto::ChangeWindowAttributesAux::default().event_mask(
            EventMask::BUTTON_PRESS
                | EventMask::BUTTON_RELEASE
                | EventMask::ENTER_WINDOW
                | EventMask::LEAVE_WINDOW
                | EventMask::POINTER_MOTION,
        ),
    )
    .unwrap();
    conn.flush().unwrap();

    // Inject: move onto the sprite, press briefly, release.
    const XI_MOTION: u8 = 2;
    const XI_BUTTON_PRESS: u8 = 4;
    const XI_BUTTON_RELEASE: u8 = 5;
    fake_motion(&conn, root, cx as i16, cy as i16);
    std::thread::sleep(Duration::from_millis(120));
    conn.xtest_fake_input(XI_BUTTON_PRESS, 1, CURRENT_TIME, root, cx as i16, cy as i16, 0)
        .unwrap();
    conn.flush().unwrap();
    std::thread::sleep(Duration::from_millis(80));
    conn.xtest_fake_input(XI_BUTTON_RELEASE, 1, CURRENT_TIME, root, cx as i16, cy as i16, 0)
        .unwrap();
    conn.flush().unwrap();
    std::thread::sleep(Duration::from_millis(500));

    println!("--- core events on pet window ---");
    let deadline = Instant::now() + Duration::from_millis(300);
    while Instant::now() < deadline {
        match conn.poll_for_event() {
            Ok(Some(ev)) => {
                println!("core: {}", describe_core(&ev));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(e) => {
                println!("core poll err: {e}");
                break;
            }
        }
    }
    println!("--- raw events ---");
    while let Ok(d) = rx.try_recv() {
        println!("raw: {d}");
    }
    println!("done.");
}

use x11rb::rust_connection::RustConnection;

fn fake_motion(conn: &RustConnection, root: u32, x: i16, y: i16) {
    conn.xtest_fake_input(2, 0, CURRENT_TIME, root, x, y, 0) // 2 = motion, detail 0 = absolute
        .unwrap();
    conn.flush().unwrap();
}

fn describe_core(ev: &x11rb::protocol::Event) -> String {
    use x11rb::protocol::Event;
    match ev {
        Event::ButtonPress(e) => format!("ButtonPress on {} detail {}", e.event, e.detail),
        Event::ButtonRelease(e) => format!("ButtonRelease on {} detail {}", e.event, e.detail),
        Event::EnterNotify(e) => format!("EnterNotify on {}", e.event),
        Event::LeaveNotify(e) => format!("LeaveNotify on {}", e.event),
        Event::MotionNotify(_) => "MotionNotify".to_string(),
        other => format!("Other({:?})", std::mem::discriminant(other)),
    }
}

fn find_window(conn: &RustConnectionAlias, title: &str) -> Option<u32> {
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

type RustConnectionAlias = x11rb::rust_connection::RustConnection;

fn window_title(conn: &RustConnectionAlias, window: u32) -> Option<String> {
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

fn window_pos(conn: &RustConnectionAlias, root: u32, window: u32) -> (i32, i32) {
    let reply = conn
        .translate_coordinates(window, root, 0, 0)
        .expect("translate")
        .reply()
        .expect("reply");
    (reply.dst_x as i32, reply.dst_y as i32)
}

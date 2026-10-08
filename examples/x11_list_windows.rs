//! List all managed X11 windows with title + geometry.
//! Run: `cargo run --example x11_list_windows`
#![allow(dead_code)]

use x11rb::connection::Connection as _;
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _};
use x11rb::rust_connection::RustConnection;

fn main() {
    let Ok((conn, _screen)) = x11rb::connect(None) else {
        eprintln!("no X connection");
        return;
    };
    let root = conn.setup().roots.first().unwrap().root;
    let list = conn
        .intern_atom(false, b"_NET_CLIENT_LIST")
        .unwrap()
        .reply()
        .unwrap()
        .atom;
    let windows: Vec<u32> = conn
        .get_property(false, root, list, AtomEnum::WINDOW, 0, u32::MAX)
        .unwrap()
        .reply()
        .unwrap()
        .value32()
        .unwrap()
        .collect();

    for w in windows {
        let title = window_title(&conn, w).unwrap_or_default();
        let geo = conn.get_geometry(w).ok().and_then(|c| c.reply().ok());
        let pos = conn
            .translate_coordinates(w, root, 0, 0)
            .ok()
            .and_then(|c| c.reply().ok())
            .map(|r| (r.dst_x as i32, r.dst_y as i32));
        let mapped = conn
            .get_window_attributes(w)
            .ok()
            .and_then(|c| c.reply().ok())
            .map(|r| u8::from(r.map_state))
            .unwrap_or(0);
        if let (Some(g), Some((x, y))) = (geo, pos) {
            println!(
                "id={w:>9} mapped={mapped} ({x:>5},{y:>5}) {width:>4}x{height:<4} '{title}'",
                width = g.width,
                height = g.height
            );
        }
    }
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

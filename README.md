# watch-folder

Watches directories and sorts new files into subfolders by extension, with a
system tray icon and a settings window.

## Build

```
cargo build --release
```

## Run

```
./target/release/watch-folder
```

This starts the tray icon plus the settings window. From the window you can add
or remove watched folders, edit categories, ignored extensions and the
stability debounce, and toggle login startup. Each folder has a **Sort now**
button to sort its existing files on demand (plus a **Sort all folders now**
button). Closing the window hides it to the tray; use **Quit** in the tray menu
to actually exit.

Tray menu:

- **Status: Watching / Paused** — current state at the top of the menu
- **Show / hide settings** — toggle the window
- **Pause watching / Resume watching** — stop/continue moving files
- **Reload config** — re-read the YAML from disk
- **Open config file** — opens the YAML (or its folder if no editor is associated)
- **Open logs** — opens `~/.config/watch-dir/watcher.log`
- **Quit**

Closing the window keeps the watcher running in the tray. To get the window
back, click the tray icon and choose **Show / hide settings**.

### Install & update

Run the downloaded executable. It compares its own version with the running
instance:

- If it is **older**, it does nothing (the newer running copy stays).
- If it is **newer** — or nothing is installed yet — it shows a prompt:
  - **Install** — copies the exe to `%LOCALAPPDATA%\Programs\watch-folder`
    (Windows), `~/.local/bin/watch-folder` (Linux) or `~/Applications/watch-folder`
    (macOS), and enables autostart for the installed copy.
  - **Test run** — runs without installing.
  - **Cancel** — exits.

Once installed, the installed copy starts **hidden in the tray only** at login
(the settings window does not pop up). Use `--install` or `--test-run` to skip
the prompt.

### Single instance

Only one instance runs at a time. Launching the app again just shows the
existing window. If the binary you launch is **newer** than the running one, it
takes over: it asks the old instance to quit, then stops any remaining
`watch-folder` processes (including pre-v0.2.1 builds that have no instance
listener) and starts itself. Enabling "Start automatically on login" also
refreshes the stored path to the current binary, so upgrades don't keep
launching an old copy.

### Headless mode

For servers or machines without a tray:

```
./target/release/watch-folder --headless
```

On Linux/macOS this daemonizes (detaches) and writes a `watch-dir.pid` in the
working dir; on Windows there is no daemon support, so it runs in the
foreground. If no display/session bus is available, the app starts headless
automatically.

On first run it writes a default config to `~/.config/watch-dir/config.yaml`.
Logs go to `~/.config/watch-dir/watcher.log` (`RUST_LOG` controls verbosity,
defaults to `info`).

On startup it also scans each watched directory once and sorts the files already
sitting there, then keeps watching for new ones. Only files directly in the
watched directory are scanned — existing category subfolders are left untouched.

## Config

`~/.config/watch-dir/config.yaml`:

```yaml
config:
  watch:
    - path: ~/Downloads
    - path: ~/Desktop
      file-types:          # optional per-dir override
        images:
          - png
          - jpg
  file-types:               # default, used by any watch entry without its own file-types
    documents: [pdf, doc, docx, xls, xlsx, ppt, pptx, txt]
    images: [jpg, jpeg, png, gif, tiff, bmp]
    videos: [mp4, mov, avi, mkv, flv, m4v, rmvb, rm, 3gp, mpg, mpeg, webm]
    audios: [mp3, wav, wma, ogg, m4a, aac, aiff]
  ignore-extensions:        # skipped outright (partial/temp downloads)
    - crdownload
    - part
    - download
    - opdownload
    - tmp
    - partial
  stability:                 # debounce before moving a new file
    interval-ms: 1000
    required-stable-ticks: 2
```

Each `watch` entry is a directory; give it its own `file-types` block to sort it
differently from the default. Extension matching is case-insensitive.

A new file is only moved once its size holds steady for `required-stable-ticks`
consecutive polls (`interval-ms` apart) — protects in-progress downloads/copies
from being moved mid-write. Files with an ignored extension, or no
extension/category match, are left alone.

## Autostart

Use **Start automatically on login** in the settings window, or install the app
(the **Install** prompt / `--install`) which enables it automatically. This uses
the platform's native mechanism:

- **Linux/BSD**: an XDG autostart `.desktop` file in `~/.config/autostart/`
- **Windows**: a `HKCU\...\CurrentVersion\Run` registry entry
- **macOS**: a `LaunchAgent` plist

The registered entry points at the **installed** copy and starts it hidden in
the tray. There is no separate installer script to run — the executable copies
itself into place, registers autostart, and takes over any older running
instance.

To uninstall, turn off autostart in the settings window, quit from the tray, and
delete the install folder (`%LOCALAPPDATA%\Programs\watch-folder` on Windows,
`~/.local/bin/watch-folder` on Linux, `~/Applications/watch-folder` on macOS)
along with `~/.config/watch-dir/`.

## Linux tray notes

- The tray uses StatusNotifierItem (D-Bus); there are **no** GTK or
  libappindicator build dependencies.
- KDE Plasma, XFCE, Cinnamon, MATE, Budgie and similar desktops show it
  natively.
- Stock **GNOME hides tray icons**. Install the *AppIndicator and
  KStatusNotifierItem Support* GNOME extension, or run with `--headless`.

### Desktop pet on Linux (XWayland)

The desktop pet anchors itself to the bottom of the screen, stays always on
top, and can be dragged with the mouse. GNOME's Wayland session does not let a
normal app position, raise, or make its window click-through, so the app
switches to the X11 backend (XWayland) when an X server is available.

That backend needs one extra runtime library:

```
sudo apt install libxkbcommon-x11-0
```

Without it the app stays on native Wayland: file sorting and the tray still
work, but the pet cannot be positioned or kept on top.

A desktop entry and icon are written to `~/.local/share/applications` and
`~/.local/share/watch-folder` on startup, so the taskbar/dock shows the app's
own icon instead of a generic placeholder. GNOME matches the window to that
entry by its app id / `WM_CLASS` (`watch-folder`).

## Windows build notes

The tray/GUI stack needs the MSVC linker. Install **Visual Studio Build Tools**
with the "Desktop development with C++" workload, keep the default
`stable-x86_64-pc-windows-msvc` toolchain, then `cargo build --release`.

The GNU (`x86_64-pc-windows-gnu`) fallback currently can't link the GUI: rustc's
raw-dylib import-library generation calls
`dlltool --temp-prefix kernel32.dll:`, and the colon makes an invalid filename on
Windows. CI cross-compiles the Windows binary from Linux (where this works), and
the Linux build is unaffected.

## Releases

Pushing a `v*` tag (e.g. `v0.2.0`) triggers `.github/workflows/release.yml`,
which cross-compiles Linux and Windows binaries and attaches them to the GitHub
release:

```
git tag v0.2.0
git push origin v0.2.0
```

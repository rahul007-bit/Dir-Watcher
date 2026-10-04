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

### Single instance

Only one instance runs at a time. Launching the app again just shows the
existing window. If the binary you launch is **newer** than the running one, it
takes over: it asks the old instance to quit and then starts itself. (The very
first upgrade away from v0.2.0 needs the old process ended manually, since that
version had no instance listener.)

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

Use **Start automatically on login** in the settings window. This uses the
platform's native mechanism:

- **Linux/BSD**: an XDG autostart `.desktop` file in `~/.config/autostart/`
- **Windows**: a `HKCU\...\CurrentVersion\Run` registry entry
- **macOS**: a `LaunchAgent` plist

The `scripts/*.ps1` Scheduled Task from earlier versions still works but is now
legacy — use either the in-app toggle or the scripts, not both.

## Linux tray notes

- The tray uses StatusNotifierItem (D-Bus); there are **no** GTK or
  libappindicator build dependencies.
- KDE Plasma, XFCE, Cinnamon, MATE, Budgie and similar desktops show it
  natively.
- Stock **GNOME hides tray icons**. Install the *AppIndicator and
  KStatusNotifierItem Support* GNOME extension, or run with `--headless`.

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

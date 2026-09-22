# watch-folder

Daemon that watches directories and sorts new files into subfolders by extension.

## Build

```
cargo build --release
```

## Run

```
./target/release/watch-folder
```

On first run it writes a default config to `~/.config/watch-dir/config.yaml` and daemonizes (Unix only — see Platform notes). Delete `watch-dir.pid` in the working dir if the daemon needs a restart.

On startup it also scans each watched directory once and sorts the files already sitting there, then keeps watching for new ones. Only files directly in the watched directory are scanned — existing category subfolders are left untouched.

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

Each `watch` entry is a directory; give it its own `file-types` block to sort it differently from the default. Extension matching is case-insensitive.

A new file is only moved once its size holds steady for `required-stable-ticks` consecutive polls (`interval-ms` apart) — protects in-progress downloads/copies from being moved mid-write. Files with an ignored extension, or no extension/category match, are left alone.

## Platform notes

- Linux/macOS: daemonizes and detaches (fork-based).
- Windows: daemonizing isn't supported (the daemonize crate is Unix-only) — the process runs in the foreground.

### Windows autostart

The scripts in `scripts/` register a Scheduled Task that starts the watcher hidden at logon, so it keeps sorting in the background:

```powershell
# Register the task and start it now (needs an elevated shell)
sudo pwsh -File scripts\install-autostart.ps1

# Stop the watcher and remove the task
sudo pwsh -File scripts\uninstall-autostart.ps1
```

- Task name: `WatchFolder` (runs at logon, hidden, unlimited runtime, restarts on failure).
- `start-watcher.ps1` is the launcher; run it by hand to start the watcher without registering anything.
- Logs go to `%LOCALAPPDATA%\watch-folder\watcher.err.log` (`RUST_LOG` controls verbosity, defaults to `info`).

Registering a Scheduled Task requires administrator rights; `sudo` is the Windows 11 built-in elevation helper. If `sudo` isn't enabled, run the same command from an Administrator PowerShell instead.

If the build fails with `linker link.exe not found`, no MSVC toolchain is installed — switch to the bundled MinGW toolchain, which needs no Visual Studio:

```powershell
rustup toolchain install stable-x86_64-pc-windows-gnu
rustup default stable-x86_64-pc-windows-gnu
cargo build --release
```

## Releases

Pushing a `v*` tag (e.g. `v0.1.0`) triggers `.github/workflows/release.yml`, which cross-compiles Linux and Windows binaries and attaches them to the GitHub release:

```
git tag v0.1.0
git push origin v0.1.0
```

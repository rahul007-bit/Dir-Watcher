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
- Windows: daemonizing isn't supported (the daemonize crate is Unix-only) — the process runs in the foreground. Background it yourself via Task Scheduler.

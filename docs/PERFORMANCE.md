# Performance testing & measurement

This document records **how** the desktop-pet/overlay performance work was
measured, **what** was changed and **where** in the code, and the exact
scripts/commands to reproduce the tests.

> Scope: the `watch-folder` tray app and its "Mochi" desktop companion.
> Everything here was measured on Windows. Numbers are from one machine; treat
> them as relative (before/after), not absolute.

---

## 1. Test environment

| | |
|---|---|
| OS | Windows (console session, not RDP) |
| CPU | 16 logical cores |
| GPU | Intel(R) Graphics, OpenGL 3.3 (`driver 32.0.101.8801`), Vulkan available |
| Display | 1920 x 1200, taskbar at the bottom |
| Builds | `cargo build --release`; installed copy under `%LOCALAPPDATA%\Programs\watch-folder` |

---

## 2. Measurement methodology

### 2.1 CPU ("cores busy")

`TotalProcessorTime` is cumulative CPU time across all threads. Sampling the
delta over `N` seconds gives the **average number of logical cores** the process
keeps busy. Convert to the Task-Manager-style system percentage by dividing by
the logical-core count.

```powershell
function Sample([int]$sec, $p) {
  $t0 = $p.TotalProcessorTime.TotalSeconds
  Start-Sleep -Seconds $sec
  $p.Refresh()
  $t1 = $p.TotalProcessorTime.TotalSeconds
  return (($t1 - $t0) / $sec)     # == cores busy
}
$p     = Get-Process -Name 'watch-folder' | Select-Object -First 1
$cores = (Get-CimInstance Win32_ComputerSystem).NumberOfLogicalProcessors
$busy  = Sample 12 $p
"cores_busy={0:N3}  sys={1:N2}%  WS={2:N1}MB  Private={3:N1}MB" -f `
  $busy, ($busy / $cores * 100), ($p.WorkingSet64/1MB), ($p.PrivateMemorySize64/1MB)
```

Notes:
- `WS` = working set (what Task Manager's "Memory" roughly shows); `Private` is
  the private commit.
- Always let the app settle ~8-10 s after start before the first sample.
- Use enough seconds (10-15) so scheduling noise averages out.

### 2.2 Repaint rate + per-frame cost

`src/app.rs` contains a debug diagnostic, `record_frame()` (app.rs:422), that
logs frame rate and the cost of the eframe `update()` (and, on the non-Windows
path, pet update/render). Enable it with:

```powershell
$env:RUST_LOG = 'watch_folder=debug,info'
Get-Content "$env:USERPROFILE\.config\watch-dir\watcher.log" -Tail 40 |
  Select-String 'ui fps'
```

Example line:

```
ui fps 4.3  frame 0.80ms pet_update 0.01ms pet_render 1.02ms pet_enabled=true state=Sleeping papers=0 carried=0
```

This is how we distinguished "the repaint loop is spinning" (high fps) from
"the repaint is expensive" (high `frame`/`pet_render` ms). **Keep this function**
- it is intentionally left in the code for future debugging.

### 2.3 Transparency verification (pixel diff)

A screenshot cannot tell "transparent" from "black", so compare the pixels of
the overlay region with the app running vs stopped:

- With the app running, capture the screen.
- Stop the app, capture again.
- Sample pixels **inside the pet-window rect but away from the sprite**.
  - Pixels identical in both captures → the overlay is transparent there.
  - Pixels black (`#000000`) only when running → opaque background bug.

Use `winenum.cs` (below) to get the pet window rect, then compare. A ready-made
script is in §3.5.

### 2.4 Window geometry / styles

`EnumWindows` + `GetWindowRect` + `GetWindowLong(GWL_EXSTYLE)` tell us the
overlay rectangle and whether the styles are correct
(`WS_EX_LAYERED | TOOLWINDOW | TRANSPARENT | TOPMOST | NOACTIVATE`, and **no**
`WS_EX_APPWINDOW`). Helper (`winenum.cs`):

```csharp
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;

public class WinEnum {
    delegate bool EnumProc(IntPtr h, IntPtr p);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc cb, IntPtr p);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] static extern int GetWindowLong(IntPtr h, int i);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }

    public static List<string> ForPid(uint want) {
        var res = new List<string>();
        EnumWindows(delegate (IntPtr h, IntPtr p) {
            uint pid; GetWindowThreadProcessId(h, out pid);
            if (pid != want) return true;
            var sb = new StringBuilder(256); GetWindowTextW(h, sb, 256);
            RECT r; GetWindowRect(h, out r);
            int ex = GetWindowLong(h, -20);
            res.Add(string.Format("hwnd={0} vis={1} rect=({2},{3},{4},{5}) {6}x{7} ex=0x{8:X} title='{9}'",
                h, IsWindowVisible(h), r.Left, r.Top, r.Right, r.Bottom,
                r.Right - r.Left, r.Bottom - r.Top, ex, sb.ToString()));
            return true;
        }, IntPtr.Zero);
        return res;
    }
}
```

Usage:

```powershell
Add-Type -Path .\winenum.cs
$p = Get-Process -Name 'watch-folder' | Select-Object -First 1
[WinEnum]::ForPid([uint32]$p.Id) | Where-Object { $_ -match 'DirWatcherPet|watch-folder' }
```

### 2.5 Generating pet activity

The pet idles (sleeping) until a watched file is created. Drop temp files in the
watched folder to trigger the collect/sort animation:

```powershell
$dl = Join-Path $env:USERPROFILE 'Downloads'
1..12 | ForEach-Object { Set-Content -LiteralPath (Join-Path $dl "zzperftest_$_.txt") -Value 'x' }
# ... measure ...
Get-ChildItem -Path $dl -Recurse -Filter 'zzperftest_*' | Remove-Item -Force
```

---

## 3. Step-by-step test recipes

All recipes are PowerShell 7. Kill existing instances first:
`Stop-Process -Name 'watch-folder' -Force`.

### 3.1 Build & run

```powershell
# fast iteration (console window, ~faster link)
cargo build

# representative performance build
cargo build --release

# run hidden in the tray (how autostart launches it)
Start-Process .\target\release\watch-folder.exe -ArgumentList '--autostart' `
  -WorkingDirectory .
Start-Sleep -Seconds 9
```

### 3.2 Idle + active pet CPU

```powershell
$env:RUST_LOG = 'info'
$p     = Get-Process -Name 'watch-folder' | Select-Object -First 1
$cores = (Get-CimInstance Win32_ComputerSystem).NumberOfLogicalProcessors
function Sample([int]$s) {
  $a = $p.TotalProcessorTime.TotalSeconds; Start-Sleep -Seconds $s; $p.Refresh()
  ($p.TotalProcessorTime.TotalSeconds - $a) / $s
}
$idle = Sample 12
'IDLE   cores={0:N3} sys={1:N2}%' -f $idle, ($idle/$cores*100)

$dl = Join-Path $env:USERPROFILE 'Downloads'
1..12 | ForEach-Object { Set-Content (Join-Path $dl "zzperftest_$_.txt") 'x' }
Start-Sleep -Seconds 3
$active = Sample 12
'ACTIVE cores={0:N3} sys={1:N2}%' -f $active, ($active/$cores*100)
Get-ChildItem $dl -Recurse -Filter 'zzperftest_*' | Remove-Item -Force
```

### 3.3 Settings window: idle vs. move/resize

```powershell
# launch visible
Start-Process .\target\release\watch-folder.exe -WorkingDirectory .
Start-Sleep -Seconds 9
# measure OPEN-IDLE with Sample 8, then drag/resize by calling SetWindowPos in a loop
# (see docs scripts: GetWindowRect + SetWindowPos with SWP_NOSIZE / SWP_NOMOVE)
```

`SetWindowPos` P/Invoke used for the "drag/resize" stress:

```powershell
Add-Type -Namespace MW -Name W -MemberDefinition @'
[DllImport("user32.dll")] public static extern bool SetWindowPos(System.IntPtr h, System.IntPtr a, int x, int y, int cx, int cy, uint f);
[DllImport("user32.dll")] public static extern bool GetWindowRect(System.IntPtr h, out R r);
[StructLayout(LayoutKind.Sequential)] public struct R { public int L,T,Rt,B; }
'@
```

### 3.4 Show the settings window from the tray (for tests)

The app listens on loopback `127.0.0.1:49717`; send `SHOW` to pop the window:

```powershell
$c = New-Object System.Net.Sockets.TcpClient
$c.Connect('127.0.0.1', 49717)
$w = New-Object System.IO.StreamWriter($c.GetStream()); $w.WriteLine('SHOW'); $w.Flush()
$c.Close()
```

### 3.5 Transparency test (app on vs off)

```powershell
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type -Path .\winenum.cs

function Grab {
  $v = [System.Windows.Forms.SystemInformation]::VirtualScreen
  $b = New-Object System.Drawing.Bitmap($v.Width, $v.Height)
  $g = [System.Drawing.Graphics]::FromImage($b); $g.CopyFromScreen($v.Left,$v.Top,0,0,$b.Size); $g.Dispose()
  $b
}

Stop-Process -Name 'watch-folder' -Force -ErrorAction SilentlyContinue; Start-Sleep 3
Start-Process .\target\release\watch-folder.exe -ArgumentList '--autostart' -WorkingDirectory .; Start-Sleep 9
$p = Get-Process -Name 'watch-folder' | Select-Object -First 1
$pet = [WinEnum]::ForPid([uint32]$p.Id) | Where-Object { $_ -match 'DirWatcherPet' } | Select-Object -First 1
$m = [regex]::Match($pet, 'rect=\((\d+),(\d+),(\d+),(\d+)\)')
$L=[int]$m.Groups[1].Value; $T=[int]$m.Groups[2].Value; $R=[int]$m.Groups[3].Value
$on = Grab
Stop-Process -Name 'watch-folder' -Force; Start-Sleep 3
$off = Grab

# strip just above the sprite
$y=$T+8; $same=0; $diff=0
for ($x=$L+2; $x -lt $R-2; $x+=20) {
  $a=$on.GetPixel($x,$y); $b=$off.GetPixel($x,$y)
  if (([math]::Abs($a.R-$b.R)+[math]::Abs($a.G-$b.G)+[math]::Abs($a.B-$b.B)) -le 6) { $same++ } else { $diff++ }
}
"same=$same diff=$diff   (diff>0 with on=#000000 => opaque bug)"
```

### 3.6 Screenshot the overlay (visual check)

```powershell
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
# ... get $L,$T,$R,$B from winenum as in 3.5 ...
$v=[System.Windows.Forms.SystemInformation]::VirtualScreen
$full=New-Object System.Drawing.Bitmap($v.Width,$v.Height)
$g=[System.Drawing.Graphics]::FromImage($full); $g.CopyFromScreen($v.Left,$v.Top,0,0,$full.Size); $g.Dispose()
$crop=New-Object System.Drawing.Rectangle($L,$T,([Math]::Min($R,$v.Width)-$L),($B-$T))
$dst=New-Object System.Drawing.Bitmap(($crop.Width*3),($crop.Height*3))
$g2=[System.Drawing.Graphics]::FromImage($dst)
$g2.InterpolationMode=[System.Drawing.Drawing2D.InterpolationMode]::NearestNeighbor
$g2.DrawImage($full,(New-Object System.Drawing.Rectangle(0,0,$dst.Width,$dst.Height)),$crop,[System.Drawing.GraphicsUnit]::Pixel)
$dst.Save("$env:TEMP\native_pet.png"); $g2.Dispose(); $dst.Dispose(); $full.Dispose()
```

> PowerShell gotcha: variable names are case-insensitive, so `$b` (bitmap) and
> `$B` (bottom) collide. Use distinct names (e.g. `$full`, `$bottom`).

### 3.7 Sanity: which GL renderer eframe picked (optional)

Temporarily, in `src/app.rs`'s `run_native` closure:

```rust
if let Some(gl) = &cc.gl {
    use eframe::glow::{self, HasContext};
    unsafe {
        log::info!("GL_RENDERER={}", gl.get_parameter_string(glow::RENDERER));
        log::info!("GL_VENDOR={}",   gl.get_parameter_string(glow::VENDOR));
        log::info!("GL_VERSION={}",  gl.get_parameter_string(glow::VERSION));
    }
}
```

---

## 4. Baseline vs final

Original report: **~40 MB, ~6% CPU constant with the pet, ~0.1% without.**

| Scenario | Original | Final |
|---|---|---|
| Hidden / tray idle | ~1.0 core (**~6%** system, *constant*) | **0.005 cores (0.03%)** |
| Settings open, idle | n/a (~0.36 core under the wgpu experiment) | **0.016 cores (0.10%)** |
| Pet animating (file sort) | ~1.0 core | **0.041 cores (0.25%)** |
| Settings move/resize (active drag) | — | 0.365 cores (2.3%, transient) |
| Working set (hidden) | ~40 MB | ~77 MB (GDI+ pet), older glow-only ~73 MB |

The headline fix: the pet overlay no longer pins a core; idle is essentially
free, and animation costs roughly a quarter of one percent of the machine.

---

## 5. Improvement log (what changed & where)

### 5.1 Repaint storm from per-frame viewport commands  → `src/pet/mod.rs`
The old egui pet called, **every frame** inside the immediate viewport:

```rust
sub_ctx.send_viewport_cmd(egui::ViewportCommand::Transparent(true));
sub_ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(false));
sub_ctx.send_viewport_cmd(egui::ViewportCommand::MousePassthrough(true));
```

Each `send_viewport_cmd` marks the viewport dirty and requests an immediate
repaint, so the loop ran continuously (~24 fps) even while sleeping. The
`ViewportBuilder` already sets those flags, so the calls were removed.

Effect: idle pet fps 24 → 4.3.

### 5.2 Overlay window resized/repositioned every frame → `src/pet/mod.rs`
`strip_x` depended on the moving `current_x`, so the child window was
moved/resized every frame, forcing GL surface recreation. It is now a **fixed,
generous strip** computed from `folder_x` only (and clamped on-screen).

### 5.3 FPS cap that actually caps → `src/app.rs` (`update`, non-Windows path)
`request_repaint_after(delay)` schedules ~`delay - predicted_dt` because egui
subtracts the predicted frame time. The cap now adds `predicted_dt` back:

```rust
let predicted = Duration::from_secs_f32(ctx.input(|i| i.predicted_dt));
let budget = Duration::from_secs_f32(1.0 / 60.0) + predicted; // ~60 fps
ctx.request_repaint_after(delay.max(budget));
```

### 5.4 Native layered pet window (the big one) → `src/pet/native.rs`
`PetController` still owns all logic/state; a new dedicated thread owns the
presentation:

- one `WS_POPUP` window with `WS_EX_LAYERED | TOOLWINDOW | TRANSPARENT |
  TOPMOST | NOACTIVATE` (`native.rs` `run`),
- GDI+ (`GdiplusStartup`, `GdipCreateBitmapFromScan0`, `DrawImage`/`DrawString`)
  draws into a 32-bpp **premultiplied BGRA** buffer (`native.rs` `draw_pet`),
- presented with `UpdateLayeredWindow(..., AC_SRC_ALPHA)` (`native.rs` ~465),
- its own `PeekMessageW` pump; sleeps up to 250 ms when the pet is sleeping,
  ~16 ms while animating.

This gives real per-pixel-alpha transparency **and** low CPU, independent of the
eframe renderer.

Wiring: `App` no longer draws the pet on Windows; it shares enable/speed/offset
and reads status via `SharedPet` (`src/app.rs` `pet_shared`, `NativePet::spawn`
at app.rs:821). The egui pet remains for `#[cfg(not(windows))]`.

### 5.5 Settings window renderer: wgpu → glow → `Cargo.toml`, `src/app.rs`
During the investigation we tried the wgpu backend. It was needed only for the
old egui pet (glow's second-window present was slow; wgpu's was fast). Once the
pet went native, wgpu became unnecessary **and** its Vulkan present path cost
~0.6 cores for the visible settings window on this Intel driver. Reverting
eframe to the default **glow** backend dropped settings-idle to 0.10%.

- `Cargo.toml`: `eframe = "0.31"` (was `features = ["wgpu"]`).
- `src/app.rs`: removed `renderer: eframe::Renderer::Wgpu`.
- Also removed the now-pointless root `with_transparent(true)` and the black
  `clear_color` override (they only existed for the old wgpu pet).
- Reduced the settings preview repaint from 100 ms to 250 ms (app.rs:1563).

### 5.6 Autostart showed a blank window → `src/app.rs` (`startup_frame`)
eframe force-shows the root window after the first painted frame
(`window.set_visible(true)`), even when created hidden. On a hidden autostart
launch this left an empty window. `App::update` now parks it (offscreen, as a
tool window) on the first frames once it is hidden (app.rs:1520).

### 5.7 Ghost in Task View after closing → `src/app.rs` (`os_set_visible`)
winit sets `WS_EX_APPWINDOW`, which overrides `WS_EX_TOOLWINDOW`, so the parked
window stayed in Alt-Tab / "all virtual desktops". `os_set_visible` now clears
`WS_EX_APPWINDOW` when hiding and re-asserts it when showing (app.rs:332).

### 5.8 Install/autostart scripts removed
The executable self-installs and registers autostart (`install_self` +
`auto_launch_for`), so the legacy Scheduled-Task scripts were deleted
(`scripts/{install,start,uninstall}-autostart.ps1`) and the README updated.

---

## 6. Known remaining costs / notes

- **Settings drag/resize** is ~2-3% of the system *while actively dragging*.
  That is egui repainting on every `WM_MOVE`/`WM_SIZE`; it stops when the drag
  stops. If it ever matters we can coalesce resize events.
- The native pet is **Windows-only** (`#[cfg(windows)]`); other platforms keep
  the egui viewport path. TODOs for native Linux/macOS overlays are in
  `src/pet/mod.rs` and `src/app.rs`.
- `PetController::render()` (egui) is still compiled for non-Windows and for
  tests/`examples/`.
- `taskbar::apply_pet_window_transparency` (the old GDI color-key helper) was
  removed once the native GDI+ overlay replaced it.
- Numbers scale with machine; the key metric is *relative* improvement and the
  disappearance of the constant idle load.

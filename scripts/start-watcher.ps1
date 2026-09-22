<#
    Launches the watch-folder daemon hidden, with output redirected to
    %LOCALAPPDATA%\watch-folder\. Used by the scheduled task registered by
    install-autostart.ps1, but safe to run by hand as well.
#>
[CmdletBinding()]
param(
    # Block until the watcher exits. Required when launched from a Scheduled
    # Task so the task instance stays alive and Windows does not tear down the
    # child process when this script returns.
    [switch]$Wait
)

$ErrorActionPreference = 'Stop'

if (Get-Process -Name 'watch-folder' -ErrorAction SilentlyContinue) {
    return
}

$root = Split-Path -Parent $PSScriptRoot
$exe = Join-Path $root 'target\release\watch-folder.exe'
if (-not (Test-Path -LiteralPath $exe)) {
    throw "Watcher binary not found at '$exe'. Build it first with: cargo build --release"
}

$logDir = Join-Path $env:LOCALAPPDATA 'watch-folder'
New-Item -ItemType Directory -Force -Path $logDir | Out-Null

if (-not $env:RUST_LOG) {
    $env:RUST_LOG = 'info'
}

$startArgs = @{
    FilePath               = $exe
    WorkingDirectory       = $root
    WindowStyle            = 'Hidden'
    RedirectStandardOutput = (Join-Path $logDir 'watcher.out.log')
    RedirectStandardError  = (Join-Path $logDir 'watcher.err.log')
}
if ($Wait) {
    $startArgs.Wait = $true
}

Start-Process @startArgs

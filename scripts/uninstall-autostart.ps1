<#
    Removes the WatchFolder scheduled task and stops any running daemon.
#>
[CmdletBinding()]
param(
    [string]$TaskName = 'WatchFolder'
)

$ErrorActionPreference = 'Stop'

if (Get-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue) {
    Unregister-ScheduledTask -TaskName $TaskName -Confirm:$false
    Write-Host "Scheduled task '$TaskName' removed."
} else {
    Write-Host "Scheduled task '$TaskName' was not registered."
}

Get-Process -Name 'watch-folder' -ErrorAction SilentlyContinue | Stop-Process -Force

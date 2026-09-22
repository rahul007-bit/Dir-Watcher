<#
    Registers (or replaces) a Windows Scheduled Task that starts the
    watch-folder daemon hidden at logon, then starts it immediately.
#>
[CmdletBinding()]
param(
    [string]$TaskName = 'WatchFolder'
)

$ErrorActionPreference = 'Stop'

$launcher = Join-Path $PSScriptRoot 'start-watcher.ps1'
if (-not (Test-Path -LiteralPath $launcher)) {
    throw "Launcher not found: $launcher"
}

$action = New-ScheduledTaskAction -Execute 'powershell.exe' `
    -Argument "-NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File `"$launcher`" -Wait"

$trigger = New-ScheduledTaskTrigger -AtLogOn

$principal = New-ScheduledTaskPrincipal `
    -UserId "$env:USERDOMAIN\$env:USERNAME" `
    -LogonType Interactive `
    -RunLevel Limited

$settings = New-ScheduledTaskSettingsSet `
    -AllowStartIfOnBatteries `
    -DontStopIfGoingOnBatteries `
    -StartWhenAvailable `
    -ExecutionTimeLimit ([TimeSpan]::Zero) `
    -RestartCount 3 `
    -RestartInterval (New-TimeSpan -Minutes 1)

Register-ScheduledTask -TaskName $TaskName `
    -Action $action `
    -Trigger $trigger `
    -Principal $principal `
    -Settings $settings `
    -Description 'Watches ~/Downloads and sorts new files into category subfolders.' `
    -Force | Out-Null

Start-ScheduledTask -TaskName $TaskName

Write-Host "Scheduled task '$TaskName' registered and started."

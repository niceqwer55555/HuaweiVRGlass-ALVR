#requires -Version 5.1
<#
Background watcher: detects when the Huawei VR glasses are plugged into / unplugged
from the PC (by monitor EDID) and automatically runs auto-install-driver.ps1 to
register + switch the matching OpenVR driver. No window, no prompts.

Install: run setup-auto-vr-watcher.ps1 once (adds a Startup shortcut).
Run manually:  powershell -NoProfile -WindowStyle Hidden -File auto-vr-watcher.ps1
Stop:          stop the `AutoVrWatcher` PowerShell process, or delete the shortcut.

State + log live next to this script (auto_vr_state.txt / auto_vr_watcher.log).
#>
[CmdletBinding()]
param(
    [int]$PollSeconds = 3,
    [switch]$Once
)

$ErrorActionPreference = 'SilentlyContinue'
$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$logPath = Join-Path $scriptDir 'auto_vr_watcher.log'
$statePath = Join-Path $scriptDir 'auto_vr_state.txt'
$installer = Join-Path $scriptDir 'auto-install-driver.ps1'
$edidFile = Join-Path $scriptDir 'glasses_edid.txt'

function Log([string]$msg) {
    $line = "[{0:yyyy-MM-dd HH:mm:ss}] {1}" -f (Get-Date), $msg
    Add-Content -LiteralPath $logPath -Value $line -Encoding UTF8
}

function Get-Keywords {
    if (Test-Path -LiteralPath $edidFile -PathType Leaf) {
        $kw = @(Get-Content -LiteralPath $edidFile | Where-Object { $_.Trim() } | ForEach-Object { $_.Trim() })
        if ($kw.Count -gt 0) { return $kw }
    }
    return @('RTK2560', 'HUAWEI', 'VRGLASS', 'VR GLASS', 'HWV', 'AV03', 'CV10')
}

function Test-GlassesOnPc {
    $kw = Get-Keywords
    $mons = @(Get-PnpDevice -PresentOnly -Class Monitor -ErrorAction SilentlyContinue | ForEach-Object { $_.InstanceId })
    foreach ($m in $mons) {
        $up = $m.ToUpperInvariant()
        foreach ($k in $kw) { if ($k -and $up.Contains($k.ToUpperInvariant())) { return $true } }
    }
    return $false
}

# SteamVR rewrites openvrpaths.vrpath on exit and can drop our HMD driver
# registration without any plug/unplug event. Check the active driver is still
# registered; if not, re-run the installer to heal it.
function Test-DriverRegistered([string]$mode) {
    $vpPath = Join-Path $env:LOCALAPPDATA 'openvr\openvrpaths.vrpath'
    if (-not (Test-Path -LiteralPath $vpPath -PathType Leaf)) { return $false }
    try {
        $vp = Get-Content -LiteralPath $vpPath -Raw | ConvertFrom-Json
        $wanted = if ($mode -eq 'usb') { 'huawei-driver' } else { Join-Path $scriptDir '' }
        foreach ($p in @($vp.external_drivers)) {
            if (-not $p) { continue }
            $norm = ([string]$p).TrimEnd('\').ToLowerInvariant()
            if ($mode -eq 'usb' -and $norm.EndsWith('\huawei-driver')) { return $true }
            if ($mode -eq 'adb' -and (Test-Path -LiteralPath (Join-Path $p 'driver.vrdrivermanifest') -PathType Leaf)) {
                $m = Get-Content -LiteralPath (Join-Path $p 'driver.vrdrivermanifest') -Raw
                if ($m -match '"name"\s*:\s*"alvr_server"') { return $true }
            }
        }
    } catch {}
    return $false
}

# Single instance via a named mutex (no external dependency).
$mutexName = 'Local\AutoVrWatcher_SingleInstance'
$createdNew = $false
$mutex = New-Object System.Threading.Mutex($true, $mutexName, [ref]$createdNew)
if (-not $createdNew) {
    Log 'another watcher instance already running; exiting'
    if ($mutex) { $mutex.Close() }
    exit 0
}

Log "watcher started (poll=${PollSeconds}s, once=$Once)"

$prev = $null
if (Test-Path -LiteralPath $statePath -PathType Leaf) {
    $prev = (Get-Content -LiteralPath $statePath -Raw).Trim()
}

try {
    while ($true) {
        $onPc = Test-GlassesOnPc
        $state = if ($onPc) { 'usb' } else { 'adb' }
        $needInstall = ($state -ne $prev)
        if ($needInstall) {
            Log "state changed: '$prev' -> '$state' (glasses on PC = $onPc)"
        } elseif (-not (Test-DriverRegistered $state)) {
            Log "driver registration missing for '$state' (SteamVR may have cleared it); healing"
            $needInstall = $true
        }
        if ($needInstall) {
            if (Test-Path -LiteralPath $installer -PathType Leaf) {
                & $installer -Mode auto 2>&1 | ForEach-Object { Log ("  install: " + $_) }
                Log "applied mode '$state'"
            } else {
                Log "installer not found: $installer"
            }
            Set-Content -LiteralPath $statePath -Value $state -Encoding Ascii -Force
            $prev = $state
        }
        if ($Once) { break }
        Start-Sleep -Seconds $PollSeconds
    }
} finally {
    if ($mutex) { $mutex.Close() }
    Log 'watcher stopped'
}

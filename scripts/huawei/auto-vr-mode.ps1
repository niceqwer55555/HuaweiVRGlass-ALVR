#requires -Version 5.1
<#
Auto-VRMode.ps1 - pick the HMD driver from what is actually plugged in.

Policy
------
  phone on USB (visible to adb)  ->  alvr    (glasses on phone, stream over USB)
  no phone on USB                ->  huawei  (glasses on PC, official wired)

Rationale: the glasses have a single USB-C port. If a phone is wired to the PC,
the glasses are on the phone, so official wired mode is physically impossible and
ALVR must win. If no phone is present, the glasses are assumed to be on the PC and
the official driver should win.

This script only decides and applies the driver enable flags; the actual switching
is delegated to switch-vr-driver.ps1 in the same folder.

Usage
-----
    .\auto-vr-mode.ps1              # detect, switch, print what to do next
    .\auto-vr-mode.ps1 -Status      # detect and report only, change nothing
    .\auto-vr-mode.ps1 -Launch      # switch, then start the matching stack
    .\auto-vr-mode.ps1 -Watch       # keep running, re-evaluate as you plug/unplug
    .\auto-vr-mode.ps1 -Watch -Launch -Interval 3

    -AdbPath <exe>    use a specific adb.exe instead of auto-detecting
    -Interval <sec>   polling interval for -Watch (default 5)

Notes
-----
* "Phone on USB" means adb reports it with a usb: transport. A phone reachable
  only over WiFi (ip:5555) does NOT count - official wired stays selectable then.
* An unauthorized phone (USB debugging not confirmed) is reported as a warning and
  treated as absent, because ALVR would not be able to drive it anyway.
* SteamVR is stopped before the flags are edited (vrserver rewrites them on exit).
#>

param(
    [switch]$Status,
    [switch]$Launch,
    [switch]$Watch,
    [int]$Interval = 5,
    [string]$AdbPath
)

$ErrorActionPreference = 'Stop'
$switchScript = Join-Path $PSScriptRoot 'switch-vr-driver.ps1'

# ------------------------------------------------------------------ adb lookup
function Find-Adb {
    param([string]$Preferred)

    if ($Preferred -and (Test-Path -LiteralPath $Preferred)) { return $Preferred }

    $vrpathFile = Join-Path $env:LOCALAPPDATA 'openvr\openvrpaths.vrpath'
    $candidates = New-Object System.Collections.ArrayList

    if (Test-Path -LiteralPath $vrpathFile) {
        try {
            $vp = Get-Content -LiteralPath $vrpathFile -Raw | ConvertFrom-Json
            foreach ($d in $vp.external_drivers) {
                if (-not $d) { continue }
                $d = $d.TrimEnd('\')
                [void]$candidates.Add((Join-Path $d 'bin\platform-tools\adb.exe'))   # where ALVR looks
                [void]$candidates.Add((Join-Path $d 'platform-tools\adb.exe'))
            }
        } catch {}
    }

    $cmd = Get-Command adb.exe -ErrorAction SilentlyContinue
    if ($cmd) { [void]$candidates.Add($cmd.Source) }

    foreach ($c in $candidates) {
        if ($c -and (Test-Path -LiteralPath $c)) { return $c }
    }
    return $null
}

# ------------------------------------------------------------- phone detection
function Get-UsbPhones {
    param([string]$Adb)

    $result = @{ Phones = @(); Unauthorized = @(); Raw = @() }
    if (-not $Adb) { return $result }

    # stdout only: adb logs daemon startup / transport noise to stderr, which
    # would otherwise pollute the device list (and trip ErrorActionPreference).
    $prev = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try { $out = & $Adb devices -l 2>$null | Out-String } finally { $ErrorActionPreference = $prev }

    foreach ($line in ($out -split "`r?`n")) {
        # skip anything that is not a "<serial> <state> ..." line
        $line = $line.Trim()
        if (-not $line) { continue }
        if ($line -like 'List of devices*') { continue }
        if ($line -like '*daemon*') { continue }
        if ($line -notmatch '^[A-Za-z0-9._:-]+\s+\S') { continue }   # not a device row
        $result.Raw += $line

        if ($line -match '^(\S+)\s+unauthorized') {
            $result.Unauthorized += $Matches[1]
            continue
        }
        # state must be "device" and transport must be USB
        if ($line -match '^(\S+)\s+device\b') {
            $serial = $Matches[1]     # grab it before any later -match resets $Matches
            if ($line -match '\busb:') { $result.Phones += $serial }
        }
    }
    return $result
}

# ------------------------------------------------------------- apply + launch
function Apply-Mode {
    param([string]$Mode)

    if (-not (Test-Path -LiteralPath $switchScript)) {
        Write-Host "switch-vr-driver.ps1 not found next to this script: $switchScript" -ForegroundColor Red
        return $false
    }
    & $switchScript $Mode 2>&1 | Out-String -Width 200 | Write-Host
    return $true
}

function Start-VRStack {
    param([string]$Mode, [string]$Adb)

    $vp = Get-Content -LiteralPath (Join-Path $env:LOCALAPPDATA 'openvr\openvrpaths.vrpath') -Raw | ConvertFrom-Json
    $runtime = $vp.runtime | Select-Object -First 1
    $vrmonitor = Join-Path $runtime 'bin\win64\vrmonitor.exe'

    if ($Mode -eq 'alvr') {
        $alvrDir = $null
        foreach ($d in $vp.external_drivers) {
            if (-not $d) { continue }
            $d = $d.TrimEnd('\')
            if (Test-Path -LiteralPath (Join-Path $d 'ALVR Dashboard.exe')) { $alvrDir = $d; break }
        }
        if ($alvrDir) {
            Write-Host "Starting ALVR Dashboard: $alvrDir"
            Start-Process -FilePath (Join-Path $alvrDir 'ALVR Dashboard.exe') -WorkingDirectory $alvrDir | Out-Null
            Start-Sleep -Seconds 3
        } else {
            Write-Host "ALVR Dashboard.exe not found in any registered driver dir." -ForegroundColor Yellow
        }

        if ($Adb) {
            Write-Host "Launching VR client on the phone..."
            $prev = $ErrorActionPreference
            $ErrorActionPreference = 'Continue'
            try {
                & $Adb shell am start -a com.huawei.android.vr.action.MAIN -n com.lht.huaweivr.alvr/.HuaweiVrActivity 2>&1 |
                    Out-String -Width 200 | Write-Host
            } finally { $ErrorActionPreference = $prev }
        }
    } else {
        Write-Host "Official wired mode: no helper app required, just SteamVR."
    }

    if (Test-Path -LiteralPath $vrmonitor) {
        Write-Host "Starting SteamVR..."
        Start-Process -FilePath $vrmonitor | Out-Null
    } else {
        Write-Host "vrmonitor.exe not found: $vrmonitor" -ForegroundColor Red
    }
}

# --------------------------------------------------------------------- main
$adb = Find-Adb -Preferred $AdbPath
if ($adb) {
    Write-Host "adb            : $adb"
} else {
    Write-Host "adb            : NOT FOUND - USB phone cannot be detected, assuming official wired." -ForegroundColor Yellow
}

function Show-Decision {
    param([string]$Adb)

    $det = Get-UsbPhones $Adb
    $mode = if ($det.Phones.Count -gt 0) { 'alvr' } else { 'huawei' }

    Write-Host ""
    Write-Host "phone on USB   : $(if ($det.Phones.Count -gt 0) { $det.Phones -join ', ' } else { 'none' })"
    if ($det.Unauthorized.Count -gt 0) {
        Write-Host "  WARNING unauthorized (confirm USB debugging on phone): $($det.Unauthorized -join ', ')" -ForegroundColor Yellow
    }
    if ($det.Phones.Count -eq 0 -and $det.Raw.Count -gt 0) {
        Write-Host "  adb sees: $($det.Raw -join ' | ')" -ForegroundColor DarkGray
    }
    Write-Host "=> decided mode: $mode"
    return $mode
}

if ($Watch) {
    Write-Host "Watch mode - polling every $Interval s. Ctrl+C to stop." -ForegroundColor Cyan
    $last = $null
    while ($true) {
        $mode = Show-Decision $adb
        if ($mode -ne $last) {
            Write-Host "[$(Get-Date -Format 'HH:mm:ss')] switching to '$mode'" -ForegroundColor Cyan
            [void](Apply-Mode $mode)
            if ($Launch) { Start-VRStack -Mode $mode -Adb $adb }
            $last = $mode
        }
        Start-Sleep -Seconds $Interval
    }
}

$mode = Show-Decision $adb

if ($Status) {
    Write-Host ""
    Write-Host "-Status: nothing changed." -ForegroundColor Cyan
    exit 0
}

Write-Host ""
[void](Apply-Mode $mode)

if ($Launch) {
    Start-VRStack -Mode $mode -Adb $adb
} else {
    Write-Host ""
    Write-Host "Driver flags applied. Restart SteamVR to use it,"
    Write-Host "or re-run with -Launch to start everything now."
}

exit 0

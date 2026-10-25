#requires -Version 5.1
<#
Switch-VRDriver.ps1 - pick which HMD driver SteamVR activates.

Why this exists
---------------
SteamVR allows exactly one active HMD. On this machine both drivers declare
alwaysActivate=true and hmd_presence ["*.*"], so they fight for the HMD slot and
the winner depends on load order. SteamVR ships a per-driver kill switch in
steamvr.vrsettings:

    "driver_alvr_server" : { "enable" : true }
    "driver_huaweiVR"    : { "enable" : false }

Flipping those two is the supported way to choose a winner without touching
vrpathreg. Note the physical cabling is different for each mode:

    alvr   : glasses -> phone, phone <-> PC is the stream link (USB or WiFi)
    huawei : glasses -> PC directly (official wired, DisplayPort over USB-C)

They cannot run at the same time; this script only switches between them.

Usage
-----
    .\switch-vr-driver.ps1 status            # show current state, change nothing
    .\switch-vr-driver.ps1 huawei            # official wired: glasses on PC
    .\switch-vr-driver.ps1 alvr              # ALVR stream: glasses on phone
    .\switch-vr-driver.ps1 alvr -KeepSteamVR # do not kill vrserver first
    .\switch-vr-driver.ps1 status -Dedupe    # unregister duplicate huaweiVR copies

-Dedupe matters because SteamVR keys the enable flag on the driver NAME, so three
copies all called "huaweiVR" share one flag. Enabling the name turns all of them
on at once, and one of those copies is a shim (a 111 KB loader that forwards to
driver_huaweiVR_real.dll). Leaving only the real one makes the choice deterministic.

SteamVR rewrites steamvr.vrsettings on exit, so it is stopped before editing
and must be restarted afterwards for the change to take effect.
#>

param(
    [Parameter(Position = 0)]
    [ValidateSet('status', 'huawei', 'alvr')]
    [string]$Mode = 'status',

    [switch]$KeepSteamVR,

    [switch]$Dedupe
)

$ErrorActionPreference = 'Stop'

# ---------------------------------------------------------------- paths
$vrpathFile = Join-Path $env:LOCALAPPDATA 'openvr\openvrpaths.vrpath'
if (-not (Test-Path -LiteralPath $vrpathFile)) {
    Write-Host "openvrpaths.vrpath not found: $vrpathFile" -ForegroundColor Red
    exit 1
}

$vrpath = Get-Content -LiteralPath $vrpathFile -Raw | ConvertFrom-Json
$runtime = $vrpath.runtime | Select-Object -First 1
$configDir = $vrpath.config | Select-Object -First 1
$settingsFile = Join-Path $configDir 'steamvr.vrsettings'
$vrpathreg = Join-Path $runtime 'bin\win64\vrpathreg.exe'

Write-Host "SteamVR runtime : $runtime"
Write-Host "vrsettings      : $settingsFile"
Write-Host ""

# ------------------------------------------------------- registered drivers
$drivers = @()
if ($vrpath.external_drivers) { $drivers += $vrpath.external_drivers }
$drivers = $drivers | Where-Object { $_ } | ForEach-Object { $_.TrimEnd('\') }

function Get-ManifestName([string]$dir) {
    $m = Join-Path $dir 'driver.vrdrivermanifest'
    if (-not (Test-Path -LiteralPath $m)) { return $null }
    try {
        $txt = Get-Content -LiteralPath $m -Raw
        if ($txt -match '"name"\s*:\s*"([^"]+)"') { return $Matches[1] }
    } catch {}
    return $null
}

function Get-DriverDll([string]$dir) {
    $p = Join-Path $dir 'bin\win64\driver_' -ErrorAction SilentlyContinue
    $base = Join-Path $dir "bin\win64"
    if (-not (Test-Path -LiteralPath $base)) { return $null }
    $f = Get-ChildItem -LiteralPath $base -Filter 'driver_*.dll' -ErrorAction SilentlyContinue |
         Where-Object { $_.Name -notlike '*_real*' } | Select-Object -First 1
    return $f
}

Write-Host "== registered external drivers =="
$hwCopies = @()
foreach ($d in $drivers) {
    $exists = Test-Path -LiteralPath $d
    $nm = if ($exists) { Get-ManifestName $d } else { $null }
    $state = if ($exists) { 'OK' } else { 'MISSING' }
    $note = ''
    if ($exists) {
        $dll = Get-DriverDll $d
        if ($dll) {
            $kb = [math]::Round($dll.Length / 1KB)
            $note = " (driver dll ${kb} KB)"
            if ($dll.Length -lt 300KB) { $note += ' [SHIM]' }
            if ($nm -eq 'huaweiVR') {
                $hwCopies += [pscustomobject]@{ Dir = $d; Size = $dll.Length; Name = $dll.Name }
            }
        }
    }
    "{0,-8} name={1,-14}{2} {3}" -f $state, $(if ($nm) { $nm } else { '-' }), $note, $d | Write-Host
}
Write-Host ""

if ($hwCopies.Count -gt 1) {
    Write-Host ("WARNING: {0} copies share the driver name 'huaweiVR'." -f $hwCopies.Count) -ForegroundColor Yellow
    Write-Host "         SteamVR keys the enable flag on the NAME, so they switch together."
    Write-Host "         Run with -Dedupe to keep only the largest real one."
    Write-Host ""
}

# ------------------------------------------------------------- enable flags
$settingsRaw = $null
$hasSettings = Test-Path -LiteralPath $settingsFile
if ($hasSettings) { $settingsRaw = Get-Content -LiteralPath $settingsFile -Raw }

function Get-Enable([string]$raw, [string]$driverName) {
    if (-not $raw) { return $null }
    $pat = '"driver_' + [regex]::Escape($driverName) + '"\s*:\s*\{\s*"enable"\s*:\s*(true|false)\s*\}'
    $m = [regex]::Match($raw, $pat, [System.Text.RegularExpressions.RegexOptions]::IgnoreCase)
    if ($m.Success) { return ($m.Groups[1].Value -eq 'true') }
    return $null
}

$curAlvr = Get-Enable $settingsRaw 'alvr_server'
$curHw = Get-Enable $settingsRaw 'huaweiVR'

function Show([string]$label, $val) {
    $t = if ($val -eq $null) { 'not set (default = enabled)' } elseif ($val) { 'enabled' } else { 'DISABLED' }
    "{0,-18}: {1}" -f $label, $t | Write-Host
}

Write-Host "== per-driver enable flags =="
Show 'driver_alvr_server' $curAlvr
Show 'driver_huaweiVR' $curHw

if ($settingsRaw -match '"ActualHMDDriver"\s*:\s*"([^"]+)"') {
    Write-Host ""
    "Last HMD SteamVR actually used: {0}" -f $Matches[1] | Write-Host
}
Write-Host ""

if ($Dedupe) {
    if ($hwCopies.Count -le 1) {
        Write-Host "No duplicate huaweiVR registration to clean up."
    } elseif (-not (Test-Path -LiteralPath $vrpathreg)) {
        Write-Host "vrpathreg.exe not found: $vrpathreg" -ForegroundColor Red
    } else {
        $keep = $hwCopies | Sort-Object -Property Size -Descending | Select-Object -First 1
        Write-Host "Keeping  : $($keep.Dir)  [$([math]::Round($keep.Size/1KB)) KB]"
        foreach ($c in $hwCopies) {
            if ($c.Dir -eq $keep.Dir) { continue }
            Write-Host "Removing : $($c.Dir)  [$([math]::Round($c.Size/1KB)) KB]"
            & $vrpathreg removedriver "$($c.Dir)" 2>&1 | Out-String -Width 200 | Write-Host
        }
        Write-Host "Done. Re-run without -Dedupe to confirm." -ForegroundColor Green
    }
    Write-Host ""
}

if ($Mode -eq 'status') {
    Write-Host "status only - nothing changed." -ForegroundColor Cyan
    exit 0
}

# ------------------------------------------------------------------ apply
$wantAlvr = ($Mode -eq 'alvr')
$wantHw = -not $wantAlvr

if ($curAlvr -eq $wantAlvr -and $curHw -eq $wantHw) {
    Write-Host "Already in '$Mode' mode - no edit needed." -ForegroundColor Yellow
} else {
    if (-not $KeepSteamVR) {
        Write-Host "Stopping SteamVR (it rewrites vrsettings on exit)..." -ForegroundColor Yellow
        foreach ($n in 'vrmonitor', 'vrserver', 'vrcompositor', 'vrdashboard', 'vrwebhelper',
                      'vrserverhelper', 'vrmonitorhelper', 'ALVR Dashboard') {
            Get-Process -Name $n -ErrorAction SilentlyContinue |
                Stop-Process -Force -ErrorAction SilentlyContinue
        }
        Start-Sleep -Seconds 3
    }

    Copy-Item -LiteralPath $settingsFile -Destination ($settingsFile + '.bak_switch') -Force
    $raw = Get-Content -LiteralPath $settingsFile -Raw

    foreach ($pair in @(@('alvr_server', $wantAlvr), @('huaweiVR', $wantHw))) {
        $name = $pair[0]
        $val = if ($pair[1]) { 'true' } else { 'false' }
        $pat = '("driver_' + [regex]::Escape($name) + '"\s*:\s*\{\s*"enable"\s*:\s*)(true|false)(\s*\})'
        if ([regex]::IsMatch($raw, $pat, [System.Text.RegularExpressions.RegexOptions]::IgnoreCase)) {
            $raw = [regex]::Replace($raw, $pat, "`${1}$val`${3}",
                [System.Text.RegularExpressions.RegexOptions]::IgnoreCase)
        } else {
            # key absent - insert a new block after the opening brace
            $block = "`n   `"driver_$name`" : {`n      `"enable`" : $val`n   },"
            $idx = $raw.IndexOf('{')
            $raw = $raw.Insert($idx + 1, $block)
        }
        Write-Host "  driver_$name.enable -> $val"
    }

    [IO.File]::WriteAllText($settingsFile, $raw, (New-Object Text.UTF8Encoding($false)))
    Write-Host "written: $settingsFile  (backup: .bak_switch)" -ForegroundColor Green
}

# --------------------------------------------------------------- guidance
Write-Host ""
Write-Host "================ mode: $Mode ================" -ForegroundColor Cyan
if ($Mode -eq 'huawei') {
    Write-Host "Plug the glasses into the PC (USB-C / official DP alt-mode cable)."
    Write-Host "Do NOT run the ALVR Dashboard - ALVR is disabled in SteamVR now."
} else {
    Write-Host "Plug the glasses into the PHONE, phone links to PC by USB or WiFi."
    Write-Host "Start 'ALVR Dashboard.exe', then launch the client on the phone:"
    Write-Host "  adb shell am start -a com.huawei.android.vr.action.MAIN -n com.lht.huaweivr.alvr/.HuaweiVrActivity"
}
Write-Host ""
Write-Host "Restart SteamVR for the change to take effect (vrserver reads drivers at startup)."

if ($Mode -eq 'huawei' -and $curHw -eq $false) {
    Write-Host ""
    Write-Host "NOTE: driver_huaweiVR was DISABLED before this run - that alone is enough" -ForegroundColor Yellow
    Write-Host "      to make official wired mode show a black screen." -ForegroundColor Yellow
}

exit 0

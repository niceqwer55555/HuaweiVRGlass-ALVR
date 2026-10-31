#requires -Version 5.1
<#
  wired-adb.ps1 - restore / verify ALVR's wired (USB adb) link.

  ALVR's wired mode needs this chain, in order:
    1. adb.exe reachable         (bin\platform-tools\adb.exe next to the dashboard)
    2. adb server running
    3. session.json: wired_client_type = com.lht.huaweivr.alvr (variant Custom)
                     wired_client_autolaunch = false
    4. a device visible to adb   (data-capable USB cable + "USB debugging" enabled)
    5. ports 9943 and 9944 forwarded to the device
    6. the Huawei VR client activity actually resumed - ALVR's own autolaunch
       uses `monkey`, which starts the APK outside of VR mode

  Hardware note: the glasses occupy the phone's only USB-C port, so the wired
  path only exists while the phone is on USB *without* the glasses attached to
  it. Otherwise PC<->phone is WiFi and this mode does not apply.

  Usage:
    .\wired-adb.ps1                          # status report, changes nothing
    .\wired-adb.ps1 -Restore                 # fix session.json, wait for device, forward, start client
    .\wired-adb.ps1 -Watch                   # wait for a device, then restore
    .\wired-adb.ps1 -Restore -Wireless 192.168.1.50:5555
    .\wired-adb.ps1 -Restore -TimeoutSec 90

  ASCII-only on purpose: PowerShell 5.1 mis-reads UTF-8 without BOM.
#>
[CmdletBinding()]
param(
    [string]$Deploy = 'D:\HuaweiVRGlass_ALVR_Windows_v0.5.0',
    [string]$Session = '',
    [switch]$Restore,
    [switch]$Watch,
    [string]$Wireless = '',
    [int]$TimeoutSec = 30
)

$ErrorActionPreference = 'Continue'

$script:fail = 0
function Section([string]$t) { Write-Host ''; Write-Host "== $t ==" -ForegroundColor Cyan }
function Good([string]$t) { Write-Host "  [OK]   $t" -ForegroundColor Green }
function Bad([string]$t) { Write-Host "  [FAIL] $t" -ForegroundColor Red; $script:fail++ }
function Warn([string]$t) { Write-Host "  [WARN] $t" -ForegroundColor Yellow }
function Info([string]$t) { Write-Host "         $t" -ForegroundColor Gray }

# ALVR parses session.json with serde_json, which rejects a UTF-8 BOM.
# Set-Content -Encoding utf8 in PowerShell 5.1 always writes one, so write raw bytes.
function Write-JsonFile([string]$Path, $Object) {
    $text = $Object | ConvertTo-Json -Depth 32
    [System.IO.File]::WriteAllText($Path, $text, (New-Object System.Text.UTF8Encoding($false)))
}
function Note([string]$t) { Write-Host "$t" -ForegroundColor White }

function Resolve-Adb {
    $candidates = @(
        (Join-Path $Deploy 'bin\platform-tools\adb.exe'),
        (Join-Path $PSScriptRoot '..\..\bin\platform-tools\adb.exe'),
        (Join-Path $PSScriptRoot '..\..\build\end\server\bin\platform-tools\adb.exe')
    )
    foreach ($c in $candidates) {
        if (Test-Path -LiteralPath $c -PathType Leaf) { return (Resolve-Path -LiteralPath $c).Path }
    }
    $onPath = Get-Command adb.exe -ErrorAction SilentlyContinue
    if ($onPath) { return $onPath.Source }
    return $null
}

function Invoke-Adb {
    param([string[]]$Arguments)
    $raw = & $script:adb @Arguments 2>&1
    return (($raw | ForEach-Object { $_.ToString() }) -join "`n").Trim()
}

function Get-Devices {
    $text = Invoke-Adb @('devices')
    $result = @()
    foreach ($line in ($text -split "`n")) {
        $l = $line.Trim()
        if ($l -match '^(\S+)\s+(device|unauthorized|offline)$') {
            $result += [pscustomobject]@{ Serial = $Matches[1]; State = $Matches[2] }
        }
    }
    return $result
}

function Wait-Device {
    param([int]$Seconds)
    $deadline = (Get-Date).AddSeconds($Seconds)
    while ((Get-Date) -lt $deadline) {
        $dev = Get-Devices | Where-Object { $_.State -eq 'device' } | Select-Object -First 1
        if ($dev) { Write-Host ''; return $dev }
        Write-Host '.' -NoNewline -ForegroundColor DarkGray
        Start-Sleep -Seconds 2
    }
    Write-Host ''
    return $null
}

function Show-WiredSession {
    param([string]$Path, [switch]$Apply)
    $label = '2. session.json'
    Section $label
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        Bad "session.json not found: $Path"
        return
    }
    Info "session: $Path"

    try {
        $json = Get-Content -LiteralPath $Path -Raw | ConvertFrom-Json
        if (-not $json.session_settings.connection) {
            Bad 'session.json has no session_settings.connection block'
            return
        }
        $conn = $json.session_settings.connection

        $typeOk = ($conn.wired_client_type.Custom -eq 'com.lht.huaweivr.alvr') -and
                  ($conn.wired_client_type.variant -eq 'Custom')
        $autoOk = ($conn.wired_client_autolaunch -eq $false)

        if ($typeOk) { Good 'wired_client_type = com.lht.huaweivr.alvr (Custom)' }
        else { Warn "wired_client_type = $($conn.wired_client_type.Custom) / $($conn.wired_client_type.variant)" }

        if ($autoOk) { Good 'wired_client_autolaunch = false' }
        else { Warn "wired_client_autolaunch = $($conn.wired_client_autolaunch)" }

        if (-not $Apply -or ($typeOk -and $autoOk)) {
            if (-not $Apply -and -not ($typeOk -and $autoOk)) {
                Info 'run with -Restore to write the correct values'
            }
            return
        }

        $conn.wired_client_type = [pscustomobject]@{
            Custom  = 'com.lht.huaweivr.alvr'
            variant = 'Custom'
        }
        $conn.wired_client_autolaunch = $false

        Copy-Item -LiteralPath $Path -Destination "$Path.bak_adb" -Force
        Write-JsonFile -Path $Path -Object $json
        Good "patched (backup: $Path.bak_adb)"
        Info 'restart the dashboard so it reloads the session'
    } catch {
        Bad "failed to update session.json: $($_.Exception.Message)"
    }
}

# The dashboard's "Wired client" switch only adds this entry; add it here so the
# wired link exists without touching the UI (same shape as ClientListAction::AddIfMissing).
function Ensure-WiredClientEntry {
    param([string]$Path, [switch]$Apply)
    Section '3. wired client entry'
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        Bad "session.json not found: $Path"
        return
    }
    try {
        $json = Get-Content -LiteralPath $Path -Raw | ConvertFrom-Json
        $existing = $null
        if ($json.client_connections) {
            $existing = $json.client_connections.PSObject.Properties | Where-Object { $_.Name -eq 'client.wired' }
        }
        if ($existing) {
            Good "client.wired present (trusted: $($existing.Value.trusted))"
            if ($existing.Value.trusted -ne $true -and $Apply) {
                $existing.Value.trusted = $true
                Copy-Item -LiteralPath $Path -Destination "$Path.bak_adb" -Force
                Write-JsonFile -Path $Path -Object $json
                Good 'client.wired -> trusted'
            }
            return
        }

        Warn 'client.wired is missing (the dashboard shows the wired switch as off)'
        if (-not $Apply) { Info 'run with -Restore to add it'; return }

        $entry = [pscustomobject]@{
            display_name     = 'HUAWEI VR Glass (USB)'
            current_ip       = $null
            manual_ips       = @()
            trusted          = $true
            connection_state = 'Disconnected'
        }
        if (-not $json.client_connections) {
            $json | Add-Member -NotePropertyName client_connections -NotePropertyValue ([pscustomobject]@{})
        }
        $json.client_connections | Add-Member -NotePropertyName 'client.wired' -NotePropertyValue $entry -Force

        Copy-Item -LiteralPath $Path -Destination "$Path.bak_adb" -Force
        Write-JsonFile -Path $Path -Object $json
        Good 'client.wired added (trusted, no manual IPs)'
        Info 'restart the dashboard so it reloads the session'
    } catch {
        Bad "failed to update client list: $($_.Exception.Message)"
    }
}

# --------------------------------------------------------------------- start
Note ''
Note 'ALVR wired adb link'

Section '1. adb'
$script:adb = Resolve-Adb
if (-not $script:adb) {
    Bad 'adb.exe not found (expected bin\platform-tools\adb.exe next to the dashboard)'
    Note ''
    Note 'Cannot continue without adb.'
    exit 1
}
Good "adb: $script:adb"
$version = Invoke-Adb @('version')
Info (($version -split "`n" | Select-Object -First 1).Trim())
$serverStart = Invoke-Adb @('start-server')
if ($serverStart) { Info (($serverStart -split "`n" | Select-Object -Last 1)) }
Good 'adb server is running'

$sessionPath = if ($Session) { $Session } else { Join-Path $Deploy 'session.json' }
Show-WiredSession -Path $sessionPath -Apply:$Restore
Ensure-WiredClientEntry -Path $sessionPath -Apply:$Restore

Section '4. device'
if ($Wireless) {
    Info "connecting $Wireless ..."
    Info (Invoke-Adb @('connect', $Wireless))
}

$device = $null
if ($Watch) {
    Info 'waiting for a device (Ctrl+C to stop) ...'
    while (-not $device) { $device = Wait-Device -Seconds 4 }
} else {
    $device = Get-Devices | Where-Object { $_.State -eq 'device' } | Select-Object -First 1
    if (-not $device -and $Restore) {
        Info "no device yet, waiting up to $TimeoutSec s ..."
        $device = Wait-Device -Seconds $TimeoutSec
    }
}

foreach ($d in (Get-Devices)) {
    if ($d.State -eq 'device') { Good "device: $($d.Serial)  [device]" }
    elseif ($d.State -eq 'unauthorized') {
        Bad "device: $($d.Serial)  [unauthorized]"
        Info 'Unlock the phone and tap "Allow USB debugging"; tick "Always allow".'
    } else { Warn "device: $($d.Serial)  [$($d.State)]" }
}

if (-not $device) {
    Bad 'no authorized adb device'
    Info 'Checklist: data-capable cable (not charge-only), developer options on,'
    Info 'USB debugging on; the glasses must NOT hold the phone port at the same time.'
    Info 'WiFi alternative: .\wired-adb.ps1 -Restore -Wireless <phone-ip>:5555'
    Note ''
    Note 'Stopped at the device step; session.json was still checked above.'
    exit 1
}

$serial = $device.Serial

Section '5. client package'
$pkg = Invoke-Adb @('-s', $serial, 'shell', 'pm', 'list', 'packages', 'com.lht.huaweivr.alvr')
if ($pkg -match 'package:com\.lht\.huaweivr\.alvr') {
    Good 'com.lht.huaweivr.alvr installed'
} else {
    Bad 'com.lht.huaweivr.alvr is NOT installed on the device'
    Info 'Install the ALVR Huawei client APK (build\HuaweiVRGlass_ALVR_Android_v0.5.0_3dof_original.apk).'
}

Section '6. port forwarding'
foreach ($port in 9943, 9944) {
    $out = Invoke-Adb @('-s', $serial, 'forward', "tcp:$port", "tcp:$port")
    if ($LASTEXITCODE -eq 0 -or -not $out) { Good "tcp:$port -> tcp:$port" }
    else { Bad "forward $port failed: $out" }
}
Info ((Invoke-Adb @('-s', $serial, 'forward', '--list')) -replace "`n", '; ')

Section '7. VR client'
$clientPid = Invoke-Adb @('-s', $serial, 'shell', 'pidof', 'com.lht.huaweivr.alvr')
if ($clientPid -match '\d+') {
    Good "client already running (pid $($clientPid.Trim()))"
} elseif ($Restore -or $Watch) {
    Info (Invoke-Adb @('-s', $serial, 'shell', 'am', 'start',
            '-a', 'com.huawei.android.vr.action.MAIN',
            '-n', 'com.lht.huaweivr.alvr/.HuaweiVrActivity'))
    Start-Sleep -Seconds 3
    $clientPid = Invoke-Adb @('-s', $serial, 'shell', 'pidof', 'com.lht.huaweivr.alvr')
    if ($clientPid -match '\d+') { Good "client started (pid $($clientPid.Trim()))" }
    else { Bad 'client did not start - the glasses are probably not attached to the phone' }
} else {
    Warn 'client not running (run with -Restore to start it in VR mode)'
}

Section 'verdict'
if ($script:fail -eq 0) {
    Note '  Wired adb link looks ready.'
    Note '  Dashboard: Devices -> toggle "Wired client", then launch SteamVR.'
} else {
    Note "  $($script:fail) check(s) failed - see above."
}

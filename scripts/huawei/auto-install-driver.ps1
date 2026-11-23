#requires -Version 5.1
<#
Auto-install / auto-switch the OpenVR drivers this project ships.

  * Bundled official Huawei driver  -> <dashboard dir>\huawei-driver
  * ALVR driver                     -> <dashboard dir> (driver.vrdrivermanifest)

It registers BOTH paths once (remembered in hmd_drivers.json next to this
script), then keeps only the one matching the current physical connection in
openvrpaths.vrpath `external_drivers`, and sets the matching enable flag in
steamvr.vrsettings. Third-party drivers (driver4VR etc.) are never touched.

Run as normal user (no admin needed). Run again after plugging/unplugging the
glasses, or launch it from the dashboard / a shortcut.

Usage:
  .\auto-install-driver.ps1                 # detect + switch automatically
  .\auto-install-driver.ps1 -Mode usb       # force official driver
  .\auto-install-driver.ps1 -Mode adb       # force ALVR driver
  .\auto-install-driver.ps1 -Status         # report only, change nothing
#>
[CmdletBinding()]
param(
    [ValidateSet('auto', 'usb', 'adb')]
    [string]$Mode = 'auto',
    [switch]$Status
)

$ErrorActionPreference = 'Stop'
$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path

# The server directory IS the script's own directory - nothing hardcoded.
# This folder travels with the deployment package to any PC / any drive.
$art = (Resolve-Path -LiteralPath $scriptDir).Path

$alvrDir = $art
$huaweiDir = Join-Path $art 'huawei-driver'
$vrpathPath = Join-Path $env:LOCALAPPDATA 'openvr\openvrpaths.vrpath'

# Resolve SteamVR's config from openvrpaths.vrpath (no fixed drive/path).
# Fall back to a few conventional Steam installs only if not registered yet.
function Resolve-SteamVrSettings {
    if (Test-Path -LiteralPath $vrpathPath -PathType Leaf) {
        try {
            $vp = Get-Content -LiteralPath $vrpathPath -Raw | ConvertFrom-Json
            foreach ($c in @($vp.config)) {
                if (-not $c) { continue }
                $candidate = Join-Path $c 'steamvr.vrsettings'
                if (Test-Path -LiteralPath $candidate -PathType Leaf) { return $candidate }
            }
            foreach ($r in @($vp.runtime)) {
                if (-not $r) { continue }
                # runtime dir's ../config is also used by portable SteamVR
                $candidate = Join-Path (Split-Path -Parent $r) 'config\steamvr.vrsettings'
                if (Test-Path -LiteralPath $candidate -PathType Leaf) { return $candidate }
            }
        } catch {}
    }
    $programDirs = @($env:ProgramFiles, ${env:ProgramFiles(x86)}, "$env:LOCALAPPDATA\Programs")
    foreach ($base in $programDirs) {
        if (-not $base) { continue }
        $candidate = Join-Path $base 'Steam\steamapps\common\SteamVR\config\steamvr.vrsettings'
        if (Test-Path -LiteralPath $candidate -PathType Leaf) { return $candidate }
    }
    return $null
}
$steamSettings = Resolve-SteamVrSettings
if (-not $steamSettings) { throw 'SteamVR steamvr.vrsettings not found (start SteamVR once, or place its config path in openvrpaths.vrpath)' }
$sidecar = Join-Path $art 'hmd_drivers.json'
$edidFile = Join-Path $art 'glasses_edid.txt'

function Say([string]$msg, [string]$color = 'Gray') { Write-Host $msg -ForegroundColor $color }
function Fail([string]$msg) { throw $msg }

function Read-Json([string]$path) {
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { return $null }
    $raw = Get-Content -LiteralPath $path -Raw -Encoding UTF8
    try { return ($raw | ConvertFrom-Json) } catch { return $null }
}
function Write-Json([string]$path, $obj) {
    $json = $obj | ConvertTo-Json -Depth 12
    [IO.File]::WriteAllText($path, $json, (New-Object Text.UTF8Encoding($false)))
}
function ManifestName([string]$dir) {
    $m = Join-Path $dir 'driver.vrdrivermanifest'
    if (-not (Test-Path -LiteralPath $m -PathType Leaf)) { return $null }
    try {
        $t = Get-Content -LiteralPath $m -Raw
        if ($t -match '"name"\s*:\s*"([^"]+)"') { return $Matches[1] }
    } catch {}
    return $null
}
function Normalize([string]$p) { return $p.TrimEnd('\').TrimEnd('/').ToLowerInvariant() }

Say "dashboard dir : $art" 'Cyan'
Say "vrpath        : $vrpathPath" 'Cyan'

# --- Collect known HMD driver paths (sidecar + current registration) --------
$knownAlvr = New-Object 'System.Collections.Generic.List[string]'
$knownHuawei = New-Object 'System.Collections.Generic.List[string]'
$sidecarJson = Read-Json $sidecar
if ($sidecarJson) {
    foreach ($p in @($sidecarJson.alvr)) { if ($p -and -not (Normalize $p).Contains('\build\end\') -and -not $knownAlvr.Contains($p)) { $knownAlvr.Add($p) } }
    foreach ($p in @($sidecarJson.huawei)) { if ($p -and -not (Normalize $p).Contains('\build\end\') -and -not $knownHuawei.Contains($p)) { $knownHuawei.Add($p) } }
}

if (-not (Test-Path -LiteralPath $vrpathPath -PathType Leaf)) {
    Fail "openvrpaths.vrpath not found: $vrpathPath (start SteamVR once first)"
}
$vrpath = Read-Json $vrpathPath
if (-not $vrpath) { Fail "cannot parse $vrpathPath" }
$registered = @($vrpath.external_drivers | Where-Object { $_ })
foreach ($p in $registered) {
    $nm = if (Test-Path -LiteralPath $p -PathType Container) { ManifestName $p } else { $null }
    if ($nm -eq 'alvr_server' -and -not $knownAlvr.Contains($p)) { $knownAlvr.Add($p) }
    if ($nm -eq 'huaweiVR' -and -not $knownHuawei.Contains($p)) { $knownHuawei.Add($p) }
}
# Always remember the paths we ship.
foreach ($p in @($alvrDir)) { if (-not $knownAlvr.Contains($p)) { $knownAlvr.Add($p) } }
foreach ($p in @($huaweiDir)) { if (-not $knownHuawei.Contains($p)) { $knownHuawei.Add($p) } }

Say ""
Say "== bundled drivers ==" 'Cyan'
Say ("  ALVR   : {0}  [{1}]" -f $alvrDir, $(if (Test-Path (Join-Path $alvrDir 'driver.vrdrivermanifest')) {'OK'} else {'MISSING'}))
Say ("  Huawei : {0}  [{1}]" -f $huaweiDir, $(if (Test-Path (Join-Path $huaweiDir 'driver.vrdrivermanifest')) {'OK'} else {'MISSING'}))

# --- Decide mode ------------------------------------------------------------
$glassesOnPc = $false
if (Test-Path -LiteralPath $edidFile -PathType Leaf) {
    $kw = @(Get-Content -LiteralPath $edidFile | Where-Object { $_.Trim() } | ForEach-Object { $_.Trim() })
} else {
    $kw = @('RTK2560', 'HUAWEI', 'VRGLASS', 'VR GLASS', 'HWV', 'AV03', 'CV10')
}
$monitors = @(Get-PnpDevice -PresentOnly -Class Monitor -ErrorAction SilentlyContinue | ForEach-Object { $_.InstanceId })
foreach ($m in $monitors) {
    $up = $m.ToUpperInvariant()
    foreach ($k in $kw) { if ($k -and $up.Contains($k.ToUpperInvariant())) { $glassesOnPc = $true } }
}
if ($Mode -eq 'auto') { $chosen = if ($glassesOnPc) { 'usb' } else { 'adb' } }
else { $chosen = $Mode }
Say ""
Say "glasses on PC : $glassesOnPc"
Say "mode          : $chosen  ($(if ($Mode -eq 'auto') {'auto detect'} else {"forced -Mode $Mode"}))" 'Cyan'
if ($Status) { Say "-Status: nothing changed." 'Yellow'; return }

# --- Rewrite external_drivers: keep only the active HMD driver --------------
$keepList = if ($chosen -eq 'usb') { $knownHuawei } else { $knownAlvr }
$dropList = if ($chosen -eq 'usb') { $knownAlvr } else { $knownHuawei }
$keepNorm = @($keepList | ForEach-Object { Normalize $_ })
$dropNorm = @($dropList | ForEach-Object { Normalize $_ })
$newList = New-Object 'System.Collections.Generic.List[string]'
foreach ($p in $registered) {
    $n = Normalize $p
    # Never keep a driver registered from a packaging output directory: those
    # are temporary build artifacts and vanish when the folder is cleaned.
    if ($n.Contains('\build\end\')) { Say "  skip temp build path: $p" 'DarkGray'; continue }
    # Drop stale registrations pointing at missing folders (e.g. an old
    # driver4VR install). SteamVR rewrites the whole external_drivers table
    # when it hits one of these, wiping our HMD driver registration too.
    if (-not (Test-Path -LiteralPath $p -PathType Container)) {
        Say "  skip stale missing path: $p" 'DarkGray'; continue
    }
    if ($dropNorm -contains $n) { continue }   # drop inactive HMD driver
    if (-not $newList.Contains($p)) { $newList.Add($p) }
}
# Ensure the active driver is present (canonical path we ship).
$activeDir = if ($chosen -eq 'usb') { $huaweiDir } else { $alvrDir }
if ($keepNorm -notcontains (Normalize $activeDir)) { $keepList.Add($activeDir) }
if ($newList.Count -eq 0 -or -not ($newList | Where-Object { (Normalize $_) -eq (Normalize $activeDir) })) {
    $newList.Add($activeDir)
}
$vrpath.external_drivers = @($newList)
Write-Json $vrpathPath $vrpath
Write-Json $sidecar ([pscustomobject]@{ alvr = @($knownAlvr); huawei = @($knownHuawei) })

# --- Set enable flags -------------------------------------------------------
if (-not (Test-Path -LiteralPath $steamSettings -PathType Leaf)) {
    Fail "steamvr.vrsettings not found: $steamSettings"
}
$raw = Get-Content -LiteralPath $steamSettings -Raw
$wantHuawei = ($chosen -eq 'usb')
$alvrValue = if ($wantHuawei) { 'false' } else { 'true' }
$hwValue = if ($wantHuawei) { 'true' } else { 'false' }
# Use a MatchEvaluator callback instead of a replacement string: '$1' / '${1}'
# escaping in PowerShell string concatenation previously corrupted the JSON
# (it wrote a literal "${{1}}" into the file). The callback keeps prefix intact.
$evalAlvr = [System.Text.RegularExpressions.MatchEvaluator]{
    param($m) $m.Groups[1].Value + $alvrValue
}
$evalHw = [System.Text.RegularExpressions.MatchEvaluator]{
    param($m) $m.Groups[1].Value + $hwValue
}
$raw = [regex]::Replace($raw, '("driver_alvr_server"\s*:\s*\{\s*"enable"\s*:\s*)(true|false)', $evalAlvr)
$raw = [regex]::Replace($raw, '("driver_huaweiVR"\s*:\s*\{\s*"enable"\s*:\s*)(true|false)', $evalHw)
# Sanity check: never write invalid JSON back to SteamVR's config.
$null = $raw | ConvertFrom-Json
if (-not $raw.Contains('"driver_alvr_server"') -or -not $raw.Contains('"driver_huaweiVR"')) {
    Fail 'driver enable blocks missing after edit; refusing to write'
}
$bak = "$steamSettings.bak_installdrv"
Copy-Item -LiteralPath $steamSettings -Destination $bak -Force
[IO.File]::WriteAllText($steamSettings, $raw, (New-Object Text.UTF8Encoding($false)))

Say ""
Say "== result ==" 'Green'
Say "external_drivers now:"
foreach ($p in @($newList)) { Say "  $p" }
Say ("enable: alvr_server={0}  huaweiVR={1}" -f $(if ($wantHuawei) {'false'} else {'true'}), $(if ($wantHuawei) {'true'} else {'false'}))
Say "backup of vrsettings: $bak" 'DarkGray'
Say "Restart SteamVR for the load-list change to take effect." 'Yellow'

#requires -Version 5.1
<#
Auto-VRMode.ps1 - pick how the HMD is connected, then let SteamVR use the
matching driver. Two independent wired paths are supported:

  usb   - glasses docked straight into the PC (USB-C DP alt-mode), official
          HUAWEI wired driver. Detected by monitor EDID (e.g. DISPLAY\RTK2560).
  adb   - glasses on the phone, phone linked to the PC over USB with adb; ALVR
          streams to the phone. Detected by an adb device with a usb transport.

There is also auto mode: read the EDID and adb signals together and decide.
Because both cables can be attached at once (and EDID can be ambiguous), -Mode
lets you force a choice. The choice is persisted in vr_mode.txt next to the
dashboard executable, so the dashboard's own driver selection will not undo it.

Usage
-----
    .\auto-vr-mode.ps1                    # auto: detect both signals, switch
    .\auto-vr-mode.ps1 -Mode usb          # force official wired (glasses on PC)
    .\auto-vr-mode.ps1 -Mode adb          # force ALVR (phone on USB adb)
    .\auto-vr-mode.ps1 -Mode auto         # explicit auto
    .\auto-vr-mode.ps1 -Status            # detect + report only, change nothing
    .\auto-vr-mode.ps1 -Launch            # switch, then start the matching stack
    .\auto-vr-mode.ps1 -Watch             # re-evaluate on plug/unplug
    .\auto-vr-mode.ps1 -Watch -Launch -Interval 3

    -AdbPath <exe>          use a specific adb.exe instead of auto-detecting
    -DashboardDir <dir>     where to write vr_mode.txt (default: auto-detect)
    -EdidKeywords a,b       override the glasses EDID keywords
    -Interval <sec>         -Watch polling interval (default 5)

Notes
-----
* WriteAllText uses UTF-8 without BOM - vr_mode.txt is read by Rust.
* SteamVR is stopped before the flags are edited (vrserver rewrites them on
  exit). The actual flag switch is delegated to switch-vr-driver.ps1.
#>
[CmdletBinding()]
param(
    [ValidateSet('auto', 'usb', 'adb')]
    [string]$Mode = 'auto',
    [switch]$Status,
    [switch]$Launch,
    [switch]$Watch,
    [int]$Interval = 5,
    [string]$AdbPath,
    [string]$DashboardDir,
    [string]$EdidKeywords
)

$ErrorActionPreference = 'Stop'
$switchScript = Join-Path $PSScriptRoot 'switch-vr-driver.ps1'
$modeFile = 'vr_mode.txt'

# ------------------------------------------------------------------ helpers
function Write-TextFile {
    param([string]$Path, [string]$Text)
    $parent = Split-Path -Parent $Path
    if ($parent -and -not (Test-Path -LiteralPath $parent)) {
        New-Item -ItemType Directory -Path $parent -Force | Out-Null
    }
    [IO.File]::WriteAllText($Path, $Text, (New-Object Text.UTF8Encoding($false)))
}

function Get-EdidKeywordList {
    if ($EdidKeywords) {
        return @($EdidKeywords -split '[,\r\n]' | ForEach-Object { $_.Trim() } | Where-Object { $_ })
    }
    # Same defaults as Rust (steamvr_launcher/mod.rs DEFAULT_GLASSES_KEYWORDS).
    $result = @('RTK2560', 'HUAWEI', 'VRGLASS', 'VR GLASS', 'HWV', 'AV03', 'CV10')
    if ($DashboardDir) {
        $f = Join-Path $DashboardDir 'glasses_edid.txt'
        if (Test-Path -LiteralPath $f) {
            try {
                $custom = @((Get-Content -LiteralPath $f -Raw) -split '[,\r\n]' |
                            ForEach-Object { $_.Trim() } | Where-Object { $_ })
                if ($custom.Count -gt 0) { return $custom }
            } catch {}
        }
    }
    return $result
}

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

    # stdout only: adb logs daemon startup / transport noise to stderr
    $prev = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try { $out = & $Adb devices -l 2>$null | Out-String } finally { $ErrorActionPreference = $prev }

    foreach ($line in ($out -split "`r?`n")) {
        $line = $line.Trim()
        if (-not $line) { continue }
        if ($line -like 'List of devices*') { continue }
        if ($line -like '*daemon*') { continue }
        if ($line -notmatch '^[A-Za-z0-9._:-]+\s+\S') { continue }
        $result.Raw += $line

        if ($line -match '^(\S+)\s+unauthorized') {
            $result.Unauthorized += $Matches[1]
            continue
        }
        if ($line -match '^(\S+)\s+device\b') {
            $serial = $Matches[1]
            if ($line -match '\busb:') { $result.Phones += $serial }
        }
    }
    return $result
}

# --------------------------------------------------------- glasses-on-PC (EDID)
function Get-GlassesOnPc {
    param([string[]]$Keywords)

    $ids = New-Object System.Collections.ArrayList
    # Monitor PnP device ids: DISPLAY\RTK2560\5&....
    try {
        $pnp = @(Get-CimInstance Win32_PnPEntity -ErrorAction SilentlyContinue |
                 Where-Object { $_.PNPDeviceID -like 'DISPLAY\*' })
        foreach ($p in $pnp) {
            [void]$ids.Add(("$($p.PNPDeviceID) $($p.Name)"))
        }
    } catch {}
    # WMI EDID monitor instance names
    try {
        $mon = @(Get-CimInstance -Namespace root\wmi -ClassName WmiMonitorBasicDisplayParams -ErrorAction SilentlyContinue)
        foreach ($m in $mon) { [void]$ids.Add([string]$m.InstanceName) }
    } catch {}

    foreach ($id in $ids) {
        if (-not $id) { continue }
        $upper = $id.ToUpperInvariant()
        foreach ($k in $Keywords) {
            if ($k -and $upper.Contains($k.ToUpperInvariant())) {
                return [pscustomobject]@{ Found = $true; Match = $id }
            }
        }
    }
    return [pscustomobject]@{ Found = $false; Match = '' }
}

# ------------------------------------------------------------- dashboard dir
function Find-DashboardDir {
    if ($DashboardDir -and (Test-Path -LiteralPath $DashboardDir -PathType Container)) {
        return [IO.Path]::GetFullPath($DashboardDir)
    }
    $candidates = New-Object System.Collections.ArrayList
    $vrpathFile = Join-Path $env:LOCALAPPDATA 'openvr\openvrpaths.vrpath'
    if (Test-Path -LiteralPath $vrpathFile) {
        try {
            $vp = Get-Content -LiteralPath $vrpathFile -Raw | ConvertFrom-Json
            foreach ($d in $vp.external_drivers) {
                if (-not $d) { continue }
                $d = $d.TrimEnd('\')
                [void]$candidates.Add($d)
                [void]$candidates.Add((Split-Path -Parent $d))
            }
        } catch {}
    }
    [void]$candidates.Add((Split-Path -Parent $PSScriptRoot))
    [void]$candidates.Add((Join-Path (Split-Path -Parent $PSScriptRoot) 'build\end\server'))

    foreach ($c in $candidates) {
        if (-not $c) { continue }
        if (Test-Path -LiteralPath (Join-Path $c 'ALVR Dashboard.exe') -PathType Leaf) {
            return [IO.Path]::GetFullPath($c)
        }
    }
    return $null
}

# ------------------------------------------------------------- apply + launch
function Apply-Mode {
    param([string]$Target)

    if (-not (Test-Path -LiteralPath $switchScript)) {
        Write-Host "switch-vr-driver.ps1 not found next to this script: $switchScript" -ForegroundColor Red
        return $false
    }
    # auto-vr-mode uses connection kinds (usb/adb); the switch script uses
    # driver names (huawei/alvr).
    $driver = if ($Target -eq 'usb') { 'huawei' } else { 'alvr' }
    & $switchScript $driver 2>&1 | Out-String -Width 200 | Write-Host
    return $true
}

function Persist-Mode {
    param([string]$Target, [string]$Dir)
    if (-not $Dir) {
        Write-Host "WARN: dashboard dir not found; $modeFile not written." -ForegroundColor Yellow
        Write-Host "      Pass -DashboardDir <dir> so the choice survives SteamVR restart." -ForegroundColor Yellow
        return
    }
    $p = Join-Path $Dir $modeFile
    Write-TextFile -Path $p -Text "$Target`n"
    Write-Host "mode persisted: $p ($Target)" -ForegroundColor Green
}

function Start-VRStack {
    param([string]$Target, [string]$Adb, [string]$Dir)

    $vp = Get-Content -LiteralPath (Join-Path $env:LOCALAPPDATA 'openvr\openvrpaths.vrpath') -Raw | ConvertFrom-Json
    $runtime = $vp.runtime | Select-Object -First 1
    $vrmonitor = Join-Path $runtime 'bin\win64\vrmonitor.exe'

    if ($Target -eq 'adb') {
        $alvrDir = $Dir
        if (-not $alvrDir) {
            foreach ($d in $vp.external_drivers) {
                if (-not $d) { continue }
                $d = $d.TrimEnd('\')
                if (Test-Path -LiteralPath (Join-Path $d 'ALVR Dashboard.exe')) { $alvrDir = $d; break }
            }
        }
        if ($alvrDir) {
            Write-Host "Starting ALVR Dashboard: $alvrDir"
            Start-Process -FilePath (Join-Path $alvrDir 'ALVR Dashboard.exe') -WorkingDirectory $alvrDir | Out-Null
            Start-Sleep -Seconds 3
        } else {
            Write-Host "ALVR Dashboard.exe not found; pass -DashboardDir." -ForegroundColor Yellow
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
    Write-Host "adb             : $adb"
} else {
    Write-Host "adb             : NOT FOUND - USB phone cannot be detected" -ForegroundColor Yellow
}

$keywords = Get-EdidKeywordList
$dashboardDir = Find-DashboardDir
if ($dashboardDir) { Write-Host "dashboard dir   : $dashboardDir" }
Write-Host "EDID keywords   : $($keywords -join ', ')"

function Decide-Mode {
    param([string]$Adb, [string]$Explicit, [string[]]$Kw)

    $phones = Get-UsbPhones $Adb
    $glasses = Get-GlassesOnPc $Kw

    Write-Host ""
    Write-Host "phone on USB adb: $(if ($phones.Phones.Count -gt 0) { $phones.Phones -join ', ' } else { 'none' })"
    if ($phones.Unauthorized.Count -gt 0) {
        Write-Host "  WARNING unauthorized (confirm USB debugging): $($phones.Unauthorized -join ', ')" -ForegroundColor Yellow
    }
    Write-Host "glasses on PC   : $(if ($glasses.Found) { "YES ($($glasses.Match))" } else { 'no (EDID not matched)' })"

    # Physical topology decides - no Huawei-assistant dependency. The official
    # driver launches the assistant itself via its named mutex if it needs it.
    # Glasses visible as a monitor/USB device => docked to PC => official driver.
    $detected = if ($glasses.Found) {
        'usb'
    } elseif ($phones.Phones.Count -gt 0) {
        'adb'
    } else {
        $null
    }

    if ($Explicit -and $Explicit -ne 'auto') {
        $chosen = $Explicit
        $reason = "forced by -Mode $Explicit"
        if ($detected -and $detected -ne $chosen) {
            Write-Host "  NOTE: hardware suggests '$detected' but mode is '$chosen'." -ForegroundColor Yellow
        }
    } elseif ($detected) {
        $chosen = $detected
        $reason = 'auto detect'
        if ($glasses.Found -and $phones.Phones.Count -gt 0) {
            Write-Host "  NOTE: both connected; auto picked 'usb' (glasses on PC)." -ForegroundColor Yellow
        }
    } else {
        Write-Host "  neither a USB phone nor the glasses could be detected." -ForegroundColor Yellow
        Write-Host "  Use -Mode usb or -Mode adb to choose explicitly." -ForegroundColor Yellow
        return [pscustomobject]@{ Mode = $null; Reason = 'undetermined'; Phones = $phones; Glasses = $glasses }
    }

    Write-Host "=> mode: $chosen ($reason)"
    return [pscustomobject]@{ Mode = $chosen; Reason = $reason; Phones = $phones; Glasses = $glasses }
}

function Run-Once {
    $decision = Decide-Mode -Adb $adb -Explicit $Mode -Kw $keywords
    if (-not $decision.Mode) { return $null }

    if ($Status) {
        Write-Host ""
        Write-Host "-Status: nothing changed." -ForegroundColor Cyan
        return $decision.Mode
    }

    Write-Host ""
    [void](Apply-Mode -Target $decision.Mode)
    Persist-Mode -Target $decision.Mode -Dir $dashboardDir

    if ($Launch) {
        Start-VRStack -Target $decision.Mode -Adb $adb -Dir $dashboardDir
    } else {
        Write-Host ""
        Write-Host "Restart SteamVR to apply, or re-run with -Launch."
    }
    return $decision.Mode
}

if ($Watch) {
    Write-Host "Watch mode - polling every $Interval s. Ctrl+C to stop." -ForegroundColor Cyan
    $last = $null
    while ($true) {
        $current = Run-Once
        if ($current -and $current -ne $last) {
            Write-Host "[$(Get-Date -Format 'HH:mm:ss')] mode is '$current'" -ForegroundColor Cyan
            $last = $current
        }
        Start-Sleep -Seconds $Interval
    }
}

[void](Run-Once)
exit 0

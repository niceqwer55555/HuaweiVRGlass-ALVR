[CmdletBinding()]
param(
    [string]$DashboardUrl = 'http://127.0.0.1:8082',
    [double]$Scale = 0.6,
    [int]$BitrateMbps = 20,
    [int]$Threads = 6,
    [switch]$Disable
)

# Sets ALVR to use the FFmpeg/x264 software (CPU) encoder - required on this Steam Deck
# because the Valve Windows driver exposes no hardware H.264 encoder.
# Requires: ALVR Dashboard running with the GPL driver build (driver_alvr_server.dll + avcodec-*.dll).

$ErrorActionPreference = 'Stop'

function New-AlvrPath {
    param([Parameter(Mandatory)][string[]]$Names)
    @($Names | ForEach-Object { @{ Name = $_ } })
}

$force = -not $Disable.IsPresent

$values = @(
    @{
        path  = New-AlvrPath @('session_settings', 'video', 'encoder_config', 'software', 'force_software_encoding')
        value = $force
    },
    @{
        path  = New-AlvrPath @('session_settings', 'video', 'encoder_config', 'software', 'thread_count')
        value = $Threads
    },
    @{
        path  = New-AlvrPath @('session_settings', 'video', 'transcoding_view_resolution', 'variant')
        value = 'Scale'
    },
    @{
        path  = New-AlvrPath @('session_settings', 'video', 'transcoding_view_resolution', 'Scale')
        value = $Scale
    },
    @{
        path  = New-AlvrPath @('session_settings', 'video', 'bitrate', 'mode', 'ConstantMbps')
        value = $BitrateMbps
    },
    @{
        path  = New-AlvrPath @('session_settings', 'video', 'preferred_codec', 'variant')
        value = 'H264'
    }
)

$headers = @{ 'X-ALVR' = 'true' }
$body = @{ SetValues = $values } | ConvertTo-Json -Depth 15 -Compress
$uri = "$($DashboardUrl.TrimEnd('/'))/api/dashboard-request"

try {
    Invoke-WebRequest -Uri $uri -Method Post -Headers $headers -ContentType 'application/json' -TimeoutSec 5 -Body $body | Out-Null
}
catch {
    throw "Cannot reach ALVR dashboard at $DashboardUrl. Start 'ALVR Dashboard.exe' first. Original error: $($_.Exception.Message)"
}

if ($force) {
    Write-Host "Software encoding ENABLED (x264 ultrafast, $Threads threads), resolution scale $Scale, $BitrateMbps Mbps, H.264."
} else {
    Write-Host "Software encoding DISABLED (force_software_encoding = false)."
}
Write-Host "Restart SteamVR from the dashboard (or press 'Restart SteamVR') so the driver reloads the encoder."

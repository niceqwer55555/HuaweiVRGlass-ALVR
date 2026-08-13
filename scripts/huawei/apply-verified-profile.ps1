[CmdletBinding()]
param(
    [string]$DashboardUrl = 'http://127.0.0.1:8082'
)

$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true

function New-AlvrPath {
    param([Parameter(Mandatory)][string[]]$Names)

    @($Names | ForEach-Object { @{ Name = $_ } })
}

$values = @(
    @{
        path = New-AlvrPath @('session_settings', 'video', 'transcoding_view_resolution', 'Absolute', 'width')
        value = 896
    },
    @{
        path = New-AlvrPath @('session_settings', 'video', 'transcoding_view_resolution', 'Absolute', 'height', 'content')
        value = 896
    },
    @{
        path = New-AlvrPath @('session_settings', 'video', 'emulated_headset_view_resolution', 'Absolute', 'width')
        value = 1552
    },
    @{
        path = New-AlvrPath @('session_settings', 'video', 'emulated_headset_view_resolution', 'Absolute', 'height', 'content')
        value = 1552
    },
    @{
        path = New-AlvrPath @('session_settings', 'video', 'preferred_fps')
        value = 70.0
    },
    @{
        path = New-AlvrPath @('session_settings', 'video', 'bitrate', 'mode', 'ConstantMbps')
        value = 14
    },
    @{
        path = New-AlvrPath @('session_settings', 'video', 'preferred_codec', 'variant')
        value = 'H264'
    },
    @{
        path = New-AlvrPath @('session_settings', 'video', 'foveated_encoding', 'enabled')
        value = $false
    },
    @{
        path = New-AlvrPath @('session_settings', 'video', 'max_buffering_frames')
        value = 1.0
    },
    @{
        path = New-AlvrPath @('session_settings', 'connection', 'stream_protocol', 'variant')
        value = 'Tcp'
    },
    @{
        path = New-AlvrPath @('session_settings', 'connection', 'max_queued_server_video_frames')
        value = 1
    }
)

$headers = @{ 'X-ALVR' = 'true' }
$requestBody = @{ SetValues = $values } | ConvertTo-Json -Depth 15 -Compress
$requestUri = "$($DashboardUrl.TrimEnd('/'))/api/dashboard-request"

try {
    Invoke-WebRequest `
        -Uri $requestUri `
        -Method Post `
        -Headers $headers `
        -ContentType 'application/json' `
        -TimeoutSec 5 `
        -Body $requestBody |
        Out-Null
}
catch {
    throw "无法连接 ALVR 服务端。请先启动 ALVR Dashboard.exe 和 SteamVR，再运行本脚本。原始错误：$($_.Exception.Message)"
}

Write-Host '已应用 Huawei VR Glass 真机验收配置：'
Write-Host '  串流：896x896/眼，H.264，14 Mbps，70 Hz'
Write-Host '  显示目标：1552x1552/眼，固定注视点编码关闭'
Write-Host '  延迟：客户端缓冲 1 帧，PC 待发送队列 1 帧，TCP'
Write-Host '请在 Dashboard 中重启 SteamVR，使分辨率和刷新率生效。'

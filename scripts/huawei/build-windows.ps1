[CmdletBinding()]
param(
    [ValidateSet('Debug', 'Release')]
    [string]$Configuration = 'Release',
    [string]$ToolsRoot,
    [switch]$ResetConfiguration
)

$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$projectRoot = (Resolve-Path (Join-Path $repoRoot '..')).Path

# 工具链位置统一由 toolchain-env.ps1 运行时探测（默认根目录 D:\Tools）。
# 这里不再调用 VsDevCmd.bat —— 它会调用 reg.exe，在构建沙箱里被拦截。
. (Join-Path $PSScriptRoot 'toolchain-env.ps1')
$tc = Get-VrToolchain -ToolsRoot $ToolsRoot -Validate
$tc.Apply()
Write-Host ($tc.Summary())

Push-Location $repoRoot
try {
    $buildArguments = @('xtask', 'build-streamer')
    if ($Configuration -eq 'Release') {
        $buildArguments += '--release'
    }
    # GPL: bundle FFmpeg so the x264 software encoder is available. This machine (Steam Deck
    # Windows, AMD Van Gogh) has no working hardware encoder, without it the streamer cannot
    # start any encoder at all.
    $buildArguments += '--gpl'
    if (-not $ResetConfiguration) {
        $buildArguments += '--keep-config'
    }
    # cargo 把进度信息写进 stderr，在 $ErrorActionPreference='Stop' 下会导致构建被中断
    $previousErrorAction = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    $PSNativeCommandUseErrorActionPreference = $false
    & $tc.CargoExe @buildArguments 2>&1 | Tee-Object -FilePath (Join-Path $repoRoot 'build\windows-build.log')
    $cargoExit = $LASTEXITCODE
    $ErrorActionPreference = $previousErrorAction
    if ($cargoExit -ne 0) {
        throw "cargo xtask build-streamer 失败，退出码：$cargoExit"
    }

    $streamerDirectory = Join-Path $repoRoot 'build\alvr_streamer_windows'
    if (-not (Test-Path -LiteralPath $streamerDirectory -PathType Container)) {
        throw "构建结束但未找到 Windows 串流端目录：$streamerDirectory"
    }

    $licensesDirectory = Join-Path $streamerDirectory 'licenses'
    New-Item -ItemType Directory -Path $licensesDirectory -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $repoRoot 'LICENSE') -Destination (Join-Path $licensesDirectory 'ALVR-MIT.txt') -Force
    Copy-Item -LiteralPath (Join-Path $repoRoot 'alvr\dashboard\resources\Noto-CJK-LICENSE.txt') -Destination (Join-Path $licensesDirectory 'Noto-CJK-OFL.txt') -Force
    Copy-Item -LiteralPath (Join-Path $repoRoot 'alvr\dashboard\resources\MaterialSymbols-LICENSE.txt') -Destination (Join-Path $licensesDirectory 'Material-Symbols-Apache-2.0.txt') -Force
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'README.md') -Destination (Join-Path $streamerDirectory '华为专版说明.md') -Force
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'apply-verified-profile.ps1') -Destination (Join-Path $streamerDirectory '应用华为已验收配置.ps1') -Force

    $artifacts = Join-Path $projectRoot 'artifacts'
    New-Item -ItemType Directory -Path $artifacts -Force | Out-Null
    $destinationZip = Join-Path $artifacts 'HuaweiVRGlass-ALVR-Windows-v20.14.1.zip'
    if (Test-Path -LiteralPath $destinationZip) {
        Remove-Item -LiteralPath $destinationZip -Force
    }

    $packageRoot = Join-Path $repoRoot 'build\huawei_windows_package'
    $resolvedBuildRoot = [System.IO.Path]::GetFullPath((Join-Path $repoRoot 'build')).TrimEnd('\') + '\'
    $resolvedPackageRoot = [System.IO.Path]::GetFullPath($packageRoot).TrimEnd('\') + '\'
    if (-not $resolvedPackageRoot.StartsWith($resolvedBuildRoot, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "拒绝清理仓库 build 目录以外的路径：$resolvedPackageRoot"
    }
    if (Test-Path -LiteralPath $packageRoot) {
        Remove-Item -LiteralPath $packageRoot -Recurse -Force
    }
    New-Item -ItemType Directory -Path $packageRoot -Force | Out-Null

    $excludedRuntimeFiles = @(
        'session.json',
        'session_old.json',
        'session_log.txt',
        'crash_log.txt'
    )
    Get-ChildItem -LiteralPath $streamerDirectory -Force |
        Where-Object { $_.Name -notin $excludedRuntimeFiles } |
        Copy-Item -Destination $packageRoot -Recurse -Force

    Compress-Archive -Path (Join-Path $packageRoot '*') -DestinationPath $destinationZip -CompressionLevel Optimal
    Remove-Item -LiteralPath $packageRoot -Recurse -Force

    Write-Host "Windows 构建完成：$destinationZip"
    Get-FileHash -Algorithm SHA256 -LiteralPath $destinationZip
}
finally {
    Pop-Location
}

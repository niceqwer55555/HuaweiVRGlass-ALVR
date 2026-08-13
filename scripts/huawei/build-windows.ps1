[CmdletBinding()]
param(
    [ValidateSet('Debug', 'Release')]
    [string]$Configuration = 'Release'
)

$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$projectRoot = (Resolve-Path (Join-Path $repoRoot '..')).Path
$vsDevCmd = 'C:\Program Files\Microsoft Visual Studio\2022\Community\Common7\Tools\VsDevCmd.bat'
$libclangCandidates = @(
    'C:\Program Files\LLVM\bin',
    'C:\Program Files\Autodesk\3ds Max 2024',
    'C:\Program Files\Autodesk\Maya2023\bin'
)
$libclangPath = $libclangCandidates |
    Where-Object { Test-Path -LiteralPath (Join-Path $_ 'libclang.dll') -PathType Leaf } |
    Select-Object -First 1

if (-not (Test-Path -LiteralPath $vsDevCmd -PathType Leaf)) {
    throw "找不到 Visual Studio 开发环境：$vsDevCmd"
}
if ([string]::IsNullOrWhiteSpace($libclangPath)) {
    throw "找不到 libclang.dll；已检查：$($libclangCandidates -join '、')"
}

$vsEnvironment = & $env:ComSpec /d /s /c "`"$vsDevCmd`" -no_logo -arch=x64 -host_arch=x64 && set"
foreach ($line in $vsEnvironment) {
    if ($line -match '^([^=]+)=(.*)$') {
        [Environment]::SetEnvironmentVariable($Matches[1], $Matches[2], 'Process')
    }
}
$env:LIBCLANG_PATH = $libclangPath

Push-Location $repoRoot
try {
    $buildArguments = @('xtask', 'build-streamer')
    if ($Configuration -eq 'Release') {
        $buildArguments += '--release'
    }
    cargo @buildArguments

    $streamerDirectory = Join-Path $repoRoot 'build\alvr_streamer_windows'
    if (-not (Test-Path -LiteralPath $streamerDirectory -PathType Container)) {
        throw "构建结束但未找到 Windows 串流端目录：$streamerDirectory"
    }

    $licensesDirectory = Join-Path $streamerDirectory 'licenses'
    New-Item -ItemType Directory -Path $licensesDirectory -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $repoRoot 'LICENSE') -Destination (Join-Path $licensesDirectory 'ALVR-MIT.txt') -Force
    Copy-Item -LiteralPath (Join-Path $repoRoot 'alvr\dashboard\resources\Noto-CJK-LICENSE.txt') -Destination (Join-Path $licensesDirectory 'Noto-CJK-OFL.txt') -Force
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'README.md') -Destination (Join-Path $streamerDirectory '华为专版说明.md') -Force

    $artifacts = Join-Path $projectRoot 'artifacts'
    New-Item -ItemType Directory -Path $artifacts -Force | Out-Null
    $destinationZip = Join-Path $artifacts 'HuaweiVRGlass-ALVR-Windows-v20.14.1.zip'
    if (Test-Path -LiteralPath $destinationZip) {
        Remove-Item -LiteralPath $destinationZip -Force
    }
    Compress-Archive -Path (Join-Path $streamerDirectory '*') -DestinationPath $destinationZip -CompressionLevel Optimal

    Write-Host "Windows 构建完成：$destinationZip"
    Get-FileHash -Algorithm SHA256 -LiteralPath $destinationZip
}
finally {
    Pop-Location
}

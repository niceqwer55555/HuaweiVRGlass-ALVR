[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path

if (-not (Get-Command node -ErrorAction SilentlyContinue)) {
    throw '找不到 Node.js，无法生成 Huawei VR Launcher 图标。'
}

Push-Location $repoRoot
try {
    node (Join-Path $PSScriptRoot 'generate-vr-launcher-icons.mjs')
}
finally {
    Pop-Location
}

[CmdletBinding()]
param(
    [ValidateSet('armeabi-v7a', 'arm64-v8a')]
    [string]$AndroidAbi = 'armeabi-v7a',
    [string]$SdkRoot
)

$ErrorActionPreference = 'Stop'
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path

if ([string]::IsNullOrWhiteSpace($SdkRoot)) {
    $SdkRoot = Join-Path $repoRoot '..\sdk\hvrsdk-openxr-3.5.0.79'
}
$SdkRoot = (Resolve-Path -LiteralPath $SdkRoot).Path

$sourceLoader = Join-Path $SdkRoot "sdkDemo\HVRSDK_XrDemo\jni\$AndroidAbi\libxr_loader.so"

$expectedHashes = switch ($AndroidAbi) {
    'armeabi-v7a' {
        @{
            Loader = '7D2CD6D4A4F2C674CC96C53F35625DE0AB7D11171046B18E0DBAF87496E1B337'
        }
    }
    'arm64-v8a' {
        @{
            Loader = '9035F04659FCBF649FF53AEEC7A85C42169CB6A84C63552DCDD4F496B88193AD'
        }
    }
}

$runtimeFiles = @(
    @{
        Label = '华为 OpenXR 加载器'
        Source = $sourceLoader
        Destination = 'libopenxr_loader_huawei.so'
        Hash = $expectedHashes.Loader
    }
)

$runtimeDirectory = Join-Path $repoRoot "deps\android_openxr\$AndroidAbi"
New-Item -ItemType Directory -Path $runtimeDirectory -Force | Out-Null
Get-ChildItem -LiteralPath $runtimeDirectory -Filter '*.so' -File | Remove-Item -Force

foreach ($runtimeFile in $runtimeFiles) {
    if (-not (Test-Path -LiteralPath $runtimeFile.Source -PathType Leaf)) {
        throw "找不到$($runtimeFile.Label)：$($runtimeFile.Source)"
    }

    $actualHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $runtimeFile.Source).Hash
    if ($actualHash -ne $runtimeFile.Hash) {
        throw "$($runtimeFile.Label)校验失败：期望 $($runtimeFile.Hash)，实际 $actualHash"
    }

    $destination = Join-Path $runtimeDirectory $runtimeFile.Destination
    Copy-Item -LiteralPath $runtimeFile.Source -Destination $destination -Force

    Write-Host "已准备$($runtimeFile.Label)：$destination"
    Write-Host "SHA-256：$actualHash"
}

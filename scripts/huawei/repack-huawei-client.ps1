#Requires -Version 5.1
<#
    以已打包好的客户端 APK 为模板重打包含新原生库的安装包。

    华为私有 SDK（hvrbridge.jar / hvrprompt.aar 原始包）在部分开发机上不可用；
    但它们的编译产物已经固化在既有 APK 中（classes.dex、res/*、resources.arsc、
    AndroidManifest.xml、libxr_loader.so）。本脚本直接复用这些产物，只替换
    lib/armeabi-v7a/libalvr_client_openxr.so，再重新对齐并签名，从而在不取回
    原始 SDK 的前提下重建 APK。

    用法：
        .\repack-huawei-client.ps1 -NativeLibrary target\armv7-linux-androideabi\release\libalvr_client_openxr.so `
            -BaseApk build\HuaweiVRGlass_ALVR_Android_v0.5.0_6dof.apk
#>
[CmdletBinding()]
param(
    [string]$NativeLibrary = (Join-Path $PSScriptRoot '..\..\target\armv7-linux-androideabi\release\libalvr_client_openxr.so'),
    [string]$BaseApk = (Join-Path $PSScriptRoot '..\..\build\HuaweiVRGlass_ALVR_Android_v0.5.0_6dof.apk'),
    [string]$OutputApk,
    [string]$AndroidSdkRoot,
    [string]$JavaHome,
    [string]$Keystore,
    [string]$KeystoreAlias = 'androiddebugkey',
    [string]$KeystorePassword = 'alvrclient',
    [string]$ToolsRoot
)

$ErrorActionPreference = 'Stop'

function Resolve-FirstExistingPath {
    param([string[]]$Candidates)
    foreach ($candidate in $Candidates) {
        if (-not [string]::IsNullOrWhiteSpace($candidate) -and (Test-Path -LiteralPath $candidate -PathType Container)) {
            return (Resolve-Path -LiteralPath $candidate).Path
        }
    }
    return $null
}

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path

# 工具链位置统一由 toolchain-env.ps1 运行时探测（默认根目录 D:\Tools）。
. (Join-Path $PSScriptRoot 'toolchain-env.ps1')
if ($AndroidSdkRoot) { $env:ANDROID_HOME = $AndroidSdkRoot }
if ($JavaHome) { $env:JAVA_HOME = $JavaHome }
$tc = Get-VrToolchain -ToolsRoot $ToolsRoot -RequireAndroid
$androidSdk = $tc.AndroidRoot
$javaHome = $tc.JavaHome
# apksigner.bat / zipalign 依赖 JAVA_HOME，必须在调用前注入。
$env:JAVA_HOME = $javaHome
$env:Path = "$(Join-Path $javaHome 'bin');$env:Path"

$buildToolsVersion = $tc.BuildTools
Write-Host "Android SDK：$androidSdk"
Write-Host "build-tools：$buildToolsVersion"
Write-Host "JDK：$javaHome"

foreach ($requiredFile in @($NativeLibrary, $BaseApk)) {
    if (-not (Test-Path -LiteralPath $requiredFile -PathType Leaf)) { throw "缺少打包输入：$requiredFile" }
}

if ([string]::IsNullOrWhiteSpace($Keystore)) {
    $Keystore = Join-Path $repoRoot 'build\alvr_client_android\debug.keystore'
}
$keystoreDirectory = Split-Path -Parent $Keystore
if (-not (Test-Path -LiteralPath $keystoreDirectory)) {
    New-Item -ItemType Directory -Path $keystoreDirectory -Force | Out-Null
}
if (-not (Test-Path -LiteralPath $Keystore -PathType Leaf)) {
    Write-Host "生成调试签名：$Keystore"
    $keytool = Get-ChildItem -LiteralPath $javaHome -Filter 'keytool.exe' -Recurse -File -ErrorAction SilentlyContinue |
        Select-Object -First 1 -ExpandProperty FullName
    if (-not $keytool) { $keytool = Join-Path $javaHome 'bin\keytool.exe' }
    $previousErrorAction = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        & $keytool -genkeypair -v `
            -keystore $Keystore `
            -storetype JKS `
            -alias $KeystoreAlias `
            -keyalg RSA -keysize 2048 -validity 10950 `
            -storepass $KeystorePassword -keypass $KeystorePassword `
            -dname 'CN=Android Debug,O=HuaweiVRGlass ALVR,C=US' 2>&1 | Out-Null
    }
    finally {
        $ErrorActionPreference = $previousErrorAction
    }
    if (-not (Test-Path -LiteralPath $Keystore -PathType Leaf)) {
        throw "调试签名生成失败：$Keystore"
    }
}

if ([string]::IsNullOrWhiteSpace($OutputApk)) {
    $OutputApk = Join-Path $repoRoot 'build\HuaweiVRGlass-ALVR-Android-repacked.apk'
}
$outputDirectory = Split-Path -Parent $OutputApk
if (-not (Test-Path -LiteralPath $outputDirectory)) {
    New-Item -ItemType Directory -Path $outputDirectory -Force | Out-Null
}

$nativeEntryName = 'lib/armeabi-v7a/libalvr_client_openxr.so'
$workRoot = Join-Path $repoRoot 'build\huawei_repack'
if (-not (Test-Path -LiteralPath $workRoot)) {
    New-Item -ItemType Directory -Path $workRoot -Force | Out-Null
}
$unsignedApk = Join-Path $workRoot 'repacked-unsigned.apk'

Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem

Write-Host "以模板重打包：$BaseApk"
$source = [System.IO.Compression.ZipFile]::OpenRead((Resolve-Path -LiteralPath $BaseApk).Path)
try {
    $stream = [System.IO.File]::Create($unsignedApk)
    try {
        $target = New-Object System.IO.Compression.ZipArchive($stream, [System.IO.Compression.ZipArchiveMode]::Create)
        try {
            foreach ($entry in $source.Entries) {
                # 旧签名会在重打包后失效，必须丢弃后重新签名。
                if ($entry.FullName.StartsWith('META-INF/', [System.StringComparison]::OrdinalIgnoreCase)) { continue }
                if ($entry.FullName -eq $nativeEntryName) { continue }

                $newEntry = $target.CreateEntry($entry.FullName, [System.IO.Compression.CompressionLevel]::NoCompression)
                $readStream = $entry.Open()
                try {
                    $writeStream = $newEntry.Open()
                    try { $readStream.CopyTo($writeStream) }
                    finally { $writeStream.Dispose() }
                }
                finally { $readStream.Dispose() }
            }

            $clientEntry = $target.CreateEntry($nativeEntryName, [System.IO.Compression.CompressionLevel]::NoCompression)
            $clientStream = $clientEntry.Open()
            try {
                $inputStream = [System.IO.File]::OpenRead((Resolve-Path -LiteralPath $NativeLibrary).Path)
                try { $inputStream.CopyTo($clientStream) }
                finally { $inputStream.Dispose() }
            }
            finally { $clientStream.Dispose() }
        }
        finally { $target.Dispose() }
    }
    finally { $stream.Dispose() }
}
finally { $source.Dispose() }

$zipalign = Join-Path $buildToolsVersion 'zipalign.exe'
$apksigner = Join-Path $buildToolsVersion 'apksigner.bat'

Write-Host "对齐：$unsignedApk"
& $zipalign -p -f 4 $unsignedApk $OutputApk

Write-Host "签名：$OutputApk"
& $apksigner sign --ks $Keystore --ks-key-alias $KeystoreAlias `
    --ks-pass "pass:$KeystorePassword" --key-pass "pass:$KeystorePassword" $OutputApk

& $apksigner verify --verbose $OutputApk

Write-Host "完成：$OutputApk"

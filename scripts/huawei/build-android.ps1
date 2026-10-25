[CmdletBinding()]
param(
    [ValidateSet('Debug', 'Release')]
    [string]$Configuration = 'Release',
    [switch]$SkipToolInstall,
    [string]$AndroidSdkRoot,
    [string]$JavaHome
)

$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
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
$projectRoot = (Resolve-Path (Join-Path $repoRoot '..')).Path

# Allow overriding the toolchain location; otherwise probe the usual places on this machine.
$resolvedAndroidSdk = Resolve-FirstExistingPath @(
    $AndroidSdkRoot,
    $env:ANDROID_HOME,
    $env:ANDROID_SDK_ROOT,
    'F:\AndroidSDK',
    (Join-Path $env:LOCALAPPDATA 'Android\Sdk')
)
if (-not $resolvedAndroidSdk) {
    throw "找不到 Android SDK；可用 -AndroidSdkRoot 指定，例如 .\build-android.ps1 -AndroidSdkRoot F:\AndroidSDK"
}

$androidSdk = $resolvedAndroidSdk
$ndkRoot = Join-Path $androidSdk 'ndk\26.1.10909125'
if (-not (Test-Path -LiteralPath $ndkRoot -PathType Container)) {
    $installedNdk = Get-ChildItem -LiteralPath (Join-Path $androidSdk 'ndk') -Directory -ErrorAction SilentlyContinue |
        Sort-Object Name -Descending |
        Select-Object -First 1
    if ($installedNdk) {
        $ndkRoot = $installedNdk.FullName
    }
}

$resolvedJavaHome = Resolve-FirstExistingPath @(
    $JavaHome,
    $env:JAVA_HOME,
    'F:\Download\jdk17\jdk-17.0.17+10',
    (Get-ChildItem -LiteralPath 'F:\Download\jdk17' -Directory -ErrorAction SilentlyContinue |
        Sort-Object Name -Descending | Select-Object -First 1 -ExpandProperty FullName),
    'C:\Program Files\Microsoft\jdk-17.0.17.10-hotspot'
)

if (-not $resolvedJavaHome) {
    throw "找不到 JDK 17；可用 -JavaHome 指定，例如 .\build-android.ps1 -JavaHome F:\Download\jdk17\jdk-17.0.17+10"
}
$javaHome = $resolvedJavaHome

foreach ($requiredPath in @($androidSdk, $ndkRoot, $javaHome)) {
    if (-not (Test-Path -LiteralPath $requiredPath -PathType Container)) {
        throw "缺少 Android 构建依赖：$requiredPath"
    }
}

Write-Host "Android SDK：$androidSdk"
Write-Host "Android NDK：$ndkRoot"
Write-Host "JDK：$javaHome"

$env:ANDROID_HOME = $androidSdk
$env:ANDROID_SDK_ROOT = $androidSdk
$env:ANDROID_NDK_HOME = $ndkRoot
$env:ANDROID_NDK_ROOT = $ndkRoot
$env:JAVA_HOME = $javaHome
$env:Path = "$(Join-Path $javaHome 'bin');$env:Path"

Push-Location $repoRoot
try {
    & (Join-Path $PSScriptRoot 'prepare-huawei-openxr.ps1')

    rustup target add armv7-linux-androideabi

    if (-not (Get-Command cargo-apk -ErrorAction SilentlyContinue)) {
        if ($SkipToolInstall) {
            throw '未安装 cargo-apk；请去掉 -SkipToolInstall 后重试。'
        }
        cargo install --git https://github.com/zarik5/cargo-apk cargo-apk --locked
    }

    $buildArguments = @('xtask', 'build-client')
    if ($Configuration -eq 'Release') {
        $buildArguments += '--release'
    }
    cargo @buildArguments

    $sourceApk = Join-Path $repoRoot 'build\alvr_client_android\alvr_client_android.apk'
    if (-not (Test-Path -LiteralPath $sourceApk -PathType Leaf)) {
        throw "构建结束但未找到 APK：$sourceApk"
    }

    $bridgeJar = Join-Path $projectRoot 'sdk\hvrsdk-openxr-3.5.0.79\sdkDemo\HVRSDK_XrDemo\libs\hvrbridge.jar'
    $expectedBridgeHash = '76DED95D6FAC94ADE69DE24B364FCD78849E181377CDD3BE32267781B3A540F1'
    $actualBridgeHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $bridgeJar).Hash
    if ($actualBridgeHash -ne $expectedBridgeHash) {
        throw "华为 hvrbridge.jar 校验失败：期望 $expectedBridgeHash，实际 $actualBridgeHash"
    }

    $promptAar = Join-Path $projectRoot 'sdk\vrsdk-3.5.0.68\unityPlugin_0930NewPackage\Plugins\Android\hvrprompt.aar'
    $expectedPromptHash = 'C1DAA950A17AD27928E491130EBBAE67DEA426CC1C2495A6C5992D1FFA407F8A'
    $actualPromptHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $promptAar).Hash
    if ($actualPromptHash -ne $expectedPromptHash) {
        throw "华为 hvrprompt.aar 校验失败：期望 $expectedPromptHash，实际 $actualPromptHash"
    }

    $keystore = Join-Path $repoRoot 'build\alvr_client_android\debug.keystore'
    $nativeLibrary = Join-Path $repoRoot 'target\armv7-linux-androideabi\release\libalvr_client_openxr.so'
    $loaderLibrary = Join-Path $repoRoot 'deps\android_openxr\armeabi-v7a\libopenxr_loader_huawei.so'
    & (Join-Path $PSScriptRoot 'package-huawei-activity.ps1') `
        -NativeLibrary $nativeLibrary `
        -LoaderLibrary $loaderLibrary `
        -BridgeJar $bridgeJar `
        -PromptAar $promptAar `
        -OutputApk $sourceApk `
        -AndroidSdk $androidSdk `
        -JavaHome $javaHome `
        -Keystore $keystore

    $artifacts = Join-Path $projectRoot 'artifacts'
    New-Item -ItemType Directory -Path $artifacts -Force | Out-Null
    $destinationApk = Join-Path $artifacts 'HuaweiVRGlass-ALVR-Android-v20.14.1.apk'
    Copy-Item -LiteralPath $sourceApk -Destination $destinationApk -Force

    Write-Host "Android 构建完成：$destinationApk"
    Get-FileHash -Algorithm SHA256 -LiteralPath $destinationApk
}
finally {
    Pop-Location
}

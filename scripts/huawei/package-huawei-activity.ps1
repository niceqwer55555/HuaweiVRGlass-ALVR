[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [string]$NativeLibrary,
    [Parameter(Mandatory)]
    [string]$LoaderLibrary,
    [Parameter(Mandatory)]
    [string]$BridgeJar,
    [Parameter(Mandatory)]
    [string]$PromptAar,
    [Parameter(Mandatory)]
    [string]$OutputApk,
    [Parameter(Mandatory)]
    [string]$AndroidSdk,
    [Parameter(Mandatory)]
    [string]$JavaHome,
    [Parameter(Mandatory)]
    [string]$Keystore,
    # Huawei VR Glass DoF mode declared to the VR service. 6dof requires the Huawei tracking kit,
    # 3dof uses the glasses IMU (rotation only).
    [ValidateSet('3dof', '6dof')]
    [string]$FreeDegree = '6dof',
    # Also rewrite the checked in manifest so every following build uses the same mode.
    [switch]$SyncSourceManifest
)

$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$clientRoot = Join-Path $repoRoot 'alvr\client_openxr'
$buildRoot = Join-Path $repoRoot 'build'
$workRoot = Join-Path $buildRoot 'huawei_activity_package'
$resolvedBuildRoot = [System.IO.Path]::GetFullPath($buildRoot).TrimEnd('\') + '\'
$resolvedWorkRoot = [System.IO.Path]::GetFullPath($workRoot).TrimEnd('\') + '\'

if (-not $resolvedWorkRoot.StartsWith($resolvedBuildRoot, [System.StringComparison]::OrdinalIgnoreCase)) {
    throw "拒绝清理仓库 build 目录以外的路径：$resolvedWorkRoot"
}

foreach ($requiredFile in @($NativeLibrary, $LoaderLibrary, $BridgeJar, $PromptAar, $Keystore)) {
    if (-not (Test-Path -LiteralPath $requiredFile -PathType Leaf)) {
        throw "缺少 Huawei Activity 打包输入：$requiredFile"
    }
}

$buildTools = Join-Path $AndroidSdk 'build-tools\34.0.0'
$androidJar = Join-Path $AndroidSdk 'platforms\android-34\android.jar'
$manifest = Join-Path $clientRoot 'android_huawei\AndroidManifest.xml'
$huaweiAssets = Join-Path $clientRoot 'android_huawei\assets'
$javaSources = @(
    (Join-Path $clientRoot 'android_huawei\src\com\lht\huaweivr\alvr\HuaweiVrActivity.java'),
    (Join-Path $clientRoot 'android_huawei\src\com\lht\huaweivr\alvr\HuaweiVrLauncherActivity.java')
)
$resources = Join-Path $clientRoot 'resources'

foreach ($requiredAsset in @('vr_icon_background.png', 'vr_icon_foreground.png')) {
    $assetPath = Join-Path $huaweiAssets $requiredAsset
    if (-not (Test-Path -LiteralPath $assetPath -PathType Leaf)) {
        throw "缺少 Huawei VR Launcher 双层图标资产：$assetPath"
    }
}

if (Test-Path -LiteralPath $workRoot) {
    Remove-Item -LiteralPath $workRoot -Recurse -Force
}
$classesRoot = Join-Path $workRoot 'classes'
$dexRoot = Join-Path $workRoot 'dex'
$packageRoot = Join-Path $workRoot 'package'
$nativeRoot = Join-Path $packageRoot 'lib\armeabi-v7a'
$manifestBackupRoot = Join-Path $workRoot 'manifest'
New-Item -ItemType Directory -Path $classesRoot, $dexRoot, $nativeRoot, $manifestBackupRoot -Force | Out-Null

# DoF mode is declared to the Huawei VR service with this meta-data entry. Patch a copy of the
# manifest so a normal build keeps using whatever is checked in.
$packagedManifest = Join-Path $manifestBackupRoot 'AndroidManifest.xml'
$manifestText = Get-Content -LiteralPath $manifest -Raw
if ($manifestText -notmatch 'com\.huawei\.vr\.application\.freeDegree') {
    throw "manifest 缺少 com.huawei.vr.application.freeDegree 声明：$manifest"
}
$patchedManifestText = [regex]::Replace(
    $manifestText,
    '(android:value=")(3dof|6dof)("\s*/?>\s*<!--?[^>]*?freeDegree)',
    { param($m) $m.Groups[1].Value + $FreeDegree + $m.Groups[3].Value }
)
$patchedManifestText = [regex]::Replace(
    $patchedManifestText,
    '(com\.huawei\.vr\.application\.freeDegree"\s+android:value=")(3dof|6dof)(")',
    "`${1}$FreeDegree`${3}"
)
if ($patchedManifestText -eq $manifestText) {
    Write-Host "manifest freeDegree 已是 $FreeDegree，无需改写"
} else {
    Write-Host "manifest freeDegree -> $FreeDegree"
}
Set-Content -LiteralPath $packagedManifest -Value $patchedManifestText -NoNewline -Encoding UTF8
if ($SyncSourceManifest) {
    Copy-Item -LiteralPath $manifest "$manifest.bak.$(Get-Date -Format yyyyMMdd_HHmmss)" -Force
    Set-Content -LiteralPath $manifest -Value $patchedManifestText -NoNewline -Encoding UTF8
    Write-Host "已写回仓库 manifest（备份在 $manifest.bak.*）"

    $clientManifest = Join-Path $clientRoot 'Cargo.toml'
    $clientManifestText = Get-Content -LiteralPath $clientManifest -Raw
    $patchedClientManifestText = [regex]::Replace(
        $clientManifestText,
        '(com\.huawei\.vr\.application\.freeDegree"\s*\]\s*\r?\nvalue = ")(3dof|6dof)(")',
        "`${1}$FreeDegree`${3}"
    )
    if ($patchedClientManifestText -ne $clientManifestText) {
        Copy-Item -LiteralPath $clientManifest "$clientManifest.bak.$(Get-Date -Format yyyyMMdd_HHmmss)" -Force
        Set-Content -LiteralPath $clientManifest -Value $patchedClientManifestText -NoNewline -Encoding UTF8
        Write-Host "已同步 Cargo.toml 的 freeDegree -> $FreeDegree"
    }
}
$manifest = $packagedManifest

$promptRoot = Join-Path $workRoot 'hvrprompt'
New-Item -ItemType Directory -Path $promptRoot -Force | Out-Null
Copy-Item -LiteralPath $PromptAar -Destination (Join-Path $promptRoot 'hvrprompt.zip')
Expand-Archive -LiteralPath (Join-Path $promptRoot 'hvrprompt.zip') -DestinationPath $promptRoot -Force
$promptClasses = Join-Path $promptRoot 'classes.jar'

$unsignedApk = Join-Path $workRoot 'alvr-huawei-activity-unsigned.apk'
$alignedApk = Join-Path $workRoot 'alvr-huawei-activity-aligned.apk'
$generatedSources = Join-Path $workRoot 'generated'
New-Item -ItemType Directory -Path $generatedSources -Force | Out-Null
& (Join-Path $buildTools 'aapt.exe') package `
    -f `
    -m `
    -M $manifest `
    -S $resources `
    -S (Join-Path $promptRoot 'res') `
    -A $huaweiAssets `
    -I $androidJar `
    --auto-add-overlay `
    --extra-packages com.huawei.vrlab `
    -J $generatedSources `
    -F $unsignedApk

$generatedJavaSources = @(Get-ChildItem -LiteralPath $generatedSources -Recurse -Filter '*.java' | Select-Object -ExpandProperty FullName)
if ($generatedJavaSources.Count -eq 0) {
    throw 'aapt 未生成 Android 资源绑定源码。'
}

& (Join-Path $JavaHome 'bin\javac.exe') `
    -encoding UTF-8 `
    -source 8 `
    -target 8 `
    -classpath "$androidJar;$BridgeJar;$promptClasses" `
    -d $classesRoot `
    $javaSources `
    $generatedJavaSources

$activityJar = Join-Path $workRoot 'huawei-activity.jar'
& (Join-Path $JavaHome 'bin\jar.exe') cf $activityJar -C $classesRoot .
& (Join-Path $buildTools 'd8.bat') --min-api 26 --output $dexRoot $activityJar $BridgeJar $promptClasses

Copy-Item -LiteralPath $NativeLibrary -Destination (Join-Path $nativeRoot 'libalvr_client_openxr.so')
Copy-Item -LiteralPath $LoaderLibrary -Destination (Join-Path $nativeRoot 'libxr_loader.so')
& (Join-Path $JavaHome 'bin\jar.exe') uf $unsignedApk -C $dexRoot classes.dex -C $packageRoot lib
& (Join-Path $buildTools 'zipalign.exe') -f 4 $unsignedApk $alignedApk
& (Join-Path $buildTools 'apksigner.bat') sign `
    --ks $Keystore `
    --ks-key-alias androiddebugkey `
    --ks-pass pass:alvrclient `
    --key-pass pass:alvrclient `
    $alignedApk

$outputDirectory = Split-Path -Parent $OutputApk
New-Item -ItemType Directory -Path $outputDirectory -Force | Out-Null
Copy-Item -LiteralPath $alignedApk -Destination $OutputApk -Force

& (Join-Path $buildTools 'zipalign.exe') -c -v 4 $OutputApk
& (Join-Path $buildTools 'apksigner.bat') verify --verbose $OutputApk

$packagedAssets = & (Join-Path $buildTools 'aapt.exe') list $OutputApk
foreach ($requiredAsset in @('assets/vr_icon_background.png', 'assets/vr_icon_foreground.png')) {
    if ($requiredAsset -notin $packagedAssets) {
        throw "Huawei VR Launcher 图标资产未写入 APK：$requiredAsset"
    }
}

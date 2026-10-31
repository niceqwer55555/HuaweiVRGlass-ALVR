[CmdletBinding()]
param(
    [string]$NativeLibrary = (Join-Path $PSScriptRoot '..\..\target\armv7-linux-androideabi\release\libalvr_client_openxr.so'),
    [string]$BaseApk = (Join-Path $PSScriptRoot '..\..\build\HuaweiVRGlass_ALVR_Android_v0.5.0_6dof.apk'),
    [string]$OutputDirectory,
    [string]$AndroidSdkRoot,
    [string]$JavaHome,
    [string]$Keystore,
    [string]$KeystoreAlias = 'androiddebugkey',
    [string]$KeystorePassword = 'alvrclient',
    [string]$ToolsRoot
)

<#
    从同一份原生库打包出 3dof / 6dof 两个 APK（可共存安装）。

    华为 VR 框架按应用自己声明的 meta-data 决定要起哪一种会话：

        <meta-data android:name="com.huawei.vr.application.freeDegree" android:value="6dof" />

    "6dof" = 视觉定位套件 + 两只手柄，"3dof" = 眼镜 IMU + 小遥控器。
    曾经试过合并成 "3dof|6dof" 单包，但现役客户端二进制（libalvr_client_openxr.so）
    只会建议 /interaction_profiles/huawei/6dof_controller 这一个 profile，声明改成
    "3dof|6dof" 后 runtime 起的会话与该 profile 不匹配，6DoF 下手柄直接失灵；
    客户端又依赖已丢失的 openxrs-huawei fork，无法重编加运行时判定——所以保持两个包。
    两个包可以共存：3DoF 变体的 applicationId 是 com.lht.huaweivr.alvr.dof3（manifest
    package 属性整串替换），label 加（3DoF）后缀；dex 类名与 JNI 符号不受影响。

    freeDegree / 包名 / label 由 patch_apk_degree.py 改写（字符串池重建）。

    用法：
        .\package-huawei-variants.ps1 -NativeLibrary <libalvr_client_openxr.so>
#>

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
$zipalign = $tc.ZipalignExe
$apksigner = $tc.Apksigner
Write-Host "Android SDK：$androidSdk"
Write-Host "build-tools：$buildToolsVersion"
Write-Host "JDK：$javaHome"

foreach ($requiredFile in @($NativeLibrary, $BaseApk)) {
    if (-not (Test-Path -LiteralPath $requiredFile -PathType Leaf)) { throw "缺少打包输入：$requiredFile" }
}

if ([string]::IsNullOrWhiteSpace($OutputDirectory)) {
    $OutputDirectory = Join-Path $repoRoot 'build'
}
if (-not (Test-Path -LiteralPath $OutputDirectory)) {
    New-Item -ItemType Directory -Path $OutputDirectory -Force | Out-Null
}

if ([string]::IsNullOrWhiteSpace($Keystore)) {
    $Keystore = Join-Path $repoRoot 'build\alvr_client_android\debug.keystore'
}
if (-not (Test-Path -LiteralPath $Keystore -PathType Leaf)) {
    throw "找不到签名：$Keystore（可用 -Keystore / -KeystorePassword 指定）"
}

$workRoot = Join-Path $repoRoot 'build\huawei_variants'
if (-not (Test-Path -LiteralPath $workRoot)) {
    New-Item -ItemType Directory -Path $workRoot -Force | Out-Null
}

# 两个变体：现役客户端只会建议 6dof_controller profile，必须与会话声明一一对应。
$degrees = @('3dof', '6dof')

$stem = [System.IO.Path]::GetFileNameWithoutExtension($BaseApk) -replace '_(3dof6dof|3dof|6dof)$', ''

# patch_apk_degree.py 一次完成：改 freeDegree + 换 .so（保持 stored 不压缩）+ 丢旧签名。
# 不用 .NET ZipArchive 重打包：它会把 .so 压成 deflate，extractNativeLibs=false 时加载失败。
$python = $null
foreach ($candidate in @(
        (Get-Command python3 -ErrorAction SilentlyContinue).Source,
        (Get-Command python -ErrorAction SilentlyContinue).Source,
        (Get-ChildItem (Join-Path $env:USERPROFILE '.workbuddy\binaries\python\versions\*\python.exe') -ErrorAction SilentlyContinue |
            Select-Object -First 1 -ExpandProperty FullName)
    )) {
    if (-not $candidate -or -not (Test-Path -LiteralPath $candidate -PathType Leaf)) { continue }

    # Microsoft Store 的 python.exe 占位 stub 不是有效应用，实测跑一下才知道。
    try {
        & $candidate --version *> $null
        if ($LASTEXITCODE -eq 0) { $python = $candidate; break }
    } catch { }
}
if (-not $python) { throw 'Need a working python on PATH to rewrite AndroidManifest.xml (freeDegree)' }
$patcher = Join-Path $PSScriptRoot 'patch_apk_degree.py'

foreach ($degree in $degrees) {
    $patchedApk = Join-Path $workRoot "$stem`_$degree`_unsigned.apk"
    $alignedApk = Join-Path $workRoot "$stem`_$degree`_aligned.apk"
    $signedApk = Join-Path $OutputDirectory "$stem`_$degree.apk"

    # 3DoF 变体换 applicationId 并改桌面名，与 6DoF 包共存安装。
    # dex 类名与 .so 的 JNI 符号都不动（manifest 里 activity 是全限定名）。
    $renamePackage = $null
    $label = $null
    if ($degree -eq '3dof') {
        $renamePackage = 'com.lht.huaweivr.alvr.dof3'
        $label = '华为 VR Glass 串流（3DoF）'
    }

    Write-Host "改写 freeDegree -> $degree$(if ($renamePackage) { "（包名 $renamePackage，label $label）" })"
    $patchArgs = @(
        $patcher,
        (Resolve-Path -LiteralPath $BaseApk).Path,
        $patchedApk,
        $degree,
        '--native', (Resolve-Path -LiteralPath $NativeLibrary).Path
    )
    if ($renamePackage) { $patchArgs += @('--rename-package', $renamePackage) }
    if ($label) { $patchArgs += @('--label', $label) }
    & $python @patchArgs
    if ($LASTEXITCODE -ne 0) { throw 'patch_apk_degree.py failed' }

    Write-Host "对齐：$alignedApk"
    & $zipalign -p -f 4 $patchedApk $alignedApk
    if ($LASTEXITCODE -ne 0) { throw 'zipalign failed' }

    Write-Host "签名：$signedApk"
    & $apksigner sign --ks $Keystore --ks-key-alias $KeystoreAlias `
        --ks-pass "pass:$KeystorePassword" --key-pass "pass:$KeystorePassword" `
        --out $signedApk $alignedApk
    if ($LASTEXITCODE -ne 0) { throw 'apksigner sign failed' }

    & $apksigner verify $signedApk
    if ($LASTEXITCODE -ne 0) { throw 'apksigner verify failed' }
}

Write-Host "完成：$OutputDirectory"
Get-ChildItem -LiteralPath $OutputDirectory -Filter "$stem`_*.apk"

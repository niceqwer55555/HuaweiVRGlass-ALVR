# 项目长期记忆 — HuaweiVRGlass-ALVR

## 环境固定值
- SteamVR：`D:\Program Files\Steam\steamapps\common\SteamVR`；Steam 客户端 `D:\Program Files\Steam`（D:\SteamLibrary 无 SteamVR）
- 部署目录：`D:\HuaweiVRGlass_ALVR_Windows_v0.5.0`；`build/end/` = 成品包（server/+client/，见其 README）
- dashboard 启动读 exe 同目录 `steamvr_path.txt` 持久化 SteamVR 路径；「上次设置的路径」即上条
- 产物输出：`D:/PY/1/artifacts/`

## 构建/工具链（统一 `F:\Tools`，旧路径全作废；`D:\Tools` 为同内容镜像可切）
- 布局：rust\{cargo,rustup}、msvc MSVC 14.44、Windows Kits 10.0.26100.0、LLVM 18.1.8、AndroidSDK(ndk 26.1.10909125)、nodejs/python/go/mingw64；JDK17 在 `F:\Download\jdk17\jdk-17.0.17+10`
- 构建脚本统一 dot-source `scripts/huawei/toolchain-env.ps1` → `Get-VrToolchain -Validate [-RequireAndroid]` → `$tc.Apply()`；切盘改 `scripts/huawei/toolchain-paths.json` 的 toolsRoot
- **release 构建必须 `CARGO_TARGET_DIR=<repo>\build\target`**（沙箱拦 target\release 写入）；debug/check 正常
- 只改 dashboard/server_io/adb 时仅 `cargo build --release -p alvr_dashboard`（AMF C++ 沙箱必拦，驱动 DLL 沿用旧版）
- 打包：`scripts/huawei/package-huawei-server.ps1`（-Deploy 回写安装目录）
- 禁 `VsDevCmd.bat`（reg.exe 被拦）、禁 `Remove-Item Env:XXX`（用 `[Environment]::SetEnvironmentVariable('X',$null,'Process')`）；`build/end/` 内脚本只准 ASCII

## Android 构建
- SDK `F:\Tools\AndroidSDK`（或 D 盘镜像）；交叉编译 `cargo build -p alvr_client_openxr --target armv7-linux-androideabi --release`（需 ANDROID_NDK_HOME + NDK clang CC；linker 配置在 cargo config.toml）；sdkmanager 非交互须管道喂 "y"

## 客户端 .so（最重要前提）
- **唯一可用客户端 = 原作者 2026-08-15 的 libalvr_client_openxr.so**（67,581,504 B，SHA256 80068d91…02bd8，在 build/HuaweiVRGlass_ALVR_Android_v0.5.0.apk）；本机重编不等价（华为 fork 已丢，deps/openxrs-huawei 是官方占位），坏帧先怀疑编译链
- 华为 SDK 可从成品 APK 恢复（libxr_loader.so SHA256 7D2CD6D4…E1B337）；缺 jar/aar 用 `scripts/huawei/repack-huawei-client.ps1`
- 原版 .so refresh_rates 写死 70Hz；服务端 connection.rs 已改为 preferred_fps 低于声明值时直接采用
- 3DoF/6DoF：会话由 APK manifest `com.huawei.vr.application.freeDegree` 声明；`adb shell setprop debug.alvr.degree 3dof` 可覆盖；变体打包 `scripts/huawei/package-huawei-variants.ps1`（UTF-16LE 等长替换+重签，keystore build/alvr_client_android/debug.keystore）；3DoF 时 rotation-only 手柄锚到头前，否则 SteamVR 菜单掉出视野
- interaction profile：runtime 只认 `/huawei/controller`(3DoF) 与 `/huawei/6dof_controller`；绑定建议有 full/core/minimal 降级；**绝不能同时建议两个 profile**（libvrsdk.so 崩溃）

## GPU 硬编（Windows 无解，勿再试）
- 本机 GPU = Van Gogh DEV_163F（Valve OEM 驱动 31.0.14033.2008）。Valve Windows 驱动无媒体引擎；已实测 Amernime AZ-SoC 驱动能装、无 Code 43 但 **VCN/DXVA/AMF 全不可用** → Windows 唯一可行 = x264 软编；硬编须回 SteamOS（见 steamos/SteamOS迁移手册.md）
- 覆盖 System32 被占用 DLL：先 `mv x.dll x.dll.old_lock` 再落新文件
- 回滚 GPU 驱动：`pnputil /add-driver C:\Windows\System32\DriverStore\FileRepository\u0388790.inf_amd64_*\u0388790.inf /install`（build/end 内旧 restore 脚本已不在）

## 串流拓扑与驱动切换
- 眼镜仅 1 个 USB-C：插 PC = DP 显示器（EDID `RTK2560`，官方有线直出）；插手机 = ALVR WiFi 串流（手机口被占，**adb 有线模式物理不可能**）。两者互斥
- 切换勿动 vrpathreg，改 `D:\Program Files\Steam\config\steamvr.vrsettings` 的 `driver_alvr_server.enable` / `driver_huaweiVR.enable`（先停 SteamVR）；工具 `scripts/huawei/switch-vr-driver.ps1`、自动版 `auto-vr-mode.ps1`（按 EDID 判眼镜插哪）
- 三个 huaweiVR 驱动同名共享 enable 键；`HwVRGlassLite` 那份是 shim（109KB→_real.dll 558KB）
- WiFi ALVR：wired_client_type 须 `{Custom:"com.lht.huaweivr.alvr",variant:"Custom"}`、autolaunch=false（用 `am start -a com.huawei.android.vr.action.MAIN -n com.lht.huaweivr.alvr/.HuaweiVrActivity`）；adb 须在 `bin/platform-tools/adb.exe`
- 服务端 driver 自动选择：`server_io::apply_auto_driver_selection(glasses_on_pc)`，EDID 关键词来自 exe 同目录 `glasses_edid.txt`（缺省 RTK2560/HUAWEI/VRGLASS/VR GLASS）
- 眼镜 USB 数据通道是 CDC ACM（VID_12D1&PID_107E，尚未装通）；`scripts/huawei/glasses-usb.ps1` 查/装驱动；6DoF SLAM 是华为私有不可逆向，当大屏 3DoF 够用

## 游戏库功能（dashboard）
- `game_library.rs`：本地游戏列表存 exe 同目录 `game_library.json`，启动前确保 SteamVR 运行（4s 延迟拉游戏）
- `steam_library.rs`：写 Steam 非 VR 快捷方式（shortcuts.vdf 二进制 VDF 读写，保未知字段，appid=CRC32 高位置 1）
- 独立运行关键：`%LOCALAPPDATA%\openvr\openvrpaths.vrpath` 缺失时 ALVR 驱动注册静默失败；已实现 `server_io::ensure_openvr_paths_file(runtime_root)` 与 vrmanifest 注册（见 2026-10-31 日志）

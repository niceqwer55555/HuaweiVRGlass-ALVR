# 项目长期记忆 — HuaweiVRGlass-ALVR

## SteamVR 路径（用户环境固定值）
- 本机 SteamVR 安装：`D:\Program Files\Steam\steamapps\common\SteamVR`（vrmonitor.exe 在 `bin\win64\vrmonitor.exe`，已实测存在）
- Steam 客户端本体：`D:\Program Files\Steam`；另一 Steam 库：`D:\SteamLibrary`（其中没有 SteamVR）
- 当前部署目录：`D:\HuaweiVRGlass_ALVR_Windows_v0.5.0`；旧版备份 `D:\HuaweiVRGlass_ALVR_Windows_v0.5.0_backup_20261024`
- 手动路径持久化机制：dashboard 启动时 `steamvr_launcher::load_persisted_steamvr_path()` 读取 exe 同目录的 `steamvr_path.txt`（首行纯文本，无引号），UI 里点「应用」也会回写该文件
- 约定：用户提到「上次设置的 SteamVR 路径」即指上面这条，无需再问

## 构建/工具链
- 详见 2026-10-24 日志：Rust 在 `D:/PY/1/toolchain-setup/rust-toolchain`，VS BuildTools 在 `F:/VS2022/BuildTools`，LLVM 在 `F:/LLVM`，SDK 在 C 盘
- 产物输出目录：`D:/PY/1/artifacts/`

## Android 构建环境（本机，2026-10-24 搭起）
- Android SDK root：`F:\AndroidSDK`（platform-tools / platforms;android-30 / build-tools;34.0.0 / ndk;26.1.10909125）
- JDK 17：`F:\Download\jdk17\jdk-17.0.17+10`
- rust target std 是手工装的：`D:\PY\1\toolchain-setup\rust-toolchain\lib\rustlib\armv7-linux-androideabi`（须与 rustc 1.99.1 commit 58cf8f9c3 同源：dist/2026-10-15/rust-std-1.99.1-*.tar.xz）
- linker/ar 配置写在 `D:\PY\1\toolchain-setup\cargo\config.toml` 的 `[target.armv7-linux-androideabi]`
- 交叉编译命令：`cargo build -p alvr_client_openxr --target armv7-linux-androideabi --release`（需 ANDROID_NDK_HOME + CC 指向 NDK clang 包装器）
- sdkmanager 在非交互 host 会挂住，调用时必须用管道输入 "y"

## 华为 SDK 获取方式（重要）
- 原始私有 SDK（hvrbridge.jar / hvrprompt.aar / hvrsdk-openxr-3.5.0.79）本机已不存在，不必再找
- 需要时从成品 APK 恢复：`build\HuaweiVRGlass_ALVR_Android_v0.5.0_6dof.apk` 内 lib/armeabi-v7a/libxr_loader.so
  即为脚本校验期望的那份 loader（SHA256 7D2CD6D4…E1B337），classes.dex / res / AndroidManifest 也已含 hvrbridge 与 hvrprompt 产物
- 缺 jar/aar 时用 `scripts/huawei/repack-huawei-client.ps1`，以既有 APK 为模板换 .so 后 zipalign -p + apksigner

## 【硬编码结论：Windows 侧确认无解，别再试魔改驱动】
- 本机 GPU = **Van Gogh / `PCI\VEN_1002&DEV_163F&SUBSYS_01231002&REV_AE`**，驱动 `31.0.14033.2008`（Valve OEM，`oem3.inf` / `u0388790.inf`，InfSection `ati2mtag_VanGogh`）
- **魔改驱动包的判定标准只有一条：INF 里必须有 `ati2mtag_VanGogh` + `DEV_163F` 的 Models 段。**
  没有就装不上（只会替换音频/GPIO 等无关组件，零收益）。
- 已核验 `WHQL-AMD-Software-Hybrid-Edition-23.11.1-GCNFlex-Nebula-Native-DCH.7z`（Amernime Zone "Nebula" 23.11.1）：
  90 个 INF 全无 `DEV_163F`、全无 `VanGogh` 字符串，支持族仅 Polaris/Vega/Navi/Renoir/Cezanne/Mendocino/DragonRange → **不可用于 Deck**
- 只有 **Amernime Zone 23.3.1 "Remix Discovery Beta"（PVN-SoC APU）** 声明过
  `ati2mtag_VanGogh, DEV_163F, REV_AE/E1/00`；guru3D 实测在 Deck 上装完首次报 Code 43，重启后能起但 **DXVA 不可用**（GPU-Z 显示无 DXVA）→ 仍拿不到硬编
- 把 ROG Ally 驱动（`u0420842.inf`，同 APU 家族）的 Van Gogh 条目改 INF 装 Deck → 装得上但 **Code 43**，`amdkmdag.sys` 内部自检失败，需二进制级改动
- 印证：Valve 官方 Windows 驱动**不含媒体引擎**（编解码授权问题），AMF/MF/VAAPI 三条路在 Windows 都不通
- **唯一支持 Van Gogh 的魔改包位置**：Amernime SourceForge `Release Polaris-Vega-Navi/Release AMD SoC/`
  （根目录的 `PolarisVegaNavi-*` 系列**都不含** 163F）。最新是 `...-23.11.1-PVN-AMDSoC-Nebula-Native-DCH.7z`
- 该包 `MultiParse/*.inf` 的 catalog 是**伪造的微软证书**（`0CC5D26B…`，2039 到期），
  `pnputil` 报 `0xE0000247` / 底层 `0x80096004`，**不开签名强制装不上**；
  可改用 `MSFTParse/u0394729_msft.inf`（真 AMD 签名，改名 `u0394729.inf`）+ 其 cat
  + 用 `WT6A_INF/B397235` 的真实文件填 `B395833` 的 0 字节占位 → 能装成 `oem49.inf`
- **装了也是 Code 43**：`amdkmdag.sys` 对 Deck 硬件自检失败（用户态 atieclxx 正常），
  与 guru3D 社区报告一致；社区称冷重启后能起但 **DXVA 仍不可用**。故 Windows 硬编仍无解
- **【2026-10-25 已完成实测】** 冷重启后 oem49.inf 状态 OK、WDDM 2.7、Feature Level 12_1、
  无 Code 43，**但媒体引擎依然为零**：`h264_amf`/`hevc_amf` 均 `Init() failed error 1`；
  `d3d11va`/`dxva2` 硬解均初始化失败；`AMDh265Encoder` 硬件 MFT 虽注册且 CoCreateInstance=S_OK，
  但 `ActivateObject` 失败（`hevc_mf` 报 could not create MFT）；dxdiag 的 `HardwareMFT` 键为空。
  D3D11 设备本身正常（已排除）。换过与 KMD 配套的 AMF 三件套
  （`amfrt64.dll` 133168 / `amfrtdrv64.dll` 10345624 / `amdenc64.dll` 1238760，来源 `WT6A_INF/B397235`）
  仍无效 → **AZ SoC 驱动只能修 Code 43，拿不到 VCN。Windows 硬编这条路彻底封死**
- 覆盖 System32 里被占用的 DLL：`cp` 会 `Device or resource busy`，须先
  `mv x.dll x.dll.old_lock` 再落新文件
- **回滚已完成**（2026-10-25 11:00 冷重启后确认）：`oem49.inf` 已删，设备绑回 `oem3.inf`
  Valve `31.0.14033.2008`，`Status=OK`；System32 的 AMF 三件套是原始版
  （`amfrt64.dll` 147472 / `amfrtdrv64.dll` 21878800 / `amdenc64.dll` 1258848，
  其中前两个实际版本 32.0.22042.14002，配 Valve KMD 并不严格配套但就是本机原始状态）
- ⚠️ `build/end/` 曾被清空过，曾经的 `driver_backup_20261025/`、`amf_backup_20261025/`、
  `restore-gpu-driver.ps1`、`Test-HwCodec.ps1`、`diagnose-amf.py` **现在都不在了**。
  `scripts/huawei/restore-gpu-driver.ps1` 只是转发到 `build/end/restore-gpu-driver.ps1`
  → **该转发入口已悬空**。再要回滚只能 `pnputil /add-driver <u0388790.inf> /install`，
  Valve 驱动包本体仍在 `C:\Windows\System32\DriverStore\FileRepository\u0388790.inf_amd64_*`
- `build/end/` 现在的定位 = **成品包**（`server/` + `client/`），详见其中 `README.md`
- PowerShell 5.1 按 ANSI 解析无 BOM 的 UTF-8，中文注释会让解析器直接崩
  → `build/end/` 里的 `.ps1` / `.bat` 一律保持 ASCII，不要加中文注释
- 因此 **Windows 侧唯一可行 = CPU 软编（x264）**；要硬编必须回 SteamOS 走 VAAPI（见 `steamos/SteamOS迁移手册.md`）

## 【最重要的前提】唯一可用的客户端二进制 = 原作者 2026-08-15 的 .so
- `build/HuaweiVRGlass_ALVR_Android_v0.5.0.apk` 内 `lib/armeabi-v7a/libalvr_client_openxr.so`
  （67,581,504 B，CRC32 99a093b0，SHA256 80068d91…02bd8，编译于 2026-08-15 12:55）是**已验证能出画面的唯一版本**
- 本机 `deps/openxrs-huawei/{openxr,sys}`、`deps/wgpu-hal-huawei` 都是 **crates.io 官方版占位**，华为私有 fork 已丢失；
  `huawei-loader-linked` / `huawei-openxr-1-0-3` / `huawei-vr-egl` 均为空特性
- 因此**本机重编的 .so 与原版不等价**，症状（xrBeginFrame VALIDATION_FAILURE、swapchain 句柄全 0、
  eglSwapBuffers 永久阻塞）应首先怀疑编译链而非 runtime。重编前不要再沿着「runtime 帧循环坏了」深挖
- 原版 .so + 3dof manifest 的可用包：`build/HuaweiVRGlass_ALVR_Android_v0.5.0_3dof_original.apk`
  （由 `_3dof_aligned.apk` 加签，keystore `build/alvr_client_android/debug.keystore`）
- 原版 .so 里 `refresh_rates` 写死 `vec![70.0]`（lib.rs:496），要 35Hz 必须靠服务端：
  `alvr/server_core/src/connection.rs` 已改为「preferred_fps 低于客户端声明值时直接采用」

## 华为 OpenXR interaction profile
- runtime 只注册两个：`/interaction_profiles/huawei/controller`（3DoF 遥控器/手机）与 `/interaction_profiles/huawei/6dof_controller`
- inputs.rs 里 `interaction_profile!` 宏会拼出 `<vendor>/<name>_controller`，3DoF 那条必须手写常量
- 建议绑定若含 runtime 不支持的路径，整次 suggest 会失败 -> interaction.rs 有 full/core/minimal 降级阶梯
- **绝不能同时建议两个 profile**：session attach 时会在 libvrsdk.so!hxrFindProfileForDevice 空指针崩溃（已 tombstone 证实）

## 会话自由度（3DoF vs 6DoF）判定与变体打包
- 服务端 3DoF（`ThreeDofMode::Auto/On`）：头被钳到 `(0, eye_height_m, 0)`；
  **手柄必须一起处理**，否则只报旋转的手柄留在 y≈0，比眼睛低 1.6m 直接掉出视野，
  连 SteamVR 菜单（挂在手柄上）一起消失。已实现：rotation-only 手柄锚到头前 `(±0.18, -0.28, -0.35)`
- **原版 .so 只认 `/interaction_profiles/huawei/6dof_controller`**（3dof 的 `/huawei/controller` 不存在），
  所以 3dof manifest 下按键必然全失效；要按键就得换 6dof manifest + 服务端 Auto 3DoF 兜位置
- 会话类型由 APK 自己声明：manifest meta-data `com.huawei.vr.application.freeDegree` = `6dof`|`3dof`
- 客户端 `huawei_session.rs` 运行时用 JNI 读回该 meta-data 决定绑定哪个 profile；可用
  `adb shell setprop debug.alvr.degree 3dof`（或 persist.alvr.degree）临时覆盖，无需重打包
- 一个 .so 服务两个变体：`scripts/huawei/package-huawei-variants.ps1` 在二进制 XML 里等长替换
  6dof/3dof（UTF-16LE，8 字节），替换 .so → zipalign -p → apksigner（keystore
  `build\alvr_client_android\debug.keystore`，alias androiddebugkey，密码 alvrclient）
- 交叉编译必须设置 `CARGO_HOME=D:/PY/1/toolchain-setup/cargo`、`RUSTUP_HOME=.../rustup-home`，
  否则会用错 cargo home 而 git 依赖（settings-schema-rs）拉不到；建议加 `--offline`

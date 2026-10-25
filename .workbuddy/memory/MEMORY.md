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

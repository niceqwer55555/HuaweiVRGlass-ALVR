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

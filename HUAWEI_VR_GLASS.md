# Huawei VR Glass ALVR 专版

这是基于 ALVR v20.14.1 开发的非官方 Huawei VR Glass 串流专版，目标设备为连接 Huawei VR Glass 的华为手机，当前真机环境为鸿蒙 4.2（Android 10 基础）。

## 当前状态

- 已完成 Huawei VR SDK Service、旧版 OpenXR Loader 和专用 Activity 接入。
- 已完成 Huawei VR Glass 双眼显示、画面 Y 轴镜像修正与 70 Hz 会话适配。
- 已完成左右 6DoF 手柄按键、摇杆方向和双手独立空间姿态修复。
- 返回 Glass Home 后会停止串流并退出 ALVR 进程。
- PC Dashboard 已加入简体中文界面和中文字体。
- 客户端与服务端不再因 ALVR 版本字符串不同而拒绝连接。

`huawei-vr-glass-v0.1.0` 的稳定基线为 TCP、8 Mbps、70 Hz、768×576/眼、H.264、固定注视点编码关闭。该版本优先保证旧款华为手机软件解码稳定。

`main` 当前开发版会在首个追踪帧强制向 PC 上报 Runtime 的双眼参数，避免 SteamVR 持续使用 ALVR 的占位 FOV。Huawei VR Glass 实测为 63 mm IPD、95° 对称 FOV，Runtime 推荐交换链分辨率为 1552×1552/眼。当前真机测试档为 TCP、14 Mbps、70 Hz、896×896/眼、1552×1552/眼模拟目标、1 帧客户端缓冲、1 帧 PC 待发送队列，并继续关闭固定注视点编码。

Huawei 路径在解码器暂时没有交出新图像时会继续显示上一张完整帧，不再主动清成蓝绿色占位画面。正在显示的 `AHardwareBuffer` 与待显示队列分离；新图像到达时直接选取队列中最新一张并丢弃积压帧，以防止“消除蓝闪”反而增加视频延迟。其他 Android 客户端仍保持上游的解码帧队列行为。

华为旧 Runtime 的合成路径仍会出现 GPU 完成等待和错过显示时限；当前开发版已经把同一视频帧的 `AHardwareBuffer` 改为每帧只导入一次并供左右眼复用。用户已佩戴确认 `896×896/眼、14 Mbps` 档画面清晰且延迟可以接受，该配置作为当前已验收开发基线。

## 构建

环境和脚本说明见 [`scripts/huawei/README.md`](scripts/huawei/README.md)。

```powershell
pwsh -NoProfile -File .\scripts\huawei\build-android.ps1 -Configuration Release -SkipToolInstall
pwsh -NoProfile -File .\scripts\huawei\build-windows.ps1 -Configuration Release
```

Windows 构建默认使用 ALVR `--keep-config` 保留现有 `session.json`；如需有意恢复默认配置，可为脚本传入 `-ResetConfiguration`。
Windows ZIP 同时提供 `应用华为已验收配置.ps1`，用于在新安装或配置重置后一次恢复真机验收的清晰低延迟档。

华为官方 SDK、签名文件、构建产物、设备日志和本机配置不会提交到 Git。构建脚本会从项目外部 SDK 目录读取已校验的 Huawei OpenXR Loader 和 `hvrbridge.jar`。

## 调试连接

VR Glass 会占用手机 USB-C 接口，因此建议先通过 USB 执行 `adb tcpip 5555`，确认无线 ADB 可用后再接入眼镜。多设备状态下应始终使用 `adb -s <手机IP>:5555` 指定目标。

## 上游与许可

本项目基于 [alvr-org/ALVR](https://github.com/alvr-org/ALVR) 修改，不是华为或 ALVR 官方发行版。上游版权与许可文件保持不变；提交改动前请同时遵守 ALVR 与所使用华为 SDK 的许可要求。

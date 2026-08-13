# Huawei VR Glass ALVR 专版

这是基于 ALVR v20.14.1 开发的非官方 Huawei VR Glass 串流专版，目标设备为连接 Huawei VR Glass 的华为手机，当前真机环境为鸿蒙 4.2（Android 10 基础）。

## 当前状态

- 已完成 Huawei VR SDK Service、旧版 OpenXR Loader 和专用 Activity 接入。
- 已完成 Huawei VR Glass 双眼显示、画面 Y 轴镜像修正与 70 Hz 会话适配。
- 已完成左右 6DoF 手柄按键、摇杆方向和双手独立空间姿态修复。
- 返回 Glass Home 后会停止串流并退出 ALVR 进程。
- PC Dashboard 已加入简体中文界面和中文字体。
- 客户端与服务端不再因 ALVR 版本字符串不同而拒绝连接。

当前稳定基线为 TCP、8 Mbps、70 Hz、768×576/眼、H.264、固定注视点编码关闭。该基线优先保证旧款华为手机软件解码稳定；FOV、清晰度和端到端延迟仍在继续优化。

## 构建

环境和脚本说明见 [`scripts/huawei/README.md`](scripts/huawei/README.md)。

```powershell
pwsh -NoProfile -File .\scripts\huawei\build-android.ps1 -Configuration Release -SkipToolInstall
pwsh -NoProfile -File .\scripts\huawei\build-windows.ps1 -Configuration Release -SkipToolInstall
```

华为官方 SDK、签名文件、构建产物、设备日志和本机配置不会提交到 Git。构建脚本会从项目外部 SDK 目录读取已校验的 Huawei OpenXR Loader 和 `hvrbridge.jar`。

## 调试连接

VR Glass 会占用手机 USB-C 接口，因此建议先通过 USB 执行 `adb tcpip 5555`，确认无线 ADB 可用后再接入眼镜。多设备状态下应始终使用 `adb -s <手机IP>:5555` 指定目标。

## 上游与许可

本项目基于 [alvr-org/ALVR](https://github.com/alvr-org/ALVR) 修改，不是华为或 ALVR 官方发行版。上游版权与许可文件保持不变；提交改动前请同时遵守 ALVR 与所使用华为 SDK 的许可要求。

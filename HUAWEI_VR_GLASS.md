# Huawei VR Glass ALVR 专版技术说明

这是基于 ALVR v20.14.1 开发的非官方 Huawei VR Glass 串流专版。目标设备是连接 Huawei VR Glass 的华为手机；当前真机为 HUAWEI LYA-AL00、HarmonyOS 4.2（Android 10 / API 29 基础）、Huawei VR SDK Service 3.5.0.82 和 Huawei 6DoF Gaming Kit。

面向普通用户的下载、安装、截图和排错入口见根目录 [`README.md`](README.md)。本文记录 Huawei Runtime 的技术差异、当前实现和验证边界。

## 当前版本状态

公开稳定版为 [`huawei-vr-glass-v0.5.0`](https://github.com/LHT02/HuaweiVRGlass-ALVR/releases/tag/huawei-vr-glass-v0.5.0)。当前已经完成：

- Huawei VR SDK Service、旧版 OpenXR Loader、官方 2D Prompt 和专用 VR Activity 接入。
- Huawei VR Glass 双眼显示、70 Hz 会话、Y 轴镜像和每眼物理方向适配。
- 左右 6DoF 手柄独立 Pose Action、按键、扳机、握把和振动路径接入。
- HiSilicon H.264 硬件解码、软件回退、上一帧保留和低积压帧队列。
- 解码时间戳与重投影 Pose 的重新匹配。
- Huawei 原始 95° FOV 上报，避免通用倾斜视锥算法扩张到约 102.82°。
- 返回 Glass Home 后停止串流并退出进程。
- 手机端入口、VR 等待界面、中文 PC Dashboard、Huawei VR Launcher/商店双层图标和截图存储权限。
- 客户端与服务端不再仅因 ALVR 版本字符串不同而拒绝连接。

Huawei Runtime 的 6DoF 摇杆轴使用 0～1 范围、中心约为 0.5，而不是 OpenXR 常见的 -1～1、中心为 0。v0.4.0 只对 Y 轴取负；当前开发版已经加入完整中心化、Y 方向修正和圆形死区，具体证据与验证边界见“控制器输入”一节。

## 已验证硬件和 Runtime 参数

| 项目 | 真机结果 |
| --- | --- |
| 手机 | HUAWEI LYA-AL00 |
| 系统 | HarmonyOS 4.2，Android 10 / API 29 |
| 眼镜 USB 设备 | `HUAWEI GLASS CV10`、`HUAWEI GLASS HID` |
| 6DoF | Huawei 6DoF Gaming Kit，SLAM 摄像头和双手柄 |
| 外接显示 | 3200×1600、70 Hz |
| OpenXR 推荐交换链 | 1552×1552/眼 |
| Runtime FOV | 左右眼各 95°×95°，对称投影中心 |
| IPD | 63 mm，眼位约 ±31.5 mm |
| Android ABI | 当前发布包为 ARMv7 |
| 视频路径 | H.264，优先 `OMX.hisi.video.decoder.avc` |

这些值只代表当前设备和 Runtime。其他华为机型、SDK Service 版本或第三方 6DoF 套装需要重新验证。

## 显示、FOV 与重投影

### 眼镜方向

Huawei acquire-bound 帧缓冲的原始方向不能按普通 Android 外接显示处理。专版保留 Runtime 所需的每眼 90°物理方向，并单独修复视频 Y 轴镜像。由 ADB 或系统外接显示抓到的原始双眼图会分别横转 90°，不代表用户在眼镜中看到的方向错误。

README 使用的 SteamVR 截图只对左、右眼图分别做了无损 90° 转正，未交换左右眼、未镜像、未改变 FOV，也没有使用生成式重绘结果。

### 原始 FOV

Huawei Runtime 上报每眼 95°×95° 对称 FOV。ALVR 通用倾斜视锥正交化曾把它扩张为约 102.821°，使投影尺度降低约 12.9%，产生偏广角和斜看立方体顶面变形的感觉。

专版仍正交化双眼局部姿态以满足 OpenVR，但 Huawei 路径直接保留 Runtime 原始 FOV。该修改已经由用户佩戴确认，几何观感达到预期。

### 重投影时间戳

ALVR 会把高精度时间写入 MediaCodec PTS。通用路径使用 `releaseOutputBufferAtTime` 时可保留该时间；HiSilicon 为避免无画面改用立即渲染后，Surface 会重新解释时间单位，导致 `AImage.timestamp` 与 `head_pose_queue` 错位。

当前 Huawei 路径会从 Surface 时间戳反推出原 ALVR PTS，在待输出队列中精确匹配并丢弃已跳过项目；只有厂商未保留对应关系时才回退 FIFO。这样修复了左右反向、上下比例错误、边缘裁剪和随后的一帧抖动。

## 视频解码路径

- H.264 优先选择 HiSilicon `OMX.hisi.video.decoder.avc`。
- HiSilicon 路径立即释放输出帧到 Surface，再由 ImageReader/AHardwareBuffer 进入 OpenGL ES。
- 正在显示的 `AHardwareBuffer` 与待显示队列分离；新图到达时使用最新完整帧并丢弃积压。
- 暂时没有新图时继续显示上一张完整帧，不主动清成蓝绿色占位画面。
- 同一视频帧的 `AHardwareBuffer` 每帧只导入一次并供左右眼复用。
- 软件解码仍可作为兼容回退，但不应在视频稳定时随意改变已经验收的网络、码率和队列参数。

## 控制器输入

### 双手姿态

Huawei Runtime 要求左右手分别创建带单一 subaction path 的 Pose Action 和 ActionSpace。专版为 `/user/hand/left` 与 `/user/hand/right` 各建一套 Grip/Aim 路径，再把两手姿态独立发送给 SteamVR，避免按钮都存在但只有一只手柄有空间位置。

### 摇杆归一化修复

旧版 Runtime 日志在双手柄静置时持续给出非零原始值：

| Runtime 索引 | 样本数 | X 静置值 | Y 静置值 |
| --- | ---: | ---: | ---: |
| 0 | 60 | 0.4921208 | 0.4589786 |
| 1 | 62 | 0.479317～0.484898 | 0.446458～0.452718 |

华为 Unity SDK 同样把 `0.5` 作为摇杆中心：其输入模块使用 `x - 0.5` 和 `-(y - 0.5)`，手柄动画再乘 2 映射到完整范围。这说明静置漂移不是“上下翻转把 0 变成错误值”，而是当前代码在翻转前就漏掉了 Huawei 特有的 0.5 中心化。

专版使用以下归一化：

```text
X = clamp((rawX - 0.5) × 2, -1, 1)
Y = clamp(-(rawY - 0.5) × 2, -1, 1)
```

中心化后应用半径 `0.12` 的圆形死区，并把死区外幅值重新映射到 0～1，既吸收当前设备的中心偏差，也保留完整行程。只要某一轴发生变化，客户端就同时发送同一只手柄归一化后的 X/Y，避免圆形死区使另一个轴的输出变化后仍残留旧值。该算法已有单元测试覆盖中心、四个方向、两只手柄的实测静置样本、对角线和边界钳制；手机当前无电，因此双手静置、四向和满量程仍待恢复供电后佩戴验证。

## 手机、VR 等待界面和生命周期

手机桌面入口提供连接状态、手机局域网 IP、连接步骤和直接启动入口。“连接眼镜并进入 VR”调用 Huawei VR SDK 的官方 `com.huawei.android.vr.PROMPT` 2D 提示页；插入眼镜后由 Huawei VR Launcher 进入专用 VR Activity。

未连接电脑时，眼镜内显示中文等待界面并随连接状态更新。界面直接按 1552×1552/眼栅格化，避免低分辨率中间图二次放大。返回 Glass Home 时，Activity 生命周期会停止串流并结束进程，不在后台持续占用解码器和网络。

Huawei VR Launcher/商店读取 APK 中固定的双层资产：

- `assets/vr_icon_background.png`：由官方 `resources/BG-Gray.svg` 生成。
- `assets/vr_icon_foreground.png`：由官方横版 `resources/ALVR-White.svg` 生成。

这两项不会替换普通 Android `@mipmap/ic_launcher`，也不会改变手机入口或 VR 等待界面。

Android Manifest 申请读写存储权限，手机端首次打开时由 2D 页面请求；不在 VR Activity 中强弹授权框，避免权限 UI 投到眼镜。拒绝后 Huawei VR 系统截图可能无法保存。

## PC 默认配置

当前 `main` 分支将新会话的编解码分辨率和 SteamVR 模拟头显分辨率都设为 Runtime 推荐视图的 `1.0×`。`scripts/huawei/apply-verified-profile.ps1` 也会写入相同模式，并配套：

- H.264；
- 首选 72 Hz；Huawei Runtime 当前只公布 70 Hz，因此实际会话仍协商为 70 Hz；
- 14 Mbps 起步码率；
- TCP；
- 1 帧客户端缓冲；
- 1 帧 PC 待发送队列；
- 固定注视点编码关闭。

比例基准来自客户端通过 OpenXR 上报的 `recommended_image_rect_width/height`，不是物理面板尺寸，也不依赖另一项分辨率设置。当前 Huawei Runtime 上报 1552×1552/眼；ALVR 将协商尺寸向下对齐到 32 像素倍数，所以 `1.0×` 实际得到 1536×1536/眼。设备性能或网络不足时，可把编解码分辨率改为已确认清晰且延迟可接受的绝对 896×896/眼，同时让模拟头显分辨率保持 `1.0×`，以保留正确 FOV 和几何关系。

v0.5.0 的新会话与重置配置使用该比例默认值；升级安装默认保留已有 `session.json`，可运行 Windows 包内的 `应用华为已验收配置.ps1` 主动切换。比例默认值与 72 Hz 首选值已有配置单元测试；1536×1536/眼档此前已正常串流。摇杆归一化和 v0.5.0 发行包仍需手机恢复供电后进行真机验证。

## 构建

环境、SDK 路径、哈希校验和脚本参数见 [`scripts/huawei/README.md`](scripts/huawei/README.md)。

```powershell
pwsh -NoProfile -File .\scripts\huawei\build-android.ps1 -Configuration Release -SkipToolInstall
pwsh -NoProfile -File .\scripts\huawei\build-windows.ps1 -Configuration Release
```

Windows 构建默认用 ALVR `--keep-config` 保留已有 `session.json`。需要有意恢复源码默认值时，向脚本传入 `-ResetConfiguration`。Windows ZIP 同时包含 `应用华为已验收配置.ps1`。

华为官方 SDK、签名文件、构建产物、设备日志和本机配置不提交到 Git。构建脚本从项目外部 SDK 目录读取已校验的 Huawei OpenXR Loader、`hvrbridge.jar`、`hvrprompt.aar` 和兼容 Runtime 库。公开或商业再分发前必须另行确认 Huawei SDK/Runtime 许可。

## ADB 调试

VR Glass 会占用手机 USB-C，推荐先通过 USB 打开无线 ADB：

```powershell
adb tcpip 5555
adb connect <手机局域网IP>:5555
adb -s <手机局域网IP>:5555 shell getprop ro.product.model
```

确认无线 Shell 可用后再把 USB-C 切给眼镜。多设备状态下始终使用 `adb -s <serial>`，避免把安装、日志或截图命令发到错误设备。

当前设备上，眼镜连接时 Huawei 安装器可能拦截或隐藏 ADB 安装确认。安装新版 APK 时优先拔下眼镜，在手机屏幕完成安装和权限授权；重新接入后由用户从 Glass Home 手动启动，不要通过 ADB 强开 VR Activity。

## 官方资料、上游与许可

实现参考华为官方文档：

- [VR Engine 简介](https://developer.huawei.com/consumer/cn/doc/graphics-Guides/introduction-0000001144238265)
- [配置 2D 提示页](https://developer.huawei.com/consumer/cn/doc/development/graphics-Guides/configuration-set-prompt-0000001205644943)
- [Manifest 配置](https://developer.huawei.com/consumer/cn/doc/graphics-Guides/configuration-manifest-0000001205526407)

本项目基于 [alvr-org/ALVR](https://github.com/alvr-org/ALVR) 修改，不是华为或 ALVR 官方发行版。ALVR 可公开部分继续遵守 [MIT License](LICENSE)；华为 SDK、Runtime 和商店资产还可能受额外许可约束。

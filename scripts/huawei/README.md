# ALVR 华为 VR Glass 串流专版

本专版基于 ALVR v20.14.1，加入 HUAWEI VR Glass OpenXR 客户端适配、Windows 简体中文控制台，以及跨版本尝试连接支持。

## 构建

请使用 PowerShell 7：

```powershell
pwsh -NoProfile -File .\scripts\huawei\build-android.ps1 -Configuration Release
pwsh -NoProfile -File .\scripts\huawei\build-windows.ps1 -Configuration Release
```

Android 脚本会校验并复制 HVRSDK OpenXR 3.5.0.79 官方 Demo 发布的应用侧 Loader，把官方 `hvrbridge.jar` 编译为 DEX，并合并 HUAWEI VR SDK 3.5 的官方 `hvrprompt.aar` 2D 连接提示资源，随后重新对齐和签名 APK。客户端会在进入 OpenXR 前调用 `LibUpdateClient.runUpdate()`，由手机已安装的 HUAWEI VR SDK Service 加载配套 Runtime/IPC 和 Java 框架；不再把服务 APK 的私有运行库捆进客户端。华为官方 Demo 与手机实际运行时都以 armeabi-v7a 为生产 ABI；服务 APK 内部 Loader 不作为第三方应用 Loader 使用。

Windows 脚本会自动载入 Visual Studio x64 开发环境、保留现有 `session.json` 并把中文字体许可证一起打包，因此重新编译不会清空受信任手机和已调好的华为串流参数。本机 `session.json`、旧配置和运行/崩溃日志会从发布 ZIP 中排除，不会把设备地址或信任记录带入分发包。只有确实需要恢复全新默认配置时才使用：

```powershell
pwsh -NoProfile -File .\scripts\huawei\build-windows.ps1 -Configuration Release -ResetConfiguration
```

## 安装与连接

1. 解压 Windows ZIP，运行 `ALVR Dashboard.exe`。
2. 在“安装”页注册专版 ALVR 驱动；如存在无效旧驱动，可在同一页面移除。
3. 新安装或配置被重置时，启动 SteamVR，再用 PowerShell 7 执行 `应用华为已验收配置.ps1`，然后从 Dashboard 重启一次 SteamVR。
4. 在华为手机上安装并打开 APK；点击“连接眼镜并进入 VR”进入华为官方连接提示，插入眼镜并完成空间确认。眼镜已经连接时也可以使用“直接启动”。
5. 确保手机和电脑位于同一局域网，在“设备”页信任客户端并启动 SteamVR。

未连接电脑时，眼镜端会显示中文状态等待界面。该界面按 `1552×1552` 原生纹理绘制，使用 62% 内容尺寸；为适配 Huawei VR Glass 的 acquire-bound 帧缓冲，外接显示截图中的左右眼画面保持各旋转 90°，不要根据桌面截图将其转正。

## 跨版本说明

专版不会仅因客户端、Dashboard 或 SteamVR 服务的 ALVR 版本号不同而拒绝连接，也不会因版本标记变化清空客户端配置。协议 ID 不同时会写入警告并继续尝试。

此行为只能取消人为版本门槛，不能让结构完全不同的网络协议自动兼容。若两个版本的实际数据结构差异过大，序列化或反序列化仍会失败并保留错误日志，避免把损坏数据继续传入串流管线。建议优先使用同一专版构建，跨版本能力用于兼容可互通的相邻版本。

## 当前验证状态

- Android armeabi-v7a Release APK：已通过构建、zipalign、APK v2/v3 签名和鸿蒙 4.2 真机安装验证。
- Windows x64 Release：已完成中文专版构建和打包。
- 中文界面：Dashboard 主界面、设置名称、选项、说明、提示和客户端状态页均已覆盖；SteamVR、驱动和游戏输出的原始诊断日志保持原文，便于排错和检索。
- 真机串流：双眼画面、95° FOV、70 Hz、左右手柄独立 6DoF、Home 生命周期和解码 underflow 防蓝闪路径均已接通。新会话与一键配置默认把编解码和模拟头显分辨率设为 Runtime 推荐值 `1.0×`，当前上报 1552×1552/眼、编码对齐后实际为 1536×1536/眼；PC 首选帧率为 72 Hz，Huawei Runtime 只提供 70 Hz 时会自动选择 70 Hz。其余默认值为 TCP、H.264、14 Mbps、1 帧客户端缓冲、1 帧 PC 待发送队列和 FFE 关闭。性能不足时可把编解码分辨率改为已验收的绝对 `896×896/眼` 低延迟档。

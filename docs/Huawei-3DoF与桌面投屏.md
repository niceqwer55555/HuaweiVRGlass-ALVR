# Huawei VR Glass：3DoF 模式与桌面投屏

本文记录本专版新增的两个显示/跟踪模式。`main` 分支默认两者都关闭，行为与之前完全一致。

## 一、3DoF 模式

### 什么是 3DoF

Huawei VR Glass 支持两种跟踪形态：

| 模式 | 跟踪来源 | 能给出的数据 |
| --- | --- | --- |
| 6DoF | Huawei 6DoF Gaming Kit（SLAM 摄像头 + 双手柄） | 头部旋转 + 位置 |
| 3DoF | 眼镜内置 IMU（HVR-DIAG 中的 `HUAWEI GLASS HID`） | 只有旋转 |

模式由 APK 里的 meta-data 声明给 Huawei VR Service：

```xml
<meta-data android:name="com.huawei.vr.application.freeDegree" android:value="6dof" />
```

改成 `3dof` 后，runtime 只提供旋转，适合没插 6DoF 套件、只有眼镜本体，或者想彻底去掉位置抖动/漂移的场景。

### 如何启用

两件事都要做：

1. **打 3DoF 版本的 APK**（决定 runtime 给不给位置）：

```powershell
powershell -ExecutionPolicy Bypass -File "scripts\huawei\package-huawei-activity.ps1" `
    -NativeLibrary <libalvr_client_openxr.so> -LoaderLibrary <libxr_loader.so> `
    -BridgeJar <hvrbridge.jar> -PromptAar <hvrprompt.aar> `
    -OutputApk <out.apk> -AndroidSdk <sdk> -JavaHome <jdk> -Keystore <ks> `
    -FreeDegree 3dof
```

`-FreeDegree` 可接受 `3dof` / `6dof`，默认 `6dof`。脚本只会改写 `build/huawei_activity_package/manifest/` 下的临时副本，不会动仓库里的 `android_huawei\AndroidManifest.xml`；加 `-SyncSourceManifest` 才会写回仓库 manifest，并顺带同步 `alvr/client_openxr/Cargo.toml` 里同名 meta-data（两者都会先备份成 `.bak.<时间戳>`）。

2. **Dashboard 打开开关**：「头显 → 3DoF mode」。

### 双端都做什么

- **客户端**（`alvr/client_openxr/src/stream.rs`）
  `ParsedStreamConfig` 增加 `three_dof` 字段，取值就是 `settings.headset.three_dof`。`poll_stream_inputs` 拿到头部 motion 后立刻把位置压成 `(0, height, 0)`、线速度清零，**角速度保留**，这样重投影还能正常工作。由于 `view_params` 是相对头部位姿算出来的（`motion.pose.inverse() * view pose`），左右眼的 IPD 偏移不受影响。
- **服务端**（`alvr/server_core/src/tracking/mod.rs`）
  `report_device_motions` 在 recenter **之后**对 `HEAD_ID` 做同样处理。这样即使 runtime 仍然上报位置（例如插着套件但用户想要 3DoF），或者某个客户端没做处理，SteamVR 侧看到的依然是纯旋转。手柄/身体跟踪点不受影响，保持 6DoF。

### 参数

| 设置 | 默认 | 说明 |
| --- | --- | --- |
| Eye height | 1.6 m | 视点高度。3DoF 没有位置，不给一个高度的话头会贴在 play space 原点（地面）。坐姿可以调到 1.2 左右。 |
| Prefer runtime height | 开 | runtime 报出的高度 > 0.2 m 时优先用它，更符合实际身高。 |

两个开关都是 `real-time`，改完不用重启 SteamVR。

### 验证

启动后看 `session_log.txt`，正常的 3DoF 会话里头部 `pose.position` 恒为 `(0, height, 0)`，且 SteamVR 里转头正常、走动时不跟随。

## 二、桌面投屏

### 是什么

把 PC 桌面直接串流到眼镜里，显示成眼前一块黑色背景上的平面画面。不需要任何 VR 游戏运行，也不依赖 SteamVR 的合成层内容。

### 如何启用

Dashboard：「头显 → Desktop cast mode」，里面可改：

| 设置 | 默认 | 说明 |
| --- | --- | --- |
| Monitor index | 0 | 按 Windows 枚举顺序的显示器序号，0 通常是主显示器。 |
| Screen size | 0.9 | 桌面占视野的比例。小于 1 时四周留黑边，同时保持桌面原始宽高比。 |
| Stereo separation | 0 px | 左右眼画布的水平偏移量。0 表示两眼同源（像很远的平面），加到 16–32 px 可以让屏幕显得更近。 |
| Capture frame rate | 60 Hz | 桌面采集帧率。 |

投屏属于 `steamvr-restart` 类设置：**改完要重启 SteamVR** 才会生效（服务端 C++ 只在启动时读一次配置）。

### 实现位置

```
alvr/session/src/settings.rs                    CastConfig + HeadsetConfig.cast
alvr/session/src/lib.rs                         OpenvrConfig.cast_* 字段
alvr/server_core/src/connection.rs              把设置写进 OpenvrConfig，发给 C++
alvr/server_openvr/cpp/alvr_server/Settings.*   m_castMode / m_castMonitorIndex / ...
alvr/server_openvr/cpp/platform/win32/DesktopCapture.{h,cpp}   DXGI Output Duplication
alvr/server_openvr/cpp/platform/win32/CEncoder.cpp             编码循环里换成桌面帧
```

流程：

1. `DesktopCapture` 用 `IDXGIOutput1::DuplicateOutput` 抓桌面；每抓到一帧就把它拷进一块**左右眼各一块**的黑色画布中央，画布按 `1 / screen_size` 放大，所以桌面自然居中并带黑边。
2. `CEncoder::Run` 检测到采集可用时，不再等合成器的 `Present()`，而是按 `capture_fps` 自驱；`RenderCastFrame` 用一个「一层两眼」的合成层调用 `FrameRender::RenderFrame`，再交给原来的编码器。这样即使 SteamVR 空闲（没有 VR 应用）也照样出画面。
3. 后续处理（色彩校正、FFR、编码器选择）完全复用老路径，包括我们刚编好的 x264 软编。

### 已知限制

- DXGI 桌面复制在同一时刻只允许一个客户端。开投屏前请关掉远程桌面、OBS 显示器捕获、游戏内录屏等占用者，否则日志会出现 `DuplicateOutput failed`。
- 显示器分辨率切换、插拔显示器后可能需要重启 SteamVR 重新初始化采集。
- 屏幕内容是「填满视野的一块平面」，不是真正的空间曲面；`stereo separation` 只是近似的距离感，不建议设得过大。
- 这台 Steam Deck 的硬件编码器不可用（见 `docs/Windows软件编码说明.md`），投屏走的是 CPU 软编。桌面内容比游戏画面好压缩，`1400×900 ~ 1920×1080`、30 Mbps 左右通常够看；卡顿就先降 `Capture frame rate`。

## 三、回滚

- 3DoF：Dashboard 里关掉即可，不需要重新打包 APK（除非你想让 runtime 层也回到 6DoF）。
- 投屏：关掉并重启 SteamVR，服务端立即回到普通 VR 合成链路。
- `scripts/huawei/fix-steamvr-driver.ps1 -Revert` 用于还原 SteamVR 驱动开关（与本文功能无关，但同源的温度来自同一套配置改动）。

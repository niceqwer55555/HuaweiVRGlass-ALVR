# HwVRDesktop —— 华为 VR Glass 桌面模式（独立渲染版）

不启动 SteamVR，直接在自己这块 3200x1600 的屏上画：左半屏 = 左眼，右半屏 = 右眼。

## 运行

1. 确认眼镜在 Windows 里是**扩展显示器**（分辨率 3200x1600）。
   - 如果 `显示器设置` 里看不到它：拔插一次 USB-C 线（SteamVR 直显退出后这块屏有时不会自动回来），或者戴上让它唤醒。
2. 双击 `HwVRDesktop.exe`。
   - 它会在眼镜那块屏上开一个全屏窗口，其余屏幕不受影响。
   - 找不到眼镜屏时会弹提示并退出。
3. 退出：`Ctrl+Alt+Q`（默认没绑，需要先在 ini 里绑 `exit=`），或直接关进程 / 关掉全屏窗口。

## 默认快捷键（全部可在 `desktop_mode.ini` 的 `[hotkeys]` 里改）

| 键 | 动作 |
| --- | --- |
| Ctrl+Alt+Home | 桌面回中（按当前朝向重新摆到正前方） |
| Ctrl+Alt+方向键 | 桌面平移（可按住连续，越按越快） |
| Ctrl+Alt+= / Ctrl+Alt+- | 放大 / 缩小 |
| Ctrl+Alt+PageUp / PageDown | 弯曲度 增 / 减（0 = 平面，1 = 贴着脸的圆柱） |
| Ctrl+Alt+] / Ctrl+Alt+[ | 距离 远 / 近 |
| Ctrl+Alt+3 | SBS 3D 循环：关 → SBS Half → SBS Full → 上下 |
| Ctrl+Alt+F | 切换 跟随 / 悬停 |
| Ctrl+Alt+H | 显示/隐藏 监控面板 + 快捷键提示 |
| Ctrl+Alt+O / Ctrl+Alt+Shift+O | 桌面透明度 增 / 减 |
| Ctrl+Alt+D | 显示/隐藏 桌面 |
| （默认未绑）bg_cycle | 背景切换：渐变 → 天空 → 全景图 |

> 热键被别的程序占用时，HUD 的快捷键列表里会标"被占用"。

## 两种模式

- **跟随（默认）**：跟着 IMU 走，但**延迟/平滑**地跟（防眩晕）。
  死区内不动（默认 8°）；转头超过死区后按指数平滑追上来（时间常数 tau=0.6s），
  快转头时还有最大角速度限制（90°/s），追上后进入 2° 的滞回阈值停住，不会来回抖。
  参数都在 ini 的 `[mode]` 里：`deadzone / hold / tau / max_speed / pitch_follow`。
- **悬停**：位置和朝向都世界锁定（转头时桌面定在原地）。

## 配置：`desktop_mode.ini`（与 exe 同目录）

关键项：

| 段 | 键 | 说明 |
| --- | --- | --- |
| general | enabled | 1 = 启用桌面模式 |
| display | monitor | 采集哪块屏（0 = 主屏，顺序同 Windows 显示设置） |
| display | distance / scale / curvature / opacity / tilt | 距离(m)/缩放/弯曲/透明度/下倾角 |
| display | ipd | 瞳距 mm（默认 64） |
| display | fov_tan_x / fov_tan_y | 每眼半视角（tan）。1.0 = 90°；觉得画面偏小就往小调（0.8 ≈ 77°） |
| mode | follow | 1 = 跟随，0 = 悬停 |
| background | mode / venv | 0 = 虚拟环境（venv 0 渐变 / 1 天空 / 2 全景图），1 = 摄像头透视（暂未接入） |
| s3d | mode / swap_eyes / parallax | 0 关 / 1 SBS Half / 2 SBS Full / 3 上下 |
| imu | enabled | 1 = 用 IMU 做头动（默认开，启动自动标定零偏） |
| advanced | output | 0 = 无头模式（只渲染到内存，调试用） |
| advanced | dump | >0 = 第 N 帧导出 `frame_dump.bmp`（不戴眼镜也能看渲染对不对） |

改完保存即可，程序每 0.6 秒检查一次文件，会自动重载。

## 踩过的坑（改代码时别踩回去）

- **渲染目标和交换链后备缓冲必须是同一格式**（都 `B8G8R8A8_UNORM`）。不一样时 `CopyResource` 静默失败，`Present` 照样成功、帧率正常、帧转存有图，只有屏是黑的。
- 输出窗口必须处理 `WM_PAINT`（BeginPaint/EndPaint），否则更新区永远不合法，消息循环空转、一帧都不渲染。
- 程序必须是 per-monitor DPI 感知（否则 3200x1600 的窗口被缩成 2133x1067）。
- 这块面板不接受 DXGI 独占全屏（`0x887A0022`），用窗口模式。

## 现状与已知限制

- 桌面镜像、曲面、透明度、SBS、HUD、热键、配置热重载都已可用（已在内存渲染里截图验证）。
- **摄像头透视没接**：`background=1` 现在会回落到天空背景。
- **头动**：6DoF 套件 IMU 已接上（HID `pid_3a09&mi_02`，约 1000 包/秒）。
  加速度在偏移 22/24/26（4096 LSB/g），角速度在 28/30/32（16.4 LSB/(°/s)）。
  陀螺仪有明显零偏（实测 Y 轴 ≈ -100°/s），程序**每次启动会自动标定**（静止采 300 包取均值扣掉），
  日志里会打印"IMU 零偏标定完成"。开机后别马上动眼镜，等这条日志出来再戴。
- **还没有镜头畸变校正**：官方是渲染 4320x4320 的畸变网格再采样（`EnlargeFactor=2.7`）来抵消镜片畸变；我们暂时直出，边缘可能会看着往外扩，后面补一个可调畸变。
- 设置面板还没做进客户端（现在只能改 ini / 用热键）。

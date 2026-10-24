# Windows 侧软件编码（x264/FFmpeg）方案说明

> 适用：Steam Deck **Windows** 环境（Valve Jupiter / AMD Custom APU 0405 / 核显 Van Gogh）
> 目的：在 Windows 上让 ALVR 服务端能把画面编出来——核显硬编走不通，改用 CPU 软编。

---

## 1. 为什么必须是软编

已经穷举验证过的事实（详见工作日志）：

| 通路 | 结果 |
|---|---|
| AMF（`AMFVideoEncoderVCE_AVC`） | `CreateComponent` 成功，`Init` 恒定返回 `AMF_FAIL(1)`；4 个 AMF 运行时版本、2 个 `amdenc64.dll`、AVC/HEVC、任意分辨率全部失败 |
| Media Foundation | `MFTEnumEx` 只枚举到微软**软件**编码器，驱动没有注册任何硬件编码 MFT |
| NVENC | 机器上没有 NVIDIA 显卡 |
| 软编（FFmpeg/x264） | Windows 官方发行版**默认不编译**（`ALVR_GPL` 宏未定义），所以原版一启动就报 `All VideoEncoder are not available` |

结论：Valve 给 Steam Deck 的 Windows 驱动（`31.0.14033.2008`）不提供可用的硬件 H.264 编码服务。
核显的 VCN 编码单元在 Windows 侧没有暴露出来，**只能让 CPU 用 x264 编码**，核显继续负责渲染 SteamVR 画面。

---

## 2. 这次做了什么

1. 引入 FFmpeg 6.1.2（GPL，MSVC x64 共享库）到 `deps/windows/ffmpeg/`（`include` / `lib` / `bin`）。
2. 用 `--features gpl` 重新编译 `alvr_server_openvr`，把 `VideoEncoderSW`（FFmpeg `libx264`，`preset=ultrafast`、`tune=zerolatency`）编进驱动 DLL。
3. 构建脚本：`scripts/huawei/build-driver-gpl.ps1`
   - 不调用 `VsDevCmd.bat`（它会触发 `reg.exe`，被安全策略拦），改为手工设置 `INCLUDE` / `LIB` / `CC`
   - `CARGO_HOME` 指向 `D:\PY\1\toolchain-setup\cargo`，离线编译
   - 产物落到 `build/gpl_driver_stage/`
4. 配置脚本：`scripts/huawei/apply-software-encoding.ps1`（通过 dashboard HTTP API 下发参数）

> 环境坑（已解决，记一下）：`target/` 是从旧路径 `D:\PY\1\HuaweiVRGlass-ALVR-huawei-vr-glass-v0.5.0` 拷贝过来的，
> 里面的 `alvr_filesystem` rlib 把旧 `CARGO_MANIFEST_DIR` 烘焙进去了，导致构建脚本找 `deps/windows/ffmpeg` 时指向旧路径。
> 解决办法：`touch alvr/filesystem/src/lib.rs` 强制重编（或删除该 crate 的产物后重建）。

---

## 3. 已部署位置

驱动 DLL + FFmpeg 运行时已覆盖到三个目录：

```
F:\PY\HuaweiVRGlass-ALVR\build\huawei_windows_package\bin\win64\   ← 测试包
D:\HuaweiVRGlass_ALVR_Windows_v0.5.0\bin\win64\                    ← 已安装目录
F:\PY\HuaweiVRGlass-ALVR\build\alvr_streamer_windows\bin\win64\    ← 构建输出
```

新增/替换的文件：

| 文件 | 说明 |
|---|---|
| `driver_alvr_server.dll` | 14 MB，含 `VideoEncoderSW`（依赖 `avutil-58` / `avcodec-60` / `swscale-7`） |
| `avcodec-60.dll` | 80 MB，FFmpeg 编解码库（内含 libx264） |
| `avutil-58.dll` / `swscale-7.dll` / `swresample-4.dll` | FFmpeg 运行时依赖 |
| `driver_alvr_server.dll.hwonly.bak` | 旧的“只有硬编”版本，回滚用 |

---

## 4. 使用步骤

1. 启动 `ALVR Dashboard.exe`（已安装目录或测试包里的都行）。
2. **先连接头显**（手机端 ALVR → Connected），再启动 SteamVR——顺序反了会再次出现“房间设置失败”。
3. 启动 SteamVR。日志里应当看到：
   ```
   Try to use VideoEncoderAMF.
   AMF Error ...
   Try to use VideoEncoderSW.
   Initializing VideoEncoderSW.
   Successfully initialized VideoEncoderSW
   ```
   只要最后一行出现，就说明软编接管成功。日志文件：安装目录下的 `session_log.txt` / `crash_log.txt`。
4. 参数下发（dashboard 运行时）：
   ```powershell
   powershell -ExecutionPolicy Bypass -File "F:\PY\HuaweiVRGlass-ALVR\scripts\huawei\apply-software-encoding.ps1" -Scale 0.6 -BitrateMbps 20 -Threads 6
   ```
   等价于在 dashboard 里勾选 **视频 → 编码器配置 → 软件（CPU）编码 → 强制软件编码**，并调低分辨率缩放。
5. 改完参数需要**重启 SteamVR** 让驱动重载。

### 推荐起始参数

| 参数 | 建议值 | 说明 |
|---|---|---|
| 编码器 | 强制软件编码 | 跳过 AMF/NVENC 的失败尝试 |
| 编码线程数 | 6 | Deck 是 4C/8T，留 2 个给 SteamVR 主线程 |
| 分辨率缩放 | 0.6（先试） | 0.6 → 约 1920×960 传输，手机端拉伸；卡就降到 0.5，顺可试 0.7–0.8 |
| 帧率 | 72（卡就 60） | 与已验收配置一致 |
| 码率 | 20 Mbps 起 | 软编 + 低缩放不需要太高，画质不够再往上加 |
| 编码格式 | H.264 | 华为客户端只有 HiSilicon AVC 硬解 |
| 协议 | TCP（沿用已验收配置） | |

---

## 5. 性能预期（要有心理准备）

- x264 `ultrafast` + `zerolatency` 在 4 核 Zen2（Deck 大约 15 W 功耗预算）上，**同时还要跑 SteamVR 和游戏**。
- 分辨率缩放 0.6 时大概率能到 60–72 fps；0.8 以上基本会掉帧。
- 画面会偏“软”（低分辨率 + 超快预设），细节不如硬编。
- 核显仍在全力渲染，编码抢的是 CPU 和内存带宽；如果游戏本身吃满 CPU，帧率会一起掉。

如果实测“能连上但卡成 PPT”，那就说明这条路到头了，只能回到 **SteamOS / Linux + VAAPI 硬件编码**（同一块硬件，Linux 驱动里 VCN 编码是完整可用的）。

---

## 6. 回滚

```powershell
# 恢复为只有硬编的旧驱动（会回到 All VideoEncoder are not available 的状态）
Copy-Item 'D:\HuaweiVRGlass_ALVR_Windows_v0.5.0\bin\win64\driver_alvr_server.dll.hwonly.bak' `
          'D:\HuaweiVRGlass_ALVR_Windows_v0.5.0\bin\win64\driver_alvr_server.dll' -Force
```

关掉软编：
```powershell
powershell -ExecutionPolicy Bypass -File "F:\PY\HuaweiVRGlass-ALVR\scripts\huawei\apply-software-encoding.ps1" -Disable
```

---

## 7. 许可提醒

FFmpeg 用的是 **GPL** 构建（为了带 libx264）。自用没问题；如果要对外分发这套 DLL 组合，需要按 GPL 提供相应源码/许可声明。

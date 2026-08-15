# Huawei VR Launcher 双层图标

Huawei VR Launcher 通过 APK `assets` 根目录中的两个固定文件名加载 16:9 双层图标：

- `vr_icon_background.png`：576×324，不透明背景层；
- `vr_icon_foreground.png`：576×324，透明前景层。

本专版的背景直接取仓库根目录 `resources/BG-Gray.svg`，前景直接取
`resources/ALVR-White.svg`（ALVR 官方横版 Logo），不修改普通 Android
`@mipmap/ic_launcher`、眼镜等待界面或 Windows 图标。

生成后的 PNG 位于 `../assets/`。如需重新生成，可安装 Node.js 与 `sharp`，并设置
`HUAWEI_ICON_SHARP_MODULE` 为可加载的 `sharp` 模块目录，然后执行：

```powershell
pwsh -NoProfile -File .\scripts\huawei\generate-vr-launcher-icons.ps1
```

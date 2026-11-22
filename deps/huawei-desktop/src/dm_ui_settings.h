// dm_ui_settings.h —— 桌面配置窗口（热键 + 全部参数）
#pragma once
#include "dm_settings.h"

// 打开/切换配置窗口；applyCb 在用户点“应用”后调用（用来重装热键/重载）
void DmUiToggleSettings(DmSettings* st, void (*applyCb)(void));
bool DmUiSettingsVisible();
void DmUiRegisterHotkey();      // 注册 Ctrl+Alt+S
bool DmUiIsOurHotkey(int id);   // 是不是本模块的热键 id

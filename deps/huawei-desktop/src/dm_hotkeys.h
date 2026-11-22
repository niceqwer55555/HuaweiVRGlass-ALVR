// dm_hotkeys.h —— 全局热键（RegisterHotKey 负责注册/冲突检测，轮询负责长按连续）
#pragma once
#include <windows.h>
#include "dm_settings.h"

struct DmHotkeys {
    DmHotkey hk[DM_A_COUNT];
    bool  ok[DM_A_COUNT] = {false};      // 注册成功
    bool  active[DM_A_COUNT] = {false};  // 处于按住状态
    ULONGLONG holdStart[DM_A_COUNT] = {0};
    ULONGLONG lastFire[DM_A_COUNT] = {0};
    bool  anyRegistered = false;

    // 返回失败数量；failList 里放失败的动作号
    int Install(const DmSettings& st, int* failList, int* failCount);
    void Uninstall();
    int  OnMessage(UINT msg, WPARAM wp);              // 返回动作号，-1 = 无关
    void Tick(ULONGLONG now, void (*fire)(int action, bool fast));
    bool IsDown(int action);
};

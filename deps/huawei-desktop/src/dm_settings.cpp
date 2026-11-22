// dm_settings.cpp —— 桌面模式配置（desktop_mode.ini）与动作/热键表
#include "dm_settings.h"
#include <wctype.h>
#include <wchar.h>

// ---------------- INI 路径 ----------------
std::wstring DmIniPath() {
    wchar_t exe[MAX_PATH] = {0};
    GetModuleFileNameW(NULL, exe, MAX_PATH);
    wchar_t* slash = wcsrchr(exe, L'\\');
    if (slash) *(slash + 1) = 0;
    std::wstring dir = exe;
    std::wstring beside = dir + L"desktop_mode.ini";

    // 安装到不可写目录（如 Program Files）时，配置改放 %LOCALAPPDATA%\HwVRGlassLite
    HANDLE h = CreateFileW(beside.c_str(), GENERIC_WRITE, 0, NULL, OPEN_ALWAYS, FILE_ATTRIBUTE_NORMAL, NULL);
    if (h != INVALID_HANDLE_VALUE) { CloseHandle(h); return beside; }

    wchar_t lad[MAX_PATH] = {0};
    if (GetEnvironmentVariableW(L"LOCALAPPDATA", lad, MAX_PATH) > 0) {
        std::wstring d = std::wstring(lad) + L"\\HwVRGlassLite";
        CreateDirectoryW(d.c_str(), NULL);
        return d + L"\\desktop_mode.ini";
    }
    return beside;
}

// ---------------- 动作表 ----------------
static const DmActionInfo g_actions[DM_A_COUNT] = {
    { L"recenter",      L"桌面回中",          0 },
    { L"pan_up",        L"桌面上移",          1 },
    { L"pan_down",      L"桌面下移",          1 },
    { L"pan_left",      L"桌面左移",          1 },
    { L"pan_right",     L"桌面右移",          1 },
    { L"zoom_in",       L"桌面放大",          1 },
    { L"zoom_out",      L"桌面缩小",          1 },
    { L"curv_up",       L"曲面增强",          1 },
    { L"curv_down",     L"曲面减弱",          1 },
    { L"dist_far",      L"桌面变远",          1 },
    { L"dist_near",     L"桌面变近",          1 },
    { L"s3d_cycle",     L"3D 模式循环切换",   0 },
    { L"toggle_hover",  L"跟随/悬停切换",     0 },
    { L"toggle_hud",    L"监控面板开关",      0 },
    { L"opacity_up",    L"透明度增加",        1 },
    { L"opacity_down",  L"透明度减少",        1 },
    { L"toggle_screen", L"显示/隐藏桌面",     0 },
    { L"bg_cycle",      L"背景切换",          0 },
    { L"rezero",        L"定位重置",          0 },
    { L"imu_source",    L"姿态源切换",        0 },
    { L"eye_roll",      L"两眼朝向循环",      0 },
    { L"lens_mode",     L"畸变 官方/关闭",    0 },
    { L"lens_in",       L"畸变 收",           1 },
    { L"lens_out",      L"畸变 放",           1 },
    { L"zerodof",       L"0DOF 开关",         0 },
    { L"autocal",       L"自动标定偏航轴",    0 },
    { L"exit",          L"退出程序",          0 },
};

const DmActionInfo* DmActionTable() { return g_actions; }

// ---------------- 读取/写入辅助 ----------------
static std::wstring IniGet(const wchar_t* sec, const wchar_t* key, const wchar_t* def, const wchar_t* file) {
    wchar_t buf[2048] = {0};
    GetPrivateProfileStringW(sec, key, def ? def : L"", buf, 2048, file);
    return std::wstring(buf);
}

std::wstring DmReadStr(const wchar_t* sec, const wchar_t* key, const wchar_t* def, const std::wstring& ini) {
    return IniGet(sec, key, def, ini.c_str());
}

float DmReadFloat(const wchar_t* sec, const wchar_t* key, float def, const std::wstring& ini) {
    wchar_t d[64], buf[128] = {0};
    swprintf(d, 64, L"%.6f", def);
    GetPrivateProfileStringW(sec, key, d, buf, 128, ini.c_str());
    return (float)_wtof(buf);
}

int DmReadInt(const wchar_t* sec, const wchar_t* key, int def, const std::wstring& ini) {
    wchar_t d[32], buf[128] = {0};
    swprintf(d, 32, L"%d", def);
    GetPrivateProfileStringW(sec, key, d, buf, 128, ini.c_str());
    return _wtoi(buf);
}

void DmWriteFloat(const wchar_t* sec, const wchar_t* key, float v, const std::wstring& ini) {
    wchar_t buf[64];
    swprintf(buf, 64, L"%.4f", v);
    WritePrivateProfileStringW(sec, key, buf, ini.c_str());
}

void DmWriteInt(const wchar_t* sec, const wchar_t* key, int v, const std::wstring& ini) {
    wchar_t buf[32];
    swprintf(buf, 32, L"%d", v);
    WritePrivateProfileStringW(sec, key, buf, ini.c_str());
}

void DmWriteStr(const wchar_t* sec, const wchar_t* key, const std::wstring& v, const std::wstring& ini) {
    WritePrivateProfileStringW(sec, key, v.c_str(), ini.c_str());
}
// ---------------- 热键文本 <-> 结构 ----------------
struct VkName { const wchar_t* name; int vk; };
static const VkName g_vkNames[] = {
    { L"home", VK_HOME }, { L"end", VK_END }, { L"insert", VK_INSERT }, { L"delete", VK_DELETE },
    { L"up", VK_UP }, { L"down", VK_DOWN }, { L"left", VK_LEFT }, { L"right", VK_RIGHT },
    { L"pageup", VK_PRIOR }, { L"pagedown", VK_NEXT },
    { L"space", VK_SPACE }, { L"tab", VK_TAB }, { L"enter", VK_RETURN }, { L"esc", VK_ESCAPE },
    { L"backspace", VK_BACK }, { L"pause", VK_PAUSE }, { L"capslock", VK_CAPITAL },
    { L"num0", VK_NUMPAD0 }, { L"num1", VK_NUMPAD1 }, { L"num2", VK_NUMPAD2 }, { L"num3", VK_NUMPAD3 },
    { L"num4", VK_NUMPAD4 }, { L"num5", VK_NUMPAD5 }, { L"num6", VK_NUMPAD6 }, { L"num7", VK_NUMPAD7 },
    { L"num8", VK_NUMPAD8 }, { L"num9", VK_NUMPAD9 },
    { L"f1", VK_F1 }, { L"f2", VK_F2 }, { L"f3", VK_F3 }, { L"f4", VK_F4 }, { L"f5", VK_F5 }, { L"f6", VK_F6 },
    { L"f7", VK_F7 }, { L"f8", VK_F8 }, { L"f9", VK_F9 }, { L"f10", VK_F10 }, { L"f11", VK_F11 }, { L"f12", VK_F12 },
};

static std::wstring Lower(std::wstring s) {
    for (size_t i = 0; i < s.size(); i++) s[i] = (wchar_t)towlower(s[i]);
    return s;
}

static int VkFromName(const std::wstring& n) {
    if (n.size() == 1) {
        wchar_t c = n[0];
        if (c >= L'a' && c <= L'z') return (int)towupper(c);
        if (c >= L'0' && c <= L'9') return (int)c;
        if (c == L'=') return VK_OEM_PLUS;
        if (c == L'-') return VK_OEM_MINUS;
        if (c == L';') return VK_OEM_1;
        if (c == L'/') return VK_OEM_2;
        if (c == L'`') return VK_OEM_3;
        if (c == L'[') return VK_OEM_4;
        if (c == L'\\') return VK_OEM_5;
        if (c == L']') return VK_OEM_6;
        if (c == L'\'') return VK_OEM_7;
        if (c == L',') return VK_OEM_COMMA;
        if (c == L'.') return VK_OEM_PERIOD;
    }
    for (size_t i = 0; i < sizeof(g_vkNames) / sizeof(g_vkNames[0]); i++)
        if (n == g_vkNames[i].name) return g_vkNames[i].vk;
    return 0;
}

static std::wstring NameFromVk(int vk) {
    for (size_t i = 0; i < sizeof(g_vkNames) / sizeof(g_vkNames[0]); i++)
        if (vk == g_vkNames[i].vk) return g_vkNames[i].name;
    if (vk >= 'A' && vk <= 'Z') { wchar_t b[2] = { (wchar_t)towlower((wint_t)vk), 0 }; return b; }
    if (vk >= '0' && vk <= '9') { wchar_t b[2] = { (wchar_t)vk, 0 }; return b; }
    switch (vk) {
    case VK_OEM_PLUS: return L"=";
    case VK_OEM_MINUS: return L"-";
    case VK_OEM_1: return L";";
    case VK_OEM_2: return L"/";
    case VK_OEM_3: return L"`";
    case VK_OEM_4: return L"[";
    case VK_OEM_5: return L"\\";
    case VK_OEM_6: return L"]";
    case VK_OEM_7: return L"'";
    case VK_OEM_COMMA: return L",";
    case VK_OEM_PERIOD: return L".";
    }
    return L"";
}

static const wchar_t* kPadNames[] = {
    L"pad-up", L"pad-down", L"pad-left", L"pad-right",
    L"pad-start", L"pad-back", L"pad-ls", L"pad-rs",
    L"pad-lb", L"pad-rb", L"pad-lt", L"pad-rt",
    L"pad-a", L"pad-b", L"pad-x", L"pad-y"
};
std::wstring DmHotkeyToString(const DmHotkey& hk) {
    if (hk.kind == 1) {
        std::wstring s;
        if (hk.mods & DMK_CTRL) s += L"ctrl+";
        if (hk.mods & DMK_ALT)  s += L"alt+";
        if (hk.mods & DMK_SHIFT)s += L"shift+";
        s += L"mouse"; s += (wchar_t)(L'0' + (hk.code < 1 ? 1 : (hk.code > 5 ? 5 : hk.code)));
        return s;
    }
    if (hk.kind == 2) {
        if (hk.code < 0 || hk.code >= 16) return L"";
        return kPadNames[hk.code];
    }
    if (!hk.vk) return L"";
    std::wstring s;
    if (hk.mods & DMK_CTRL) s += L"ctrl+";
    if (hk.mods & DMK_ALT) s += L"alt+";
    if (hk.mods & DMK_SHIFT) s += L"shift+";
    if (hk.mods & DMK_WIN) s += L"win+";
    s += NameFromVk(hk.vk);
    return s;
}

bool DmHotkeyParse(const std::wstring& text, DmHotkey* out) {
    out->vk = 0; out->mods = 0; out->kind = 0; out->code = 0;
    std::wstring t = Lower(text);
    if (t.empty()) return false;
    // 手柄：pad-xxx
    if (t.find(L"pad-") == 0) {
        for (int i = 0; i < 16; i++) if (t == kPadNames[i]) { out->kind = 2; out->code = i; return true; }
        return false;
    }
    // 鼠标：mouse1..mouse5（可带修饰键）
    size_t mp = t.find(L"mouse");
    if (mp != std::wstring::npos && mp + 6 < t.size()) {
        int n = (int)(t[mp + 5] - L'0');
        if (n >= 1 && n <= 5) { out->kind = 1; out->code = n; }
        // 修饰键照常解析
        size_t q = 0;
        while (q < mp) {
            size_t plus = t.find(L'+', q);
            std::wstring part = (plus == std::wstring::npos || plus > mp) ? t.substr(q, mp - q) : t.substr(q, plus - q);
            if (part == L"ctrl" || part == L"control") out->mods |= DMK_CTRL;
            else if (part == L"alt") out->mods |= DMK_ALT;
            else if (part == L"shift") out->mods |= DMK_SHIFT;
            else if (part == L"win") out->mods |= DMK_WIN;
            if (plus == std::wstring::npos || plus > mp) break;
            q = plus + 1;
        }
        return out->kind == 1;
    }
    size_t pos = 0;
    while (pos < t.size()) {
        size_t plus = t.find(L'+', pos);
        std::wstring part = (plus == std::wstring::npos) ? t.substr(pos) : t.substr(pos, plus - pos);
        if (part == L"ctrl" || part == L"control") out->mods |= DMK_CTRL;
        else if (part == L"alt") out->mods |= DMK_ALT;
        else if (part == L"shift") out->mods |= DMK_SHIFT;
        else if (part == L"win") out->mods |= DMK_WIN;
        else if (!part.empty()) out->vk = VkFromName(part);
        if (plus == std::wstring::npos) break;
        pos = plus + 1;
    }
    return out->vk != 0;
}

std::wstring DmHotkeyDisplay(const DmHotkey& hk) {
    if (!hk.vk) return L"未绑定";
    std::wstring s;
    if (hk.mods & DMK_CTRL) s += L"Ctrl+";
    if (hk.mods & DMK_ALT) s += L"Alt+";
    if (hk.mods & DMK_SHIFT) s += L"Shift+";
    if (hk.mods & DMK_WIN) s += L"Win+";
    std::wstring n = NameFromVk(hk.vk);
    if (n.size() == 1) {
        wchar_t c = n[0];
        if (c >= L'a' && c <= L'z') n[0] = c;
    } else if (!n.empty()) {
        n[0] = (wchar_t)towupper(n[0]);
    }
    s += n;
    return s;
}
// ---------------- 默认值 ----------------
void DmSettingsDefault(DmSettings* s) {
    memset(s, 0, sizeof(*s));

    s->version = 1;
    s->enabled = 1;

    // [display]
    s->monitorIndex = 0;
    s->scale = 4.29f;              // 桌面铺满视野（官方：整块面板 = 整个桌面）
    s->curvature = 0.0f;
    s->opacity = 1.0f;
    s->tiltDeg = 0.0f;
    s->keepLevel = 1;
    s->fovBaseDeg = 50.0f;
    s->screenAspect = 1.0f;        // 官方 m_fAspect = 1.0（拉到正方形）
    s->ipdMm = 40.0f;
    s->fovTanX = 1.0f;             // 90°
    s->fovTanY = 1.0f;
    s->rollLeft = 0.0f;
    s->rollRight = 0.0f;
    s->flip180 = 1;
    s->invertPose = 1;
    s->invertYaw = 0;
    s->invertPitch = 0;
    s->invertRoll = 0;
    s->swapPitchRoll = 1;
    s->poseDeadband = 0.0f;   // 0 = 不量化，保留 IMU 原始精度
    s->viewDeadband = 0.25f;  // 旧参数
    s->gyroGateDeg = 0.0f;    // 0 = 不做噪声门，使用真实角速度
    s->autoRezero = 1;        // 拿起后自动归零
    s->oeEnabled = 1;         // 1€ 自适应低通：静止压抖动、转动不拖尾
    s->oeMinCutoff = 1.0f;    // Hz：越小静止越稳（现在只在静止时启用，不会拖慢转头）
    s->oeBeta = 0.05f;        // 度/秒：越大快速转动时延迟越小（0.012 → 0.05 明显更跟手）
    s->oeStillSpeed = 4.0f;   // 度/秒：实测静止时角速度 p99≈2.1、最大 4.07 → 取 4.0
    s->oeMoveCutoff = 15.0f;  // Hz：转动时用轻滤波（τ≈10ms），抹掉 IMU 离散更新造成的卡顿
    s->superres = 1.5f;       // 渲染分辨率倍数（1.0~2.0），默认 1.5×
    s->panX = 0.0f; s->panY = 0.0f;
    s->panoKind = 0;
    s->followRangeDeg = 20.0f;// 跟随触发范围
    s->eyeRot[0] = 270.0f;
    s->eyeRot[1] = 90.0f;          // 与左眼相差 180°（面板+镜片的物理特性）
    s->eyeMirrorX[0] = s->eyeMirrorX[1] = 0;
    s->eyeMirrorY[0] = s->eyeMirrorY[1] = 0;
    s->eyeDx[0] = s->eyeDx[1] = 0.0f;
    s->eyeDy[0] = s->eyeDy[1] = 0.0f;
    s->keepMouseOut = 0;

    // [mode]
    s->simYaw = 0.0f;
    s->simPitch = 0.0f;
    s->zeroDof = 0;              // 默认 3DOF：相机跟头（背景固定在空间），窗口延迟跟随
    s->followMode = 1;
    s->deadZoneDeg = 8.0f;
    s->holdDeg = 2.0f;
    s->tau = 0.6f;
    s->maxFollowDegPerSec = 90.0f;
    s->moveRateDeg = 25.0f;
    s->settleSec = 0.25f;
    s->pitchFollow = 0.6f;
    s->transitionSec = 0.3f;
    s->panStep = 0.05f;
    s->panStepFast = 0.15f;
    s->zoomStep = 1.08f;
    s->curvStep = 0.1f;
    s->distStep = 0.15f;
    s->opacityStep = 0.1f;

    // [background]
    s->bgMode = 1;
    s->venvMode = 1;
    s->panoPath[0] = 0;
    s->bgBrightness = 1.0f;
    s->passthroughMirror = 0;

    // [s3d]
    s->sbsMode = 0;              // 默认 2D：两眼显示同一幅桌面
    s->swapEyes = 0;
    s->parallax = 0.0f;

    // [imu]
    s->imuEnabled = 0;
    s->imuSource = 1;
    s->axisYaw = 0; s->axisPitch = 1; s->axisRoll = 2;
    s->signYaw = 1.0f; s->signPitch = 1.0f; s->signRoll = 0.0f;
    s->imuMap = 0;
    s->gyroScale = 16.4f;
    s->accelScale = 4096.0f;
    s->imuYawOffset = s->imuPitchOffset = s->imuRollOffset = 0.0f;

    // [hud]
    s->hudPanel = 1;
    s->hudPos = 0;
    s->hudKeys = 1;
    s->hudFontPx = 42;        // 字号（HUD 与提示条统一，1.5 倍后）
    s->hudDistance = 1.4f;

    // [advanced]
    s->srgb = 0;
    s->capFpsLimit = 0;
    s->verboseLog = 0;
    s->dumpFrame = 20;
    s->outputEnabled = 1;
    s->vsync = 1;
    s->exclusive = 1;

    // [lens] 官方 HallidayDistortMesh 实测系数
    s->lensSupersample = 2;
    s->alignOverlay = 0;
    s->lensMode = 1;
    s->lensK1 = 0.0f;
    s->lensK2 = 0.0f;
    s->lensK[0] = 0.9989f; s->lensK[1] = 0.3699f; s->lensK[2] = -0.4853f;
    s->lensK[3] = 2.306f;  s->lensK[4] = -3.277f; s->lensK[5] = 1.604f;
    s->lensD = 0.01807f;
    s->lensScale = 1.5165f;
    s->lensZoom = 1.0f;

    // [hotkeys]
    struct { const wchar_t* text; } def[DM_A_COUNT] = {
        { L"ctrl+alt+home" }, { L"ctrl+alt+up" }, { L"ctrl+alt+down" }, { L"ctrl+alt+left" }, { L"ctrl+alt+right" },
        { L"ctrl+alt+=" }, { L"ctrl+alt+-" },
        { L"ctrl+alt+pageup" }, { L"ctrl+alt+pagedown" },
        { L"ctrl+alt+]" }, { L"ctrl+alt+[" },
        { L"ctrl+alt+3" },
        { L"ctrl+alt+f" }, { L"ctrl+alt+h" },
        { L"ctrl+alt+o" }, { L"ctrl+alt+shift+o" },
        { L"ctrl+alt+d" },
        { L"" },
        { L"ctrl+alt+z" }, { L"ctrl+alt+i" }, { L"ctrl+alt+r" },
        { L"ctrl+alt+m" }, { L"ctrl+alt+k" }, { L"ctrl+alt+shift+k" },
        { L"ctrl+alt+0" },
        { L"ctrl+alt+a" },
        { L"ctrl+alt+q" },
    };
    for (int i = 0; i < DM_A_COUNT; i++) DmHotkeyParse(def[i].text, &s->hk[i]);
}
// ---------------- 夹取 ----------------
void DmSettingsClamp(DmSettings* s) {
        if (s->distance < 0.5f) s->distance = 0.5f;
    if (s->distance > 10.0f) s->distance = 10.0f;
    if (s->scale < 1.0f) s->scale = 1.0f;
    if (s->scale > 10.0f) s->scale = 10.0f;
    if (s->curvature < 0.0f) s->curvature = 0.0f;
    if (s->curvature > 50.0f) s->curvature = 50.0f;
    if (s->opacity < 0.1f) s->opacity = 0.1f;
    if (s->opacity > 1.0f) s->opacity = 1.0f;
    if (s->tiltDeg < -15.0f) s->tiltDeg = -15.0f;
    if (s->tiltDeg > 15.0f) s->tiltDeg = 15.0f;
    if (s->fovBaseDeg < 20.0f) s->fovBaseDeg = 20.0f;
    if (s->fovBaseDeg > 110.0f) s->fovBaseDeg = 110.0f;
    if (s->deadZoneDeg < 0.0f) s->deadZoneDeg = 0.0f;
    if (s->deadZoneDeg > 40.0f) s->deadZoneDeg = 40.0f;
    if (s->holdDeg < 0.0f) s->holdDeg = 0.0f;
    if (s->holdDeg > 20.0f) s->holdDeg = 20.0f;
    if (s->tau < 0.1f) s->tau = 0.1f;
    if (s->tau > 3.0f) s->tau = 3.0f;
    if (s->maxFollowDegPerSec < 10.0f) s->maxFollowDegPerSec = 10.0f;
    if (s->maxFollowDegPerSec > 720.0f) s->maxFollowDegPerSec = 720.0f;
    if (s->pitchFollow < 0.0f) s->pitchFollow = 0.0f;
    if (s->pitchFollow > 1.0f) s->pitchFollow = 1.0f;
    if (s->bgBrightness < 0.2f) s->bgBrightness = 0.2f;
    if (s->bgBrightness > 2.0f) s->bgBrightness = 2.0f;
    if (s->fovTanX < 0.05f) s->fovTanX = 0.05f;
    if (s->fovTanY < 0.05f) s->fovTanY = 0.05f;
    if (s->lensSupersample != 2) s->lensSupersample = 1;
    if (s->lensZoom < 0.2f) s->lensZoom = 0.2f;
    if (s->lensZoom > 5.0f) s->lensZoom = 5.0f;
    if (s->lensD < 0.001f) s->lensD = 0.01807f;
}

// ---------------- 载入 ----------------
bool DmSettingsLoad(DmSettings* s) {
    std::wstring ini = DmIniPath();
    DmSettingsDefault(s);

    s->version = DmReadInt(L"general", L"version", s->version, ini);
    s->enabled = DmReadInt(L"general", L"enabled", s->enabled, ini);

    s->monitorIndex = DmReadInt(L"display", L"monitor", s->monitorIndex, ini);
    s->distance = DmReadFloat(L"display", L"distance", s->distance, ini);
    s->scale = DmReadFloat(L"display", L"scale", s->scale, ini);
    s->curvature = DmReadFloat(L"display", L"curvature", s->curvature, ini);
    s->opacity = DmReadFloat(L"display", L"opacity", s->opacity, ini);
    s->tiltDeg = DmReadFloat(L"display", L"tilt", s->tiltDeg, ini);
    s->keepLevel = DmReadInt(L"display", L"keep_level", s->keepLevel, ini);
    s->fovBaseDeg = DmReadFloat(L"display", L"fov_base", s->fovBaseDeg, ini);
    s->screenAspect = DmReadFloat(L"display", L"aspect", s->screenAspect, ini);
    s->ipdMm = DmReadFloat(L"display", L"ipd", s->ipdMm, ini);
    s->fovTanX = DmReadFloat(L"display", L"fov_tan_x", s->fovTanX, ini);
    s->fovTanY = DmReadFloat(L"display", L"fov_tan_y", s->fovTanY, ini);
    s->rollLeft = DmReadFloat(L"display", L"roll_left", s->rollLeft, ini);
    s->rollRight = DmReadFloat(L"display", L"roll_right", s->rollRight, ini);
    s->eyeRot[0] = DmReadFloat(L"display", L"eye_rot_left", s->eyeRot[0], ini);
    s->eyeRot[1] = DmReadFloat(L"display", L"eye_rot_right", s->eyeRot[1], ini);
    s->eyeMirrorX[0] = DmReadInt(L"display", L"eye_mx_left", s->eyeMirrorX[0], ini);
    s->eyeMirrorY[0] = DmReadInt(L"display", L"eye_my_left", s->eyeMirrorY[0], ini);
    s->eyeMirrorX[1] = DmReadInt(L"display", L"eye_mx_right", s->eyeMirrorX[1], ini);
    s->eyeMirrorY[1] = DmReadInt(L"display", L"eye_my_right", s->eyeMirrorY[1], ini);
    s->keepMouseOut = DmReadInt(L"display", L"keep_mouse_out", s->keepMouseOut, ini);
    s->eyeDx[0] = DmReadFloat(L"display", L"eye_dx_left", s->eyeDx[0], ini);
    s->eyeDy[0] = DmReadFloat(L"display", L"eye_dy_left", s->eyeDy[0], ini);
    s->eyeDx[1] = DmReadFloat(L"display", L"eye_dx_right", s->eyeDx[1], ini);
    s->eyeDy[1] = DmReadFloat(L"display", L"eye_dy_right", s->eyeDy[1], ini);
    // [mode]
    s->simYaw = DmReadFloat(L"debug", L"sim_yaw", s->simYaw, ini);
    s->simPitch = DmReadFloat(L"debug", L"sim_pitch", s->simPitch, ini);
    s->zeroDof = DmReadInt(L"mode", L"zerodof", s->zeroDof, ini);
    s->followMode = DmReadInt(L"mode", L"follow", s->followMode, ini);
    s->deadZoneDeg = DmReadFloat(L"mode", L"deadzone", s->deadZoneDeg, ini);
    s->holdDeg = DmReadFloat(L"mode", L"hold", s->holdDeg, ini);
    s->tau = DmReadFloat(L"mode", L"tau", s->tau, ini);
    s->maxFollowDegPerSec = DmReadFloat(L"mode", L"max_speed", s->maxFollowDegPerSec, ini);
    s->moveRateDeg = DmReadFloat(L"mode", L"move_rate", s->moveRateDeg, ini);
    s->settleSec   = DmReadFloat(L"mode", L"settle",    s->settleSec, ini);
    s->flip180     = DmReadInt(L"display", L"flip180", s->flip180, ini);
    s->invertPose  = DmReadInt(L"imu", L"invert_pose", s->invertPose, ini);
    s->invertYaw   = DmReadInt(L"imu", L"invert_yaw", s->invertYaw, ini);
    s->invertPitch = DmReadInt(L"imu", L"invert_pitch", s->invertPitch, ini);
    s->invertRoll  = DmReadInt(L"imu", L"invert_roll", s->invertRoll, ini);
    s->swapPitchRoll = DmReadInt(L"imu", L"swap_pitch_roll", s->swapPitchRoll, ini);
    s->poseDeadband = DmReadFloat(L"imu", L"pose_deadband", s->poseDeadband, ini);
    s->viewDeadband = DmReadFloat(L"display", L"view_deadband", s->viewDeadband, ini);
    s->gyroGateDeg = DmReadFloat(L"imu", L"gyro_gate", s->gyroGateDeg, ini);
    s->autoRezero = DmReadInt(L"imu", L"auto_rezero", s->autoRezero, ini);
    s->oeEnabled = DmReadInt(L"imu", L"oe_enabled", s->oeEnabled, ini);
    s->oeMinCutoff = DmReadFloat(L"imu", L"oe_mincut", s->oeMinCutoff, ini);
    s->oeBeta = DmReadFloat(L"imu", L"oe_beta", s->oeBeta, ini);
    s->oeStillSpeed = DmReadFloat(L"imu", L"oe_still_speed", s->oeStillSpeed, ini);
    s->oeMoveCutoff = DmReadFloat(L"imu", L"oe_move_cutoff", s->oeMoveCutoff, ini);
    s->superres = DmReadFloat(L"display", L"superres", s->superres, ini);
    s->panX = DmReadFloat(L"display", L"pan_x", s->panX, ini);
    s->panY = DmReadFloat(L"display", L"pan_y", s->panY, ini);
    s->panoKind = DmReadInt(L"background", L"pano_kind", s->panoKind, ini);
    s->followRangeDeg = DmReadFloat(L"mode", L"follow_range", s->followRangeDeg, ini);
    s->hudPos = 0;   // 固定桌面下方
    s->pitchFollow = DmReadFloat(L"mode", L"pitch_follow", s->pitchFollow, ini);
    s->transitionSec = DmReadFloat(L"mode", L"transition", s->transitionSec, ini);
    s->panStep = DmReadFloat(L"mode", L"pan_step", s->panStep, ini);
    s->panStepFast = DmReadFloat(L"mode", L"pan_step_fast", s->panStepFast, ini);
    s->zoomStep = DmReadFloat(L"mode", L"zoom_step", s->zoomStep, ini);
    s->curvStep = DmReadFloat(L"mode", L"curv_step", s->curvStep, ini);
    s->distStep = DmReadFloat(L"mode", L"dist_step", s->distStep, ini);
    s->opacityStep = DmReadFloat(L"mode", L"opacity_step", s->opacityStep, ini);

    // [background]
    s->bgMode = DmReadInt(L"background", L"mode", s->bgMode, ini);
    s->venvMode = DmReadInt(L"background", L"venv", s->venvMode, ini);
    {
        std::wstring pano = DmReadStr(L"background", L"panorama", L"", ini);
        wcsncpy(s->panoPath, pano.c_str(), MAX_PATH - 1);
        s->panoPath[MAX_PATH - 1] = 0;
    }
    s->bgBrightness = DmReadFloat(L"background", L"brightness", s->bgBrightness, ini);
    s->passthroughMirror = DmReadInt(L"background", L"pt_mirror", s->passthroughMirror, ini);

    // [s3d]
    s->sbsMode = DmReadInt(L"s3d", L"mode", s->sbsMode, ini);
    s->swapEyes = DmReadInt(L"s3d", L"swap_eyes", s->swapEyes, ini);
    s->parallax = DmReadFloat(L"s3d", L"parallax", s->parallax, ini);

    // [imu]
    s->imuEnabled = DmReadInt(L"imu", L"enabled", s->imuEnabled, ini);
    s->imuSource = DmReadInt(L"imu", L"source", s->imuSource, ini);
    s->axisYaw = DmReadInt(L"imu", L"axis_yaw", s->axisYaw, ini);
    s->axisPitch = DmReadInt(L"imu", L"axis_pitch", s->axisPitch, ini);
    s->axisRoll = DmReadInt(L"imu", L"axis_roll", s->axisRoll, ini);
    s->signYaw = DmReadFloat(L"imu", L"sign_yaw", s->signYaw, ini);
    s->signPitch = DmReadFloat(L"imu", L"sign_pitch", s->signPitch, ini);
    s->signRoll = DmReadFloat(L"imu", L"sign_roll", s->signRoll, ini);
    s->imuMap = DmReadInt(L"imu", L"map", s->imuMap, ini);
    s->gyroScale = DmReadFloat(L"imu", L"gyro_scale", s->gyroScale, ini);
    s->accelScale = DmReadFloat(L"imu", L"accel_scale", s->accelScale, ini);
    s->imuYawOffset = DmReadFloat(L"imu", L"yaw_offset", s->imuYawOffset, ini);
    s->imuPitchOffset = DmReadFloat(L"imu", L"pitch_offset", s->imuPitchOffset, ini);
    s->imuRollOffset = DmReadFloat(L"imu", L"roll_offset", s->imuRollOffset, ini);

    // [hud]
    s->hudPanel = DmReadInt(L"hud", L"panel", s->hudPanel, ini);
    s->hudKeys = DmReadInt(L"hud", L"keys", s->hudKeys, ini);
    s->hudFontPx = DmReadInt(L"hud", L"font", s->hudFontPx, ini);
    s->hudDistance = DmReadFloat(L"hud", L"distance", s->hudDistance, ini);

    // [advanced]
    s->srgb = DmReadInt(L"advanced", L"srgb", s->srgb, ini);
    s->capFpsLimit = DmReadInt(L"advanced", L"cap_fps", s->capFpsLimit, ini);
    s->verboseLog = DmReadInt(L"advanced", L"verbose", s->verboseLog, ini);
    s->dumpFrame = DmReadInt(L"advanced", L"dump", s->dumpFrame, ini);
    s->outputEnabled = DmReadInt(L"advanced", L"output", s->outputEnabled, ini);
    s->vsync = DmReadInt(L"advanced", L"vsync", s->vsync, ini);
    s->exclusive = DmReadInt(L"advanced", L"exclusive", s->exclusive, ini);

    // [lens]
    s->lensSupersample = DmReadInt(L"lens", L"supersample", s->lensSupersample, ini);
    s->alignOverlay = DmReadInt(L"lens", L"align", s->alignOverlay, ini);
    s->lensMode = DmReadInt(L"lens", L"mode", s->lensMode, ini);
    s->lensZoom = DmReadFloat(L"lens", L"zoom", s->lensZoom, ini);
    s->lensD = DmReadFloat(L"lens", L"d", s->lensD, ini);
    s->lensScale = DmReadFloat(L"lens", L"scale", s->lensScale, ini);
    {
        const wchar_t* keys[6] = { L"k0", L"k1", L"k2", L"k3", L"k4", L"k5" };
        for (int i = 0; i < 6; i++) s->lensK[i] = DmReadFloat(L"lens", keys[i], s->lensK[i], ini);
    }

    // [hotkeys]
    //   空值 = 用内置默认（旧版本把整段写成空，导致所有热键失效）
    //   显式写 none/off 才是“关闭该热键”
    for (int i = 0; i < DM_A_COUNT; i++) {
        std::wstring t = DmReadStr(L"hotkeys", g_actions[i].key, L"", ini);
        if (t.empty() && !_wcsicmp(g_actions[i].key, L"s3d_cycle"))   // 旧键名迁移
            t = DmReadStr(L"hotkeys", L"sbs_cycle", L"", ini);
        if (t.empty()) continue;                                      // 保持默认
        if (!_wcsicmp(t.c_str(), L"none") || !_wcsicmp(t.c_str(), L"off")) {
            s->hk[i].vk = 0; s->hk[i].mods = 0; s->hk[i].kind = 0;
            continue;
        }
        DmHotkey hk;
        if (DmHotkeyParse(t, &hk)) s->hk[i] = hk;
        else { s->hk[i].vk = 0; s->hk[i].mods = 0; }
    }

    DmSettingsClamp(s);
    return true;
}
// ---------------- 保存 ----------------
bool DmSettingsSave(const DmSettings* s) {
    std::wstring ini = DmIniPath();

    DmWriteInt(L"general", L"version", s->version, ini);
    DmWriteInt(L"general", L"enabled", s->enabled, ini);

    DmWriteInt(L"display", L"monitor", s->monitorIndex, ini);
    DmWriteFloat(L"display", L"distance", s->distance, ini);
    DmWriteFloat(L"display", L"scale", s->scale, ini);
    DmWriteFloat(L"display", L"curvature", s->curvature, ini);
    DmWriteFloat(L"display", L"opacity", s->opacity, ini);
    DmWriteFloat(L"display", L"tilt", s->tiltDeg, ini);
    DmWriteInt(L"display", L"keep_level", s->keepLevel, ini);
    DmWriteFloat(L"display", L"fov_base", s->fovBaseDeg, ini);
    DmWriteFloat(L"display", L"aspect", s->screenAspect, ini);
    DmWriteFloat(L"display", L"ipd", s->ipdMm, ini);
    DmWriteFloat(L"display", L"fov_tan_x", s->fovTanX, ini);
    DmWriteFloat(L"display", L"fov_tan_y", s->fovTanY, ini);
    DmWriteFloat(L"display", L"roll_left", s->rollLeft, ini);
    DmWriteFloat(L"display", L"roll_right", s->rollRight, ini);
    DmWriteFloat(L"display", L"eye_rot_left", s->eyeRot[0], ini);
    DmWriteFloat(L"display", L"eye_rot_right", s->eyeRot[1], ini);
    DmWriteInt(L"display", L"eye_mx_left", s->eyeMirrorX[0], ini);
    DmWriteInt(L"display", L"eye_my_left", s->eyeMirrorY[0], ini);
    DmWriteInt(L"display", L"eye_mx_right", s->eyeMirrorX[1], ini);
    DmWriteInt(L"display", L"eye_my_right", s->eyeMirrorY[1], ini);
    DmWriteInt(L"display", L"keep_mouse_out", s->keepMouseOut, ini);
    DmWriteFloat(L"display", L"eye_dx_left", s->eyeDx[0], ini);
    DmWriteFloat(L"display", L"eye_dy_left", s->eyeDy[0], ini);
    DmWriteFloat(L"display", L"eye_dx_right", s->eyeDx[1], ini);
    DmWriteFloat(L"display", L"eye_dy_right", s->eyeDy[1], ini);

    DmWriteFloat(L"debug", L"sim_yaw", s->simYaw, ini);
    DmWriteFloat(L"debug", L"sim_pitch", s->simPitch, ini);
    DmWriteInt(L"mode", L"zerodof", s->zeroDof, ini);
    DmWriteInt(L"mode", L"follow", s->followMode, ini);
    DmWriteFloat(L"mode", L"deadzone", s->deadZoneDeg, ini);
    DmWriteFloat(L"mode", L"hold", s->holdDeg, ini);
    DmWriteFloat(L"mode", L"tau", s->tau, ini);
    DmWriteFloat(L"mode", L"max_speed", s->maxFollowDegPerSec, ini);
    DmWriteFloat(L"mode", L"move_rate", s->moveRateDeg, ini);
    DmWriteFloat(L"mode", L"settle", s->settleSec, ini);
    DmWriteInt(L"display", L"flip180", s->flip180, ini);
    DmWriteInt(L"imu", L"invert_pose", s->invertPose, ini);
    DmWriteInt(L"imu", L"invert_yaw", s->invertYaw, ini);
    DmWriteInt(L"imu", L"invert_pitch", s->invertPitch, ini);
    DmWriteInt(L"imu", L"invert_roll", s->invertRoll, ini);
    DmWriteInt(L"imu", L"swap_pitch_roll", s->swapPitchRoll, ini);
    DmWriteFloat(L"imu", L"pose_deadband", s->poseDeadband, ini);
    DmWriteFloat(L"display", L"view_deadband", s->viewDeadband, ini);
    DmWriteFloat(L"imu", L"gyro_gate", s->gyroGateDeg, ini);
    DmWriteInt(L"imu", L"auto_rezero", s->autoRezero, ini);
    DmWriteInt(L"imu", L"oe_enabled", s->oeEnabled, ini);
    DmWriteFloat(L"imu", L"oe_mincut", s->oeMinCutoff, ini);
    DmWriteFloat(L"imu", L"oe_beta", s->oeBeta, ini);
    DmWriteFloat(L"imu", L"oe_still_speed", s->oeStillSpeed, ini);
    DmWriteFloat(L"imu", L"oe_move_cutoff", s->oeMoveCutoff, ini);
    DmWriteFloat(L"display", L"superres", s->superres, ini);
    DmWriteFloat(L"display", L"pan_x", s->panX, ini);
    DmWriteFloat(L"display", L"pan_y", s->panY, ini);
    DmWriteInt(L"background", L"pano_kind", s->panoKind, ini);
    DmWriteFloat(L"mode", L"follow_range", s->followRangeDeg, ini);
    DmWriteInt(L"hud", L"pos", s->hudPos, ini);
    DmWriteFloat(L"mode", L"pitch_follow", s->pitchFollow, ini);
    DmWriteFloat(L"mode", L"transition", s->transitionSec, ini);
    DmWriteFloat(L"mode", L"pan_step", s->panStep, ini);
    DmWriteFloat(L"mode", L"pan_step_fast", s->panStepFast, ini);
    DmWriteFloat(L"mode", L"zoom_step", s->zoomStep, ini);
    DmWriteFloat(L"mode", L"curv_step", s->curvStep, ini);
    DmWriteFloat(L"mode", L"dist_step", s->distStep, ini);
    DmWriteFloat(L"mode", L"opacity_step", s->opacityStep, ini);

    DmWriteInt(L"background", L"mode", s->bgMode, ini);
    DmWriteInt(L"background", L"venv", s->venvMode, ini);
    DmWriteStr(L"background", L"panorama", std::wstring(s->panoPath), ini);
    DmWriteFloat(L"background", L"brightness", s->bgBrightness, ini);
    DmWriteInt(L"background", L"pt_mirror", s->passthroughMirror, ini);

    DmWriteInt(L"s3d", L"mode", s->sbsMode, ini);
    DmWriteInt(L"s3d", L"swap_eyes", s->swapEyes, ini);
    DmWriteFloat(L"s3d", L"parallax", s->parallax, ini);

    DmWriteInt(L"imu", L"enabled", s->imuEnabled, ini);
    DmWriteInt(L"imu", L"source", s->imuSource, ini);
    DmWriteInt(L"imu", L"axis_yaw", s->axisYaw, ini);
    DmWriteInt(L"imu", L"axis_pitch", s->axisPitch, ini);
    DmWriteInt(L"imu", L"axis_roll", s->axisRoll, ini);
    DmWriteFloat(L"imu", L"sign_yaw", s->signYaw, ini);
    DmWriteFloat(L"imu", L"sign_pitch", s->signPitch, ini);
    DmWriteFloat(L"imu", L"sign_roll", s->signRoll, ini);
    DmWriteInt(L"imu", L"map", s->imuMap, ini);
    DmWriteFloat(L"imu", L"gyro_scale", s->gyroScale, ini);
    DmWriteFloat(L"imu", L"accel_scale", s->accelScale, ini);
    DmWriteFloat(L"imu", L"yaw_offset", s->imuYawOffset, ini);
    DmWriteFloat(L"imu", L"pitch_offset", s->imuPitchOffset, ini);
    DmWriteFloat(L"imu", L"roll_offset", s->imuRollOffset, ini);

    DmWriteInt(L"hud", L"panel", s->hudPanel, ini);
    DmWriteInt(L"hud", L"keys", s->hudKeys, ini);
    DmWriteInt(L"hud", L"font", s->hudFontPx, ini);
    DmWriteFloat(L"hud", L"distance", s->hudDistance, ini);

    DmWriteInt(L"advanced", L"srgb", s->srgb, ini);
    DmWriteInt(L"advanced", L"cap_fps", s->capFpsLimit, ini);
    DmWriteInt(L"advanced", L"verbose", s->verboseLog, ini);
    DmWriteInt(L"advanced", L"dump", s->dumpFrame, ini);
    DmWriteInt(L"advanced", L"output", s->outputEnabled, ini);
    DmWriteInt(L"advanced", L"vsync", s->vsync, ini);
    DmWriteInt(L"advanced", L"exclusive", s->exclusive, ini);

    DmWriteInt(L"lens", L"supersample", s->lensSupersample, ini);
    DmWriteInt(L"lens", L"align", s->alignOverlay, ini);
    DmWriteInt(L"lens", L"mode", s->lensMode, ini);
    DmWriteFloat(L"lens", L"zoom", s->lensZoom, ini);
    DmWriteFloat(L"lens", L"d", s->lensD, ini);
    DmWriteFloat(L"lens", L"scale", s->lensScale, ini);
    {
        const wchar_t* keys[6] = { L"k0", L"k1", L"k2", L"k3", L"k4", L"k5" };
        for (int i = 0; i < 6; i++) DmWriteFloat(L"lens", keys[i], s->lensK[i], ini);
    }

    for (int i = 0; i < DM_A_COUNT; i++)
        DmWriteStr(L"hotkeys", g_actions[i].key, DmHotkeyToString(s->hk[i]), ini);

    return true;
}

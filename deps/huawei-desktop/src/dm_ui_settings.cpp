// dm_ui_settings.cpp —— 桌面配置窗口
#include "dm_ui_settings.h"
#include "dm_log.h"
#include <commctrl.h>
#include <string>
#include <vector>

static HWND g_wnd = NULL;
static DmSettings* g_st = NULL;
static void (*g_apply)(void) = NULL;
static int g_capture = -1;          // 正在捕获热键的动作索引
static HBRUSH g_bg = NULL;
static HFONT g_font = NULL;

static const int kUiHotkeyId = 0xD4F0;
enum { ID_APPLY = 1001, ID_RELOAD, ID_RESET, ID_CLOSE, ID_HINT, ID_HOTKEY_BASE = 2000, ID_FIELD_BASE = 3000 };

// ---------------- 数值字段表 ----------------
struct FieldDef { const wchar_t* label; int kind; float* f; int* iv; float minv, maxv; };
enum { FK_FLOAT = 0, FK_INT = 1, FK_BOOL = 2 };

static std::vector<FieldDef> g_fields;

static void BuildFields(DmSettings* s) {
    g_fields.clear();
    auto F = [&](const wchar_t* n, float* p, float mn, float mx) { g_fields.push_back({ n, FK_FLOAT, p, NULL, mn, mx }); };
    auto I = [&](const wchar_t* n, int* p, float mn, float mx) { g_fields.push_back({ n, FK_INT, NULL, p, mn, mx }); };
    auto B = [&](const wchar_t* n, int* p) { g_fields.push_back({ n, FK_BOOL, NULL, p, 0, 1 }); };

    F(L"距离 distance (m)", &s->distance, 0.6f, 10.0f);
    F(L"尺寸 scale", &s->scale, 0.3f, 8.0f);
    F(L"曲面 curvature (0-5)", &s->curvature, 0.0f, 50.0f);
    F(L"透明度 opacity", &s->opacity, 0.1f, 1.0f);
    F(L"下倾 tilt", &s->tiltDeg, -15.0f, 15.0f);
    F(L"基准视场 fov_base", &s->fovBaseDeg, 20.0f, 110.0f);
    F(L"屏宽高比 aspect(0=自动)", &s->screenAspect, 0.0f, 4.0f);
    F(L"瞳距 ipd (mm)", &s->ipdMm, 0.0f, 90.0f);
    F(L"每眼半视角 X (tan)", &s->fovTanX, 0.2f, 3.0f);
    F(L"每眼半视角 Y (tan)", &s->fovTanY, 0.2f, 3.0f);
    B(L"跟随头部（关=悬停）", &s->followMode);
    F(L"死区 deadzone (°)", &s->deadZoneDeg, 0.0f, 40.0f);
    F(L"滞回 hold (°)", &s->holdDeg, 0.0f, 20.0f);
    F(L"平滑 tau (s)", &s->tau, 0.05f, 3.0f);
    F(L"最大角速度 (°/s)", &s->maxFollowDegPerSec, 10.0f, 720.0f);
    F(L"俯仰跟随系数", &s->pitchFollow, 0.0f, 1.0f);
    I(L"背景模式 0环境/1透视", &s->bgMode, 0, 1);
    I(L"虚拟环境 0渐变/1天空/2全景", &s->venvMode, 0, 2);
    F(L"背景亮度", &s->bgBrightness, 0.2f, 2.0f);
    I(L"SBS 3D 0关/1半宽/2全宽/3上下", &s->sbsMode, 0, 3);
    B(L"交换左右眼", &s->swapEyes);
    F(L"立体视差 parallax", &s->parallax, -0.05f, 0.05f);
    I(L"畸变 0关/1官方多项式", &s->lensMode, 0, 1);
    F(L"畸变 zoom", &s->lensZoom, 0.2f, 5.0f);
    F(L"畸变 d (米)", &s->lensD, 0.001f, 0.05f);
    F(L"畸变 scale", &s->lensScale, 0.2f, 5.0f);
    I(L"超采样 1/2", &s->lensSupersample, 1, 2);
    F(L"调试 模拟偏航 sim_yaw", &s->simYaw, -180.0f, 180.0f);
    F(L"调试 模拟俯仰 sim_pitch", &s->simPitch, -90.0f, 90.0f);
}
// ---------------- 控件创建 ----------------
static void CreateControls(HWND h) {
    const int rowH = 26;
    const int lx = 12, lw = 210, ex = 226, ew = 96;
    g_font = CreateFontW(-16, 0, 0, 0, FW_NORMAL, 0, 0, 0, DEFAULT_CHARSET, 0, 0, CLEARTYPE_QUALITY, 0, L"Microsoft YaHei UI");

    for (size_t i = 0; i < g_fields.size(); i++) {
        int y = 10 + (int)i * rowH;
        HWND lbl = CreateWindowExW(0, L"STATIC", g_fields[i].label, WS_CHILD | WS_VISIBLE | SS_LEFT,
                                   lx, y + 4, lw, 20, h, NULL, NULL, NULL);
        SendMessageW(lbl, WM_SETFONT, (WPARAM)g_font, TRUE);
        if (g_fields[i].kind == FK_BOOL) {
            HWND cb = CreateWindowExW(0, L"BUTTON", L"", WS_CHILD | WS_VISIBLE | BS_AUTOCHECKBOX,
                                      ex, y + 2, 40, 22, h, (HMENU)(INT_PTR)(ID_FIELD_BASE + i), NULL, NULL);
            SendMessageW(cb, WM_SETFONT, (WPARAM)g_font, TRUE);
        } else {
            wchar_t buf[64];
            if (g_fields[i].kind == FK_FLOAT) swprintf(buf, 64, L"%.4f", *g_fields[i].f);
            else swprintf(buf, 64, L"%d", *g_fields[i].iv);
            HWND ed = CreateWindowExW(WS_EX_CLIENTEDGE, L"EDIT", buf, WS_CHILD | WS_VISIBLE | ES_AUTOHSCROLL,
                                      ex, y + 2, ew, 22, h, (HMENU)(INT_PTR)(ID_FIELD_BASE + i), NULL, NULL);
            SendMessageW(ed, WM_SETFONT, (WPARAM)g_font, TRUE);
        }
    }

    const DmActionInfo* tbl = DmActionTable();
    int rx = 500, rlw = 180, rbw = 190;
    for (int i = 0; i < DM_A_COUNT; i++) {
        int y = 10 + i * rowH;
        HWND lbl = CreateWindowExW(0, L"STATIC", tbl[i].label, WS_CHILD | WS_VISIBLE | SS_LEFT,
                                   rx, y + 4, rlw, 20, h, NULL, NULL, NULL);
        SendMessageW(lbl, WM_SETFONT, (WPARAM)g_font, TRUE);
        std::wstring txt = DmHotkeyDisplay(g_st->hk[i]);
        HWND b = CreateWindowExW(0, L"BUTTON", txt.c_str(), WS_CHILD | WS_VISIBLE | BS_PUSHBUTTON,
                                 rx + rlw + 8, y + 1, rbw, 23, h, (HMENU)(INT_PTR)(ID_HOTKEY_BASE + i), NULL, NULL);
        SendMessageW(b, WM_SETFONT, (WPARAM)g_font, TRUE);
    }

    struct { int id; const wchar_t* text; } btns[] = {
        { ID_APPLY, L"应用并保存" }, { ID_RELOAD, L"重新载入" }, { ID_RESET, L"恢复默认" }, { ID_CLOSE, L"关闭" },
    };
    int bx = 12;
    for (int i = 0; i < 4; i++) {
        HWND b = CreateWindowExW(0, L"BUTTON", btns[i].text, WS_CHILD | WS_VISIBLE | BS_PUSHBUTTON,
                                 bx, 800, 120, 30, h, (HMENU)(INT_PTR)btns[i].id, NULL, NULL);
        SendMessageW(b, WM_SETFONT, (WPARAM)g_font, TRUE);
        bx += 130;
    }
    HWND hint = CreateWindowExW(0, L"STATIC", L"点右侧按钮后按下新键即可改键；Ctrl+Alt+S 开关本窗口。改完点“应用并保存”。",
                                WS_CHILD | WS_VISIBLE | SS_LEFT, 550, 802, 430, 40, h, (HMENU)ID_HINT, NULL, NULL);
    SendMessageW(hint, WM_SETFONT, (WPARAM)g_font, TRUE);
}

// ---------------- 数值读写 ----------------
static void RefreshControls(HWND h) {
    for (size_t i = 0; i < g_fields.size(); i++) {
        HWND c = GetDlgItem(h, ID_FIELD_BASE + (int)i);
        if (!c) continue;
        if (g_fields[i].kind == FK_BOOL) {
            SendMessageW(c, BM_SETCHECK, (*g_fields[i].iv != 0) ? BST_CHECKED : BST_UNCHECKED, 0);
        } else {
            wchar_t buf[64];
            if (g_fields[i].kind == FK_FLOAT) swprintf(buf, 64, L"%.4f", *g_fields[i].f);
            else swprintf(buf, 64, L"%d", *g_fields[i].iv);
            SetWindowTextW(c, buf);
        }
    }
    for (int i = 0; i < DM_A_COUNT; i++) {
        HWND b = GetDlgItem(h, ID_HOTKEY_BASE + i);
        if (b) SetWindowTextW(b, DmHotkeyDisplay(g_st->hk[i]).c_str());
    }
}

static void ApplyControls(HWND h) {
    for (size_t i = 0; i < g_fields.size(); i++) {
        HWND c = GetDlgItem(h, ID_FIELD_BASE + (int)i);
        if (!c) continue;
        if (g_fields[i].kind == FK_BOOL) {
            LRESULT r = SendMessageW(c, BM_GETCHECK, 0, 0);
            *g_fields[i].iv = (r == BST_CHECKED) ? 1 : 0;
        } else {
            wchar_t buf[64] = {0};
            GetWindowTextW(c, buf, 64);
            if (g_fields[i].kind == FK_FLOAT) {
                float v = (float)_wtof(buf);
                if (v < g_fields[i].minv) v = g_fields[i].minv;
                if (v > g_fields[i].maxv) v = g_fields[i].maxv;
                *g_fields[i].f = v;
            } else {
                int v = _wtoi(buf);
                if (v < (int)g_fields[i].minv) v = (int)g_fields[i].minv;
                if (v > (int)g_fields[i].maxv) v = (int)g_fields[i].maxv;
                *g_fields[i].iv = v;
            }
        }
    }
    DmSettingsClamp(g_st);
    DmSettingsSave(g_st);
    if (g_apply) g_apply();
    RefreshControls(h);
}
// ---------------- 窗口过程 ----------------
static LRESULT CALLBACK UiWndProc(HWND h, UINT m, WPARAM w, LPARAM l) {
    switch (m) {
    case WM_CREATE:
        CreateControls(h);
        RefreshControls(h);
        return 0;

    case WM_CTLCOLORSTATIC:
    case WM_CTLCOLOREDIT: {
        HDC dc = (HDC)w;
        SetTextColor(dc, RGB(225, 228, 234));
        SetBkColor(dc, RGB(28, 31, 38));
        if (!g_bg) g_bg = CreateSolidBrush(RGB(28, 31, 38));
        return (LRESULT)g_bg;
    }
    case WM_ERASEBKGND: {
        HDC dc = (HDC)w;
        RECT r;
        GetClientRect(h, &r);
        if (!g_bg) g_bg = CreateSolidBrush(RGB(28, 31, 38));
        FillRect(dc, &r, g_bg);
        return 1;
    }

    case WM_KEYDOWN: {
        if (g_capture >= 0) {
            int vk = (int)w;
            if (vk == VK_CONTROL || vk == VK_MENU || vk == VK_SHIFT || vk == VK_LWIN || vk == VK_RWIN) return 0;
            if (vk == VK_ESCAPE) { g_capture = -1; RefreshControls(h); return 0; }
            DmHotkey hk; hk.vk = vk; hk.mods = 0;
            if (GetKeyState(VK_CONTROL) & 0x8000) hk.mods |= DMK_CTRL;
            if (GetKeyState(VK_MENU) & 0x8000) hk.mods |= DMK_ALT;
            if (GetKeyState(VK_SHIFT) & 0x8000) hk.mods |= DMK_SHIFT;
            if ((GetKeyState(VK_LWIN) & 0x8000) || (GetKeyState(VK_RWIN) & 0x8000)) hk.mods |= DMK_WIN;
            g_st->hk[g_capture] = hk;
            g_capture = -1;
            RefreshControls(h);
            return 0;
        }
        break;
    }

    case WM_COMMAND: {
        int id = LOWORD(w);
        if (id >= ID_HOTKEY_BASE && id < ID_HOTKEY_BASE + DM_A_COUNT) {
            g_capture = id - ID_HOTKEY_BASE;
            HWND b = GetDlgItem(h, id);
            if (b) SetWindowTextW(b, L"请按键…(Esc 取消)");
            SetFocus(h);
            return 0;
        }
        switch (id) {
        case ID_APPLY:  ApplyControls(h); return 0;
        case ID_RELOAD: DmSettingsLoad(g_st); RefreshControls(h); if (g_apply) g_apply(); return 0;
        case ID_RESET:  DmSettingsDefault(g_st); ApplyControls(h); return 0;
        case ID_CLOSE:  ShowWindow(h, SW_HIDE); return 0;
        }
        break;
    }

    case WM_CLOSE:
        ShowWindow(h, SW_HIDE);      // 只隐藏，不销毁（渲染继续）
        return 0;
    }
    return DefWindowProcW(h, m, w, l);
}

// ---------------- 对外接口 ----------------
void DmUiRegisterHotkey() {
    BOOL ok = RegisterHotKey(NULL, kUiHotkeyId, MOD_CONTROL | MOD_ALT | MOD_NOREPEAT, 'S');
    DmLog("配置窗口：热键 Ctrl+Alt+S 注册 %s", ok ? "成功" : "失败");
}

bool DmUiIsOurHotkey(int id) { return id == kUiHotkeyId; }

bool DmUiSettingsVisible() {
    return g_wnd && IsWindowVisible(g_wnd);
}

void DmUiToggleSettings(DmSettings* st, void (*applyCb)(void)) {
    g_st = st;
    g_apply = applyCb;
    if (!g_wnd) {
        DmLog("配置窗口：创建中");
        WNDCLASSEXW wc = {0};
        wc.cbSize = sizeof(wc);
        wc.lpfnWndProc = UiWndProc;
        wc.hInstance = GetModuleHandleW(NULL);
        wc.hCursor = LoadCursorW(NULL, IDC_ARROW);
        wc.lpszClassName = L"DmSettingsWnd";
        wc.hbrBackground = NULL;
        RegisterClassExW(&wc);
        BuildFields(g_st);
        g_wnd = CreateWindowExW(0, L"DmSettingsWnd", L"华为 VR 桌面模式 · 配置",
                                WS_OVERLAPPEDWINDOW & ~WS_MAXIMIZEBOX,
                                120, 60, 1000, 880, NULL, NULL, wc.hInstance, NULL);
    }
    if (!g_wnd) { DmLog("配置窗口：CreateWindowEx 失败 err=%lu", GetLastError()); return; }
    if (IsWindowVisible(g_wnd)) { ShowWindow(g_wnd, SW_HIDE); return; }
    RefreshControls(g_wnd);
    ShowWindow(g_wnd, SW_SHOW);
    SetForegroundWindow(g_wnd);
}

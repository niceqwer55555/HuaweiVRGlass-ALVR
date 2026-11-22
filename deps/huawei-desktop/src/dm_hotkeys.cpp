// dm_hotkeys.cpp
#include "dm_hotkeys.h"
#include "dm_log.h"

static const int kBaseId = 0xD400;   // RegisterHotKey 的 id 基址

// ---------------- 手柄 / 鼠标支持 ----------------
struct XiGamepad { WORD buttons; BYTE lt, rt; SHORT lx, ly, rx, ry; };
struct XiState   { DWORD packet; XiGamepad gp; };
typedef DWORD (WINAPI *PFN_XIGET)(DWORD, XiState*);

static PFN_XIGET XiGet() {
    static PFN_XIGET fn = NULL;
    static bool tried = false;
    if (!tried) {
        tried = true;
        const wchar_t* names[] = { L"XInput1_4.dll", L"XInput1_3.dll", L"XInput9_1_0.dll" };
        for (int i = 0; i < 3 && !fn; i++) {
            HMODULE m = LoadLibraryW(names[i]);
            if (m) fn = (PFN_XIGET)GetProcAddress(m, "XInputGetState");
        }
    }
    return fn;
}
static bool PadDown(int code) {
    PFN_XIGET f = XiGet();
    if (!f) return false;
    XiState st; ZeroMemory(&st, sizeof(st));
    if (f(0, &st) != 0) return false;
    if (code == 10) return st.gp.lt > 128;
    if (code == 11) return st.gp.rt > 128;
    static const WORD mask[10] = {
        0x0001, 0x0002, 0x0004, 0x0008,   // up down left right
        0x0010, 0x0020,                   // start back
        0x0040, 0x0080,                   // ls rs
        0x0100, 0x0200                    // lb rb
    };
    static const WORD face[4] = { 0x1000, 0x2000, 0x4000, 0x8000 };  // a b x y
    if (code >= 12 && code <= 15) return (st.gp.buttons & face[code - 12]) != 0;
    if (code >= 0 && code < 10) return (st.gp.buttons & mask[code]) != 0;
    return false;
}
static bool MouseDown(int code) {
    int vk = (code == 1) ? VK_LBUTTON : (code == 2) ? VK_RBUTTON :
             (code == 3) ? VK_MBUTTON : (code == 4) ? VK_XBUTTON1 : VK_XBUTTON2;
    return (GetAsyncKeyState(vk) & 0x8000) != 0;
}
static bool KeyDown(int vk) { return (GetAsyncKeyState(vk) & 0x8000) != 0; }

int DmHotkeys::Install(const DmSettings& st, int* failList, int* failCount) {
    Uninstall();
    int fails = 0;
    if (failCount) *failCount = 0;
    for (int i = 0; i < DM_A_COUNT; i++) {
        hk[i] = st.hk[i];
        ok[i] = false;
        active[i] = false;
        if (hk[i].kind != 0 || !hk[i].vk) continue;   // 鼠标/手柄走轮询，不注册
        UINT mods = MOD_NOREPEAT;
        if (hk[i].mods & DMK_CTRL)  mods |= MOD_CONTROL;
        if (hk[i].mods & DMK_ALT)   mods |= MOD_ALT;
        if (hk[i].mods & DMK_SHIFT) mods |= MOD_SHIFT;
        if (hk[i].mods & DMK_WIN)   mods |= MOD_WIN;
        if (RegisterHotKey(NULL, kBaseId + i, mods, (UINT)hk[i].vk)) {
            ok[i] = true;
            anyRegistered = true;
        } else {
            fails++;
            if (failList && failCount && *failCount < 32) failList[(*failCount)++] = i;
            DmLog("热键注册失败（被占用）：%S", DmHotkeyDisplay(hk[i]).c_str());
        }
    }
    DmLog("热键注册：成功 %d 个，失败 %d 个", DM_A_COUNT - fails, fails);
    return fails;
}

void DmHotkeys::Uninstall() {
    for (int i = 0; i < DM_A_COUNT; i++)
        if (ok[i]) { UnregisterHotKey(NULL, kBaseId + i); ok[i] = false; }
    anyRegistered = false;
}

int DmHotkeys::OnMessage(UINT msg, WPARAM wp) {
    if (msg != WM_HOTKEY) return -1;
    int id = (int)wp - kBaseId;
    if (id < 0 || id >= DM_A_COUNT) return -1;
    ULONGLONG now = GetTickCount64();
    active[id] = true;
    holdStart[id] = now;
    lastFire[id] = now;
    return id;
}

bool DmHotkeys::IsDown(int action) {
    const DmHotkey& h = hk[action];
    if (h.kind == 1) return MouseDown(h.code);
    if (h.kind == 2) return PadDown(h.code);
    if (!h.vk) return false;
    return KeyDown(h.vk);
}

void DmHotkeys::Tick(ULONGLONG now, void (*fire)(int action, bool fast)) {
    static const DmActionInfo* tbl = DmActionTable();
    // 鼠标/手柄/键盘：统一靠上升沿轮询
    //   键盘也轮询（不依赖 WM_HOTKEY）：即使组合键被别的程序抢注，物理按键状态依然能读到
    static bool wasDown[DM_A_COUNT] = { false };
    bool ctrl  = (GetAsyncKeyState(VK_CONTROL) & 0x8000) != 0;
    bool alt   = (GetAsyncKeyState(VK_MENU)    & 0x8000) != 0;
    bool shift = (GetAsyncKeyState(VK_SHIFT)   & 0x8000) != 0;
    bool win   = ((GetAsyncKeyState(VK_LWIN) & 0x8000) || (GetAsyncKeyState(VK_RWIN) & 0x8000)) != 0;
    bool modsOK = true;
    for (int i = 0; i < DM_A_COUNT; i++) {
        if (hk[i].kind != 0 || !hk[i].vk) continue;
        modsOK = (!(hk[i].mods & DMK_CTRL)  || ctrl)
              && (!(hk[i].mods & DMK_ALT)   || alt)
              && (!(hk[i].mods & DMK_SHIFT) || shift)
              && (!(hk[i].mods & DMK_WIN)   || win);
        // 额外要求：没声明的锁定修饰键必须没按下，避免 Ctrl+Alt+Shift+X 误触发 Ctrl+Alt+X
        if (!(hk[i].mods & DMK_SHIFT) && shift) modsOK = false;
        if (!(hk[i].mods & DMK_CTRL)  && ctrl)  modsOK = false;
        if (!(hk[i].mods & DMK_ALT)   && alt)   modsOK = false;
        bool d = modsOK && ((GetAsyncKeyState((int)hk[i].vk) & 0x8000) != 0);
        if (d && !wasDown[i]) { active[i] = true; holdStart[i] = now; lastFire[i] = now; fire(i, false); }
        if (!d) active[i] = false;
        wasDown[i] = d;
    }
    for (int i = 0; i < DM_A_COUNT; i++) {
        if (hk[i].kind != 1 && hk[i].kind != 2) continue;
        bool d = IsDown(i);
        if (d && !wasDown[i]) {
            active[i] = true; holdStart[i] = now; lastFire[i] = now;
            fire(i, false);
        }
        wasDown[i] = d;
        if (!d) active[i] = false;
    }
    for (int i = 0; i < DM_A_COUNT; i++) {
        if (!active[i]) continue;
        if (!IsDown(i)) { active[i] = false; continue; }
        if (!tbl[i].repeatable) { active[i] = false; continue; }
        ULONGLONG held = now - holdStart[i];
        ULONGLONG interval = (held > 1200) ? 45 : ((held > 450) ? 80 : 0);
        if (!interval) continue;
        if (now - lastFire[i] >= interval) {
            lastFire[i] = now;
            fire(i, true);
        }
    }
}

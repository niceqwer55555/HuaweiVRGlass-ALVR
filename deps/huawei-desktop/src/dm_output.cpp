// dm_output.cpp
#include "dm_output.h"
#include "dm_log.h"
#include <stdio.h>
#include <vector>

const wchar_t* kDmOutputClass = L"DmOutputWnd";

struct MonEntry { HMONITOR h; RECT r; std::wstring dev, name; };

static BOOL CALLBACK MonProc(HMONITOR hm, HDC, LPRECT, LPARAM lp) {
    auto* v = (std::vector<MonEntry>*)lp;
    MONITORINFOEXW mi; mi.cbSize = sizeof(mi);
    if (!GetMonitorInfoW(hm, &mi)) return TRUE;
    MonEntry e;
    e.h = hm;
    e.r = mi.rcMonitor;
    e.dev = mi.szDevice;
    DISPLAY_DEVICEW dd; dd.cb = sizeof(dd);
    if (EnumDisplayDevicesW(mi.szDevice, 0, &dd, 0)) e.name = dd.DeviceString;
    v->push_back(e);
    return TRUE;
}

static std::vector<MonEntry> EnumMons() {
    std::vector<MonEntry> v;
    EnumDisplayMonitors(NULL, NULL, MonProc, (LPARAM)&v);
    return v;
}

int DmFindGlassesMonitorIndex() {
    auto v = EnumMons();
    for (size_t i = 0; i < v.size(); i++) {
        int w = v[i].r.right - v[i].r.left, h = v[i].r.bottom - v[i].r.top;
        std::wstring n = v[i].name + v[i].dev;
        for (auto& c : n) c = towupper(c);
        if (n.find(L"HUAWEI") != std::wstring::npos || n.find(L"HWV") != std::wstring::npos ||
            (w == 3200 && h == 1600))
            return (int)i;
    }
    return -1;
}

static LRESULT CALLBACK OutWndProc(HWND h, UINT m, WPARAM w, LPARAM l) {
    // ★ 必须处理 WM_PAINT：否则更新区永远不合法，系统会不停投递 WM_PAINT，
    //   主循环的 PeekMessage 就会一直返回 TRUE，渲染代码一帧都跑不到（表现为整屏黑）。
    if (m == WM_PAINT) {
        PAINTSTRUCT ps;
        BeginPaint(h, &ps);
        EndPaint(h, &ps);
        return 0;
    }
    if (m == WM_ERASEBKGND) return 1;
    if (m == WM_NCHITTEST) return HTTRANSPARENT;   // 别挡鼠标
    return DefWindowProcW(h, m, w, l);
}

// 把眼镜那块屏（HWV/HUAWEI，3200x1600）在 Windows 里启用为扩展显示器
bool DmEnableGlassesDisplay() {
    UINT32 np = 0, nm = 0;
    if (GetDisplayConfigBufferSizes(QDC_ALL_PATHS, &np, &nm) != ERROR_SUCCESS) return false;
    std::vector<DISPLAYCONFIG_PATH_INFO> paths(np);
    std::vector<DISPLAYCONFIG_MODE_INFO> modes(nm);
    if (QueryDisplayConfig(QDC_ALL_PATHS, &np, paths.data(), &nm, modes.data(), NULL) != ERROR_SUCCESS) return false;
    paths.resize(np);
    modes.resize(nm);

    int cand = -1;
    for (UINT32 i = 0; i < np; i++) {
        if (paths[i].flags & DISPLAYCONFIG_PATH_ACTIVE) continue;
        DISPLAYCONFIG_TARGET_DEVICE_NAME tn; ZeroMemory(&tn, sizeof(tn));
        tn.header.type = DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME;
        tn.header.size = sizeof(tn);
        tn.header.adapterId = paths[i].targetInfo.adapterId;
        tn.header.id = paths[i].targetInfo.id;
        if (DisplayConfigGetDeviceInfo(&tn.header) != ERROR_SUCCESS) continue;
        if (!paths[i].targetInfo.targetAvailable) continue;
        std::wstring name = tn.monitorFriendlyDeviceName;
        std::wstring path = tn.monitorDevicePath;
        for (auto& c : name) c = towupper(c);
        for (auto& c : path) c = towupper(c);
        if (name.find(L"HUAWEI") != std::wstring::npos || path.find(L"HWV") != std::wstring::npos ||
            name.find(L"AV03") != std::wstring::npos) {
            cand = (int)i;
            break;
        }
    }
    if (cand < 0) return false;

    // 主屏右边缘
    LONG right = 0;
    for (UINT32 i = 0; i < np; i++) {
        if (!(paths[i].flags & DISPLAYCONFIG_PATH_ACTIVE)) continue;
        UINT32 mi = paths[i].sourceInfo.modeInfoIdx;
        if (mi < nm && modes[mi].infoType == DISPLAYCONFIG_MODE_INFO_TYPE_SOURCE) {
            LONG r = modes[mi].sourceMode.position.x + (LONG)modes[mi].sourceMode.width;
            if (r > right) right = r;
        }
    }

    // 目标首选模式（EDID 给的）
    DISPLAYCONFIG_TARGET_PREFERRED_MODE pref; ZeroMemory(&pref, sizeof(pref));
    pref.header.type = DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_PREFERRED_MODE;
    pref.header.size = sizeof(pref);
    pref.header.adapterId = paths[cand].targetInfo.adapterId;
    pref.header.id = paths[cand].targetInfo.id;
    if (DisplayConfigGetDeviceInfo(&pref.header) != ERROR_SUCCESS) return false;

    UINT32 srcIdx = (UINT32)modes.size();
    DISPLAYCONFIG_MODE_INFO sm; ZeroMemory(&sm, sizeof(sm));
    sm.infoType = DISPLAYCONFIG_MODE_INFO_TYPE_SOURCE;
    sm.id = paths[cand].sourceInfo.id;
    sm.adapterId = paths[cand].sourceInfo.adapterId;
    sm.sourceMode.width = pref.targetMode.targetVideoSignalInfo.activeSize.cx;
    sm.sourceMode.height = pref.targetMode.targetVideoSignalInfo.activeSize.cy;
    sm.sourceMode.pixelFormat = DISPLAYCONFIG_PIXELFORMAT_32BPP;
    sm.sourceMode.position.x = right;
    sm.sourceMode.position.y = 0;
    modes.push_back(sm);

    UINT32 tgtIdx = (UINT32)modes.size();
    DISPLAYCONFIG_MODE_INFO tm; ZeroMemory(&tm, sizeof(tm));
    tm.infoType = DISPLAYCONFIG_MODE_INFO_TYPE_TARGET;
    tm.id = paths[cand].targetInfo.id;
    tm.adapterId = paths[cand].targetInfo.adapterId;
    tm.targetMode.targetVideoSignalInfo = pref.targetMode.targetVideoSignalInfo;
    modes.push_back(tm);

    paths[cand].sourceInfo.modeInfoIdx = srcIdx;
    paths[cand].targetInfo.modeInfoIdx = tgtIdx;
    paths[cand].sourceInfo.statusFlags = DISPLAYCONFIG_SOURCE_IN_USE;
    paths[cand].targetInfo.statusFlags = DISPLAYCONFIG_TARGET_IN_USE;
    paths[cand].flags |= DISPLAYCONFIG_PATH_ACTIVE;

    LONG r = SetDisplayConfig((UINT32)paths.size(), paths.data(), (UINT32)modes.size(), modes.data(),
                              SDC_APPLY | SDC_USE_SUPPLIED_DISPLAY_CONFIG | SDC_ALLOW_CHANGES);
    DmLog("启用眼镜显示器：SetDisplayConfig 返回 %ld（%ux%u @ %ld）", r,
          sm.sourceMode.width, sm.sourceMode.height, sm.sourceMode.position.x);
    return r == ERROR_SUCCESS;
}

bool DmOutput::Create(ID3D11Device* dev, int monitorIndex, std::string* err, bool wantExclusive) {
    Destroy();
    auto mons = EnumMons();
    if (mons.empty()) { if (err) *err = "没有显示器"; return false; }
    int idx = monitorIndex;
    if (idx < 0 || idx >= (int)mons.size()) idx = DmFindGlassesMonitorIndex();
    if (idx < 0) {
        // 眼镜屏被 SteamVR 直显关掉了 → 自己把它扩展出来再试一次
        if (DmEnableGlassesDisplay()) {
            Sleep(1200);
            mons = EnumMons();
            idx = DmFindGlassesMonitorIndex();
        }
    }
    if (idx < 0 || idx >= (int)mons.size()) { if (err) *err = "找不到眼镜显示器（3200x1600）"; return false; }
    monitorIndex = idx;
    RECT r = mons[idx].r;
    w = (uint32_t)(r.right - r.left);
    h = (uint32_t)(r.bottom - r.top);

    static bool clsReg = false;
    if (!clsReg) {
        WNDCLASSEXW wc = { sizeof(WNDCLASSEXW) };
        wc.lpfnWndProc = OutWndProc;
        wc.hInstance = GetModuleHandleW(NULL);
        wc.hCursor = NULL;
        wc.hbrBackground = (HBRUSH)GetStockObject(BLACK_BRUSH);
        wc.lpszClassName = kDmOutputClass;
        RegisterClassExW(&wc);
        clsReg = true;
    }
    hwnd = CreateWindowExW(WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW, kDmOutputClass,
                           L"HwVR Desktop", WS_POPUP | WS_VISIBLE,
                           r.left, r.top, (int)w, (int)h, NULL, NULL, GetModuleHandleW(NULL), NULL);
    if (!hwnd) { if (err) *err = "创建输出窗口失败"; return false; }
    ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    SetWindowPos(hwnd, HWND_TOPMOST, r.left, r.top, (int)w, (int)h, SWP_NOACTIVATE | SWP_SHOWWINDOW);

    IDXGIDevice* dxdev = NULL;
    dev->QueryInterface(__uuidof(IDXGIDevice), (void**)&dxdev);
    if (!dxdev) { if (err) *err = "取 IDXGIDevice 失败"; return false; }
    IDXGIAdapter* ad = NULL;
    dxdev->GetAdapter(&ad);
    dxdev->Release();
    IDXGIFactory2* fac = NULL;
    if (ad) { ad->GetParent(__uuidof(IDXGIFactory2), (void**)&fac); ad->Release(); }
    if (!fac) { if (err) *err = "取 IDXGIFactory2 失败"; return false; }

    DXGI_SWAP_CHAIN_DESC1 sd; ZeroMemory(&sd, sizeof(sd));
    sd.Width = w; sd.Height = h;
    sd.Format = DXGI_FORMAT_B8G8R8A8_UNORM;
    sd.Stereo = FALSE;
    sd.SampleDesc.Count = 1;
    sd.BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT;
    sd.BufferCount = 2;
    sd.Scaling = DXGI_SCALING_STRETCH;
    sd.SwapEffect = DXGI_SWAP_EFFECT_FLIP_DISCARD;
    sd.AlphaMode = DXGI_ALPHA_MODE_IGNORE;
    HRESULT hr = fac->CreateSwapChainForHwnd(dev, hwnd, &sd, NULL, NULL, &swap);
    fac->Release();
    if (FAILED(hr)) {
        char b[80]; sprintf(b, "CreateSwapChainForHwnd 失败 0x%08lX", (unsigned long)hr);
        if (err) *err = b;
        return false;
    }
    // ★ 独占全屏：让 GPU 直接把我们的后备缓冲扫到这块屏上，绕过 DWM 合成。
    //   HMD 面板（尤其被厂商驱动接管过的）用窗口模式常常只显示黑底，独占全屏才是标准做法。
    exclusive = false;
    if (wantExclusive) {
        IDXGIAdapter* ad2 = NULL;
        IDXGIDevice* dx2 = NULL;
        dev->QueryInterface(__uuidof(IDXGIDevice), (void**)&dx2);
        if (dx2) { dx2->GetAdapter(&ad2); dx2->Release(); }
        if (ad2) {
            // 按设备名（\\.\DISPLAY3）找对应的 DXGI 输出
            MONITORINFOEXW mi; ZeroMemory(&mi, sizeof(mi)); mi.cbSize = sizeof(mi);
            GetMonitorInfoW(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST), &mi);
            IDXGIOutput* target = NULL;
            for (UINT i = 0; ; i++) {
                IDXGIOutput* o = NULL;
                if (ad2->EnumOutputs(i, &o) == DXGI_ERROR_NOT_FOUND) break;
                DXGI_OUTPUT_DESC od; o->GetDesc(&od);
                if (wcscmp(od.DeviceName, mi.szDevice) == 0) { target = o; break; }
                o->Release();
            }
            ad2->Release();
            if (target) {
                HRESULT eh = swap->SetFullscreenState(TRUE, target);
                target->Release();
                exclusive = SUCCEEDED(eh);
                DmLog("独占全屏 -> hr=0x%08lX %s", (unsigned long)eh, exclusive ? "成功" : "失败，退回窗口模式");
            } else {
                DmLog("独占全屏 -> 找不到对应的 DXGI 输出，保持窗口模式");
            }
        }
    }
    if (FAILED(swap->GetBuffer(0, __uuidof(ID3D11Texture2D), (void**)&backTex))) {
        if (err) *err = "取交换链后备缓冲失败";
        return false;
    }
    if (FAILED(dev->CreateRenderTargetView(backTex, NULL, &backRtv))) {
        if (err) *err = "建后备缓冲 RTV 失败";
        return false;
    }
    {
        RECT wr = {0,0,0,0}, cr = {0,0,0,0};
        GetWindowRect(hwnd, &wr);
        GetClientRect(hwnd, &cr);
        HMONITOR hm = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        MONITORINFOEXW mi; ZeroMemory(&mi, sizeof(mi)); mi.cbSize = sizeof(mi);
        GetMonitorInfoW(hm, &mi);
        DmLog("窗口所在显示器=%S rect=(%ld,%ld)-(%ld,%ld) dpi=%.0f", mi.szDevice,
              mi.rcMonitor.left, mi.rcMonitor.top, mi.rcMonitor.right, mi.rcMonitor.bottom,
              (double)96.0);
        DmLog("输出：显示器 %S 目标=%ux%u @ (%d,%d) | 窗口=(%d,%d)-(%d,%d) 客户区=%dx%d visible=%d",
              mons[idx].name.c_str(), w, h, r.left, r.top,
              wr.left, wr.top, wr.right, wr.bottom,
              cr.right - cr.left, cr.bottom - cr.top, IsWindowVisible(hwnd) ? 1 : 0);
    }
    return true;
}

void DmOutput::Present(ID3D11DeviceContext* ctx, ID3D11Texture2D* src, bool vsync) {
    if (!swap || !backTex || !src) return;
    ctx->CopyResource(backTex, src);
    HRESULT hr = swap->Present(vsync ? 1 : 0, 0);   // 跟显示器刷新（90Hz）
    static int n = 0;
    if (n < 3 || (n % 300) == 0) DmLog("Present #%d hr=0x%08lX", n, (unsigned long)hr);
    n++;
}

void DmOutput::Destroy() {
    if (swap && exclusive) { swap->SetFullscreenState(FALSE, NULL); exclusive = false; }
    if (backRtv) { backRtv->Release(); backRtv = NULL; }
    if (backTex) { backTex->Release(); backTex = NULL; }
    if (swap) { swap->Release(); swap = NULL; }
    if (hwnd) { DestroyWindow(hwnd); hwnd = NULL; }
}

// dm_capture.cpp
#include "dm_capture.h"
#include "dm_log.h"
#include "dm_settings.h"
#include <dxgi1_2.h>
#include <stdio.h>

struct MonEnumCtx { std::vector<DmMonitorInfo>* out; };

static BOOL CALLBACK MonProc(HMONITOR hm, HDC, LPRECT, LPARAM lp) {
    auto* out = (std::vector<DmMonitorInfo>*)lp;
    MONITORINFOEXW mi; mi.cbSize = sizeof(mi);
    if (!GetMonitorInfoW(hm, &mi)) return TRUE;
    DmMonitorInfo m;
    m.device = mi.szDevice;
    m.rect = mi.rcMonitor;
    m.primary = (mi.dwFlags & MONITORINFOF_PRIMARY) != 0;
    m.w = mi.rcMonitor.right - mi.rcMonitor.left;
    m.h = mi.rcMonitor.bottom - mi.rcMonitor.top;
    // 友好名
    DISPLAY_DEVICEW dd; dd.cb = sizeof(dd);
    if (EnumDisplayDevicesW(mi.szDevice, 0, &dd, 0)) m.name = dd.DeviceString;
    out->push_back(m);
    return TRUE;
}

std::vector<DmMonitorInfo> DmEnumMonitors() {
    std::vector<DmMonitorInfo> v;
    EnumDisplayMonitors(NULL, NULL, MonProc, (LPARAM)&v);
    // 主屏放第一个
    for (size_t i = 0; i < v.size(); i++)
        if (v[i].primary && i != 0) { std::swap(v[0], v[i]); break; }
    return v;
}

void DmCapture::Stop() {
    if (dup) { dup->Release(); dup = NULL; }
    if (srv) { srv->Release(); srv = NULL; }
    if (tex) { tex->Release(); tex = NULL; }
    if (curSrv) { curSrv->Release(); curSrv = NULL; }
    curHandle = NULL;
    started = false;
    w = h = 0;
}

bool DmCapture::Start(ID3D11Device* d, ID3D11DeviceContext* c, int monitorIndex, std::string* err) {
    Stop();
    dev = d; ctx = c;
    auto mons = DmEnumMonitors();
    if (mons.empty()) { if (err) *err = "没有找到显示器"; return false; }
    if (monitorIndex < 0 || monitorIndex >= (int)mons.size()) monitorIndex = 0;
    index = monitorIndex;
    const DmMonitorInfo& m = mons[monitorIndex];
    rect = m.rect;

    IDXGIDevice* dxdev = NULL;
    dev->QueryInterface(__uuidof(IDXGIDevice), (void**)&dxdev);
    if (!dxdev) { if (err) *err = "取 IDXGIDevice 失败"; return false; }
    IDXGIAdapter* ad = NULL;
    dxdev->GetAdapter(&ad);
    dxdev->Release();
    if (!ad) { if (err) *err = "取 IDXGIAdapter 失败"; return false; }

    IDXGIOutput* target = NULL;
    for (UINT i = 0; ; i++) {
        IDXGIOutput* o = NULL;
        if (ad->EnumOutputs(i, &o) == DXGI_ERROR_NOT_FOUND) break;
        DXGI_OUTPUT_DESC od; o->GetDesc(&od);
        if (!wcscmp(od.DeviceName, m.device.c_str())) { target = o; break; }
        o->Release();
    }
    ad->Release();
    if (!target) { if (err) *err = "DXGI 里找不到对应输出"; return false; }

    IDXGIOutput1* o1 = NULL;
    target->QueryInterface(__uuidof(IDXGIOutput1), (void**)&o1);
    target->Release();
    if (!o1) { if (err) *err = "该输出不支持 Duplication"; return false; }
    HRESULT hr = o1->DuplicateOutput(dev, &dup);
    o1->Release();
    if (FAILED(hr)) {
        char b[64]; sprintf(b, "DuplicateOutput 失败 hr=0x%08lX", (unsigned long)hr);
        if (err) *err = b;
        lastErr = (int)hr;
        return false;
    }
    started = true;
    DmLog("屏幕采集已启动：%S (%S) %ux%u", m.device.c_str(), m.name.c_str(), m.w, m.h);
    return true;
}

bool DmCapture::Acquire() {
    if (!started || !dup) return false;
    DXGI_OUTDUPL_FRAME_INFO fi;
    IDXGIResource* res = NULL;
    HRESULT hr = dup->AcquireNextFrame(0, &fi, &res);
    if (hr == DXGI_ERROR_WAIT_TIMEOUT) return false;
    if (FAILED(hr)) {
        // 锁屏 / 分辨率变化 / 权限变化：重建
        DmLog("AcquireNextFrame 失败 hr=0x%08lX，重建采集", (unsigned long)hr);
        lastErr = (int)hr;
        int idx = index;
        Start(dev, ctx, idx, NULL);
        return false;
    }
    if (res) {
        ID3D11Texture2D* src = NULL;
        if (SUCCEEDED(res->QueryInterface(__uuidof(ID3D11Texture2D), (void**)&src))) {
            D3D11_TEXTURE2D_DESC sd;
            src->GetDesc(&sd);
            if (!tex || w != sd.Width || h != sd.Height) {
                if (srv) { srv->Release(); srv = NULL; }
                if (tex) { tex->Release(); tex = NULL; }
                D3D11_TEXTURE2D_DESC td = sd;
                td.Usage = D3D11_USAGE_DEFAULT;
                td.BindFlags = D3D11_BIND_SHADER_RESOURCE;
                td.CPUAccessFlags = 0;
                td.MiscFlags = 0;
                td.ArraySize = 1;
                td.MipLevels = 1;
                if (FAILED(dev->CreateTexture2D(&td, NULL, &tex))) {
                    DmLog("创建采集纹理失败 %ux%u", sd.Width, sd.Height);
                } else {
                    w = sd.Width; h = sd.Height;
                    D3D11_SHADER_RESOURCE_VIEW_DESC vd; ZeroMemory(&vd, sizeof(vd));
                    vd.Format = td.Format;
                    vd.ViewDimension = D3D11_SRV_DIMENSION_TEXTURE2D;
                    vd.Texture2D.MipLevels = 1;
                    dev->CreateShaderResourceView(tex, &vd, &srv);
                    DmLog("采集尺寸 %ux%u fmt=%d", w, h, (int)td.Format);
                }
            }
            if (tex && srv) ctx->CopyResource(tex, src);
            src->Release();
        }
        res->Release();
        dup->ReleaseFrame();
        frames++;
        return true;
    }
    dup->ReleaseFrame();
    return false;
}

void DmCapture::UpdateCursor(ID3D11Device* d) {
    CURSORINFO ci; ci.cbSize = sizeof(ci);
    if (!GetCursorInfo(&ci) || !(ci.flags & CURSOR_SHOWING)) { curVisible = false; return; }
    POINT p;
    if (!GetCursorPos(&p)) { curVisible = false; return; }
    if (p.x < rect.left || p.x >= rect.right || p.y < rect.top || p.y >= rect.bottom) { curVisible = false; return; }
    float lu = (float)(p.x - rect.left) / (float)(rect.right - rect.left);  // logical 0..1 (rect space)
    float lv = (float)(p.y - rect.top) / (float)(rect.bottom - rect.top);
    // Some panels (e.g. Steam Deck portrait 800x1280 reported as landscape
    // 1280x800 by the EDID) give a DDA texture whose W/H are SWAPPED relative
    // to the logical monitor rect. The texture is the desktop rotated 90deg,
    // so the cursor UV must be mapped into that rotated texture space.
    // Without this the cursor lands at the wrong spot (mirrored/rotated).
    float rw = (float)(rect.right - rect.left), rh = (float)(rect.bottom - rect.top);
    bool swapped = (w > 0 && h > 0 && rw > 0 && rh > 0) &&
                   ((w > h) != (rw > rh));   // texture orientation differs from rect
    curSwapped = swapped;
    if (swapped) {
        // DDA texture W/H are swapped vs the logical monitor (portrait panel
        // reported as landscape). Pick the mapping via ini [display] cursor_rot:
        //   0: u=lv,   v=1-lu
        //   1: u=1-lv, v=lu
        //   2: u=1-lv, v=1-lu
        //   3: u=lv,   v=lu
        int rot = GetPrivateProfileIntW(L"display", L"cursor_rot", 0, DmIniPath().c_str());
        switch (rot) {
        case 1:  curU = 1.0f - lv; curV = lu;        break;
        case 2:  curU = 1.0f - lv; curV = 1.0f - lu; break;
        case 3:  curU = lv;        curV = lu;        break;
        default: curU = lv;        curV = 1.0f - lu; break;
        }
    } else {
        curU = lu;
        curV = lv;
    }
    curVisible = true;

    if (ci.hCursor == curHandle && curSrv) return;
    // 画一次光标
    ICONINFO ii;
    if (!GetIconInfo(ci.hCursor, &ii)) return;
    curHotX = ii.xHotspot; curHotY = ii.yHotspot;
    // 记录图标原始尺寸（后面画成 64x64 只是为了采样，显示尺寸要按原始像素算）
    {
        BITMAP bm;
        if (ii.hbmColor && GetObject(ii.hbmColor, sizeof(bm), &bm)) { curW = bm.bmWidth; curH = bm.bmHeight; }
        else if (ii.hbmMask && GetObject(ii.hbmMask, sizeof(bm), &bm)) { curW = bm.bmWidth; curH = bm.bmHeight / 2; }
        if (curW <= 0 || curH <= 0) { curW = 32; curH = 32; }
    }
    if (ii.hbmMask) DeleteObject(ii.hbmMask);
    if (ii.hbmColor) DeleteObject(ii.hbmColor);
    const int S = 64;
    BITMAPV5HEADER bh; ZeroMemory(&bh, sizeof(bh));
    bh.bV5Size = sizeof(bh);
    bh.bV5Width = S; bh.bV5Height = -S;
    bh.bV5Planes = 1; bh.bV5BitCount = 32; bh.bV5Compression = BI_BITFIELDS;
    bh.bV5RedMask = 0x00FF0000; bh.bV5GreenMask = 0x0000FF00; bh.bV5BlueMask = 0x000000FF; bh.bV5AlphaMask = 0xFF000000;
    HDC screen = GetDC(NULL);
    void* bits = NULL;
    HBITMAP dib = CreateDIBSection(screen, (BITMAPINFO*)&bh, DIB_RGB_COLORS, &bits, NULL, 0);
    HDC mem = CreateCompatibleDC(screen);
    HGDIOBJ old = SelectObject(mem, dib);
    memset(bits, 0, S * S * 4);
    if (!DrawIconEx(mem, 0, 0, ci.hCursor, S, S, 0, NULL, DI_NORMAL))
        DrawIconEx(mem, 0, 0, ci.hCursor, 32, 32, 0, NULL, DI_NORMAL);
    SelectObject(mem, old);
    DeleteDC(mem);
    ReleaseDC(NULL, screen);
    // 用 alpha 判断是否有内容；很多光标 alpha 全 0（老式 AND/XOR 光标）
    BYTE* px = (BYTE*)bits;
    bool anyAlpha = false;
    for (int i = 0; i < S * S; i++) if (px[i * 4 + 3]) { anyAlpha = true; break; }
    if (!anyAlpha)
        for (int i = 0; i < S * S; i++) {
            // 亮度当 alpha：黑底白字的 XOR 蒙版
            int lum = (px[i * 4] + px[i * 4 + 1] + px[i * 4 + 2]) / 3;
            px[i * 4 + 3] = (BYTE)(lum > 8 ? 255 : 0);
        }
    if (curSrv) { curSrv->Release(); curSrv = NULL; }
    D3D11_TEXTURE2D_DESC td; ZeroMemory(&td, sizeof(td));
    td.Width = S; td.Height = S; td.MipLevels = 1; td.ArraySize = 1;
    td.Format = DXGI_FORMAT_B8G8R8A8_UNORM; td.SampleDesc.Count = 1;
    td.Usage = D3D11_USAGE_DEFAULT; td.BindFlags = D3D11_BIND_SHADER_RESOURCE;
    D3D11_SUBRESOURCE_DATA sd; ZeroMemory(&sd, sizeof(sd));
    sd.pSysMem = bits; sd.SysMemPitch = S * 4;
    ID3D11Texture2D* t = NULL;
    if (SUCCEEDED(d->CreateTexture2D(&td, &sd, &t))) {
        D3D11_SHADER_RESOURCE_VIEW_DESC vd; ZeroMemory(&vd, sizeof(vd));
        vd.Format = td.Format; vd.ViewDimension = D3D11_SRV_DIMENSION_TEXTURE2D; vd.Texture2D.MipLevels = 1;
        d->CreateShaderResourceView(t, &vd, &curSrv);
        t->Release();
        curW = curH = S;
        curHandle = ci.hCursor;
        curAspect = 1.0f;
    }
    DeleteObject(dib);
}

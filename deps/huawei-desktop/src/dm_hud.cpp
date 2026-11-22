// dm_hud.cpp
#include "dm_hud.h"
#include "dm_log.h"
#include <stdio.h>
#include <string.h>

struct GdiCtx {
    HDC colorDC = NULL, maskDC = NULL;
    HBITMAP colorBmp = NULL, maskBmp = NULL;
    HGDIOBJ oldColor = NULL, oldMask = NULL;
    void* colorPtr = NULL;
    void* maskPtr = NULL;
    int w = 0, h = 0;

    bool Create(int W, int H) {
        w = W; h = H;
        HDC screen = GetDC(NULL);
        BITMAPINFO bi; ZeroMemory(&bi, sizeof(bi));
        bi.bmiHeader.biSize = sizeof(BITMAPINFOHEADER);
        bi.bmiHeader.biWidth = W;
        bi.bmiHeader.biHeight = -H;
        bi.bmiHeader.biPlanes = 1;
        bi.bmiHeader.biBitCount = 32;
        bi.bmiHeader.biCompression = BI_RGB;
        colorBmp = CreateDIBSection(screen, &bi, DIB_RGB_COLORS, &colorPtr, NULL, 0);
        maskBmp  = CreateDIBSection(screen, &bi, DIB_RGB_COLORS, &maskPtr, NULL, 0);
        colorDC = CreateCompatibleDC(screen);
        maskDC  = CreateCompatibleDC(screen);
        oldColor = SelectObject(colorDC, colorBmp);
        oldMask  = SelectObject(maskDC, maskBmp);
        ReleaseDC(NULL, screen);
        return colorBmp && maskBmp && colorDC && maskDC;
    }
    void Destroy() {
        if (colorDC) { SelectObject(colorDC, oldColor); DeleteDC(colorDC); colorDC = NULL; }
        if (maskDC) { SelectObject(maskDC, oldMask); DeleteDC(maskDC); maskDC = NULL; }
        if (colorBmp) { DeleteObject(colorBmp); colorBmp = NULL; }
        if (maskBmp) { DeleteObject(maskBmp); maskBmp = NULL; }
    }
    void Clear(unsigned bgColor, unsigned bgMask) {
        for (int i = 0; i < w * h; i++) ((unsigned*)colorPtr)[i] = bgColor;
        for (int i = 0; i < w * h; i++) ((unsigned*)maskPtr)[i] = bgMask;
    }
    void FillBox(int x, int y, int rw, int rh, unsigned color, unsigned mask) {
        RECT r = { x, y, x + rw, y + rh };
        HBRUSH b1 = CreateSolidBrush(RGB((color >> 16) & 0xFF, (color >> 8) & 0xFF, color & 0xFF));
        FillRect(colorDC, &r, b1);
        DeleteObject(b1);
        HBRUSH b2 = CreateSolidBrush(RGB((mask >> 16) & 0xFF, (mask >> 8) & 0xFF, mask & 0xFF));
        FillRect(maskDC, &r, b2);
        DeleteObject(b2);
    }
    void Text(int x, int y, const wchar_t* s, unsigned color, HFONT font, int maxW) {
        SetBkMode(colorDC, TRANSPARENT);
        SetBkMode(maskDC, TRANSPARENT);
        HGDIOBJ oc = SelectObject(colorDC, font);
        HGDIOBJ om = SelectObject(maskDC, font);
        SetTextColor(colorDC, RGB((color >> 16) & 0xFF, (color >> 8) & 0xFF, color & 0xFF));
        SetTextColor(maskDC, RGB(255, 255, 255));
        RECT rc = { x, y, x + maxW, y + 200 };
        DrawTextW(colorDC, s, -1, &rc, DT_LEFT | DT_TOP | DT_SINGLELINE | DT_NOPREFIX);
        rc.left = x; rc.top = y; rc.right = x + maxW; rc.bottom = y + 200;
        DrawTextW(maskDC, s, -1, &rc, DT_LEFT | DT_TOP | DT_SINGLELINE | DT_NOPREFIX);
        SelectObject(colorDC, oc);
        SelectObject(maskDC, om);
    }
};

static HFONT MakeFont(int px) {
    return CreateFontW(-px, 0, 0, 0, FW_NORMAL, 0, 0, 0, DEFAULT_CHARSET,
                       OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS, ANTIALIASED_QUALITY,
                       DEFAULT_PITCH, L"Microsoft YaHei UI");
}

static void Combine(const GdiCtx& g, std::vector<uint32_t>* out) {
    out->resize((size_t)g.w * g.h);
    const uint32_t* c = (const uint32_t*)g.colorPtr;
    const uint32_t* m = (const uint32_t*)g.maskPtr;
    for (int i = 0; i < g.w * g.h; i++) {
        uint32_t a = m[i] & 0xFF;                 // 只有灰度，取蓝通道即可
        uint32_t rgb = c[i] & 0x00FFFFFF;
        (*out)[i] = (a << 24) | rgb;
    }
}

void DmHud::Ensure(ID3D11Device* dev) {
    if (tex) return;
    D3D11_TEXTURE2D_DESC td; ZeroMemory(&td, sizeof(td));
    td.Width = w; td.Height = h; td.MipLevels = 1; td.ArraySize = 1;
    td.Format = DXGI_FORMAT_B8G8R8A8_UNORM; td.SampleDesc.Count = 1;
    td.Usage = D3D11_USAGE_DEFAULT; td.BindFlags = D3D11_BIND_SHADER_RESOURCE;
    if (FAILED(dev->CreateTexture2D(&td, NULL, &tex))) { DmLog("HUD 纹理创建失败"); return; }
    D3D11_SHADER_RESOURCE_VIEW_DESC vd; ZeroMemory(&vd, sizeof(vd));
    vd.Format = td.Format; vd.ViewDimension = D3D11_SRV_DIMENSION_TEXTURE2D; vd.Texture2D.MipLevels = 1;
    dev->CreateShaderResourceView(tex, &vd, &srv);
}

void DmHud::Rebuild(ID3D11Device* dev, ID3D11DeviceContext* ctx, const DmHudData& d, int fontPxIn) {
    Ensure(dev);
    if (!tex) return;
    fontPx = fontPxIn;

    GdiCtx g;
    if (!g.Create((int)w, (int)h)) { g.Destroy(); return; }
    g.Clear(0x000000, 0);

    HFONT f = MakeFont(fontPx);
    HFONT fTitle = MakeFont(fontPx + 2);
    const unsigned kBox = 0x1A1F2A;        // 面板底色
    const unsigned kBoxMask = 190;
    const unsigned kText = 0xE8EEF7;
    const unsigned kTextDim = 0x9FB0C6;
    const unsigned kTitle = 0x7FD4FF;

    int halfW = (int)w / 2;
    if (d.showPanel) {
        g.FillBox(6, 6, halfW - 10, (int)h - 12, kBox, kBoxMask);
        g.Text(20, 12, L"监控面板", kTitle, fTitle, halfW - 40);
        int y = 12 + fontPx + 10;
        for (auto& s : d.panel) {
            if (y > (int)h - fontPx - 16) break;
            g.Text(20, y, s.c_str(), kText, f, halfW - 40);
            y += fontPx + 8;
        }
    }
    if (d.showKeys) {
        g.FillBox(halfW + 4, 6, halfW - 10, (int)h - 12, kBox, kBoxMask);
        g.Text(halfW + 18, 12, L"快捷键", kTitle, fTitle, halfW - 40);
        int y = 12 + fontPx + 10;
        for (auto& s : d.keys) {
            if (y > (int)h - fontPx - 16) break;
            g.Text(halfW + 18, y, s.c_str(), kTextDim, f, halfW - 36);
            y += fontPx + 6;
        }
    }
    Combine(g, &pixels);
    g.Destroy();
    DeleteObject(f);
    DeleteObject(fTitle);

    D3D11_BOX box;
    box.left = 0; box.top = 0; box.front = 0;
    box.right = w; box.bottom = h; box.back = 1;
    ctx->UpdateSubresource(tex, 0, &box, pixels.data(), w * 4, 0);
}

void DmHud::Destroy() {
    if (srv) { srv->Release(); srv = NULL; }
    if (tex) { tex->Release(); tex = NULL; }
}

void DmToast::Rebuild(ID3D11Device* dev, ID3D11DeviceContext* ctx, const std::wstring& text, int fontPx) {
    if (!tex) {
        D3D11_TEXTURE2D_DESC td; ZeroMemory(&td, sizeof(td));
        td.Width = w; td.Height = h; td.MipLevels = 1; td.ArraySize = 1;
        td.Format = DXGI_FORMAT_B8G8R8A8_UNORM; td.SampleDesc.Count = 1;
        td.Usage = D3D11_USAGE_DEFAULT; td.BindFlags = D3D11_BIND_SHADER_RESOURCE;
        if (FAILED(dev->CreateTexture2D(&td, NULL, &tex))) return;
        D3D11_SHADER_RESOURCE_VIEW_DESC vd; ZeroMemory(&vd, sizeof(vd));
        vd.Format = td.Format; vd.ViewDimension = D3D11_SRV_DIMENSION_TEXTURE2D; vd.Texture2D.MipLevels = 1;
        dev->CreateShaderResourceView(tex, &vd, &srv);
    }
    GdiCtx g;
    if (!g.Create((int)w, (int)h)) { g.Destroy(); return; }
    g.Clear(0x000000, 0);
    g.FillBox(4, 4, (int)w - 8, (int)h - 8, 0x1A1F2A, 210);
    HFONT f = MakeFont(fontPx);
    g.Text(24, (int)h / 2 - fontPx, text.c_str(), 0xFFFFFF, f, (int)w - 48);
    DeleteObject(f);
    std::vector<uint32_t> px;
    Combine(g, &px);
    g.Destroy();
    D3D11_BOX box;
    box.left = 0; box.top = 0; box.front = 0;
    box.right = w; box.bottom = h; box.back = 1;
    ctx->UpdateSubresource(tex, 0, &box, px.data(), w * 4, 0);
    lastText = text;
}

void DmToast::Destroy() {
    if (srv) { srv->Release(); srv = NULL; }
    if (tex) { tex->Release(); tex = NULL; }
}

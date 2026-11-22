// dm_hud.h —— 监控面板 / 快捷键提示（GDI 画字 → D3D 纹理）
#pragma once
#include <windows.h>
#include <d3d11.h>
#include <string>
#include <vector>
#include "dm_gfx.h"

struct DmHudData {
    bool showPanel = true;
    bool showKeys = true;
    std::vector<std::wstring> panel;   // 监控面板每行
    std::vector<std::wstring> keys;    // 快捷键每行
    std::wstring toast;                // 短暂提示（空 = 不显示）
};

struct DmHud {
    ID3D11Texture2D* tex = NULL;
    ID3D11ShaderResourceView* srv = NULL;
    uint32_t w = 1400, h = 520;
    int fontPx = 24;
    std::vector<uint32_t> pixels;
    std::vector<uint8_t> colorBits;
    std::vector<uint8_t> maskBits;

    void Ensure(ID3D11Device* dev);
    void Rebuild(ID3D11Device* dev, ID3D11DeviceContext* ctx, const DmHudData& d, int fontPxIn);
    void Destroy();
    bool HasContent() const { return srv != NULL; }
};

// 小提示条（单独一张纹理）
struct DmToast {
    ID3D11Texture2D* tex = NULL;
    ID3D11ShaderResourceView* srv = NULL;
    uint32_t w = 900, h = 110;
    std::wstring lastText;
    void Rebuild(ID3D11Device* dev, ID3D11DeviceContext* ctx, const std::wstring& text, int fontPx);
    void Destroy();
};

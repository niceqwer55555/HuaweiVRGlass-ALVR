// dm_output.h —— 独立输出：在眼镜那块屏上开全屏窗口 + DXGI 交换链（不经过 SteamVR）
#pragma once
#include <windows.h>
#include <cstdint>
#include <d3d11.h>
#include <dxgi1_2.h>
#include <string>

// 找眼镜屏（3200x1600 或 HUAWEI/HWV 设备名）；找不到返回 -1
int DmFindGlassesMonitorIndex();

struct DmOutput {
    HWND hwnd = NULL;
    IDXGISwapChain1* swap = NULL;
    ID3D11Texture2D* backTex = NULL;
    ID3D11RenderTargetView* backRtv = NULL;
    uint32_t w = 0, h = 0;
    int monitorIndex = -1;
    bool exclusive = false;      // 是否已进入独占全屏

    bool Create(ID3D11Device* dev, int monitorIndex, std::string* err, bool wantExclusive = true);
    void Present(ID3D11DeviceContext* ctx, ID3D11Texture2D* src, bool vsync = true);
    void Destroy();
    bool IsValid() const { return hwnd && swap && backTex; }
};

extern const wchar_t* kDmOutputClass;

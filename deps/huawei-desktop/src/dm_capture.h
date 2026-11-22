// dm_capture.h —— DXGI Desktop Duplication 屏幕采集
#pragma once
#include <windows.h>
#include <cstdint>
#include <d3d11.h>
#include <dxgi1_2.h>
#include <string>
#include <vector>

struct DmMonitorInfo {
    std::wstring device;      // \\.\DISPLAY2
    std::wstring name;        // 友好名
    RECT rect;
    bool primary;
    uint32_t w, h;
};

std::vector<DmMonitorInfo> DmEnumMonitors();

struct DmCapture {
    ID3D11Device* dev = NULL;
    ID3D11DeviceContext* ctx = NULL;

    IDXGIOutputDuplication* dup = NULL;
    ID3D11Texture2D* tex = NULL;
    ID3D11ShaderResourceView* srv = NULL;
    uint32_t w = 0, h = 0;
    int index = -1;
    RECT rect = {0, 0, 0, 0};
    bool started = false;
    double fps = 0;
    uint64_t frames = 0;
    int lastErr = 0;

    // 光标
    ID3D11ShaderResourceView* curSrv = NULL;
    uint32_t curW = 0, curH = 0, curHotX = 0, curHotY = 0;
    HCURSOR curHandle = NULL;
    bool curVisible = false;
    float curU = 0, curV = 0;      // 0..1 相对屏幕
    float curAspect = 1.0f;        // 宽/高

    bool Start(ID3D11Device* d, ID3D11DeviceContext* c, int monitorIndex, std::string* err);
    void Stop();
    bool Acquire();                 // 有新帧则更新 tex，返回是否更新
    void UpdateCursor(ID3D11Device* d);
};

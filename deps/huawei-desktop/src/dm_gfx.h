// dm_gfx.h —— D3D11 渲染层（桌面四边形、背景、HUD 贴图）
#pragma once
#include <windows.h>
#include <d3d11.h>
#include <string>
#include <vector>
#include "dm_math.h"

struct DmVert {
    float x, y, z;     // 世界坐标
    float u, v;
};

struct DmGfx {
    ID3D11Device* dev = NULL;
    ID3D11DeviceContext* ctx = NULL;

    ID3D11Texture2D* rtTex = NULL;
    ID3D11RenderTargetView* rtv = NULL;
    uint32_t rtW = 0, rtH = 0;
    bool rtSrgb = false;

    ID3D11VertexShader* vsQuad = NULL;
    ID3D11VertexShader* vsBg = NULL;
    ID3D11PixelShader*  psQuad = NULL;
    ID3D11PixelShader*  psBg = NULL;
    ID3D11PixelShader*  psBlit = NULL;
    ID3D11InputLayout*  layout = NULL;
    ID3D11Buffer*       cb = NULL;
    ID3D11Buffer*       vb = NULL;
    ID3D11SamplerState* sampLinear = NULL;
    ID3D11SamplerState* sampPoint = NULL;
    ID3D11BlendState*   blendAlpha = NULL;
    ID3D11BlendState*   blendOpaque = NULL;
    ID3D11RasterizerState* raster = NULL;

    // 单眼离屏目标（畸变校正用）
    ID3D11Texture2D* eyeTex = NULL;
    ID3D11RenderTargetView* eyeRtv = NULL;
    ID3D11ShaderResourceView* eyeSrv = NULL;
    uint32_t eyeW = 0, eyeH = 0;

    bool Create(const std::string& shaderSrc, std::string* err);
    bool EnsureTarget(uint32_t w, uint32_t h, bool srgb);
    void Destroy();

    // 每只眼一次：设置 viewport + RT
    void BeginEye(const M4& viewRow, const M4& projRow, uint32_t x, uint32_t y, uint32_t w, uint32_t h);
    // 有一只眼的离屏渲染（之后用 BlitEyeToRT 做畸变再贴到 RT）
    bool EnsureEyeTarget(uint32_t w, uint32_t h);
    void BeginEyeOffscreen(const M4& viewRow, const M4& projRow);
    void BlitEyeToRT(uint32_t dstX, uint32_t dstW, uint32_t dstH, const float* k, float d, float scale,
                     int mode, float zoom, float mx, float my, float dx, float dy, float rotRad = 0.0f);
    void SetLens(const float* k, float d, float scale, int mode, float zoom,
                 float mx = 0, float my = 0, float dx = 0, float dy = 0, float rotRad = 0.0f);
    void EndEye();

    // 背景：dirBasis 用眼的旋转（eye→world 的三个轴）
    void DrawBackground(int mode, float brightness, const Vec3& right, const Vec3& up, const Vec3& fwd,
                        float tanX, float tanY, ID3D11ShaderResourceView* pano);
    // 贴图四边形（世界坐标顶点）
    void DrawQuad(const DmVert* verts, int n, ID3D11ShaderResourceView* srv, float opacity, bool pointSample);
    // 设置下一次 DrawQuad 用的 UV 子矩形（SBS 分眼 / 视差微调都在这里）
    void SetUv(float uMin, float vMin, float uMax, float vMax);
    void SetCursorRect(float u0, float v0, float u1, float v1);   // 指针在桌面 UV 里的矩形（0,0,0,0=无）
    void SetCursorTexture(ID3D11ShaderResourceView* srv);
    // 面板像素坐标画纯色矩形（校准图案）
    void FillRectPanel(float px, float py, float pw, float ph, float R, float G, float B, float A = 1.0f);

    // 贴图工具
    ID3D11ShaderResourceView* TexFromBGRA(const void* pixels, uint32_t w, uint32_t h, bool pointSample);
    ID3D11ShaderResourceView* TexFromFileWic(const std::wstring& path, std::string* err);
    // 官方 skybox：未压缩 32 位 DDS（BGRA）
    ID3D11ShaderResourceView* TexFromDDS(const std::wstring& path, std::string* err);
    void ReleaseSrv(ID3D11ShaderResourceView* srv);

    void* RTTexture() const { return rtTex; }
private:
    float g_vp[16] = {0};
    float g_uv[4] = {0, 0, 1, 1};
    float g_cur[4] = {0, 0, 0, 0};
    ID3D11ShaderResourceView* g_curSrv = NULL;
    void UpdateCB(const M4& vp, const float* tint, const float* uv,
                  const Vec3& rx, float tanX, const Vec3& uy, float tanY, const Vec3& fz, float bgMode);
};

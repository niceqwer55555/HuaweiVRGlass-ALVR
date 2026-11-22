// dm_gfx.cpp
#include "dm_gfx.h"
#include "dm_log.h"
#include <d3dcompiler.h>
#include <wincodec.h>
#include <stdio.h>

#pragma comment(lib, "d3dcompiler")
#pragma comment(lib, "windowscodecs")

static const char* kHLSL = R"(
cbuffer Frame : register(b0)
{
    row_major float4x4 gVP;
    float4 gTint;     // rgb 染色, a 透明度
    float4 gUv;       // umin, vmin, umax, vmax
    float4 gCur;      // 指针矩形 umin,vmin,umax,vmax（gCurOn>0.5 时生效）
    float4 gRayX;     // 眼的右向量(xyz) + tanHalfX
    float4 gRayY;     // 眼的上向量(xyz) + tanHalfY
    float4 gFwd;      // 眼的前向量(xyz) + 背景模式
    float4 gP;        // 亮度, 时间, 保留, 保留
    float4 gLens;     // 畸变 k1, k2, 1/缩放, 立体视差
    float4 gLens2;    // 官方: k3, k4, k5, d
    float4 gLens3;    // 官方: k0, scale, 模式(1=官方多项式), 保留
    float4 gEye;      // 单眼面板校准: 镜像X, 镜像Y, 偏移X, 偏移Y
    float4 gEye2;     // 单眼面板校准: 旋转(弧度), 保留x3
};

struct VSIn { float3 pos : POSITION; float2 uv : TEXCOORD0; };
struct VSOut { float4 pos : SV_POSITION; float2 uv : TEXCOORD0; };

Texture2D gTex : register(t0);
Texture2D gCurTex : register(t1);
SamplerState gSamp : register(s0);

VSOut VSQuad(VSIn i)
{
    VSOut o;
    o.pos = mul(float4(i.pos, 1.0), gVP);
    o.uv = i.uv;
    return o;
}

float4 PSQuad(VSOut i) : SV_TARGET
{
    if (gUv.x < -0.5) return float4(gTint.rgb, gTint.a);   // 纯色矩形（校准图案/面板空间 UI）
    float2 uv = float2(lerp(gUv.x, gUv.z, i.uv.x), lerp(gUv.y, gUv.w, i.uv.y));
    float4 c = gTex.Sample(gSamp, uv);
    // 指针已合成进桌面：在桌面 UV 空间里叠加，跟着曲面一起弯
    if (gCur.z > gCur.x && gCur.w > gCur.y &&
        uv.x >= gCur.x && uv.x <= gCur.z && uv.y >= gCur.y && uv.y <= gCur.w) {
        float2 cuv = (uv - gCur.xy) / (gCur.zw - gCur.xy);
        float4 cc = gCurTex.Sample(gSamp, cuv);
        c.rgb = lerp(c.rgb, cc.rgb, cc.a);
        c.a = max(c.a, cc.a);
    }
    c.rgb *= gTint.rgb;
    c.a *= gTint.a;
    return c;
}

struct BgOut { float4 pos : SV_POSITION; float2 ndc : TEXCOORD0; };

BgOut VSBg(uint id : SV_VertexID)
{
    float2 p[3] = { float2(-1.0, -1.0), float2(3.0, -1.0), float2(-1.0, 3.0) };
    BgOut o;
    o.pos = float4(p[id], 1.0, 1.0);
    o.ndc = p[id];
    return o;
}

// ---- 镜头畸变补偿：把整只眼的画面当纹理，按径向多项式重采样后再铺到面板 ----
float4 PSBlit(BgOut i) : SV_TARGET
{
    float2 c = i.ndc;                       // -1..1
    float2 s;
    if (gLens3.z > 0.5) {
        // 官方 HWVR::HallidayDistortMesh 模型：r² 的五次多项式
        //   q = 面板坐标(米) / d        f = k0 + k1*r2 + k2*r2² + ... + k5*r2⁵
        //   采样点 = f * q / scale
        float kk = 0.0192 / gLens2.w;       // 半面板 0.0192 m / d
        float2 q = c * kk;
        float r2o = dot(q, q);
        if (r2o > 1.0) r2o = 1.0;                   // 圆外按圆边处理，避免抹黑背景
        float fo = gLens3.x + r2o * (gLens.x + r2o * (gLens.y + r2o * (gLens2.x + r2o * (gLens2.y + r2o * gLens2.z))));
        s = fo * q / gLens3.y * gLens.z;    // gLens.z = 1/额外缩放
        s = clamp(s, -1.0, 1.0);            // 采样点夹在画面内（边缘拉伸，不动背景）
    } else {
        float r2 = dot(c, c);
        float f = 1.0 + gLens.x * r2 + gLens.y * r2 * r2;
        s = c * f * gLens.z;                // gLens.z = 1/放大倍数
    }
    float2 q = s;
    if (abs(gEye2.x) > 0.0001) {             // 单眼朝向（官方：右眼相对左眼 180°）
        float ca = cos(gEye2.x), sa = sin(gEye2.x);
        q = float2(q.x * ca - q.y * sa, q.x * sa + q.y * ca);
    }
    if (gEye.x > 0.5) q.x = -q.x;            // 左右镜像
    if (gEye.y > 0.5) q.y = -q.y;            // 上下镜像
    q.x += gEye.z; q.y += gEye.w;            // 像素级微调（归一化）
    float2 uv = float2(q.x * 0.5 + 0.5, 1.0 - (q.y * 0.5 + 0.5));
    if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0) return float4(0, 0, 0, 1);
    return gTex.Sample(gSamp, uv);
}

float4 PSBg(BgOut i) : SV_TARGET
{
    float3 d = normalize(gFwd.xyz + gRayX.xyz * (i.ndc.x * gRayX.w) + gRayY.xyz * (i.ndc.y * gRayY.w));
    float br = gP.x;
    int mode = (int)gFwd.w;
    float3 rgb;
    if (mode == 2) {
        float u = atan2(d.x, -d.z) / 6.2831853 + 0.5;
        float v = acos(clamp(d.y, -1.0, 1.0)) / 3.14159265;
        rgb = gTex.Sample(gSamp, float2(u, v)).rgb;
    } else if (mode >= 3) {
        // 3=360 2D  4=180 2D  5=360 左眼  6=360 右眼  7=180 左眼  8=180 右眼
        bool stereo = (mode >= 5);
        bool half180 = (mode == 4) || (mode >= 7);
        float az = atan2(d.x, -d.z);
        float u;
        if (half180) {
            // 180° 图：背面镜像一份，形成完整 360° 视野
            float a = abs(az);                       // 0..pi
            if (a > 1.5707963) a = 3.14159265 - a;    // |az|>90° 折回去
            u = a / 3.14159265 + 0.5;                 // 0..1
        } else {
            u = az / 6.2831853 + 0.5;
        }
        float v = acos(clamp(d.y, -1.0, 1.0)) / 3.14159265;
        if (stereo) {
            float uu = u * 0.5 + ((mode == 6 || mode == 8) ? 0.5 : 0.0);
            rgb = gTex.Sample(gSamp, float2(uu, v)).rgb;
        } else {
            rgb = gTex.Sample(gSamp, float2(u, v)).rgb;
        }
    } else if (mode == 9) {
        rgb = float3(0.0, 0.0, 0.0);            // 纯黑 360°（黑底）
    } else if (mode == 1) {
        float h = clamp(d.y, -1.0, 1.0);
        float3 zenith = float3(0.04, 0.07, 0.15);
        float3 horiz  = float3(0.33, 0.43, 0.60);
        float3 ground = float3(0.045, 0.045, 0.055);
        float t = saturate(h * 1.6);
        float3 sky = lerp(horiz, zenith, pow(t, 0.6));
        // 地平线处两侧取同一颜色，向下用更缓的幂次过渡到地面，衔接不断层
        float hb = saturate(-h * 2.2);
        float3 below = lerp(horiz * 0.92, ground, pow(hb, 0.75));
        rgb = (h >= 0.0) ? sky : below;
        float3 sunDir = normalize(float3(-0.45, 0.55, -0.70));
        float sd = saturate(dot(d, sunDir));
        rgb += float3(1.00, 0.85, 0.60) * pow(sd, 200.0) * 1.5;
        rgb += float3(0.50, 0.55, 0.70) * pow(sd, 6.0) * 0.22;
        // 角向特征：星空 + 地面网格（用于确认背景是否世界锁定/随头旋转）
        if (h > 0.02) {
            float3 sp = d * 200.0;
            float3 cell = floor(sp);
            float3 f = frac(sp) - 0.5;
            float hh = frac(sin(dot(cell, float3(12.9898, 78.233, 37.719))) * 43758.5453);
            if (hh > 0.982 && dot(f, f) < 0.10)
                rgb += float3(0.85, 0.90, 1.00) * saturate(h * 3.0) * 0.9;
        }
        // 下半部分只保留平滑渐变（原来叠加的地面网格已去掉）
    } else {
        float t = saturate(d.y * 0.5 + 0.5);
        float3 top = float3(0.020, 0.028, 0.055);
        float3 bot = float3(0.135, 0.150, 0.200);
        rgb = lerp(bot, top, pow(t, 1.3));
    }
    return float4(rgb * br, 1.0);
}
)";

struct DmCB {
    float vp[16];
    float tint[4];
    float uv[4];
    float cur[4];
    float rayX[4];
    float rayY[4];
    float fwd[4];
    float p[4];
    float lens[4];
    float lens2[4];
    float lens3[4];
    float eye[4];
    float eye2[4];
};

static DmCB g_cbCache;

static bool Compile(const char* src, const char* entry, const char* target, ID3DBlob** blob, std::string* err) {
    ID3DBlob* e = NULL;
    UINT flags = D3DCOMPILE_ENABLE_STRICTNESS | D3DCOMPILE_OPTIMIZATION_LEVEL3;
    HRESULT hr = D3DCompile(src, strlen(src), "dm", NULL, NULL, entry, target, flags, 0, blob, &e);
    if (FAILED(hr)) {
        if (err) {
            *err = "着色器编译失败: ";
            if (e) *err += (const char*)e->GetBufferPointer();
        }
        if (e) e->Release();
        return false;
    }
    if (e) e->Release();
    return true;
}

bool DmGfx::Create(const std::string& shaderSrc, std::string* err) {
    UINT flags = D3D11_CREATE_DEVICE_BGRA_SUPPORT;
    D3D_FEATURE_LEVEL fl;
    D3D_FEATURE_LEVEL want[] = { D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_10_0 };
    HRESULT hr = D3D11CreateDevice(NULL, D3D_DRIVER_TYPE_HARDWARE, NULL, flags, want, 2,
                                   D3D11_SDK_VERSION, &dev, &fl, &ctx);
    if (FAILED(hr)) { if (err) *err = "D3D11CreateDevice 失败"; return false; }

    const char* src = shaderSrc.empty() ? kHLSL : shaderSrc.c_str();
    ID3DBlob* vsb = NULL; ID3DBlob* psb = NULL; ID3DBlob* bgb = NULL; ID3DBlob* pbg = NULL; ID3DBlob* blb = NULL;
    if (!Compile(src, "VSQuad", "vs_4_0", &vsb, err)) return false;
    if (!Compile(src, "PSQuad", "ps_4_0", &psb, err)) return false;
    if (!Compile(src, "VSBg", "vs_4_0", &bgb, err)) return false;
    if (!Compile(src, "PSBg", "ps_4_0", &pbg, err)) return false;
    if (!Compile(src, "PSBlit", "ps_4_0", &blb, err)) return false;

    dev->CreateVertexShader(vsb->GetBufferPointer(), vsb->GetBufferSize(), NULL, &vsQuad);
    dev->CreatePixelShader(psb->GetBufferPointer(), psb->GetBufferSize(), NULL, &psQuad);
    dev->CreateVertexShader(bgb->GetBufferPointer(), bgb->GetBufferSize(), NULL, &vsBg);
    dev->CreatePixelShader(pbg->GetBufferPointer(), pbg->GetBufferSize(), NULL, &psBg);
    dev->CreatePixelShader(blb->GetBufferPointer(), blb->GetBufferSize(), NULL, &psBlit);

    D3D11_INPUT_ELEMENT_DESC il[] = {
        { "POSITION", 0, DXGI_FORMAT_R32G32B32_FLOAT, 0, 0, D3D11_INPUT_PER_VERTEX_DATA, 0 },
        { "TEXCOORD", 0, DXGI_FORMAT_R32G32_FLOAT, 0, 12, D3D11_INPUT_PER_VERTEX_DATA, 0 },
    };
    dev->CreateInputLayout(il, 2, vsb->GetBufferPointer(), vsb->GetBufferSize(), &layout);
    vsb->Release(); psb->Release(); bgb->Release(); pbg->Release();

    D3D11_BUFFER_DESC bd; ZeroMemory(&bd, sizeof(bd));
    bd.Usage = D3D11_USAGE_DYNAMIC; bd.ByteWidth = sizeof(DmCB);
    bd.BindFlags = D3D11_BIND_CONSTANT_BUFFER; bd.CPUAccessFlags = D3D11_CPU_ACCESS_WRITE;
    dev->CreateBuffer(&bd, NULL, &cb);

    bd.ByteWidth = sizeof(DmVert) * 16384;
    bd.BindFlags = D3D11_BIND_VERTEX_BUFFER;
    dev->CreateBuffer(&bd, NULL, &vb);

    D3D11_SAMPLER_DESC sd; ZeroMemory(&sd, sizeof(sd));
    sd.Filter = D3D11_FILTER_ANISOTROPIC;
    sd.MaxAnisotropy = 8;
    sd.AddressU = sd.AddressV = sd.AddressW = D3D11_TEXTURE_ADDRESS_CLAMP;
    sd.ComparisonFunc = D3D11_COMPARISON_NEVER;
    sd.MaxLOD = D3D11_FLOAT32_MAX;
    dev->CreateSamplerState(&sd, &sampLinear);
    sd.Filter = D3D11_FILTER_MIN_MAG_MIP_POINT;
    sd.MaxAnisotropy = 1;
    dev->CreateSamplerState(&sd, &sampPoint);

    D3D11_BLEND_DESC bld; ZeroMemory(&bld, sizeof(bld));
    bld.RenderTarget[0].BlendEnable = TRUE;
    bld.RenderTarget[0].SrcBlend = D3D11_BLEND_SRC_ALPHA;
    bld.RenderTarget[0].DestBlend = D3D11_BLEND_INV_SRC_ALPHA;
    bld.RenderTarget[0].BlendOp = D3D11_BLEND_OP_ADD;
    bld.RenderTarget[0].SrcBlendAlpha = D3D11_BLEND_ONE;
    bld.RenderTarget[0].DestBlendAlpha = D3D11_BLEND_INV_SRC_ALPHA;
    bld.RenderTarget[0].BlendOpAlpha = D3D11_BLEND_OP_ADD;
    bld.RenderTarget[0].RenderTargetWriteMask = D3D11_COLOR_WRITE_ENABLE_ALL;
    dev->CreateBlendState(&bld, &blendAlpha);
    bld.RenderTarget[0].BlendEnable = FALSE;
    dev->CreateBlendState(&bld, &blendOpaque);

    D3D11_RASTERIZER_DESC rd; ZeroMemory(&rd, sizeof(rd));
    rd.FillMode = D3D11_FILL_SOLID;
    rd.CullMode = D3D11_CULL_NONE;
    rd.DepthClipEnable = TRUE;
    dev->CreateRasterizerState(&rd, &raster);
    return true;
}

void DmGfx::Destroy() {
    if (rtv) { rtv->Release(); rtv = NULL; }
    if (rtTex) { rtTex->Release(); rtTex = NULL; }
    if (vsQuad) { vsQuad->Release(); vsQuad = NULL; }
    if (vsBg) { vsBg->Release(); vsBg = NULL; }
    if (psQuad) { psQuad->Release(); psQuad = NULL; }
    if (psBg) { psBg->Release(); psBg = NULL; }
    if (psBlit) { psBlit->Release(); psBlit = NULL; }
    if (eyeSrv) { eyeSrv->Release(); eyeSrv = NULL; }
    if (eyeRtv) { eyeRtv->Release(); eyeRtv = NULL; }
    if (eyeTex) { eyeTex->Release(); eyeTex = NULL; }
    if (layout) { layout->Release(); layout = NULL; }
    if (cb) { cb->Release(); cb = NULL; }
    if (vb) { vb->Release(); vb = NULL; }
    if (sampLinear) { sampLinear->Release(); sampLinear = NULL; }
    if (sampPoint) { sampPoint->Release(); sampPoint = NULL; }
    if (blendAlpha) { blendAlpha->Release(); blendAlpha = NULL; }
    if (blendOpaque) { blendOpaque->Release(); blendOpaque = NULL; }
    if (raster) { raster->Release(); raster = NULL; }
    if (ctx) { ctx->Release(); ctx = NULL; }
    if (dev) { dev->Release(); dev = NULL; }
}

bool DmGfx::EnsureTarget(uint32_t w, uint32_t h, bool srgb) {
    if (rtTex && rtW == w && rtH == h && rtSrgb == srgb) return true;
    if (rtv) { rtv->Release(); rtv = NULL; }
    if (rtTex) { rtTex->Release(); rtTex = NULL; }
    D3D11_TEXTURE2D_DESC td; ZeroMemory(&td, sizeof(td));
    td.Width = w; td.Height = h; td.MipLevels = 1; td.ArraySize = 1;
    // ★ 必须和交换链后备缓冲同格式（B8G8R8A8），否则 Present 前那次 CopyResource
    //   会静默失败 → 屏上永远是全黑（渲染目标里却有图，转存出来是好的）。
    td.Format = srgb ? DXGI_FORMAT_B8G8R8A8_UNORM_SRGB : DXGI_FORMAT_B8G8R8A8_UNORM;
    td.SampleDesc.Count = 1;
    td.Usage = D3D11_USAGE_DEFAULT;
    td.BindFlags = D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE;
    HRESULT hr = dev->CreateTexture2D(&td, NULL, &rtTex);
    if (FAILED(hr)) { DmLog("CreateTexture2D(RT) 失败 hr=0x%08lX", (unsigned long)hr); return false; }
    hr = dev->CreateRenderTargetView(rtTex, NULL, &rtv);
    if (FAILED(hr)) { DmLog("CreateRenderTargetView 失败"); return false; }
    rtW = w; rtH = h; rtSrgb = srgb;
    DmLog("渲染目标 %ux%u srgb=%d", w, h, srgb ? 1 : 0);
    return true;
}

void DmGfx::BeginEye(const M4& viewRow, const M4& projRow, uint32_t x, uint32_t y, uint32_t w, uint32_t h) {
    M4 vp = Mul(viewRow, projRow);
    D3D11_VIEWPORT vp4;
    vp4.TopLeftX = (float)x; vp4.TopLeftY = (float)y;
    vp4.Width = (float)w; vp4.Height = (float)h;
    vp4.MinDepth = 0.0f; vp4.MaxDepth = 1.0f;
    ctx->RSSetViewports(1, &vp4);
    ctx->RSSetScissorRects(0, NULL);
    ctx->RSSetState(raster);
    ID3D11RenderTargetView* rtvs[1] = { rtv };
    ctx->OMSetRenderTargets(1, rtvs, NULL);
    ctx->OMSetBlendState(blendOpaque, NULL, 0xFFFFFFFF);
    ctx->IASetInputLayout(layout);
    ctx->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
    ctx->VSSetConstantBuffers(0, 1, &cb);
    ctx->PSSetConstantBuffers(0, 1, &cb);
    ctx->PSSetSamplers(0, 1, &sampLinear);
    // 存起来，供背景/四边形使用
    memcpy(g_vp, vp.m, sizeof(g_vp));
}

void DmGfx::UpdateCB(const M4& vp, const float* tint, const float* uv,
                     const Vec3& rx, float tanX, const Vec3& uy, float tanY, const Vec3& fz, float bgMode) {
    DmCB c;
    memcpy(c.vp, vp.m, sizeof(c.vp));
    c.tint[0] = tint ? tint[0] : 1.0f;
    c.tint[1] = tint ? tint[1] : 1.0f;
    c.tint[2] = tint ? tint[2] : 1.0f;
    c.tint[3] = tint ? tint[3] : 1.0f;
    c.uv[0] = uv ? uv[0] : 0.0f;
    c.uv[1] = uv ? uv[1] : 0.0f;
    c.uv[2] = uv ? uv[2] : 1.0f;
    c.uv[3] = uv ? uv[3] : 1.0f;
    c.rayX[0] = rx.x; c.rayX[1] = rx.y; c.rayX[2] = rx.z; c.rayX[3] = tanX;
    c.rayY[0] = uy.x; c.rayY[1] = uy.y; c.rayY[2] = uy.z; c.rayY[3] = tanY;
    c.fwd[0] = fz.x; c.fwd[1] = fz.y; c.fwd[2] = fz.z; c.fwd[3] = bgMode;
    c.p[0] = 1.0f; c.p[1] = 0.0f; c.p[2] = 0.0f; c.p[3] = 0.0f;
    c.lens[0] = g_cbCache.lens[0]; c.lens[1] = g_cbCache.lens[1];
    c.lens[2] = g_cbCache.lens[2]; c.lens[3] = g_cbCache.lens[3];
    for (int i = 0; i < 4; i++) c.eye[i] = g_cbCache.eye[i];
    for (int i = 0; i < 4; i++) c.eye2[i] = g_cbCache.eye2[i];
    g_cbCache = c;
    D3D11_MAPPED_SUBRESOURCE ms;
    if (SUCCEEDED(ctx->Map(cb, 0, D3D11_MAP_WRITE_DISCARD, 0, &ms))) {
        memcpy(ms.pData, &c, sizeof(c));
        ctx->Unmap(cb, 0);
    }
}

void DmGfx::SetLens(const float* k, float d, float scale, int mode, float zoom,
                    float mx, float my, float dx, float dy, float rotRad) {
    g_cbCache.lens[0] = k[1];
    g_cbCache.lens[1] = k[2];
    g_cbCache.lens[2] = (zoom > 0.05f) ? (1.0f / zoom) : 1.0f;
    g_cbCache.lens[3] = 0.0f;
    g_cbCache.lens2[0] = k[3];
    g_cbCache.lens2[1] = k[4];
    g_cbCache.lens2[2] = k[5];
    g_cbCache.lens2[3] = (d > 1e-5f) ? d : 0.01807f;
    g_cbCache.lens3[0] = k[0];
    g_cbCache.lens3[1] = (scale > 1e-5f) ? scale : 1.5165f;
    g_cbCache.lens3[2] = (float)mode;
    g_cbCache.lens3[3] = 0.0f;
    g_cbCache.eye[0] = mx; g_cbCache.eye[1] = my;
    g_cbCache.eye[2] = dx; g_cbCache.eye[3] = dy;
    g_cbCache.eye2[0] = rotRad; g_cbCache.eye2[1] = 0; g_cbCache.eye2[2] = 0; g_cbCache.eye2[3] = 0;
    D3D11_MAPPED_SUBRESOURCE ms;
    if (SUCCEEDED(ctx->Map(cb, 0, D3D11_MAP_WRITE_DISCARD, 0, &ms))) {
        memcpy(ms.pData, &g_cbCache, sizeof(g_cbCache));
        ctx->Unmap(cb, 0);
    }
}

bool DmGfx::EnsureEyeTarget(uint32_t w, uint32_t h) {
    if (eyeTex && eyeW == w && eyeH == h) return true;
    if (eyeSrv) { eyeSrv->Release(); eyeSrv = NULL; }
    if (eyeRtv) { eyeRtv->Release(); eyeRtv = NULL; }
    if (eyeTex) { eyeTex->Release(); eyeTex = NULL; }
    D3D11_TEXTURE2D_DESC td; ZeroMemory(&td, sizeof(td));
    td.Width = w; td.Height = h; td.MipLevels = 1; td.ArraySize = 1;
    td.Format = rtSrgb ? DXGI_FORMAT_B8G8R8A8_UNORM_SRGB : DXGI_FORMAT_B8G8R8A8_UNORM;
    td.SampleDesc.Count = 1;
    td.Usage = D3D11_USAGE_DEFAULT;
    td.BindFlags = D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE;
    HRESULT hr = dev->CreateTexture2D(&td, NULL, &eyeTex);
    if (FAILED(hr)) { DmLog("CreateTexture2D(eye) 失败 0x%08lX", (unsigned long)hr); return false; }
    if (FAILED(dev->CreateRenderTargetView(eyeTex, NULL, &eyeRtv))) return false;
    if (FAILED(dev->CreateShaderResourceView(eyeTex, NULL, &eyeSrv))) return false;
    eyeW = w; eyeH = h;
    return true;
}

void DmGfx::BeginEyeOffscreen(const M4& viewRow, const M4& projRow) {
    M4 vp = Mul(viewRow, projRow);
    D3D11_VIEWPORT v;
    v.TopLeftX = 0; v.TopLeftY = 0;
    v.Width = (float)eyeW; v.Height = (float)eyeH;
    v.MinDepth = 0.0f; v.MaxDepth = 1.0f;
    ctx->RSSetViewports(1, &v);
    ctx->RSSetScissorRects(0, NULL);
    ctx->RSSetState(raster);
    ID3D11RenderTargetView* rtvs[1] = { eyeRtv };
    ctx->OMSetRenderTargets(1, rtvs, NULL);
    ctx->OMSetBlendState(blendOpaque, NULL, 0xFFFFFFFF);
    ctx->IASetInputLayout(layout);
    ctx->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
    ctx->VSSetConstantBuffers(0, 1, &cb);
    ctx->PSSetConstantBuffers(0, 1, &cb);
    ctx->PSSetSamplers(0, 1, &sampLinear);
    memcpy(g_vp, vp.m, sizeof(g_vp));
}

void DmGfx::BlitEyeToRT(uint32_t dstX, uint32_t dstW, uint32_t dstH, const float* k, float d, float scale,
                        int mode, float zoom, float mx, float my, float dx, float dy, float rotRad) {
    if (!eyeSrv || !rtv) return;
    D3D11_VIEWPORT v;
    v.TopLeftX = (float)dstX; v.TopLeftY = 0.0f;
    v.Width = (float)dstW; v.Height = (float)dstH;   // 目标半屏尺寸（不是超采样后的纹理尺寸）
    v.MinDepth = 0.0f; v.MaxDepth = 1.0f;
    ctx->RSSetViewports(1, &v);
    ID3D11RenderTargetView* rtvs[1] = { rtv };
    ctx->OMSetRenderTargets(1, rtvs, NULL);
    ctx->OMSetBlendState(blendOpaque, NULL, 0xFFFFFFFF);
    ctx->IASetInputLayout(NULL);
    ctx->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
    ctx->VSSetShader(vsBg, NULL, 0);
    ctx->PSSetShader(psBlit, NULL, 0);
    ctx->VSSetConstantBuffers(0, 1, &cb);
    ctx->PSSetConstantBuffers(0, 1, &cb);
    ctx->PSSetSamplers(0, 1, &sampLinear);
    SetLens(k, d, scale, mode, zoom, mx, my, dx, dy, rotRad);
    ID3D11ShaderResourceView* srvs[1] = { eyeSrv };
    ctx->PSSetShaderResources(0, 1, srvs);
    ctx->Draw(3, 0);
    ID3D11ShaderResourceView* none[1] = { NULL };
    ctx->PSSetShaderResources(0, 1, none);
}

void DmGfx::DrawBackground(int mode, float brightness, const Vec3& right, const Vec3& up, const Vec3& fwd,
                           float tanX, float tanY, ID3D11ShaderResourceView* pano) {
    DmCB c;
    memcpy(c.vp, g_vp, sizeof(c.vp));
    c.tint[0] = c.tint[1] = c.tint[2] = c.tint[3] = 1.0f;
    c.uv[0] = 0; c.uv[1] = 0; c.uv[2] = 1; c.uv[3] = 1;
    c.rayX[0] = right.x; c.rayX[1] = right.y; c.rayX[2] = right.z; c.rayX[3] = tanX;
    c.rayY[0] = up.x; c.rayY[1] = up.y; c.rayY[2] = up.z; c.rayY[3] = tanY;
    c.fwd[0] = fwd.x; c.fwd[1] = fwd.y; c.fwd[2] = fwd.z; c.fwd[3] = (float)mode;
    c.p[0] = brightness; c.p[1] = 0; c.p[2] = 0; c.p[3] = 0;
    D3D11_MAPPED_SUBRESOURCE ms;
    if (SUCCEEDED(ctx->Map(cb, 0, D3D11_MAP_WRITE_DISCARD, 0, &ms))) {
        memcpy(ms.pData, &c, sizeof(c));
        ctx->Unmap(cb, 0);
    }
    ctx->IASetInputLayout(NULL);
    ctx->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
    ctx->VSSetShader(vsBg, NULL, 0);
    ctx->PSSetShader(psBg, NULL, 0);
    if (pano) ctx->PSSetShaderResources(0, 1, &pano);
    ctx->OMSetBlendState(blendOpaque, NULL, 0xFFFFFFFF);
    ctx->Draw(3, 0);
    ctx->IASetInputLayout(layout);
}

// 面板像素坐标画纯色矩形（直接写 RT，用于校准图案）
void DmGfx::FillRectPanel(float px, float py, float pw, float ph, float R, float G, float B, float A) {
    if (!rtv || rtW == 0 || rtH == 0) return;
    D3D11_VIEWPORT v;
    v.TopLeftX = 0; v.TopLeftY = 0; v.Width = (float)rtW; v.Height = (float)rtH;
    v.MinDepth = 0.0f; v.MaxDepth = 1.0f;
    ctx->RSSetViewports(1, &v);
    ID3D11RenderTargetView* rtvs[1] = { rtv };
    ctx->OMSetRenderTargets(1, rtvs, NULL);
    ctx->OMSetBlendState(blendOpaque, NULL, 0xFFFFFFFF);
    ctx->IASetInputLayout(layout);
    ctx->IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
    ctx->VSSetShader(vsQuad, NULL, 0);
    ctx->PSSetShader(psQuad, NULL, 0);
    ctx->PSSetSamplers(0, 1, &sampPoint);
    float nx0 = px / (float)rtW * 2.0f - 1.0f;
    float nx1 = (px + pw) / (float)rtW * 2.0f - 1.0f;
    float ny0 = 1.0f - py / (float)rtH * 2.0f;
    float ny1 = 1.0f - (py + ph) / (float)rtH * 2.0f;
    DmVert v6[6] = {
        { nx0, ny0, 0, 0, 0 }, { nx1, ny1, 0, 1, 1 }, { nx1, ny0, 0, 1, 0 },
        { nx0, ny0, 0, 0, 0 }, { nx0, ny1, 0, 0, 1 }, { nx1, ny1, 0, 1, 1 },
    };
    D3D11_MAPPED_SUBRESOURCE ms;
    if (FAILED(ctx->Map(vb, 0, D3D11_MAP_WRITE_DISCARD, 0, &ms))) return;
    memcpy(ms.pData, v6, sizeof(v6));
    ctx->Unmap(vb, 0);
    UINT stride = sizeof(DmVert), off = 0;
    ctx->IASetVertexBuffers(0, 1, &vb, &stride, &off);
    DmCB c;
    ZeroMemory(&c, sizeof(c));
    const float ident[16] = { 1,0,0,0, 0,1,0,0, 0,0,1,0, 0,0,0,1 };
    memcpy(c.vp, ident, sizeof(ident));
    c.tint[0] = R; c.tint[1] = G; c.tint[2] = B; c.tint[3] = A;
    c.uv[0] = -1.0f; c.uv[1] = -1.0f; c.uv[2] = -1.0f; c.uv[3] = -1.0f;
    if (SUCCEEDED(ctx->Map(cb, 0, D3D11_MAP_WRITE_DISCARD, 0, &ms))) {
        memcpy(ms.pData, &c, sizeof(c));
        ctx->Unmap(cb, 0);
    }
    ctx->Draw(6, 0);
}

void DmGfx::DrawQuad(const DmVert* verts, int n, ID3D11ShaderResourceView* srv, float opacity, bool pointSample) {
    if (!srv || n <= 0) return;
    D3D11_MAPPED_SUBRESOURCE ms;
    if (FAILED(ctx->Map(vb, 0, D3D11_MAP_WRITE_DISCARD, 0, &ms))) return;
    memcpy(ms.pData, verts, sizeof(DmVert) * n);
    ctx->Unmap(vb, 0);
    UINT stride = sizeof(DmVert), offset = 0;
    ctx->IASetVertexBuffers(0, 1, &vb, &stride, &offset);
    ctx->IASetInputLayout(layout);
    ctx->VSSetShader(vsQuad, NULL, 0);
    ctx->PSSetShader(psQuad, NULL, 0);
    ctx->PSSetShaderResources(0, 1, &srv);
    ctx->PSSetSamplers(0, 1, pointSample ? &sampPoint : &sampLinear);
    ctx->OMSetBlendState(blendAlpha, NULL, 0xFFFFFFFF);
    // 染色/UV 由调用者在 BeginEye 之后设置（见 SetQuadParams）
    const float tint[4] = { 1, 1, 1, opacity };
    const float uv[4] = { g_uv[0], g_uv[1], g_uv[2], g_uv[3] };
    const bool useCur = (g_curSrv != NULL && g_cur[2] > g_cur[0] && g_cur[3] > g_cur[1]);
    if (useCur) { ID3D11ShaderResourceView* cs = g_curSrv; ctx->PSSetShaderResources(1, 1, &cs); }
    DmCB c;
    memcpy(c.vp, g_vp, sizeof(c.vp));
    memcpy(c.tint, tint, sizeof(tint));
    memcpy(c.uv, uv, sizeof(uv));
    const float cur[4] = { useCur ? g_cur[0] : 0.0f, useCur ? g_cur[1] : 0.0f,
                           useCur ? g_cur[2] : 0.0f, useCur ? g_cur[3] : 0.0f };
    memcpy(c.cur, cur, sizeof(cur));
    c.rayX[0] = c.rayX[1] = c.rayX[2] = c.rayX[3] = 0;
    c.rayY[0] = c.rayY[1] = c.rayY[2] = c.rayY[3] = 0;
    c.fwd[0] = c.fwd[1] = c.fwd[2] = c.fwd[3] = 0;
    c.p[0] = 1; c.p[1] = 0; c.p[2] = 0; c.p[3] = 0;
    if (SUCCEEDED(ctx->Map(cb, 0, D3D11_MAP_WRITE_DISCARD, 0, &ms))) {
        memcpy(ms.pData, &c, sizeof(c));
        ctx->Unmap(cb, 0);
    }
    ctx->Draw((UINT)n, 0);
}

void DmGfx::EndEye() {
    ctx->OMSetRenderTargets(0, NULL, NULL);
}

void DmGfx::SetCursorTexture(ID3D11ShaderResourceView* srv) { g_curSrv = srv; }

void DmGfx::SetCursorRect(float u0, float v0, float u1, float v1) {
    g_cur[0] = u0; g_cur[1] = v0; g_cur[2] = u1; g_cur[3] = v1;
}

void DmGfx::SetUv(float uMin, float vMin, float uMax, float vMax) {
    g_uv[0] = uMin; g_uv[1] = vMin; g_uv[2] = uMax; g_uv[3] = vMax;
}

ID3D11ShaderResourceView* DmGfx::TexFromBGRA(const void* pixels, uint32_t w, uint32_t h, bool pointSample) {
    (void)pointSample;
    D3D11_TEXTURE2D_DESC td; ZeroMemory(&td, sizeof(td));
    td.Width = w; td.Height = h; td.MipLevels = 1; td.ArraySize = 1;
    td.Format = DXGI_FORMAT_B8G8R8A8_UNORM;
    td.SampleDesc.Count = 1;
    td.Usage = D3D11_USAGE_DEFAULT;
    td.BindFlags = D3D11_BIND_SHADER_RESOURCE;
    D3D11_SUBRESOURCE_DATA sd; ZeroMemory(&sd, sizeof(sd));
    sd.pSysMem = pixels;
    sd.SysMemPitch = w * 4;
    ID3D11Texture2D* tex = NULL;
    if (FAILED(dev->CreateTexture2D(&td, &sd, &tex))) return NULL;
    D3D11_SHADER_RESOURCE_VIEW_DESC vd; ZeroMemory(&vd, sizeof(vd));
    vd.Format = td.Format;
    vd.ViewDimension = D3D11_SRV_DIMENSION_TEXTURE2D;
    vd.Texture2D.MipLevels = 1;
    ID3D11ShaderResourceView* srv = NULL;
    dev->CreateShaderResourceView(tex, &vd, &srv);
    tex->Release();
    return srv;
}

void DmGfx::ReleaseSrv(ID3D11ShaderResourceView* srv) {
    if (srv) srv->Release();
}

ID3D11ShaderResourceView* DmGfx::TexFromDDS(const std::wstring& path, std::string* err) {
    FILE* f = _wfopen(path.c_str(), L"rb");
    if (!f) { if (err) *err = "DDS 打不开"; return NULL; }
    unsigned char hdr[128] = {0};
    if (fread(hdr, 1, 128, f) != 128 || memcmp(hdr, "DDS ", 4) != 0) {
        fclose(f); if (err) *err = "不是 DDS"; return NULL;
    }
    unsigned int hh = *(unsigned int*)(hdr + 12);
    unsigned int ww = *(unsigned int*)(hdr + 16);
    unsigned int pfFlags = *(unsigned int*)(hdr + 80);
    unsigned int fourCC = *(unsigned int*)(hdr + 84);
    unsigned int bpp = *(unsigned int*)(hdr + 88);
    unsigned int aMask = *(unsigned int*)(hdr + 104);
    if (fourCC != 0 || bpp != 32 || ww == 0 || hh == 0) {
        fclose(f); if (err) *err = "只支持未压缩 32 位 DDS"; return NULL;
    }
    size_t bytes = (size_t)ww * hh * 4;
    std::vector<unsigned char> pix(bytes);
    size_t got = fread(pix.data(), 1, bytes, f);
    fclose(f);
    if (got != bytes) { if (err) *err = "DDS 数据不足"; return NULL; }

    D3D11_TEXTURE2D_DESC td;
    ZeroMemory(&td, sizeof(td));
    td.Width = ww; td.Height = hh; td.MipLevels = 1; td.ArraySize = 1;
    td.Format = (aMask == 0xff000000u) ? DXGI_FORMAT_B8G8R8A8_UNORM : DXGI_FORMAT_B8G8R8X8_UNORM;
    td.SampleDesc.Count = 1;
    td.Usage = D3D11_USAGE_IMMUTABLE;
    td.BindFlags = D3D11_BIND_SHADER_RESOURCE;
    D3D11_SUBRESOURCE_DATA sd;
    ZeroMemory(&sd, sizeof(sd));
    sd.pSysMem = pix.data();
    sd.SysMemPitch = ww * 4;
    ID3D11Texture2D* tex = NULL;
    HRESULT hr = dev->CreateTexture2D(&td, &sd, &tex);
    if (FAILED(hr)) { if (err) *err = "CreateTexture2D(DDS) 失败"; return NULL; }
    ID3D11ShaderResourceView* srv = NULL;
    hr = dev->CreateShaderResourceView(tex, NULL, &srv);
    tex->Release();
    if (FAILED(hr)) { if (err) *err = "CreateSRV(DDS) 失败"; return NULL; }
    return srv;
}

ID3D11ShaderResourceView* DmGfx::TexFromFileWic(const std::wstring& path, std::string* err) {
    IWICImagingFactory* f = NULL;
    HRESULT hr = CoCreateInstance(CLSID_WICImagingFactory, NULL, CLSCTX_INPROC_SERVER,
                                  IID_IWICImagingFactory, (void**)&f);
    if (FAILED(hr)) { if (err) *err = "WIC 初始化失败"; return NULL; }
    IWICBitmapDecoder* dec = NULL;
    hr = f->CreateDecoderFromFilename(path.c_str(), NULL, GENERIC_READ, WICDecodeMetadataCacheOnDemand, &dec);
    if (FAILED(hr)) { f->Release(); if (err) *err = "打不开图片（WIC）"; return NULL; }
    IWICBitmapFrameDecode* fr = NULL;
    dec->GetFrame(0, &fr);
    IWICFormatConverter* cv = NULL;
    f->CreateFormatConverter(&cv);
    if (fr && cv) cv->Initialize(fr, GUID_WICPixelFormat32bppBGRA, WICBitmapDitherTypeNone, NULL, 0, WICBitmapPaletteTypeCustom);
    UINT w = 0, h = 0;
    if (cv) cv->GetSize(&w, &h);
    ID3D11ShaderResourceView* srv = NULL;
    if (w && h) {
        std::vector<BYTE> buf((size_t)w * h * 4);
        if (SUCCEEDED(cv->CopyPixels(NULL, w * 4, (UINT)buf.size(), buf.data())))
            srv = TexFromBGRA(buf.data(), w, h, false);
        DmLog("全景图载入 %ux%u %s", w, h, srv ? "OK" : "失败");
    }
    if (cv) cv->Release();
    if (fr) fr->Release();
    if (dec) dec->Release();
    f->Release();
    return srv;
}

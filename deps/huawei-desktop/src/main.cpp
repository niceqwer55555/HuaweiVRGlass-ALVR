// main.cpp —— 华为 VR Glass 桌面模式（独立渲染，不启动 SteamVR）
// 输出：在眼镜那块 3200x1600 屏上开全屏窗口，左半/右半各画一只眼
// 头姿：6DoF 套件 IMU（3DoF 旋转；没有平移数据）
// 功能：桌面镜像 / 跟随与悬停 / 曲面 / 透明度 / 背景 / SBS 3D / 热键 / HUD
#include <windows.h>
#include <tlhelp32.h>
#include <stdio.h>
#include <math.h>
#include <string>
#include <vector>
#include "dm_log.h"
#include "dm_math.h"
#include "dm_settings.h"
#include "dm_gfx.h"
#include "dm_capture.h"
#include "dm_scene.h"
#include "dm_hud.h"
#include "dm_hotkeys.h"
#include "dm_ui_settings.h"
#include "dm_imu.h"
#include "dm_hmdpose.h"
#include "dm_fusion.h"
#include "dm_oneeuro.h"
#include <wbemidl.h>
#include <pdh.h>
#include <pdhmsg.h>
#include "dm_output.h"

static const float kPi = 3.14159265358979f;
static bool g_oeResetReq = false;      // 姿态归零时同步复位 1€ 滤波器
static inline float Rad(float d) { return d * kPi / 180.0f; }

static DmSettings g_st;
static DmScene    g_scene;
static DmHotkeys  g_hk;
static DmGfx      g_gfx;
static DmCapture  g_cap;
static DmHud      g_hud;
static DmToast    g_toast;
static DmImu      g_imu;
static DmOutput   g_out;

static bool  g_quit = false;
static DmHmdPose g_hmdPose;
volatile LONG g_poseZeroReq = 0;
volatile LONG g_calReq = 0;   // 自动标定请求
static HANDLE g_parentProc = NULL;   // 启动本程序的客户端进程（用于随客户端退出）
static ULONGLONG g_rangeHintUntil = 0;   // 触发范围提示（拖动滑条后显示 2 秒）
static float     g_lastRangeDeg  = -1.0f;
static ULONGLONG g_suppressAutoZeroUntil = 0;   // 该时间点前不自动归零（改设置期间）
static Quat  g_headRot;          // IMU 朝向（已做轴映射）
static Vec3  g_headPos;          // 3DoF：固定原点
static bool  g_headValid = false;

static std::wstring g_toastText;
static std::wstring g_toastShown;
static ULONGLONG    g_toastUntil = 0;
static bool g_settingsDirty = false;
static ULONGLONG g_lastSave = 0;
static ULONGLONG g_ignoreReloadUntil = 0;
static FILETIME g_iniStamp = {0, 0};

static ID3D11ShaderResourceView* g_panoSrv = NULL;
static std::wstring g_panoLoaded;

// 统计
static double g_renderFps = 0, g_capFps = 0;
static double g_frameMs = 0;
static bool   g_scanWarned = false;
static uint64_t g_frameCount = 0;
static ULONGLONG g_fpsT0 = 0;
static uint32_t g_fpsFrames = 0;
static uint32_t g_capFramesStat = 0;
static bool g_screenHidden = false;
static float g_savedOpacity = 1.0f;

static std::wstring Fmt(const wchar_t* f, ...) {
    wchar_t buf[1024];
    va_list ap; va_start(ap, f);
    vswprintf(buf, 1024, f, ap);
    va_end(ap);
    return std::wstring(buf);
}

static void ToastShow(const std::wstring& s, int ms = 1600) {
    g_toastText = s;
    g_toastUntil = GetTickCount64() + ms;
}

static void Announce(const wchar_t* name, float v, const wchar_t* unit) {
    ToastShow(Fmt(L"%ls：%.2f%ls", name, v, unit));
}

static void MarkDirty() { g_settingsDirty = true; }

static void SaveNow() {
    if (!g_settingsDirty) return;
    g_settingsDirty = false;
    g_lastSave = GetTickCount64();
    DmSettingsSave(&g_st);
    g_ignoreReloadUntil = g_lastSave + 900;
    WIN32_FILE_ATTRIBUTE_DATA fa;
    if (GetFileAttributesExW(DmIniPath().c_str(), GetFileExInfoStandard, &fa))
        g_iniStamp = fa.ftLastWriteTime;
}

static void ReloadPanorama();

// ---------------------------------------------------------------- 动作
static void OpenSettingsWindow();
static void UiApplied();

static void UiApplied() {
    int failList[32], failCount = 0;
    g_hk.Install(g_st, failList, &failCount);
    DmLog("配置窗口：已应用并保存");
}
static void OpenSettingsWindow() { DmUiToggleSettings(&g_st, UiApplied); }

// 在屏幕平面上画一圈线框：半径 = tan(触发角) × 距离（= 该角度在屏幕平面上的落点）
static void DmBuildRangeRing(const DmPose& plane, float radius, float thick,
                             std::vector<DmVert>* out) {
    out->clear();
    const int N = 96;
    M4 R = M4FromQuat(plane.rot);
    Vec3 X = XformDir(R, Vec3(1, 0, 0));
    Vec3 Y = XformDir(R, Vec3(0, 1, 0));
    auto P = [&](float a, float rr) {
        return plane.pos + X * (cosf(a) * rr) + Y * (sinf(a) * rr);
    };
    for (int i = 0; i < N; i++) {
        float a0 = (float)i / N * 6.2831853f;
        float a1 = (float)(i + 1) / N * 6.2831853f;
        Vec3 in0 = P(a0, radius - thick), out0 = P(a0, radius + thick);
        Vec3 in1 = P(a1, radius - thick), out1 = P(a1, radius + thick);
        auto v = [](const Vec3& p) { DmVert d; d.x = p.x; d.y = p.y; d.z = p.z; d.u = 0; d.v = 0; return d; };
        DmVert a = v(in0), b = v(out0), c = v(in1), e = v(out1);
        out->push_back(a); out->push_back(b); out->push_back(c);
        out->push_back(c); out->push_back(b); out->push_back(e);
    }
}

static void FireAction(int a, bool fast) {
    DmLog("动作触发 a=%d fast=%d", a, (int)fast);
    const float panStep = fast ? g_st.panStepFast : g_st.panStep;
    switch (a) {
    case DM_A_RECENTER:
        g_scene.Recenter(g_headPos, g_headRot);
        ToastShow(L"桌面已回中");
        break;
    case DM_A_PAN_UP: case DM_A_PAN_DOWN: case DM_A_PAN_LEFT: case DM_A_PAN_RIGHT: {
        // 步长随距离缩放：远处也看得见（原来固定 5cm，6 米处只有 0.5°，几乎看不出）
        float step = panStep * (g_st.distance > 0.1f ? g_st.distance * 0.5f : 1.0f);
        float dx = 0, dy = 0;
        if (a == DM_A_PAN_UP)    dy = -step;
        if (a == DM_A_PAN_DOWN)  dy =  step;
        if (a == DM_A_PAN_LEFT)  dx = -step;
        if (a == DM_A_PAN_RIGHT) dx =  step;
        g_scene.Pan(dx, dy, g_st);
        // 与面板按钮完全等价：写回配置并落盘
        g_st.panX = g_scene.panX; g_st.panY = g_scene.panY;
        DmSettingsSave(&g_st);
        ToastShow(Fmt(L"平移 %.2f / %.2f 米", g_scene.panX, g_scene.panY));
        break;
    }
    case DM_A_ZOOM_IN:
        g_st.scale = Clampf(g_st.scale * g_st.zoomStep, 1.0f, 10.0f);
        Announce(L"缩放", g_st.scale, L"x"); MarkDirty(); break;
    case DM_A_ZOOM_OUT:
        g_st.scale = Clampf(g_st.scale / g_st.zoomStep, 1.0f, 10.0f);
        Announce(L"缩放", g_st.scale, L"x"); MarkDirty(); break;
    case DM_A_CURV_UP:
        g_st.curvature = Clampf(g_st.curvature + g_st.curvStep, 0.0f, 20.0f);
        Announce(L"弯曲度", g_st.curvature, L""); MarkDirty(); break;
    case DM_A_CURV_DOWN:
        g_st.curvature = Clampf(g_st.curvature - g_st.curvStep, 0.0f, 20.0f);
        Announce(L"弯曲度", g_st.curvature, L""); MarkDirty(); break;
    case DM_A_DIST_FAR: case DM_A_DIST_NEAR: {
        float dir = (a == DM_A_DIST_FAR) ? 1.0f : -1.0f;
        float stepD = fast ? 1.0f : g_st.distStep;
        g_st.distance = Clampf(g_st.distance + dir * stepD, 0.5f, 10.0f);
        MarkDirty();
        Announce(L"距离", g_st.distance, L"m");
        break;
    }
    case DM_A_S3D_CYCLE: {
        int m = (g_st.sbsMode + 1) % 4;
        g_st.sbsMode = m;
        const wchar_t* nm[4] = { L"关闭（2D）", L"左右（半宽）", L"左右（全宽）", L"上下" };
        ToastShow(Fmt(L"3D 模式：%ls", nm[m]));
        MarkDirty();
        break;
    }
    case DM_A_TOGGLE_HOVER:
        g_st.followMode = g_st.followMode ? 0 : 1;
        g_scene.ToggleMode(g_headPos, g_headRot, g_st);
        ToastShow(g_st.followMode ? L"模式：跟随（只跟随平移）" : L"模式：悬停（世界锁定）");
        MarkDirty();
        break;
    case DM_A_TOGGLE_HUD:
        if (g_st.hudPanel || g_st.hudKeys) { g_st.hudPanel = 0; g_st.hudKeys = 0; ToastShow(L"HUD 已隐藏"); }
        else { g_st.hudPanel = 1; g_st.hudKeys = 1; ToastShow(L"HUD 已显示"); }
        MarkDirty();
        break;
    case DM_A_OPACITY_UP:
        if (g_screenHidden) { g_screenHidden = false; g_st.opacity = g_savedOpacity; }
        g_st.opacity = Clampf(g_st.opacity + g_st.opacityStep, 0.1f, 1.0f);
        Announce(L"透明度", g_st.opacity, L""); MarkDirty(); break;
    case DM_A_OPACITY_DOWN:
        g_st.opacity = Clampf(g_st.opacity - g_st.opacityStep, 0.1f, 1.0f);
        Announce(L"透明度", g_st.opacity, L""); MarkDirty(); break;
    case DM_A_TOGGLE_SCREEN:
        if (!g_screenHidden) { g_savedOpacity = g_st.opacity; g_screenHidden = true; ToastShow(L"桌面已隐藏"); }
        else { g_screenHidden = false; ToastShow(L"桌面已显示"); }
        break;
    case DM_A_BG_CYCLE:
        if (g_st.bgMode == 1) { g_st.bgMode = 0; g_st.venvMode = 0; }
        else if (g_st.venvMode < 2) g_st.venvMode++;
        else { g_st.bgMode = 0; g_st.venvMode = 1; }
        {
            const wchar_t* nm[3] = { L"纯色渐变", L"程序化天空", L"全景图" };
            ToastShow(g_st.bgMode == 1 ? L"背景：摄像头透视" : Fmt(L"背景：%ls", nm[g_st.venvMode]));
        }
        ReloadPanorama();
        MarkDirty();
        break;
    case DM_A_ZERODOF:      // 窗口锁定跟随（3DOF 基底 + 窗口延迟跟头）
        g_st.followMode = g_st.followMode ? 0 : 1;
        ToastShow(Fmt(L"窗口跟随：%ls", g_st.followMode ? L"开（延迟锁定头部）" : L"关（停在原地）"));
        MarkDirty();
        break;
    case DM_A_LENS_MODE:
        g_st.lensMode = g_st.lensMode ? 0 : 1;
        ToastShow(Fmt(L"畸变：%ls", g_st.lensMode ? L"官方多项式" : L"关闭"));
        MarkDirty();
        break;
    case DM_A_LENS_IN:
        // 微调整体倍率（官方模型下 = 缩放，旧模型下 = k1）
        g_st.lensZoom = Clampf(g_st.lensZoom * 1.02f, 0.5f, 2.0f);
        ToastShow(Fmt(L"画面缩放 %.3f", g_st.lensZoom));
        MarkDirty();
        break;
    case DM_A_LENS_OUT:
        g_st.lensZoom = Clampf(g_st.lensZoom / 1.02f, 0.5f, 2.0f);
        ToastShow(Fmt(L"画面缩放 %.3f", g_st.lensZoom));
        MarkDirty();
        break;
    case DM_A_EYE_ROLL_CYCLE: {
        // 4 种组合，用来确定面板物理朝向（官方是单块面板，通常是两眼同向）
        int stage = 0;
        if (g_st.rollLeft < 0 && g_st.rollRight < 0) stage = 1;
        else if (g_st.rollLeft > 0 && g_st.rollRight > 0) stage = 2;
        else if (g_st.rollLeft < 0 && g_st.rollRight > 0) stage = 3;
        switch (stage) {
        case 0: g_st.rollLeft = 90;  g_st.rollRight = 90;  break;
        case 1: g_st.rollLeft = -90; g_st.rollRight = 90;  break;
        case 2: g_st.rollLeft = 90;  g_st.rollRight = -90; break;
        case 3: g_st.rollLeft = -90; g_st.rollRight = -90; break;
        }
        ToastShow(Fmt(L"两眼朝向：左 %.0f° 右 %.0f°", g_st.rollLeft, g_st.rollRight));
        MarkDirty();
        break;
    }
    case DM_A_IMU_SOURCE:
        g_st.imuSource = g_st.imuSource ? 0 : 1;
        if (g_st.imuSource == 1) { g_imu.Stop(); g_hmdPose.Start(); }
        else { g_hmdPose.Stop(); g_imu.Start(); }
        ToastShow(g_st.imuSource == 1 ? L"姿态源：官方 getRawPose" : L"姿态源：自研 3DoF（HID 陀螺）");
        MarkDirty();
        break;
    case DM_A_AUTOCAL: {
        extern volatile LONG g_calReq;
        g_calReq = 1;
        ToastShow(L"自动标定：6 秒内请左右转头");
        break;
    }
    case DM_A_REZERO:
        g_imu.RequestRecalibrate();
        { extern volatile LONG g_poseZeroReq; g_poseZeroReq = 1; }
        ToastShow(L"头动归零：保持不动 0.8 秒");
        break;
    case DM_A_EXIT:
        ToastShow(L"退出桌面模式");
        g_quit = true;
        break;
    default:
        break;
    }
}

// ---------------------------------------------------------------- 全景图
static void ReloadPanorama() {
    std::wstring want;
    if (g_st.bgMode == 0 && g_st.venvMode == 3)
        want = L"C:/Program/HwPCVR Assistant/pcCastScreen/Texture/skybox1.dds";
    else if (g_st.bgMode == 0 && g_st.venvMode == 2 && g_st.panoPath[0])
        want = g_st.panoPath;
    if (want.empty() || want == g_panoLoaded) return;

    if (g_panoSrv) { g_gfx.ReleaseSrv(g_panoSrv); g_panoSrv = NULL; }
    std::string err;
    g_panoSrv = (g_st.venvMode == 3) ? g_gfx.TexFromDDS(want, &err)
                                     : g_gfx.TexFromFileWic(want, &err);
    if (!g_panoSrv) DmLog("全景图载入失败：%s", err.c_str());
    else DmLog("全景图已载入（%s）", g_st.venvMode == 3 ? "官方 skybox1.dds" : "自定义图片");
    g_panoLoaded = g_panoSrv ? want : L"";
}

// ---------------------------------------------------------------- 设置热重载
static void CheckSettingsFile() {
    WIN32_FILE_ATTRIBUTE_DATA fa;
    if (!GetFileAttributesExW(DmIniPath().c_str(), GetFileExInfoStandard, &fa)) return;
    if (CompareFileTime(&fa.ftLastWriteTime, &g_iniStamp) == 0) return;
    g_iniStamp = fa.ftLastWriteTime;
    if (GetTickCount64() < g_ignoreReloadUntil) return;

    DmSettings ns;
    DmSettingsLoad(&ns);
    bool monChanged = (ns.monitorIndex != g_st.monitorIndex);
    bool panoChanged = (wcscmp(ns.panoPath, g_st.panoPath) != 0) || ns.venvMode != g_st.venvMode || ns.bgMode != g_st.bgMode;
    bool fovChanged = (ns.fovTanX != g_st.fovTanX) || (ns.ipdMm != g_st.ipdMm);
    bool modeChanged = (ns.followMode != g_st.followMode);
    bool imuChanged = (ns.imuMap != g_st.imuMap) || (ns.gyroScale != g_st.gyroScale) || (ns.accelScale != g_st.accelScale);
    g_st = ns;
    g_st.opacity = g_screenHidden ? g_savedOpacity : g_st.opacity;

    int fail[32], nf = 0;
    g_hk.Install(g_st, fail, &nf);
    if (monChanged && g_gfx.dev) g_cap.Start(g_gfx.dev, g_gfx.ctx, g_st.monitorIndex, NULL);
    if (panoChanged) { g_panoLoaded.clear(); ReloadPanorama(); }
    if (imuChanged) { g_imu.gyroScale = g_st.gyroScale; g_imu.accelScale = g_st.accelScale; g_imu.mapMode = g_st.imuMap; }
    if (modeChanged) g_scene.ToggleMode(g_headPos, g_headRot, g_st);
    // 仅改参数时不要动窗口位姿（否则每次改设置画面都会"跳一下"，看着像自动归零）
    g_suppressAutoZeroUntil = GetTickCount64() + 3000;
    (void)fovChanged;
    // 控制面板下发的一次性命令：执行后立即清空，避免重复触发
    {
        wchar_t cmd[64] = {0};
        GetPrivateProfileStringW(L"debug", L"cmd", L"", cmd, 64, DmIniPath().c_str());
        if (cmd[0]) {
            if (!_wcsicmp(cmd, L"rezero")) {
                extern volatile LONG g_poseZeroReq;
                g_poseZeroReq = 1;
                ToastShow(L"姿态已归零（当前朝向 = 正前方）");
                DmLog("面板命令：姿态归零");
            } else if (!_wcsicmp(cmd, L"pan_left"))  { FireAction(DM_A_PAN_LEFT, false); }
            else if (!_wcsicmp(cmd, L"pan_right")) { FireAction(DM_A_PAN_RIGHT, false); }
            else if (!_wcsicmp(cmd, L"pan_up"))    { FireAction(DM_A_PAN_UP, false); }
            else if (!_wcsicmp(cmd, L"pan_down"))  { FireAction(DM_A_PAN_DOWN, false); }
            else if (!_wcsicmp(cmd, L"recenter")) {
                g_scene.Recenter(g_headPos, g_headRot);
                ToastShow(L"桌面已回中");
                DmLog("面板命令：桌面回中");
            }
            WritePrivateProfileStringW(L"debug", L"cmd", L"", DmIniPath().c_str());
            g_ignoreReloadUntil = GetTickCount64() + 300;
        }
    }
    ToastShow(L"设置已重新载入");
    DmLog("配置已重载：monitor=%d distance=%.2f scale=%.2f curv=%.2f opacity=%.2f follow=%d",
          g_st.monitorIndex, g_st.distance, g_st.scale, g_st.curvature, g_st.opacity, g_st.followMode);
}

// ---------------------------------------------------------------- 系统指标（HUD 用，1 秒采一次）
static float  g_mCpuPct = 0.0f, g_mCpuTemp = -1.0f;
static float  g_mGpuPct = -1.0f, g_mGpuTemp = -1.0f;
static float  g_mMemPct = 0.0f;
static double g_mMemUsedGB = 0, g_mMemTotalGB = 0;
static float  g_mDiskPct = 0.0f;
static double g_mDiskFreeGB = 0;
static wchar_t g_mDiskDrv = 0;      // 占用率最高的那块盘
static ULONGLONG g_mLastTick = 0;

// --- NVML（System32\nvml.dll）动态加载：GPU 占用 / 温度，不进导入表 ---
typedef struct { unsigned gpu; unsigned memory; } DmNvmlUtil;
typedef int (*PFN_nvmlInit)(void);
typedef int (*PFN_nvmlHandle)(unsigned, void**);
typedef int (*PFN_nvmlUtil)(void*, DmNvmlUtil*);
typedef int (*PFN_nvmlTemp)(void*, int, unsigned*);
static bool  g_nvmlTried = false, g_nvmlOk = false;
static void* g_nvmlDev = NULL;
static PFN_nvmlUtil g_nvmlUtilFn = NULL;
static PFN_nvmlTemp g_nvmlTempFn = NULL;
static bool DmNvmlInit() {
    if (g_nvmlTried) return g_nvmlOk;
    g_nvmlTried = true;
    HMODULE h = LoadLibraryW(L"nvml.dll");
    if (!h) return false;
    PFN_nvmlInit init = (PFN_nvmlInit)GetProcAddress(h, "nvmlInit_v2");
    PFN_nvmlHandle getH = (PFN_nvmlHandle)GetProcAddress(h, "nvmlDeviceGetHandleByIndex_v2");
    g_nvmlUtilFn = (PFN_nvmlUtil)GetProcAddress(h, "nvmlDeviceGetUtilizationRates");
    g_nvmlTempFn = (PFN_nvmlTemp)GetProcAddress(h, "nvmlDeviceGetTemperature");
    if (!init || !getH || !g_nvmlUtilFn || !g_nvmlTempFn) return false;
    if (init() != 0) return false;
    if (getH(0, &g_nvmlDev) != 0) return false;
    g_nvmlOk = true;
    return true;
}

// --- CPU 温度：WMI 两个来源都试（台机常有第二个）---
//   ① root\WMI  MSAcpi_ThermalZoneTemperature.CurrentTemperature         (0.1K)
//   ② root\cimv2 Win32_PerfFormattedData_Counters_ThermalZoneInformation
//      .HighPrecisionTemperature                                          (0.1K)
static bool g_wmiReady = false, g_wmiOk = false;
static IWbemServices* g_wmiSvc = NULL;

static bool DmWmiInit() {
    if (g_wmiReady) return g_wmiOk;
    g_wmiReady = true;
    IWbemLocator* loc = NULL;
    if (SUCCEEDED(CoCreateInstance(CLSID_WbemLocator, 0, CLSCTX_INPROC_SERVER,
                                   IID_IWbemLocator, (LPVOID*)&loc)) && loc) {
        BSTR ns = SysAllocString(L"ROOT\\WMI");
        if (SUCCEEDED(loc->ConnectServer(ns, NULL, NULL, 0, 0, 0, 0, &g_wmiSvc))) {
            CoSetProxyBlanket(g_wmiSvc, RPC_C_AUTHN_WINNT, RPC_C_AUTHZ_NONE, NULL,
                              RPC_C_AUTHN_LEVEL_CALL, RPC_C_IMP_LEVEL_IMPERSONATE, NULL, EOAC_NONE);
            g_wmiOk = true;
        }
        SysFreeString(ns);
        loc->Release();
    }
    return g_wmiOk;
}

// 在指定命名空间下查一条记录的一个整型属性；成功返回 true
static bool DmWmiQueryInt(const wchar_t* ns, const wchar_t* query, const wchar_t* prop, int* out) {
    IWbemLocator* loc = NULL;
    IWbemServices* svc = NULL;
    bool ok = false;
    if (FAILED(CoCreateInstance(CLSID_WbemLocator, 0, CLSCTX_INPROC_SERVER,
                                IID_IWbemLocator, (LPVOID*)&loc)) || !loc) return false;
    BSTR bns = SysAllocString(ns);
    if (SUCCEEDED(loc->ConnectServer(bns, NULL, NULL, 0, 0, 0, 0, &svc))) {
        CoSetProxyBlanket(svc, RPC_C_AUTHN_WINNT, RPC_C_AUTHZ_NONE, NULL,
                          RPC_C_AUTHN_LEVEL_CALL, RPC_C_IMP_LEVEL_IMPERSONATE, NULL, EOAC_NONE);
        IEnumWbemClassObject* en = NULL;
        BSTR lang = SysAllocString(L"WQL");
        BSTR q = SysAllocString(query);
        if (SUCCEEDED(svc->ExecQuery(lang, q, WBEM_FLAG_FORWARD_ONLY | WBEM_FLAG_RETURN_IMMEDIATELY, NULL, &en))) {
            IWbemClassObject* obj = NULL; ULONG ret = 0;
            if (en->Next(WBEM_INFINITE, 1, &obj, &ret) == S_OK && obj) {
                VARIANT v; VariantInit(&v);
                if (SUCCEEDED(obj->Get(prop, 0, &v, NULL, NULL)) &&
                    (v.vt == VT_I4 || v.vt == VT_UI4 || v.vt == VT_UI2 || v.vt == VT_I2)) {
                    *out = (v.vt == VT_I4) ? v.lVal : (int)v.ulVal;
                    ok = true;
                }
                VariantClear(&v);
                obj->Release();
            }
            en->Release();
        }
        SysFreeString(lang); SysFreeString(q);
        svc->Release();
    }
    SysFreeString(bns);
    loc->Release();
    return ok;
}

// 返回 CPU 温度（°C），取不到返回 -1
static float DmCpuTempWmi() {
    static float cached = -1.0f;
    int val = 0;
    if (DmWmiQueryInt(L"ROOT\\WMI",
                      L"SELECT CurrentTemperature FROM MSAcpi_ThermalZoneTemperature",
                      L"CurrentTemperature", &val))
        cached = (float)((double)val / 10.0 - 273.15);
    else if (DmWmiQueryInt(L"ROOT\\CIMV2",
                           L"SELECT HighPrecisionTemperature FROM Win32_PerfFormattedData_Counters_ThermalZoneInformation",
                           L"HighPrecisionTemperature", &val))
        cached = (float)((double)val / 10.0 - 273.15);
    return cached;
}

// --- 磁盘读写占用率：PDH \LogicalDisk(*)\% Disk Time，取占用最高的盘 ---
static PDH_HQUERY g_pdhQ = NULL;
static PDH_HCOUNTER g_pdhC = NULL;
static bool g_pdhOk = false, g_pdhPrimed = false;

static void DmSampleDiskIO() {
    if (!g_pdhOk) {
        if (PdhOpenQueryW(NULL, 0, &g_pdhQ) != ERROR_SUCCESS) return;
        if (PdhAddEnglishCounterW(g_pdhQ, L"\\LogicalDisk(*)\\% Disk Time", 0, &g_pdhC) != ERROR_SUCCESS) {
            PdhCloseQuery(g_pdhQ); g_pdhQ = NULL; return;
        }
        PdhCollectQueryData(g_pdhQ);      // 第一次只是打点，下次才有增量
        g_pdhOk = true; g_pdhPrimed = true;
        return;
    }
    if (!g_pdhPrimed) { PdhCollectQueryData(g_pdhQ); g_pdhPrimed = true; return; }
    if (PdhCollectQueryData(g_pdhQ) != ERROR_SUCCESS) return;
    DWORD sz = 0, cnt = 0;
    if (PdhGetFormattedCounterArrayW(g_pdhC, PDH_FMT_DOUBLE, &sz, &cnt, NULL) != PDH_MORE_DATA || sz == 0) return;
    std::vector<BYTE> buf(sz);
    PPDH_FMT_COUNTERVALUE_ITEM_W items = (PPDH_FMT_COUNTERVALUE_ITEM_W)buf.data();
    if (PdhGetFormattedCounterArrayW(g_pdhC, PDH_FMT_DOUBLE, &sz, &cnt, items) != ERROR_SUCCESS) return;
    double best = -1.0; wchar_t bestDrv = 0;
    for (DWORD i = 0; i < cnt; i++) {
        const wchar_t* nm = items[i].szName;
        if (!nm || wcslen(nm) < 2 || nm[1] != L':') continue;      // 只要盘符（跳过 _total / harddiskvolumeN）
        if (items[i].FmtValue.CStatus != PDH_CSTATUS_VALID_DATA) continue;
        double v = items[i].FmtValue.doubleValue;
        if (v < 0.0) continue;
        if (v > best) { best = v; bestDrv = nm[0]; }
    }
    if (best >= 0.0) {
        g_mDiskPct = (float)(best > 100.0 ? 100.0 : best);
        g_mDiskDrv = bestDrv;
    }
}

static void DmSampleMetrics() {
    ULONGLONG now = GetTickCount64();
    if (now - g_mLastTick < 1000) return;
    g_mLastTick = now;
    // CPU 占用
    static ULONGLONG li = 0, lk = 0, lu = 0; static bool first = true;
    FILETIME fi, fk, fu;
    if (GetSystemTimes(&fi, &fk, &fu)) {
        ULONGLONG i = ((ULONGLONG)fi.dwHighDateTime << 32) | fi.dwLowDateTime;
        ULONGLONG k = ((ULONGLONG)fk.dwHighDateTime << 32) | fk.dwLowDateTime;
        ULONGLONG u = ((ULONGLONG)fu.dwHighDateTime << 32) | fu.dwLowDateTime;
        if (!first) {
            ULONGLONG di = i - li, total = (k - lk) + (u - lu);   // 内核时间含空闲
            if (total > 0) g_mCpuPct = (float)(100.0 * (double)(total - di) / (double)total);
        }
        li = i; lk = k; lu = u; first = false;
    }
    // CPU 温度（WMI 有开销，5 秒一次）
    static ULONGLONG tCpu = 0;
    if (now - tCpu > 5000) { tCpu = now; g_mCpuTemp = DmCpuTempWmi(); }
    // GPU 占用 / 温度
    if (DmNvmlInit()) {
        DmNvmlUtil ut; ZeroMemory(&ut, sizeof(ut));
        if (g_nvmlUtilFn(g_nvmlDev, &ut) == 0) g_mGpuPct = (float)ut.gpu;
        unsigned t = 0;
        if (g_nvmlTempFn(g_nvmlDev, 0, &t) == 0) g_mGpuTemp = (float)t;   // 0 = NVML_TEMPERATURE_GPU
    }
    // 内存
    MEMORYSTATUSEX ms; ZeroMemory(&ms, sizeof(ms)); ms.dwLength = sizeof(ms);
    if (GlobalMemoryStatusEx(&ms)) {
        g_mMemPct = (float)ms.dwMemoryLoad;
        g_mMemTotalGB = (double)ms.ullTotalPhys / (1024.0 * 1024 * 1024);
        g_mMemUsedGB = (double)(ms.ullTotalPhys - ms.ullAvailPhys) / (1024.0 * 1024 * 1024);
    }
    // 磁盘：读写占用率（不是空间）—— 取所有逻辑盘里 I/O 占用最高的那块
    DmSampleDiskIO();
}

static std::wstring DmTempStr(float t) {
    if (t <= 0.0f) return L"—";
    return Fmt(L"%.0f°C", t);
}

// ---------------------------------------------------------------- HUD 内容
static void BuildHudData(DmHudData* d) {
    d->showPanel = g_st.hudPanel != 0;
    d->showKeys = 0;    // 只保留状态信息，不再显示快捷键列
    d->panel.clear();
    d->keys.clear();
    DmSampleMetrics();

    const wchar_t* s3d = (g_st.sbsMode == 0) ? L"2D"
                       : (g_st.sbsMode == 1) ? L"SBS 半宽"
                       : (g_st.sbsMode == 2) ? L"SBS 全宽" : L"上下 3D";
    d->panel.push_back(Fmt(L"FPS %.0f", g_renderFps));
    d->panel.push_back(Fmt(L"CPU %.0f%%    温度 %ls", g_mCpuPct, DmTempStr(g_mCpuTemp).c_str()));
    d->panel.push_back(Fmt(L"GPU %.0f%%    温度 %ls", g_mGpuPct < 0 ? 0.0f : g_mGpuPct, DmTempStr(g_mGpuTemp).c_str()));
    d->panel.push_back(Fmt(L"内存 %.0f%%    %.1f / %.1f GB", g_mMemPct, g_mMemUsedGB, g_mMemTotalGB));
    d->panel.push_back(Fmt(L"磁盘 %.0f%%    %lc:", g_mDiskPct, g_mDiskDrv ? g_mDiskDrv : L'-'));
    d->panel.push_back(Fmt(L"渲染延迟 %.1f ms", g_frameMs));
    d->panel.push_back(Fmt(L"3D 模式：%ls", s3d));
    d->toast = L"";
}

// ---------------------------------------------------------------- 渲染
static void DumpRTToBmp(ID3D11Device* dev, ID3D11DeviceContext* ctx, ID3D11Texture2D* tex, const wchar_t* path) {
    if (!tex) return;
    D3D11_TEXTURE2D_DESC td;
    tex->GetDesc(&td);
    D3D11_TEXTURE2D_DESC sd = td;
    sd.Usage = D3D11_USAGE_STAGING;
    sd.BindFlags = 0;
    sd.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
    sd.MiscFlags = 0;
    ID3D11Texture2D* staging = NULL;
    if (FAILED(dev->CreateTexture2D(&sd, NULL, &staging))) return;
    ctx->CopyResource(staging, tex);
    D3D11_MAPPED_SUBRESOURCE ms;
    if (SUCCEEDED(ctx->Map(staging, 0, D3D11_MAP_READ, 0, &ms))) {
        FILE* f = _wfopen(path, L"wb");
        if (f) {
            BITMAPFILEHEADER fh; ZeroMemory(&fh, sizeof(fh));
            BITMAPINFOHEADER ih; ZeroMemory(&ih, sizeof(ih));
            uint32_t rowBytes = td.Width * 3;
            uint32_t pad = (4 - (rowBytes % 4)) % 4;
            ih.biSize = sizeof(ih);
            ih.biWidth = td.Width;
            ih.biHeight = -(int)td.Height;
            ih.biPlanes = 1;
            ih.biBitCount = 24;
            ih.biCompression = BI_RGB;
            ih.biSizeImage = (rowBytes + pad) * td.Height;
            fh.bfType = 0x4D42;
            fh.bfOffBits = sizeof(fh) + sizeof(ih);
            fh.bfSize = fh.bfOffBits + ih.biSizeImage;
            fwrite(&fh, 1, sizeof(fh), f);
            fwrite(&ih, 1, sizeof(ih), f);
            std::vector<uint8_t> row(rowBytes + pad);
            for (uint32_t y = 0; y < td.Height; y++) {
                const uint8_t* src = (const uint8_t*)ms.pData + (size_t)y * ms.RowPitch;
                for (uint32_t x = 0; x < td.Width; x++) {
                    row[x * 3 + 0] = src[x * 4 + 0];
                    row[x * 3 + 1] = src[x * 4 + 1];
                    row[x * 3 + 2] = src[x * 4 + 2];
                }
                memset(&row[rowBytes], 0, pad);
                fwrite(row.data(), 1, rowBytes + pad, f);
            }
            fclose(f);
            DmLog("已导出帧画面 %S", path);
        }
        ctx->Unmap(staging, 0);
    }
    staging->Release();
}

// 行向量约定的投影矩阵
static M4 MakeProjRow(float tanX, float tanY, float zn, float zf) {
    M4 m;
    m.m[0][0] = 1.0f / tanX;
    m.m[1][1] = 1.0f / tanY;
    m.m[2][2] = -(zf + zn) / (zf - zn);
    m.m[3][2] = -2.0f * zf * zn / (zf - zn);
    m.m[2][3] = -1.0f;
    return m;
}

static void RenderFrame(double dt) {
    // 面板/窗口分辨率：跟随实际显示（眼镜屏）分辨率；无头模式回退 3200x1600
    uint32_t panelW = g_out.IsValid() ? g_out.w : 3200u;
    uint32_t panelH = g_out.IsValid() ? g_out.h : 1600u;
    if (panelW < 16 || panelH < 16) { panelW = 3200; panelH = 1600; }
    const uint32_t perEyeW = panelW / 2, perEyeH = panelH;
    if (!g_gfx.EnsureTarget(perEyeW * 2, perEyeH, g_st.srgb != 0)) return;
    // 渲染分辨率 = 每眼面板分辨率 × 倍数（默认 1.5×）：眼睛离屏按这个尺寸渲染，
    //   再由畸变贴图采样缩到面板分辨率，等效 SSAA。显示分辨率变了会自动跟着变。
    float ssF = g_st.superres;
    if (ssF < 1.0f) ssF = 1.0f;
    if (ssF > 2.0f) ssF = 2.0f;
    uint32_t eyeW = (uint32_t)(perEyeW * ssF + 0.5f);
    uint32_t eyeH = (uint32_t)(perEyeH * ssF + 0.5f);
    if (!g_gfx.EnsureEyeTarget(eyeW, eyeH)) return;
    if (g_frameCount == 0)
        DmLog("渲染分辨率：面板 %ux%u，每眼渲染 %ux%u（%.2f×，超采样后缩到面板）",
              panelW, panelH, eyeW, eyeH, ssF);

    float aspect = (g_cap.w && g_cap.h) ? (float)g_cap.w / (float)g_cap.h : 1.6f;
    // 屏幕形状只由桌面自身比例决定：切换 SBS/3D 模式不改变屏幕尺寸与朝向
    float uiAspect = aspect;

    std::vector<DmVert> screenMesh;
    DmPose shown = g_scene.shown;
    float screenAspect = (g_st.screenAspect > 0.05f) ? g_st.screenAspect : aspect;   // 官方 m_fAspect = 1.0
    // 全宽 SBS（FSBS）：半幅源的宽高比是原来的 1/2 → 屏幕按 A/2 建（同宽、双高），
    // 这样半幅画面既不裁切也不拉伸，整体视觉尺寸翻倍
    DmBuildScreenMesh(g_st, shown, g_st.sbsMode == 2 ? screenAspect * 0.5f : screenAspect, &screenMesh);
    // 跟随触发范围提示环（画在屏幕平面上）
    std::vector<DmVert> rangeRing;
    const bool rangeHintOn = (GetTickCount64() < g_rangeHintUntil);
    if (rangeHintOn) {
        float rad = g_st.distance * tanf(Rad(g_st.followRangeDeg > 0.5f ? g_st.followRangeDeg : 0.5f));
        DmBuildRangeRing(shown, rad, rad * 0.012f + 0.004f, &rangeRing);
    }

    // HUD：贴在“桌面窗口下沿”，并沿用桌面的姿态（跟着整块屏走）
    float scrHalfW = 0.8f, scrHalfH = 0.5f;
    if (!screenMesh.empty()) {
        Quat invQ(-shown.rot.w, -shown.rot.x, -shown.rot.y, -shown.rot.z);
        float mx = 0.0f, my = 0.0f;
        for (size_t k = 0; k < screenMesh.size(); k++) {
            Vec3 w(screenMesh[k].x - shown.pos.x, screenMesh[k].y - shown.pos.y, screenMesh[k].z - shown.pos.z);
            Vec3 l = XformDir(M4FromQuat(invQ), w);
            if (fabsf(l.x) > mx) mx = fabsf(l.x);
            if (fabsf(l.y) > my) my = fabsf(l.y);
        }
        if (mx > 0.01f) scrHalfW = mx;
        if (my > 0.01f) scrHalfH = my;
    }
    // HUD：贴着桌面窗口的【下沿】挂，两边和窗口等宽（左右边缘齐平），
    // 并且绕这条公共边（铰链）转向用户 —— 面板法线指向头显中心，这样侧着看也读得清。
    DmPose hudPose;
    Vec3 hudTopCenter(0, 0, 0);          // 桌面窗口上沿中点（提示条挂在它上面）
    float hudW, hudH;
    {
        M4 Rs = M4FromQuat(shown.rot);
        Vec3 ex = XformDir(Rs, Vec3(1, 0, 0)).Norm();       // 屏幕右向 = 铰链轴
        Vec3 ey = XformDir(Rs, Vec3(0, 1, 0)).Norm();       // 屏幕上向
        Vec3 fw = XformDir(Rs, Vec3(0, 0, 1)).Norm();       // 屏幕法线（朝向用户）
        Vec3 dn(-ey.x, -ey.y, -ey.z);                       // 屏幕下向

        // 铰链取【桌面网格真实的底边中点顶点】，HUD 挂在正下方、水平居中，
        // 姿态与桌面窗口一致（不动、不转向头），跟着窗口一起走。
        Vec3 hinge = shown.pos - ey * scrHalfH;             // 兜底：公式位置
        hudTopCenter = shown.pos + ey * scrHalfH;           // 兜底：顶边中点
        {
            Quat invS(-shown.rot.w, -shown.rot.x, -shown.rot.y, -shown.rot.z);
            M4 Mi = M4FromQuat(invS);
            float lowest = 1e9f, highest = -1e9f;
            for (size_t k = 0; k < screenMesh.size(); k++) {
                Vec3 l = XformDir(Mi, Vec3(screenMesh[k].x, screenMesh[k].y, screenMesh[k].z) - shown.pos);
                if (l.y < lowest) lowest = l.y;
                if (l.y > highest) highest = l.y;
            }
            // 在"最靠下 / 最靠上"的那一圈顶点里取 |x| 最小的 = 该边中点
            float absXb = 1e9f, absXt = 1e9f; Vec3 wb = hinge, wt = hudTopCenter;
            for (size_t k = 0; k < screenMesh.size(); k++) {
                Vec3 w(screenMesh[k].x, screenMesh[k].y, screenMesh[k].z);
                Vec3 l = XformDir(Mi, w - shown.pos);
                if (l.y < lowest + 0.01f && fabsf(l.x) < absXb) { absXb = fabsf(l.x); wb = w; }
                if (l.y > highest - 0.01f && fabsf(l.x) < absXt) { absXt = fabsf(l.x); wt = w; }
            }
            if (absXb < 1e8f) hinge = wb;
            if (absXt < 1e8f) hudTopCenter = wt;
        }

        // 桌面窗口占的视角很大，HUD 做成窗口宽度的 40%，居中挂在窗口下沿
        hudW = scrHalfW * 2.0f * 0.40f;
        hudH = hudW * (float)g_hud.h / (float)g_hud.w;

        // 固定向内倾斜（绕铰链轴、法线向上翘）→ 面板朝视野中心/用户，位置仍固定在窗口下沿
        {
            Vec3 dnx(-ey.x, -ey.y, -ey.z);
            float ang = -Rad(20.0f);                        // 负 = 法线朝上，面向位于上方的用户
            Vec3 znew = fw * cosf(ang) + dnx * sinf(ang);
            Vec3 ynew = Vec3(znew.y * ex.z - znew.z * ex.y,
                             znew.z * ex.x - znew.x * ex.z,
                             znew.x * ex.y - znew.y * ex.x).Norm();
            M4 Rh = M4::Identity();
            Rh.m[0][0] = ex.x;   Rh.m[0][1] = ex.y;   Rh.m[0][2] = ex.z;
            Rh.m[1][0] = ynew.x; Rh.m[1][1] = ynew.y; Rh.m[1][2] = ynew.z;
            Rh.m[2][0] = znew.x; Rh.m[2][1] = znew.y; Rh.m[2][2] = znew.z;
            hudPose.rot = QuatFromM4(Rh);
            hudPose.pos = hinge + XformDir(Rh, Vec3(0, -hudH * 0.5f, 0));
        }
    }
    std::vector<DmVert> hudQuad;
    DmBuildFlatQuad(hudPose, hudW, hudH, &hudQuad);

    bool toastVisible = GetTickCount64() < g_toastUntil;
    std::vector<DmVert> toastQuad;
    if (toastVisible && g_toast.srv) {
        // 提示条（快捷键/状态）：挂在桌面窗口【上沿】外侧、水平居中，与窗口同朝向
        DmPose tp;
        tp.rot = shown.rot;
        M4 R = M4FromQuat(tp.rot);
        float tw = hudW * 0.62f;
        float th = tw * (float)g_toast.h / (float)g_toast.w;
        tp.pos = hudTopCenter + XformDir(R, Vec3(0, th * 0.5f + 0.02f, 0.0f));   // 再留 2cm 缝，别压住窗口
        DmBuildFlatQuad(tp, tw, th, &toastQuad);
    }

    float ipd = g_st.ipdMm * 0.001f;
    float tanX = g_st.fovTanX > 0.05f ? g_st.fovTanX : 1.0f;
    float tanY = g_st.fovTanY > 0.05f ? g_st.fovTanY : 1.0f;
    M4 proj = MakeProjRow(tanX, tanY, 0.01f, 1000.0f);   // 官方日志：near 0.01 / far 1000

    // 3DOF 基底：相机始终跟随头部旋转（背景固定在空间中）
    for (int e = 0; e < 2; e++) {
        float eyeX = (e == 0 ? -0.5f : 0.5f) * ipd * (g_st.swapEyes ? -1.0f : 1.0f);
        M4 rot = M4FromQuat(g_headRot);
        Vec3 eyePos = g_headPos + XformDir(rot, Vec3(eyeX, 0, 0));
        M4 eyeAbs = rot;                       // 行向量：旋转在前三行
        eyeAbs.m[3][0] = eyePos.x;
        eyeAbs.m[3][1] = eyePos.y;
        eyeAbs.m[3][2] = eyePos.z;
        // ★ 面板物理布局：左眼那块 1600x1600 转了 90°(CW)、右眼转了 90°(CCW)，
        //   所以每只眼的画面要先反向转回去。滚转要作用在【整幅画面】上：
        //   视图矩阵（桌面/HUD）和背景射线向量都得用同一份"带滚转的朝向"，
        //   否则背景不跟着转，只有桌面被拧过去。
        float rollDeg = (e == 0) ? g_st.rollLeft : g_st.rollRight;
        M4 rotE = rot;
        if (fabsf(rollDeg) > 0.01f) {
            float hh = Rad(rollDeg) * 0.5f;
            Quat qr(cosf(hh), 0.0f, 0.0f, sinf(hh));   // 绕 +Z（视轴）
            rotE = Mul(rot, M4FromQuat(qr));
        }
        M4 eyeAbsE = rotE;
        eyeAbsE.m[3][0] = eyePos.x;
        eyeAbsE.m[3][1] = eyePos.y;
        eyeAbsE.m[3][2] = eyePos.z;
        M4 view = InverseRigid(eyeAbsE);

        g_gfx.BeginEyeOffscreen(view, proj);

        // 几何自检：把桌面四角投影到 NDC，确认是否居中/多大
        if (g_frameCount == 30 && !screenMesh.empty()) {
            M4 vp = Mul(view, proj);
            float mnx = 1e9f, mxx = -1e9f, mny = 1e9f, mxy = -1e9f;
            for (size_t k = 0; k < screenMesh.size(); k++) {
                const DmVert& v = screenMesh[k];
                float px = v.x * vp.m[0][0] + v.y * vp.m[1][0] + v.z * vp.m[2][0] + vp.m[3][0];
                float py = v.x * vp.m[0][1] + v.y * vp.m[1][1] + v.z * vp.m[2][1] + vp.m[3][1];
                float pw = v.x * vp.m[0][3] + v.y * vp.m[1][3] + v.z * vp.m[2][3] + vp.m[3][3];
                if (fabsf(pw) < 1e-6f) continue;
                float nx = px / pw, ny = py / pw;
                if (nx < mnx) mnx = nx; if (nx > mxx) mxx = nx;
                if (ny < mny) mny = ny; if (ny > mxy) mxy = ny;
            }
            char gbuf[256];
            sprintf(gbuf, "几何自检 eye%d: NDC x=%.3f..%.3f y=%.3f..%.3f 中心=(%.3f,%.3f) scale=%.3f dist=%.2f",
                    e, mnx, mxx, mny, mxy, (mnx+mxx)*0.5f, (mny+mxy)*0.5f, g_st.scale, g_st.distance);
            DmLog(gbuf);
        }

        Vec3 right = XformDir(rotE, Vec3(1, 0, 0));
        Vec3 up    = XformDir(rotE, Vec3(0, 1, 0));
        Vec3 fwd   = XformDir(rotE, Vec3(0, 0, -1));
        int bgMode = (g_st.bgMode == 1) ? 1 : (g_st.bgMode == 2 ? 9 : g_st.venvMode);   // 2=黑底 → 9 纯黑
        if (bgMode == 3) bgMode = 2;                      // 官方天空盒走全景采样
        if (bgMode == 2) {
            // 自定义全景：按类型/眼别选择采样方式
            int k = g_st.panoKind;
            if (k == 1) bgMode = 4;
            else if (k == 2) bgMode = (e == 0) ? 5 : 6;
            else if (k == 3) bgMode = (e == 0) ? 7 : 8;
        }
        ID3D11ShaderResourceView* pano = (bgMode == 2) ? g_panoSrv : NULL;
        if (bgMode == 2 && !pano) bgMode = 1;
        if (bgMode >= 0)
            g_gfx.DrawBackground(bgMode, g_st.bgBrightness, right, up, fwd, tanX, tanY, pano);

        if (!g_screenHidden && g_st.opacity > 0.001f && g_cap.srv) {
            float u0 = 0, u1 = 1, v0 = 0, v1 = 1;
            bool rightEye = (e == 1) ^ (g_st.swapEyes != 0);
            switch (g_st.sbsMode) {
            case 0: break;
            case 1:
                if (rightEye) { u0 = 0.5f; u1 = 1.0f; } else { u0 = 0.0f; u1 = 0.5f; }
                break;
            case 2:             // 全宽 FSBS：各取一半（整幅高度，不裁切）
                if (rightEye) { u0 = 0.5f; u1 = 1.0f; } else { u0 = 0.0f; u1 = 0.5f; }
                break;
            case 3:
                if (rightEye) { v0 = 0.5f; v1 = 1.0f; } else { v0 = 0.0f; v1 = 0.5f; }
                break;
            }
            if (g_st.sbsMode != 0 && g_st.parallax != 0.0f) {
                float shift = g_st.parallax * (rightEye ? -1.0f : 1.0f);
                u0 += shift; u1 += shift;
            }
            // 指针合成：把指针纹理按它在桌面里的矩形交给着色器，跟着曲面一起弯
            if (g_cap.curVisible && g_cap.curSrv && g_st.sbsMode == 0 && g_cap.w > 0 && g_cap.h > 0) {
                float pw = (float)g_cap.curW / (float)g_cap.w;
                float ph = (float)g_cap.curH / (float)g_cap.h;
                float cu0 = g_cap.curU - (float)g_cap.curHotX / (float)g_cap.w;
                float cv0 = g_cap.curV - (float)g_cap.curHotY / (float)g_cap.h;
                g_gfx.SetCursorTexture(g_cap.curSrv);
                g_gfx.SetCursorRect(cu0, cv0, cu0 + pw, cv0 + ph);
            } else {
                g_gfx.SetCursorTexture(NULL);
                g_gfx.SetCursorRect(0, 0, 0, 0);
            }
            g_gfx.SetUv(u0, v0, u1, v1);
            g_gfx.DrawQuad(screenMesh.data(), (int)screenMesh.size(), g_cap.srv,
                           g_screenHidden ? 0.0f : g_st.opacity, false);
            g_gfx.SetCursorTexture(NULL);
            g_gfx.SetCursorRect(0, 0, 0, 0);
            if (rangeHintOn && !rangeRing.empty()) {
                g_gfx.SetUv(-1.0f, -1.0f, -1.0f, -1.0f);     // 纯色
                g_gfx.DrawQuad(rangeRing.data(), (int)rangeRing.size(), g_cap.srv, 0.85f, false);
            }

            if (false && g_st.sbsMode == 0 && g_cap.curVisible && g_cap.curSrv && g_st.opacity > 0.2f) {  // 指针已合成进桌面
                float halfW = g_st.distance * tanf(Rad(g_st.fovBaseDeg * 0.5f)) * g_st.scale;
                float screenW = halfW * 2.0f;
                float screenH = screenW / uiAspect;
                float szX = screenW * (float)g_cap.curW / (float)(g_cap.w ? g_cap.w : 1);
                float szY = screenH * (float)g_cap.curH / (float)(g_cap.h ? g_cap.h : 1);
                // 光标位置是“热点”，图标要放到 热点-热点偏移 处，否则整体偏右下
                float uTL = g_cap.curU - (g_cap.w ? (float)g_cap.curHotX / (float)g_cap.w : 0.0f);
                float vTL = g_cap.curV - (g_cap.h ? (float)g_cap.curHotY / (float)g_cap.h : 0.0f);
                std::vector<DmVert> cq;
                DmBuildCursorQuad(g_st, shown, uiAspect, uTL, vTL, szX, szY, &cq);
                g_gfx.SetUv(0, 0, 1, 1);
                g_gfx.DrawQuad(cq.data(), (int)cq.size(), g_cap.curSrv, g_st.opacity, true);
            }
        }

        if (g_hud.srv && (g_st.hudPanel || g_st.hudKeys)) {
            g_gfx.SetUv(0, 0, 1, 1);
            g_gfx.DrawQuad(hudQuad.data(), (int)hudQuad.size(), g_hud.srv, 0.95f, false);
        }
        if (toastVisible && g_toast.srv && toastQuad.size()) {
            float remain = (float)(g_toastUntil - GetTickCount64()) / 1500.0f;
            float a = Clampf(remain * 1.6f, 0.0f, 1.0f);
            g_gfx.SetUv(0, 0, 1, 1);
            g_gfx.DrawQuad(toastQuad.data(), (int)toastQuad.size(), g_toast.srv, a, false);
        }
        g_gfx.EndEye();
        // 畸变校正：把这只眼的画面按径向多项式重采样后贴到面板对应半边
        g_gfx.BlitEyeToRT(e == 0 ? 0 : perEyeW, perEyeW, perEyeH, g_st.lensK, g_st.lensD, g_st.lensScale,
                          g_st.lensMode, g_st.lensZoom,
                          (float)g_st.eyeMirrorX[e], (float)g_st.eyeMirrorY[e],
                          g_st.eyeDx[e] / (float)perEyeW, g_st.eyeDy[e] / (float)perEyeH,
                          Rad(g_st.eyeRot[e]) + (g_st.flip180 ? Rad(180.0f) : 0.0f));
    }
    // 校准图案：每只眼的四角彩块 + 中心十字（判断镜片实际覆盖面板的哪一块）
    if (g_st.alignOverlay) {
        const float W = (float)perEyeW, H = (float)perEyeH;
        for (int e = 0; e < 2; e++) {
            float x0 = (e == 0) ? 0.0f : W;
            g_gfx.FillRectPanel(x0, 0, W, 6, 1, 1, 1);            // 上边
            g_gfx.FillRectPanel(x0, H - 6, W, 6, 1, 1, 1);        // 下边
            g_gfx.FillRectPanel(x0, 0, 6, H, 1, 1, 1);            // 左边
            g_gfx.FillRectPanel(x0 + W - 6, 0, 6, H, 1, 1, 1);    // 右边
            g_gfx.FillRectPanel(x0 + W * 0.5f - 80, H * 0.5f - 5, 160, 10, 1, 1, 1);  // 中心十字
            g_gfx.FillRectPanel(x0 + W * 0.5f - 5, H * 0.5f - 80, 10, 160, 1, 1, 1);
            float b = 90;
            g_gfx.FillRectPanel(x0 + 12, 12, b, b, 1, 0, 0);                    // 左上 红
            g_gfx.FillRectPanel(x0 + W - 12 - b, 12, b, b, 0, 1, 0);            // 右上 绿
            g_gfx.FillRectPanel(x0 + 12, H - 12 - b, b, b, 0, 0, 1);            // 左下 蓝
            g_gfx.FillRectPanel(x0 + W - 12 - b, H - 12 - b, b, b, 1, 1, 0);    // 右下 黄
        }
    }
    if (g_out.IsValid()) g_out.Present(g_gfx.ctx, g_gfx.rtTex);
    else Sleep(11);                 // 无头模式限一下速，别把 CPU 吃满
    g_frameCount++;
    g_fpsFrames++;
}

// 结束 SteamVR 进程（桌面模式期间防止它被自动拉起抢走面板）。返回成功结束的个数。
static int DmKillVrProcesses() {
    const wchar_t* names[] = { L"vrserver.exe", L"vrmonitor.exe", L"vrcompositor.exe",
                               L"vrdashboard.exe", L"vrstartup.exe" };
    int killed = 0;
    HANDLE hs = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
    if (hs == INVALID_HANDLE_VALUE) return 0;
    PROCESSENTRY32W pe; ZeroMemory(&pe, sizeof(pe)); pe.dwSize = sizeof(pe);
    if (Process32FirstW(hs, &pe)) do {
        for (int i = 0; i < 5; i++) {
            if (!_wcsicmp(pe.szExeFile, names[i])) {
                HANDLE hp = OpenProcess(PROCESS_TERMINATE, FALSE, pe.th32ProcessID);
                if (hp) { if (TerminateProcess(hp, 0)) killed++; CloseHandle(hp); }
            }
        }
    } while (Process32NextW(hs, &pe));
    CloseHandle(hs);
    return killed;
}

// ---------------------------------------------------------------- 主循环
static int RunDesktopMode() {
    DmLogInit();
    DmLog("=== 桌面模式启动（独立渲染，不经过 SteamVR）===");
    if (GetFileAttributesW(DmIniPath().c_str()) == INVALID_FILE_ATTRIBUTES) {
        DmSettings def;
        DmSettingsDefault(&def);
        DmSettingsSave(&def);
        DmLog("已生成默认配置 desktop_mode.ini");
    }
    DmSettingsLoad(&g_st);
    g_settingsDirty = false;
    // 每次重开桌面模式都从 2D 开始（上次的 SBS 模式不带过来）
    if (g_st.sbsMode != 0) {
        g_st.sbsMode = 0;
        DmSettingsSave(&g_st);
        DmLog("启动：3D 模式已重置为 2D");
    }
    if (g_st.verboseLog) g_logVerbose = true;
    DmLog("面板朝向：相机滚转 左%.0f°/右%.0f°，贴屏旋转 左%.0f°/右%.0f° 翻转180=%d | 跟随 move_rate=%.0f°/s settle=%.2fs tau=%.2fs | imu=%d src=%d", g_st.rollLeft, g_st.rollRight, g_st.eyeRot[0], g_st.eyeRot[1], g_st.flip180, g_st.moveRateDeg, g_st.settleSec, g_st.tau, g_st.imuEnabled, g_st.imuSource);
    DmLog("配置：distance=%.2f scale=%.2f curv=%.2f opacity=%.2f follow=%d monitor=%d sbs=%d lens=%d 0dof=%d",
          g_st.distance, g_st.scale, g_st.curvature, g_st.opacity, g_st.followMode, g_st.monitorIndex,
          g_st.sbsMode, g_st.lensMode, g_st.zeroDof);
    {
        float hw = g_st.distance * tanf(Rad(g_st.fovBaseDeg * 0.5f)) * g_st.scale;
        float ang = 2.0f * atanf(hw / (g_st.distance > 0.01f ? g_st.distance : 0.01f)) * 57.2958f;
        DmLog("几何：距离=%.3f 大小=%.2f 半宽=%.4f 角宽=%.2f°", g_st.distance, g_st.scale, hw, ang);
    }

    std::string err;
    if (!g_gfx.Create("", &err)) {
        DmLog("图形初始化失败：%s", err.c_str());
        // No MessageBox: this exe is launched by the ALVR dashboard. A modal
        // dialog would block the user with no visible parent window. The error
        // is already logged above; the dashboard shows the log path.
        return 1;
    }
    if (g_st.outputEnabled) {
        // 屏幕的插拔由客户端统一负责（DisplayPlugIn/Unplug 只调一次，避免重复造屏）。
        // 这里只负责等屏出现；等不到就报错退出，不再自己调 dll 或改显示拓扑。
        // 显示切换交给 dm_output.cpp 的 EnableGlassesDisplay（精确枚举 + 重试）
        bool ok = false;
        for (int attempt = 0; attempt < 24 && !ok; attempt++) {
            if (g_out.Create(g_gfx.dev, -1, &err, g_st.exclusive != 0)) { ok = true; break; }   // -1 = 自动找眼镜那块 3200x1600 屏
            if (attempt == 0) DmLog("等待眼镜显示器回到扩展模式…（VR 退出后需要几秒）");
            Sleep(500);
        }
        if (!ok) {
            DmLog("输出窗口创建失败：%s", err.c_str());
            // No MessageBox: launched headless by the dashboard. Log only;
            // the dashboard reads desktop_mode.log and reports the failure.
            return 1;
        }
    } else {
        DmLog("无头模式：不建输出窗口，只渲染到内存（用于调试/截图）");
    }
    g_hud.Ensure(g_gfx.dev);
    if (!g_cap.Start(g_gfx.dev, g_gfx.ctx, g_st.monitorIndex, &err))
        DmLog("屏幕采集启动失败：%s", err.c_str());
    int fail[32], nf = 0;
    int nfail = g_hk.Install(g_st, fail, &nf);
    if (nfail) ToastShow(Fmt(L"%d 个热键被占用，见监控面板", nfail), 4000);
    ReloadPanorama();

    g_imu.gyroScale = g_st.gyroScale;
    g_imu.accelScale = g_st.accelScale;
    g_imu.mapMode = g_st.imuMap;
    if (g_st.imuEnabled && g_st.imuSource == 1) g_hmdPose.Start();
    if (g_st.imuEnabled && g_st.imuSource == 0) g_imu.Start();   // HID 自研解析（备用）

    // 等 IMU 第一帧（最多 2 秒）
    for (int i = 0; i < 200 && !g_imu.Connected(); i++) Sleep(10);
    Sleep(300);
    {
        float w, x, y, z;
        g_imu.GetQuat(&w, &x, &y, &z);
        g_headRot = Quat(w, x, y, z);
        g_headValid = g_imu.Connected();
        float hy, hp, hr;
        DmHeadYawPitchRoll(g_headRot, &hy, &hp, &hr);
        DmLog("IMU 首帧：q=(%.3f %.3f %.3f %.3f) accel=(%.2f %.2f %.2f) gyro=(%.1f %.1f %.1f) yaw=%.1f pitch=%.1f roll=%.1f",
              w, x, y, z, g_imu.accel[0], g_imu.accel[1], g_imu.accel[2],
              g_imu.gyro[0], g_imu.gyro[1], g_imu.gyro[2],
              hy * 57.2958f, hp * 57.2958f, hr * 57.2958f);
    }
    DmUiRegisterHotkey();
    g_scene.ResetToHead(g_headPos, g_headRot);

    ULONGLONG tPrev = GetTickCount64();
    ULONGLONG tHud = 0, tStat = 0, tSettings = 0;
    g_fpsT0 = tPrev;

    while (!g_quit) {
        MSG msg;
        int pump = 0;
        while (pump++ < 64 && PeekMessageW(&msg, NULL, 0, 0, PM_REMOVE)) {
            if (msg.message == WM_HOTKEY) {
                // 只用日志：动作统一由 Tick() 里的按键轮询触发，
                // 否则同一次按键会被 WM_HOTKEY 和轮询各触发一次（3D 模式会一次跳两档）
                DmLog("WM_HOTKEY id=0x%X（忽略，动作由轮询触发）", (unsigned)msg.wParam);
                if (DmUiIsOurHotkey((int)msg.wParam)) OpenSettingsWindow();
            } else if (msg.message == WM_QUIT) {
                g_quit = true;
            } else {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        if (g_quit) break;
        // 客户端退出后自动关闭（启动时传入 --parent <pid>）
        if (g_parentProc) {
            if (WaitForSingleObject(g_parentProc, 0) == WAIT_OBJECT_0) {
                DmLog("客户端已退出，桌面模式随之关闭");
                break;
            }
        }

        ULONGLONG now = GetTickCount64();
        double dt = (now - tPrev) / 1000.0;
        tPrev = now;

        g_hk.Tick(now, FireAction);
        // SteamVR 真起来时让出屏幕（SteamVR 会抢面板进直连模式）。
        // 但客户端切换流程里 SteamVR 会"被杀又自启"一下，所以：
        //   ① 启动后前 20 秒不看（宽限期，此时它在收尾/重试）
        //   ② 要连续 3 次（约 3 秒）都看到 vrserver 才认定是真启动
        {
            static ULONGLONG tCheck = 0;
            static ULONGLONG tStartW = GetTickCount64();
            static int vrHits = 0;
            if (now - tCheck > 1000) {
                tCheck = now;
                HANDLE hs = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
                if (hs != INVALID_HANDLE_VALUE) {
                    PROCESSENTRY32W pe; ZeroMemory(&pe, sizeof(pe)); pe.dwSize = sizeof(pe);
                    bool vr = false;
                    if (Process32FirstW(hs, &pe)) do {
                        if (!_wcsicmp(pe.szExeFile, L"vrserver.exe") || !_wcsicmp(pe.szExeFile, L"vrmonitor.exe")) vr = true;
                    } while (!vr && Process32NextW(hs, &pe));
                    CloseHandle(hs);
                    if (now - tStartW > 20000 && vr) {
                        vrHits++;
                        if (vrHits >= 3) {
                            // 桌面模式期间 SteamVR 会自己重启来抢屏：能杀就杀（有权限时），
                            // 杀不掉（普通权限）才让出屏幕退出。
                            int killed = DmKillVrProcesses();
                            DmLog("检测到 SteamVR 运行：已尝试结束 %d 个进程%s", killed,
                                  killed > 0 ? "，继续渲染" : "（权限不足），让出屏幕并退出");
                            if (killed <= 0) g_quit = true;
                            vrHits = 0;
                        }
                    } else {
                        vrHits = 0;
                    }
                }
            }
        }
        // 触发范围变了 → 在画面上显示 2 秒的范围圈
        if (g_lastRangeDeg != g_st.followRangeDeg) {
            g_lastRangeDeg = g_st.followRangeDeg;
            g_rangeHintUntil = now + 2000;
        }

        // 头姿（IMU）
        if (g_st.imuEnabled && g_imu.state == 3) {
            g_st.imuEnabled = 0;
            g_imu.Stop();
            ToastShow(L"IMU 数据不可用（陀螺仪噪声/零偏异常），已自动关闭头动");
            DmLog("IMU 数据自检未通过，已自动关闭头动（state=3）");
            MarkDirty();
        }
        if (!g_st.imuEnabled) {
            g_headRot = Quat();
            g_headValid = false;
        } else if (g_st.imuSource == 1) {
            // 官方 Hmd.dll::getRawPose 返回的是原始 9 轴数据：
            //   acc m/s² / gyro rad/s / mag µT（依据 K1=1/9.8、K2=π/180 与 imulog 格式）
            // 自己做 Mahony MARG 融合得到姿态四元数，而不是把加速度当角度用。
            static DmFusion fus = {0};
            static bool fusReady = false;
            static Quat qBase(1, 0, 0, 0);
            static bool baseInit = false;
            float acc[3], gyro[3], mag[3], ts = 0, st = 0;
            if (g_hmdPose.ReadRaw(acc, gyro, mag, &ts, &st)) {
                if (!fusReady) { fus.Init(); fusReady = true; }
                // 灵敏度：角速度低于门限视为静止抖动 → 角速度归零，不积分不漂
                fus.gyroGate = Rad(g_st.gyroGateDeg > 0.0f ? g_st.gyroGateDeg : 0.0f);

                // 拿起/戴上检测：加速度模长明显偏离 9.8 → 说明在被搬动；
                // 安静下来后自动做一次姿态归零，让当前朝向成为正前方。
                if (g_st.autoRezero && GetTickCount64() > g_suppressAutoZeroUntil) {
                    // ---- 拿起检测（按"动作幅度"，不是时长）----
                    // 实测：静止时加速度偏离 ≤0.35 m/s²、角速度 ≤1.7°/s；
                    //       从桌上拿起→戴上→放下时，加速度偏离峰值 9.08 m/s²、角速度峰值 547.7°/s。
                    // 判据：动作窗口内只要出现"够大"的动作（偏离 > 6 m/s² 或角速度 > 400°/s），
                    //       就算被搬动过；动作停下后静止 0.5 秒即自动归零。
                    const float kDevTrig  = 6.0f;     // m/s²（静止 0.35，拿起峰值 9.08）
                    const float kGyroTrig = 400.0f;   // °/s （静止 1.7，拿起峰值 547.7）
                    const float kActDev   = 1.0f;     // 判定"窗口还没停"的低门槛
                    const float kActGyro  = 20.0f;
                    const ULONGLONG kQuietMs = 500;    // 动作停下后静止多久归零
                    float an = sqrtf(acc[0] * acc[0] + acc[1] * acc[1] + acc[2] * acc[2]);
                    float dev = fabsf(an - 9.8f);
                    float gmag = sqrtf(gyro[0] * gyro[0] + gyro[1] * gyro[1] + gyro[2] * gyro[2]) * 57.29578f;
                    static float peakDev = 0.0f, peakGyro = 0.0f;
                    static bool  active = false;
                    static ULONGLONG tLastMotion = 0;
                    ULONGLONG nowH = GetTickCount64();
                    bool moving = (dev > kActDev) || (gmag > kActGyro);
                    if (moving) {
                        active = true;
                        tLastMotion = nowH;
                        if (dev > peakDev) peakDev = dev;
                        if (gmag > peakGyro) peakGyro = gmag;
                    } else if (active && nowH - tLastMotion > kQuietMs) {
                        active = false;
                        bool big = (peakDev > kDevTrig) || (peakGyro > kGyroTrig);
                        float pd = peakDev, pg = peakGyro;
                        peakDev = peakGyro = 0.0f;
                        if (big) {
                            extern volatile LONG g_poseZeroReq;
                            g_poseZeroReq = 1;
                            DmLog("检测到拿起（峰值 偏离%.2f m/s² / 角速度%.0f°/s）后静止：已自动姿态归零", pd, pg);
                            ToastShow(L"已自动归零（当前朝向=正前方）");
                        } else {
                            DmLog("动作幅度不够（峰值 偏离%.2f m/s² / 角速度%.0f°/s）：不归零", pd, pg);
                        }
                    }
                }

                fus.Update(acc, gyro, mag, (float)dt);
                Quat q(fus.qw, fus.qx, fus.qy, fus.qz);

                // 起始姿态归零：把戴上时的朝向当作 0（按归零键可重设）
                extern volatile LONG g_poseZeroReq;
                // 等融合收敛（约 2 秒 / 120 帧）后再取基准，否则启动瞬间的姿态是未收敛的
                // 融合前 3 秒（约 180 帧）还在收敛，基准每帧跟着更新；
                // 收敛后再冻结，避免"刚开启就带着十几度偏移"。
                if (g_poseZeroReq) {
                    // 只重置 IMU 基准：不改变用户对桌面窗口位置/平移的自定义
                    qBase = q; baseInit = true; g_poseZeroReq = 0;
                    g_oeResetReq = true;
                }
                else if (!baseInit || fus.frames < 300) { qBase = q; if (fus.frames >= 300) baseInit = true; }
                if (baseInit) {
                    Quat qi(qBase.w, -qBase.x, -qBase.y, -qBase.z);   // 单位四元数的逆 = 共轭
                    Quat rel = QNorm(QMul(qi, q));                    // 相对起始姿态
                    // 按轴取反：面板里可单独开关（这台机器的光学/面板朝向导致 pitch/roll 与 yaw 符号不同）
                    {
                        float hy, hp, hr;
                        DmHeadYawPitchRoll(rel, &hy, &hp, &hr);
                        if (g_st.swapPitchRoll) { float t = hp; hp = hr; hr = t; }   // 轴对调：pitch ↔ roll
                        if (g_st.invertPose) {
                            if (g_st.invertYaw)   hy = -hy;
                            if (g_st.invertPitch) hp = -hp;
                            if (g_st.invertRoll)  hr = -hr;
                        }
                        // ---- 3DoF 防抖：阈值内（接近静止）滤波，阈值外（在转头）直接透传 ----
                        // 之前的自适应滤波在中等转速也有残留延迟；改成"只在静止时压抖动"：
                        //   |角速度| < oe_still_speed  → 启用 1€ 低通（截止频率低，抖动压得狠）
                        //   |角速度| > oe_still_speed×1.6 → 关闭滤波，原始姿态直通（零延迟）
                        // 用 1.6 倍迟滞避免在阈值附近来回切；切回滤波时把原始值喂进滤波器保持状态连续。
                        if (g_st.oeEnabled) {
                            static DmOneEuro oeY(90, 1.0, 0.05), oeP(90, 1.0, 0.05), oeR(90, 1.0, 0.05);
                            static bool oeReady = false;
                            static int  oeKey = -1;
                            static bool oeStill = true;
                            int key = (int)(g_st.oeMinCutoff * 1000.0f) * 100000
                                    + (int)(g_st.oeMoveCutoff * 1000.0f) * 10
                                    + (int)(g_st.oeStillSpeed * 10.0f);
                            double fq = 1.0 / ((dt > 1e-4) ? dt : 0.011);
                            if (!oeReady || key != oeKey) {
                                oeReady = true; oeKey = key;
                                DmLog("防抖：静止截止=%.2fHz 转动截止=%.2fHz 静止阈值=%.1f°/s（迟滞 1.5×）",
                                      g_st.oeMinCutoff, g_st.oeMoveCutoff, g_st.oeStillSpeed);
                            }
                            if (g_oeResetReq) { oeY.reset(); oeP.reset(); oeR.reset(); g_oeResetReq = false; oeStill = true; }
                            oeY.setFrequency(fq); oeP.setFrequency(fq); oeR.setFrequency(fq);
                            // 实测（头显戴好静止 60 秒）：角速度模长 均值 0.62、p99 2.15、最大 4.07 °/s
                            float spd = sqrtf(gyro[0] * gyro[0] + gyro[1] * gyro[1] + gyro[2] * gyro[2]) * 57.29578f;
                            float thrOn  = g_st.oeStillSpeed > 0.1f ? g_st.oeStillSpeed : 4.0f;
                            float thrOff = thrOn * 1.5f;
                            if (oeStill) { if (spd > thrOff) oeStill = false; }
                            else         { if (spd < thrOn)  oeStill = true;  }
                            // 静止 → 重滤波（1Hz，抖动压狠）；转动 → 轻滤波（15Hz，τ≈10ms），
                            // 只抹掉 IMU 离散更新造成的"一格一格"卡顿，不产生可感知延迟。
                            double mc = oeStill ? g_st.oeMinCutoff : g_st.oeMoveCutoff;
                            oeY.setParams(fq, mc, 0.0, 1.0);
                            oeP.setParams(fq, mc, 0.0, 1.0);
                            oeR.setParams(fq, mc, 0.0, 1.0);
                            const double kRad2Deg = 57.29577951308232, kDeg2Rad = 0.017453292519943295;
                            hy = (float)(oeY.filter(hy * kRad2Deg) * kDeg2Rad);
                            hp = (float)(oeP.filter(hp * kRad2Deg) * kDeg2Rad);
                            hr = (float)(oeR.filter(hr * kRad2Deg) * kDeg2Rad);
                        }

                        // 静止抖动抑制：连续死区（不是"冻结"）
                        //   输出 = sign(x)·max(0, |x| - db)  → 小抖动被削掉，但输出始终连续，
                        //   不会出现"停住-突然跳"的卡顿（之前用 hold 导致特别卡）。
                        {
                            float db = Rad(g_st.poseDeadband > 0.0f ? g_st.poseDeadband : 0.0f);
                            if (db > 0.0001f) {
                                hy = (fabsf(hy) <= db) ? 0.0f : (hy - (hy > 0 ? db : -db));
                                hp = (fabsf(hp) <= db) ? 0.0f : (hp - (hp > 0 ? db : -db));
                                hr = (fabsf(hr) <= db) ? 0.0f : (hr - (hr > 0 ? db : -db));
                            }
                        }
                        g_headRot = QuatFromYawPitchRoll(hy, hp, hr);
                    }
                } else {
                    g_headRot = Quat();
                }

                {   // 每秒记录一次融合结果，便于核对官方数据
                    static ULONGLONG tL = 0;
                    ULONGLONG nowF = GetTickCount64();
                    if (nowF - tL > 1000) {
                        tL = nowF;
                        float yw, pt, rl;
                        fus.EulerDeg(&yw, &pt, &rl);
                        char fb[240];
                        sprintf(fb, "融合姿态 yaw=%.1f pitch=%.1f roll=%.1f | acc=(%.2f %.2f %.2f) gyro=(%.3f %.3f %.3f) mag=(%.1f %.1f %.1f)",
                                yw, pt, rl, acc[0], acc[1], acc[2], gyro[0], gyro[1], gyro[2], mag[0], mag[1], mag[2]);
                        DmLog(fb);
                    }
                }
                g_headValid = true;
            } else {
                g_headValid = false;
            }

        } else if (g_imu.Connected()) {
            float w, x, y, z;
            g_imu.GetQuat(&w, &x, &y, &z);
            Quat q = QNorm(Quat(w, x, y, z));
            if (g_st.imuYawOffset != 0 || g_st.imuPitchOffset != 0 || g_st.imuRollOffset != 0) {
                Quat qo = QuatFromYawPitchRoll(Rad(g_st.imuYawOffset), Rad(g_st.imuPitchOffset), Rad(g_st.imuRollOffset));
                q = QMul(q, qo);
            }
            g_headRot = q;
            g_headValid = true;
        } else {
            g_headValid = false;
        }

        if (g_cap.started && g_cap.Acquire()) g_capFramesStat++;
        g_cap.UpdateCursor(g_gfx.dev);

        // 调试：给头姿叠加模拟角度，用于验证背景是否随头部旋转（3DOF）
        if (g_st.simYaw != 0.0f || g_st.simPitch != 0.0f) {
            static ULONGLONG tSim = 0;
            ULONGLONG nowSim = GetTickCount64();
            if (nowSim - tSim > 1000) {
                tSim = nowSim;
                float sy, sp, sr;
                DmHeadYawPitchRoll(g_headRot, &sy, &sp, &sr);
                char sb[160];
                sprintf(sb, "模拟注入 sim_yaw=%.1f sim_pitch=%.1f → 姿态 yaw=%.1f pitch=%.1f",
                        g_st.simYaw, g_st.simPitch, sy * 57.2958f, sp * 57.2958f);
                DmLog(sb);
            }
            Quat qs = QuatFromYawPitchRoll(Rad(g_st.simYaw), Rad(g_st.simPitch), 0.0f);
            g_headRot = QMul(qs, g_headRot);
        }
        g_scene.Update((float)dt, g_headPos, g_headRot, g_st, false);

        if (now - tHud > 200 || (g_toastText != g_toastShown && now < g_toastUntil)) {
            tHud = now;
            if (g_st.hudPanel) {
                // 横向/竖向两种纹理尺寸（换方向时重建纹理）
                bool vert = (g_st.hudPos == 1 || g_st.hudPos == 2);
                uint32_t wantW = vert ? 620u : 1400u;
                uint32_t wantH = vert ? 900u : 520u;
                if (g_hud.w != wantW || g_hud.h != wantH) {
                    g_hud.Destroy();
                    g_hud.w = wantW; g_hud.h = wantH;
                }
                DmHudData d;
                BuildHudData(&d);
                g_hud.Rebuild(g_gfx.dev, g_gfx.ctx, d, g_st.hudFontPx);
            }
            if (!g_toastText.empty() && g_toastText != g_toastShown) {
                g_toast.Rebuild(g_gfx.dev, g_gfx.ctx, g_toastText, g_st.hudFontPx);   // 与监控面板字号统一
                g_toastShown = g_toastText;
            }
        }

        if (now - tStat > 500) {
            double secs = (now - g_fpsT0) / 1000.0;
            g_renderFps = g_fpsFrames / (secs > 0.001 ? secs : 1.0);
            g_capFps = g_capFramesStat / (secs > 0.001 ? secs : 1.0);
            g_frameMs = g_fpsFrames > 0 ? (secs * 1000.0 / g_fpsFrames) : 0.0;
            g_fpsT0 = now; g_fpsFrames = 0; g_capFramesStat = 0;
            tStat = now;
            if (g_st.verboseLog)
                DmLogV("fps=%.1f cap=%.1f frame=%.2fms imu=%d yaw=%.1f", g_renderFps, g_capFps, g_frameMs,
                       g_headValid ? 1 : 0, [&]{ float a,b,c; DmHeadYawPitchRoll(g_headRot,&a,&b,&c); return a*57.2958f; }());

            // 出图健康检查：开了垂直同步却跑得比刷新率高 1.5 倍以上 = 输出端根本没在消费帧，
            // 这块屏只是"Windows 认为它是活动的"，实际没有扫描出画面（面板待机 / direct mode 残留）。
            if (g_st.vsync != 0 && g_out.IsValid()) {
                uint32_t hz = 90;
                if (g_out.h > 0) {
                    DEVMODEW dm; ZeroMemory(&dm, sizeof(dm)); dm.dmSize = sizeof(dm);
                    if (EnumDisplaySettingsW(NULL, ENUM_CURRENT_SETTINGS, &dm) && dm.dmDisplayFrequency > 1)
                        hz = dm.dmDisplayFrequency;
                }
                if (g_renderFps > hz * 1.5 && !g_scanWarned) {
                    g_scanWarned = true;
                    DmLog("★ 警告：输出端没有按垂直同步供帧（fps=%.0f >> %uHz），这块屏很可能没有真正在出画面"
                          "（面板待机 / 被 VR 运行时留在 direct mode）。先看重启后 Windows 桌面能不能显示在这块屏上。",
                          g_renderFps, hz);
                } else if (g_renderFps <= hz * 1.5) {
                    g_scanWarned = false;
                }
            }
        }

        // 鼠标约束：不让光标进入眼镜那块屏（它是扩展屏，鼠标能划进去，会干扰画面/抢焦点）
        if (g_st.keepMouseOut && g_out.IsValid()) {
            static ULONGLONG tMouse = 0;
            if (now - tMouse > 33) {
                tMouse = now;
                POINT pt;
                if (GetCursorPos(&pt)) {
                    HMONITOR hm = MonitorFromPoint(pt, MONITOR_DEFAULTTONULL);
                    HMONITOR hg = MonitorFromWindow(g_out.hwnd, MONITOR_DEFAULTTONEAREST);
                    if (hm && hm == hg) {
                        // 推到主屏（眼镜屏左边那块）最靠右的位置
                        POINT back = pt;
                        HMONITOR hp = MonitorFromPoint(POINT{0, 0}, MONITOR_DEFAULTTOPRIMARY);
                        MONITORINFO mi; ZeroMemory(&mi, sizeof(mi)); mi.cbSize = sizeof(mi);
                        if (hp && GetMonitorInfoW(hp, &mi)) {
                            back.x = (mi.rcMonitor.right > mi.rcMonitor.left) ? (mi.rcMonitor.right - 2) : 0;
                            if (mi.rcMonitor.top <= pt.y && pt.y < mi.rcMonitor.bottom) back.y = pt.y;
                            else back.y = (mi.rcMonitor.top + mi.rcMonitor.bottom) / 2;
                        } else {
                            back.x = pt.x > 0 ? 2 : (GetSystemMetrics(SM_CXSCREEN) - 2);
                        }
                        SetCursorPos(back.x, back.y);
                    }
                }
            }
        }

        if (now - tSettings > 600) { tSettings = now; CheckSettingsFile(); }
        if (g_settingsDirty && now - g_lastSave > 1200) SaveNow();

        RenderFrame(dt);
        // 垂直同步开着却没有真正的 vblank 时（面板没出图），Present 会立刻返回，
        // 循环会飙到几千帧把 CPU 吃满；这里自己按刷新周期兜底限速。
        if (g_st.vsync != 0 && g_out.IsValid()) {
            ULONGLONG frameEnd = GetTickCount64();
            ULONGLONG spent = frameEnd - now;
            if (spent < 11) Sleep((DWORD)(11 - spent));
        }
        if (g_st.dumpFrame > 0 && (int)g_frameCount == g_st.dumpFrame) {
            DumpRTToBmp(g_gfx.dev, g_gfx.ctx, g_gfx.rtTex, (DmExeDir() + L"\\frame_dump.bmp").c_str());
            DumpRTToBmp(g_gfx.dev, g_gfx.ctx, g_cap.tex, (DmExeDir() + L"\\capture_dump.bmp").c_str());
            DumpRTToBmp(g_gfx.dev, g_gfx.ctx, g_hud.tex, (DmExeDir() + L"\\hud_dump.bmp").c_str());
        }
    }

    SaveNow();
    g_hk.Uninstall();
    g_imu.Stop();
    g_cap.Stop();
    g_hud.Destroy();
    g_toast.Destroy();
    if (g_panoSrv) g_gfx.ReleaseSrv(g_panoSrv);
    g_out.Destroy();
    g_gfx.Destroy();
    DmLog("=== 桌面模式退出 ===");
    return 0;
}

int WINAPI wWinMain(HINSTANCE, HINSTANCE, LPWSTR cmdline, int) {
    // 解析 --parent <pid>：由客户端拉起时，客户端退出后本程序自动退出
    {
        std::wstring cl = GetCommandLineW();
        size_t p = cl.find(L"--parent");
        if (p != std::wstring::npos) {
            int pid = _wtoi(cl.c_str() + p + 8);
            if (pid > 0) g_parentProc = OpenProcess(SYNCHRONIZE, FALSE, (DWORD)pid);
        }
    }
    // ★ 必须在任何窗口/显示器查询之前设置：不然眼镜屏（150% 缩放）会被 Windows
    //   缩成 2133x1067 的逻辑尺寸，交换链和客户区对不上，画面出不来。
    {
        HMODULE u32 = GetModuleHandleW(L"user32.dll");
        typedef BOOL (WINAPI* PFN_SetCtx)(HANDLE);
        PFN_SetCtx setCtx = u32 ? (PFN_SetCtx)GetProcAddress(u32, "SetProcessDpiAwarenessContext") : NULL;
        if (!setCtx || !setCtx((HANDLE)-4 /* PER_MONITOR_AWARE_V2 */)) SetProcessDPIAware();
    }
    HANDLE once = CreateMutexW(NULL, TRUE, L"HwVRDesktop_SingleInstance");
    if (once && GetLastError() == ERROR_ALREADY_EXISTS) {
        // No MessageBox: just log and exit quietly. The dashboard checks
        // renderer_running() before spawning and shows the status in its UI.
        DmLog("桌面模式已在运行，跳过本次启动");
        if (g_logf) { fflush(g_logf); }
        return 0;
    }
    CoInitializeEx(NULL, COINIT_APARTMENTTHREADED);
    int rc = RunDesktopMode();
    CoUninitialize();
    return rc;
}

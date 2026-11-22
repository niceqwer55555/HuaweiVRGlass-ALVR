// dm_hmdpose.cpp
#include "dm_hmdpose.h"
#include "dm_log.h"
#include <stdio.h>
#include <cmath>

typedef void* (*PFN_Create)();
typedef void* (*PFN_Pose)(void* out);
typedef void  (*PFN_Void)();

static const wchar_t* kCandidates[] = {
    L"C:\\Program\\HwPCVR Assistant\\pcCastScreen",
    L"C:\\Program Files\\HwVRGlassLite\\steamVRDriver\\huaweiVR\\bin\\win64",
    NULL
};

bool DmHmdPose::Start() {
    DmLog("姿态源：开始加载官方 Hmd.dll");
    if (dll) return true;
    wchar_t dir[MAX_PATH] = {0};
    for (int i = 0; kCandidates[i]; i++) {
        wchar_t probe[MAX_PATH];
        swprintf(probe, MAX_PATH, L"%ls\\Hmd.dll", kCandidates[i]);
        if (GetFileAttributesW(probe) != INVALID_FILE_ATTRIBUTES) { wcscpy(dir, kCandidates[i]); break; }
    }
    if (!dir[0]) { DmLog("姿态源：官方 Hmd.dll 没找到"); return false; }

    SetDllDirectoryW(dir);
    SetCurrentDirectoryW(dir);
    dll = LoadLibraryW(L"Hmd.dll");
    if (!dll) { DmLog("加载 Hmd.dll 失败（err=%lu）", GetLastError()); return false; }

    crtFn = (void*)GetProcAddress(dll, "createHmdDriver");
    rawFn = (void*)GetProcAddress(dll, "getRawPose");
    imuFn = (void*)GetProcAddress(dll, "enableHmdImu");
    if (!rawFn) { DmLog("Hmd.dll 里没有 getRawPose"); return false; }

    if (crtFn) drv = ((PFN_Create)crtFn)();
    if (imuFn) ((PFN_Void)imuFn)();
    DmLog("姿态源：已加载 官方 Hmd.dll (driver=%p rawPose=%p)", drv, rawFn);
    return true;
}

void DmHmdPose::Stop() {
    if (dll) { FreeLibrary(dll); dll = NULL; }
    drv = NULL; rawFn = crtFn = imuFn = NULL; ok = 0;
}

bool DmHmdPose::Read(float* a, float* b, float* c, float* d) {
    if (!rawFn) return false;
    // 官方 getRawPose 写入的字段数量未知，给足空间避免踩栈（曾经用 8 个 float 导致崩溃）
    float buf[64];
    ZeroMemory(buf, sizeof(buf));
    ((PFN_Pose)rawFn)(buf);
    last[0] = buf[0]; last[1] = buf[1]; last[2] = buf[2]; last[3] = buf[3];
    if (a) *a = buf[0];
    if (b) *b = buf[1];
    if (c) *c = buf[2];
    if (d) *d = buf[3];
    // 三个角全是 0 视为没有数据（未连接/未使能）
    bool have = (fabsf(buf[0]) > 0.001f || fabsf(buf[1]) > 0.001f || fabsf(buf[2]) > 0.001f);
    ok = have ? 1 : 0;
    return have;
}

// 官方 getRawPose：11 个 float
//   [0..2] 加速度 m/s²   [3..5] 角速度 rad/s   [6..8] 磁力计 µT   [9] 时间戳 s   [10] 状态
// 依据：Hmd.dll 里 K1=1/9.8（g→m/s²）、K2=π/180（度→弧度）、K3=1e9（ns→s），
//       以及 ./log/imulog.txt 的格式串 accel/gyro/magn/orientation。
bool DmHmdPose::ReadRaw(float acc[3], float gyro[3], float mag[3], float* t, float* status) {
    if (!rawFn) return false;
    float buf[64];
    ZeroMemory(buf, sizeof(buf));
    ((PFN_Pose)rawFn)(buf);

    if (acc)  { acc[0] = buf[0];  acc[1] = buf[1];  acc[2] = buf[2]; }
    if (gyro) { gyro[0] = buf[3]; gyro[1] = buf[4]; gyro[2] = buf[5]; }
    if (mag)  { mag[0] = buf[6];  mag[1] = buf[7];  mag[2] = buf[8]; }
    if (t)    *t = buf[9];
    if (status) *status = buf[10];

    // 加速度模长应接近 9.8（重力）。远离这个量级说明没数据。
    float n = sqrtf(buf[0] * buf[0] + buf[1] * buf[1] + buf[2] * buf[2]);
    bool have = (n > 5.0f && n < 20.0f);
    ok = have ? 1 : 0;
    return have;
}

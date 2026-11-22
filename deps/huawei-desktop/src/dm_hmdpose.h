// dm_hmdpose.h —— 用官方 Hmd.dll 的 getRawPose 取姿态（厂商自己解析 IMU，比我们猜报文靠谱）
#pragma once
#include <windows.h>

struct DmHmdPose {
    HMODULE dll = NULL;
    void*   drv = NULL;
    void*   rawFn = NULL;      // getRawPose
    void*   crtFn = NULL;      // createHmdDriver
    void*   imuFn = NULL;      // enableHmdImu
    volatile LONG ok = 0;      // 1 = 正在出数据
    float last[4] = {0, 0, 0, 0};

    bool Start();
    void Stop();
    // 返回 false = 没数据（未连接/全 0）
    bool Read(float* a, float* b, float* c, float* d);
    // 官方 getRawPose 真实语义（11 个 float）：
    //   acc m/s² / gyro rad/s / mag µT / 时间戳 s / 状态
    bool ReadRaw(float acc[3], float gyro[3], float mag[3], float* t, float* status);
};

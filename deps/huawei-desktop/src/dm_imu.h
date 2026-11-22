// dm_imu.h —— 华为 VR Glass 6DoF 套件（Par-CG012）IMU：HID 直读 + Mahony 融合（3DoF 朝向）
//
// 实测报文（pid_3a09&mi_02，128 字节，约 1000 包/秒）：
//   偏移 22..27 : int16 LE ×3 = 加速度（x,y,z），静态模长约 4182 ≈ 1g → 4096 LSB/g
//   偏移 28..33 : int16 LE ×3 = 角速度（x,y,z），静止时约 0 → 16.4 LSB/(°/s)
#pragma once
#include <windows.h>
#include <cstdint>
#include <string>

struct DmImu {
    HANDLE thread = NULL;
    volatile LONG run = 0;
    volatile float q[4] = {1, 0, 0, 0};      // w,x,y,z（设备坐标系）
    volatile float gyro[3] = {0, 0, 0};      // °/s
    volatile float accel[3] = {0, 0, 0};     // g
    volatile LONG  samples = 0;
    volatile LONG  connected = 0;
    volatile LONG  state = 0;        // 0=未连 1=在出数据 2=套件待机（报文全 0）
    volatile LONG  recal = 0;        // 置 1 = 重新标定零偏（静止时按热键）

    // 可调参数
    float gyroScale = 16.4f;    // LSB / (°/s)
    float accelScale = 4096.0f; // LSB / g
    // 轴映射：把设备轴映射到 OpenVR 约定（X 右、Y 上、-Z 前）
    int   mapMode = 0;          // 0=原样 1=Y/Z 交换 2=X 取反 3=Z 取反 …

    void RequestRecalibrate() { InterlockedExchange(&recal, 1); }
    bool Start();
    void Stop();
    void GetQuat(float* w, float* x, float* y, float* z) {
        *w = q[0]; *x = q[1]; *y = q[2]; *z = q[3];
    }
    float SampleRate() const;
    bool Connected() const { return connected != 0; }
};

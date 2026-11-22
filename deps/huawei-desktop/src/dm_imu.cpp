// dm_imu.cpp
#include "dm_imu.h"
#include "dm_log.h"
#include <setupapi.h>
#include <hidsdi.h>
#include <stdio.h>
#include <math.h>
#include <vector>
#include <string>

static const GUID kHidGuid = { 0x4d1e55b2, 0xf16f, 0x11cf, { 0x88, 0xcb, 0x00, 0x11, 0x11, 0x00, 0x00, 0x30 } };

static std::wstring FindGlassImuHid() {
    HDEVINFO di = SetupDiGetClassDevsW(&kHidGuid, NULL, NULL, DIGCF_PRESENT | DIGCF_DEVICEINTERFACE);
    if (di == INVALID_HANDLE_VALUE) return L"";
    SP_DEVICE_INTERFACE_DATA ifd; ZeroMemory(&ifd, sizeof(ifd)); ifd.cbSize = sizeof(ifd);
    std::wstring found;
    for (DWORD i = 0; SetupDiEnumDeviceInterfaces(di, NULL, &kHidGuid, i, &ifd); i++) {
        DWORD need = 0; SetupDiGetDeviceInterfaceDetailW(di, &ifd, NULL, 0, &need, NULL);
        if (!need) continue;
        std::vector<BYTE> b(need);
        auto* d = (SP_DEVICE_INTERFACE_DETAIL_DATA_W*)b.data();
        d->cbSize = sizeof(SP_DEVICE_INTERFACE_DETAIL_DATA_W);
        if (!SetupDiGetDeviceInterfaceDetailW(di, &ifd, d, need, NULL, NULL)) continue;
        std::wstring p = d->DevicePath, low = p;
        for (auto& c : low) c = towlower(c);
        // 6DoF 套件的 IMU/相机通道
        if (low.find(L"vid_12d1&pid_3a09&mi_02") != std::wstring::npos) { found = p; break; }
        if (found.empty() && low.find(L"vid_12d1&pid_107e&mi_03") != std::wstring::npos) found = p;
    }
    SetupDiDestroyDeviceInfoList(di);
    return found;
}

static inline float RdF32(const BYTE* b, int o) { float f; memcpy(&f, b + o, 4); return f; }
static inline short RdI16(const BYTE* b, int o) { short v; memcpy(&v, b + o, 2); return v; }

struct ImuCtx {
    DmImu* self;
    double qw, qx, qy, qz;
    double ix, iy, iz;
};

static DWORD WINAPI ImuThread(LPVOID param) {
    DmImu* self = (DmImu*)param;
    double qw = 1, qx = 0, qy = 0, qz = 0, ix = 0, iy = 0, iz = 0;
    const double Kp = 1.0, Ki = 0.05;
    ULONGLONG tprev = GetTickCount64();
    std::vector<BYTE> ib(256);
    // 陀螺仪零偏标定：连上后先静止采 800 个样本（约 0.8s）取均值，之后一直减掉。
    // 采样期间做运动检测：方差过大说明眼镜在被拿着动，丢掉重采，避免把"运动"当零偏扣掉。
    double biasX = 0, biasY = 0, biasZ = 0;
    int    calN = 0;
    double calSX = 0, calSY = 0, calSZ = 0;
    double calS2X = 0, calS2Y = 0, calS2Z = 0;
    bool   calibrated = false;
    const int kCalN = 2000;   // 约 2 秒：陀螺这几字节噪声大，靠长均值把噪声平掉

    while (self->run) {
        std::wstring path = FindGlassImuHid();
        if (path.empty()) { self->connected = 0; Sleep(1000); continue; }
        HANDLE h = CreateFileW(path.c_str(), GENERIC_READ | GENERIC_WRITE,
                               FILE_SHARE_READ | FILE_SHARE_WRITE, NULL, OPEN_EXISTING, 0, NULL);
        if (h == INVALID_HANDLE_VALUE) {
            DmLog("IMU: 打开 HID 失败 err=%lu", GetLastError());
            self->connected = 0;
            Sleep(1000);
            continue;
        }
        DmLog("IMU: 已连接 %S", path.c_str());
        self->connected = 1;
        tprev = GetTickCount64();
        while (self->run) {
            OVERLAPPED ov; ZeroMemory(&ov, sizeof(ov));
            ov.hEvent = CreateEventW(NULL, TRUE, FALSE, NULL);
            DWORD got = 0;
            BOOL ok = ReadFile(h, ib.data(), (DWORD)ib.size(), &got, &ov);
            if (!ok && GetLastError() == ERROR_IO_PENDING) {
                if (WaitForSingleObject(ov.hEvent, 300) == WAIT_OBJECT_0) {
                    GetOverlappedResult(h, &ov, &got, FALSE);
                    ok = TRUE;
                } else {
                    CancelIo(h);
                }
            }
            CloseHandle(ov.hEvent);
            if (!ok) break;                 // 设备掉了 → 重连
            if (got < 40) continue;

            double ax = RdI16(ib.data(), 22) / self->accelScale;
            double ay = RdI16(ib.data(), 24) / self->accelScale;
            double az = RdI16(ib.data(), 26) / self->accelScale;
            double gx = RdI16(ib.data(), 28) / self->gyroScale;   // °/s
            double gy = RdI16(ib.data(), 30) / self->gyroScale;
            double gz = RdI16(ib.data(), 32) / self->gyroScale;

            // 轴映射（默认原样；可后续按实测调）
            if (self->mapMode == 1) { double t = ay; ay = az; az = t; }
            else if (self->mapMode == 2) { ax = -ax; gx = -gx; }
            else if (self->mapMode == 3) { az = -az; gz = -gz; }

            // 套件待机时 HID 仍在 1kHz 送包，但字段全 0（只剩心跳头），
            // 这种情况下积分只会得到噪声，直接跳过并标记状态。
            {
                double am = sqrt(ax * ax + ay * ay + az * az);
                if (am < 0.2) {
                    self->state = 2;
                    self->accel[0] = 0; self->accel[1] = 0; self->accel[2] = 0;
                    self->gyro[0] = 0; self->gyro[1] = 0; self->gyro[2] = 0;
                    InterlockedIncrement(&self->samples);
                    continue;
                }
                self->state = 1;
            }

            ULONGLONG now = GetTickCount64();
            double dt = (now - tprev) / 1000.0;
            tprev = now;
            if (dt <= 0 || dt > 0.05) dt = 0.001;

            // 需要标定时（首次连上 / 用户按热键重新归零）：一边采样一边继续用旧零偏积分
            if (!calibrated || self->recal != 0) {
                calSX += gx; calSY += gy; calSZ += gz;
                calS2X += ax; calS2Y += ay; calS2Z += az;   // 用加速度判"有没有在动"
                if (++calN >= kCalN) {
                    double mX = calSX / calN, mY = calSY / calN, mZ = calSZ / calN;
                    double aX = calS2X / calN, aY = calS2Y / calN, aZ = calS2Z / calN;
                    double a2 = aX * aX + aY * aY + aZ * aZ;   // 加速度合矢量稳定 = 静止
                    static double aRef = 0; static int aN = 0;
                    if (aRef < 0.5) { aRef = a2; aN = 0; }
                    double varA = fabs(a2 - aRef);
                    aN++;
                    double vX = varA, vY = varA, vZ = varA;
                    // 加速度合矢量变化 > 0.02 (≈0.1g) 才算在动；陀螺噪声大不影响标定，均值会把噪声平掉
                    if (varA > 0.02) {
                        calN = 0; calSX = calSY = calSZ = 0; calS2X = calS2Y = calS2Z = 0;
                        DmLog("IMU 标定：检测到眼镜在动，重新采集（加速度变化 %.4f）", varA);
                    } else {
                        biasX = mX; biasY = mY; biasZ = mZ;
                        InterlockedExchange(&self->recal, 0);
                        if (!calibrated) {
                            calibrated = true;
                            self->q[0] = 1; self->q[1] = self->q[2] = self->q[3] = 0;
                        }
                        calN = 0; calSX = calSY = calSZ = 0; calS2X = calS2Y = calS2Z = 0;
                        DmLog("IMU 零偏标定完成：%.2f %.2f %.2f °/s（%d 样本）", biasX, biasY, biasZ, kCalN);
                    }
                }
                if (!calibrated) {
                    self->accel[0] = (float)ax; self->accel[1] = (float)ay; self->accel[2] = (float)az;
                    self->gyro[0] = (float)gx; self->gyro[1] = (float)gy; self->gyro[2] = (float)gz;
                    InterlockedIncrement(&self->samples);
                    continue;
                }
            }
            gx -= biasX; gy -= biasY; gz -= biasZ;
            // 这几字节的噪声有 ±18°/s，直接积分会抖成一团：一阶低通（rc≈0.15s）后再用，
            // 反应慢一点正好符合"延迟跟随防眩晕"的诉求。
            {
                static double fx = 0, fy = 0, fz = 0;
                const double rc = 0.15;
                double k = dt / (rc + dt);
                fx += (gx - fx) * k;
                fy += (gy - fy) * k;
                fz += (gz - fz) * k;
                gx = fx; gy = fy; gz = fz;
            }
            if (fabs(gx) < 1.0) gx = 0;      // 静止死区
            if (fabs(gy) < 1.0) gy = 0;
            if (fabs(gz) < 1.0) gz = 0;

            // 数据可信度：静止时若扣完零偏的角速度仍然持续偏大，说明这几字节不是可用陀螺仪
            // （噪声/零偏不可消除），累计 3 秒就标记 state=3，由上层自动关闭头动。
            {
                static double accX = 0, accY = 0, accZ = 0;
                static int    nAcc = 0, bad = 0;
                static ULONGLONG tAcc = 0;
                accX += fabs(gx); accY += fabs(gy); accZ += fabs(gz); nAcc++;
                ULONGLONG tn = GetTickCount64();
                if (nAcc && tn - tAcc > 1000) {
                    double mx = accX / nAcc, my = accY / nAcc, mz = accZ / nAcc;
                    if (mx > 5.0 || my > 5.0 || mz > 5.0) bad++; else bad = 0;
                    if (bad >= 3) self->state = 3;
                    accX = accY = accZ = 0; nAcc = 0; tAcc = tn;
                }
            }

            double an = sqrt(ax * ax + ay * ay + az * az);
            if (an > 0.2) {
                ax /= an; ay /= an; az /= an;
                // 估计重力方向（设备系 → 世界 Y 向上）：把测到的重力旋到 +Y
                double vx = 2 * (qx * qz - qw * qy);
                double vy = 2 * (qw * qx + qy * qz);
                double vz = qw * qw - qx * qx - qy * qy + qz * qz;
                // 目标：加速度计测到的方向应与世界 +Y 对齐（静止时 ≈ (0,1,0)）
                double ex = ay * vz - az * vy;
                double ey = az * vx - ax * vz;
                double ez = ax * vy - ay * vx;
                ix += ex * dt; iy += ey * dt; iz += ez * dt;
                double rgx = gx * 0.01745329252 + Kp * ex + Ki * ix;
                double rgy = gy * 0.01745329252 + Kp * ey + Ki * iy;
                double rgz = gz * 0.01745329252 + Kp * ez + Ki * iz;
                double dqw = 0.5 * (-qx * rgx - qy * rgy - qz * rgz);
                double dqx = 0.5 * ( qw * rgx + qy * rgz - qz * rgy);
                double dqy = 0.5 * ( qw * rgy + qz * rgx - qx * rgz);
                double dqz = 0.5 * ( qw * rgz + qx * rgy - qy * rgx);
                qw += dqw * dt; qx += dqx * dt; qy += dqy * dt; qz += dqz * dt;
                double n = sqrt(qw * qw + qx * qx + qy * qy + qz * qz);
                if (n > 1e-6) { qw /= n; qx /= n; qy /= n; qz /= n; }
            }
            self->q[0] = (float)qw; self->q[1] = (float)qx;
            self->q[2] = (float)qy; self->q[3] = (float)qz;
            self->accel[0] = (float)ax; self->accel[1] = (float)ay; self->accel[2] = (float)az;
            self->gyro[0] = (float)gx; self->gyro[1] = (float)gy; self->gyro[2] = (float)gz;
            InterlockedIncrement(&self->samples);
        }
        CloseHandle(h);
        self->connected = 0;
        DmLog("IMU: 连接断开，准备重连");
    }
    return 0;
}

bool DmImu::Start() {
    run = 1;
    thread = CreateThread(NULL, 0, ImuThread, this, 0, NULL);
    return thread != NULL;
}

void DmImu::Stop() {
    run = 0;
    if (thread) {
        WaitForSingleObject(thread, 3000);
        CloseHandle(thread);
        thread = NULL;
    }
}

float DmImu::SampleRate() const { return 0; }

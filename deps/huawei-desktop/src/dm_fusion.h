// dm_fusion.h —— 9 轴姿态融合（Mahony MARG，显式向量版）
//
// 输入（全部来自官方 Hmd.dll 的 getRawPose，语义已由逆向确认）：
//   acc  m/s²   静止时指向【重力方向】（即 -Y 世界向上），实测 (0.2, -9.75, -0.3)
//   gyro rad/s  角速度
//   mag  µT     磁力计，实测模长 ≈ 36 µT（地磁）
//
// 约定：世界系 +X 右、+Y 上、+Z 后（-Z 前），与 dm_math.h 的 Mat/M4FromQuat 一致。
// 设备系与世界系轴向一致（静止时设备 +Y 向上），不需要额外轴向置换。
#pragma once
#include <math.h>

struct DmFusion {
    float qw, qx, qy, qz;   // 姿态四元数（体 → 世界）
    float bx, by, bz;       // 陀螺零偏（rad/s）
    float Kp, Ki;
    float gyroGate;      // 角速度噪声门（rad/s）：低于它视为静止抖动，角速度归零
    bool  inited;
    int   frames;

    void Init() {
        qw = 1; qx = qy = qz = 0;
        bx = by = bz = 0;
        gyroGate = Rad(1.5f);
        Kp = 2.0f;          // 重力/地磁校正强度（约 0.5s 收敛）
        Ki = 0.05f;         // 零偏积分
        inited = true;
        frames = 0;
    }

    // 用四元数旋转向量（q: 体 → 世界）
    static void RotQ(float w, float x, float y, float z, const float v[3], float o[3]) {
        float tx = 2.0f * (y * v[2] - z * v[1]);
        float ty = 2.0f * (z * v[0] - x * v[2]);
        float tz = 2.0f * (x * v[1] - y * v[0]);
        o[0] = v[0] + w * tx + (y * tz - z * ty);
        o[1] = v[1] + w * ty + (z * tx - x * tz);
        o[2] = v[2] + w * tz + (x * ty - y * tx);
    }

    static float Rad(float d) { return d * 3.14159265f / 180.0f; }

    void Update(const float acc[3], const float g[3], const float mag[3], float dt) {
        if (!inited) Init();
        if (dt <= 0.0f || dt > 0.5f) return;

        // ★ 角速度噪声门：静止时的轻微抖动让角速度直接归零（不积分、不漂移）；
        //   只在真正转头（角速度超过门限）时才把真实角速度送进融合。
        float gx = g[0], gy = g[1], gz = g[2];
        if (gyroGate > 1e-5f) {
            float rate = sqrtf(gx * gx + gy * gy + gz * gz);
            if (rate < gyroGate) { gx = gy = gz = 0.0f; }
        }

        // 世界向上在体坐标里的实测方向 = -acc 归一化
        float aUp[3] = { -acc[0], -acc[1], -acc[2] };
        float na = sqrtf(aUp[0] * aUp[0] + aUp[1] * aUp[1] + aUp[2] * aUp[2]);
        if (na < 1e-3f) return;
        aUp[0] /= na; aUp[1] /= na; aUp[2] /= na;

        // 当前估计：世界 +Y 在体坐标里的方向 = q* 旋转 (0,1,0)
        float upB[3];
        float one[3] = { 0, 1, 0 };
        RotQ(qw, -qx, -qy, -qz, one, upB);

        // 误差 = 实测上 × 估计上
        float ex = aUp[1] * upB[2] - aUp[2] * upB[1];
        float ey = aUp[2] * upB[0] - aUp[0] * upB[2];
        float ez = aUp[0] * upB[1] - aUp[1] * upB[0];

        // 磁力计：把实测磁矢量转到世界系，取其水平方向作为“北”的参考
        float nm = sqrtf(mag[0] * mag[0] + mag[1] * mag[1] + mag[2] * mag[2]);
        if (nm > 8.0f) {
            float mB[3] = { mag[0] / nm, mag[1] / nm, mag[2] / nm };
            float mW[3];
            RotQ(qw, qx, qy, qz, mB, mW);           // 世界系里的磁矢量
            float horiz = sqrtf(mW[0] * mW[0] + mW[2] * mW[2]);
            if (horiz > 1e-3f) {
                // 期望：北 = 世界 -Z，垂直分量保留实测值
                float expW[3] = { 0.0f, mW[1], -horiz };
                float n2 = sqrtf(expW[1] * expW[1] + expW[2] * expW[2]);
                if (n2 > 1e-3f) {
                    expW[1] /= n2; expW[2] /= n2;
                    float expB[3];
                    RotQ(qw, -qx, -qy, -qz, expW, expB);   // 期望磁矢量在体坐标
                    ex += mB[1] * expB[2] - mB[2] * expB[1];
                    ey += mB[2] * expB[0] - mB[0] * expB[2];
                    ez += mB[0] * expB[1] - mB[1] * expB[0];
                }
            }
        }

        // 零偏积分
        bx -= Ki * ex * dt;
        by -= Ki * ey * dt;
        bz -= Ki * ez * dt;

        gx = gx - bx + Kp * ex;
        gy = gy - by + Kp * ey;
        gz = gz - bz + Kp * ez;

        // q' = 0.5 * q ⊗ (0, g)
        float hd = 0.5f * dt;
        float q0 = qw, q1 = qx, q2 = qy, q3 = qz;
        float n0 = q0 + hd * (-q1 * gx - q2 * gy - q3 * gz);
        float n1 = q1 + hd * ( q0 * gx + q2 * gz - q3 * gy);
        float n2 = q2 + hd * ( q0 * gy - q1 * gz + q3 * gx);
        float n3 = q3 + hd * ( q0 * gz + q1 * gy - q2 * gx);
        float nn = sqrtf(n0 * n0 + n1 * n1 + n2 * n2 + n3 * n3);
        if (nn < 1e-6f) return;
        qw = n0 / nn; qx = n1 / nn; qy = n2 / nn; qz = n3 / nn;
        frames++;
    }

    void EulerDeg(float* yaw, float* pitch, float* roll) const {
        float sinp = 2.0f * (qw * qx - qy * qz);
        if (sinp > 1.0f) sinp = 1.0f;
        if (sinp < -1.0f) sinp = -1.0f;
        *pitch = asinf(sinp) * 57.29578f;
        *roll  = atan2f(2.0f * (qw * qz + qx * qy), 1.0f - 2.0f * (qx * qx + qz * qz)) * 57.29578f;
        *yaw   = atan2f(2.0f * (qw * qy + qx * qz), 1.0f - 2.0f * (qy * qy + qz * qz)) * 57.29578f;
    }
};

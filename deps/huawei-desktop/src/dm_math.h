// dm_math.h —— 桌面模式用的最小数学库（不引 DirectXMath，MinGW 下零依赖）
// 约定：矩阵按【行主序存储 + 行向量约定】，即 clip = v_row * M
//      OpenVR 的 HmdMatrix34_t / HmdMatrix44_t 是列向量约定，转换函数里统一转置。
#pragma once
#include <math.h>
#include <string.h>
#include "openvr.h"

struct Vec3 {
    float x, y, z;
    Vec3() : x(0), y(0), z(0) {}
    Vec3(float X, float Y, float Z) : x(X), y(Y), z(Z) {}
    Vec3 operator+(const Vec3& o) const { return Vec3(x + o.x, y + o.y, z + o.z); }
    Vec3 operator-(const Vec3& o) const { return Vec3(x - o.x, y - o.y, z - o.z); }
    Vec3 operator*(float s) const { return Vec3(x * s, y * s, z * s); }
    float Len() const { return sqrtf(x * x + y * y + z * z); }
    Vec3 Norm() const { float l = Len(); return l > 1e-6f ? Vec3(x / l, y / l, z / l) : Vec3(0, 0, -1); }
};

// 行主序 4x4，行向量约定
struct M4 {
    float m[4][4];
    M4() { memset(m, 0, sizeof(m)); }
    static M4 Identity() { M4 r; for (int i = 0; i < 4; i++) r.m[i][i] = 1.0f; return r; }
};

static inline M4 Mul(const M4& a, const M4& b) {          // v*(a*b) = (v*a)*b
    M4 r;
    for (int i = 0; i < 4; i++)
        for (int j = 0; j < 4; j++) {
            float s = 0;
            for (int k = 0; k < 4; k++) s += a.m[i][k] * b.m[k][j];
            r.m[i][j] = s;
        }
    return r;
}
static inline Vec3 XformPoint(const M4& a, const Vec3& p) { // 行向量：p * M（含平移）
    Vec3 r;
    r.x = p.x * a.m[0][0] + p.y * a.m[1][0] + p.z * a.m[2][0] + a.m[3][0];
    r.y = p.x * a.m[0][1] + p.y * a.m[1][1] + p.z * a.m[2][1] + a.m[3][1];
    r.z = p.x * a.m[0][2] + p.y * a.m[1][2] + p.z * a.m[2][2] + a.m[3][2];
    return r;
}
static inline Vec3 XformDir(const M4& a, const Vec3& p) {
    Vec3 r;
    r.x = p.x * a.m[0][0] + p.y * a.m[1][0] + p.z * a.m[2][0];
    r.y = p.x * a.m[0][1] + p.y * a.m[1][1] + p.z * a.m[2][1];
    r.z = p.x * a.m[0][2] + p.y * a.m[1][2] + p.z * a.m[2][2];
    return r;
}
static inline M4 Transpose(const M4& a) {
    M4 r;
    for (int i = 0; i < 4; i++) for (int j = 0; j < 4; j++) r.m[i][j] = a.m[j][i];
    return r;
}
// OpenVR 3x4（列向量）→ 行向量 4x4
static inline M4 FromOVR(const vr::HmdMatrix34_t& t) {
    M4 r;
    for (int i = 0; i < 3; i++) {
        for (int j = 0; j < 3; j++) r.m[j][i] = t.m[i][j];   // 转置旋转
        r.m[3][i] = t.m[i][3];                               // 平移放到最后一行
    }
    r.m[3][3] = 1.0f;
    return r;
}
static inline M4 FromOVR44(const vr::HmdMatrix44_t& t) {
    M4 r;
    for (int i = 0; i < 4; i++) for (int j = 0; j < 4; j++) r.m[j][i] = t.m[i][j];  // 转置
    return r;
}
// 刚体逆（旋转正交）
static inline M4 InverseRigid(const M4& a) {
    M4 r = M4::Identity();
    for (int i = 0; i < 3; i++) for (int j = 0; j < 3; j++) r.m[i][j] = a.m[j][i];
    Vec3 t(a.m[3][0], a.m[3][1], a.m[3][2]);
    r.m[3][0] = -(t.x * r.m[0][0] + t.y * r.m[1][0] + t.z * r.m[2][0]);
    r.m[3][1] = -(t.x * r.m[0][1] + t.y * r.m[1][1] + t.z * r.m[2][1]);
    r.m[3][2] = -(t.x * r.m[0][2] + t.y * r.m[1][2] + t.z * r.m[2][2]);
    return r;
}

// 四元数（w,x,y,z）
struct Quat {
    float w, x, y, z;
    Quat() : w(1), x(0), y(0), z(0) {}
    Quat(float W, float X, float Y, float Z) : w(W), x(X), y(Y), z(Z) {}
};
static inline Quat QNorm(Quat q) {
    float n = sqrtf(q.w * q.w + q.x * q.x + q.y * q.y + q.z * q.z);
    if (n < 1e-8f) return Quat();
    return Quat(q.w / n, q.x / n, q.y / n, q.z / n);
}
static inline Quat QMul(const Quat& a, const Quat& b) {
    return Quat(a.w * b.w - a.x * b.x - a.y * b.y - a.z * b.z,
                a.w * b.x + a.x * b.w + a.y * b.z - a.z * b.y,
                a.w * b.y - a.x * b.z + a.y * b.w + a.z * b.x,
                a.w * b.z + a.x * b.y - a.y * b.x + a.z * b.w);
}
static inline Quat QSlerp(Quat a, Quat b, float t) {
    float d = a.w * b.w + a.x * b.x + a.y * b.y + a.z * b.z;
    if (d < 0) { b = Quat(-b.w, -b.x, -b.y, -b.z); d = -d; }
    if (d > 0.9995f) {
        Quat r(a.w + (b.w - a.w) * t, a.x + (b.x - a.x) * t,
               a.y + (b.y - a.y) * t, a.z + (b.z - a.z) * t);
        return QNorm(r);
    }
    float th = acosf(d), s = sinf(th);
    float wa = sinf((1 - t) * th) / s, wb = sinf(t * th) / s;
    return QNorm(Quat(a.w * wa + b.w * wb, a.x * wa + b.x * wb, a.y * wa + b.y * wb, a.z * wa + b.z * wb));
}
// 由 OpenVR 位姿矩阵取四元数
static inline Quat QuatFromM4(const M4& a) {
    // ★ 注意：我们的 M4 是【行向量约定】= 标准旋转矩阵的转置，
    //   所以所有“差”项要反号（对称的和不变），否则会得到共轭四元数。
    float m00 = a.m[0][0], m01 = a.m[0][1], m02 = a.m[0][2];
    float m10 = a.m[1][0], m11 = a.m[1][1], m12 = a.m[1][2];
    float m20 = a.m[2][0], m21 = a.m[2][1], m22 = a.m[2][2];
    float tr = m00 + m11 + m22;
    Quat q;
    if (tr > 0) {
        float s = sqrtf(tr + 1.0f) * 2;
        q.w = 0.25f * s; q.x = (m12 - m21) / s; q.y = (m20 - m02) / s; q.z = (m01 - m10) / s;
    } else if (m00 > m11 && m00 > m22) {
        float s = sqrtf(1.0f + m00 - m11 - m22) * 2;
        q.w = (m12 - m21) / s; q.x = 0.25f * s; q.y = (m01 + m10) / s; q.z = (m02 + m20) / s;
    } else if (m11 > m22) {
        float s = sqrtf(1.0f + m11 - m00 - m22) * 2;
        q.w = (m20 - m02) / s; q.x = (m01 + m10) / s; q.y = 0.25f * s; q.z = (m12 + m21) / s;
    } else {
        float s = sqrtf(1.0f + m22 - m00 - m11) * 2;
        q.w = (m01 - m10) / s; q.x = (m02 + m20) / s; q.y = (m12 + m21) / s; q.z = 0.25f * s;
    }
    return QNorm(q);
}
static inline Quat QuatFromYawPitchRoll(float yaw, float pitch, float roll) {
    // OpenVR：+X 右，+Y 上，−Z 前。yaw 绕 Y（右转为正），pitch 绕 X（抬头为正），roll 绕 Z
    // ★ 用四元数乘法拼，别再手写展开（之前 yaw/pitch 轴写反过）
    Quat qy(cosf(yaw * 0.5f), 0, -sinf(yaw * 0.5f), 0);   // 右转为正（绕 +Y 右手旋转取负号）
    Quat qx(cosf(pitch * 0.5f), sinf(pitch * 0.5f), 0, 0);
    Quat qz(cosf(roll * 0.5f), 0, 0, sinf(roll * 0.5f));
    return QNorm(QMul(QMul(qy, qx), qz));
}
// 四元数 → 行向量 4x4（旋转部分）
static inline M4 M4FromQuat(const Quat& q) {
    M4 r = M4::Identity();
    float xx = q.x * q.x, yy = q.y * q.y, zz = q.z * q.z;
    float xy = q.x * q.y, xz = q.x * q.z, yz = q.y * q.z;
    float wx = q.w * q.x, wy = q.w * q.y, wz = q.w * q.z;
    // 行向量约定 = 列向量旋转矩阵的转置
    r.m[0][0] = 1 - 2 * (yy + zz); r.m[0][1] = 2 * (xy + wz);     r.m[0][2] = 2 * (xz - wy);
    r.m[1][0] = 2 * (xy - wz);     r.m[1][1] = 1 - 2 * (xx + zz); r.m[1][2] = 2 * (yz + wx);
    r.m[2][0] = 2 * (xz + wy);     r.m[2][1] = 2 * (yz - wx);     r.m[2][2] = 1 - 2 * (xx + yy);
    return r;
}
// 只取朝向里的 yaw/pitch（用于跟随控制），忽略 roll（保证桌面水平）
static inline void YawPitchFromQuat(const Quat& q, float* yaw, float* pitch) {
    // 前向量（-Z 在设备坐标里）= 旋转矩阵第三列取负
    Vec3 fwd(-2 * (q.x * q.z + q.w * q.y),
             -2 * (q.y * q.z - q.w * q.x),
             -(1 - 2 * (q.x * q.x + q.y * q.y)));
    Vec3 f = fwd.Norm();
    if (yaw)   *yaw = atan2f(f.x, -f.z);
    if (pitch) *pitch = asinf(f.y > 1 ? 1.0f : (f.y < -1 ? -1.0f : f.y));
}
static inline float Clampf(float v, float lo, float hi) { return v < lo ? lo : (v > hi ? hi : v); }
static inline float WrapPi(float a) {
    while (a > 3.14159265f) a -= 6.2831853f;
    while (a < -3.14159265f) a += 6.2831853f;
    return a;
}

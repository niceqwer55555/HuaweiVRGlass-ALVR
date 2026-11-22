// dm_scene.cpp
#include "dm_scene.h"
#include "dm_log.h"
#include <math.h>

static const float kPi = 3.14159265358979f;
static inline float Rad(float d) { return d * kPi / 180.0f; }

void DmHeadYawPitchRoll(const Quat& q, float* yaw, float* pitch, float* roll) {
    M4 R = M4FromQuat(q);
    Vec3 f = XformDir(R, Vec3(0, 0, -1));
    Vec3 u = XformDir(R, Vec3(0, 1, 0));
    Vec3 r = XformDir(R, Vec3(1, 0, 0));
    if (yaw) *yaw = atan2f(f.x, -f.z);
    if (pitch) *pitch = asinf(f.y > 1 ? 1.0f : (f.y < -1 ? -1.0f : f.y));
    if (roll) {
        // 把 yaw/pitch 去掉后剩下的滚转
        Quat qYP = QuatFromYawPitchRoll(atan2f(f.x, -f.z), asinf(f.y > 1 ? 1.0f : (f.y < -1 ? -1.0f : f.y)), 0);
        Quat qInv(qYP.w, -qYP.x, -qYP.y, -qYP.z);
        Quat qRoll = QMul(qInv, q);
        *roll = 2.0f * atan2f(qRoll.z, qRoll.w);
    }
    (void)u; (void)r;
}

static DmPose PoseInFront(const Vec3& headPos, const Quat& headRot, const DmSettings& st) {
    float hy, hp, hr;
    DmHeadYawPitchRoll(headRot, &hy, &hp, &hr);
    float pitch = hp + Rad(st.tiltDeg);
    if (hp > Rad(60)) pitch = Rad(60);
    if (hp < -Rad(60)) pitch = -Rad(60);
    Quat q = QuatFromYawPitchRoll(hy, pitch, st.keepLevel ? 0.0f : hr);
    M4 R = M4FromQuat(q);
    DmPose p;
    p.rot = q;
    p.pos = headPos + XformDir(R, Vec3(0, 0, -st.distance))
                     + XformDir(R, Vec3(st.panX, st.panY, 0));
    return p;
}

void DmScene::ResetToHead(const Vec3& headPos, const Quat& headRot) {
    float hy, hp, hr;
    DmHeadYawPitchRoll(headRot, &hy, &hp, &hr);
    yaw = hy; pitch = hp;
    followingMotion = false;
    // 不清平移量：平移由面板/配置控制，归零只负责把窗口摆正
    hoverValid = false;
    shownValid = false;
    transT = 1.0f;
}

void DmScene::Recenter(const Vec3& headPos, const Quat& headRot) {
    float hy, hp, hr;
    DmHeadYawPitchRoll(headRot, &hy, &hp, &hr);
    yaw = hy; pitch = hp;
    followingMotion = false;
    panX = panY = 0;
    hoverValid = false;                       // 下一帧按当前视线重建
    transT = 1.0f;
}

void DmScene::ToggleMode(const Vec3& headPos, const Quat& headRot, const DmSettings& st) {
    fromPose = shown;
    transT = 0.0f;
    if (st.followMode) {
        // 悬停 → 跟随：把屏体朝向对齐到「当前屏幕相对头部」的方向，避免跳变
        Vec3 rel = shown.pos - headPos;
        if (rel.Len() > 0.01f) {
            yaw = atan2f(rel.x, -rel.z);
            pitch = asinf(Clampf(rel.y / rel.Len(), -1, 1));
        }
        hoverValid = false;
        followingMotion = false;
        panX = panY = 0;
    } else {
        // 跟随 → 悬停：以当前显示位姿为锁定基准
        hover = shown;
        hoverValid = true;
    }
}

void DmScene::Pan(float dx, float dy, const DmSettings& st) {
    if (!st.followMode && hoverValid) {
        M4 R = M4FromQuat(hover.rot);
        hover.pos = hover.pos + XformDir(R, Vec3(dx, dy, 0));
    } else {
        panX += dx; panY += dy;
        panX = Clampf(panX, -3.0f, 3.0f);
        panY = Clampf(panY, -3.0f, 3.0f);
        (void)0;
    }
}

void DmScene::ChangeDistance(float delta, const DmSettings& st) {
    if (!st.followMode && hoverValid) {
        M4 R = M4FromQuat(hover.rot);
        hover.pos = hover.pos + XformDir(R, Vec3(0, 0, -delta));   // 沿屏体法线
    }
    // 跟随模式下由 main 改 st.distance（配置项）
}

DmPose DmScene::Update(float dt, const Vec3& headPos, const Quat& headRot, const DmSettings& st, bool modeChanged) {
    if (dt < 0.0001f) dt = 0.0001f;
    if (dt > 0.1f) dt = 0.1f;                     // 卡顿钳制

    DmPose target;
    // 配置里的平移量变了（面板按钮）→ 立即采纳；热键平移只改本类的 panX，不受影响
    if (st.distance != lastDist) { lastDist = st.distance; hoverValid = false; }
    if (st.panX != lastPanX || st.panY != lastPanY) {
        panX = st.panX; panY = st.panY;
        lastPanX = st.panX; lastPanY = st.panY;
        hoverValid = false;                 // 悬停模式需要重新摆位
    }

    if (!st.followMode) {
        // 悬停：位置和朝向都世界锁定
        if (!hoverValid) { hover = PoseInFront(headPos, headRot, st); hoverValid = true; }
        target = hover;
    } else {
        // 跟随（按你的要求）：跟着 IMU 走，但是**延迟/平滑**地跟（防眩晕）。
        //   死区（deadzone）内不动 → 超出后按指数平滑追，接近时用更小的滞回阈值停住；
        //   追的速度还受 max_speed 限制，快转头时桌面不会甩。
        float hy, hp, hr;
        DmHeadYawPitchRoll(headRot, &hy, &hp, &hr);
        if (!shownValid) { yaw = hy; pitch = hp; panX = st.panX; panY = st.panY; }

        // 偏移 = 视线中心 - 窗口中心（跟随时把这个偏移驱到 0，窗口中心就压在视线中心上，
        //    不会像"每次只补一部分"那样越跟越偏）
        float eYaw = WrapPi(hy - yaw);
        float ePitch = hp - pitch;
        float mag = sqrtf(eYaw * eYaw + ePitch * ePitch);

        float trigDeg = st.followRangeDeg > 0.5f ? st.followRangeDeg : 0.5f;
        float trig = Rad(trigDeg);          // 超过这个范围才跟随
        float hd   = Rad(st.holdDeg);       // 跟到这么近就停

        // 头部角速度（settle>0 时用来判断"停稳了没有"）
        float headRate = 0.0f;
        if (lastHeadValid && dt > 1e-4f) {
            float dY = WrapPi(hy - lastHeadYaw);
            float dP = hp - lastHeadPitch;
            headRate = sqrtf(dY * dY + dP * dP) / dt;
        }
        lastHeadYaw = hy; lastHeadPitch = hp; lastHeadValid = true;

        bool canStart = true;
        if (st.settleSec > 0.001f) {
            float moveRate = Rad(st.moveRateDeg > 1.0f ? st.moveRateDeg : 1.0f);
            if (headRate > moveRate) stillTime = 0.0f;
            else stillTime += dt;
            canStart = (headRate <= moveRate && stillTime >= st.settleSec);
        }

        if (mag > trig && canStart) followingMotion = true;
        if (followingMotion && mag <= hd) followingMotion = false;

        if (followingMotion && mag > 1e-5f) {
            float alpha = 1.0f - expf(-dt / (st.tau > 0.01f ? st.tau : 0.01f));
            float dYaw = eYaw * alpha;
            float dPitch = ePitch * alpha;
            float maxStep = Rad(st.maxFollowDegPerSec) * dt;
            float m2 = sqrtf(dYaw * dYaw + dPitch * dPitch);
            if (m2 > maxStep && m2 > 1e-6f) { float s = maxStep / m2; dYaw *= s; dPitch *= s; }
            yaw += dYaw;
            pitch += dPitch;
        }
        pitch = Clampf(pitch, -Rad(75), Rad(75));

        float pitchWithTilt = pitch + Rad(st.tiltDeg);
        Quat q = QuatFromYawPitchRoll(yaw, pitchWithTilt, st.keepLevel ? 0.0f : hr);
        M4 R = M4FromQuat(q);
        target.rot = q;
        target.pos = headPos
                   + XformDir(R, Vec3(0, 0, -st.distance))
                   + XformDir(R, Vec3(panX, panY, 0));
    }

    // 过渡
    DmPose out = target;
    if (transT < 1.0f && st.transitionSec > 0.01f) {
        transT += dt / st.transitionSec;
        if (transT > 1.0f) transT = 1.0f;
        float s = transT * transT * (3 - 2 * transT);     // smoothstep
        out.pos = fromPose.pos + (target.pos - fromPose.pos) * s;
        out.rot = QSlerp(fromPose.rot, target.rot, s);
    } else {
        transT = 1.0f;
    }
    shown = out;
    shownValid = true;
    (void)modeChanged;
    return out;
}

// ---------------------------------------------------------------- 网格
// 局部坐标：x 右，y 上，z 朝观察者
static void ScreenPointLocal(float tX, float tY, const DmSettings& st, float aspect, Vec3* out) {
    double d = st.distance > 0.05 ? st.distance : 0.05;
    // 屏的"世界尺寸"固定（不随距离变化）：越远看起来越小，符合正常透视
    const double kRefDist = 2.0;
    double halfW = kRefDist * tan((double)st.fovBaseDeg * 3.14159265358979 / 360.0) * st.scale;
    double halfH = halfW / (aspect > 0.01 ? aspect : 1.0);
    // 平面屏：x/y 线性分布
    double x = halfW * tX;
    double y = halfH * tY;
    double z = -d;
    double c = st.curvature;
    if (c > 0.002) {
        // 曲面：双轴圆弧弯折（弧长守恒）——把平面按等弧长卷到半径 Rc 的圆柱/球面上。
        //   入参 x 是"弧长"，出参 (x', z') 是弧上的点 → 画面不会被拉伸，只有弯曲。
        double halfDiag = sqrt(halfW * halfW + halfH * halfH);
        double Rc = halfDiag * 30.0 / c;         // 曲率 20 → 1.5×半对角；曲率 1 → 30×（不卷折）
        double ax = x / Rc, ay = y / Rc;
        x = Rc * sin(ax);
        y = Rc * sin(ay);
        z = -d + 0.5 * Rc * ((1.0 - cos(ax)) + (1.0 - cos(ay)));
    }
    out->x = (float)x;
    out->y = (float)y;
    out->z = (float)z;
}

void DmBuildScreenMesh(const DmSettings& st, const DmPose& pose, float aspect,
                       std::vector<DmVert>* out) {
    out->clear();
    const int N = 64;
    M4 R = M4FromQuat(pose.rot);
    Vec3 grid[2][N + 1];
    for (int i = 0; i <= N; i++) {
        float tX = -1.0f + 2.0f * (float)i / (float)N;
        for (int j = 0; j < 2; j++) {
            float tY = (j == 0) ? 1.0f : -1.0f;
            Vec3 local;
            ScreenPointLocal(tX, tY, st, aspect, &local);
            grid[j][i] = pose.pos + XformDir(R, local);
        }
    }
    for (int i = 0; i < N; i++) {
        float u0 = (float)i / (float)N, u1 = (float)(i + 1) / (float)N;
        DmVert a = { grid[0][i].x, grid[0][i].y, grid[0][i].z,       u0, 0.0f };
        DmVert b = { grid[0][i + 1].x, grid[0][i + 1].y, grid[0][i + 1].z, u1, 0.0f };
        DmVert c = { grid[1][i].x, grid[1][i].y, grid[1][i].z,       u0, 1.0f };
        DmVert dd= { grid[1][i + 1].x, grid[1][i + 1].y, grid[1][i + 1].z, u1, 1.0f };
        out->push_back(a); out->push_back(c); out->push_back(b);
        out->push_back(b); out->push_back(c); out->push_back(dd);
    }
}

void DmBuildFlatQuad(const DmPose& pose, float w, float h, std::vector<DmVert>* out) {
    out->clear();
    M4 R = M4FromQuat(pose.rot);
    Vec3 hw = XformDir(R, Vec3(w * 0.5f, 0, 0));
    Vec3 hh = XformDir(R, Vec3(0, h * 0.5f, 0));
    Vec3 p00 = pose.pos - hw + hh;   // 左上
    Vec3 p10 = pose.pos + hw + hh;
    Vec3 p01 = pose.pos - hw - hh;
    Vec3 p11 = pose.pos + hw - hh;
    DmVert v00 = { p00.x, p00.y, p00.z, 0, 0 };
    DmVert v10 = { p10.x, p10.y, p10.z, 1, 0 };
    DmVert v01 = { p01.x, p01.y, p01.z, 0, 1 };
    DmVert v11 = { p11.x, p11.y, p11.z, 1, 1 };
    out->push_back(v00); out->push_back(v01); out->push_back(v10);
    out->push_back(v10); out->push_back(v01); out->push_back(v11);
}

void DmBuildCursorQuad(const DmSettings& st, const DmPose& pose, float aspect,
                       float u, float v, float sizeX, float sizeY, std::vector<DmVert>* out) {
    out->clear();
    M4 R = M4FromQuat(pose.rot);
    float tX = u * 2.0f - 1.0f;
    float tY = 1.0f - v * 2.0f;
    Vec3 local;
    ScreenPointLocal(tX, tY, st, aspect, &local);
    Vec3 center = pose.pos + XformDir(R, local);
    // 切线（沿曲面）
    Vec3 l2;
    ScreenPointLocal(tX + 0.002f, tY, st, aspect, &l2);
    Vec3 tangent = (pose.pos + XformDir(R, l2)) - center;
    if (tangent.Len() < 1e-6f) tangent = Vec3(1, 0, 0);
    tangent = tangent.Norm();
    Vec3 upLocal = XformDir(R, Vec3(0, 1, 0));
    Vec3 nrm = Vec3(tangent.y * upLocal.z - tangent.z * upLocal.y,
                    tangent.z * upLocal.x - tangent.x * upLocal.z,
                    tangent.x * upLocal.y - tangent.y * upLocal.x).Norm();
    if (nrm.Len() < 0.01f) nrm = Vec3(0, 0, -1);
    Vec3 up = Vec3(nrm.y * tangent.z - nrm.z * tangent.y,
                   nrm.z * tangent.x - nrm.x * tangent.z,
                   nrm.x * tangent.y - nrm.y * tangent.x).Norm();
    Vec3 hx = tangent * (sizeX * 0.5f);
    Vec3 hy = up * (sizeY * 0.5f);
    // Windows 约定：热点就是图标左上角，图标从热点【向右下】展开。
    // （之前把热点当成左边中点，指针整体往下偏了半个图标高）
    Vec3 p00 = center;                       // 左上 = 热点
    Vec3 p10 = center + hx * 2;              // 右上
    Vec3 p01 = center - hy * 2;              // 左下
    Vec3 p11 = center + hx * 2 - hy * 2;     // 右下
    DmVert v00 = { p00.x, p00.y, p00.z, 0, 0 };
    DmVert v10 = { p10.x, p10.y, p10.z, 1, 0 };
    DmVert v01 = { p01.x, p01.y, p01.z, 0, 1 };
    DmVert v11 = { p11.x, p11.y, p11.z, 1, 1 };
    out->push_back(v00); out->push_back(v01); out->push_back(v10);
    out->push_back(v10); out->push_back(v01); out->push_back(v11);
}

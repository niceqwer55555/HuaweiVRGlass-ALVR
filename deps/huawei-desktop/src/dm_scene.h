// dm_scene.h —— 桌面位姿（跟随/悬停）、曲面网格、HUD/光标四边形
#pragma once
#include <vector>
#include "dm_math.h"
#include "dm_settings.h"
#include "dm_gfx.h"

struct DmPose {
    Vec3 pos;
    Quat rot;
};

struct DmScene {
    // 跟随模式的状态量（屏体朝向，单位弧度）
    float yaw = 0, pitch = 0;
    bool  followingMotion = false;   // 滞回：当前是否在追
    // 「稳定后才追」状态
    float lastHeadYaw = 0, lastHeadPitch = 0;
    bool  lastHeadValid = false;
    float stillTime = 0;             // 头部静止累计时间（秒）
    // 悬停模式
    DmPose hover;
    bool hoverValid = false;
    // 显示用的当前位姿（过渡用）
    DmPose shown;
    bool shownValid = false;
    // 过渡
    float transT = 1.0f;
    DmPose fromPose;
    // 平移偏移（跟随模式下相对视线；悬停模式下直接改 hover.pos）
    float panX = 0, panY = 0;
    float lastPanX = 1e9f, lastPanY = 1e9f;   // 配置里的平移量，变化时采纳
    float lastDist = -1.0f;

    void ResetToHead(const Vec3& headPos, const Quat& headRot);
    void Recenter(const Vec3& headPos, const Quat& headRot);
    void ToggleMode(const Vec3& headPos, const Quat& headRot, const DmSettings& st);
    void Pan(float dx, float dy, const DmSettings& st);          // 米
    void ChangeDistance(float delta, const DmSettings& st);      // 米（正=远）

    // 每帧计算目标位姿并返回（含过渡）
    DmPose Update(float dt, const Vec3& headPos, const Quat& headRot, const DmSettings& st, bool modeChanged);
};

// 生成桌面曲面网格（世界坐标）
void DmBuildScreenMesh(const DmSettings& st, const DmPose& pose, float aspect,
                       std::vector<DmVert>* out);
// 生成一个贴在某位姿上的平面四边形（HUD 用）
void DmBuildFlatQuad(const DmPose& pose, float w, float h, std::vector<DmVert>* out);
// 光标：贴在桌面曲面上的小方块
void DmBuildCursorQuad(const DmSettings& st, const DmPose& pose, float aspect,
                       float u, float v, float sizeX, float sizeY, std::vector<DmVert>* out);

void DmHeadYawPitchRoll(const Quat& q, float* yaw, float* pitch, float* roll);

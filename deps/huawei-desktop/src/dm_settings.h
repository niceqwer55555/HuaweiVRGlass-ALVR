// dm_settings.h —— 桌面模式配置（desktop_mode.ini）与动作/热键表
#pragma once
#include <windows.h>
#include <stdio.h>
#include <wchar.h>
#include <string>
#include <vector>
#include "dm_log.h"

// ---------------- 动作 ----------------
enum DmAction {
    DM_A_RECENTER = 0,
    DM_A_PAN_UP, DM_A_PAN_DOWN, DM_A_PAN_LEFT, DM_A_PAN_RIGHT,
    DM_A_ZOOM_IN, DM_A_ZOOM_OUT,
    DM_A_CURV_UP, DM_A_CURV_DOWN,
    DM_A_DIST_FAR, DM_A_DIST_NEAR,
    DM_A_S3D_CYCLE,      // 3D 模式循环切换（面板里仍可单独选）
    DM_A_TOGGLE_HOVER,
    DM_A_TOGGLE_HUD,
    DM_A_OPACITY_UP, DM_A_OPACITY_DOWN,
    DM_A_TOGGLE_SCREEN,
    DM_A_BG_CYCLE,
    DM_A_REZERO,
    DM_A_IMU_SOURCE,
    DM_A_EYE_ROLL_CYCLE, // 两眼朝向组合循环（校准面板物理旋转）     // 切换姿态源：官方 getRawPose / 自研 3DoF
    DM_A_LENS_MODE,      // 切换 无畸变 / 官方多项式
    DM_A_LENS_IN,        // 畸变 k1 减小（画面往里收）
    DM_A_LENS_OUT,       // 畸变 k1 增大          // 头动重新归零（静止时按）
    DM_A_ZERODOF,        // 0DOF 模式开关（背景锁定，窗口延迟跟随）
    DM_A_AUTOCAL,        // 自动标定偏航轴（转 6 秒头，取摆动最大者）
    DM_A_EXIT,
    DM_A_COUNT
};

// 修饰键位
enum { DMK_CTRL = 1, DMK_ALT = 2, DMK_SHIFT = 4, DMK_WIN = 8 };

struct DmHotkey {
    int vk;      // 键盘虚拟键（kind=0 时有效）
    int mods;
    int kind;    // 0=键盘  1=鼠标  2=手柄
    int code;    // 鼠标 1..5 / 手柄按钮号（见 dm_hotkeys.cpp 的 kPadTable）
};

struct DmSettings {
    // [general]
    int   version;
    int   enabled;              // 桌面模式是否启用（客户端读）
    // [display]
    int   monitorIndex;         // 采集源：DM 内部枚举序号（0 = 主屏）
    float distance;             // 米
    float scale;                // 尺寸倍率
    float curvature;            // 0..1
    float opacity;              // 0.1..1
    float tiltDeg;              // 下倾角 0..15
    int   keepLevel;            // 1 = 忽略头部 roll，桌面保持水平
    float fovBaseDeg;           // scale=1 时的水平视场角
    float ipdMm;                // 瞳距（mm）
    float fovTanX, fovTanY;     // 每眼半视角（tan 值；1.0 = 90°）
    // [mode]
    float simYaw, simPitch;     // 调试：给头姿叠加的偏航/俯仰角（度，0=关闭）
    int   zeroDof;              // 1 = 0DOF：相机不跟头，背景锁定，窗口延迟跟随
    int   followMode;           // 1 = 跟随，0 = 悬停
    float deadZoneDeg;
    float holdDeg;              // 追到多近算停（滞回）
    float tau;                  // 指数平滑时间常数（秒）
    float maxFollowDegPerSec;
    float moveRateDeg;          // 头部角速度超过它就不跟随（度/秒）
    float settleSec;            // 静止多久后才开始追（秒）
    float pitchFollow;          // 0..1
    float transitionSec;        // 模式切换过渡
    float panStep;              // 平移步长（米/次）
    float panStepFast;          // 长按加速后步长
    float zoomStep;             // 缩放倍率步长（比例）
    float curvStep;
    float distStep;             // 距离步进（米）
    float opacityStep;
    // [background]
    int   bgMode;               // 0 = 虚拟环境，1 = 摄像头透视
    int   venvMode;             // 0 = 渐变，1 = 程序化天空，2 = 全景图
    wchar_t panoPath[MAX_PATH];
    float bgBrightness;         // 0.2..2
    int   passthroughMirror;    // 透视画面左右镜像
    // [3d]
    int   sbsMode;
    int   imuSource;           // 0 = HID 自研解析，1 = 官方 Hmd.dll getRawPose
    int   axisYaw, axisPitch, axisRoll;   // 官方姿态 4 个分量里哪个是 yaw/pitch/roll
    float signYaw, signPitch, signRoll;
    float lensK1;              // 旧模型 k1（lensMode=0 时用）
    float lensK2;              // 旧模型 k2
    float lensZoom;            // 额外缩放（1.0 = 官方原值）
    int   lensMode;            // 1 = 官方 r² 五次多项式（默认），0 = 旧 k1/k2
    float lensK[6];            // 官方系数 k0..k5（R/G/B 三组相同）
    float lensD;               // 官方归一化半径 d（米）
    float lensScale;           // 官方 scale（= f(1)）
    float screenAspect;        // 屏幕宽高比 0=跟随桌面(1.6)，1.0=官方正方形拉伸
    int   alignOverlay;        // 1 = 画校准图案（四角彩块+中心十字）
    int   lensSupersample;     // 1 或 2（官方 scaleRate=2.0）            // 放大倍数（补掉畸变后的黑边）
    float eyeRot[2];           // 每只眼的面板朝向（度）：官方左 0 / 右 180
    int   flip180;             // 整幅画面再翻转 180°（1=开）
    int   invertPose;          // 总开关（保留兼容，1=下面按轴生效）
    int   invertYaw;           // 偏航取反
    int   invertPitch;         // 俯仰取反
    int   invertRoll;          // 滚转取反
    int   swapPitchRoll;       // 上下/旋转轴对调（pitch↔roll）
    float poseDeadband;        // 姿态静止阈值（度，保留兼容，当前为 0）
    float viewDeadband;        // 旧参数（保留兼容，不再使用）
    float gyroGateDeg;         // 角速度噪声门（度/秒）：低于它视为静止抖动，角速度归零
    int   autoRezero;          // 拿起后静置自动姿态归零
    int   oeEnabled;           // 1 = 用 1€ 自适应低通做 3DoF 防抖
    float oeMinCutoff;         // 1€ 静止截止频率（Hz），越小静止越稳
    float oeBeta;              // 1€ 速度系数（度/秒 单位），越大转动延迟越小
    float oeStillSpeed;        // 阈值（度/秒）：低于它=静止（重滤波），高于它=在动（轻滤波）
    float oeMoveCutoff;        // 转动时的截止频率（Hz）：轻滤波，只抹掉 IMU 离散跳变
    float superres;            // 画面超分倍数（1.0 ~ 2.0）：眼睛离屏渲染分辨率倍数
    float panX, panY;          // 桌面窗口平移（米）
    int   panoKind;            // 背景图类型：0=360 2D 1=180 2D 2=360 立体 3=180 立体
    float followRangeDeg;      // 跟随触发范围（度）：窗口中心偏离视线超过它才回中
    int   eyeMirrorX[2];       // 每只眼：左右镜像（0/1）
    int   eyeMirrorY[2];       // 每只眼：上下镜像
    float eyeDx[2], eyeDy[2];  // 每只眼：像素级偏移（面板像素）
    float rollLeft;            // 左眼画面预旋转（度）：面板物理上左右眼各转了 90°
    float rollRight;           // 右眼画面预旋转（度）              // 0 off / 1 sbs_half / 2 sbs_full / 3 top_bottom
    int   swapEyes;
    float parallax;             // 像素级视差微调（-0.05..0.05 UV）
    // [imu] 头姿（6DoF 套件 IMU）
    int   imuEnabled;           // 0 = 不启用头动（桌面固定在正前方）
    int   imuMap;               // 轴映射预设
    float gyroScale;            // LSB/(°/s)
    float accelScale;           // LSB/g
    float imuYawOffset, imuPitchOffset, imuRollOffset;   // 度

    // [hud]
    int   hudPanel;
    int   hudPos;               // HUD 位置：0 下 1 右 2 左 3 上             // 监控面板
    int   hudKeys;              // 快捷键提示
    int   hudFontPx;
    float hudDistance;
    // [advanced]
    int   srgb;                 // 1 = 渲染目标用 _SRGB
    int   capFpsLimit;          // 0 = 不限制
    int   verboseLog;
    int   dumpFrame;            // >0 = 在第 N 帧把画面导出成 frame_dump.bmp
    int   outputEnabled;        // 0 = 无头模式（只渲染到内存，不出图）
    int   keepMouseOut;        // 1 = 鼠标不许进入眼镜那块屏
    int   vsync;                // 1 = 跟显示器垂直同步；0 = 不等（画面不动/黑屏时用）
    int   exclusive;            // 1 = 独占全屏直出（绕过 DWM 合成）
    // [hotkeys]
    DmHotkey hk[DM_A_COUNT];
};

// 动作元信息（名字 / 中文名 / 是否连续动作）
struct DmActionInfo { const wchar_t* key; const wchar_t* label; int repeatable; };
const DmActionInfo* DmActionTable();          // 长度 DM_A_COUNT

std::wstring DmIniPath();                     // <exe>\desktop_mode.ini

void DmSettingsDefault(DmSettings* s);
void DmSettingsClamp(DmSettings* s);
bool DmSettingsLoad(DmSettings* s);           // 文件不存在/字段缺失 → 用默认值，不失败
bool DmSettingsSave(const DmSettings* s);

// 热键文本 <-> 结构（"ctrl+alt+home"）
std::wstring DmHotkeyToString(const DmHotkey& hk);
bool DmHotkeyParse(const std::wstring& text, DmHotkey* out);
std::wstring DmHotkeyDisplay(const DmHotkey& hk);   // 给人看的："Ctrl+Alt+Home"

// 读取辅助
float DmReadFloat(const wchar_t* sec, const wchar_t* key, float def, const std::wstring& ini);
int   DmReadInt(const wchar_t* sec, const wchar_t* key, int def, const std::wstring& ini);
std::wstring DmReadStr(const wchar_t* sec, const wchar_t* key, const wchar_t* def, const std::wstring& ini);
void  DmWriteFloat(const wchar_t* sec, const wchar_t* key, float v, const std::wstring& ini);
void  DmWriteInt(const wchar_t* sec, const wchar_t* key, int v, const std::wstring& ini);
void  DmWriteStr(const wchar_t* sec, const wchar_t* key, const std::wstring& v, const std::wstring& ini);

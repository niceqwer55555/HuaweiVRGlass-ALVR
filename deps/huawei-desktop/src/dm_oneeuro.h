// dm_oneeuro.h —— 1€ 自适应低通滤波器（速度越低平滑越强，速度越高延迟越小）
//
// 论文：Casiez, Roussel, Vogel, "1€ Filter: A Simple Speed-Based Low-Pass Filter for
//       Noisy Input in Interactive Systems", CHI '12, doi:10.1145/2207676.2208639
// 参考实现：https://github.com/casiez/OneEuroFilter （BSD-3-Clause，Inria）
//
// 为什么用它做 3DoF 防抖：固定低通（或"角速度阈值/降采样"）在静止时想压住抖动就得把截止
// 频率压很低，转动时就会明显滞后 → 用户感觉"卡顿"。1€ 让截止频率随速度自适应：
//   cutoff = minCutoff + beta * |速度|
// 静止时 cutoff 很小（抖动被压掉），快速转头时 cutoff 自动升高（几乎不拖尾）。
#pragma once
#include <cmath>

// 一阶低通
struct DmLowPass {
    double y = 0, a = 1.0, s = 0;
    bool init = false;
    void setAlpha(double alpha) { a = alpha; }
    double filter(double value) {
        double result = init ? (a * value + (1.0 - a) * s) : value;
        y = value; s = result; init = true;
        return result;
    }
    double filterWithAlpha(double value, double alpha) { setAlpha(alpha); return filter(value); }
    bool hasLast() const { return init; }
    double lastRaw() const { return y; }
};

struct DmOneEuro {
    double freq = 90.0;
    double minCutoff = 1.5;
    double beta = 0.012;
    double dCutoff = 1.0;
    DmLowPass x, dx;

    explicit DmOneEuro(double f = 90.0, double mc = 1.5, double b = 0.012, double dc = 1.0) {
        setParams(f, mc, b, dc);
    }
    void setFrequency(double f) { if (f > 1.0) freq = f; }
    void setParams(double f, double mc, double b, double dc) {
        freq = (f > 1.0) ? f : 90.0;
        minCutoff = (mc > 0.001) ? mc : 0.001;
        beta = (b >= 0.0) ? b : 0.0;
        dCutoff = (dc > 0.001) ? dc : 1.0;
        x.setAlpha(alpha(minCutoff));
        dx.setAlpha(alpha(dCutoff));
    }
    void reset() { x.init = false; dx.init = false; }
    double alpha(double cutoff) const {
        double te = 1.0 / freq;
        double tau = 1.0 / (2.0 * 3.14159265358979323846 * cutoff);
        return 1.0 / (1.0 + tau / te);
    }
    double filter(double value) {
        double dvalue = x.hasLast() ? (value - x.lastRaw()) * freq : 0.0;
        double edvalue = dx.filterWithAlpha(dvalue, alpha(dCutoff));
        double cutoff = minCutoff + beta * fabs(edvalue);
        return x.filterWithAlpha(value, alpha(cutoff));
    }
};

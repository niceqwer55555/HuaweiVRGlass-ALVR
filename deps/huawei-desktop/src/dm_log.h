// dm_log.h —— 桌面模式日志（写到 exe 同目录 desktop_mode.log）
#pragma once
#include <windows.h>
#include <stdio.h>
#include <stdarg.h>
#include <string>

static FILE* g_logf = NULL;
static bool  g_logVerbose = false;

static std::wstring DmExeDir() {
    wchar_t buf[MAX_PATH] = {0};
    GetModuleFileNameW(NULL, buf, MAX_PATH);
    wchar_t* p = wcsrchr(buf, L'\\');
    if (p) *p = 0;
    return std::wstring(buf);
}
static inline void DmLogInit() {
    if (g_logf) return;
    std::wstring p = DmExeDir() + L"\\desktop_mode.log";
    g_logf = _wfopen(p.c_str(), L"ab");
    if (g_logf) {
        SYSTEMTIME st; GetLocalTime(&st);
        fprintf(g_logf, "=== %04d-%02d-%02d %02d:%02d:%02d ===\n", st.wYear, st.wMonth, st.wDay, st.wHour, st.wMinute, st.wSecond);
        fflush(g_logf);
    }
}
static inline void DmLog(const char* fmt, ...) {
    DmLogInit();
    if (!g_logf) return;
    SYSTEMTIME st; GetLocalTime(&st);
    fprintf(g_logf, "[%02d:%02d:%02d.%03d] ", st.wHour, st.wMinute, st.wSecond, st.wMilliseconds);
    va_list ap; va_start(ap, fmt);
    vfprintf(g_logf, fmt, ap);
    va_end(ap);
    fputc('\n', g_logf);
    fflush(g_logf);
}
static inline void DmLogV(const char* fmt, ...) {
    if (!g_logVerbose) return;
    DmLogInit();
    if (!g_logf) return;
    SYSTEMTIME st; GetLocalTime(&st);
    fprintf(g_logf, "[%02d:%02d:%02d.%03d] (v) ", st.wHour, st.wMinute, st.wSecond, st.wMilliseconds);
    va_list ap; va_start(ap, fmt);
    vfprintf(g_logf, fmt, ap);
    va_end(ap);
    fputc('\n', g_logf);
    fflush(g_logf);
}

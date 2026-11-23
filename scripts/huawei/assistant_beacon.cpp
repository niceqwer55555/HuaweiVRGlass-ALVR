// assistant_beacon.cpp
//
// The Huawei SteamVR driver (driver_huaweiVR.dll) checks at startup whether a
// mutex named "HwPCVRAssistant.exe" exists. If it does NOT exist, the driver
// ShellExecute's the real HwPCVRAssistant.exe from the registry Path. If it
// DOES exist, the driver assumes the assistant is already running and skips
// launching it.
//
// This tiny beacon just holds that mutex so the driver never launches the real
// assistant. No UI, no dependencies. Exit with -Stop (or kill the process).
//
// Build:  g++ -O2 -s -static -municode -mwindows assistant_beacon.cpp -o AssistantBeacon.exe
// Usage:  AssistantBeacon.exe          (start beacon, stays until killed)
//         AssistantBeacon.exe -Check   (exit 0 if beacon held, 1 if not)
#include <windows.h>
#include <cstring>

static const char* kMutexName = "HwPCVRAssistant.exe";

int wmain(int argc, wchar_t** argv) {
    bool check = (argc > 1 && wcscmp(argv[1], L"-Check") == 0);

    HANDLE h = CreateMutexA(NULL, TRUE, kMutexName);
    bool held = (h != NULL && GetLastError() != ERROR_ALREADY_EXISTS);

    if (check) {
        // In check mode we want to know whether SOMEONE ELSE holds it. If we
        // just created it, close it and report "not held".
        if (h) CloseHandle(h);
        return held ? 1 : 0;   // 0 = beacon/assistant present, 1 = absent
    }

    if (!held) {
        // Already held (beacon running or real assistant running). Nothing to do.
        if (h) CloseHandle(h);
        return 0;
    }

    // Hold the mutex forever (until process killed). A GUI app without a
    // window has no message queue, so GetMessage returns -1 immediately;
    // use a plain sleep loop instead.
    while (true) Sleep(60000);
    // unreachable; handle closes on process exit
    if (h) CloseHandle(h);
    return 0;
}

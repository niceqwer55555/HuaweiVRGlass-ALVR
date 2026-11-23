#!/usr/bin/env python3
"""
Restore the Huawei VR Glass panel to its native 3200x1600 extended-desktop mode.

Context: SteamVR's DirectMode takes the glasses panel out of the Windows desktop
and, on AMD (no NVAPI), killing SteamVR leaves it stuck at 1280x720. HwVRDesktop
(dashboard -> Desktop Casting) needs the panel at 3200x1600 to render both eyes.

Uses raw Win32 (ctypes) EnumDisplayDevicesW / EnumDisplaySettingsExW /
ChangeDisplaySettingsExW. No hardcoded drive letters; the glasses monitor is
found by EDID keyword (HWV0003 / RTK2560 / HUAWEI / VRGLASS / AV03 / CV10).

Usage:
  python restore-glasses-display.py --list
  python restore-glasses-display.py --restore
  python restore-glasses-display.py --restore --hz 60
"""
import argparse
import ctypes
from ctypes import wintypes

user32 = ctypes.WinDLL("user32", use_last_error=True)

# --- Win32 structures -------------------------------------------------------
class DISPLAY_DEVICEW(ctypes.Structure):
    _fields_ = [
        ("cb", wintypes.DWORD),
        ("DeviceName", wintypes.WCHAR * 32),
        ("DeviceString", wintypes.WCHAR * 128),
        ("StateFlags", wintypes.DWORD),
        ("DeviceID", wintypes.WCHAR * 128),
        ("DeviceKey", wintypes.WCHAR * 128),
    ]

class DEVMODEW(ctypes.Structure):
    # 220 bytes on Windows x64. Layout mirrors display.rs DevmodeW exactly.
    # The printer/display union after dmFields is 16 bytes total: POINTL(8) +
    # display_orientation(4) + display_fixed_output(4). Do NOT also add the
    # 8 printer shorts - that double-counts the union and shifts all offsets.
    _fields_ = [
        ("dmDeviceName", wintypes.WCHAR * 32),
        ("dmSpecVersion", wintypes.WORD),
        ("dmDriverVersion", wintypes.WORD),
        ("dmSize", wintypes.WORD),
        ("dmDriverExtra", wintypes.WORD),
        ("dmFields", wintypes.DWORD),
        ("dmPositionX", wintypes.LONG),
        ("dmPositionY", wintypes.LONG),
        ("dmDisplayOrientation", wintypes.DWORD),
        ("dmDisplayFixedOutput", wintypes.DWORD),
        ("dmColor", ctypes.c_short),
        ("dmDuplex", ctypes.c_short),
        ("dmYResolution", ctypes.c_short),
        ("dmTTOption", ctypes.c_short),
        ("dmCollate", ctypes.c_short),
        ("dmFormName", wintypes.WCHAR * 32),
        ("dmLogPixels", wintypes.WORD),
        ("dmBitsPerPel", wintypes.DWORD),
        ("dmPelsWidth", wintypes.DWORD),
        ("dmPelsHeight", wintypes.DWORD),
        ("dmDisplayFlags", wintypes.DWORD),
        ("dmDisplayFrequency", wintypes.DWORD),
        ("dmICMMethod", wintypes.DWORD),
        ("dmICMIntent", wintypes.DWORD),
        ("dmMediaType", wintypes.DWORD),
        ("dmDitherType", wintypes.DWORD),
        ("dmReserved1", wintypes.DWORD),
        ("dmReserved2", wintypes.DWORD),
        ("dmPanningWidth", wintypes.DWORD),
        ("dmPanningHeight", wintypes.DWORD),
    ]

ENUM_CURRENT_SETTINGS = -1
DISPLAY_DEVICE_ATTACHED = 0x00000001
DM_POSITION = 0x00000020
DM_PELSWIDTH = 0x00080000
DM_PELSHEIGHT = 0x00100000
DM_DISPLAYFREQUENCY = 0x00400000
CDS_UPDATEREGISTRY = 0x00000001
CDS_NORESET = 0x10000000
CDS_FULLSCREEN = 0x00000004
DISP_CHANGE_SUCCESSFUL = 0

GLASSES_KEYWORDS = ["HWV", "RTK2560", "HUAWEI", "VRGLASS", "VR GLASS", "AV03", "CV10"]


def enum_displays():
    out = []
    i = 0
    while True:
        dd = DISPLAY_DEVICEW()
        dd.cb = ctypes.sizeof(dd)
        if not user32.EnumDisplayDevicesW(None, i, ctypes.byref(dd), 0):
            break
        attached = bool(dd.StateFlags & DISPLAY_DEVICE_ATTACHED)
        # Enumerate the MONITOR child of this display adapter: its DeviceID
        # carries the EDID (e.g. DISPLAY\HWV0003\...), the adapter itself only
        # shows the PCI path (VEN_1002...).
        monitor_id = ""
        monitor_desc = ""
        j = 0
        while True:
            mon = DISPLAY_DEVICEW()
            mon.cb = ctypes.sizeof(mon)
            if not user32.EnumDisplayDevicesW(dd.DeviceName, j, ctypes.byref(mon), 0):
                break
            blob = mon.DeviceID.upper()
            if any(k in blob for k in GLASSES_KEYWORDS) or "DISPLAY\\" in blob:
                monitor_id = mon.DeviceID
                monitor_desc = mon.DeviceString
            j += 1
        out.append({
            "name": dd.DeviceName,
            "desc": dd.DeviceString,
            "id": dd.DeviceID,
            "monitor_id": monitor_id,
            "monitor_desc": monitor_desc,
            "attached": attached,
            "primary": bool(dd.StateFlags & 0x00000004),
        })
        i += 1
    return out


def is_glasses(disp):
    blob = (disp["monitor_id"] + " " + disp["monitor_desc"]).upper()
    return any(k in blob for k in GLASSES_KEYWORDS)


def current_mode(name):
    dm = DEVMODEW()
    dm.dmSize = ctypes.sizeof(dm)
    if not user32.EnumDisplaySettingsExW(name, ENUM_CURRENT_SETTINGS, ctypes.byref(dm), 0):
        return None
    return dm


def list_modes(name):
    modes = []
    i = 0
    seen = set()
    while True:
        dm = DEVMODEW()
        dm.dmSize = ctypes.sizeof(dm)
        if not user32.EnumDisplaySettingsExW(name, i, ctypes.byref(dm), 0):
            break
        key = (dm.dmPelsWidth, dm.dmPelsHeight, dm.dmDisplayFrequency)
        if key not in seen:
            seen.add(key)
            modes.append(key)
        i += 1
    return modes


def set_mode(name, width, height, hz, position=None, fullscreen=False, force=False):
    dm = DEVMODEW()
    dm.dmSize = ctypes.sizeof(dm)
    # In force mode prefer the first enumerated mode as template: the current
    # mode may be a bogus 0x0 left over from SteamVR DirectMode.
    if force:
        if not user32.EnumDisplaySettingsExW(name, 0, ctypes.byref(dm), 0):
            dm = DEVMODEW(); dm.dmSize = ctypes.sizeof(dm)
    elif not user32.EnumDisplaySettingsExW(name, ENUM_CURRENT_SETTINGS, ctypes.byref(dm), 0):
        dm = DEVMODEW(); dm.dmSize = ctypes.sizeof(dm)
        if not user32.EnumDisplaySettingsExW(name, 0, ctypes.byref(dm), 0):
            return False, "cannot read any current mode"
    dm.dmPelsWidth = width
    dm.dmPelsHeight = height
    if hz:
        dm.dmDisplayFrequency = hz
    dm.dmFields = DM_PELSWIDTH | DM_PELSHEIGHT | (DM_DISPLAYFREQUENCY if hz else 0)
    if position is not None:
        dm.dmPositionX, dm.dmPositionY = position
        dm.dmFields |= DM_POSITION
    flags = CDS_UPDATEREGISTRY | CDS_NORESET | (CDS_FULLSCREEN if fullscreen else 0)
    r = user32.ChangeDisplaySettingsExW(name, ctypes.byref(dm), None, flags, None)
    if r != DISP_CHANGE_SUCCESSFUL:
        return False, f"ChangeDisplaySettingsEx returned {r}"
    # Commit the registry-pending change
    r2 = user32.ChangeDisplaySettingsExW(None, None, None, 0, None)
    if r2 != DISP_CHANGE_SUCCESSFUL:
        return False, f"commit ChangeDisplaySettingsEx returned {r2}"
    return True, "ok"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--list", action="store_true", help="list displays and glasses modes")
    ap.add_argument("--restore", action="store_true", help="set glasses to 3200x1600")
    ap.add_argument("--hz", type=int, default=0, help="refresh rate (0 = keep current/default)")
    ap.add_argument("--width", type=int, default=3200)
    ap.add_argument("--height", type=int, default=1600)
    ap.add_argument("--force", action="store_true", help="set mode even if it is not in EnumDisplaySettings list")
    args = ap.parse_args()

    displays = enum_displays()
    print("Displays:")
    glasses = []
    for d in displays:
        tag = " <== GLASSES" if is_glasses(d) else ""
        print(f"  {d['name']}: {d['monitor_desc'] or d['desc']} attached={d['attached']}{tag}")
        print(f"      monitor={d['monitor_id']}")
        if is_glasses(d) and d["attached"]:
            glasses.append(d)

    if not glasses:
        print("\nNo attached glasses display found by EDID keyword.")
        return

    for g in glasses:
        cur = current_mode(g["name"])
        if cur:
            print(f"\n{g['name']} current: {cur.dmPelsWidth}x{cur.dmPelsHeight} @ {cur.dmDisplayFrequency}Hz "
                  f"pos=({cur.dmPositionX},{cur.dmPositionY})")
        modes = list_modes(g["name"])
        print(f"  available modes ({len(modes)}):")
        for w, h, hz in modes:
            mark = " *" if (w, h) == (args.width, args.height) else ""
            print(f"    {w}x{h} @ {hz}Hz{mark}")

        if args.restore:
            target = next((m for m in modes if m[0] == args.width and m[1] == args.height), None)
            if not target and not args.force:
                print(f"  !! {args.width}x{args.height} not in mode list; use --force to try anyway")
                continue
            hz = args.hz or (target[2] if target else 60)
            pos = (cur.dmPositionX, cur.dmPositionY) if cur else None
            print(f"  restoring to {args.width}x{args.height} @ {hz}Hz pos={pos} ...")
            ok, msg = set_mode(g["name"], args.width, args.height, hz, pos, force=args.force)
            print(f"  result: {'OK' if ok else 'FAIL'} ({msg})")
            if ok:
                after = current_mode(g["name"])
                if after:
                    print(f"  now: {after.dmPelsWidth}x{after.dmPelsHeight} @ {after.dmDisplayFrequency}Hz")


if __name__ == "__main__":
    main()

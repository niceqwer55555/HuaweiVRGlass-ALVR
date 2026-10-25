#!/usr/bin/env python3
"""Manage the "Include in VR library" flag of Steam non-Steam shortcuts.

Steam stores non-Steam games in ``<steam>/userdata/<steamid3>/config/shortcuts.vdf``
(binary VDF). A shortcut only shows up inside the SteamVR library when its
``OpenVR`` field is 1. That flag maps to the GUI option
"Include in VR library" (Steam -> right click the shortcut -> Properties).

This script only rewrites that single int32 field, keeps a .bak and refuses to
run while the Steam client is up (it would overwrite the file on exit).

Usage:
    vr_library.py list
    vr_library.py enable --all
    vr_library.py enable Doom Fallout
    vr_library.py disable --all

Options:
    --steam  <dir>   Steam installation root (auto-detected from the registry
                     or the usual install locations when omitted)
    --account <id>   SteamID3 / SteamID64 of the account to edit (auto: the one
                     with the newest timestamp in loginusers.vdf)
"""

import argparse
import os
import shutil
import struct
import subprocess
import sys

STEAMID64_BASE = 76561197960265728


# --------------------------------------------------------------------------- VDF
def read_string(buf, i):
    end = buf.index(b"\x00", i)
    return buf[i:end].decode("utf-8", "replace"), end + 1


def skip_value(buf, i, vtype):
    """Advance past one value of the given type. Returns new offset."""
    if vtype == 0x01:  # string
        _, i = read_string(buf, i)
    elif vtype == 0x02:  # int32
        i += 4
    elif vtype == 0x07:  # uint64
        i += 8
    elif vtype == 0x00:  # nested map
        depth = 1
        while depth and i < len(buf):
            t = buf[i]
            i += 1
            if t == 0x00:
                depth += 1
                _, i = read_string(buf, i)
            elif t == 0x08:
                depth -= 1
            elif t == 0x01:
                _, i = read_string(buf, i)
                _, i = read_string(buf, i)
            elif t == 0x02:
                i += 4
            elif t == 0x07:
                i += 8
            else:
                raise ValueError("unexpected type byte %02x" % t)
    else:
        raise ValueError("unexpected type byte %02x" % vtype)
    return i


def parse_shortcuts(path):
    """Return a list of dicts; each carries ``_offsets`` for int fields."""
    with open(path, "rb") as fh:
        buf = fh.read()
    i = 0
    if i < len(buf) and buf[i] == 0x00:
        i += 1
        _, i = read_string(buf, i)  # "shortcuts"
    entries = []
    while i < len(buf):
        if buf[i] == 0x08:
            i += 1
            break
        if buf[i] != 0x00:
            break
        i += 1
        index, i = read_string(buf, i)
        entry = {"_idx": index, "_offsets": {}}
        while i < len(buf) and buf[i] != 0x08:
            vtype = buf[i]
            i += 1
            key, i = read_string(buf, i)
            if vtype == 0x01:
                value, i = read_string(buf, i)
            elif vtype == 0x02:
                entry["_offsets"][key] = i
                value = struct.unpack_from("<i", buf, i)[0]
                i += 4
            elif vtype == 0x07:
                value = struct.unpack_from("<Q", buf, i)[0]
                i += 8
            elif vtype == 0x00:
                value = "<map>"
                i = skip_value(buf, i, 0x00)
            else:
                break
            entry[key] = value
        if i < len(buf) and buf[i] == 0x08:
            i += 1
        entries.append(entry)
    return buf, entries


# ------------------------------------------------------------------- discovery
def detect_steam_root():
    candidates = []
    try:  # Windows registry
        import winreg

        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, r"Software\Valve\Steam") as k:
            value, _ = winreg.QueryValueEx(k, "SteamPath")
            if value:
                candidates.append(value)
    except Exception:
        pass
    candidates += [
        r"C:\Program Files\Steam",
        r"C:\Program Files (x86)\Steam",
        r"D:\Program Files\Steam",
        r"D:\Steam",
        r"F:\Program Files\Steam",
    ]
    for c in candidates:
        if c and os.path.isfile(os.path.join(c, "steam.exe")):
            return os.path.abspath(c)
    for c in candidates:
        if c and os.path.isdir(c):
            return os.path.abspath(c)
    return None


def detect_account(steam_root):
    """Pick the account most recently used, from config/loginusers.vdf."""
    path = os.path.join(steam_root, "config", "loginusers.vdf")
    if not os.path.isfile(path):
        return None
    best, best_ts = None, -1
    with open(path, "r", encoding="utf-8", errors="replace") as fh:
        text = fh.read()
    current = None
    for line in text.splitlines():
        line = line.strip()
        if line.startswith('"') and line.endswith('"') and line[1:-1].isdigit():
            current = line[1:-1]
        elif line.startswith('"timestamp"') and current:
            try:
                ts = int(line.split('"')[-2])
            except (ValueError, IndexError):
                ts = 0
            if ts > best_ts:
                best, best_ts = current, ts
    if not best:
        return None
    sid64 = int(best)
    return sid64 - STEAMID64_BASE


def shortcut_path(steam_root, account):
    return os.path.join(steam_root, "userdata", str(account), "config", "shortcuts.vdf")


def steam_running():
    try:
        out = subprocess.run(
            ["tasklist", "/FI", "IMAGENAME eq steam.exe", "/NH"],
            capture_output=True, text=True, timeout=10,
        ).stdout
        return "steam.exe" in out.lower()
    except Exception:
        return False


# ---------------------------------------------------------------------- actions
def exe_status(exe):
    if not exe:
        return "?"
    cleaned = exe.strip().strip('"')
    if cleaned.startswith("/"):
        return "NOT-WINDOWS-PATH"
    return "ok" if os.path.isfile(cleaned) else "missing"


def cmd_list(args, path, buf, entries):
    print("shortcuts file : %s" % path)
    print("account        : %s" % args.account)
    print()
    print("  #   VRlib  Exe           Name")
    print("  --  -----  ------------  ------------------------------")
    for e in entries:
        print("  %-3s %-5s  %-12s  %s" % (
            e.get("_idx"),
            "YES" if e.get("OpenVR") == 1 else "no",
            exe_status(e.get("Exe")),
            e.get("AppName"),
        ))
    missing = sum(1 for e in entries if exe_status(e.get("Exe")) == "missing")
    foreign = sum(1 for e in entries if exe_status(e.get("Exe")) == "NOT-WINDOWS-PATH")
    print()
    print("total=%d  in-vr-library=%d  exe-missing=%d  non-windows-path=%d" % (
        len(entries),
        sum(1 for e in entries if e.get("OpenVR") == 1),
        missing, foreign,
    ))


def cmd_set(args, path, buf, entries, enable):
    value = 1 if enable else 0
    if args.all:
        targets = entries
    else:
        needles = [n.lower() for n in args.names]
        targets = [e for e in entries
                   if any(n in str(e.get("AppName", "")).lower() for n in needles)]
    if not targets:
        print("no shortcut matched")
        return 1
    if steam_running():
        print("ERROR: the Steam client is running. Close Steam first, otherwise it")
        print("       overwrites shortcuts.vdf on exit and the change is lost.")
        return 2
    changed = []
    data = bytearray(buf)
    for e in targets:
        off = e["_offsets"].get("OpenVR")
        if off is None:
            print("  skip #%s %s (no OpenVR field)" % (e.get("_idx"), e.get("AppName")))
            continue
        if e.get("OpenVR") == value:
            continue
        struct.pack_into("<i", data, off, value)
        changed.append(e)
    if not changed:
        print("nothing to change (all targets already have OpenVR=%d)" % value)
        return 0
    backup = path + ".bak_vrlibrary"
    shutil.copy2(path, backup)
    with open(path, "wb") as fh:
        fh.write(data)
    print("backup : %s" % backup)
    for e in changed:
        print("  #%s  OpenVR=%d  %s" % (e.get("_idx"), value, e.get("AppName")))
    print()
    print("%d shortcut(s) updated. Start Steam, then SteamVR - they now show up"
          % len(changed))
    print("in the SteamVR library panel.")
    return 0


def main():
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("action", choices=["list", "enable", "disable"])
    ap.add_argument("names", nargs="*", help="substrings of the shortcut name")
    ap.add_argument("--all", action="store_true", help="apply to every shortcut")
    ap.add_argument("--steam", help="Steam installation root")
    ap.add_argument("--account", help="SteamID3 or SteamID64")
    args = ap.parse_args()

    steam_root = args.steam or detect_steam_root()
    if not steam_root or not os.path.isdir(steam_root):
        print("ERROR: cannot locate the Steam installation; pass --steam <dir>")
        return 2
    print("steam root     : %s" % steam_root)

    if args.account:
        acc = int(args.account)
        if acc > STEAMID64_BASE:
            acc -= STEAMID64_BASE
    else:
        acc = detect_account(steam_root)
    if not acc:
        print("ERROR: cannot determine the Steam account; pass --account <steamid3>")
        return 2

    path = shortcut_path(steam_root, acc)
    if not os.path.isfile(path):
        print("ERROR: %s not found" % path)
        return 2

    buf, entries = parse_shortcuts(path)
    if args.action == "list":
        cmd_list(args, path, buf, entries)
        return 0
    if not args.all and not args.names:
        print("ERROR: give shortcut name(s) or pass --all")
        return 2
    return cmd_set(args, path, buf, entries, args.action == "enable")


if __name__ == "__main__":
    sys.exit(main())

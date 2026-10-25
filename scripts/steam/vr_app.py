#!/usr/bin/env python3
"""Register plain (non-VR) games with a SPECIFIC SteamVR install, without Steam.

Why this exists
---------------
SteamVR normally learns about non-Steam games from the Steam client, which
writes them into <steam>/config/steamapps.vrmanifest. Editing that file by hand
does not work (vrserver only reads it while the Steam client is feeding it), and
it would also modify Steam's own config - which we explicitly do not want.

The supported alternative is the OpenVR application manifest:
``IVRApplications::AddApplicationManifest``. SteamVR ships a command line
wrapper for it: ``vrcmd.exe --appmanifest <file.vrmanifest>``. Nothing under
the Steam installation is touched.

    manifest files live in <steamvr>/apps/   -> they belong to THAT SteamVR copy
    registration uses <steamvr>/bin/win64/vrcmd.exe

Registration requires a running vrserver (vrcmd otherwise fails with
VRInitError_Init_HmdNotFound), so add games any time and run `register` while
SteamVR is up.

Usage
-----
    vr_app.py add --name "Cyberpunk" --exe "D:\\Games\\cp.exe" [--args "-dx11"]
    vr_app.py list
    vr_app.py register          # push every manifest to the running SteamVR
    vr_app.py register --name Cyberpunk
    vr_app.py remove --name Cyberpunk

    --steamvr <dir>   target SteamVR root (default: auto-detect, see below)
    --launch          after registering, show how to start it

Auto-detect order for the SteamVR root: $VR_STEAMVR_DIR, then any directory
given in scripts/steam/steamvr-target.json, then the usual locations.
"""

import argparse
import json
import os
import re
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
TARGET_FILE = os.path.join(HERE, "steamvr-target.json")

CANDIDATES = [
    r"F:\PY\SteamVR2.18.4\SteamVR",
    r"D:\Program Files\Steam\steamapps\common\SteamVR",
    r"C:\Program Files\Steam\steamapps\common\SteamVR",
    r"C:\Program Files (x86)\Steam\steamapps\common\SteamVR",
]


# ------------------------------------------------------------------ helpers
def detect_steamvr():
    for c in [os.environ.get("VR_STEAMVR_DIR"), _read_target_file()]:
        if c and os.path.isdir(c):
            return os.path.abspath(c)
    for c in CANDIDATES:
        if os.path.isdir(c):
            return c
    return None


def _read_target_file():
    if not os.path.isfile(TARGET_FILE):
        return None
    try:
        with open(TARGET_FILE, "r", encoding="utf-8") as fh:
            return (json.load(fh) or {}).get("steamvrRoot")
    except Exception:
        return None


def slugify(name):
    s = re.sub(r"[^0-9a-zA-Z]+", ".", name).strip(".").lower()
    return s or "app"


def app_key(name):
    return "local." + slugify(name)


def manifest_dir(root):
    return os.path.join(root, "apps")


def manifest_path(root, name):
    return os.path.join(manifest_dir(root), slugify(name) + ".vrmanifest")


def load_manifest(path):
    with open(path, "r", encoding="utf-8") as fh:
        return json.load(fh)


def save_manifest(path, data):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w", encoding="utf-8") as fh:
        json.dump(data, fh, indent=2, ensure_ascii=False)
        fh.write("\n")


def build_manifest(name, exe, args, icon, description):
    exe = os.path.abspath(exe)
    icon = os.path.abspath(icon) if icon else ""
    return {
        "source": "external",
        "applications": [
            {
                "app_key": app_key(name),
                "launch_type": "binary",
                "binary_path_windows": exe,
                "binary_path_linux": "",
                "arguments": args or "",
                "image_path": icon,
                "strings": {
                    "en_us": {
                        "name": name,
                        "description": description or name,
                    }
                },
            }
        ],
    }


def list_manifests(root):
    d = manifest_dir(root)
    if not os.path.isdir(d):
        return []
    out = []
    for f in sorted(os.listdir(d)):
        if not f.endswith(".vrmanifest"):
            continue
        p = os.path.join(d, f)
        try:
            data = load_manifest(p)
            for a in data.get("applications", []):
                out.append((f, a))
        except Exception as e:
            out.append((f, {"app_key": "<unreadable: %s>" % e}))
    return out


def vrserver_running():
    # tasklist output is in the OEM code page (GBK on zh-CN), never UTF-8 -
    # compare raw bytes instead of decoding it.
    try:
        out = subprocess.run(["tasklist", "/FI", "IMAGENAME eq vrserver.exe", "/NH"],
                             capture_output=True, timeout=10).stdout
        return b"vrserver.exe" in out.lower()
    except Exception:
        return False


# ------------------------------------------------------------------ actions
def cmd_add(args, root):
    if not os.path.isfile(args.exe):
        print("WARNING: %s does not exist (you can still add it)" % args.exe)
    path = manifest_path(root, args.name)
    data = build_manifest(args.name, args.exe, args.args, args.icon, args.description)
    save_manifest(path, data)
    print("steamvr : %s" % root)
    print("manifest: %s" % path)
    print("app_key : %s" % app_key(args.name))
    if vrserver_running():
        print()
        print("vrserver is running - you can register right now:")
        print("  vr_app.py register --name \"%s\"" % args.name)
    else:
        print()
        print("Start SteamVR (headset ready), then run:")
        print("  vr_app.py register --name \"%s\"" % args.name)


def cmd_list(args, root):
    print("steamvr      : %s" % root)
    print("manifest dir : %s" % manifest_dir(root))
    print()
    items = list_manifests(root)
    if not items:
        print("no application manifests")
        return 0
    print("  %-34s %-26s %s" % ("NAME", "APP_KEY", "EXE"))
    for f, a in items:
        print("  %-34s %-26s %s" % (
            a.get("strings", {}).get("en_us", {}).get("name", "?"),
            a.get("app_key", "?"),
            a.get("binary_path_windows", ""),
        ))
    print()
    print("vrserver: %s" % ("running" if vrserver_running() else "NOT running"))
    return 0


def cmd_register(args, root):
    vrcmd = os.path.join(root, "bin", "win64", "vrcmd.exe")
    if not os.path.isfile(vrcmd):
        print("ERROR: vrcmd.exe not found: %s" % vrcmd)
        return 2
    if not vrserver_running():
        print("ERROR: vrserver is not running.")
        print("       Start SteamVR with the headset ready, then re-run this command.")
        return 3

    items = list_manifests(root)
    if args.name:
        items = [(f, a) for f, a in items
                 if args.name.lower() in str(a.get("strings", {}).get("en_us", {}).get("name", "")).lower()]
    if not items:
        print("no manifest matched")
        return 1

    ok = 0
    for f, a in items:
        p = os.path.join(manifest_dir(root), f)
        r = subprocess.run([vrcmd, "--appmanifest", p], capture_output=True, text=True, timeout=60)
        status = "ok" if r.returncode == 0 else "exit=%d" % r.returncode
        print("  %-34s -> %s" % (a.get("app_key", f), status))
        if r.returncode == 0:
            ok += 1
        elif r.stdout:
            print("      %s" % r.stdout.strip()[:200])
    print()
    print("%d/%d registered. They appear in the SteamVR library / dashboard." % (ok, len(items)))
    return 0 if ok else 1


def cmd_remove(args, root):
    path = manifest_path(root, args.name)
    if not os.path.isfile(path):
        print("not found: %s" % path)
        return 1
    os.remove(path)
    print("removed manifest: %s" % path)
    print("SteamVR forgets the app on next restart (or use SteamVR's own app list).")
    return 0


def main():
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("action", choices=["add", "list", "register", "remove"])
    ap.add_argument("--name", help="display name shown in SteamVR")
    ap.add_argument("--exe", help="full path to the game executable")
    ap.add_argument("--args", default="", help="launch arguments")
    ap.add_argument("--icon", default="", help="optional PNG cover image (absolute path)")
    ap.add_argument("--description", default="", help="optional description")
    ap.add_argument("--steamvr", help="target SteamVR root directory")
    args = ap.parse_args()

    root = args.steamvr or detect_steamvr()
    if not root or not os.path.isdir(root):
        print("ERROR: cannot locate the SteamVR install; pass --steamvr <dir>")
        return 2

    if args.action == "add":
        if not args.name or not args.exe:
            print("ERROR: --name and --exe are required")
            return 2
        cmd_add(args, root)
        return 0
    if args.action == "list":
        return cmd_list(args, root)
    if args.action == "register":
        return cmd_register(args, root)
    if args.action == "remove":
        if not args.name:
            print("ERROR: --name is required")
            return 2
        return cmd_remove(args, root)
    return 0


if __name__ == "__main__":
    sys.exit(main())

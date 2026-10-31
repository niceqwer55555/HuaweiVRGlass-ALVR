# Embed Huawei original libs (from glass reference project) into the client APK.
# Experiment: test whether libvrsdk.so can work without Huawei system services.
# Usage: python embed_huawei_libs.py <input.apk> <libs_dir> <output.apk>
# .so files are kept STORED (compress_type=0) as required.
import sys
import os
import zipfile
import shutil
import subprocess

BUILD_TOOLS = r"F:\Tools\AndroidSDK\build-tools\34.0.0"
JAVA_HOME = r"F:\Download\jdk17\jdk-17.0.17+10"
REPO_ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
KEYSTORE = os.path.join(REPO_ROOT, "build", "alvr_client_android", "debug.keystore")
SKIP = {"libxr_loader.so", "libalvr_client_openxr.so"}


def main():
    src_apk, libs_dir, out_apk = sys.argv[1], sys.argv[2], sys.argv[3]
    work = out_apk + ".unsigned"

    orig = zipfile.ZipFile(src_apk, "r")
    # collect original lib names
    existing = {os.path.basename(i.filename) for i in orig.infolist()
                if i.filename.startswith("lib/")}
    print("existing libs:", sorted(existing))

    with zipfile.ZipFile(work, "w") as out:
        for item in orig.infolist():
            if item.filename.startswith("META-INF/") and item.filename.endswith(
                    (".SF", ".RSA", ".DSA", ".EC")):
                continue  # drop old signature
            data = orig.read(item.filename)
            out.writestr(item.filename, data, compress_type=item.compress_type)
        added = []
        for name in sorted(os.listdir(libs_dir)):
            if not name.endswith(".so") or name in SKIP:
                continue
            path = os.path.join(libs_dir, name)
            with open(path, "rb") as f:
                data = f.read()
            out.writestr("lib/armeabi-v7a/" + name, data, compress_type=zipfile.ZIP_STORED)
            added.append(name)
        print("added %d libs:" % len(added))
        for n in added:
            print("  " + n)
    orig.close()

    # zipalign
    aligned = out_apk + ".aligned"
    subprocess.run([os.path.join(BUILD_TOOLS, "zipalign.exe"), "-f", "-p", "4",
                    work, aligned], check=True)
    # sign
    env = dict(os.environ)
    env["JAVA_HOME"] = JAVA_HOME
    subprocess.run([os.path.join(BUILD_TOOLS, "apksigner.bat"), "sign",
                    "--ks", KEYSTORE, "--ks-pass", "pass:alvrclient",
                    "--key-pass", "pass:alvrclient",
                    "--out", out_apk, aligned], check=True, env=env)
    os.remove(work)
    os.remove(aligned)
    print("OK ->", out_apk, os.path.getsize(out_apk), "bytes")


if __name__ == "__main__":
    main()

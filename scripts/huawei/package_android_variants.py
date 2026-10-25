# -*- coding: utf-8 -*-
"""Build the 3DoF and 6DoF variants of the Huawei VR Glass client from one APK.

The only difference between the two variants is the
`com.huawei.vr.application.freeDegree` meta-data in AndroidManifest.xml, which tells the
Huawei VR framework which kind of session to start ("3dof" = glasses IMU only, "6dof" =
tracking kit). The value is patched in place (same length bytes) so the manifest stays valid,
and the v1/v2 signature blocks are dropped, because the result has to be zipaligned and
re-signed afterwards:

    zipalign -f 4 <variant>-unsigned.apk <variant>-aligned.apk
    apksigner sign --ks <keystore> --ks-key-alias androiddebugkey \
        --ks-pass pass:android --key-pass pass:android \
        --out <variant>.apk <variant>-aligned.apk

Usage:
    python package_android_variants.py <source.apk> [<output dir>]
"""

import os
import sys
import zipfile

MANIFEST = 'AndroidManifest.xml'
FREE_DEGREE_KEY = 'com.huawei.vr.application.freeDegree'
# Free degree values, UTF-16LE, as stored in the binary XML string pool.
VARIANTS = {
    '3dof': [(b'6\x00d\x00o\x00f\x00', b'3\x00d\x00o\x00f\x00')],
    '6dof': [],
}
# Old v1 signature files: they refer to the previous key and must not survive.
SIGNATURE_SUFFIXES = ('.MF', '.SF', '.RSA', '.DSA', '.EC')


def strip_signature(name):
    upper = name.upper()
    return upper.startswith('META-INF/') and upper.endswith(SIGNATURE_SUFFIXES)


def build_variant(source_path, output_path, replacements):
    with zipfile.ZipFile(source_path, 'r') as zin:
        names = [n for n in zin.namelist() if not strip_signature(n)]
        manifest = zin.read(MANIFEST)

        total = 0
        for old, new in replacements:
            total += manifest.count(old)
            manifest = manifest.replace(old, new)

        with zipfile.ZipFile(output_path, 'w') as zout:
            for name in names:
                if name == MANIFEST:
                    # aapt keeps the manifest uncompressed, keep the same layout.
                    zout.writestr(
                        zipfile.ZipInfo(name, date_time=(2026, 10, 24, 17, 0, 0)),
                        manifest,
                        compress_type=zipfile.ZIP_STORED,
                    )
                else:
                    zout.writestr(name, zin.read(name))

    # Verify: free degree rewritten as asked, archive readable.
    expected = '3dof'.encode('utf-16-le') if replacements else '6dof'.encode('utf-16-le')
    with zipfile.ZipFile(output_path) as z:
        data = z.read(MANIFEST)
        assert expected in data, f'{output_path}: freeDegree not set as expected'
        assert z.testzip() is None, f'{output_path}: corrupt entry'

    return total


def main():
    if len(sys.argv) < 2:
        sys.exit(__doc__)

    source = sys.argv[1]
    out_dir = sys.argv[2] if len(sys.argv) > 2 else os.path.dirname(os.path.abspath(source))
    stem = os.path.splitext(os.path.basename(source))[0]

    for variant, replacements in VARIANTS.items():
        output = os.path.join(out_dir, f'{stem}_{variant}_unsigned.apk')
        count = build_variant(source, output, replacements)
        print(f'{variant}: rewrote {count} occurrence(s) -> {output}')


if __name__ == '__main__':
    main()

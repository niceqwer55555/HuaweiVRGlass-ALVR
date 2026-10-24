# -*- coding: utf-8 -*-
"""Patch freeDegree=6dof -> 3dof inside a built APK (same-length byte replace),
rebuilding the zip so entry CRCs stay valid."""
import shutil
import sys
import zipfile

src, dst = sys.argv[1], sys.argv[2]

REPLACEMENTS = [
    (b'6dof', b'3dof'),
    (b'6\x00d\x00o\x00f\x00', b'3\x00d\x00o\x00f\x00'),  # UTF-16LE
]

with zipfile.ZipFile(src, 'r') as zin:
    names = zin.namelist()
    manifest = zin.read('AndroidManifest.xml')
    total = 0
    for old, new in REPLACEMENTS:
        n = manifest.count(old)
        manifest = manifest.replace(old, new)
        total += n
    if total == 0:
        sys.exit('ERROR: no 6dof found in manifest')
    with zipfile.ZipFile(dst, 'w', zipfile.ZIP_DEFLATED) as zout:
        for name in names:
            if name == 'AndroidManifest.xml':
                # store manifest uncompressed like aapt does
                zout.writestr(
                    zipfile.ZipInfo(name, date_time=(2026, 10, 24, 17, 0, 0)),
                    manifest,
                    compress_type=zipfile.ZIP_STORED,
                )
            else:
                zout.writestr(name, zin.read(name))

print(f'patched {total} occurrence(s); wrote {dst}')
# verify
with zipfile.ZipFile(dst) as z:
    m = z.read('AndroidManifest.xml')
    assert b'6dof' not in m and b'6\x00d\x00o\x00f\x00' not in m
    bad = z.testzip()
    assert bad is None, f'corrupt entry: {bad}'
print('verify OK: no 6dof remains, zip intact')

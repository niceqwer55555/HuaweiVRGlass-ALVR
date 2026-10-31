# -*- coding: utf-8 -*-
"""Set the Huawei freeDegree meta-data of a built APK to an arbitrary value.

The old helpers (patch_apk_3dof.py / package-huawei-variants.ps1) rely on the
fact that "6dof" and "3dof" have the same length, so they can overwrite bytes
in place. "3dof|6dof" (the value Huawei uses for an app that accepts both
session types) is longer, which means the binary XML string pool has to be
rebuilt: offsets, chunk sizes and the overall document size all change.

Usage:
    python patch_apk_degree.py <source.apk> <dest.apk> [<value>]

Default value: 3dof|6dof
"""
import struct
import sys
import zipfile

RES_STRING_POOL_TYPE = 0x0001
RES_XML_TYPE = 0x0003

SORTED_FLAG = 0x1
UTF8_FLAG = 0x100

OLD_VALUES = ("6dof", "3dof", "3dof6dof", "3dof|6dof")


def _u16(data, off):
    return struct.unpack_from("<H", data, off)[0]


def _u32(data, off):
    return struct.unpack_from("<I", data, off)[0]


def _p16(value):
    return struct.pack("<H", value)


def _p32(value):
    return struct.pack("<I", value)


def _decode_utf8(data, off):
    # 16-bit length in UTF-16 code units, then the 8-bit length of the bytes.
    utf16_len = _u16(data, off)
    off += 2
    first = data[off]
    if first & 0x80:
        length = ((first & 0x7F) << 8) | data[off + 1]
        off += 2
    else:
        length = first
        off += 1
    return data[off:off + length].decode("utf-8"), utf16_len


def _decode_utf16(data, off):
    length = _u16(data, off)
    off += 2
    if length & 0x8000:
        length = ((length & 0x7FFF) << 16) | _u16(data, off)
        off += 2
    return data[off:off + length * 2].decode("utf-16-le"), length


def _encode_utf8(text):
    raw = text.encode("utf-8")
    out = _p16(len(text.encode("utf-16-le")) // 2)
    if len(raw) > 0x7F:
        out += bytes([0x80 | (len(raw) >> 8), len(raw) & 0xFF])
    else:
        out += bytes([len(raw)])
    return out + raw + b"\x00"


def _encode_utf16(text):
    raw = text.encode("utf-16-le")
    units = len(raw) // 2
    if units > 0x7FFF:
        return _p16(0x8000 | (units >> 16)) + _p16(units & 0xFFFF) + raw + b"\x00\x00"
    return _p16(units) + raw + b"\x00\x00"


def _pad4(data):
    return data + b"\x00" * ((4 - len(data) % 4) % 4)


def _parse_string_pool(data, start):
    """Returns (strings, flags, chunk_size, style_count, style_data)."""
    assert _u16(data, start) == RES_STRING_POOL_TYPE, "not a string pool"
    header_size = _u16(data, start + 2)
    chunk_size = _u32(data, start + 4)
    count = _u32(data, start + 8)
    style_count = _u32(data, start + 12)
    flags = _u32(data, start + 16)
    strings_start = _u32(data, start + 20)
    styles_start = _u32(data, start + 24)

    is_utf8 = bool(flags & UTF8_FLAG)
    decode = _decode_utf8 if is_utf8 else _decode_utf16

    base = start + strings_start
    strings = []
    for i in range(count):
        offset = _u32(data, start + header_size + 4 * i)
        text, _ = decode(data, base + offset)
        strings.append(text)

    style_data = b""
    if style_count and styles_start:
        style_data = data[start + styles_start:start + chunk_size]

    return strings, flags, chunk_size, style_count, style_data


def _build_string_pool(strings, flags, style_count, style_data):
    is_utf8 = bool(flags & UTF8_FLAG)
    encode = _encode_utf8 if is_utf8 else _encode_utf16

    data = b""
    offsets = []
    for text in strings:
        offsets.append(len(data))
        # aapt aligns every UTF-8 string entry to 4 bytes; harmless for UTF-16.
        data = _pad4(data + encode(text))
    data = _pad4(data)

    header_size = 28
    strings_start = header_size + 4 * len(strings) + 4 * style_count
    styles_start = strings_start + len(data) if (style_count and style_data) else 0
    chunk_size = strings_start + len(data) + len(style_data)

    out = _p16(RES_STRING_POOL_TYPE) + _p16(header_size) + _p32(chunk_size)
    out += _p32(len(strings)) + _p32(style_count) + _p32(flags)
    out += _p32(strings_start) + _p32(styles_start)
    for offset in offsets:
        out += _p32(offset)
    out += b"\x00" * (4 * style_count)
    out += data + style_data
    assert len(out) == chunk_size
    return out


def patch_manifest(manifest, value):
    total_size = _u32(manifest, 4)
    assert _u16(manifest, 0) == RES_XML_TYPE

    pool_start = _u16(manifest, 2)
    strings, flags, chunk_size, style_count, style_data = _parse_string_pool(
        manifest, pool_start
    )

    patched = 0
    for i, text in enumerate(strings):
        if text in OLD_VALUES:
            strings[i] = value
            patched += 1
    if not patched:
        raise SystemExit("ERROR: no freeDegree value found in the manifest")

    # The pool is no longer sorted by value; drop the flag so parsers do not
    # binary-search stale ordering.
    flags &= ~SORTED_FLAG

    new_pool = _build_string_pool(strings, flags, style_count, style_data)
    new_manifest = manifest[:pool_start] + new_pool + manifest[pool_start + chunk_size:]
    new_manifest = new_manifest[:4] + _p32(len(new_manifest)) + new_manifest[8:]
    assert _u32(new_manifest, 4) == len(new_manifest)
    return new_manifest, patched


def main():
    if len(sys.argv) < 3:
        raise SystemExit(__doc__)
    src, dst = sys.argv[1], sys.argv[2]
    value = sys.argv[3] if len(sys.argv) > 3 else "3dof|6dof"

    with zipfile.ZipFile(src) as zin:
        infos = zin.infolist()
        manifest = zin.read("AndroidManifest.xml")
        patched_manifest, patched = patch_manifest(manifest, value)

        with zipfile.ZipFile(dst, "w") as zout:
            for info in infos:
                name = info.filename
                # v1 signatures die with the rewrite; apksigner adds fresh ones.
                if name.startswith("META-INF/") and name.upper().endswith(
                    (".SF", ".RSA", ".DSA", ".MF", ".EC")
                ):
                    continue
                data = (
                    patched_manifest
                    if name == "AndroidManifest.xml"
                    else zin.read(name)
                )
                new_info = zipfile.ZipInfo(name, date_time=info.date_time)
                new_info.compress_type = info.compress_type
                new_info.external_attr = info.external_attr
                new_info.create_system = info.create_system
                zout.writestr(new_info, data)

    # Re-read the result: the pool must still parse and carry the new value.
    with zipfile.ZipFile(dst) as check:
        assert check.testzip() is None, "corrupt archive"
        strings, _, _, _, _ = _parse_string_pool(
            check.read("AndroidManifest.xml"), _u16(check.read("AndroidManifest.xml"), 2)
        )
    assert value in strings, "patched value missing from string pool"

    print(f"patched {patched} string(s) -> {value!r}; wrote {dst}")


if __name__ == "__main__":
    main()

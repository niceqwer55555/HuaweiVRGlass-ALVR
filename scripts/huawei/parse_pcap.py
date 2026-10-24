import struct

try:
    data = open(r'F:\Download\cap9943.pcapng', 'rb').read()
except FileNotFoundError:
    print('pcapng missing')
    raise SystemExit

off = 0
pkts = 0
flows = {}
while off + 12 <= len(data):
    btype, blen = struct.unpack_from('<II', data, off)
    if blen < 12 or off + blen > len(data):
        break
    if btype == 6:  # Enhanced Packet Block
        pkts += 1
        caplen = struct.unpack_from('<I', data, off + 20)[0]
        pkt = data[off + 28:off + 28 + min(caplen, 64)]
        if len(pkt) >= 34:
            src = '.'.join(str(b) for b in pkt[26:30])
            dst = '.'.join(str(b) for b in pkt[30:34])
            proto = pkt[23]
            flows[(src, dst, proto)] = flows.get((src, dst, proto), 0) + 1
    off += blen

print('packets:', pkts)
for k, v in flows.items():
    print(k, v)

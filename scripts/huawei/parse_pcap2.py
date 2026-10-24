import struct
from collections import Counter

data = open(r'F:\Download\capall.pcapng', 'rb').read()

off = 0
pkts = 0
by_pair = Counter()
udp_ports = Counter()
while off + 12 <= len(data):
    btype, blen = struct.unpack_from('<II', data, off)
    if blen < 12 or off + blen > len(data):
        break
    if btype == 6:
        pkts += 1
        caplen = struct.unpack_from('<I', data, off + 20)[0]
        pkt = data[off + 28:off + 28 + min(caplen, 78)]
        if len(pkt) >= 34:
            src = '.'.join(str(b) for b in pkt[26:30])
            dst = '.'.join(str(b) for b in pkt[30:34])
            proto = pkt[23]
            key = (src, dst, proto)
            by_pair[key] += 1
            if proto == 17 and len(pkt) >= 38:  # UDP
                sp, dp = struct.unpack_from('>HH', pkt, 34)
                udp_ports[(sp, dp)] += 1
    off += blen

print('total packets:', pkts)
print('--- 与手机 192.168.43.1 相关 ---')
for (s, d, p), n in by_pair.most_common(40):
    if '192.168.43.1' in (s, d):
        print(f'{s} -> {d}  proto={p}  n={n}')
print('--- UDP 端口对 top20 ---')
for (sp, dp), n in udp_ports.most_common(20):
    print(f'sport={sp} dport={dp}  n={n}')

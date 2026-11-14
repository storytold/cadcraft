# -*- coding: utf-8 -*-
"""定位 out/plant.dxf 里那些“乱码文字”到底以什么字节存着（结果写 UTF-8 文件，避开 GBK 控制台）。"""
import io

raw = io.open('out/plant.dxf', 'rb').read()
out = io.open('out/probe-dxf.txt', 'w', encoding='utf-8', newline='\n')
out.write('size %d\n' % len(raw))
probes = {
    'utf8 ¶ (C2 B6)': '\u00b6'.encode('utf-8'),
    'utf8 ¶þ (C2 B6 C3 BE)': '\u00b6\u00fe'.encode('utf-8'),
    'raw gbk B6 FE': b'\xb6\xfe',
    'escape \\U+00B6': b'\\U+00B6',
    'escape \\U+4E8C': b'\\U+4E8C',
    'utf8 二级道路': '二级道路'.encode('utf-8'),
}
for name, pat in probes.items():
    i = raw.find(pat)
    out.write('%-24s at %-10d %s\n' % (name, i, '' if i < 0 else repr(raw[i-45:i+25])))

lines = raw.split(b'\r\n') if b'\r\n' in raw else raw.split(b'\n')
hits = [i for i, ln in enumerate(lines) if any(b > 0x7F for b in ln)]
out.write('non-ascii lines: %d\n' % len(hits))
for i in hits[:8]:
    prev = lines[i-1].strip() if i > 0 else b''
    out.write('  line %d group=%r text=%r\n' % (i, prev, lines[i][:60]))
out.close()
print('written out/probe-dxf.txt')

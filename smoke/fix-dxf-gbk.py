# -*- coding: utf-8 -*-
r"""把 DXF 里「GBK 字节被当成 Latin-1 解码」的文字修回中文。

## 为什么需要（2026-10-23 实测）
CADCraft 读 DWG 时对未标注编码的 TEXT/MTEXT 按 Latin-1 解码 ⇒ 图纸里用 GBK 直存的文字
（例：`二级` 两个字节 B6 FE）在渲染/出 PDF 时变成 `¶þ`。写出来的 DXF 长这样：

    {\C7;\U+00B6\U+00FE}          ← \U+00XX 就是那两个 GBK 字节

## 做法
把连续的 `\U+00HH`（HH ≥ 0x80）还原成字节串，按 GBK 解码 → 再把结果逐字写成 `\U+XXXX`
（ASCII 字符原样保留）。解不开的序列**原样不动**，不猜。

## 用法
    python smoke\fix-dxf-gbk.py in.dxf out.dxf [--stats]
退出码 0 = 跑完（哪怕 0 处修改）；2 = 输入不合法。
"""
import io
import re
import sys

# \U+00XX 其中 XX 在 80..FF（Latin-1 扩展区，正是“GBK 字节被误解码”的落点）
RUN = re.compile(r'(?:\\U\+00[89A-Fa-f][0-9A-Fa-f])+')


def esc(ch):
    """把非 ASCII 字符写成 DXF 的 \\U+XXXX 转义。"""
    return '\\U+%04X' % ord(ch)


def fix_run(match):
    run = match.group(0)
    n = len(run) // 7
    # `\U+00B6` 的十六进制在最后两位（索引 5..7）；早一版取了 4..6 会得到 0B 这种控制字符
    raw = bytes(int(run[i * 7 + 5:i * 7 + 7], 16) for i in range(n))
    if not all(0x20 <= b <= 0xFF for b in raw):
        return run  # 含控制字节 ⇒ 不可能是 GBK 文本，别动
    for enc in ('gbk', 'gb18030'):
        try:
            s = raw.decode(enc)
        except UnicodeDecodeError:
            continue
        if not s or any(ord(c) < 0x20 for c in s) or any(0xD800 <= ord(c) <= 0xDFFF for c in s):
            continue  # 解出来有控制字符/代理对 ⇒ 不是我们要修的 GBK 文本
        if all(0x20 <= ord(c) < 0x7F for c in s):
            continue  # 解出来还是 ASCII 乱码 ⇒ 不是 GBK，别动
        return ''.join(c if 0x20 <= ord(c) < 0x7F else esc(c) for c in s)
    return run


def main(argv):
    if len(argv) < 3:
        print('usage: python smoke\\fix-dxf-gbk.py in.dxf out.dxf [--stats]')
        return 2
    src, dst = argv[1], argv[2]
    stats = '--stats' in argv
    try:
        text = io.open(src, encoding='utf-8', errors='surrogateescape', newline='').read()
    except OSError as e:
        print('读不了 %s: %s' % (src, e))
        return 2
    runs = RUN.findall(text)
    fixed, count = RUN.subn(lambda m: fix_run(m), text)
    io.open(dst, 'w', encoding='utf-8', errors='surrogateescape', newline='').write(fixed)
    if stats:
        changed = sum(1 for r in runs if fix_run(re.match(RUN.pattern, r)))
        print('escaped runs=%d, changed=%d, bytes %d -> %d'
              % (len(runs), changed, len(text), len(fixed)))
        # 抽样打印前 5 个改动，方便人眼核
        shown = 0
        for m in RUN.finditer(text):
            new = fix_run(m)
            if new != m.group(0):
                print('  %r -> %r' % (m.group(0), new))
                shown += 1
                if shown >= 5:
                    break
    return 0


if __name__ == '__main__':
    sys.exit(main(sys.argv))

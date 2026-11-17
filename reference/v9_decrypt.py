#!/usr/bin/env python3
"""天正 T20 V9（真正天正格式）加密字段解密参考实现。

实测自「02建筑图纸/02 配电装置室建筑施工图.dwg」（AC1032/DWG2018，967 个天正实体）。

结论（取代旧的 v9_decrypt.py 里那套「动态密钥 + XOR」——那是错的，只做了 round-trip
自测，从未用真实密文验证）：

- 真正天正格式的 text 流里，加密固定字段是 **Base-26 编码，偏移 key2 = 0xD0**。
  每字节 b → 两个字符 chr(0xD0 + b//26)、chr(0xD0 + b%26)，密文字符范围 U+00D0..U+00E9。
- **无 XOR**（旧版 V8 的 XOR 0x05 不适用于此）。解出即是明文字节。
- 明文是数值的 ASCII：整数用 %d，double 用定点格式（如 "0000000000000000.00817"）。

text 流布局（read_variable_text 逐串读，每串 = BS 长度前缀 + UTF-16LE）：
  [产品 GUID(明文 UTF-16)]
  [加密固定字段 × N]        ← 本文件解密的对象
  [TDBXDATAMAP_BLOCK_BEGIN_]
  [字段名/值(明文)]          ← 中文语义字段名，如「门口线偏移距离」「乙级防火门」
  [TDBXDATAMAP_BLOCK_END_]
  [关联对象 GUID(明文)]
  [加密固定字段 × M]
"""

KEY2 = 0xD0  # Base-26 字符偏移（密文字符 = KEY2 + 0..25，即 U+00D0..U+00E9）


def base26_decode(cipher: str, key2: int = KEY2) -> bytes:
    """Base-26 解码：每两个密文字符 → 一字节 (hi*26 + lo)。"""
    units = list(map(ord, cipher))
    assert len(units) % 2 == 0, f"密文长度必须是偶数，实为 {len(units)}"
    out = bytearray()
    for i in range(0, len(units), 2):
        hi = units[i] - key2
        lo = units[i + 1] - key2
        assert 0 <= hi <= 25 and 0 <= lo <= 25, f"密文字符 {units[i]:#x}/{units[i+1]:#x} 超出 key2+0..25"
        out.append(hi * 26 + lo)
    return bytes(out)


def decrypt(cipher: str, key2: int = KEY2) -> str:
    """解密加密字段：密文字符串 → 明文 ASCII。"""
    return base26_decode(cipher, key2).decode("ascii")


if __name__ == "__main__":
    # 自测：用真实图纸 dump 出的密文 ↔ 明文对
    samples = [
        # (密文字符串, 期望明文)
        ("ÒÔÒÒÒÒÑé", "8663"),
        ("ÑæÑæÑæÑæÑæÑæÑæÑæÑæÑæÑæÑæÑæÑæÑæÑæÑäÑæÑæÒÔÑçÒÓ", "0000000000000000.00817"),
        ("ÒÓÒÕÒÔÒÒÒÐÒÔÑéÑæÒÔÒÕÒÔÒÔÑéÑèÑçÒÓÑäÒÓÒÐÒÔ", "7986483089883217.748"),
        ("ÒÐÑæÒÑ", "405"),
    ]
    for cipher, want in samples:
        got = decrypt(cipher)
        assert got == want, f"{cipher!r} -> {got!r} != {want!r}"
        hexs = " ".join(f"{ord(c):04x}" for c in cipher)
        print(f"[{hexs}] -> {got!r} OK")
    print("V9 解密自测通过（真实密文）")

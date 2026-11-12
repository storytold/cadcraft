#!/usr/bin/env python3
"""天正 T20 数据混淆算法（TgDataEncrypt）参考实现。

逆向自 tch_kernal.arx（T20 V8.0, sys24x64）：
- TDEncrypt   = sub_1c49b4b0
- TDUnEncrypt = sub_1c49b320
- 密钥 0x05 来自硬编码常量 0x528 的第二字节（sub_1c49b310 写入上下文）

算法：
  加密 = 每个字节 (b ^ 0x05)，再编码为 ('A' + b//26)('A' + b%26)
  解密 = 每两个字母 ((c0-'A')*26 + (c1-'A'))，再 ^ 0x05

启用条件：仅 DWG 2004+（保存版本号 >= 16）；更早版本明文直写。

注意：Base-26 只编码低字节（0-255），天正只对可单字节表示的内容
（数值/布尔先 swprintf 成 ASCII 字符串）做加密。
"""

KEY = 0x05


def encrypt(plaintext: str) -> str:
    """天正加密：XOR 0x05 后 Base-26 编码。"""
    out = []
    for ch in plaintext:
        b = (ord(ch) & 0xFF) ^ KEY
        out.append(chr(ord("A") + b // 26))
        out.append(chr(ord("A") + b % 26))
    return "".join(out)


def decrypt(ciphertext: str) -> str:
    """天正解密：Base-26 解码后 XOR 0x05。"""
    if len(ciphertext) % 2 != 0:
        raise ValueError(f"密文长度必须是偶数，实际 {len(ciphertext)}")
    out = []
    for i in range(0, len(ciphertext), 2):
        c0 = ord(ciphertext[i]) - ord("A")
        c1 = ord(ciphertext[i + 1]) - ord("A")
        if not (0 <= c0 < 26 and 0 <= c1 < 26):
            raise ValueError(f"非法 Base-26 字符: {ciphertext[i:i+2]!r}")
        b = c0 * 26 + c1
        out.append(chr(b ^ KEY))
    return "".join(out)


if __name__ == "__main__":
    tests = ["0", "1", "-1", "3.14159", "100.000000", "wall", "WALL", "pipe-123"]
    for t in tests:
        c = encrypt(t)
        d = decrypt(c)
        assert d == t, f"{t!r} -> {c!r} -> {d!r} FAILED"
        print(f"{t!r:20} -> {c!r:40} -> {d!r}")
    print("OK")

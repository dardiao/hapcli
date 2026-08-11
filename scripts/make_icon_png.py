#!/usr/bin/env python3
"""生成 hapcli 应用图标（.icns，仅用标准库）。

画一个深色圆角方块，中央是绿色的 ">" 提示符和白色下划线光标，
然后按 macOS ICNS 格式（PNG 分块）打包为 .icns。
"""

import struct
import sys
import zlib

SIZE = 1024
ROUND = 190

BG = (0x0d, 0x0f, 0x12, 255)
GREEN = (0x3f, 0xc5, 0x6b, 255)
WHITE = (0xf2, 0xf4, 0xf7, 255)

# ICNS 分块类型 -> 像素尺寸
ICNS_SIZES = [
    (b"icp4", 16),
    (b"icp5", 32),
    (b"icp6", 64),
    (b"ic07", 128),
    (b"ic08", 256),
    (b"ic09", 512),
    (b"ic10", 1024),
]


def lerp(a, b, t):
    return int(a + (b - a) * t)


def inside_rounded(x, y):
    """判断点是否在圆角矩形内。"""
    if not (0 <= x < SIZE and 0 <= y < SIZE):
        return False
    inner_min = ROUND
    inner_max = SIZE - 1 - ROUND
    if inner_min <= x <= inner_max or inner_min <= y <= inner_max:
        return True
    cx = inner_min if x < inner_min else inner_max
    cy = inner_min if y < inner_min else inner_max
    dx, dy = x - cx, y - cy
    return dx * dx + dy * dy <= ROUND * ROUND


def dist_to_segment(px, py, ax, ay, bx, by):
    vx, vy = bx - ax, by - ay
    wx, wy = px - ax, py - ay
    length_sq = vx * vx + vy * vy
    if length_sq == 0:
        return ((px - ax) ** 2 + (py - ay) ** 2) ** 0.5
    t = max(0.0, min(1.0, (wx * vx + wy * vy) / length_sq))
    cx, cy = ax + vx * t, ay + vy * t
    return ((px - cx) ** 2 + (py - cy) ** 2) ** 0.5


def chevron_dist(x, y):
    return min(
        dist_to_segment(x, y, 290, 390, 470, 512),
        dist_to_segment(x, y, 470, 512, 290, 634),
    )


def underline_dist(x, y):
    return dist_to_segment(x, y, 520, 570, 790, 570)


def render_master():
    """渲染 1024x1024 RGBA 主图。"""
    pixels = bytearray()
    for y in range(SIZE):
        for x in range(SIZE):
            if not inside_rounded(x, y):
                pixels.extend((0, 0, 0, 0))
                continue
            color = BG
            d_chevron = chevron_dist(x, y)
            d_underline = underline_dist(x, y)
            if d_chevron <= 60:
                color = GREEN
            elif d_underline <= 38:
                color = WHITE
            if 60 < d_chevron <= 76:
                t = (d_chevron - 60) / 16
                color = tuple(lerp(GREEN[i], BG[i], t) for i in range(4))
            elif 38 < d_underline <= 54:
                t = (d_underline - 38) / 16
                color = tuple(lerp(WHITE[i], BG[i], t) for i in range(4))
            pixels.extend(color)
    return bytes(pixels)


def downscale(master, target):
    """面积平均缩放到 target x target。"""
    factor = SIZE // target
    out = bytearray()
    for oy in range(target):
        for ox in range(target):
            r = g = b = a = 0
            for dy in range(factor):
                row = (oy * factor + dy) * SIZE * 4
                base = (ox * factor) * 4
                for dx in range(factor):
                    i = row + base + dx * 4
                    r += master[i]
                    g += master[i + 1]
                    b += master[i + 2]
                    a += master[i + 3]
            n = factor * factor
            out.extend((r // n, g // n, b // n, a // n))
    return bytes(out)


def png_bytes(size, rgba):
    def chunk(kind, data):
        return (
            struct.pack(">I", len(data))
            + kind
            + data
            + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)
        )

    raw = bytearray()
    for y in range(size):
        raw.append(0)
        raw.extend(rgba[y * size * 4 : (y + 1) * size * 4])
    out = b"\x89PNG\r\n\x1a\n"
    out += chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0))
    out += chunk(b"IDAT", zlib.compress(bytes(raw), 9))
    out += chunk(b"IEND", b"")
    return out


def main():
    out_path = sys.argv[1] if len(sys.argv) > 1 else "hapcli.icns"
    master = render_master()
    if out_path.endswith(".icns"):
        write_icns(out_path, master)
    else:
        with open(out_path, "wb") as f:
            f.write(png_bytes(SIZE, master))
    print(f"icon written: {out_path}")


def write_icns(out_path, master):
    chunks = []
    for chunk_type, size in ICNS_SIZES:
        rgba = downscale(master, size) if size < SIZE else master
        data = png_bytes(size, rgba)
        chunks.append(chunk_type + struct.pack(">I", len(data) + 8) + data)

    total = 8 + sum(len(c) for c in chunks)
    with open(out_path, "wb") as f:
        f.write(b"icns" + struct.pack(">I", total))
        for chunk in chunks:
            f.write(chunk)


if __name__ == "__main__":
    main()

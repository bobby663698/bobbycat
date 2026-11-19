#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""换成自己的图标：输入一张 1024×1024 PNG，产出构建脚本要的全部尺寸。

用法:
    python tools/set_icon.py my_logo.png

产出（覆盖 assets/icons/，文件名保持不变，构建脚本无需改动）：
    concat.ico                        Windows 可执行文件图标（16-256）
    concat_logo_{16,32,64,128,256,512}.png   macOS / Linux / 关于窗口
"""
import os
import sys

from PIL import Image

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ICONS = os.path.join(ROOT, "assets", "icons")
SIZES = (16, 32, 64, 128, 256, 512)


def main():
    if len(sys.argv) < 2:
        print(__doc__)
        return 1
    src = sys.argv[1]
    im = Image.open(src).convert("RGBA")
    if im.width != im.height:
        side = min(im.size)
        left = (im.width - side) // 2
        top = (im.height - side) // 2
        im = im.crop((left, top, left + side, top + side))
        print(f"裁成正方形 {side}x{side}")
    if im.width < 512:
        print(f"警告：源图只有 {im.width}px，512 尺寸会放大发糊")

    os.makedirs(ICONS, exist_ok=True)
    for s in SIZES:
        out = os.path.join(ICONS, f"concat_logo_{s}.png")
        im.resize((s, s), Image.LANCZOS).save(out, "PNG")
        print("OK  ", os.path.relpath(out, ROOT))

    ico = os.path.join(ICONS, "concat.ico")
    im.save(ico, sizes=[(s, s) for s in (16, 24, 32, 48, 64, 128, 256)])
    print("OK  ", os.path.relpath(ico, ROOT))
    return 0


if __name__ == "__main__":
    sys.exit(main())

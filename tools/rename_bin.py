#!/usr/bin/env python
"""把二进制名和图标文件名从 concat 改成 bobbycat（BobbyCat 品牌收尾）。

只改「名字」，不动任何逻辑。跑完按文件打印替换次数，便于复核。
"""
import os
import re
import shutil
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

# (相对路径, 旧, 新)
RULES = [
    # ---- 二进制名 ----
    ("src/crates/concat/Cargo.toml", 'name = "concat"\npath = "src/main.rs"',
     'name = "bobbycat"\npath = "src/main.rs"'),

    # macOS 打包
    (".github/workflows/build-app.yml",
     'cp "src/target/${{ matrix.target }}/app/concat" "$app/Contents/MacOS/concat"',
     'cp "src/target/${{ matrix.target }}/app/bobbycat" "$app/Contents/MacOS/bobbycat"'),
    (".github/workflows/build-app.yml",
     "<key>CFBundleExecutable</key><string>concat</string>",
     "<key>CFBundleExecutable</key><string>bobbycat</string>"),

    # Linux 暂存
    (".github/workflows/build-app.yml",
     'cp "src/target/${{ matrix.target }}/app/concat" "stage/$name/concat"',
     'cp "src/target/${{ matrix.target }}/app/bobbycat" "stage/$name/bobbycat"'),
    (".github/workflows/build-app.yml", "Exec=concat", "Exec=bobbycat"),
    (".github/workflows/build-app.yml", "Icon=concat\n", "Icon=bobbycat\n"),
    (".github/workflows/build-app.yml", "StartupWMClass=concat", "StartupWMClass=bobbycat"),
    (".github/workflows/build-app.yml", "Name=Concat\n          Comment=Video editor",
     "Name=BobbyCat\n          Comment=Video editor"),
    (".github/workflows/build-app.yml",
     'cp assets/icons/concat_logo_256.png "stage/$name/concat.png"',
     'cp assets/icons/bobbycat_logo_256.png "stage/$name/bobbycat.png"'),

    # Windows 暂存
    (".github/workflows/build-app.yml",
     'Copy-Item "src\\target\\${{ matrix.target }}\\app\\concat.exe" $dir',
     'Copy-Item "src\\target\\${{ matrix.target }}\\app\\bobbycat.exe" $dir'),

    # ---- Windows 安装器 ----
    ("assets/windows/concat.iss", r'{app}\concat.exe', r'{app}\bobbycat.exe'),
    ("assets/windows/concat.iss", r'..\icons\concat.ico', r'..\icons\bobbycat.ico'),
    ("assets/windows/concat.iss", r'{app}\concat.ico', r'{app}\bobbycat.ico'),
    ("assets/windows/concat.wxs", "[INSTALLFOLDER]concat.exe", "[INSTALLFOLDER]bobbycat.exe"),
    ("assets/windows/concat.wxs", "assets\\icons\\concat.ico", "assets\\icons\\bobbycat.ico"),
    ("assets/windows/concat.wxs", 'Id="concat.ico"', 'Id="bobbycat.ico"'),
    ("assets/windows/concat.wxs", 'Value="concat.ico"', 'Value="bobbycat.ico"'),
    ("assets/windows/concat.wxs", 'Icon="concat.ico"', 'Icon="bobbycat.ico"'),

    # ---- 冒烟测试 ----
    ("scripts/ci/smoke-windows.ps1", '"concat.exe"', '"bobbycat.exe"'),
    ("scripts/ci/smoke-windows.ps1", "concat.exe --", "bobbycat.exe --"),
    ("scripts/ci/smoke-linux.sh", 'local bin="$dir/concat"', 'local bin="$dir/bobbycat"'),
    ("scripts/ci/smoke-linux.sh", '"concat --version"', '"bobbycat --version"'),
    ("scripts/ci/smoke-linux.sh", '"concat --smoke"', '"bobbycat --smoke"'),
    ("scripts/ci/smoke-linux.sh", '"concat --smoke-export"', '"bobbycat --smoke-export"'),
    ("scripts/ci/smoke-linux.sh", "concat --smoke tried to load", "bobbycat --smoke tried to load"),

    # ---- Linux 打包 ----
    ("assets/linux/nfpm.yaml", "  - src: concat\n    dst: /opt/concat/concat",
     "  - src: bobbycat\n    dst: /opt/concat/bobbycat"),
    ("assets/linux/nfpm.yaml", "  - src: pkg/concat\n    dst: /usr/bin/concat",
     "  - src: pkg/bobbycat\n    dst: /usr/bin/bobbycat"),
    ("assets/linux/nfpm.yaml", "pkg/concat.desktop", "pkg/bobbycat.desktop"),
    ("assets/linux/nfpm.yaml", "concat.desktop\n    dst: /usr/share/applications/concat.desktop",
     "bobbycat.desktop\n    dst: /usr/share/applications/bobbycat.desktop"),
    ("assets/linux/nfpm.yaml", "  - src: concat.png\n    dst: /usr/share/icons/hicolor/256x256/apps/concat.png",
     "  - src: bobbycat.png\n    dst: /usr/share/icons/hicolor/256x256/apps/bobbycat.png"),

    # ---- 图标文件名 ----
    ("src/crates/concat/build.rs", '"../../../assets/icons/concat.ico"',
     '"../../../assets/icons/bobbycat.ico"'),
    ("src/crates/concat/ui/app.slint", '"../../../../assets/icons/concat_logo_256.png"',
     '"../../../../assets/icons/bobbycat_logo_256.png"'),
    ("scripts/generate-icons.sh", "assets/concat_logo_512.png", "assets/bobbycat_logo_512.png"),
    ("scripts/generate-icons.sh", 'concat.ico', 'bobbycat.ico'),
]


def main():
    total = 0
    for rel, old, new in RULES:
        p = os.path.join(ROOT, rel)
        if not os.path.exists(p):
            print(f"MISS {rel}")
            continue
        s = open(p, encoding="utf-8").read()
        n = s.count(old)
        if not n:
            continue
        open(p, "w", encoding="utf-8", newline="").write(s.replace(old, new))
        total += n
        print(f"{rel}: {n}x  {old.splitlines()[0][:60]!r}")
    print("total", total)


if __name__ == "__main__":
    main()
